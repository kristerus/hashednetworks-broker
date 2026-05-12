-- HashedNetworks broker v0.5 — initial schema.
-- Holds only public metadata. No file content, no team data keys, no sigchain.

CREATE TABLE IF NOT EXISTS peers (
    pubkey      TEXT        PRIMARY KEY,       -- hex-encoded Ed25519 public key (64 chars)
    handle      TEXT        UNIQUE,            -- optional human-readable handle
    metadata    JSONB       NOT NULL DEFAULT '{}'::jsonb,
    first_seen  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS peers_handle_idx ON peers (handle) WHERE handle IS NOT NULL;

CREATE TABLE IF NOT EXISTS handles (
    handle      TEXT        PRIMARY KEY,
    pubkey      TEXT        NOT NULL REFERENCES peers(pubkey) ON DELETE CASCADE,
    claimed_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS teams (
    team_id        UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name           TEXT        NOT NULL,
    admin_pubkey   TEXT        NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS teams_admin_idx ON teams (admin_pubkey);

CREATE TABLE IF NOT EXISTS team_members (
    team_id          UUID        NOT NULL REFERENCES teams(team_id) ON DELETE CASCADE,
    member_pubkey    TEXT        NOT NULL,
    admin_signature  TEXT        NOT NULL,
    joined_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (team_id, member_pubkey)
);

CREATE INDEX IF NOT EXISTS team_members_member_idx ON team_members (member_pubkey);
