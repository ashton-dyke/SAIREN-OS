//! Intelligence job queue — PostgreSQL-backed async work queue
//!
//! Uses `SELECT FOR UPDATE SKIP LOCKED` so multiple hub instances (or future
//! worker threads) can each claim their own job without stepping on each other.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::warn;

// ─── Job Types ────────────────────────────────────────────────────────────────

/// Recognised job type strings (store as TEXT in DB so new types don't need migrations)
pub mod job_type {
    pub const FORMATION_BENCHMARK: &str = "formation_benchmark";
    pub const ANOMALY_FINGERPRINT:  &str = "anomaly_fingerprint";
    pub const POST_WELL_REPORT:     &str = "post_well_report";
    pub const BENCHMARK_GAP:        &str = "benchmark_gap";
}

// ─── Domain types ─────────────────────────────────────────────────────────────

/// A claimed intelligence job ready for processing
#[derive(Debug, Clone)]
pub struct IntelligenceJob {
    pub id: String,
    pub job_type: String,
    pub input_data: serde_json::Value,
    pub priority: i32,
    pub retry_count: i32,
    pub max_retries: i32,
}

/// Output produced by a worker — stored in `intelligence_outputs`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerOutput {
    /// 'advisory' | 'benchmark' | 'report' | 'fingerprint'
    pub output_type: String,
    /// Raw LLM text or formatted summary
    pub content: String,
    /// Parsed/structured version of content (optional)
    pub structured_data: Option<serde_json::Value>,
    /// Confidence 0.0–1.0 (optional)
    pub confidence: Option<f64>,
    // Scope — all optional
    pub rig_id: Option<String>,
    pub well_id: Option<String>,
    pub formation_name: Option<String>,
}

// ─── Queue Operations ─────────────────────────────────────────────────────────

/// Enqueue a new job, returning its generated ID.
///
/// Jobs with lower `priority` values are processed first (1 = highest priority).
pub async fn enqueue_job(
    pool: &PgPool,
    job_type: &str,
    input_data: serde_json::Value,
    priority: i32,
) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();

    sqlx::query(
        "INSERT INTO intelligence_jobs (id, job_type, input_data, priority) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(&id)
    .bind(job_type)
    .bind(&input_data)
    .bind(priority)
    .execute(pool)
    .await?;

    Ok(id)
}

/// Atomically claim the next pending job.
///
/// Uses `SELECT FOR UPDATE SKIP LOCKED` so concurrent callers never claim the
/// same row. Returns `None` when the queue is empty.
pub async fn claim_job(pool: &PgPool) -> Result<Option<IntelligenceJob>, sqlx::Error> {
    // Single atomic UPDATE … RETURNING that claims the top-priority pending job.
    let row: Option<(String, String, serde_json::Value, i32, i32, i32)> = sqlx::query_as(
        r#"
        UPDATE intelligence_jobs
        SET    status = 'running',
               claimed_at = NOW()
        WHERE  id = (
            SELECT id
            FROM   intelligence_jobs
            WHERE  status = 'pending'
              AND  retry_count < max_retries
            ORDER  BY priority ASC, created_at ASC
            LIMIT  1
            FOR UPDATE SKIP LOCKED
        )
        RETURNING id, job_type, input_data, priority, retry_count, max_retries
        "#,
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(id, job_type, input_data, priority, retry_count, max_retries)| {
        IntelligenceJob {
            id,
            job_type,
            input_data,
            priority,
            retry_count,
            max_retries,
        }
    }))
}

/// Mark a job as successfully completed and persist its output (if any).
pub async fn complete_job(
    pool: &PgPool,
    job: &IntelligenceJob,
    output: Option<WorkerOutput>,
) -> Result<()> {
    sqlx::query(
        "UPDATE intelligence_jobs \
         SET status = 'done', completed_at = NOW() \
         WHERE id = $1",
    )
    .bind(&job.id)
    .execute(pool)
    .await?;

    if let Some(out) = output {
        let output_id = uuid::Uuid::new_v4().to_string();
        let structured = out
            .structured_data
            .map(|v| v)
            .unwrap_or(serde_json::Value::Null);

        sqlx::query(
            r#"INSERT INTO intelligence_outputs
               (id, job_id, job_type, rig_id, well_id, formation_name,
                output_type, content, structured_data, confidence)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"#,
        )
        .bind(&output_id)
        .bind(&job.id)
        .bind(&job.job_type)
        .bind(&out.rig_id)
        .bind(&out.well_id)
        .bind(&out.formation_name)
        .bind(&out.output_type)
        .bind(&out.content)
        .bind(&structured)
        .bind(out.confidence)
        .execute(pool)
        .await?;
    }

    Ok(())
}

/// Mark a job as failed. Increments `retry_count`; if retries are exhausted
/// the job transitions to `'failed'` permanently.
pub async fn fail_job(pool: &PgPool, job: &IntelligenceJob, error: &str) -> Result<()> {
    let next_retry = job.retry_count + 1;
    let exhausted = next_retry >= job.max_retries;

    let new_status = if exhausted { "failed" } else { "pending" };

    if exhausted {
        warn!(
            job_id = %job.id,
            job_type = %job.job_type,
            retries = next_retry,
            "Job permanently failed after {} attempts",
            next_retry
        );
    }

    sqlx::query(
        "UPDATE intelligence_jobs \
         SET status        = $1, \
             retry_count   = $2, \
             error_message = $3, \
             completed_at  = CASE WHEN $4 THEN NOW() ELSE NULL END, \
             claimed_at    = NULL \
         WHERE id = $5",
    )
    .bind(new_status)
    .bind(next_retry)
    .bind(error)
    .bind(exhausted)
    .bind(&job.id)
    .execute(pool)
    .await?;

    Ok(())
}

/// Count how many jobs are currently pending (useful for metrics/logging).
pub async fn pending_job_count(pool: &PgPool) -> Result<i64, sqlx::Error> {
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM intelligence_jobs WHERE status = 'pending'")
            .fetch_one(pool)
            .await?;
    Ok(count.0)
}
