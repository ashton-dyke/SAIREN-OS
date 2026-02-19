//! Episode deduplication logic
//!
//! Deduplicates episodes based on:
//! - Same rig
//! - Same category
//! - Depth within 100 ft
//! - Timestamps within 10 minutes

use crate::fleet::types::FleetEpisode;
use sqlx::PgPool;

/// Find a duplicate episode in the library
///
/// Returns the ID of the existing episode if a duplicate is found.
pub async fn find_duplicate(
    pool: &PgPool,
    episode: &FleetEpisode,
) -> Result<Option<String>, sqlx::Error> {
    let category_str = format!("{}", episode.category);
    let ts = chrono::DateTime::from_timestamp(episode.timestamp as i64, 0)
        .unwrap_or_else(chrono::Utc::now);

    let result: Option<(String,)> = sqlx::query_as(
        r#"SELECT id FROM episodes
           WHERE rig_id = $1
             AND category = $2
             AND ABS(depth_min - $3) < 100.0
             AND ABS(EXTRACT(EPOCH FROM (timestamp - $4::timestamptz))) < 600
             AND archived = FALSE
           LIMIT 1"#,
    )
    .bind(&episode.rig_id)
    .bind(&category_str)
    .bind(episode.depth_range.0)
    .bind(ts)
    .fetch_optional(pool)
    .await?;

    Ok(result.map(|(id,)| id))
}
