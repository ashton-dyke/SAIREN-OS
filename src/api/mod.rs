//! REST API module using Axum
//!
//! Provides HTTP endpoints for the SAIREN-OS drilling intelligence dashboard:
//! - v2 API with consistent envelope and consolidated live endpoint
//! - v1 API (deprecated, sunset 2026-09-01) for backward compatibility
//! - React SPA served via `rust-embed` (compiled into the binary)

pub mod envelope;
pub mod handlers;
pub mod middleware;
mod routes;
pub mod setup;
pub mod v2_handlers;
mod v2_routes;

pub use handlers::DashboardState;

use axum::http::{header, Method, StatusCode, Uri};
use axum::middleware as axum_mw;
use axum::response::{IntoResponse, Response};
use axum::Router;
use rust_embed::Embed;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

/// Dashboard assets compiled from `dashboard/dist/` via `build.rs`.
#[derive(Embed)]
#[folder = "dashboard/dist/"]
struct DashboardAssets;

/// Serve a static asset or fall back to `index.html` for SPA routing.
async fn serve_asset(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Try exact file match first.
    if let Some(content) = DashboardAssets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, mime.as_ref())],
            content.data.into_owned(),
        )
            .into_response();
    }

    // SPA fallback — serve index.html for any non-API, non-file path.
    if let Some(index) = DashboardAssets::get("index.html") {
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html")],
            index.data.into_owned(),
        )
            .into_response();
    }

    // If dashboard was not built (CI without Node), return a plain message.
    (StatusCode::OK, "SAIREN-OS is running. Dashboard not built (npm not available during compile).").into_response()
}

/// Build a CORS layer that is restrictive by default (same-origin only).
///
/// Set `SAIREN_CORS_ORIGINS` to a comma-separated list of allowed origins
/// for development (e.g., `http://localhost:5173` for the Vite dev server).
fn build_cors_layer() -> CorsLayer {
    match std::env::var("SAIREN_CORS_ORIGINS") {
        Ok(origins) => {
            let allowed: Vec<_> = origins
                .split(',')
                .filter_map(|o| o.trim().parse().ok())
                .collect();
            tracing::info!(origins = %origins, "CORS: allowing configured origins");
            CorsLayer::new()
                .allow_origin(allowed)
                .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
                .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
        }
        Err(_) => {
            // No cross-origin allowed — dashboard is same-origin
            CorsLayer::new()
                .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
                .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
        }
    }
}

/// Create the complete application router with API and SPA serving.
pub fn create_app(state: DashboardState) -> Router {
    let cors = build_cors_layer();

    Router::new()
        // v2 API (primary)
        .nest("/api/v2", v2_routes::v2_api_routes(state.clone()))
        // v1 API (deprecated — adds Deprecation + Sunset headers)
        .nest(
            "/api/v1",
            routes::api_routes(state.clone())
                .layer(axum_mw::from_fn(middleware::add_v1_deprecation_headers)),
        )
        // Legacy health endpoint at /health
        .merge(routes::legacy_routes(state))
        // SPA fallback — serves React dashboard or index.html for any unmatched path
        .fallback(serve_asset)
        // Middleware
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        .layer(cors)
}
