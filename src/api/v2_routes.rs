//! v2 API route table.

use axum::routing::{get, post};
use axum::Router;

use super::handlers::DashboardState;
use super::v2_handlers;

/// Build the v2 API router.
pub fn v2_api_routes(state: DashboardState) -> Router {
    Router::new()
        // Core endpoints
        .route("/system/health", get(v2_handlers::system_health))
        .route("/live", get(v2_handlers::live_data))
        .route("/drilling", get(v2_handlers::drilling))
        // Reports
        .route("/reports/hourly", get(v2_handlers::reports_hourly))
        .route("/reports/daily", get(v2_handlers::reports_daily))
        .route("/reports/critical", get(v2_handlers::reports_critical))
        // ML
        .route("/ml/latest", get(v2_handlers::ml_latest))
        .route("/ml/optimal", get(v2_handlers::ml_optimal))
        // Config
        .route("/config", get(v2_handlers::get_config))
        .route("/config", post(v2_handlers::update_config))
        .route("/config/validate", post(v2_handlers::validate_config))
        .route("/config/reload", post(v2_handlers::reload_config))
        .route("/config/suggestions", get(v2_handlers::config_suggestions))
        // Campaign
        .route("/campaign", get(v2_handlers::get_campaign))
        .route("/campaign", post(v2_handlers::set_campaign))
        // Advisory
        .route("/advisory/acknowledge", post(v2_handlers::acknowledge_advisory))
        .route("/advisory/acknowledgments", get(v2_handlers::get_acknowledgments))
        // Feedback (stats before parameterized route to avoid capture)
        .route("/advisory/feedback/stats", get(v2_handlers::feedback_stats))
        .route("/advisory/feedback/:timestamp", post(v2_handlers::submit_feedback))
        // Lookahead
        .route("/lookahead/status", get(v2_handlers::lookahead_status))
        // Damping
        .route("/damping/status", get(v2_handlers::damping_status))
        .route("/damping/recipes", get(v2_handlers::damping_recipes))
        // Well debrief
        .route("/well/debrief", get(v2_handlers::get_debrief_handler))
        .route("/well/debrief", post(v2_handlers::generate_debrief_handler))
        // Shift
        .route("/shift/summary", get(v2_handlers::shift_summary))
        // Debug
        .route("/debug/baseline", get(v2_handlers::debug_baseline))
        .route("/debug/ml/history", get(v2_handlers::debug_ml_history))
        .route("/debug/fleet/intelligence", get(v2_handlers::debug_fleet_intelligence))
        // Prometheus metrics (unchanged format)
        .route("/metrics", get(v2_handlers::metrics))
        .with_state(state)
}
