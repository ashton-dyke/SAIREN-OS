//! Health check endpoint

use crate::hub::HubState;
use axum::extract::State;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub db_connected: bool,
    pub library_version: u64,
    pub last_curation: Option<String>,
}

pub async fn get_health(State(hub): State<Arc<HubState>>) -> Json<HealthResponse> {
    let db_ok = sqlx::query("SELECT 1")
        .fetch_one(&hub.db)
        .await
        .is_ok();

    Json(HealthResponse {
        status: if db_ok {
            "healthy".to_string()
        } else {
            "degraded".to_string()
        },
        db_connected: db_ok,
        library_version: hub.library_version.load(Ordering::Relaxed),
        last_curation: None,
    })
}
