//! REST API module using Axum
//!
//! Provides HTTP endpoints for the SAIREN-OS drilling intelligence dashboard:
//! - Real-time health assessment and operational scoring
//! - System status and learning progress
//! - Static file serving for the dashboard

pub mod handlers;
mod routes;

pub use handlers::DashboardState;

use axum::{response::Html, routing::get, Router};
use tower_http::{compression::CompressionLayer, cors::CorsLayer, trace::TraceLayer};

/// Dashboard HTML (embedded at compile time)
const DASHBOARD_HTML: &str = include_str!("../../static/index.html");

/// Reports page HTML (embedded at compile time)
const REPORTS_HTML: &str = include_str!("../../static/reports.html");

/// Serve the dashboard HTML
async fn serve_dashboard() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

/// Serve the reports page HTML
async fn serve_reports() -> Html<&'static str> {
    Html(REPORTS_HTML)
}

/// Create the complete application router with API and static files
pub fn create_app(state: DashboardState) -> Router {
    // CORS configuration (permissive for development)
    let cors = CorsLayer::permissive();

    Router::new()
        // Dashboard at root
        .route("/", get(serve_dashboard))
        // Reports page
        .route("/reports.html", get(serve_reports))
        // API routes
        .nest("/api/v1", routes::api_routes(state.clone()))
        // Legacy health endpoint
        .merge(routes::legacy_routes(state))
        // Middleware
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        .layer(cors)
}

/// API error type for consistent error responses
#[derive(Debug)]
pub struct ApiError {
    pub status: axum::http::StatusCode,
    pub message: String,
    pub code: String,
}

impl ApiError {
    #[allow(dead_code)]
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: axum::http::StatusCode::BAD_REQUEST,
            message: message.into(),
            code: "BAD_REQUEST".to_string(),
        }
    }
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let body = serde_json::json!({
            "error": {
                "code": self.code,
                "message": self.message,
            }
        });

        (self.status, axum::Json(body)).into_response()
    }
}

