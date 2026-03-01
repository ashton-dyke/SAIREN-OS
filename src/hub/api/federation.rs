//! Hub API handlers for federated CfC weight sharing.
//!
//! - `POST /api/fleet/federation/checkpoint` — UPSERT a rig's checkpoint
//! - `GET /api/fleet/federation/model` — return the latest aggregated model

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use std::sync::Arc;
use tracing::{info, warn};

use crate::cfc::checkpoint::DualCfcCheckpoint;
use crate::fleet::client::FederatedModelResponse;
use crate::hub::HubState;

/// In-memory storage for federation state.
///
/// In a full deployment this would be backed by the PostgreSQL tables
/// (`cfc_checkpoints` and `cfc_federated_model`). For the initial
/// implementation we use `DashMap` for lock-free concurrent access.
pub struct FederationState {
    /// One checkpoint per rig (latest wins).
    pub checkpoints: dashmap::DashMap<String, DualCfcCheckpoint>,
    /// Latest aggregated model (if any).
    pub aggregated: std::sync::RwLock<Option<AggregatedModel>>,
}

pub struct AggregatedModel {
    pub response: FederatedModelResponse,
    pub round: u64,
}

impl FederationState {
    pub fn new() -> Self {
        Self {
            checkpoints: dashmap::DashMap::new(),
            aggregated: std::sync::RwLock::new(None),
        }
    }
}

/// POST /api/fleet/federation/checkpoint
///
/// Accepts a rig's CfC checkpoint and triggers re-aggregation if
/// enough rigs (>= 2) have contributed.
pub async fn upload_checkpoint(
    State(state): State<Arc<HubState>>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    // Decompress if zstd-encoded (same pattern as event ingestion)
    let json_bytes = match zstd::decode_all(body.as_ref()) {
        Ok(decoded) => decoded,
        Err(_) => body.to_vec(), // Assume uncompressed JSON
    };

    let checkpoint: DualCfcCheckpoint = match serde_json::from_slice(&json_bytes) {
        Ok(cp) => cp,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("invalid checkpoint: {}", e)).into_response();
        }
    };

    let rig_id = checkpoint.metadata.rig_id.clone();
    info!(
        "[Federation] Received checkpoint from rig={}, packets={}, loss={:.6}",
        rig_id, checkpoint.metadata.packets_processed, checkpoint.metadata.avg_loss,
    );

    // UPSERT: store latest checkpoint for this rig
    state.federation.checkpoints.insert(rig_id, checkpoint);

    // Re-aggregate if we have 2+ rigs
    let num_rigs = state.federation.checkpoints.len();
    if num_rigs >= 2 {
        let all_checkpoints: Vec<DualCfcCheckpoint> = state
            .federation
            .checkpoints
            .iter()
            .map(|entry| entry.value().clone())
            .collect();

        match crate::hub::federation::federated_average(&all_checkpoints) {
            Ok(averaged) => {
                let round = {
                    let guard = state.federation.aggregated.read().expect("lock");
                    guard.as_ref().map(|a| a.round).unwrap_or(0) + 1
                };

                let total_packets: u64 = all_checkpoints.iter()
                    .map(|c| c.metadata.packets_processed)
                    .sum();

                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                let response = FederatedModelResponse {
                    checkpoint: averaged,
                    contributing_rigs: num_rigs,
                    total_packets,
                    aggregated_at: now,
                    round,
                };

                let mut guard = state.federation.aggregated.write().expect("lock");
                *guard = Some(AggregatedModel { response, round });

                info!(
                    "[Federation] Aggregated model updated: round={}, rigs={}, packets={}",
                    round, num_rigs, total_packets,
                );
            }
            Err(e) => {
                warn!("[Federation] Aggregation failed: {}", e);
            }
        }
    }

    StatusCode::CREATED.into_response()
}

/// Query parameters for the federated model endpoint.
#[derive(Debug, serde::Deserialize)]
pub struct FederatedModelQuery {
    /// Only return if round > since_round (returns 304 otherwise).
    pub since_round: Option<u64>,
}

/// GET /api/fleet/federation/model
///
/// Returns the latest aggregated federated model. Returns 404 if no
/// aggregation has been performed yet, or 304 if no update since
/// `since_round`.
pub async fn get_federated_model(
    State(state): State<Arc<HubState>>,
    Query(query): Query<FederatedModelQuery>,
) -> impl IntoResponse {
    let guard = state.federation.aggregated.read().expect("lock");

    match guard.as_ref() {
        None => StatusCode::NOT_FOUND.into_response(),
        Some(model) => {
            if let Some(since) = query.since_round {
                if model.round <= since {
                    return StatusCode::NOT_MODIFIED.into_response();
                }
            }
            Json(&model.response).into_response()
        }
    }
}
