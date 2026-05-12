//! End-to-end tests against an in-process broker.
//!
//! Spawns the broker on a random port (no DATABASE_URL — registration,
//! signaling, presence, and relay all work; handle claims + team
//! operations are out of scope for these tests and exercised via
//! docker-compose).

use base64::Engine;
use chrono::Utc;
use ed25519_dalek::{Signer, SigningKey};
use futures_util::{SinkExt, StreamExt};
use hashednetworks_broker::{build_router, AppState};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// Spawn the broker on a random localhost port and return its base URL.
async fn spawn_broker() -> String {
    use std::sync::Once;
    static TRACE_INIT: Once = Once::new();
    TRACE_INIT.call_once(|| {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .with_test_writer()
            .try_init();
    });

    let state = AppState::in_memory();
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let res = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
        eprintln!("[test broker] serve ended: {res:?}");
    });
    // Give the listener a tick to start accepting.
    tokio::time::sleep(Duration::from_millis(50)).await;
    format!("ws://{addr}/ws")
}

struct Peer {
    sk: SigningKey,
    pubkey_hex: String,
    ws: WsStream,
}

impl Peer {
    async fn connect(url: &str) -> Self {
        let mut rng = rand::rngs::OsRng;
        let sk = SigningKey::generate(&mut rng);
        let pubkey_hex = hex::encode(sk.verifying_key().to_bytes());
        let (ws, _) = connect_async(url).await.expect("ws connect");
        Self { sk, pubkey_hex, ws }
    }

    fn sign(&self, mut body: Value) -> String {
        let now = Utc::now().timestamp();
        let obj = body.as_object_mut().expect("body is object");
        obj.insert("pubkey".into(), Value::String(self.pubkey_hex.clone()));
        obj.insert("timestamp".into(), Value::from(now));
        obj.insert("signature".into(), Value::String(String::new()));
        let canonical = serde_json::to_vec(&body).unwrap();
        let sig = self.sk.sign(&canonical);
        body.as_object_mut().unwrap().insert(
            "signature".into(),
            Value::String(hex::encode(sig.to_bytes())),
        );
        serde_json::to_string(&body).unwrap()
    }

    async fn send(&mut self, body: Value) {
        let text = self.sign(body);
        self.ws.send(Message::Text(text)).await.expect("ws send");
    }

    async fn recv(&mut self) -> Value {
        loop {
            let frame = timeout(Duration::from_secs(2), self.ws.next())
                .await
                .expect("recv timed out")
                .expect("stream not ended")
                .expect("ws ok");
            match frame {
                Message::Text(t) => return serde_json::from_str(&t).unwrap(),
                Message::Binary(b) => return serde_json::from_slice(&b).unwrap(),
                Message::Ping(_) | Message::Pong(_) => continue,
                Message::Close(_) => panic!("ws closed unexpectedly"),
                _ => continue,
            }
        }
    }

    async fn expect(&mut self, ty: &str) -> Value {
        let v = self.recv().await;
        let got = v.get("type").and_then(|s| s.as_str()).unwrap_or("");
        assert_eq!(got, ty, "expected message type {ty}, got {v}");
        v
    }
}

#[tokio::test]
async fn registration_and_lookup() {
    let url = spawn_broker().await;
    let mut alice = Peer::connect(&url).await;

    alice
        .send(json!({
            "type": "register",
            "addresses": { "lan": ["192.168.1.42:51820"], "public": null },
            "request_id": "r1"
        }))
        .await;
    let reg = alice.expect("registered").await;
    assert_eq!(
        reg.get("peer_id").unwrap().as_str().unwrap(),
        alice.pubkey_hex
    );
    assert!(reg
        .get("reflected_address")
        .unwrap()
        .as_str()
        .unwrap()
        .starts_with("127.0.0.1:"));

    // Lookup self.
    let target = alice.pubkey_hex.clone();
    alice
        .send(json!({
            "type": "lookup_peer",
            "target_pubkey": target,
            "request_id": "l1"
        }))
        .await;
    let info = alice.expect("peer_info").await;
    assert_eq!(info.get("online").unwrap().as_bool().unwrap(), true);
}

#[tokio::test]
async fn signal_delivers_between_peers() {
    let url = spawn_broker().await;
    let mut alice = Peer::connect(&url).await;
    let mut bob = Peer::connect(&url).await;

    alice
        .send(json!({
            "type": "register",
            "addresses": { "lan": ["10.0.0.1:51820"], "public": null }
        }))
        .await;
    alice.expect("registered").await;

    bob.send(json!({
        "type": "register",
        "addresses": { "lan": ["10.0.0.2:51820"], "public": null }
    }))
    .await;
    bob.expect("registered").await;

    let payload = base64::engine::general_purpose::STANDARD.encode(b"hello bob");
    alice
        .send(json!({
            "type": "signal",
            "to": bob.pubkey_hex.clone(),
            "payload": payload.clone()
        }))
        .await;
    let recv = bob.expect("signal").await;
    assert_eq!(
        recv.get("from").unwrap().as_str().unwrap(),
        alice.pubkey_hex
    );
    assert_eq!(recv.get("payload").unwrap().as_str().unwrap(), payload);
}

