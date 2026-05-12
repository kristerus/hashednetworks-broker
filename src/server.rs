//! Axum router + WebSocket session handler. Shared between the binary and
//! the integration test harness.

use crate::auth;
use crate::db::{self, DbPool};
use crate::error::{BrokerError, Result};
use crate::handles;
use crate::metrics::Metrics;
use crate::presence::PresenceManager;
use crate::protocol::{ClientMessage, ServerMessage, TeamMember};
use crate::registry::{PeerHandle, PeerRegistry};
use crate::relay::{self as relay_mod, RelayManager};
use crate::signaling::{self as signaling_mod, SignalingLimiter};
use crate::stun;
use crate::teams;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, State,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{any, get},
    Router,
};
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::{error, info, info_span, warn, Instrument};

#[derive(Clone)]
pub struct AppState {
    pub db: Option<DbPool>,
    pub registry: Arc<PeerRegistry>,
    pub presence: Arc<PresenceManager>,
    pub relay: Arc<RelayManager>,
    pub signaling_limiter: Arc<SignalingLimiter>,
    pub metrics: Arc<Metrics>,
}

impl AppState {
    /// In-memory state, no database. Suitable for tests + ephemeral runs.
    pub fn in_memory() -> Self {
        Self {
            db: None,
            registry: PeerRegistry::new(),
            presence: PresenceManager::new(),
            relay: Arc::new(RelayManager::new()),
            signaling_limiter: Arc::new(SignalingLimiter::new()),
            metrics: Metrics::new(),
        }
    }
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(|| async { "hashednetworks-broker v0.5" }))
        .route("/health", get(health))
        .route("/metrics", get(metrics_handler))
        .route("/ws", any(ws_upgrade))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let online = state.registry.len();
    (StatusCode::OK, format!("ok\npeers_online: {online}\n"))
}

async fn metrics_handler(State(state): State<AppState>) -> impl IntoResponse {
    let body = state.metrics.encode();
    (
        StatusCode::OK,
        [("content-type", "text/plain; version=0.0.4")],
        body,
    )
}

async fn ws_upgrade(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.max_message_size(256 * 1024)
        .on_upgrade(move |socket| async move {
            if let Err(e) = serve_socket(state, addr, socket).await {
                warn!(error = %e, peer_addr = %addr, "ws session ended with error");
            }
        })
}

/// Public so the integration-test harness can drive a session directly
/// off an already-upgraded socket if it wants. Production callers should
/// route via [`build_router`].
pub async fn serve_socket(state: AppState, addr: SocketAddr, socket: WebSocket) -> Result<()> {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::channel::<ServerMessage>(64);

    // Writer task: drain server messages to the websocket.
    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let text = match serde_json::to_string(&msg) {
                Ok(t) => t,
                Err(e) => {
                    error!(?msg, "serialize server message: {e}");
                    continue;
                }
            };
            if sink.send(Message::Text(text)).await.is_err() {
                break;
            }
        }
        let _ = sink.send(Message::Close(None)).await;
    });

    let first = match stream.next().await {
        Some(Ok(Message::Text(t))) => t,
        Some(Ok(Message::Binary(b))) => String::from_utf8_lossy(&b).into_owned(),
        _ => {
            let _ = tx
                .send(error_msg(
                    BrokerError::Malformed("first message must be JSON text".into()),
                    None,
                ))
                .await;
            return Err(BrokerError::Malformed("no register message".into()));
        }
    };
    let register_msg: ClientMessage = serde_json::from_str(&first)
        .map_err(|e| BrokerError::Malformed(format!("first message parse: {e}")))?;
    if !matches!(register_msg, ClientMessage::Register { .. }) {
        let _ = tx
            .send(error_msg(
                BrokerError::Malformed("first message must be `register`".into()),
                register_msg.request_id().map(|s| s.to_string()),
            ))
            .await;
        return Err(BrokerError::Malformed(
            "first message must be register".into(),
        ));
    }
    if let Err(e) = auth::verify_signed_message(&register_msg) {
        let req_id = register_msg.request_id().map(|s| s.to_string());
        let _ = tx.send(error_msg(e, req_id)).await;
        // Drop tx so the writer task drains the error and closes.
        drop(tx);
        let _ = writer.await;
        return Err(BrokerError::SignatureVerifyFailed);
    }

    let (pubkey, requested_handle, addresses, register_request_id) = match &register_msg {
        ClientMessage::Register {
            pubkey,
            handle,
            addresses,
            request_id,
            ..
        } => (
            pubkey.clone(),
            handle.clone(),
            addresses.clone(),
            request_id.clone(),
        ),
        _ => unreachable!(),
    };

    let mut handle_claimed = false;
    if let Some(pool) = state.db.as_ref() {
        if let Err(e) = db::upsert_peer(pool, &pubkey).await {
            warn!(?e, "upsert_peer failed (continuing)");
        }
        if let Some(h) = requested_handle.as_ref() {
            match handles::claim(pool, &pubkey, h).await {
                Ok(_) => {
                    handle_claimed = true;
                    state
                        .metrics
                        .handle_claims_total
                        .with_label_values(&["ok"])
                        .inc();
                }
                Err(BrokerError::HandleTaken | BrokerError::HandleAlreadyOwned) => {
                    state
                        .metrics
                        .handle_claims_total
                        .with_label_values(&["conflict"])
                        .inc();
                }
                Err(e) => {
                    state
                        .metrics
                        .handle_claims_total
                        .with_label_values(&["error"])
                        .inc();
                    warn!(error = %e, "handle claim error");
                }
            }
        }
    } else if requested_handle.is_some() {
        let _ = tx
            .send(error_msg(
                BrokerError::Internal("handle claims require Postgres".into()),
                register_request_id.clone(),
            ))
            .await;
    }

    let peer = PeerHandle {
        pubkey: pubkey.clone(),
        handle: if handle_claimed {
            requested_handle.clone()
        } else {
            None
        },
        addresses: addresses.clone(),
        reflected: addr,
        tx: tx.clone(),
    };
    state.registry.insert(peer);
    state.metrics.connected_peers.inc();
    state.metrics.registrations_total.inc();

    let _ = tx
        .send(ServerMessage::Registered {
            peer_id: pubkey.clone(),
            reflected_address: stun::format_reflected(addr),
            handle_claimed,
            request_id: register_request_id,
        })
        .await;

    state
        .presence
        .broadcast(&state.registry, &pubkey, true)
        .await;

    let span = info_span!("peer", pubkey = %pubkey, addr = %addr);
    let pump_result = pump_messages(state.clone(), pubkey.clone(), tx.clone(), &mut stream)
        .instrument(span)
        .await;

    state.registry.remove(&pubkey);
    state.metrics.connected_peers.dec();
    state.presence.clear_subscriber(&pubkey);
    state.relay.drop_sessions_for(&pubkey);
    state
        .presence
        .broadcast(&state.registry, &pubkey, false)
        .await;
    drop(tx);
    let _ = writer.await;
    pump_result
}

