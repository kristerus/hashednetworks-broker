# HashedNetworks Broker — Wire Protocol v0.5

JSON over WebSocket. One peer per connection. The broker is at
`wss://broker.hashednetworks.network/ws` (TLS terminated at edge; inside
Fly the broker speaks plain `ws://`).

## Encoding rules

* **Transport.** RFC 6455 WebSocket. Both text and binary frames carry
  the same JSON payload; the broker prefers text.
* **Identifiers.**
  * `pubkey` — 64-character lowercase hex Ed25519 public key, optionally
    prefixed `ed25519:`.
  * `signature` — 128-character lowercase hex Ed25519 signature (64
    raw bytes).
  * `payload` and relay `data` are base64 (standard, with padding).
  * `team_id` and relay `session_id` are RFC 4122 UUIDs.
* **Timestamps.** Unix seconds, signed integer.
* **Replay window.** Messages older than 60 s or further than 60 s in
  the future are rejected (`stale_message` / `future_message`).

### Canonical signing

For every client message, the signature covers **the entire JSON object
with the `signature` field set to an empty string**. Pseudocode:

```text
canonical = json.dumps({...message, "signature": ""}, sort_keys=False)
signature = hex(ed25519_sign(sk, canonical))
```

Server canonicalises the same way on receipt and verifies under the
claimed `pubkey`.

## Lifecycle

1. WebSocket upgrade at `GET /ws`.
2. Peer sends `register` (signed) as the **first** message.
3. Broker verifies signature, persists the peer's pubkey/handle, sends
   `registered { reflected_address, handle_claimed, ... }`.
4. Peer issues any number of subsequent signed messages.
5. Either side closes; broker marks peer offline and notifies watchers.

## Client → Server messages

Every client message has these required fields in addition to those
listed: `pubkey`, `timestamp`, `signature`. Optional `request_id` is
echoed back on the relevant response.

### `register`

First message after connect.

```json
{
  "type": "register",
  "pubkey": "b3a91f...",
  "handle": "alice",
  "addresses": {
    "lan": ["192.168.1.42:51820"],
    "public": null
  },
  "timestamp": 1731340000,
  "signature": "f08a...",
  "request_id": "reg-1"
}
```

`handle` is optional. If supplied and the broker has a database, it
attempts to claim the handle.

### `lookup_peer`

```json
{ "type": "lookup_peer", "target_pubkey": "c4b8...", "pubkey": "...", "timestamp": ..., "signature": "..." }
```

### `lookup_handle`

```json
{ "type": "lookup_handle", "target_handle": "bob@org", "pubkey": "...", "timestamp": ..., "signature": "..." }
```

### `signal`

Forward an opaque base64 payload to another peer. Subject to per-sender
rate limits (100/min) and per-message size (4 KB after base64 decode).

```json
{ "type": "signal", "to": "c4b8...", "payload": "aGVsbG8gYm9i", "pubkey": "...", "timestamp": ..., "signature": "..." }
```

### `subscribe_presence` / `unsubscribe_presence`

```json
{ "type": "subscribe_presence", "peers": ["c4b8...", "d901..."], "pubkey": "...", "timestamp": ..., "signature": "..." }
```

ACK contains the original list; an initial `presence_update` for each
peer immediately follows.

### `request_relay`

Ask the broker to set up a TURN-style relay to a peer.

```json
{ "type": "request_relay", "peer": "c4b8...", "pubkey": "...", "timestamp": ..., "signature": "..." }
```

Broker creates a pending session, replies to the initiator with
`relay_session_established`, and sends `relay_offer` to the responder.

### `accept_relay`

Responder consents to a pending session, promoting it to active.

```json
{ "type": "accept_relay", "session_id": "<uuid>", "pubkey": "...", "timestamp": ..., "signature": "..." }
```

### `relay`

Forward a base64 frame within an active session (max 64 KB decoded).
Counts against the sender's bandwidth + daily quota.

```json
{ "type": "relay", "session_id": "<uuid>", "data": "...", "pubkey": "...", "timestamp": ..., "signature": "..." }
```

### `team_announce`

Announce membership. The team admin signs `team_id (16 bytes) ||
member_pubkey (32 bytes)`.

```json
{
  "type": "team_announce",
  "team_id": null,
  "team_name": "Engineering",
  "admin_pubkey": "9b...",
  "admin_signature": "ee...",
  "pubkey": "...",
  "timestamp": ...,
  "signature": "..."
}
```

If `team_id` is null the broker creates a new team and returns its id.

### `team_lookup`

