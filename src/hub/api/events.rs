//! Event ingestion and retrieval handlers

use crate::fleet::types::{FleetEvent, should_upload};
use crate::hub::HubState;
use crate::hub::auth::api_key::{ErrorResponse, RigAuth};
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn};

#[derive(Serialize)]
pub struct UploadEventResponse {
    pub id: String,
    pub status: String,
}

#[derive(Deserialize)]
pub struct UpdateOutcomeRequest {
    pub outcome: String,
    pub action_taken: Option<String>,
    pub notes: Option<String>,
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
        // Protect against zip bombs
        if decompressed.len() > 10_485_760 {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
        Ok(Bytes::from(decompressed))
    } else {
        Ok(body)
    }
}

/// Validate a FleetEvent
fn validate_event(event: &FleetEvent) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();

    // Event ID must start with rig_id
    let expected_prefix = format!("{}-", event.rig_id);
    if !event.id.starts_with(&expected_prefix) {
        errors.push("Event ID must start with rig_id".to_string());
    }

    // Risk level must be Elevated, High, or Critical
    if !should_upload(&event.advisory) {
        errors.push("Risk level must be Elevated, High, or Critical".to_string());
    }

    // Timestamp within reasonable range
    let now = chrono::Utc::now().timestamp() as u64;
    let seven_days = 7 * 24 * 3600;
    if event.timestamp > now + 300 {
        errors.push("Timestamp is in the future".to_string());
    }
    if event.timestamp + seven_days < now {
        errors.push("Timestamp is more than 7 days old".to_string());
    }

    // History window must have at least 1 snapshot
    if event.history_window.is_empty() {
        errors.push("History window must have at least 1 snapshot".to_string());
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Check if an event already exists
async fn event_exists(pool: &sqlx::PgPool, event_id: &str) -> Result<bool, sqlx::Error> {
    let result: (bool,) =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM events WHERE id = $1)")
            .bind(event_id)
            .fetch_one(pool)
            .await?;
    Ok(result.0)
}

/// Store a FleetEvent in the database
async fn store_event(pool: &sqlx::PgPool, event: &FleetEvent) -> Result<(), sqlx::Error> {
    let payload =
        serde_json::to_value(event).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;

    let ts = chrono::DateTime::from_timestamp(event.timestamp as i64, 0)
        .unwrap_or_else(chrono::Utc::now);

    let category = event
        .advisory
        .votes
        .first()
        .map(|v| v.specialist.clone())
        .unwrap_or_default();

    sqlx::query(
        r#"INSERT INTO events (id, rig_id, well_id, field, campaign, risk_level, category,
            depth, timestamp, outcome, payload, needs_curation)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, TRUE)"#,
    )
    .bind(&event.id)
    .bind(&event.rig_id)
    .bind(&event.well_id)
    .bind(&event.field)
    .bind(format!("{:?}", event.campaign))
    .bind(format!("{}", event.advisory.risk_level))
    .bind(&category)
    .bind(event.depth)
    .bind(ts)
    .bind(format!("{}", event.outcome))
    .bind(&payload)
    .execute(pool)
    .await?;

    // Update rig last_seen and event_count
    sqlx::query("UPDATE rigs SET last_seen = NOW(), event_count = event_count + 1 WHERE rig_id = $1")
        .bind(&event.rig_id)
        .execute(pool)
        .await?;

    Ok(())
}

/// POST /api/fleet/events — Upload an event
pub async fn upload_event(
    State(hub): State<Arc<HubState>>,
    auth: RigAuth,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<UploadEventResponse>), (StatusCode, Json<ErrorResponse>)> {
    // Check compressed size
    if body.len() > hub.config.max_payload_size {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(ErrorResponse {
                error: "Payload exceeds size limit".to_string(),
            }),
        ));
    }

    // Decompress
    let data = decompress_body(&headers, body).map_err(|status| {
        (
            status,
            Json(ErrorResponse {
                error: "Decompression failed".to_string(),
            }),
        )
    })?;

    // Deserialize
    let event: FleetEvent = serde_json::from_slice(&data).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Invalid JSON: {}", e),
            }),
        )
    })?;

    // Validate
    if let Err(errors) = validate_event(&event) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: errors.join("; "),
            }),
        ));
    }

    // Verify rig_id matches auth
    if auth.rig_id != event.rig_id {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "API key does not match rig_id in event".to_string(),
            }),
        ));
    }

    // Dedup check
    let exists = event_exists(&hub.db, &event.id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Database error: {}", e),
            }),
        )
    })?;

    if exists {
        return Ok((
            StatusCode::CONFLICT,
            Json(UploadEventResponse {
                id: event.id.clone(),
                status: "already_exists".to_string(),
            }),
        ));
    }

    // Store
    store_event(&hub.db, &event).await.map_err(|e| {
        warn!(error = %e, event_id = %event.id, "Failed to store event");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Storage error: {}", e),
            }),
        )
    })?;

    info!(event_id = %event.id, rig_id = %event.rig_id, "Event ingested");

    Ok((
        StatusCode::CREATED,
        Json(UploadEventResponse {
            id: event.id,
            status: "accepted".to_string(),
        }),
    ))
}

/// GET /api/fleet/events/{id} — Retrieve an event by ID
pub async fn get_event(
    State(hub): State<Arc<HubState>>,
    _auth: RigAuth,
    Path(event_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let row: Option<(serde_json::Value,)> =
        sqlx::query_as("SELECT payload FROM events WHERE id = $1")
            .bind(&event_id)
            .fetch_optional(&hub.db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match row {
        Some((payload,)) => Ok(Json(payload)),
        None => Err(StatusCode::NOT_FOUND),
    }
}

/// PATCH /api/fleet/events/{id}/outcome — Update event outcome
pub async fn update_outcome(
    State(hub): State<Arc<HubState>>,
    auth: RigAuth,
    Path(event_id): Path<String>,
    Json(req): Json<UpdateOutcomeRequest>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    // Verify the event belongs to the authenticated rig
    let event_rig: Option<(String,)> =
        sqlx::query_as("SELECT rig_id FROM events WHERE id = $1")
            .bind(&event_id)
            .fetch_optional(&hub.db)
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: "Database error".to_string(),
                    }),
                )
            })?;

    match event_rig {
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Event not found".to_string(),
                }),
            ))
        }
        Some((rig_id,)) if rig_id != auth.rig_id => {
            return Err((
                StatusCode::FORBIDDEN,
                Json(ErrorResponse {
                    error: "Not your event".to_string(),
                }),
            ))
        }
        _ => {}
    }

    // Update event outcome and trigger re-curation
    sqlx::query(
        "UPDATE events SET outcome = $1, action_taken = $2, notes = $3, needs_curation = TRUE WHERE id = $4",
    )
    .bind(&req.outcome)
    .bind(&req.action_taken)
    .bind(&req.notes)
    .bind(&event_id)
    .execute(&hub.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Failed to update outcome".to_string(),
            }),
        )
    })?;

    info!(event_id = %event_id, outcome = %req.outcome, "Outcome updated");
    Ok(StatusCode::OK)
}
