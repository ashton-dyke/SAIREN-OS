//! API Regression Tests
//!
//! In-process tests that build the Axum app via `create_app()` and exercise
//! all /api/v1/* endpoints using `tower::ServiceExt::oneshot()`.
//! No binary spawn, no network port — runs in CI without `#[ignore]`.

use sairen_os::api::{create_app, DashboardState};
use sairen_os::config;
use sairen_os::config::WellConfig;
use sairen_os::pipeline::AppState;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::ServiceExt;

fn ensure_config() {
    if !config::is_initialized() {
        config::init(WellConfig::default(), config::ConfigProvenance::default());
    }
}

fn create_test_state() -> DashboardState {
    DashboardState {
        app_state: Arc::new(RwLock::new(AppState::default())),
        strategic_storage: None,
        threshold_manager: None,
        equipment_id: "TEST-RIG".to_string(),
        ml_storage: None,
    }
}

/// All v1 GET endpoints should return 200.
#[tokio::test]
async fn test_v1_get_endpoints_return_200() {
    ensure_config();

    let endpoints = [
        "/api/v1/health",
        "/api/v1/status",
        "/api/v1/drilling",
        "/api/v1/verification",
        "/api/v1/baseline",
        "/api/v1/campaign",
        "/api/v1/config",
        "/api/v1/ml/latest",
        "/api/v1/metrics",
        "/api/v1/advisory/acknowledgments",
        "/api/v1/shift/summary",
    ];

    for endpoint in &endpoints {
        let app = create_app(create_test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(*endpoint)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert!(
            resp.status().is_success(),
            "GET {endpoint} returned status {}",
            resp.status()
        );
    }
}

/// /api/v1/health returns a JSON object.
#[tokio::test]
async fn test_v1_health_returns_json_object() {
    ensure_config();
    let app = create_app(create_test_state());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json.is_object(), "Health response should be a JSON object");
}

/// /api/v1/status returns a JSON object.
#[tokio::test]
async fn test_v1_status_returns_json_object() {
    ensure_config();
    let app = create_app(create_test_state());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json.is_object(), "Status response should be a JSON object");
}

/// /api/v1/baseline returns a JSON object.
#[tokio::test]
async fn test_v1_baseline_returns_json_object() {
    ensure_config();
    let app = create_app(create_test_state());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/baseline")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json.is_object(), "Baseline response should be a JSON object");
}

/// /api/v1/config returns a JSON object.
#[tokio::test]
async fn test_v1_config_returns_json_object() {
    ensure_config();
    let app = create_app(create_test_state());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json.is_object(), "Config response should be a JSON object");
}

/// Legacy /health returns 200.
#[tokio::test]
async fn test_legacy_health_returns_200() {
    ensure_config();
    let app = create_app(create_test_state());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}
