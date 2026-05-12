# @hashednetworks/broker-client

TypeScript SDK for the **HashedNetworks coordination broker**. Minimal class-based
API, no framework, no global state. Works on Node 20+ (uses `ws`) and in the
browser (pass `websocketFactory: (u) => new WebSocket(u)`).

## Install

```bash
npm install @hashednetworks/broker-client
```

Inside this monorepo (development):

```bash
cd clients/typescript
npm install
npm run build
```

## Quick start

```ts
import { BrokerClient, generateKeypair } from "@hashednetworks/broker-client";

const kp = await generateKeypair();
const client = new BrokerClient({
  url: "wss://broker.hashednetworks.network/ws",
  privateKey: kp.privateKey,
  publicKey: kp.publicKey,
});

await client.connect();
const reg = await client.register({
  handle: "alice",
  addresses: { lan: ["192.168.1.42:51820"], public: null },
});
console.log("peer id:", reg.peer_id);
console.log("public IP as broker saw me:", reg.reflected_address);

// React to inbound signaling.
client.onSignal((m) => {
  console.log(`signal from ${m.from}: ${Buffer.from(m.payload, "base64").toString()}`);
});

// Send a signal to another peer.
await client.sendSignal(
  bobPubkeyHex,
  Buffer.from("hello bob", "utf8").toString("base64"),
);
```

## Two-peer end-to-end

`examples/two-peers.mjs` spins up two clients in one process, exchanges a
signal, opens a relay session, and verifies bidirectional frames. It's the
TypeScript twin of the Rust `smoke-client` binary.

```bash
# Make sure the broker is running locally on :8080 (e.g. via cargo run -p
# hashednetworks-broker --bin broker, or via docker-compose up).
npm run build
BROKER_URL=ws://localhost:8080/ws node ./examples/two-peers.mjs
```

## API

### `BrokerClient`

```ts
new BrokerClient({
  url: string;
  privateKey: Uint8Array;            // 32-byte Ed25519 seed
  publicKey:  Uint8Array;            // 32-byte verifying key
  websocketFactory?: (url) => WebSocketLike;  // browser override
  requestTimeoutMs?: number;         // default 10_000
})
```

| Method | Notes |
|---|---|
| `connect()` | Open the WS and await `onopen`. |
| `close()` | Send a close frame and tear down listeners. |
| `register({ handle?, addresses })` | First call after `connect()`. Returns `registered`. |
| `lookupPeer(pubkey)` / `lookupHandle(handle)` | Returns `peer_info`. |
| `sendSignal(toPubkey, payloadBase64)` | Fire-and-forget. 4 KB cap decoded. |
| `subscribePresence(peers[])` / `unsubscribePresence` | Returns `subscribe_presence_ack`. |
| `requestRelay(peer)` | Initiator. Returns `relay_session_established`. |
| `acceptRelay(sessionId)` | Responder. |
| `sendRelayFrame(sessionId, base64)` | 64 KB cap decoded. Bandwidth-metered. |
| `announceTeam({ teamId?, teamName, adminPubkey, adminSignature })` | Returns `team_announced`. |
| `lookupTeam(teamId)` | Returns `team_members`. |
| `ping()` | Liveness, returns `pong`. |

Events:

| Event | Listener payload |
|---|---|
| `onSignal` | Inbound `signal` from another peer. |
| `onPresenceUpdate` | Online/offline transitions for subscribed peers. |
| `onRelayOffer` | Another peer requested a relay session with you. |
| `onRelaySessionEstablished` | A relay session entered active state. |
| `onRelayData` | An inbound relay frame. |
| `onError` | Server-emitted `error` (rate limit, signature failure, …). |
| `onClose` | The WebSocket closed. |

All `on*` functions return an unregister callback.

## Canonical signing

The broker verifies signatures over **JSON with alphabetically-sorted keys
and the `signature` field set to `""`** — that's what the Rust server gets
when `serde_json` re-serializes a deserialized message in its default
`BTreeMap` ordering. The SDK's `signMessage()` matches byte-for-byte; see
`src/sign.ts`.

## Why no TanStack / framework deps?

This is a transport-layer SDK. State management belongs to the caller.
There's nothing here that benefits from `@tanstack/query` (no idempotent
reads, no caching semantics), so we don't pull it in.

## License

Proprietary. © HashedNetworks.
