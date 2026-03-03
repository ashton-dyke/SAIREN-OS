//! Axum handlers for incoming gossip exchanges and mesh status.

use super::protocol::{self, GossipEnvelope, PROTOCOL_VERSION};
use super::state::MeshState;
use super::store::EventStore;
use crate::config;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, warn};

/// Shared state for mesh handlers.
#[derive(Clone)]
pub struct MeshHandlerState {
    pub node_id: String,
    pub store: Arc<Mutex<EventStore>>,
    pub mesh_state: Arc<MeshState>,
}

/// POST /api/mesh/gossip — handle an incoming gossip exchange.
///
/// Accepts a zstd-compressed `GossipEnvelope`, upserts received events,
/// and returns a response envelope with events the sender hasn't seen.
pub async fn handle_gossip(
    State(state): State<MeshHandlerState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // Verify sender is in our peer list
    let sender_id = headers
        .get("X-Node-ID")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    let peers = &config::get().mesh.peers;
    let is_known_peer = peers.iter().any(|p| p.id == sender_id);
    if !is_known_peer {
        warn!(sender = sender_id, "[Gossip] Rejected unknown peer");
        return (StatusCode::FORBIDDEN, "unknown peer").into_response();
    }

    // Decompress and deserialize
    let decompressed = match protocol::decompress(&body) {
        Ok(d) => d,
        Err(e) => {
            warn!(sender = sender_id, error = %e, "[Gossip] Decompression failed");
            return (StatusCode::BAD_REQUEST, "decompression failed").into_response();
        }
    };
    let envelope: GossipEnvelope = match serde_json::from_slice(&decompressed) {
        Ok(e) => e,
        Err(e) => {
            warn!(sender = sender_id, error = %e, "[Gossip] Deserialization failed");
            return (StatusCode::BAD_REQUEST, "invalid envelope").into_response();
        }
    };

    // Reject incompatible protocol versions
    if envelope.version != PROTOCOL_VERSION {
        warn!(
            sender = sender_id,
            got = envelope.version,
            expected = PROTOCOL_VERSION,
            "[Gossip] Protocol version mismatch"
        );
        return (StatusCode::BAD_REQUEST, "protocol version mismatch").into_response();
    }

    // Upsert received events
    let received = envelope.recent_events.len();
    {
        let store = state.store.lock().await;
        for event in &envelope.recent_events {
            if let Err(e) = store.upsert_event(event, None) {
                warn!(event_id = %event.id, error = %e, "[Gossip] Upsert failed");
            }
        }
    }

    debug!(
        sender = sender_id,
        received, "[Gossip] Processed incoming events"
    );

    // Build response with our events the sender hasn't seen
    let sender_cursor = state.mesh_state.get_cursor(sender_id);
    let gossip_config = &config::get().gossip;
    let response_events = {
        let store = state.store.lock().await;
        store
            .events_modified_since(sender_cursor, gossip_config.max_events_per_exchange)
            .unwrap_or_default()
    };

    let response_envelope = GossipEnvelope {
        sender_id: state.node_id.clone(),
        version: PROTOCOL_VERSION,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        recent_events: response_events,
        known_peers: Vec::new(),
    };

    // Update our cursor for this sender
    let new_cursor = {
        let store = state.store.lock().await;
        store.max_last_modified().unwrap_or(0)
    };
    state.mesh_state.record_success(sender_id, new_cursor);

    // Serialize and compress response
    let json = match serde_json::to_vec(&response_envelope) {
        Ok(j) => j,
        Err(e) => {
            warn!(error = %e, "[Gossip] Failed to serialize response");
            return (StatusCode::INTERNAL_SERVER_ERROR, "serialization error").into_response();
        }
    };
    let compressed = match protocol::compress(&json) {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "[Gossip] Failed to compress response");
            return (StatusCode::INTERNAL_SERVER_ERROR, "compression error").into_response();
        }
    };

    (
        StatusCode::OK,
        [("content-type", "application/octet-stream")],
        compressed,
    )
        .into_response()
}

// ─── Outcome feedback endpoint ───────────────────────────────────────────────

/// Request body for PATCH /api/mesh/events/{id}/outcome.
#[derive(Debug, Deserialize)]
pub struct OutcomeUpdate {
    /// One of: "true_positive", "false_positive", "resolved", "escalated", "inconclusive"
    pub outcome: String,
    /// Optional operator notes
    pub notes: Option<String>,
}

/// Response for outcome update.
#[derive(Debug, Serialize)]
pub struct OutcomeResponse {
    pub updated: bool,
    pub event_id: String,
    pub outcome: String,
}

