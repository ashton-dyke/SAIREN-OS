-- Fleet Performance — stores post-well performance data for offset well sharing

CREATE TABLE IF NOT EXISTS fleet_performance (
    id SERIAL PRIMARY KEY,
    rig_id TEXT NOT NULL,
    well_id TEXT NOT NULL,
    field TEXT NOT NULL,
    formation_name TEXT NOT NULL,
    performance JSONB NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE(well_id, formation_name)
);

-- Indexes for efficient querying by field and timestamp
CREATE INDEX IF NOT EXISTS idx_fleet_performance_field ON fleet_performance(field);
CREATE INDEX IF NOT EXISTS idx_fleet_performance_updated ON fleet_performance(updated_at);
CREATE INDEX IF NOT EXISTS idx_fleet_performance_rig ON fleet_performance(rig_id);

-- Auto-update updated_at trigger
CREATE OR REPLACE TRIGGER fleet_performance_updated_at
    BEFORE UPDATE ON fleet_performance
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();
