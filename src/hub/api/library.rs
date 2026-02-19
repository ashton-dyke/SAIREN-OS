//! Library sync and stats endpoints

use crate::fleet::types::FleetEpisode;
use crate::hub::HubState;
use crate::hub::auth::api_key::{ErrorResponse, RigAuth};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use std::sync::Arc;
use tracing::info;

#[derive(Serialize)]
pub struct LibraryResponse {
    pub version: i64,
    pub episodes: Vec<FleetEpisode>,
    pub total_fleet_episodes: i64,
    pub pruned_ids: Vec<String>,
}

#[derive(Serialize)]
pub struct LibraryStats {
    pub total: i64,
    pub category_breakdown: Vec<CategoryCount>,
    pub outcome_breakdown: Vec<OutcomeCount>,
}

#[derive(Serialize)]
pub struct CategoryCount {
    pub category: String,
    pub count: i64,
}

#[derive(Serialize)]
pub struct OutcomeCount {
    pub outcome: String,
    pub count: i64,
}

/// GET /api/fleet/library — Sync library (delta or full)
pub async fn get_library(
    State(hub): State<Arc<HubState>>,
    auth: RigAuth,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    // Parse If-Modified-Since (unix timestamp)
    let since = headers
        .get("if-modified-since")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<i64>().ok())
        .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0));

    let (episodes, pruned_ids, version, total) =
        get_episodes_since(&hub.db, since, &auth.rig_id)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("Database error: {}", e),
                    }),
                )
            })?;

    // If no changes, return 304
    if episodes.is_empty() && pruned_ids.is_empty() {
        return Ok(StatusCode::NOT_MODIFIED.into_response());
    }

    let response = LibraryResponse {
        version,
        episodes: episodes.clone(),
        total_fleet_episodes: total,
        pruned_ids,
    };

    // Log sync event
    let _ = sqlx::query(
        "INSERT INTO sync_log (rig_id, episodes_sent, library_version) VALUES ($1, $2, $3)",
    )
    .bind(&auth.rig_id)
    .bind(episodes.len() as i32)
    .bind(version as i32)
    .execute(&hub.db)
    .await;

    let _ = sqlx::query("UPDATE rigs SET last_sync = NOW() WHERE rig_id = $1")
        .bind(&auth.rig_id)
        .execute(&hub.db)
        .await;

    info!(
        rig_id = %auth.rig_id,
        episodes = episodes.len(),
        version = version,
        "Library sync served"
    );

    // Check if client accepts zstd
    let accepts_zstd = headers
        .get("accept-encoding")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("zstd"))
        .unwrap_or(false);

    let json = serde_json::to_vec(&response).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Serialization error: {}", e),
            }),
        )
    })?;

    if accepts_zstd {
        let compressed = zstd::encode_all(json.as_slice(), 3).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Compression error: {}", e),
                }),
            )
        })?;

        Ok(Response::builder()
            .header("Content-Type", "application/json")
            .header("Content-Encoding", "zstd")
            .header("X-Library-Version", version.to_string())
            .header("X-Total-Episodes", total.to_string())
            .header("X-Delta-Count", episodes.len().to_string())
            .body(axum::body::Body::from(compressed))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()))
    } else {
        Ok(Response::builder()
            .header("Content-Type", "application/json")
            .header("X-Library-Version", version.to_string())
            .header("X-Total-Episodes", total.to_string())
            .body(axum::body::Body::from(json))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()))
    }
}

/// Query episodes since a given timestamp with delta support
async fn get_episodes_since(
    pool: &sqlx::PgPool,
    since: Option<chrono::DateTime<chrono::Utc>>,
    requesting_rig_id: &str,
) -> Result<(Vec<FleetEpisode>, Vec<String>, i64, i64), sqlx::Error> {
    // Get episodes — for delta, only updated since last sync; exclude requesting rig's own
    let episode_rows: Vec<(serde_json::Value,)> = if let Some(since_ts) = since {
        sqlx::query_as(
            r#"SELECT row_to_json(e) FROM (
                SELECT id, rig_id, category, campaign, depth_min, depth_max,
                       risk_level, severity, outcome, resolution, score,
                       key_metrics, timestamp
                FROM episodes
                WHERE updated_at > $1
                  AND archived = FALSE
                  AND rig_id != $2
                ORDER BY score DESC
            ) e"#,
        )
        .bind(since_ts)
        .bind(requesting_rig_id)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as(
            r#"SELECT row_to_json(e) FROM (
                SELECT id, rig_id, category, campaign, depth_min, depth_max,
                       risk_level, severity, outcome, resolution, score,
                       key_metrics, timestamp
                FROM episodes
                WHERE archived = FALSE
                  AND rig_id != $1
                ORDER BY score DESC
            ) e"#,
        )
        .bind(requesting_rig_id)
        .fetch_all(pool)
        .await?
    };

    // Convert DB rows to FleetEpisode (via JSON for flexibility)
    let episodes: Vec<FleetEpisode> = episode_rows
        .into_iter()
        .filter_map(|(json,)| db_row_to_episode(&json))
        .collect();

    // Get pruned IDs (archived since last sync)
    let pruned_ids: Vec<String> = if let Some(since_ts) = since {
        sqlx::query_scalar("SELECT id FROM episodes WHERE archived = TRUE AND updated_at > $1")
            .bind(since_ts)
            .fetch_all(pool)
            .await?
    } else {
        vec![]
    };

    // Get current library version
    let version: i64 = sqlx::query_scalar("SELECT last_value FROM library_version_seq")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    // Get total active count
    let total: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM episodes WHERE archived = FALSE")
            .fetch_one(pool)
            .await
            .unwrap_or(0);

    Ok((episodes, pruned_ids, version, total))
}

