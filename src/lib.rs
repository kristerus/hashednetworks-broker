//! HashedNetworks coordination broker — library surface.
//!
//! This module re-exports the broker's internal modules plus the
//! [`AppState`] and [`build_router`] entry points used by the binary and
//! the integration tests.

pub mod auth;
pub mod db;
pub mod error;
pub mod handles;
pub mod metrics;
pub mod presence;
pub mod protocol;
pub mod registry;
pub mod relay;
pub mod signaling;
pub mod stun;
pub mod teams;

mod server;

pub use server::{build_router, serve_socket, AppState};
