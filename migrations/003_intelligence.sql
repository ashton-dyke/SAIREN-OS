-- SAIREN Fleet Hub — Intelligence Infrastructure
--
-- Adds an async job queue and output store for the hub-side LLM analysis
-- workers. The hub embeds mistralrs and runs these workers asynchronously so
-- rig report writes are never blocked by inference.
--
-- Job lifecycle:  pending → running → done
--                                   ↘ failed  (retried up to max_retries)

-- ─── Intelligence Job Queue ───────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS intelligence_jobs (
    id              TEXT        PRIMARY KEY,          -- UUID as text
    job_type        TEXT        NOT NULL,             -- 'formation_benchmark' | 'anomaly_fingerprint' | 'post_well_report' | 'benchmark_gap'
    status          TEXT        NOT NULL DEFAULT 'pending', -- pending | running | done | failed
    priority        INTEGER     NOT NULL DEFAULT 5,   -- 1 = highest, 10 = lowest
    input_data      JSONB       NOT NULL,             -- worker-specific input parameters
    retry_count     INTEGER     NOT NULL DEFAULT 0,
    max_retries     INTEGER     NOT NULL DEFAULT 3,
    error_message   TEXT,                             -- set on failure
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    claimed_at      TIMESTAMPTZ,                      -- when a worker started processing
    completed_at    TIMESTAMPTZ                       -- when done or permanently failed
);

-- ─── Intelligence Outputs ─────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS intelligence_outputs (
    id              TEXT        PRIMARY KEY,          -- UUID as text
    job_id          TEXT        NOT NULL REFERENCES intelligence_jobs(id),
    job_type        TEXT        NOT NULL,
    -- Scope (all optional — NULL means fleet-wide)
    rig_id          TEXT,
    well_id         TEXT,
    formation_name  TEXT,
    -- Content
    output_type     TEXT        NOT NULL,             -- 'advisory' | 'benchmark' | 'report' | 'fingerprint'
    content         TEXT        NOT NULL,             -- raw LLM output or summary text
    structured_data JSONB,                            -- parsed structured version
    confidence      DOUBLE PRECISION,                -- 0.0-1.0 when applicable
    -- Distribution tracking
    distributed     BOOLEAN     NOT NULL DEFAULT FALSE,
    distributed_at  TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ─── Indexes ──────────────────────────────────────────────────────────────────

-- Job queue: fast claim of highest-priority pending work
CREATE INDEX IF NOT EXISTS idx_intjobs_claim
    ON intelligence_jobs(priority ASC, created_at ASC)
    WHERE status = 'pending';

CREATE INDEX IF NOT EXISTS idx_intjobs_status   ON intelligence_jobs(status);
CREATE INDEX IF NOT EXISTS idx_intjobs_type     ON intelligence_jobs(job_type);
CREATE INDEX IF NOT EXISTS idx_intjobs_created  ON intelligence_jobs(created_at DESC);

-- Outputs: fast lookup by job, rig, or formation
CREATE INDEX IF NOT EXISTS idx_intout_job       ON intelligence_outputs(job_id);
CREATE INDEX IF NOT EXISTS idx_intout_rig       ON intelligence_outputs(rig_id) WHERE rig_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_intout_formation ON intelligence_outputs(formation_name) WHERE formation_name IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_intout_undistrib ON intelligence_outputs(created_at) WHERE distributed = FALSE;
