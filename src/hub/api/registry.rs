//! Rig registry handlers — register, list, get, revoke

use crate::hub::HubState;
use crate::hub::auth::api_key::{
    generate_api_key, hash_api_key, AdminAuth, ErrorResponse,
};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

#[derive(Deserialize)]
pub struct RegisterRigRequest {
    pub rig_id: String,
    pub well_id: String,
    pub field: String,
}

#[derive(Serialize)]
pub struct RegisterRigResponse {
    pub rig_id: String,
    pub api_key: String,
}

#[derive(Serialize)]
pub struct RigInfo {
    pub rig_id: String,
    pub well_id: Option<String>,
    pub field: Option<String>,
    pub registered_at: String,
    pub last_seen: Option<String>,
    pub last_sync: Option<String>,
    pub event_count: i32,
    pub status: String,
}

/// POST /api/fleet/rigs/register — Register a new rig (admin only)
pub async fn register_rig(
    State(hub): State<Arc<HubState>>,
    _admin: AdminAuth,
    Json(req): Json<RegisterRigRequest>,
) -> Result<(StatusCode, Json<RegisterRigResponse>), (StatusCode, Json<ErrorResponse>)> {
    let api_key = generate_api_key();
    let key_hash = hash_api_key(&api_key);

    sqlx::query(
        "INSERT INTO rigs (rig_id, api_key_hash, well_id, field) VALUES ($1, $2, $3, $4)",
    )
    .bind(&req.rig_id)
    .bind(&key_hash)
    .bind(&req.well_id)
    .bind(&req.field)
    .execute(&hub.db)
    .await
    .map_err(|e| {
        if e.to_string().contains("duplicate key") || e.to_string().contains("unique") {
            (
                StatusCode::CONFLICT,
                Json(ErrorResponse {
                    error: "Rig already registered".to_string(),
                }),
            )
        } else {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        }
    })?;

    info!(rig_id = %req.rig_id, "Rig registered");

    Ok((
        StatusCode::CREATED,
        Json(RegisterRigResponse {
            rig_id: req.rig_id,
            api_key, // Returned in plaintext ONCE
        }),
    ))
}

/// GET /api/fleet/rigs — List all rigs (admin only)
pub async fn list_rigs(
    State(hub): State<Arc<HubState>>,
    _admin: AdminAuth,
) -> Result<Json<Vec<RigInfo>>, (StatusCode, Json<ErrorResponse>)> {
    let rows: Vec<(
        String,
        Option<String>,
        Option<String>,
        chrono::DateTime<chrono::Utc>,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<chrono::DateTime<chrono::Utc>>,
        i32,
        String,
    )> = sqlx::query_as(
        "SELECT rig_id, well_id, field, registered_at, last_seen, last_sync, event_count, status FROM rigs ORDER BY registered_at",
    )
    .fetch_all(&hub.db)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Database error: {}", e),
            }),
        )
    })?;

    let rigs = rows
        .into_iter()
        .map(
            |(rig_id, well_id, field, registered_at, last_seen, last_sync, event_count, status)| {
                RigInfo {
                    rig_id,
                    well_id,
                    field,
                    registered_at: registered_at.to_rfc3339(),
                    last_seen: last_seen.map(|t| t.to_rfc3339()),
                    last_sync: last_sync.map(|t| t.to_rfc3339()),
                    event_count,
                    status,
                }
            },
        )
        .collect();

    Ok(Json(rigs))
}

/// GET /api/fleet/rigs/{id} — Get rig details (admin only)
pub async fn get_rig(
    State(hub): State<Arc<HubState>>,
    _admin: AdminAuth,
    Path(rig_id): Path<String>,
) -> Result<Json<RigInfo>, (StatusCode, Json<ErrorResponse>)> {
    let row: Option<(
        String,
        Option<String>,
        Option<String>,
        chrono::DateTime<chrono::Utc>,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<chrono::DateTime<chrono::Utc>>,
        i32,
        String,
    )> = sqlx::query_as(
        "SELECT rig_id, well_id, field, registered_at, last_seen, last_sync, event_count, status FROM rigs WHERE rig_id = $1",
    )
    .bind(&rig_id)
    .fetch_optional(&hub.db)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Database error: {}", e),
            }),
        )
    })?;

    match row {
        Some((rig_id, well_id, field, registered_at, last_seen, last_sync, event_count, status)) => {
            Ok(Json(RigInfo {
                rig_id,
                well_id,
                field,
                registered_at: registered_at.to_rfc3339(),
                last_seen: last_seen.map(|t| t.to_rfc3339()),
                last_sync: last_sync.map(|t| t.to_rfc3339()),
                event_count,
                status,
            }))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Rig not found".to_string(),
            }),
        )),
    }
}

/// POST /api/fleet/rigs/{id}/revoke — Revoke a rig (admin only)
pub async fn revoke_rig(
    State(hub): State<Arc<HubState>>,
    _admin: AdminAuth,
    Path(rig_id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let result = sqlx::query(
        "UPDATE rigs SET status = 'revoked' WHERE rig_id = $1 AND status = 'active'",
    )
    .bind(&rig_id)
    .execute(&hub.db)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Database error: {}", e),
            }),
        )
    })?;

    if result.rows_affected() == 0 {
        Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Rig not found or already revoked".to_string(),
            }),
        ))
    } else {
        // Clear API key cache for this rig
        let mut cache = hub.api_key_cache.write().await;
        cache.retain(|_, (rid, _)| rid != &rig_id);

        info!(rig_id = %rig_id, "Rig revoked");
        Ok(StatusCode::OK)
    }
}
