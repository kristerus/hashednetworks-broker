/**
 * BrokerClient — minimal class-based SDK for the HashedNetworks broker.
 *
 * Lifecycle:
 *   1. `new BrokerClient({ url, privateKey })`
 *   2. `await client.connect()`
 *   3. `await client.register({ handle?, addresses })`
 *   4. Call message methods + register event handlers.
 *   5. `await client.close()`
 *
 * Each request method that has a server response returns a Promise that
 * resolves with the matching `ServerMessage`. Correlation is by
 * `request_id` (auto-generated if not supplied).
 *
 * No React, no TanStack, no state-manager. Plain promises + event emitters.
 */

import { WebSocket as NodeWebSocket } from "ws";
import {
  type ClientMessage,
  type ErrorMessage,
  type IncomingSignalMessage,
  type LookupHandleMessage,
  type LookupPeerMessage,
  type PeerAddresses,
  type PeerInfoMessage,
  type PingMessage,
  type PongMessage,
  type PresenceUpdateMessage,
  type RegisterMessage,
  type RegisteredMessage,
  type RelayDataMessage,
  type RelayMessage,
  type RelayOfferMessage,
  type RelaySessionEstablishedMessage,
  type RequestRelayMessage,
  type ServerMessage,
  type SignalMessage,
  type SubscribePresenceAckMessage,
  type SubscribePresenceMessage,
  type TeamAnnounceMessage,
  type TeamAnnouncedMessage,
  type TeamLookupMessage,
  type TeamMembersMessage,
  type UnsubscribePresenceMessage,
} from "./protocol.js";
import { bytesToHex, signMessage } from "./sign.js";

export interface BrokerClientOptions {
  url: string;
  /** 32-byte Ed25519 seed. Use `generateKeypair()` to make one. */
  privateKey: Uint8Array;
  /** 32-byte Ed25519 public key (derived once at connect). */
  publicKey: Uint8Array;
  /** Override the connect-time WebSocket factory (for tests / browser). */
  websocketFactory?: (url: string) => WebSocketLike;
  /**
   * How long pending requests wait for a matching response. Defaults to
   * 10 s — well over the broker's 60 s replay window.
   */
  requestTimeoutMs?: number;
}

/** Minimal subset of WebSocket we rely on (works for `ws` and browser). */
export interface WebSocketLike {
  send(data: string): void;
  close(code?: number, reason?: string): void;
  readonly readyState: number;
  set onopen(cb: (() => void) | null);
  set onclose(cb: (() => void) | null);
  set onerror(cb: ((err: unknown) => void) | null);
  set onmessage(cb: ((event: { data: unknown }) => void) | null);
}

type Listener<T> = (msg: T) => void;