/// Convert a database JSON row back to a FleetEpisode
fn db_row_to_episode(json: &serde_json::Value) -> Option<FleetEpisode> {
    // The DB stores episodes in a normalized form; reconstruct the FleetEpisode
    use crate::fleet::types::{EpisodeMetrics, EventOutcome};
    use crate::types::{AnomalyCategory, Campaign, FinalSeverity, RiskLevel};

    let id = json.get("id")?.as_str()?.to_string();
    let rig_id = json.get("rig_id")?.as_str()?.to_string();

    let category = match json.get("category")?.as_str()? {
        "DrillingEfficiency" | "Drilling Efficiency" => AnomalyCategory::DrillingEfficiency,
        "Hydraulics" => AnomalyCategory::Hydraulics,
        "WellControl" | "Well Control" => AnomalyCategory::WellControl,
        "Mechanical" => AnomalyCategory::Mechanical,
        "Formation" => AnomalyCategory::Formation,
        _ => AnomalyCategory::None,
    };

    let campaign = match json.get("campaign")?.as_str()? {
        "Production" => Campaign::Production,
        "PlugAbandonment" | "Plug & Abandonment" => Campaign::PlugAbandonment,
        _ => Campaign::Production,
    };

    let depth_min = json.get("depth_min")?.as_f64().unwrap_or(0.0);
    let depth_max = json.get("depth_max")?.as_f64().unwrap_or(depth_min);

    let risk_level = match json.get("risk_level")?.as_str()? {
        "LOW" | "Low" => RiskLevel::Low,
        "ELEVATED" | "Elevated" => RiskLevel::Elevated,
        "HIGH" | "High" => RiskLevel::High,
        "CRITICAL" | "Critical" => RiskLevel::Critical,
        _ => RiskLevel::Low,
    };

    let severity = match json.get("severity")?.as_str()? {
        "Low" => FinalSeverity::Low,
        "Medium" => FinalSeverity::Medium,
        "High" => FinalSeverity::High,
        "Critical" => FinalSeverity::Critical,
        _ => FinalSeverity::Medium,
    };

    let outcome_str = json.get("outcome")?.as_str().unwrap_or("Pending");
    let resolution = json
        .get("resolution")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let outcome = if outcome_str.starts_with("RESOLVED") || outcome_str == "Resolved" {
        EventOutcome::Resolved {
            action_taken: resolution.clone(),
        }
    } else if outcome_str.starts_with("ESCALATED") || outcome_str == "Escalated" {
        EventOutcome::Escalated {
            reason: resolution.clone(),
        }
    } else if outcome_str == "FALSE_POSITIVE" || outcome_str == "FalsePositive" {
        EventOutcome::FalsePositive
    } else {
        EventOutcome::Pending
    };

    let key_metrics_json = json.get("key_metrics")?;
    let key_metrics = EpisodeMetrics {
        mse_efficiency: key_metrics_json
            .get("mse_efficiency")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
        flow_balance: key_metrics_json
            .get("flow_balance")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
        d_exponent: key_metrics_json
            .get("d_exponent")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
        torque_delta_percent: key_metrics_json
            .get("torque_delta_percent")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
        ecd_margin: key_metrics_json
            .get("ecd_margin")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
        rop: key_metrics_json
            .get("rop")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
    };

    let timestamp = json.get("timestamp")?.as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp() as u64)
        .or_else(|| json.get("timestamp")?.as_u64())
        .unwrap_or(0);

    Some(FleetEpisode {
        id,
        rig_id,
        category,
        campaign,
        depth_range: (depth_min, depth_max),
        risk_level,
        severity,
        resolution_summary: resolution,
        outcome,
        timestamp,
        key_metrics,
    })
}

/// GET /api/fleet/library/stats
pub async fn get_library_stats(
    State(hub): State<Arc<HubState>>,
    _auth: RigAuth,
) -> Json<LibraryStats> {
    let total: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM episodes WHERE archived = FALSE")
            .fetch_one(&hub.db)
            .await
            .unwrap_or(0);

    let cat_rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT category, COUNT(*) FROM episodes WHERE archived = FALSE GROUP BY category",
    )
    .fetch_all(&hub.db)
    .await
    .unwrap_or_default();

    let outcome_rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT outcome, COUNT(*) FROM episodes WHERE archived = FALSE GROUP BY outcome",
    )
    .fetch_all(&hub.db)
    .await
    .unwrap_or_default();

    Json(LibraryStats {
        total,
        category_breakdown: cat_rows
            .into_iter()
            .map(|(category, count)| CategoryCount { category, count })
            .collect(),
        outcome_breakdown: outcome_rows
            .into_iter()
            .map(|(outcome, count)| OutcomeCount { outcome, count })
            .collect(),
    })
}
