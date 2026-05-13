-- v0.7: track the identity scope a peer registers with.
-- 'user' for human-owned identities (the historical default; backfilled here);
-- 'machine' for the hashednetworks-agent daemon's per-machine identity.

ALTER TABLE peers
    ADD COLUMN IF NOT EXISTS kind TEXT NOT NULL DEFAULT 'user'
    CHECK (kind IN ('user', 'machine'));

CREATE INDEX IF NOT EXISTS peers_kind_idx ON peers (kind);
