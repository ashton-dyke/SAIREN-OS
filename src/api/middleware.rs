//! API middleware layers.
//!
//! Currently provides deprecation headers for the v1 API surface.

use axum::http::header::HeaderName;
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;

/// Axum middleware that adds RFC 8594 deprecation headers to v1 responses.
///
/// - `Deprecation: true`
/// - `Sunset: 2026-09-01`
pub async fn add_v1_deprecation_headers(
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;

    let headers = response.headers_mut();
    headers.insert(
        HeaderName::from_static("deprecation"),
        HeaderValue::from_static("true"),
    );
    headers.insert(
        HeaderName::from_static("sunset"),
        HeaderValue::from_static("2026-09-01"),
    );

    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use axum::middleware;
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_deprecation_headers_present() {
        let app = Router::new()
            .route("/test", get(|| async { "ok" }))
            .layer(middleware::from_fn(add_v1_deprecation_headers));

        let resp = app
            .oneshot(Request::get("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.headers().get("deprecation").unwrap(), "true");
        assert_eq!(resp.headers().get("sunset").unwrap(), "2026-09-01");
    }
}