/// PATCH /api/mesh/events/:id/outcome — update the outcome of an event.
///
/// Bumps `last_modified` so the outcome propagates to peers via gossip.
pub async fn handle_outcome_update(
    State(state): State<MeshHandlerState>,
    axum::extract::Path(event_id): axum::extract::Path<String>,
    Json(body): Json<OutcomeUpdate>,
) -> impl IntoResponse {
    // Validate outcome value
    let valid_outcomes = [
        "true_positive",
        "false_positive",
        "resolved",
        "escalated",
        "inconclusive",
        "Pending",
    ];
    if !valid_outcomes.contains(&body.outcome.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(OutcomeResponse {
                updated: false,
                event_id,
                outcome: format!(
                    "invalid outcome '{}', must be one of: {}",
                    body.outcome,
                    valid_outcomes.join(", ")
                ),
            }),
        )
            .into_response();
    }

    let store = state.store.lock().await;
    match store.update_outcome(&event_id, &body.outcome, body.notes.as_deref()) {
        Ok(true) => {
            debug!(event_id = %event_id, outcome = %body.outcome, "[Outcome] Updated");
            (
                StatusCode::OK,
                Json(OutcomeResponse {
                    updated: true,
                    event_id,
                    outcome: body.outcome,
                }),
            )
                .into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(OutcomeResponse {
                updated: false,
                event_id,
                outcome: "event not found".to_string(),
            }),
        )
            .into_response(),
        Err(e) => {
            warn!(error = %e, "[Outcome] Database error");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(OutcomeResponse {
                    updated: false,
                    event_id,
                    outcome: format!("database error: {e}"),
                }),
            )
                .into_response()
        }
    }
}

// ─── Status and fleet endpoints ──────────────────────────────────────────────

/// Node status returned by GET /api/mesh/status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStatus {
    pub node_id: String,
    pub well_id: String,
    pub uptime_secs: u64,
    pub mesh: MeshStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshStatus {
    pub peers_total: usize,
    pub events_stored: usize,
}

/// GET /api/mesh/status — return this node's status summary.
pub async fn handle_status(State(state): State<MeshHandlerState>) -> Json<NodeStatus> {
    let cfg = config::get();
    let events_stored = {
        let store = state.store.lock().await;
        store.count().unwrap_or(0)
    };

    Json(NodeStatus {
        node_id: state.node_id.clone(),
        well_id: cfg.well.name.clone(),
        uptime_secs: 0, // TODO: track actual uptime
        mesh: MeshStatus {
            peers_total: cfg.mesh.peers.len(),
            events_stored,
        },
    })
}

/// Fleet-wide status returned by GET /api/mesh/fleet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetStatus {
    pub nodes: Vec<FleetNodeEntry>,
    pub fleet_summary: FleetSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetNodeEntry {
    pub node_id: String,
    pub status: String,
    pub well_id: Option<String>,
    pub events_stored: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetSummary {
    pub nodes_online: usize,
    pub nodes_total: usize,
}

/// GET /api/mesh/fleet — aggregate status from all peers.
///
/// Queries all peers' `/api/mesh/status` concurrently (5s timeout)
/// and returns the aggregated fleet view.
pub async fn handle_fleet(State(state): State<MeshHandlerState>) -> Json<FleetStatus> {
    let cfg = config::get();
    let peers = &cfg.mesh.peers;

    // Start with our own status
    let own_events = {
        let store = state.store.lock().await;
        store.count().unwrap_or(0)
    };
    let mut nodes = vec![FleetNodeEntry {
        node_id: state.node_id.clone(),
        status: "online".to_string(),
        well_id: Some(cfg.well.name.clone()),
        events_stored: Some(own_events),
    }];

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_else(|e| {
            warn!("HTTP client builder failed: {e} — using default (no timeout)");
            reqwest::Client::new()
        });

    // Query all peers concurrently
    let mut handles = Vec::with_capacity(peers.len());
    for peer in peers {
        let peer_id = peer.id.clone();
        let url = format!("http://{}/api/mesh/status", peer.address);
        let http = http.clone();
        handles.push(tokio::spawn(async move {
            match http.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(status) = resp.json::<NodeStatus>().await {
                        FleetNodeEntry {
                            node_id: status.node_id,
                            status: "online".to_string(),
                            well_id: Some(status.well_id),
                            events_stored: Some(status.mesh.events_stored),
                        }
                    } else {
                        FleetNodeEntry {
                            node_id: peer_id,
                            status: "error".to_string(),
                            well_id: None,
                            events_stored: None,
                        }
                    }
                }
                _ => FleetNodeEntry {
                    node_id: peer_id,
                    status: "unreachable".to_string(),
                    well_id: None,
                    events_stored: None,
                },
            }
        }));
    }

    for handle in handles {
        if let Ok(entry) = handle.await {
            nodes.push(entry);
        }
    }

    let online = nodes.iter().filter(|n| n.status == "online").count();
    let total = nodes.len();

    Json(FleetStatus {
        nodes,
        fleet_summary: FleetSummary {
            nodes_online: online,
            nodes_total: total,
        },
    })
}
