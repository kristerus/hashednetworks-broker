# hashednetworks-broker

Coordination broker for **HashedNetworks v0.5** — like Tailscale's
coordination server: helps peers find each other and establish direct
connections, but **never sees actual data**. Free for individuals.

## What the broker does

| Role | Description |
|---|---|
| **Peer registration** | Peers prove Ed25519 keypair ownership over WebSocket, announce LAN/public addresses. |
| **Handle claiming** | Optional human-readable handles (`alice`, `alice@org`). First-come, first-served. Postgres-backed. |
| **Peer discovery** | Lookup by pubkey or handle. |
| **Signaling relay** | Forward opaque 4 KB payloads peer-to-peer for NAT-traversal negotiation. |
| **STUN-style reflection** | Reports the public IP+port the broker saw on connect. |
| **TURN-style fallback** | Rate-limited bidirectional relay (1 MB/s peak, 1 GB/day) when hole-punching fails. |
| **Team membership** | Admin-signed member announcements; team-mate discovery. |
| **Presence pub/sub** | Subscribe to other peers' online/offline state. |

## What the broker does NOT do

* See file content, team data keys, or sigchain entries.
* Issue or vault keys.
* Authenticate via passwords, email, or OAuth.
* Bill / meter — v0.5 is free tier only.

## Stack

* Rust + `axum 0.7` + `tokio` + `tower-http`
* `sqlx` (Postgres, runtime queries, embedded migrations)
* In-memory pub/sub via `dashmap` + `tokio::sync::mpsc`
* `ed25519-dalek` for signature verification
* `governor` for per-peer rate limiting
* `prometheus` + `tracing` for ops
* Docker multi-stage build, Fly.io-ready

## Quick start (local)

```bash
# Bring up Postgres + broker
docker compose up --build

# In another terminal, run the smoke client to exercise registration,
# signaling, and relay end-to-end:
docker compose run --rm smoke

# Open metrics
curl http://localhost:8080/metrics
```

Or run the broker natively against a local Postgres:

```bash
# 1. Start Postgres
docker compose up -d postgres

# 2. Run the broker
DATABASE_URL=postgres://broker:broker_dev_password@localhost:5432/broker \
  RUST_LOG=info cargo run --release --bin broker

# 3. Exercise it
BROKER_URL=ws://localhost:8080/ws cargo run --release --bin smoke-client
```

You can also run the broker **without a database** for ephemeral testing —
registration, lookup, signaling, presence, and relay all work; only handle
claims and team operations return an error.

## Binaries

| Binary | Purpose |
|---|---|
| `broker` | The server. |
| `smoke-client` | Connects two simulated peers, exchanges signaling, runs a relay session. Used as the docker-compose end-to-end check. |

## Endpoints

| Path | Method | Purpose |
|---|---|---|
| `/` | GET | Banner string. |
| `/health` | GET | Health check (used by Fly + Docker). |
| `/metrics` | GET | Prometheus exposition. |
| `/ws` | WebSocket upgrade | The peer protocol. |

## Wire protocol

JSON over WebSocket. Every client message carries `pubkey`, `timestamp`
(unix seconds), `signature` (hex Ed25519) over the canonical bytes.
Replay window is 60 seconds. See [PROTOCOL.md](./PROTOCOL.md) for every
message type with examples.

## Deploying to Fly.io

```bash
# One-time: create the app + Postgres + attach.
fly launch --no-deploy --copy-config         # generates fly.toml app name
fly postgres create --name hashednetworks-broker-db --region fra
fly postgres attach hashednetworks-broker-db
fly secrets set RUST_LOG="info,hashednetworks_broker=info"
fly deploy

# Subsequent deploys:
fly deploy

# Observe:
fly logs
fly status
curl https://<your-app>.fly.dev/metrics
```

`fly postgres attach` automatically injects `DATABASE_URL` as a Fly secret —
the broker reads it via `clap`/`env`. TLS terminates at the Fly edge;
internally the broker speaks plain `ws://`.

## Free-tier limits

| Resource | Limit |
|---|---|
| Handles per pubkey | 1 |
| Active connections per peer | unlimited |
| Signaling messages | 100 / minute / pubkey |
| Relay bandwidth | 1 MB/s peak / pubkey (token bucket) |
| Relay daily quota | 1 GB / 24h / pubkey |
| Signaling payload | 4 KB / message (after base64 decode) |
| Relay frame | 64 KB / frame (after base64 decode) |

## Project layout

```
hashednetworks-broker/
├── Cargo.toml
├── Dockerfile
├── fly.toml
├── docker-compose.yml
├── PROTOCOL.md
├── README.md
├── migrations/
│   └── 0001_init.sql           Postgres schema (peers, handles, teams, team_members)
├── src/
│   ├── main.rs                 axum server, WS upgrade, message dispatch
│   ├── bin/smoke_client.rs     End-to-end smoke test binary
│   ├── protocol.rs             Wire types + canonical signing
│   ├── auth.rs                 Ed25519 verification + replay window
│   ├── db.rs                   Postgres queries (sqlx runtime macros)
│   ├── registry.rs             In-memory pubkey -> sender map
│   ├── handles.rs              Handle claim flow + validation
│   ├── teams.rs                Team announce + admin signature verify
│   ├── signaling.rs            Signal relay + rate limiting
│   ├── relay.rs                TURN-style fallback (rate-limited)
│   ├── presence.rs             Presence pub/sub
│   ├── stun.rs                 Endpoint reflection (one helper)
│   ├── metrics.rs              Prometheus registry
│   └── error.rs                BrokerError + code mapping
└── tests/
    └── integration.rs          In-process broker + simulated peers
```

## Tests

```bash
# Unit tests (no DB required)
cargo test --lib

# Integration tests (spawn broker in-process; no DB required)
cargo test --test integration
```

Postgres-backed flows (handle claims, team membership) are exercised
end-to-end via `docker compose run smoke` and via the Fly.io deploy
healthcheck.

## License

Proprietary. © HashedNetworks.
