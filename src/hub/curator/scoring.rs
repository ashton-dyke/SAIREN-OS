//! Episode scoring algorithm
//!
//! Scores episodes based on:
//! - Outcome quality (50%)
//! - Recency (25%)
//! - Detail completeness (15%)
//! - Category diversity (10%)

use crate::fleet::types::{EventOutcome, FleetEpisode};
use sqlx::PgPool;

/// Score an episode for library ranking
pub async fn score_episode(episode: &FleetEpisode, pool: &PgPool) -> f64 {
    let outcome_weight = match &episode.outcome {
        EventOutcome::Resolved { .. } => 1.0,
        EventOutcome::Escalated { .. } => 0.7,
        EventOutcome::Pending => 0.2,
        EventOutcome::FalsePositive => 0.1,
    };

    let now_secs = chrono::Utc::now().timestamp() as u64;
    let age_days = (now_secs.saturating_sub(episode.timestamp)) as f64 / 86400.0;
    let recency_weight = (-age_days / 180.0_f64).exp();

    let detail_weight = {
        let has_notes = if !episode.resolution_summary.is_empty()
            && episode.resolution_summary != "Pending resolution"
        {
            0.3
        } else {
            0.0
        };
        let has_action = match &episode.outcome {
            EventOutcome::Resolved { action_taken } if !action_taken.is_empty() => 0.4,
            _ => 0.0,
        };
        let has_metrics = 0.3; // Always true for FleetEpisode
        has_notes + has_action + has_metrics
    };

    let diversity_weight = compute_diversity(pool, episode).await.unwrap_or(0.5);

    outcome_weight * 0.50 + recency_weight * 0.25 + detail_weight * 0.15 + diversity_weight * 0.10
}

/// Compute diversity score — underrepresented categories get higher scores
async fn compute_diversity(pool: &PgPool, episode: &FleetEpisode) -> Result<f64, sqlx::Error> {
    let category_str = format!("{}", episode.category);

    let category_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM episodes WHERE category = $1 AND archived = FALSE")
            .bind(&category_str)
            .fetch_one(pool)
            .await?;

    let total_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM episodes WHERE archived = FALSE")
            .fetch_one(pool)
            .await?;

    if total_count == 0 {
        return Ok(1.0);
    }

    let category_fraction = category_count as f64 / total_count as f64;
    Ok(1.0 - category_fraction)
}