```json
{ "type": "team_lookup", "team_id": "<uuid>", "pubkey": "...", "timestamp": ..., "signature": "..." }
```

### `ping`

Cheap liveness probe; signed because everything is.

```json
{ "type": "ping", "pubkey": "...", "timestamp": ..., "signature": "..." }
```

## Server → Client messages

### `registered`

```json
{
  "type": "registered",
  "peer_id": "b3a91f...",
  "reflected_address": "73.42.118.5:51820",
  "handle_claimed": true,
  "request_id": "reg-1"
}
```

### `peer_info`

Response to `lookup_peer` / `lookup_handle`.

```json
{
  "type": "peer_info",
  "peer_id": "c4b8...",
  "handle": "bob@org",
  "addresses": { "lan": ["10.0.0.4:51820"], "public": null },
  "reflected_address": "203.0.113.7:55222",
  "online": true,
  "request_id": "lkp-9"
}
```

### `signal`

Delivered to the recipient of a `signal`.

```json
{ "type": "signal", "from": "b3a91f...", "payload": "...", "timestamp": ... }
```

### `presence_update`

```json
{ "type": "presence_update", "peer_id": "c4b8...", "online": false }
```

### `subscribe_presence_ack`

```json
{ "type": "subscribe_presence_ack", "peers": ["c4b8..."], "request_id": "sp-1" }
```

### `relay_offer`

To the responder when an initiator opens a relay session.

```json
{ "type": "relay_offer", "session_id": "<uuid>", "from": "b3a91f..." }
```

### `relay_session_established`

To both peers when the responder accepts.

```json
{ "type": "relay_session_established", "session_id": "<uuid>", "peer": "...", "request_id": "..." }
```

### `relay_data`

```json
{ "type": "relay_data", "session_id": "<uuid>", "from": "...", "data": "..." }
```

### `team_announced` / `team_members`

```json
{ "type": "team_announced", "team_id": "<uuid>", "request_id": "ta-1" }
```

```json
{
  "type": "team_members",
  "team_id": "<uuid>",
  "members": [
    { "pubkey": "...", "online": true, "joined_at": "2026-05-11T10:00:00Z" }
  ],
  "request_id": "tl-1"
}
```

### `pong`

```json
{ "type": "pong", "timestamp": 1731340030 }
```

### `error`

```json
{ "type": "error", "code": "rate_limited", "message": "...", "request_id": "..." }
```

Error codes:

| `code` | Meaning |
|---|---|
| `invalid_pubkey` | Public key bytes don't parse. |
| `invalid_signature` | Signature bytes don't parse. |
| `signature_verify_failed` | Signature does not verify under the claimed pubkey. |
| `stale_message` | Timestamp older than 60 s. |
| `future_message` | Timestamp more than 60 s in the future. |
| `malformed` | JSON / field validation failure. |
| `handle_taken` | Another peer already owns the handle. |
| `handle_already_owned` | The peer already owns a different handle. |
| `peer_not_found` | Target peer / handle isn't known. |
| `peer_offline` | Target peer is not currently connected. |
| `payload_too_large` | Signaling or relay payload exceeds limits. |
| `rate_limited` | Per-peer rate limit hit. |
| `relay_quota_exceeded` | Daily relay quota reached. |
| `relay_not_consented` | Relay used out of order or by a non-party. |
| `relay_session_not_found` | Session id is unknown. |
| `database_error` | Backend DB failure. |
| `internal_error` | Catch-all. |

## Worked example

A two-peer signaling round trip (`alice` → `bob`):

1. Alice connects WS, sends `register` (with `request_id: "reg-a"`).
2. Broker replies `registered`.
3. Bob does the same.
4. Alice sends `signal { to: bob.pubkey, payload: "<base64>" }`.
5. Bob receives `signal { from: alice.pubkey, payload: "...", timestamp: ... }`.

A successful TURN-style relay session:

1. Alice sends `request_relay { peer: bob.pubkey }`.
2. Broker: to Alice → `relay_session_established { session_id, peer: bob }`;
   to Bob → `relay_offer { session_id, from: alice }`.
3. Bob sends `accept_relay { session_id }`.
4. Broker: to both → `relay_session_established`.
5. Either side sends `relay { session_id, data: "<base64>" }`.
6. Broker forwards as `relay_data` to the other party, charging the
   sender's bandwidth + daily quota.

## Versioning

Bumping the protocol breaks the wire format. The broker advertises
`v0.5` via the `Server` header; clients can probe `GET /` for a banner.
v0.6 will introduce protocol version negotiation; until then any minor
change is breaking.
