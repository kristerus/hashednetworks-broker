//! In-memory registry of connected peers.
//!
//! Maps pubkey -> [`PeerHandle`], which carries an MPSC sender that delivers
//! [`ServerMessage`] envelopes to the peer's WebSocket write task. Also
//! maintains a secondary index from handle -> pubkey for lookups.
//!
//! All state is ephemeral: cleared on broker restart. Durable handle and
//! team data lives in Postgres ([`crate::db`]).

use crate::error::{BrokerError, Result};
use crate::protocol::{PeerAddresses, ServerMessage};
use dashmap::DashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Send-end of the per-peer outbound channel. Cheap to clone.
pub type PeerSender = mpsc::Sender<ServerMessage>;

#[derive(Clone)]
pub struct PeerHandle {
    pub pubkey: String,
    pub handle: Option<String>,
    pub addresses: PeerAddresses,
    pub reflected: SocketAddr,
    pub tx: PeerSender,
}

#[derive(Default)]
pub struct PeerRegistry {
    by_pubkey: DashMap<String, PeerHandle>,
    handle_to_pubkey: DashMap<String, String>,
}

impl PeerRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn insert(&self, peer: PeerHandle) {
        if let Some(handle) = peer.handle.as_ref() {
            self.handle_to_pubkey
                .insert(handle.clone(), peer.pubkey.clone());
        }
        self.by_pubkey.insert(peer.pubkey.clone(), peer);
    }

    pub fn remove(&self, pubkey: &str) -> Option<PeerHandle> {
        let (_, peer) = self.by_pubkey.remove(pubkey)?;
        if let Some(handle) = peer.handle.as_ref() {
            // Only clear the index if it still points at us — another
            // connection for the same handle (re-register) may have already
            // replaced the entry.
            if let Some(entry) = self.handle_to_pubkey.get(handle) {
                if *entry == peer.pubkey {
                    drop(entry);
                    self.handle_to_pubkey.remove(handle);
                }
            }
        }
        Some(peer)
    }

    pub fn get(&self, pubkey: &str) -> Option<PeerHandle> {
        self.by_pubkey.get(pubkey).map(|p| p.clone())
    }

    pub fn resolve_handle(&self, handle: &str) -> Option<String> {
        self.handle_to_pubkey.get(handle).map(|v| v.clone())
    }

    pub fn is_online(&self, pubkey: &str) -> bool {
        self.by_pubkey.contains_key(pubkey)
    }

    pub fn len(&self) -> usize {
        self.by_pubkey.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_pubkey.is_empty()
    }

    pub fn online_set(&self, pubkeys: &[String]) -> Vec<(String, bool)> {
        pubkeys
            .iter()
            .map(|pk| (pk.clone(), self.by_pubkey.contains_key(pk)))
            .collect()
    }
}

/// Try to send a message to a peer. Maps a "channel closed" outcome to
/// [`BrokerError::PeerOffline`].
pub async fn try_send(tx: &PeerSender, msg: ServerMessage) -> Result<()> {
    tx.send(msg).await.map_err(|_| BrokerError::PeerOffline)
}
