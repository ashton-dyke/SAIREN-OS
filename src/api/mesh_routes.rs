//! Mesh gossip API routes.
//!
//! Registers `/api/mesh/*` endpoints when mesh is enabled.

use crate::gossip::server::{
    handle_fleet, handle_gossip, handle_outcome_update, handle_status, MeshHandlerState,
};
use axum::routing::{get, patch, post};
use axum::Router;

/// Build the mesh API router.
///
/// Only call this when `mesh.enabled` is true.
pub fn mesh_api_routes(state: MeshHandlerState) -> Router {
    Router::new()
        .route("/gossip", post(handle_gossip))
        .route("/status", get(handle_status))
        .route("/fleet", get(handle_fleet))
        .route("/events/{id}/outcome", patch(handle_outcome_update))
        .with_state(state)
}
