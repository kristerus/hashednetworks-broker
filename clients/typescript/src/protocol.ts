/**
 * Wire protocol types for the HashedNetworks broker.
 *
 * Mirrors the Rust `protocol::ClientMessage` / `protocol::ServerMessage`.
 * Field shape is preserved exactly so signatures over canonical JSON match
 * the server.
 */

export interface PeerAddresses {
  lan: string[];
  /** Public address advertised by the peer (broker fills in the reflected one). */
  public: string | null;
}

// ---------------------------------------------------------------------------
// Client → Server
// ---------------------------------------------------------------------------

interface SignedBase {
  pubkey: string;
  timestamp: number;
  signature: string;
  request_id?: string;
}

export interface RegisterMessage extends SignedBase {
  type: "register";
  handle?: string;
  addresses: PeerAddresses;
}

export interface LookupPeerMessage extends SignedBase {
  type: "lookup_peer";
  target_pubkey: string;
}

export interface LookupHandleMessage extends SignedBase {
  type: "lookup_handle";
  target_handle: string;
}

export interface SignalMessage extends SignedBase {
  type: "signal";
  to: string;
  /** Base64. Broker does not interpret. */
  payload: string;
}

export interface SubscribePresenceMessage extends SignedBase {
  type: "subscribe_presence";
  peers: string[];
}

export interface UnsubscribePresenceMessage extends SignedBase {
  type: "unsubscribe_presence";
  peers: string[];
}

export interface RequestRelayMessage extends SignedBase {
  type: "request_relay";
  peer: string;
}

export interface AcceptRelayMessage extends SignedBase {
  type: "accept_relay";
  session_id: string;
}

export interface RelayMessage extends SignedBase {
  type: "relay";
  session_id: string;
  /** Base64. Counts against the sender's bandwidth quota. */
  data: string;
}

export interface TeamAnnounceMessage extends SignedBase {
  type: "team_announce";
  team_id?: string;
  team_name: string;
  admin_pubkey: string;
  admin_signature: string;
}

export interface TeamLookupMessage extends SignedBase {
  type: "team_lookup";
  team_id: string;
}

export interface PingMessage extends SignedBase {
  type: "ping";
}

export type ClientMessage =
  | RegisterMessage
  | LookupPeerMessage
  | LookupHandleMessage
  | SignalMessage
  | SubscribePresenceMessage
  | UnsubscribePresenceMessage
  | RequestRelayMessage
  | AcceptRelayMessage
  | RelayMessage
  | TeamAnnounceMessage
  | TeamLookupMessage
  | PingMessage;

// ---------------------------------------------------------------------------
// Server → Client
// ---------------------------------------------------------------------------

export interface RegisteredMessage {
  type: "registered";
  peer_id: string;
  reflected_address: string;
  handle_claimed: boolean;
  request_id?: string | null;
}

export interface PeerInfoMessage {
  type: "peer_info";
  peer_id: string;
  handle: string | null;
  addresses: PeerAddresses;
  reflected_address: string | null;
  online: boolean;
  request_id?: string | null;
}

export interface IncomingSignalMessage {
  type: "signal";
  from: string;
  payload: string;
  timestamp: number;
}

export interface PresenceUpdateMessage {
  type: "presence_update";
  peer_id: string;
  online: boolean;
}

export interface SubscribePresenceAckMessage {
  type: "subscribe_presence_ack";
  peers: string[];
  request_id?: string | null;
}

export interface RelayOfferMessage {
  type: "relay_offer";
  session_id: string;
  from: string;
}

export interface RelaySessionEstablishedMessage {
  type: "relay_session_established";
  session_id: string;
  peer: string;
  request_id?: string | null;
}

export interface RelayDataMessage {
  type: "relay_data";
  session_id: string;
  from: string;
  data: string;
}

export interface TeamAnnouncedMessage {
  type: "team_announced";
  team_id: string;
  request_id?: string | null;
}

export interface TeamMember {
  pubkey: string;
  online: boolean;
  joined_at: string;
}

export interface TeamMembersMessage {
  type: "team_members";
  team_id: string;
  members: TeamMember[];
  request_id?: string | null;
}

export interface PongMessage {
  type: "pong";
  timestamp: number;
}

export interface ErrorMessage {
  type: "error";
  code: string;
  message: string;
  request_id?: string | null;
}

export type ServerMessage =
  | RegisteredMessage
  | PeerInfoMessage
  | IncomingSignalMessage
  | PresenceUpdateMessage
  | SubscribePresenceAckMessage
  | RelayOfferMessage
  | RelaySessionEstablishedMessage
  | RelayDataMessage
  | TeamAnnouncedMessage
  | TeamMembersMessage
  | PongMessage
  | ErrorMessage;

export type ServerMessageType = ServerMessage["type"];

/** 4 KB cap on signaling payloads after base64 decode. */
export const SIGNALING_MAX_PAYLOAD = 4 * 1024;
/** 64 KB cap on a single relay frame after base64 decode. */
export const RELAY_MAX_FRAME = 64 * 1024;
