//! End-to-end smoke client.
//!
//! Spawns two simulated peers (Alice & Bob), each connecting to the broker
//! at $BROKER_URL (e.g. ws://localhost:8080/ws). Verifies:
//!   1. Both register successfully.
//!   2. Alice → Bob signaling delivery.
//!   3. Relay session establishment + bidirectional payload.
//!
//! Exits 0 on success, 1 on failure.

use base64::Engine;
use chrono::Utc;
use ed25519_dalek::{Signer, SigningKey};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

struct Peer {
    sk: SigningKey,
    pubkey_hex: String,
    ws: WsStream,
}

impl Peer {
    async fn connect(url: &str) -> anyhow::Result<Self> {
        let mut rng = rand::rngs::OsRng;
        let sk = SigningKey::generate(&mut rng);
        let pubkey_hex = hex::encode(sk.verifying_key().to_bytes());
        let (ws, _) = connect_async(url).await?;
        Ok(Self { sk, pubkey_hex, ws })
    }

    /// Sign a message body (object Value), insert pubkey + timestamp +
    /// signature, return JSON text ready to send.
    fn sign(&self, mut body: Value) -> String {
        let now = Utc::now().timestamp();
        let obj = body.as_object_mut().expect("body is object");
        obj.insert("pubkey".into(), Value::String(self.pubkey_hex.clone()));
        obj.insert("timestamp".into(), Value::from(now));
        obj.insert("signature".into(), Value::String(String::new()));
        let canonical = serde_json::to_vec(&body).unwrap();
        let sig = self.sk.sign(&canonical);
        body.as_object_mut()
            .unwrap()
            .insert("signature".into(), Value::String(hex::encode(sig.to_bytes())));
        serde_json::to_string(&body).unwrap()
    }

    async fn send(&mut self, body: Value) -> anyhow::Result<()> {
        let text = self.sign(body);
        self.ws.send(Message::Text(text)).await?;
        Ok(())
    }

    async fn recv(&mut self) -> anyhow::Result<Value> {
        loop {
            match self.ws.next().await {
                Some(Ok(Message::Text(t))) => return Ok(serde_json::from_str(&t)?),
                Some(Ok(Message::Ping(_))) => continue,
                Some(Ok(Message::Pong(_))) => continue,
                Some(Ok(Message::Close(_))) => anyhow::bail!("ws closed"),
                Some(Ok(other)) => anyhow::bail!("unexpected ws frame: {other:?}"),
                Some(Err(e)) => anyhow::bail!("ws error: {e}"),
                None => anyhow::bail!("ws stream ended"),
            }
        }
    }

    async fn expect_type(&mut self, ty: &str) -> anyhow::Result<Value> {
        let v = self.recv().await?;
        let got = v.get("type").and_then(|s| s.as_str()).unwrap_or("(none)");
        if got != ty {
            anyhow::bail!("expected type={ty}, got {v}");
        }
        Ok(v)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let broker = std::env::var("BROKER_URL").unwrap_or_else(|_| "ws://localhost:8080/ws".into());
    eprintln!("[smoke] broker = {broker}");

    let mut alice = Peer::connect(&broker).await?;
    let mut bob = Peer::connect(&broker).await?;
    eprintln!("[smoke] connected; alice={} bob={}", &alice.pubkey_hex[..16], &bob.pubkey_hex[..16]);

    alice
        .send(json!({
            "type": "register",
            "addresses": { "lan": ["192.168.1.10:51820"], "public": null },
            "request_id": "reg-alice"
        }))
        .await?;
    let reg_alice = alice.expect_type("registered").await?;
    eprintln!("[smoke] alice registered: {}", reg_alice);

    bob.send(json!({
        "type": "register",
        "addresses": { "lan": ["192.168.1.20:51820"], "public": null },
        "request_id": "reg-bob"
    }))
    .await?;
    let reg_bob = bob.expect_type("registered").await?;
    eprintln!("[smoke] bob registered: {}", reg_bob);

    // Signaling Alice -> Bob.
    let signal_payload = base64::engine::general_purpose::STANDARD.encode(b"hello bob from alice");
    alice
        .send(json!({
            "type": "signal",
            "to": &bob.pubkey_hex,
            "payload": signal_payload,
            "request_id": "sig-1"
        }))
        .await?;
    let got = bob.expect_type("signal").await?;
    let from = got.get("from").and_then(|s| s.as_str()).unwrap_or("");
    if from != alice.pubkey_hex {
        anyhow::bail!("signal from mismatch: {from}");
    }
    eprintln!("[smoke] signaling alice->bob OK");

    // Relay: alice requests, bob accepts, both send data.
    alice
        .send(json!({
            "type": "request_relay",
            "peer": &bob.pubkey_hex,
            "request_id": "relay-1"
        }))
        .await?;
    let session_msg = alice.expect_type("relay_session_established").await?;
    let session_id = session_msg
        .get("session_id")
        .and_then(|s| s.as_str())
        .expect("session_id")
        .to_string();
    eprintln!("[smoke] alice opened relay session {session_id}");

    let offer = bob.expect_type("relay_offer").await?;
    let offered_id = offer.get("session_id").and_then(|s| s.as_str()).unwrap();
    assert_eq!(offered_id, session_id);

    bob.send(json!({
        "type": "accept_relay",
        "session_id": session_id,
        "request_id": "accept-1"
    }))
    .await?;
    // Each peer gets a relay_session_established after accept.
    bob.expect_type("relay_session_established").await?;
    alice.expect_type("relay_session_established").await?;
    eprintln!("[smoke] relay session active");

    // Alice -> Bob.
    let payload_a = base64::engine::general_purpose::STANDARD.encode(b"relay frame A");
    alice
        .send(json!({
            "type": "relay",
            "session_id": session_id,
            "data": payload_a,
        }))
        .await?;
    let recv_b = bob.expect_type("relay_data").await?;
    assert_eq!(recv_b.get("data").and_then(|s| s.as_str()).unwrap(), payload_a);

    // Bob -> Alice.
    let payload_b = base64::engine::general_purpose::STANDARD.encode(b"relay frame B");
    bob.send(json!({
        "type": "relay",
        "session_id": session_id,
        "data": payload_b,
    }))
    .await?;
    let recv_a = alice.expect_type("relay_data").await?;
    assert_eq!(recv_a.get("data").and_then(|s| s.as_str()).unwrap(), payload_b);
    eprintln!("[smoke] relay bidirectional OK");

    // Tiny pause so server-side cleanup runs cleanly when we drop.
    tokio::time::sleep(Duration::from_millis(50)).await;
    eprintln!("[smoke] ALL CHECKS PASSED");
    Ok(())
}
