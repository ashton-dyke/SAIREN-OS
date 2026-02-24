//! Hub API route registration and shared types

pub mod events;
pub mod graph;
pub mod intelligence;
pub mod library;
pub mod metrics;
pub mod pairing;
pub mod performance;
pub mod registry;
pub mod dashboard;
pub mod health;

use crate::hub::HubState;
use axum::Router;
use std::sync::Arc;
use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

/// Build the complete Fleet Hub API router
///
/// Rate limiting (item 4.3): IP-based, 20 req/s sustained, burst of 50.
/// Returns HTTP 429 on burst exhaustion automatically via `GovernorLayer`.
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
        .route("/rigs/{id}/revoke", axum::routing::post(registry::revoke_rig))
        // Enrollment
        .route("/enroll", axum::routing::post(registry::enroll_rig))
        // Performance data (offset well sharing)
        .route("/performance", axum::routing::post(performance::upload_performance))
        .route("/performance", axum::routing::get(performance::get_performance))
        // Dashboard
        .route("/dashboard/summary", axum::routing::get(dashboard::get_summary))
        .route("/dashboard/trends", axum::routing::get(dashboard::get_trends))
        .route("/dashboard/outcomes", axum::routing::get(dashboard::get_outcomes))
        // Intelligence distribution (rig pull)
        .route("/intelligence", axum::routing::get(intelligence::get_intelligence))
        // Knowledge graph
        .route("/graph/stats", axum::routing::get(graph::graph_stats))
        .route("/graph/formation", axum::routing::get(graph::formation_context))
        .route("/graph/rebuild", axum::routing::post(graph::rebuild_graph))
        // Prometheus metrics (item 4.1) — no auth, scraped by Prometheus
        .route("/metrics", axum::routing::get(metrics::get_metrics))
        // Health
        .route("/health", axum::routing::get(health::get_health))
        // Pairing code flow
        .route("/pair/request", axum::routing::post(pairing::request_pairing))
        .route("/pair/approve", axum::routing::post(pairing::approve_pairing))
        .route("/pair/status", axum::routing::get(pairing::pairing_status))
        .route("/pair/pending", axum::routing::get(pairing::list_pending));

    // Rate limiting: 20 req/s sustained, burst up to 50 per IP (item 4.3)
    let governor_config = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(20)
            .burst_size(50)
            .finish()
            .expect("valid governor config"),
    );

    Router::new()
        .nest("/api/fleet", fleet_routes)
        .route("/", axum::routing::get(dashboard::serve_dashboard))
        .route("/fleet_dashboard.html", axum::routing::get(dashboard::serve_dashboard))
        .layer(GovernorLayer { config: governor_config })
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}
