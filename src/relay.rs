//! TURN-style fallback relay.
//!
//! Two-party only. Workflow:
//!   1. Peer A sends `request_relay { peer: B }`. Broker creates a pending
//!      session, sends `relay_offer { session_id, from: A }` to B.
//!   2. Peer B sends `accept_relay { session_id }`. Broker promotes the
//!      session to `Active` and replies `relay_session_established`
//!      to both peers.
//!   3. Either side sends `relay { session_id, data }`. Broker forwards
//!      to the *other* party as `relay_data`. The payload is opaque
//!      base64 — broker never decrypts or inspects.
//!
//! Quotas (free tier):
//!   * 1 MB/s peak per peer (token bucket).
//!   * 1 GB/day per peer (rolling 24 h, in-memory).
//!
//! Restart loses state. v0.5 explicit non-goal: per-peer durable quota.

use crate::error::{BrokerError, Result};
use crate::protocol::{RELAY_MAX_FRAME, ServerMessage};
use crate::registry::{PeerRegistry, try_send};
use base64::Engine;
use dashmap::DashMap;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq)]
enum SessionState {
    Pending,
    Active,
}

#[derive(Clone)]
struct Session {
    initiator: String,
    responder: String,
    state: SessionState,
}

/// Token-bucket bandwidth limiter, plus rolling daily quota.
struct PeerQuota {
    tokens: f64,           // current bytes available
    last_refill: Instant,  // when tokens were last topped up
    day_used: u64,         // bytes used in the current 24h window
    day_window_start: Instant,
}

impl PeerQuota {
    fn new() -> Self {
        Self {
            tokens: BYTES_PER_SECOND as f64,
            last_refill: Instant::now(),
            day_used: 0,
            day_window_start: Instant::now(),
        }
    }

    fn try_consume(&mut self, bytes: u64) -> Result<()> {
        let now = Instant::now();

        // Refill token bucket at BYTES_PER_SECOND.
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * BYTES_PER_SECOND as f64)
            .min(BYTES_PER_SECOND as f64);
        self.last_refill = now;

        // Reset daily window if it's been 24h.
        if now.duration_since(self.day_window_start) >= Duration::from_secs(86_400) {
            self.day_used = 0;
            self.day_window_start = now;
        }

        if self.day_used + bytes > BYTES_PER_DAY {
            return Err(BrokerError::RelayQuotaExceeded);
        }
        if (bytes as f64) > self.tokens {
            return Err(BrokerError::RateLimited);
        }
        self.tokens -= bytes as f64;
        self.day_used += bytes;
        Ok(())
    }
}

pub struct RelayManager {
    sessions: DashMap<Uuid, Session>,
    quotas: DashMap<String, Arc<Mutex<PeerQuota>>>,
}

const BYTES_PER_SECOND: u64 = 1024 * 1024;          // 1 MB/s
const BYTES_PER_DAY: u64 = 1024 * 1024 * 1024;      // 1 GB/day

impl Default for RelayManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RelayManager {
    pub fn new() -> Self {
        Self {
            sessions: DashMap::new(),
            quotas: DashMap::new(),
        }
    }

    /// Initiator A asks broker to set up a relay session to B.
    /// Returns the new session id; emits `relay_offer` to B if online.
    pub async fn request(
        &self,
        registry: &Arc<PeerRegistry>,
        initiator: &str,
        responder: &str,
    ) -> Result<Uuid> {
        let recipient = registry.get(responder).ok_or(BrokerError::PeerOffline)?;
        let id = Uuid::new_v4();
        self.sessions.insert(
            id,
            Session {
                initiator: initiator.to_string(),
                responder: responder.to_string(),
                state: SessionState::Pending,
            },
        );
        try_send(
            &recipient.tx,
            ServerMessage::RelayOffer {
                session_id: id.to_string(),
                from: initiator.to_string(),
            },
        )
        .await?;
        Ok(id)
    }

