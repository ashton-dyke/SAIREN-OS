//! Hub API route registration and shared types

pub mod events;
pub mod library;
pub mod registry;
pub mod dashboard;
pub mod health;

use crate::hub::HubState;
use axum::Router;
use std::sync::Arc;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

/// Build the complete Fleet Hub API router
pub fn build_router(state: Arc<HubState>) -> Router {
    let fleet_routes = Router::new()
        // Event ingestion
        .route("/events", axum::routing::post(events::upload_event))
        .route("/events/{id}", axum::routing::get(events::get_event))
        .route(
            "/events/{id}/outcome",
            axum::routing::patch(events::update_outcome),
        )
        // Library sync
        .route("/library", axum::routing::get(library::get_library))
        .route("/library/stats", axum::routing::get(library::get_library_stats))
        // Rig registry
        .route("/rigs", axum::routing::get(registry::list_rigs))
        .route("/rigs/{id}", axum::routing::get(registry::get_rig))
        .route("/rigs/register", axum::routing::post(registry::register_rig))
        .route("/rigs/{id}/revoke", axum::routing::post(registry::revoke_rig))
        // Dashboard
        .route("/dashboard/summary", axum::routing::get(dashboard::get_summary))
        .route("/dashboard/trends", axum::routing::get(dashboard::get_trends))
        .route("/dashboard/outcomes", axum::routing::get(dashboard::get_outcomes))
        // Health
        .route("/health", axum::routing::get(health::get_health));

    Router::new()
        .nest("/api/fleet", fleet_routes)
        .route("/", axum::routing::get(dashboard::serve_dashboard))
        .route("/fleet_dashboard.html", axum::routing::get(dashboard::serve_dashboard))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}
