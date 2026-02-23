//! Library Curator — background processing pipeline
//!
//! Transforms raw FleetEvent records into scored, deduplicated
//! FleetEpisode entries in the episodes table.

pub mod scoring;
pub mod dedup;
pub mod pruning;

use crate::fleet::types::FleetEpisode;
use crate::hub::config::HubConfig;
use scoring::score_episode;
use dedup::find_duplicate;
use pruning::prune_old_episodes;
use sqlx::PgPool;
use std::time::Duration;
use tracing::{error, info, warn};

/// Run the curator as a background task
pub async fn run_curator(pool: PgPool, config: HubConfig) {
    let mut interval =
        tokio::time::interval(Duration::from_secs(config.curation_interval_secs));

    loop {
        interval.tick().await;

        match curate_pending_events(&pool).await {
            Ok(count) => {
                if count > 0 {
                    info!(curated = count, "Curation cycle complete");
                    // Rebuild knowledge graph in background — non-blocking
                    let pool_clone = pool.clone();
                    tokio::spawn(async move {
                        if let Err(e) =
                            crate::hub::knowledge_graph::builder::rebuild_graph(&pool_clone).await
                        {
                            warn!(error = %e, "Knowledge graph rebuild failed after curation");
                        }
                    });
                }
            }
            Err(e) => {
                error!(error = %e, "Curation failed");
            }
        }

        match prune_old_episodes(&pool, &config).await {
            Ok(pruned) => {
                if pruned > 0 {
                    info!(pruned = pruned, "Pruning cycle complete");
                }
            }
            Err(e) => {
                error!(error = %e, "Pruning failed");
            }
        }

    }
}

/// Curate all pending events into episodes
async fn curate_pending_events(pool: &PgPool) -> Result<u32, sqlx::Error> {
    // Fetch events needing curation
    let rows: Vec<(String, serde_json::Value)> = sqlx::query_as(
        "SELECT id, payload FROM events WHERE needs_curation = TRUE ORDER BY timestamp LIMIT 100",
    )
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(0);
    }

    let mut curated = 0u32;

    for (event_id, payload) in &rows {
        let event: crate::fleet::types::FleetEvent = match serde_json::from_value(payload.clone())
        {
            Ok(e) => e,
            Err(e) => {
                warn!(event_id = %event_id, error = %e, "Failed to deserialize event for curation");
                // Mark as curated to avoid re-processing
                sqlx::query("UPDATE events SET needs_curation = FALSE WHERE id = $1")
                    .bind(event_id)
                    .execute(pool)
                    .await?;
                continue;
            }
        };

        // Convert to episode
        let episode = FleetEpisode::from_event(&event);
        let score = score_episode(&episode, pool).await;

        // Check for duplicate
        let duplicate_id = find_duplicate(pool, &episode).await?;

        if let Some(dup_id) = duplicate_id {
            // Update existing episode if new one has better outcome
            let existing_outcome: Option<(String,)> =
                sqlx::query_as("SELECT outcome FROM episodes WHERE id = $1")
                    .bind(&dup_id)
                    .fetch_optional(pool)
                    .await?;

            let should_update = match (&episode.outcome, existing_outcome) {
                (crate::fleet::types::EventOutcome::Resolved { .. }, _) => true,
                (crate::fleet::types::EventOutcome::Escalated { .. }, Some((ref o,))) => {
                    o == "PENDING" || o == "Pending"
                }
                _ => false,
            };

            if should_update {
                sqlx::query(
                    "UPDATE episodes SET outcome = $1, resolution = $2, score = $3 WHERE id = $4",
                )
                .bind(format!("{}", episode.outcome))
                .bind(&episode.resolution_summary)
                .bind(score)
                .bind(&dup_id)
                .execute(pool)
                .await?;
            }
        } else {
            // Insert new episode
            let ts = chrono::DateTime::from_timestamp(episode.timestamp as i64, 0)
                .unwrap_or_else(chrono::Utc::now);
            let key_metrics = serde_json::to_value(&episode.key_metrics)
                .unwrap_or(serde_json::Value::Null);

            sqlx::query(
                r#"INSERT INTO episodes (id, source_event_id, rig_id, category, campaign,
                    depth_min, depth_max, risk_level, severity, outcome, resolution,
                    score, key_metrics, timestamp)
                   VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
                   ON CONFLICT (id) DO UPDATE SET
                    outcome = EXCLUDED.outcome, resolution = EXCLUDED.resolution,
                    score = EXCLUDED.score"#,
            )
            .bind(&episode.id)
            .bind(event_id)
            .bind(&episode.rig_id)
            .bind(format!("{}", episode.category))
            .bind(format!("{:?}", episode.campaign))
            .bind(episode.depth_range.0)
            .bind(episode.depth_range.1)
            .bind(format!("{}", episode.risk_level))
            .bind(format!("{}", episode.severity))
            .bind(format!("{}", episode.outcome))
            .bind(&episode.resolution_summary)
            .bind(score)
            .bind(&key_metrics)
            .bind(ts)
            .execute(pool)
            .await?;
        }

        // Mark event as curated
        sqlx::query("UPDATE events SET needs_curation = FALSE WHERE id = $1")
            .bind(event_id)
            .execute(pool)
            .await?;

        curated += 1;
    }

    // Increment library version
    if curated > 0 {
        sqlx::query("SELECT nextval('library_version_seq')")
            .execute(pool)
            .await?;
    }

    Ok(curated)
}