interface PendingRequest {
  resolve: (msg: ServerMessage) => void;
  reject: (e: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

const STATE_OPEN = 1;

export class BrokerClient {
  private ws: WebSocketLike | null = null;
  private readonly opts: BrokerClientOptions;
  private pubkeyHex: string;
  private pending = new Map<string, PendingRequest>();
  private signalListeners = new Set<Listener<IncomingSignalMessage>>();
  private presenceListeners = new Set<Listener<PresenceUpdateMessage>>();
  private relayOfferListeners = new Set<Listener<RelayOfferMessage>>();
  private relayDataListeners = new Set<Listener<RelayDataMessage>>();
  private relaySessionListeners = new Set<Listener<RelaySessionEstablishedMessage>>();
  private errorListeners = new Set<Listener<ErrorMessage>>();
  private closeListeners = new Set<() => void>();
  private nextRequestId = 0;
  private connectedResolver: ((value: void) => void) | null = null;
  private connectedRejector: ((reason: Error) => void) | null = null;

  constructor(opts: BrokerClientOptions) {
    this.opts = opts;
    this.pubkeyHex = bytesToHex(opts.publicKey);
  }

  /** Hex-encoded Ed25519 public key — also used as the broker peer id. */
  get publicKeyHex(): string {
    return this.pubkeyHex;
  }

  /** Open the WebSocket and resolve when the upgrade completes. */
  async connect(): Promise<void> {
    if (this.ws && this.ws.readyState === STATE_OPEN) return;
    const factory = this.opts.websocketFactory ?? defaultFactory;
    const ws = factory(this.opts.url);
    this.ws = ws;
    return new Promise<void>((resolve, reject) => {
      this.connectedResolver = resolve;
      this.connectedRejector = reject;
      ws.onopen = () => {
        this.connectedResolver?.();
        this.connectedResolver = null;
        this.connectedRejector = null;
      };
      ws.onerror = (err) => {
        this.connectedRejector?.(toError(err));
        this.connectedRejector = null;
      };
      ws.onclose = () => {
        for (const r of this.pending.values()) {
          clearTimeout(r.timer);
          r.reject(new Error("websocket closed"));
        }
        this.pending.clear();
        for (const l of this.closeListeners) l();
      };
      ws.onmessage = (ev) => this.handleIncoming(ev.data);
    });
  }

  /** Close the connection and clear all listeners. */
  async close(code = 1000, reason = "client close"): Promise<void> {
    this.ws?.close(code, reason);
    this.ws = null;
  }

  // ---- Request methods --------------------------------------------------

  async register(args: {
    handle?: string;
    addresses: PeerAddresses;
  }): Promise<RegisteredMessage> {
    const msg: Omit<RegisterMessage, "signature"> = {
      type: "register",
      pubkey: this.pubkeyHex,
      addresses: args.addresses,
      timestamp: nowSecs(),
      request_id: this.mintRequestId(),
    };
    if (args.handle !== undefined) (msg as RegisterMessage).handle = args.handle;
    return this.sendAndAwait(msg, "registered");
  }

  async lookupPeer(targetPubkey: string): Promise<PeerInfoMessage> {
    const msg: Omit<LookupPeerMessage, "signature"> = {
      type: "lookup_peer",
      pubkey: this.pubkeyHex,
      target_pubkey: targetPubkey,
      timestamp: nowSecs(),
      request_id: this.mintRequestId(),
    };
    return this.sendAndAwait(msg, "peer_info");
  }

  async lookupHandle(targetHandle: string): Promise<PeerInfoMessage> {
    const msg: Omit<LookupHandleMessage, "signature"> = {
      type: "lookup_handle",
      pubkey: this.pubkeyHex,
      target_handle: targetHandle,
      timestamp: nowSecs(),
      request_id: this.mintRequestId(),
    };
    return this.sendAndAwait(msg, "peer_info");
  }

  /** Send a signal. Server forwards as `IncomingSignalMessage` to `to`. */
  async sendSignal(to: string, payloadBase64: string): Promise<void> {
    const msg: Omit<SignalMessage, "signature"> = {
      type: "signal",
      pubkey: this.pubkeyHex,
      to,
      payload: payloadBase64,
      timestamp: nowSecs(),
    };
    await this.signAndSend(msg);
  }

  async subscribePresence(peers: string[]): Promise<SubscribePresenceAckMessage> {
    const msg: Omit<SubscribePresenceMessage, "signature"> = {
      type: "subscribe_presence",
      pubkey: this.pubkeyHex,
      peers,
      timestamp: nowSecs(),
      request_id: this.mintRequestId(),
    };
    return this.sendAndAwait(msg, "subscribe_presence_ack");
  }

  async unsubscribePresence(peers: string[]): Promise<void> {
    const msg: Omit<UnsubscribePresenceMessage, "signature"> = {
      type: "unsubscribe_presence",
      pubkey: this.pubkeyHex,
      peers,
      timestamp: nowSecs(),
    };
    await this.signAndSend(msg);
  }

  /** Initiator side. Returns the `relay_session_established` ack. */
  async requestRelay(peer: string): Promise<RelaySessionEstablishedMessage> {
    const msg: Omit<RequestRelayMessage, "signature"> = {
      type: "request_relay",
      pubkey: this.pubkeyHex,
      peer,
      timestamp: nowSecs(),
      request_id: this.mintRequestId(),
    };
    return this.sendAndAwait(msg, "relay_session_established");
  }

  /** Responder side. After this fires, both peers receive `RelaySessionEstablished`. */
  async acceptRelay(sessionId: string): Promise<void> {
    const msg = {
      type: "accept_relay" as const,
      pubkey: this.pubkeyHex,
      session_id: sessionId,
      timestamp: nowSecs(),
    };
    await this.signAndSend(msg);
  }

  /** Send a relay frame within an active session. */
  async sendRelayFrame(sessionId: string, dataBase64: string): Promise<void> {
    const msg: Omit<RelayMessage, "signature"> = {
      type: "relay",
      pubkey: this.pubkeyHex,
      session_id: sessionId,
      data: dataBase64,
      timestamp: nowSecs(),
    };
    await this.signAndSend(msg);
  }

  async announceTeam(args: {
    teamId?: string;
    teamName: string;
    adminPubkey: string;
    adminSignature: string;
  }): Promise<TeamAnnouncedMessage> {
    const msg: Omit<TeamAnnounceMessage, "signature"> = {
      type: "team_announce",
      pubkey: this.pubkeyHex,
      team_name: args.teamName,
      admin_pubkey: args.adminPubkey,
      admin_signature: args.adminSignature,
      timestamp: nowSecs(),
      request_id: this.mintRequestId(),
    };
    if (args.teamId !== undefined) (msg as TeamAnnounceMessage).team_id = args.teamId;
    return this.sendAndAwait(msg, "team_announced");
  }

  async lookupTeam(teamId: string): Promise<TeamMembersMessage> {
    const msg: Omit<TeamLookupMessage, "signature"> = {
      type: "team_lookup",
      pubkey: this.pubkeyHex,
      team_id: teamId,
      timestamp: nowSecs(),
      request_id: this.mintRequestId(),
    };
    return this.sendAndAwait(msg, "team_members");
  }

  async ping(): Promise<PongMessage> {
    const msg: Omit<PingMessage, "signature"> = {
      type: "ping",
      pubkey: this.pubkeyHex,
      timestamp: nowSecs(),
    };
    return new Promise<PongMessage>((resolve, reject) => {
      // No request_id correlation — pong carries only timestamp. We resolve
      // on the next pong we see.
      const handler = (m: ServerMessage) => {
        if (m.type === "pong") {
          this.pongListeners.delete(handler);
          resolve(m);
        }
      };
      this.pongListeners.add(handler);
      this.signAndSend(msg).catch((err) => {
        this.pongListeners.delete(handler);
        reject(toError(err));
      });
      setTimeout(() => {
        if (this.pongListeners.delete(handler)) {
          reject(new Error("ping timed out"));
        }
      }, this.opts.requestTimeoutMs ?? 10_000);
    });
  }

  // ---- Event registration ----------------------------------------------

  onSignal(cb: Listener<IncomingSignalMessage>): () => void {
    this.signalListeners.add(cb);
    return () => this.signalListeners.delete(cb);
  }
  onPresenceUpdate(cb: Listener<PresenceUpdateMessage>): () => void {
    this.presenceListeners.add(cb);
    return () => this.presenceListeners.delete(cb);
  }
  onRelayOffer(cb: Listener<RelayOfferMessage>): () => void {
    this.relayOfferListeners.add(cb);
    return () => this.relayOfferListeners.delete(cb);
  }
  onRelayData(cb: Listener<RelayDataMessage>): () => void {
    this.relayDataListeners.add(cb);
    return () => this.relayDataListeners.delete(cb);
  }
  onRelaySessionEstablished(cb: Listener<RelaySessionEstablishedMessage>): () => void {
    this.relaySessionListeners.add(cb);
    return () => this.relaySessionListeners.delete(cb);
  }
  onError(cb: Listener<ErrorMessage>): () => void {
    this.errorListeners.add(cb);
    return () => this.errorListeners.delete(cb);
  }
  onClose(cb: () => void): () => void {
    this.closeListeners.add(cb);
    return () => this.closeListeners.delete(cb);
  }

  // ---- Internal --------------------------------------------------------

  private pongListeners = new Set<(m: ServerMessage) => void>();

  private mintRequestId(): string {
    this.nextRequestId += 1;
    return `req-${Date.now().toString(36)}-${this.nextRequestId}`;
  }

  private async signAndSend<T extends Record<string, unknown>>(msg: T): Promise<void> {
    if (!this.ws || this.ws.readyState !== STATE_OPEN) {
      throw new Error("websocket not open");
    }
    const signed = await signMessage(this.opts.privateKey, msg);
    this.ws.send(JSON.stringify(signed));
  }

  private async sendAndAwait<T extends Record<string, unknown>, R extends ServerMessage>(
    msg: T,
    expectedType: R["type"]
  ): Promise<R> {
    if (!this.ws || this.ws.readyState !== STATE_OPEN) {
      throw new Error("websocket not open");
    }
    const requestId = (msg as Record<string, unknown>).request_id as string | undefined;
    if (requestId === undefined) {
      throw new Error("sendAndAwait requires request_id on the message");
    }
    const signed = await signMessage(this.opts.privateKey, msg);
    return new Promise<R>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(requestId);
        reject(new Error(`request ${requestId} (${expectedType}) timed out`));
      }, this.opts.requestTimeoutMs ?? 10_000);
      this.pending.set(requestId, {
        resolve: (m) => {
          if (m.type === expectedType) resolve(m as R);
          else if (m.type === "error") reject(new BrokerError(m as ErrorMessage));
          else reject(new Error(`unexpected response type ${m.type}, wanted ${expectedType}`));
        },
        reject,
        timer,
      });
      this.ws!.send(JSON.stringify(signed));
    });
  }

  private handleIncoming(raw: unknown) {
    const text = typeof raw === "string" ? raw : raw instanceof Buffer ? raw.toString("utf8") : "";
    if (!text) return;
    let msg: ServerMessage;
    try {
      msg = JSON.parse(text) as ServerMessage;
    } catch (e) {
      console.error("[broker-client] malformed JSON from server:", e);
      return;
    }

    // Route to correlated pending if applicable.
    const reqId = (msg as { request_id?: string | null }).request_id;
    if (reqId) {
      const pending = this.pending.get(reqId);
      if (pending) {
        this.pending.delete(reqId);
        clearTimeout(pending.timer);
        pending.resolve(msg);
        return;
      }
    }

    // Dispatch to handlers by type.
    switch (msg.type) {
      case "signal":
        for (const l of this.signalListeners) l(msg);
        break;
      case "presence_update":
        for (const l of this.presenceListeners) l(msg);
        break;
      case "relay_offer":
        for (const l of this.relayOfferListeners) l(msg);
        break;
      case "relay_data":
        for (const l of this.relayDataListeners) l(msg);
        break;
      case "relay_session_established":
        for (const l of this.relaySessionListeners) l(msg);
        break;
      case "pong":
        for (const l of this.pongListeners) l(msg);
        break;
      case "error":
        for (const l of this.errorListeners) l(msg);
        break;
      default:
        // Other types (registered, peer_info, …) without a matching request_id
        // are silently dropped — they were correlated above when expected.
        break;
    }
  }
}

export class BrokerError extends Error {
  readonly code: string;
  readonly request_id: string | null;
  constructor(err: ErrorMessage) {
    super(`[${err.code}] ${err.message}`);
    this.code = err.code;
    this.request_id = err.request_id ?? null;
  }
}

function nowSecs(): number {
  return Math.floor(Date.now() / 1000);
}

function toError(e: unknown): Error {
  return e instanceof Error ? e : new Error(String(e));
}

function defaultFactory(url: string): WebSocketLike {
  // Use `ws` on Node (works for tests + servers). Browsers should pass
  // `websocketFactory: (u) => new WebSocket(u)` themselves to use the
  // platform WebSocket.
  const ws = new NodeWebSocket(url);
  return ws as unknown as WebSocketLike;
}
