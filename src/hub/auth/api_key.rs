//! API key generation, hashing, and authentication extractors

use crate::hub::HubState;
use async_trait::async_trait;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::Json;
use base64::Engine;
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Error response body
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// Authenticated rig identity (extracted from Bearer token)
pub struct RigAuth {
    pub rig_id: String,
}

/// Admin authentication (extracted from Bearer token matching admin key)
pub struct AdminAuth;

/// API key cache TTL (5 minutes)
const CACHE_TTL: Duration = Duration::from_secs(300);

/// Generate a new random API key
pub fn generate_api_key() -> String {
    let random_bytes: [u8; 32] = rand::random();
    format!(
        "sk-fleet-{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_bytes)
    )
}

/// Hash an API key with bcrypt
pub fn hash_api_key(key: &str) -> String {
    bcrypt::hash(key, bcrypt::DEFAULT_COST).expect("bcrypt hash should not fail")
}

/// Verify an API key against a bcrypt hash
pub fn verify_api_key(key: &str, hash: &str) -> bool {
    bcrypt::verify(key, hash).unwrap_or(false)
}

#[async_trait]
impl FromRequestParts<Arc<HubState>> for RigAuth {
    type Rejection = (StatusCode, Json<ErrorResponse>);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<HubState>,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|s| s.to_string())
            .ok_or((
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "Missing Bearer token".to_string(),
                }),
            ))?;

        // Check cache first
        {
            let cache = state.api_key_cache.read().await;
            if let Some((rig_id, expires_at)) = cache.get(&token) {
                if Instant::now() < *expires_at {
                    return Ok(RigAuth {
                        rig_id: rig_id.clone(),
                    });
                }
            }
        }

        // Cache miss — verify against DB
        let rigs: Vec<(String, String)> = sqlx::query_as(
            "SELECT rig_id, api_key_hash FROM rigs WHERE status = 'active'",
        )
        .fetch_all(&state.db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Database error".to_string(),
                }),
            )
        })?;

        for (rig_id, hash) in &rigs {
            if verify_api_key(&token, hash) {
                // Cache the verified key
                let mut cache = state.api_key_cache.write().await;
                cache.insert(
                    token.clone(),
                    (rig_id.clone(), Instant::now() + CACHE_TTL),
                );

                return Ok(RigAuth {
                    rig_id: rig_id.clone(),
                });
            }
        }

        Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "Invalid API key".to_string(),
            }),
        ))
    }
}

#[async_trait]
impl FromRequestParts<Arc<HubState>> for AdminAuth {
    type Rejection = (StatusCode, Json<ErrorResponse>);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<HubState>,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|s| s.to_string())
            .ok_or((
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "Missing Bearer token".to_string(),
                }),
            ))?;

        if token == state.config.admin_key {
            Ok(AdminAuth)
        } else {
            Err((
                StatusCode::FORBIDDEN,
                Json(ErrorResponse {
                    error: "Invalid admin key".to_string(),
                }),
            ))
        }
    }
}
