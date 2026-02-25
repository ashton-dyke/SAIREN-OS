//! Consistent response envelope for all v2 API endpoints.
//!
//! Every v2 response is wrapped in either [`ApiResponse`] (success) or
//! [`ApiErrorResponse`] (error), ensuring a uniform JSON shape.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use serde::Serialize;

/// Metadata included in every v2 response.
#[derive(Debug, Serialize)]
pub struct ResponseMeta {
    pub timestamp: String,
    pub version: &'static str,
}

impl Default for ResponseMeta {
    fn default() -> Self {
        Self {
            timestamp: Utc::now().to_rfc3339(),
            version: "2",
        }
    }
}

/// Successful v2 response: `{ "data": T, "meta": { ... } }`
#[derive(Debug, Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub data: T,
    pub meta: ResponseMeta,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn ok(data: T) -> Response {
        let body = Self {
            data,
            meta: ResponseMeta::default(),
        };
        (StatusCode::OK, axum::Json(body)).into_response()
    }
}

/// Error detail inside [`ApiErrorResponse`].
#[derive(Debug, Serialize)]
pub struct ErrorDetail {
    pub code: String,
    pub message: String,
}

/// Error v2 response: `{ "error": { "code": "...", "message": "..." }, "meta": { ... } }`
#[derive(Debug, Serialize)]
pub struct ApiErrorResponse {
    pub error: ErrorDetail,
    pub meta: ResponseMeta,
}

impl ApiErrorResponse {
    fn build(status: StatusCode, code: &str, msg: impl Into<String>) -> Response {
        let body = Self {
            error: ErrorDetail {
                code: code.to_string(),
                message: msg.into(),
            },
            meta: ResponseMeta::default(),
        };
        (status, axum::Json(body)).into_response()
    }

    pub fn not_found(msg: impl Into<String>) -> Response {
        Self::build(StatusCode::NOT_FOUND, "NOT_FOUND", msg)
    }

    pub fn bad_request(msg: impl Into<String>) -> Response {
        Self::build(StatusCode::BAD_REQUEST, "BAD_REQUEST", msg)
    }

    pub fn internal(msg: impl Into<String>) -> Response {
        Self::build(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", msg)
    }

    pub fn service_unavailable(msg: impl Into<String>) -> Response {
        Self::build(StatusCode::SERVICE_UNAVAILABLE, "SERVICE_UNAVAILABLE", msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ok_response_shape() {
        let resp = ApiResponse::ok(serde_json::json!({"hello": "world"}));
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(v.get("data").is_some());
        assert!(v.get("meta").is_some());
        assert_eq!(v["meta"]["version"], "2");
    }

    #[tokio::test]
    async fn test_error_response_shape() {
        let resp = ApiErrorResponse::not_found("gone");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"]["code"], "NOT_FOUND");
        assert_eq!(v["error"]["message"], "gone");
    }
}
