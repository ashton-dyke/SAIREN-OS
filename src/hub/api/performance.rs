//! Performance data endpoints for sharing post-well offset data across the fleet

use crate::hub::HubState;
use crate::hub::auth::api_key::{ErrorResponse, RigAuth};
use crate::knowledge_base::fleet_bridge::{FleetPerformanceResponse, FleetPerformanceUpload};
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Deserialize;
use std::sync::Arc;
use tracing::{info, warn};

#[derive(Deserialize)]
pub struct PerformanceQuery {
    pub field: String,
    pub since: Option<i64>,
    pub exclude_rig: Option<String>,
}

/// Decompress body if Content-Encoding: zstd
fn decompress_body(headers: &HeaderMap, body: Bytes) -> Result<Bytes, StatusCode> {
    let is_zstd = headers
        .get("content-encoding")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("zstd"))
        .unwrap_or(false);

    if is_zstd {
        let decompressed = zstd::decode_all(body.as_ref())
            .map_err(|_| StatusCode::BAD_REQUEST)?;
        if decompressed.len() > 10_485_760 {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
        Ok(Bytes::from(decompressed))
    } else {
        Ok(body)
    }
}

/// POST /api/fleet/performance — Upload post-well performance data
pub async fn upload_performance(
    State(hub): State<Arc<HubState>>,
    auth: RigAuth,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    if body.len() > hub.config.max_payload_size {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(ErrorResponse {
                error: "Payload exceeds size limit".to_string(),
            }),
        ));
    }

    let data = decompress_body(&headers, body).map_err(|status| {
        (status, Json(ErrorResponse { error: "Decompression failed".to_string() }))
    })?;

    let upload: FleetPerformanceUpload = serde_json::from_slice(&data).map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: format!("Invalid JSON: {}", e) }))
    })?;

    // Verify rig_id matches auth
    if auth.rig_id != upload.rig_id {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse { error: "API key does not match rig_id".to_string() }),
        ));
    }

    let payload = serde_json::to_value(&upload.performance)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })))?;

    // Upsert into fleet_performance table
    sqlx::query(
        r#"INSERT INTO fleet_performance (rig_id, well_id, field, formation_name, performance)
           VALUES ($1, $2, $3, $4, $5)
           ON CONFLICT (well_id, formation_name)
           DO UPDATE SET performance = $5, rig_id = $1, updated_at = NOW()"#,
    )
    .bind(&upload.rig_id)
    .bind(&upload.well_id)
    .bind(&upload.field)
    .bind(&upload.formation_name)
    .bind(&payload)
    .execute(&hub.db)
    .await
    .map_err(|e| {
        warn!(error = %e, "Failed to store performance data");
        (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Storage error".to_string() }))
    })?;

    info!(
        rig = &upload.rig_id,
        well = &upload.well_id,
        formation = &upload.formation_name,
        "Performance data ingested"
    );

    Ok(StatusCode::CREATED)
}

/// GET /api/fleet/performance — Query performance data for a field
pub async fn get_performance(
    State(hub): State<Arc<HubState>>,
    _auth: RigAuth,
    Query(params): Query<PerformanceQuery>,
) -> Result<Json<FleetPerformanceResponse>, (StatusCode, Json<ErrorResponse>)> {
    let since_ts = params.since
        .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
        .unwrap_or_else(|| chrono::DateTime::from_timestamp(0, 0).expect("epoch"));

    let exclude_rig = params.exclude_rig.unwrap_or_default();

    let rows: Vec<(String, String, String, String, serde_json::Value)> = sqlx::query_as(
        r#"SELECT rig_id, well_id, field, formation_name, performance
           FROM fleet_performance
           WHERE field = $1 AND updated_at >= $2 AND ($3 = '' OR rig_id != $3)
           ORDER BY updated_at DESC"#,
    )
    .bind(&params.field)
    .bind(since_ts)
    .bind(&exclude_rig)
    .fetch_all(&hub.db)
    .await
    .map_err(|e| {
        warn!(error = %e, "Failed to query performance data");
        (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Query error".to_string() }))
    })?;

    let mut records = Vec::with_capacity(rows.len());
    for (rig_id, well_id, field, formation_name, perf_json) in &rows {
        match serde_json::from_value(perf_json.clone()) {
            Ok(performance) => {
                records.push(FleetPerformanceUpload {
                    rig_id: rig_id.clone(),
                    well_id: well_id.clone(),
                    field: field.clone(),
                    formation_name: formation_name.clone(),
                    performance,
                });
            }
            Err(e) => {
                warn!(well = well_id, formation = formation_name, error = %e, "Failed to deserialize performance record");
            }
        }
    }

    let total = records.len();
    Ok(Json(FleetPerformanceResponse { records, total }))
}