async fn pump_messages(
    state: AppState,
    pubkey: String,
    tx: mpsc::Sender<ServerMessage>,
    stream: &mut (impl StreamExt<Item = std::result::Result<Message, axum::Error>> + Unpin),
) -> Result<()> {
    while let Some(frame) = stream.next().await {
        let frame = match frame {
            Ok(f) => f,
            Err(e) => {
                info!(error = %e, "ws stream error, closing");
                break;
            }
        };
        let text = match frame {
            Message::Text(t) => t,
            Message::Binary(b) => String::from_utf8_lossy(&b).into_owned(),
            Message::Close(_) => {
                info!("peer sent Close");
                break;
            }
            Message::Ping(_) | Message::Pong(_) => continue,
        };
        let timer = state.metrics.message_handle_seconds.start_timer();
        let outcome = handle_client_message(&state, &pubkey, &text, &tx).await;
        timer.observe_duration();
        if let Err(e) = outcome {
            let req_id = parse_request_id(&text);
            let _ = tx.send(error_msg(e, req_id)).await;
        }
    }
    Ok(())
}

fn parse_request_id(text: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    v.get("request_id")?.as_str().map(|s| s.to_string())
}

fn error_msg(e: BrokerError, request_id: Option<String>) -> ServerMessage {
    ServerMessage::Error {
        code: e.code().to_string(),
        message: e.to_string(),
        request_id,
    }
}

