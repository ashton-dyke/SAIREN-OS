-- SAIREN Fleet Hub — Initial Schema
-- Creates the core tables for fleet event storage, episode library, rig registry, and sync log.

-- Rig registry
CREATE TABLE IF NOT EXISTS rigs (
    rig_id TEXT PRIMARY KEY,
    api_key_hash TEXT NOT NULL,
    well_id TEXT,
    field TEXT,
    registered_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen TIMESTAMPTZ,
    last_sync TIMESTAMPTZ,
    event_count INTEGER DEFAULT 0,
    status TEXT DEFAULT 'active'
);

-- Raw fleet events
CREATE TABLE IF NOT EXISTS events (
    id TEXT PRIMARY KEY,
    rig_id TEXT NOT NULL REFERENCES rigs(rig_id),
    well_id TEXT NOT NULL,
    field TEXT,
    campaign TEXT NOT NULL,
    risk_level TEXT NOT NULL,
    category TEXT,
    depth DOUBLE PRECISION,
    timestamp TIMESTAMPTZ NOT NULL,
    outcome TEXT DEFAULT 'Pending',
    action_taken TEXT,
    notes TEXT,
    payload JSONB NOT NULL,
    needs_curation BOOLEAN DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Curated episode library
CREATE TABLE IF NOT EXISTS episodes (
    id TEXT PRIMARY KEY,
    source_event_id TEXT REFERENCES events(id),
    rig_id TEXT NOT NULL,
    category TEXT NOT NULL,
    campaign TEXT NOT NULL,
    depth_min DOUBLE PRECISION,
    depth_max DOUBLE PRECISION,
    risk_level TEXT NOT NULL,
    severity TEXT NOT NULL,
    outcome TEXT NOT NULL,
    resolution TEXT,
    score DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    key_metrics JSONB NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    archived BOOLEAN DEFAULT FALSE
);

-- Sync log — tracks what each rig has received
CREATE TABLE IF NOT EXISTS sync_log (
    rig_id TEXT NOT NULL REFERENCES rigs(rig_id),
    synced_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    episodes_sent INTEGER NOT NULL,
    library_version INTEGER NOT NULL,
    PRIMARY KEY (rig_id, synced_at)
);

-- Library version sequence (incremented by curator)
CREATE SEQUENCE IF NOT EXISTS library_version_seq START 1;

-- Indexes: Events
CREATE INDEX IF NOT EXISTS idx_events_rig ON events(rig_id);
CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);
CREATE INDEX IF NOT EXISTS idx_events_needs_curation ON events(needs_curation) WHERE needs_curation = TRUE;

-- Indexes: Episodes
CREATE INDEX IF NOT EXISTS idx_episodes_category ON episodes(category);
CREATE INDEX IF NOT EXISTS idx_episodes_campaign ON episodes(campaign);
CREATE INDEX IF NOT EXISTS idx_episodes_score ON episodes(score DESC);
CREATE INDEX IF NOT EXISTS idx_episodes_updated ON episodes(updated_at);
CREATE INDEX IF NOT EXISTS idx_episodes_active ON episodes(archived) WHERE archived = FALSE;

-- Auto-update updated_at trigger
CREATE OR REPLACE FUNCTION update_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE TRIGGER events_updated_at
    BEFORE UPDATE ON events
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();

CREATE OR REPLACE TRIGGER episodes_updated_at
    BEFORE UPDATE ON episodes
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();
