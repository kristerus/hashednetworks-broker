//! Signaling-message relay.
//!
//! Forwards opaque base64 payloads from sender to recipient.
//! Rate-limited per-sender (100 messages/minute) and size-limited
//! (`SIGNALING_MAX_PAYLOAD` bytes after base64 decode).

use crate::error::{BrokerError, Result};
use crate::protocol::{ServerMessage, SIGNALING_MAX_PAYLOAD};
use crate::registry::{try_send, PeerRegistry};
use chrono::Utc;
use dashmap::DashMap;
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter};
use nonzero_ext::nonzero;
use std::sync::Arc;

/// Per-peer rate limiter: 100 messages / 60 seconds.
pub struct SignalingLimiter {
    map: DashMap<String, Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>>,
}

impl Default for SignalingLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalingLimiter {
    pub fn new() -> Self {
        Self {
            map: DashMap::new(),
        }
    }

    fn limiter_for(&self, pubkey: &str) -> Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>> {
        if let Some(l) = self.map.get(pubkey) {
            return l.clone();
        }
        let quota = Quota::per_minute(nonzero!(100u32));
        let limiter = Arc::new(RateLimiter::direct(quota));
        self.map.insert(pubkey.to_string(), limiter.clone());
        limiter
    }

    pub fn check(&self, pubkey: &str) -> Result<()> {
        let limiter = self.limiter_for(pubkey);
        limiter.check().map_err(|_| BrokerError::RateLimited)
    }
}

/// Relay a signaling message to the recipient. Validates payload size and
/// applies the per-sender rate limit. `payload` is the base64 string sent
/// by the peer; the broker validates its decoded length but does NOT
/// interpret content.
pub async fn relay_signal(
    registry: &Arc<PeerRegistry>,
    limiter: &SignalingLimiter,
    from_pubkey: &str,
    to_pubkey: &str,
    payload: &str,
) -> Result<()> {
    limiter.check(from_pubkey)?;
    let decoded_len = base64_decoded_len(payload)?;
    if decoded_len > SIGNALING_MAX_PAYLOAD {
        return Err(BrokerError::PayloadTooLarge {
            size: decoded_len,
            max: SIGNALING_MAX_PAYLOAD,
        });
    }
    let recipient = registry.get(to_pubkey).ok_or(BrokerError::PeerOffline)?;
    let msg = ServerMessage::Signal {
        from: from_pubkey.to_string(),
        payload: payload.to_string(),
        timestamp: Utc::now().timestamp(),
    };
    try_send(&recipient.tx, msg).await
}

fn base64_decoded_len(s: &str) -> Result<usize> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map(|v| v.len())
        .map_err(|e| BrokerError::Malformed(format!("base64: {e}")))
}