    /// Responder accepts a pending session, promoting it to Active.
    /// Both peers get `relay_session_established`.
    pub async fn accept(
        &self,
        registry: &Arc<PeerRegistry>,
        responder: &str,
        session_id: Uuid,
    ) -> Result<()> {
        let mut entry = self
            .sessions
            .get_mut(&session_id)
            .ok_or(BrokerError::RelaySessionNotFound)?;
        if entry.responder != responder {
            return Err(BrokerError::RelayNotConsented);
        }
        if entry.state != SessionState::Pending {
            return Err(BrokerError::RelayNotConsented);
        }
        entry.state = SessionState::Active;
        let initiator = entry.initiator.clone();
        drop(entry);

        let init_peer = registry.get(&initiator).ok_or(BrokerError::PeerOffline)?;
        let resp_peer = registry.get(responder).ok_or(BrokerError::PeerOffline)?;
        try_send(
            &init_peer.tx,
            ServerMessage::RelaySessionEstablished {
                session_id: session_id.to_string(),
                peer: responder.to_string(),
                request_id: None,
            },
        )
        .await?;
        try_send(
            &resp_peer.tx,
            ServerMessage::RelaySessionEstablished {
                session_id: session_id.to_string(),
                peer: initiator,
                request_id: None,
            },
        )
        .await?;
        Ok(())
    }

    /// Forward a base64-encoded data frame within an active session.
    /// Returns the number of decoded bytes that crossed the relay.
    pub async fn forward(
        &self,
        registry: &Arc<PeerRegistry>,
        sender_pubkey: &str,
        session_id: Uuid,
        data_b64: &str,
    ) -> Result<u64> {
        let entry = self
            .sessions
            .get(&session_id)
            .ok_or(BrokerError::RelaySessionNotFound)?;
        if entry.state != SessionState::Active {
            return Err(BrokerError::RelayNotConsented);
        }
        let other = if entry.initiator == sender_pubkey {
            entry.responder.clone()
        } else if entry.responder == sender_pubkey {
            entry.initiator.clone()
        } else {
            return Err(BrokerError::RelayNotConsented);
        };
        drop(entry);

        let decoded_len = base64::engine::general_purpose::STANDARD
            .decode(data_b64)
            .map(|v| v.len())
            .map_err(|e| BrokerError::Malformed(format!("relay base64: {e}")))?;
        if decoded_len > RELAY_MAX_FRAME {
            return Err(BrokerError::PayloadTooLarge {
                size: decoded_len,
                max: RELAY_MAX_FRAME,
            });
        }

        self.consume_quota(sender_pubkey, decoded_len as u64)?;
        let recipient = registry.get(&other).ok_or(BrokerError::PeerOffline)?;
        try_send(
            &recipient.tx,
            ServerMessage::RelayData {
                session_id: session_id.to_string(),
                from: sender_pubkey.to_string(),
                data: data_b64.to_string(),
            },
        )
        .await?;
        Ok(decoded_len as u64)
    }

    fn consume_quota(&self, pubkey: &str, bytes: u64) -> Result<()> {
        let arc: Arc<Mutex<PeerQuota>> = {
            let entry = self
                .quotas
                .entry(pubkey.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(PeerQuota::new())));
            entry.value().clone()
        };
        let outcome = arc.lock().try_consume(bytes);
        outcome
    }

    /// Remove sessions whose initiator or responder matches `pubkey` —
    /// used when a peer disconnects.
    pub fn drop_sessions_for(&self, pubkey: &str) {
        let stale: Vec<Uuid> = self
            .sessions
            .iter()
            .filter_map(|s| {
                if s.initiator == pubkey || s.responder == pubkey {
                    Some(*s.key())
                } else {
                    None
                }
            })
            .collect();
        for id in stale {
            self.sessions.remove(&id);
        }
    }
}

pub fn parse_session_id(s: &str) -> Result<Uuid> {
    Uuid::parse_str(s).map_err(|e| BrokerError::Malformed(format!("session_id: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn quota_token_bucket_refills() {
        let mut q = PeerQuota::new();
        q.try_consume(1024 * 1024).unwrap(); // burn the full bucket
        // Immediate retry should fail (no tokens).
        assert!(q.try_consume(1024).is_err());
        // After 100ms we should have ~100KB tokens — try a 50KB consume.
        sleep(Duration::from_millis(150));
        assert!(q.try_consume(50_000).is_ok());
    }

    #[test]
    fn quota_daily_limit() {
        let mut q = PeerQuota::new();
        q.day_used = BYTES_PER_DAY - 100;
        // 100 bytes is within the bucket and within the daily quota.
        q.try_consume(100).unwrap();
        // Next byte should be denied.
        assert!(matches!(
            q.try_consume(1).unwrap_err(),
            BrokerError::RelayQuotaExceeded
        ));
    }
}