#[tokio::test]
async fn relay_round_trip() {
    let url = spawn_broker().await;
    let mut alice = Peer::connect(&url).await;
    let mut bob = Peer::connect(&url).await;

    for peer in [&mut alice, &mut bob] {
        peer.send(json!({
            "type": "register",
            "addresses": { "lan": ["127.0.0.1:0"], "public": null }
        }))
        .await;
        peer.expect("registered").await;
    }

    alice
        .send(json!({
            "type": "request_relay",
            "peer": bob.pubkey_hex.clone(),
            "request_id": "rq1"
        }))
        .await;
    let session = alice.expect("relay_session_established").await;
    let session_id = session
        .get("session_id")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();

    let offer = bob.expect("relay_offer").await;
    assert_eq!(
        offer.get("session_id").unwrap().as_str().unwrap(),
        session_id
    );

    bob.send(json!({
        "type": "accept_relay",
        "session_id": session_id.clone(),
        "request_id": "ac1"
    }))
    .await;
    bob.expect("relay_session_established").await;
    alice.expect("relay_session_established").await;

    // Bidirectional data.
    let pa = base64::engine::general_purpose::STANDARD.encode(b"frame A");
    alice
        .send(json!({
            "type": "relay",
            "session_id": session_id.clone(),
            "data": pa.clone()
        }))
        .await;
    let got_b = bob.expect("relay_data").await;
    assert_eq!(got_b.get("data").unwrap().as_str().unwrap(), pa);

    let pb = base64::engine::general_purpose::STANDARD.encode(b"frame B");
    bob.send(json!({
        "type": "relay",
        "session_id": session_id.clone(),
        "data": pb.clone()
    }))
    .await;
    let got_a = alice.expect("relay_data").await;
    assert_eq!(got_a.get("data").unwrap().as_str().unwrap(), pb);
}

#[tokio::test]
async fn presence_subscription_emits_online_offline() {
    let url = spawn_broker().await;
    let mut alice = Peer::connect(&url).await;
    let mut bob = Peer::connect(&url).await;

    for peer in [&mut alice, &mut bob] {
        peer.send(json!({
            "type": "register",
            "addresses": { "lan": ["127.0.0.1:0"], "public": null }
        }))
        .await;
        peer.expect("registered").await;
    }

    // Alice subscribes to Bob. She must receive an initial presence update
    // (Bob is online) immediately after the ACK.
    alice
        .send(json!({
            "type": "subscribe_presence",
            "peers": [bob.pubkey_hex.clone()],
            "request_id": "sp1"
        }))
        .await;
    alice.expect("subscribe_presence_ack").await;
    let pu = alice.expect("presence_update").await;
    assert_eq!(pu.get("peer_id").unwrap().as_str().unwrap(), bob.pubkey_hex);
    assert_eq!(pu.get("online").unwrap().as_bool().unwrap(), true);

    // When Bob disconnects, Alice should get a presence_update(false).
    drop(bob);
    let pu = alice.expect("presence_update").await;
    assert_eq!(pu.get("online").unwrap().as_bool().unwrap(), false);
}

#[tokio::test]
async fn rejects_stale_signed_message() {
    let url = spawn_broker().await;
    let mut peer = Peer::connect(&url).await;

    // Hand-roll a stale (timestamp = now - 5 min) register.
    let mut rng = rand::rngs::OsRng;
    let sk = SigningKey::generate(&mut rng);
    let pubkey_hex = hex::encode(sk.verifying_key().to_bytes());
    let stale_ts = Utc::now().timestamp() - 5 * 60;
    let mut body = json!({
        "type": "register",
        "pubkey": pubkey_hex,
        "addresses": { "lan": [], "public": null },
        "timestamp": stale_ts,
        "signature": "",
    });
    let canonical = serde_json::to_vec(&body).unwrap();
    let sig = sk.sign(&canonical);
    body.as_object_mut().unwrap().insert(
        "signature".into(),
        Value::String(hex::encode(sig.to_bytes())),
    );
    peer.ws
        .send(Message::Text(serde_json::to_string(&body).unwrap()))
        .await
        .unwrap();

    let resp = peer.recv().await;
    assert_eq!(resp.get("type").unwrap().as_str().unwrap(), "error");
    assert_eq!(resp.get("code").unwrap().as_str().unwrap(), "stale_message");
}
