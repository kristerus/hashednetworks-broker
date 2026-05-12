//! STUN-style endpoint reflection.
//!
//! The broker reports the public IP+port it saw the peer connect from.
//! Inputs come from axum's `ConnectInfo<SocketAddr>` extractor on the WS
//! upgrade handler. There's not much logic here — just formatting.

use std::net::SocketAddr;

pub fn format_reflected(addr: SocketAddr) -> String {
    addr.to_string()
}
