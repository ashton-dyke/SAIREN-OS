//! Passphrase authentication extractors

use crate::hub::HubState;
use async_trait::async_trait;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;

/// Error response body
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// Authenticated rig identity (passphrase verified + X-Rig-ID header)
pub struct RigAuth {
    pub rig_id: String,
}

/// Admin authentication (passphrase verified, no rig identity needed)
pub struct AdminAuth;

/// Extract Bearer token from Authorization header.
fn extract_bearer(parts: &Parts) -> Option<String> {
    parts
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
}

#[async_trait]
impl FromRequestParts<Arc<HubState>> for RigAuth {
    type Rejection = (StatusCode, Json<ErrorResponse>);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<HubState>,
    ) -> Result<Self, Self::Rejection> {
        let token = extract_bearer(parts).ok_or((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Missing Bearer token".to_string(),
            }),
        ))?;

        if token != state.config.passphrase {
            return Err((
                StatusCode::FORBIDDEN,
                Json(ErrorResponse {
                    error: "Invalid passphrase".to_string(),
                }),
            ));
        }

        let rig_id = parts
            .headers
            .get("x-rig-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .ok_or((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Missing X-Rig-ID header".to_string(),
                }),
            ))?;

        Ok(RigAuth { rig_id })
    }
}

#[async_trait]
impl FromRequestParts<Arc<HubState>> for AdminAuth {
    type Rejection = (StatusCode, Json<ErrorResponse>);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<HubState>,
    ) -> Result<Self, Self::Rejection> {
        let token = extract_bearer(parts).ok_or((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Missing Bearer token".to_string(),
            }),
        ))?;

        if token == state.config.passphrase {
            Ok(AdminAuth)
        } else {
            Err((
                StatusCode::FORBIDDEN,
                Json(ErrorResponse {
                    error: "Invalid passphrase".to_string(),
                }),
            ))
        }
    }
}
