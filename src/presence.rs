//! Presence pub/sub.
//!
//! Each peer can subscribe to N target pubkeys; when any of them connect
//! or disconnect we push a [`ServerMessage::PresenceUpdate`] to the
//! subscriber. Subscriptions are tracked in two indexes:
//!   subscriber -> {watched pubkeys}    (per-connection cleanup)
//!   watched_pubkey -> {subscribers}    (broadcast on state change)

use crate::protocol::ServerMessage;
use crate::registry::{PeerRegistry, PeerSender, try_send};
use dashmap::DashMap;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Default)]
pub struct PresenceManager {
    /// subscriber pubkey -> set of pubkeys they watch
    subscriptions: DashMap<String, HashSet<String>>,
    /// watched pubkey -> set of subscribers
    watchers: DashMap<String, HashSet<String>>,
}

impl PresenceManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn subscribe(&self, subscriber: &str, targets: &[String]) {
        let mut subs = self
            .subscriptions
            .entry(subscriber.to_string())
            .or_default();
        for t in targets {
            subs.insert(t.clone());
            self.watchers
                .entry(t.clone())
                .or_default()
                .insert(subscriber.to_string());
        }
    }

    pub fn unsubscribe(&self, subscriber: &str, targets: &[String]) {
        if let Some(mut subs) = self.subscriptions.get_mut(subscriber) {
            for t in targets {
                subs.remove(t);
                if let Some(mut w) = self.watchers.get_mut(t) {
                    w.remove(subscriber);
                    if w.is_empty() {
                        drop(w);
                        self.watchers.remove(t);
                    }
                }
            }
        }
    }

    /// Drop everything for a disconnecting peer.
    pub fn clear_subscriber(&self, subscriber: &str) {
        let Some((_, targets)) = self.subscriptions.remove(subscriber) else {
            return;
        };
        for t in &targets {
            if let Some(mut w) = self.watchers.get_mut(t) {
                w.remove(subscriber);
                if w.is_empty() {
                    drop(w);
                    self.watchers.remove(t);
                }
            }
        }
    }

    /// Broadcast a presence change for `pubkey` to everyone watching it.
    /// The registry is consulted only to obtain the watchers' senders.
    pub async fn broadcast(
        &self,
        registry: &Arc<PeerRegistry>,
        pubkey: &str,
        online: bool,
    ) {
        let watchers: Vec<String> = match self.watchers.get(pubkey) {
            Some(w) => w.iter().cloned().collect(),
            None => return,
        };
        let payload = ServerMessage::PresenceUpdate {
            peer_id: pubkey.to_string(),
            online,
        };
        for w in watchers {
            if let Some(peer) = registry.get(&w) {
                let _ = try_send(&peer.tx, payload.clone()).await;
            }
        }
    }

    /// Snapshot helper for tests / `subscribe_presence` ACK.
    pub fn watchers_of(&self, pubkey: &str) -> Vec<String> {
        self.watchers
            .get(pubkey)
            .map(|w| w.iter().cloned().collect())
            .unwrap_or_default()
    }
}

/// Convenience: probe whether one subscriber's tx is still alive (used for
/// best-effort cleanup of zombie watchers).
#[allow(dead_code)]
pub async fn is_alive(tx: &PeerSender) -> bool {
    !tx.is_closed()
}
