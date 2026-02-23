CREATE TABLE IF NOT EXISTS enrollment_tokens (
    token_hash    TEXT PRIMARY KEY,
    rig_id        TEXT NOT NULL,
    well_id       TEXT NOT NULL,
    field         TEXT NOT NULL,
    expires_at    TIMESTAMPTZ NOT NULL,
    used          BOOLEAN DEFAULT FALSE,
    used_at       TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_enrollment_tokens_expires
    ON enrollment_tokens(expires_at) WHERE used = FALSE;
