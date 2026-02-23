//! Hub Intelligence Scheduler
//!
//! Runs embedded mistralrs LLM inference against accumulated fleet data on a
//! background timer. Jobs are stored in `intelligence_jobs` (PostgreSQL) and
//! claimed with `SELECT FOR UPDATE SKIP LOCKED` so the system is safe for
//! multiple concurrent hub instances.
//!
//! ## Design principles
//!
//! - **Never blocks rig writes.** The scheduler runs as a separate Tokio task.
//!   Rig report uploads and library sync are unaffected by LLM inference time.
//!
//! - **One job per tick.** A single job is claimed and processed per scheduler
//!   cycle. This keeps memory usage predictable and allows graceful shutdown
//!   between jobs.
//!
//! - **Retry on failure.** Failed jobs are retried up to `max_retries` (default 3)
//!   with automatic back-off via re-queuing at the same priority. Permanently
//!   failed jobs are kept for audit.
//!
//! ## Usage (fleet_hub binary)
//!
//! ```rust,ignore
//! tokio::spawn(hub::intelligence::run_intelligence_scheduler(
//!     pool.clone(),
//!     backend,
//!     config.intelligence_interval_secs,
//! ));
//! ```

pub mod job_queue;
pub mod workers;

pub use job_queue::{
    IntelligenceJob, WorkerOutput,
    enqueue_job, pending_job_count,
    job_type,
};

use crate::llm::MistralRsBackend;
use job_queue::{claim_job, complete_job, fail_job};
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};

// ─── Scheduler ────────────────────────────────────────────────────────────────

/// Run the intelligence scheduler as a long-lived background task.
///
/// Polls the job queue every `interval_secs` seconds, claims one job, runs it
/// through the embedded LLM, and stores the output. Loops forever until the
/// Tokio runtime is shut down.
pub async fn run_intelligence_scheduler(
    pool: PgPool,
    backend: Arc<MistralRsBackend>,
    interval_secs: u64,
) {
    info!(
        interval_secs = interval_secs,
        "Intelligence scheduler started"
    );

    let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
    // Don't try to "catch up" missed ticks — if inference is slow just skip.
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        interval.tick().await;

        match run_one_job(&pool, &backend).await {
            Ok(Some(job_type)) => {
                info!(job_type = %job_type, "Intelligence job completed successfully");
            }
            Ok(None) => {
                debug!("No pending intelligence jobs");
            }
            Err(e) => {
                error!(error = %e, "Intelligence job processing error");
            }
        }
    }
}

/// Claim and process one job. Returns the job type on success, None if queue empty.
async fn run_one_job(pool: &PgPool, backend: &Arc<MistralRsBackend>) -> anyhow::Result<Option<String>> {
    let job = match claim_job(pool).await? {
        Some(j) => j,
        None => return Ok(None),
    };

    let job_type = job.job_type.clone();
    let job_id = job.id.clone();

    info!(
        job_id = %job_id,
        job_type = %job_type,
        retry = job.retry_count,
        "Claimed intelligence job"
    );

    match workers::dispatch_job(backend, pool, &job).await {
        Ok(output) => {
            complete_job(pool, &job, Some(output)).await?;
        }
        Err(e) => {
            warn!(
                job_id = %job_id,
                job_type = %job_type,
                error = %e,
                "Intelligence job failed"
            );
            fail_job(pool, &job, &e.to_string()).await?;
            return Err(e);
        }
    }

    Ok(Some(job_type))
}
