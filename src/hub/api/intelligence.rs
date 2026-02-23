//! Intelligence distribution endpoint
//!
//! Serves `intelligence_outputs` to rigs on request.  Uses a cursor-based
//! pull model so every rig independently tracks what it has already seen:
//!
//! 1. Rig sends `GET /api/fleet/intelligence?since=<unix_ts>` (or no `since`
//!    on first pull to get everything).
//! 2. Hub returns outputs where `(rig_id IS NULL OR rig_id = $caller)` AND
//!    `created_at > since_ts`, ordered oldest-first (max 50 per request).
//! 3. Rig stores the `synced_at` timestamp from the response and uses it as
//!    `since` on the next pull.
//!
//! ## Output relevance rules
//!
//! | Worker           | `rig_id` in DB | Visible to            |
//! |------------------|----------------|-----------------------|
//! | formation_benchmark | NULL        | All rigs (fleet-wide) |
//! | anomaly_fingerprint | NULL        | All rigs (fleet-wide) |
//! | post_well_report    | rig_id      | Only that rig         |
//! | benchmark_gap       | rig_id      | Only that rig         |

use crate::hub::HubState;
use crate::hub::auth::api_key::{ErrorResponse, RigAuth};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn};

// ─── Request / response types ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct IntelligenceQuery {
    /// Unix timestamp cursor — only return outputs created after this time.
    /// Omit (or send 0) to fetch everything.
    pub since: Option<i64>,
    /// Optional formation filter — restrict to outputs for this formation.
    pub formation: Option<String>,
}

/// A single intelligence output delivered to a rig.
#[derive(Serialize)]
pub struct IntelligenceItem {
    pub id: String,
    pub job_type: String,
    pub output_type: String,
    pub content: String,
    pub formation_name: Option<String>,
    pub rig_id: Option<String>,
    pub well_id: Option<String>,
    pub confidence: Option<f64>,
    /// Unix timestamp (seconds)
    pub created_at: i64,
}

#[derive(Serialize)]
pub struct IntelligenceSyncResponse {
    pub outputs: Vec<IntelligenceItem>,
    /// Unix timestamp to use as `since` on the next pull
    pub synced_at: i64,
    pub total: usize,
}

// ─── Handler ─────────────────────────────────────────────────────────────────

/// GET /api/fleet/intelligence
///
/// Returns intelligence outputs relevant to the calling rig.
pub async fn get_intelligence(
    State(hub): State<Arc<HubState>>,
    auth: RigAuth,
    Query(params): Query<IntelligenceQuery>,
) -> Result<Json<IntelligenceSyncResponse>, (StatusCode, Json<ErrorResponse>)> {
    let since_ts = params
        .since
        .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
        .unwrap_or_else(|| chrono::DateTime::from_timestamp(0, 0).expect("epoch"));

    // Build query: fleet-wide outputs (rig_id IS NULL) + rig-specific outputs
    let rows: Vec<(String, String, String, String, Option<String>, Option<String>, Option<String>, Option<f64>, chrono::DateTime<chrono::Utc>)> =
        if let Some(ref formation) = params.formation {
            sqlx::query_as(
                r#"SELECT id, job_type, output_type, content,
                          formation_name, rig_id, well_id, confidence, created_at
                   FROM intelligence_outputs
                   WHERE (rig_id IS NULL OR rig_id = $1)
                     AND created_at > $2
                     AND (formation_name = $3 OR formation_name IS NULL)
                   ORDER BY created_at ASC
                   LIMIT 50"#,
            )
            .bind(&auth.rig_id)
            .bind(since_ts)
            .bind(formation)
            .fetch_all(&hub.db)
            .await
        } else {
            sqlx::query_as(
                r#"SELECT id, job_type, output_type, content,
                          formation_name, rig_id, well_id, confidence, created_at
                   FROM intelligence_outputs
                   WHERE (rig_id IS NULL OR rig_id = $1)
                     AND created_at > $2
                   ORDER BY created_at ASC
                   LIMIT 50"#,
            )
            .bind(&auth.rig_id)
            .bind(since_ts)
            .fetch_all(&hub.db)
            .await
        }
        .map_err(|e| {
            warn!(error = %e, "Failed to query intelligence outputs");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: "Database error".to_string() }),
            )
        })?;

    let total = rows.len();
    let output_ids: Vec<String> = rows.iter().map(|(id, ..)| id.clone()).collect();

    let outputs: Vec<IntelligenceItem> = rows
        .into_iter()
        .map(|(id, job_type, output_type, content, formation_name, rig_id, well_id, confidence, created_at)| {
            IntelligenceItem {
                id,
                job_type,
                output_type,
                content,
                formation_name,
                rig_id,
                well_id,
                confidence,
                created_at: created_at.timestamp(),
            }
        })
        .collect();

    // Mark fleet-wide outputs as distributed (monitoring only — correctness is via cursor)
    if !output_ids.is_empty() {
        let _ = sqlx::query(
            "UPDATE intelligence_outputs \
             SET distributed = TRUE, distributed_at = NOW() \
             WHERE id = ANY($1) AND rig_id IS NULL AND distributed = FALSE",
        )
        .bind(&output_ids)
        .execute(&hub.db)
        .await;
    }

    let synced_at = chrono::Utc::now().timestamp();

    if total > 0 {
        info!(
            rig_id = %auth.rig_id,
            outputs = total,
            "Intelligence outputs served"
        );
    }

    Ok(Json(IntelligenceSyncResponse {
        outputs,
        synced_at,
        total,
    }))
}