async fn handle_client_message(
    state: &AppState,
    session_pubkey: &str,
    text: &str,
    tx: &mpsc::Sender<ServerMessage>,
) -> Result<()> {
    let msg: ClientMessage =
        serde_json::from_str(text).map_err(|e| BrokerError::Malformed(format!("parse: {e}")))?;
    auth::verify_signed_message(&msg)?;
    if msg.pubkey() != session_pubkey {
        return Err(BrokerError::Malformed(
            "message pubkey doesn't match session pubkey".into(),
        ));
    }

    match msg {
        ClientMessage::Register { request_id, .. } => {
            let _ = tx
                .send(ServerMessage::Registered {
                    peer_id: session_pubkey.to_string(),
                    reflected_address: state
                        .registry
                        .get(session_pubkey)
                        .map(|p| stun::format_reflected(p.reflected))
                        .unwrap_or_default(),
                    handle_claimed: state
                        .registry
                        .get(session_pubkey)
                        .and_then(|p| p.handle)
                        .is_some(),
                    request_id,
                })
                .await;
        }
        ClientMessage::LookupPeer {
            target_pubkey,
            request_id,
            ..
        } => {
            let info = state
                .registry
                .get(&target_pubkey)
                .map(|p| ServerMessage::PeerInfo {
                    peer_id: p.pubkey.clone(),
                    handle: p.handle.clone(),
                    addresses: p.addresses.clone(),
                    reflected_address: Some(stun::format_reflected(p.reflected)),
                    online: true,
                    request_id: request_id.clone(),
                });
            if let Some(m) = info {
                let _ = tx.send(m).await;
            } else {
                let _ = tx
                    .send(error_msg(BrokerError::PeerNotFound, request_id))
                    .await;
            }
        }
        ClientMessage::LookupHandle {
            target_handle,
            request_id,
            ..
        } => {
            let pk_opt = if let Some(pool) = state.db.as_ref() {
                handles::resolve(pool, &target_handle).await?
            } else {
                state.registry.resolve_handle(&target_handle)
            };
            let pk = match pk_opt {
                Some(pk) => pk,
                None => {
                    let _ = tx
                        .send(error_msg(BrokerError::PeerNotFound, request_id))
                        .await;
                    return Ok(());
                }
            };
            let online_peer = state.registry.get(&pk);
            let info = ServerMessage::PeerInfo {
                peer_id: pk.clone(),
                handle: Some(target_handle.clone()),
                addresses: online_peer
                    .as_ref()
                    .map(|p| p.addresses.clone())
                    .unwrap_or_default(),
                reflected_address: online_peer
                    .as_ref()
                    .map(|p| stun::format_reflected(p.reflected)),
                online: online_peer.is_some(),
                request_id,
            };
            let _ = tx.send(info).await;
        }
        ClientMessage::Signal { to, payload, .. } => {
            match signaling_mod::relay_signal(
                &state.registry,
                &state.signaling_limiter,
                session_pubkey,
                &to,
                &payload,
            )
            .await
            {
                Ok(()) => {
                    state.metrics.signaling_messages_total.inc();
                }
                Err(e) => {
                    state
                        .metrics
                        .signaling_rejected_total
                        .with_label_values(&[e.code()])
                        .inc();
                    return Err(e);
                }
            }
        }
        ClientMessage::SubscribePresence {
            peers, request_id, ..
        } => {
            state.presence.subscribe(session_pubkey, &peers);
            state.metrics.presence_subscriptions.add(peers.len() as i64);
            let online = state.registry.online_set(&peers);
            let _ = tx
                .send(ServerMessage::SubscribePresenceAck {
                    peers: peers.clone(),
                    request_id,
                })
                .await;
            for (pk, on) in online {
                let _ = tx
                    .send(ServerMessage::PresenceUpdate {
                        peer_id: pk,
                        online: on,
                    })
                    .await;
            }
        }
        ClientMessage::UnsubscribePresence { peers, .. } => {
            state.presence.unsubscribe(session_pubkey, &peers);
            state.metrics.presence_subscriptions.sub(peers.len() as i64);
        }
        ClientMessage::RequestRelay {
            peer, request_id, ..
        } => {
            let id = state
                .relay
                .request(&state.registry, session_pubkey, &peer)
                .await?;
            state.metrics.relay_sessions_total.inc();
            let _ = tx
                .send(ServerMessage::RelaySessionEstablished {
                    session_id: id.to_string(),
                    peer: peer.clone(),
                    request_id,
                })
                .await;
        }
        ClientMessage::AcceptRelay { session_id, .. } => {
            let id = relay_mod::parse_session_id(&session_id)?;
            // `accept()` already broadcasts RelaySessionEstablished to both
            // peers — don't send an additional copy here.
            state
                .relay
                .accept(&state.registry, session_pubkey, id)
                .await?;
        }
        ClientMessage::Relay {
            session_id, data, ..
        } => {
            let id = relay_mod::parse_session_id(&session_id)?;
            let bytes = state
                .relay
                .forward(&state.registry, session_pubkey, id, &data)
                .await?;
            state.metrics.relay_bytes_total.inc_by(bytes);
        }
        ClientMessage::TeamAnnounce {
            team_id,
            team_name,
            admin_pubkey,
            admin_signature,
            request_id,
            ..
        } => {
            let pool = state
                .db
                .as_ref()
                .ok_or(BrokerError::Internal("teams require Postgres".into()))?;
            let parsed_id = match team_id.as_deref() {
                Some(t) => Some(teams::parse_team_id(t)?),
                None => None,
            };
            let id = teams::announce(
                pool,
                session_pubkey,
                parsed_id,
                &team_name,
                &admin_pubkey,
                &admin_signature,
            )
            .await?;
            state.metrics.team_announces_total.inc();
            let _ = tx
                .send(ServerMessage::TeamAnnounced {
                    team_id: id.to_string(),
                    request_id,
                })
                .await;
        }
        ClientMessage::TeamLookup {
            team_id,
            request_id,
            ..
        } => {
            let pool = state
                .db
                .as_ref()
                .ok_or(BrokerError::Internal("teams require Postgres".into()))?;
            let id = teams::parse_team_id(&team_id)?;
            let members = teams::members(pool, id).await?;
            let members_out: Vec<TeamMember> = members
                .into_iter()
                .map(|m| TeamMember {
                    online: state.registry.is_online(&m.member_pubkey),
                    pubkey: m.member_pubkey,
                    joined_at: m.joined_at,
                })
                .collect();
            let _ = tx
                .send(ServerMessage::TeamMembers {
                    team_id,
                    members: members_out,
                    request_id,
                })
                .await;
        }
        ClientMessage::Ping { timestamp, .. } => {
            let _ = tx.send(ServerMessage::Pong { timestamp }).await;
        }
    }
    Ok(())
}
