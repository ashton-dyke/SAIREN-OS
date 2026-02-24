//! Fleet pairing code endpoints
//!
//! Three-step pairing flow:
//! 1. Rig sends `POST /pair/request` with a 6-digit code + identity
//! 2. Admin approves via `POST /pair/approve` (passphrase-authenticated)
//! 3. Rig polls `GET /pair/status?code=...` until approved or expired

use crate::hub::HubState;
use crate::hub::auth::api_key::{AdminAuth, ErrorResponse};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tracing::info;

/// Time-to-live for pairing requests (10 minutes).
const PAIRING_TTL_SECS: u64 = 600;

/// A pending pairing request stored in memory.
pub struct PairingRequest {
    pub rig_id: String,
    pub well_id: String,
    pub field: String,
    pub created_at: Instant,
    pub approved: bool,
}

/// In-memory store for pending pairing requests, keyed by 6-digit code.
pub type PairingStore = Arc<DashMap<String, PairingRequest>>;

/// Create a new pairing store.
pub fn new_pairing_store() -> PairingStore {
    Arc::new(DashMap::new())
}

/// Spawn a background task that purges expired pairing requests every 60s.
pub fn spawn_pairing_cleanup(store: PairingStore) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            let now = Instant::now();
            store.retain(|_code, req| now.duration_since(req.created_at).as_secs() < PAIRING_TTL_SECS);
        }
    });
}

// ============================================================================
// Request / Response types
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct PairRequestBody {
    pub rig_id: String,
    pub well_id: String,
    pub field: String,
    pub code: String,
}

#[derive(Debug, Serialize)]
pub struct PairRequestResponse {
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct ApproveBody {
    pub code: String,
}

#[derive(Debug, Serialize)]
pub struct ApproveResponse {
    pub status: String,
    pub rig_id: String,
}

#[derive(Debug, Deserialize)]
pub struct StatusQuery {
    pub code: String,
}

#[derive(Debug, Serialize)]
pub struct PairStatusResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passphrase: Option<String>,
}

/// Pending pairing info for the dashboard.
#[derive(Debug, Serialize)]
pub struct PendingPairing {
    pub code: String,
    pub rig_id: String,
    pub well_id: String,
    pub field: String,
    pub age_secs: u64,
}

// ============================================================================
// Handlers
// ============================================================================

/// POST /api/fleet/pair/request — Rig submits a pairing request (unauthenticated).
///
/// Rate-limited by the hub's `GovernorLayer`.
pub async fn request_pairing(
    State(hub): State<Arc<HubState>>,
    Json(req): Json<PairRequestBody>,
) -> Result<(StatusCode, Json<PairRequestResponse>), (StatusCode, Json<ErrorResponse>)> {
    let code = req.code.trim().to_string();

    if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Code must be exactly 6 digits".to_string(),
            }),
        ));
    }

    // Check for duplicate code
    if hub.pairing_requests.contains_key(&code) {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "Code already in use, generate a new one".to_string(),
            }),
        ));
    }

    info!(
        code = %code,
        rig_id = %req.rig_id,
        "Pairing request received"
    );

    hub.pairing_requests.insert(
        code,
        PairingRequest {
            rig_id: req.rig_id,
            well_id: req.well_id,
            field: req.field,
            created_at: Instant::now(),
            approved: false,
        },
    );

    Ok((
        StatusCode::ACCEPTED,
        Json(PairRequestResponse {
            status: "pending".to_string(),
        }),
    ))
}

/// POST /api/fleet/pair/approve — Admin approves a pairing code (passphrase auth).
pub async fn approve_pairing(
    State(hub): State<Arc<HubState>>,
    _admin: AdminAuth,
    Json(req): Json<ApproveBody>,
) -> Result<Json<ApproveResponse>, (StatusCode, Json<ErrorResponse>)> {
    let code = req.code.trim().to_string();

    let mut entry = hub.pairing_requests.get_mut(&code).ok_or((
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "Pairing code not found or expired".to_string(),
        }),
    ))?;

    // Check TTL
    if entry.created_at.elapsed().as_secs() > PAIRING_TTL_SECS {
        drop(entry);
        hub.pairing_requests.remove(&code);
        return Err((
            StatusCode::GONE,
            Json(ErrorResponse {
                error: "Pairing code expired".to_string(),
            }),
        ));
    }

    entry.approved = true;
    let rig_id = entry.rig_id.clone();
    let well_id = entry.well_id.clone();
    let field = entry.field.clone();
    drop(entry);

    // Register rig in DB (same as enroll, but INSERT ... ON CONFLICT to handle re-pair)
    let result = sqlx::query(
        "INSERT INTO rigs (rig_id, api_key_hash, well_id, field) VALUES ($1, $2, $3, $4)
         ON CONFLICT (rig_id) DO UPDATE SET well_id = $3, field = $4, status = 'active'",
    )
    .bind(&rig_id)
    .bind("")
    .bind(&well_id)
    .bind(&field)
    .execute(&hub.db)
    .await;

    if let Err(e) = result {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Database error: {}", e),
            }),
        ));
    }

    info!(code = %code, rig_id = %rig_id, "Pairing approved");

    Ok(Json(ApproveResponse {
        status: "approved".to_string(),
        rig_id,
    }))
}

/// GET /api/fleet/pair/status?code=... — Rig polls for pairing approval (unauthenticated).
pub async fn pairing_status(
    State(hub): State<Arc<HubState>>,
    Query(params): Query<StatusQuery>,
) -> Json<PairStatusResponse> {
    let code = params.code.trim().to_string();

    match hub.pairing_requests.get(&code) {
        Some(entry) => {
            // Check TTL
            if entry.created_at.elapsed().as_secs() > PAIRING_TTL_SECS {
                drop(entry);
                hub.pairing_requests.remove(&code);
                return Json(PairStatusResponse {
                    status: "expired".to_string(),
                    passphrase: None,
                });
            }

            if entry.approved {
                // Return the fleet passphrase so the rig can configure itself
                Json(PairStatusResponse {
                    status: "approved".to_string(),
                    passphrase: Some(hub.config.passphrase.clone()),
                })
            } else {
                Json(PairStatusResponse {
                    status: "pending".to_string(),
                    passphrase: None,
                })
            }
        }
        None => Json(PairStatusResponse {
            status: "expired".to_string(),
            passphrase: None,
        }),
    }
}

/// GET /api/fleet/pair/pending — List pending pairings for the dashboard (admin auth).
pub async fn list_pending(
    State(hub): State<Arc<HubState>>,
    _admin: AdminAuth,
) -> Json<Vec<PendingPairing>> {
    let now = Instant::now();
    let mut pending = Vec::new();

    for entry in hub.pairing_requests.iter() {
        let age = now.duration_since(entry.created_at).as_secs();
        if age < PAIRING_TTL_SECS && !entry.approved {
            pending.push(PendingPairing {
                code: entry.key().clone(),
                rig_id: entry.rig_id.clone(),
                well_id: entry.well_id.clone(),
                field: entry.field.clone(),
                age_secs: age,
            });
        }
    }

    // Sort by age (newest first)
    pending.sort_by(|a, b| a.age_secs.cmp(&b.age_secs));
    Json(pending)
}
