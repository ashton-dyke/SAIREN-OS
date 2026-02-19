//! Episode pruning and archival
//!
//! Rules:
//! 1. Episodes older than 12 months → archive
//! 2. FalsePositive + age > 3 months → archive
//! 3. Pending + age > 30 days → downgrade score to 0.05
//! 4. Total episodes > 50,000 → prune lowest-scored

use crate::hub::config::HubConfig;
use sqlx::PgPool;
use tracing::debug;

/// Prune old episodes according to retention rules
pub async fn prune_old_episodes(pool: &PgPool, config: &HubConfig) -> Result<u32, sqlx::Error> {
    let mut pruned = 0u32;

    // Rule 1: Episodes older than max_age → archive
    let r1 = sqlx::query(
        "UPDATE episodes SET archived = TRUE WHERE timestamp < NOW() - INTERVAL '12 months' AND archived = FALSE",
    )
    .execute(pool)
    .await?;
    pruned += r1.rows_affected() as u32;
    if r1.rows_affected() > 0 {
        debug!(count = r1.rows_affected(), "Archived episodes older than 12 months");
    }

    // Rule 2: FalsePositive + age > 3 months → archive
    let r2 = sqlx::query(
        "UPDATE episodes SET archived = TRUE WHERE outcome = 'FALSE_POSITIVE' AND timestamp < NOW() - INTERVAL '3 months' AND archived = FALSE",
    )
    .execute(pool)
    .await?;
    pruned += r2.rows_affected() as u32;

    // Rule 3: Pending + age > 30 days → downgrade score
    sqlx::query(
        "UPDATE episodes SET score = 0.05 WHERE outcome = 'PENDING' AND timestamp < NOW() - INTERVAL '30 days' AND score > 0.05",
    )
    .execute(pool)
    .await?;

    // Rule 4: Total episodes > max → prune lowest-scored
    let total: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM episodes WHERE archived = FALSE")
            .fetch_one(pool)
            .await?;

    if total > config.library_max_episodes {
        let to_prune = total - config.library_max_episodes;
        let r4 = sqlx::query(
            "UPDATE episodes SET archived = TRUE WHERE id IN (
                SELECT id FROM episodes WHERE archived = FALSE ORDER BY score ASC LIMIT $1
            )",
        )
        .bind(to_prune)
        .execute(pool)
        .await?;
        pruned += r4.rows_affected() as u32;
        if r4.rows_affected() > 0 {
            debug!(count = r4.rows_affected(), "Pruned lowest-scored episodes");
        }
    }

    Ok(pruned)
}
