//! Wire protocol for the broker. See PROTOCOL.md for the full spec.
//!
//! All messages are JSON over WebSocket. Each [`ClientMessage`] carries a
//! `pubkey`, `timestamp` (unix seconds), and `signature` over the canonical
//! representation (the message with the signature field set to empty). See
//! [`crate::auth::verify_signed_message`].

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PeerAddresses {
    #[serde(default)]
    pub lan: Vec<String>,
    /// Public address as advertised by the peer (may be None — the broker
    /// fills in `reflected_address` on the `Registered` response).
    #[serde(default)]
    pub public: Option<String>,
}

/// Identity scope a peer declares on Register. Pre-v0.7 clients omit this
/// field, in which case the broker treats them as `User`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IdentityKind {
    User,
    Machine,
}

impl IdentityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Machine => "machine",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "user" => Some(Self::User),
            "machine" => Some(Self::Machine),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Register {
        pubkey: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        handle: Option<String>,
        addresses: PeerAddresses,
        /// Identity scope declared by the peer. Defaults to `User` when
        /// missing so pre-v0.7 clients still verify.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<IdentityKind>,
        timestamp: i64,
        #[serde(default)]
        signature: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    LookupPeer {
        pubkey: String,
        target_pubkey: String,
        timestamp: i64,
        #[serde(default)]
        signature: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    LookupHandle {
        pubkey: String,
        target_handle: String,
        timestamp: i64,
        #[serde(default)]
        signature: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    Signal {
        pubkey: String,
        to: String,
        payload: String,
        timestamp: i64,
        #[serde(default)]
        signature: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    SubscribePresence {
        pubkey: String,
        peers: Vec<String>,
        timestamp: i64,
        #[serde(default)]
        signature: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    UnsubscribePresence {
        pubkey: String,
        peers: Vec<String>,
        timestamp: i64,
        #[serde(default)]
        signature: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    RequestRelay {
        pubkey: String,
        peer: String,
        timestamp: i64,
        #[serde(default)]
        signature: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    AcceptRelay {
        pubkey: String,
        session_id: String,
        timestamp: i64,
        #[serde(default)]
        signature: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    Relay {
        pubkey: String,
        session_id: String,
        /// Base64-encoded opaque payload. Broker does not interpret.
        data: String,
        timestamp: i64,
        #[serde(default)]
        signature: String,
    },
    TeamAnnounce {
        pubkey: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        team_id: Option<String>,
        team_name: String,
        admin_pubkey: String,
        admin_signature: String,
        timestamp: i64,
        #[serde(default)]
        signature: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    TeamLookup {
        pubkey: String,
        team_id: String,
        timestamp: i64,
        #[serde(default)]
        signature: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    Ping {
        pubkey: String,
        timestamp: i64,
        #[serde(default)]
        signature: String,
    },
}

impl ClientMessage {
    pub fn pubkey(&self) -> &str {
        match self {
            Self::Register { pubkey, .. }
            | Self::LookupPeer { pubkey, .. }
            | Self::LookupHandle { pubkey, .. }
            | Self::Signal { pubkey, .. }
            | Self::SubscribePresence { pubkey, .. }
            | Self::UnsubscribePresence { pubkey, .. }
            | Self::RequestRelay { pubkey, .. }
            | Self::AcceptRelay { pubkey, .. }
            | Self::Relay { pubkey, .. }
            | Self::TeamAnnounce { pubkey, .. }
            | Self::TeamLookup { pubkey, .. }
            | Self::Ping { pubkey, .. } => pubkey,
        }
    }

    pub fn timestamp(&self) -> i64 {
        match self {
            Self::Register { timestamp, .. }
            | Self::LookupPeer { timestamp, .. }
            | Self::LookupHandle { timestamp, .. }
            | Self::Signal { timestamp, .. }
            | Self::SubscribePresence { timestamp, .. }
            | Self::UnsubscribePresence { timestamp, .. }
            | Self::RequestRelay { timestamp, .. }
            | Self::AcceptRelay { timestamp, .. }
            | Self::Relay { timestamp, .. }
            | Self::TeamAnnounce { timestamp, .. }
            | Self::TeamLookup { timestamp, .. }
            | Self::Ping { timestamp, .. } => *timestamp,
        }
    }

    pub fn signature(&self) -> &str {
        match self {
            Self::Register { signature, .. }
            | Self::LookupPeer { signature, .. }
            | Self::LookupHandle { signature, .. }
            | Self::Signal { signature, .. }
            | Self::SubscribePresence { signature, .. }
            | Self::UnsubscribePresence { signature, .. }
            | Self::RequestRelay { signature, .. }
            | Self::AcceptRelay { signature, .. }
            | Self::Relay { signature, .. }
            | Self::TeamAnnounce { signature, .. }
            | Self::TeamLookup { signature, .. }
            | Self::Ping { signature, .. } => signature,
        }
    }

    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::Register { request_id, .. }
            | Self::LookupPeer { request_id, .. }
            | Self::LookupHandle { request_id, .. }
            | Self::Signal { request_id, .. }
            | Self::SubscribePresence { request_id, .. }
            | Self::UnsubscribePresence { request_id, .. }
            | Self::RequestRelay { request_id, .. }
            | Self::AcceptRelay { request_id, .. }
            | Self::TeamAnnounce { request_id, .. }
            | Self::TeamLookup { request_id, .. } => request_id.as_deref(),
            Self::Relay { .. } | Self::Ping { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Registered {
        peer_id: String,
        reflected_address: String,
        handle_claimed: bool,
        request_id: Option<String>,
    },
    PeerInfo {
        peer_id: String,
        handle: Option<String>,
        addresses: PeerAddresses,
        reflected_address: Option<String>,
        online: bool,
        /// Mirrors the identity scope the peer registered with. Absent when
        /// the peer pre-dates v0.7.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<IdentityKind>,
        request_id: Option<String>,
    },
    Signal {
        from: String,
        payload: String,
        timestamp: i64,
    },
    PresenceUpdate {
        peer_id: String,
        online: bool,
    },
    SubscribePresenceAck {
        peers: Vec<String>,
        request_id: Option<String>,
    },
    RelayOffer {
        session_id: String,
        from: String,
    },
    RelaySessionEstablished {
        session_id: String,
        peer: String,
        request_id: Option<String>,
    },
    RelayData {
        session_id: String,
        from: String,
        data: String,
    },
    TeamAnnounced {
        team_id: String,
        request_id: Option<String>,
    },
    TeamMembers {
        team_id: String,
        members: Vec<TeamMember>,
        request_id: Option<String>,
    },
    Pong {
        timestamp: i64,
    },
    Error {
        code: String,
        message: String,
        request_id: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    pub pubkey: String,
    pub online: bool,
    pub joined_at: chrono::DateTime<chrono::Utc>,
}

/// Canonical bytes for signing: serialize the message to JSON with the
/// `signature` field forced to an empty string. This is what the peer signs
/// and what the broker verifies.
pub fn canonical_bytes(msg: &ClientMessage) -> Vec<u8> {
    let mut v = serde_json::to_value(msg).expect("ClientMessage serializes");
    if let Some(obj) = v.as_object_mut() {
        obj.insert("signature".into(), serde_json::Value::String(String::new()));
    }
    serde_json::to_vec(&v).expect("Value serializes")
}

/// Bound on signaling payload size (4 KB per spec).
pub const SIGNALING_MAX_PAYLOAD: usize = 4 * 1024;

/// Bound on a single relay frame payload (64 KB).
pub const RELAY_MAX_FRAME: usize = 64 * 1024;
