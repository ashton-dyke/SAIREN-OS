//! API route definitions
//!
//! Organizes endpoints for the TDS Guardian dashboard:
//! - /api/v1/health - LLM health assessment
//! - /api/v1/status - System status and learning progress
//! - /api/v1/spectrum - FFT spectrum data for visualization
//! - /api/v1/verification - Latest fault verification result
//! - /api/v1/baseline - Baseline learning status and thresholds

use axum::{routing::{get, post}, Router};

use super::handlers::{self, DashboardState};

/// Create all API routes for the dashboard
pub fn api_routes(state: DashboardState) -> Router {
    Router::new()
        .route("/health", get(handlers::get_health))
        .route("/status", get(handlers::get_status))
        .route("/spectrum", get(handlers::get_spectrum))
        .route("/ttf", get(handlers::get_ttf))
        .route("/drilling", get(handlers::get_drilling_metrics))
        .route("/history", get(handlers::get_history))
        .route("/report/:timestamp", get(handlers::get_report))
        .route("/strategic/hourly", get(handlers::get_hourly_reports))
        .route("/strategic/daily", get(handlers::get_daily_reports))
        .route("/verification", get(handlers::get_verification))
        .route("/diagnosis", get(handlers::get_current_diagnosis))
        .route("/baseline", get(handlers::get_baseline_status))
        // Campaign management
        .route("/campaign", get(handlers::get_campaign))
        .route("/campaign", post(handlers::set_campaign))
        // ML Engine endpoints (V2.1)
        .route("/ml/latest", get(handlers::get_ml_latest))
        .route("/ml/history", get(handlers::get_ml_history))
        .route("/ml/optimal", get(handlers::get_ml_optimal))
        // Critical reports endpoint
        .route("/reports/critical", get(handlers::get_critical_reports))
        // Test endpoint for creating sample critical report
        .route("/reports/test", post(handlers::create_test_critical_report))
        // Well configuration endpoints
        .route("/config", get(handlers::get_config))
        .route("/config", post(handlers::update_config))
        .route("/config/validate", post(handlers::validate_config))
        // Advisory acknowledgment
        .route("/advisory/acknowledge", post(handlers::acknowledge_advisory))
        .route("/advisory/acknowledgments", get(handlers::get_acknowledgments))
        // Shift summary
        .route("/shift/summary", get(handlers::get_shift_summary))
        .with_state(state)
}

/// Legacy health endpoint at root level
pub fn legacy_routes(state: DashboardState) -> Router {
    Router::new()
        .route("/health", get(handlers::legacy_health_check))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::AppState;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use tower::ServiceExt;

    fn create_test_state() -> DashboardState {
        DashboardState {
            app_state: Arc::new(RwLock::new(AppState::default())),
            storage: None,
            strategic_storage: None,
            threshold_manager: None,
            equipment_id: "RIG".to_string(),
            ml_storage: None,
        }
    }

    #[tokio::test]
    async fn test_api_routes_health() {
        let state = create_test_state();
        let app = api_routes(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_api_routes_status() {
        let state = create_test_state();
        let app = api_routes(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_api_routes_spectrum() {
        let state = create_test_state();
        let app = api_routes(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/spectrum")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_api_routes_baseline() {
        let state = create_test_state();
        let app = api_routes(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/baseline")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
