//! Config management and advisory acknowledgment endpoints

use axum::extract::State;
use axum::Json;
use serde::Serialize;
use tracing::warn;

use super::DashboardState;

// ============================================================================
// Well Configuration Endpoints
// ============================================================================

/// GET /api/v1/config - Return the active well configuration
///
/// Returns the complete WellConfig as JSON including all thresholds,
/// physics parameters, baseline learning settings, and campaign overrides.
pub async fn get_config() -> Json<serde_json::Value> {
    let cfg = crate::config::get_arc();
    match serde_json::to_value(&*cfg) {
        Ok(v) => Json(v),
        Err(e) => Json(serde_json::json!({
            "error": format!("Failed to serialize config: {}", e)
        })),
    }
}

/// Request body for updating well configuration
#[derive(Debug, serde::Deserialize)]
pub struct UpdateConfigRequest {
    /// Partial or full well config as JSON.
    /// Missing fields retain their current values.
    #[serde(flatten)]
    pub config: crate::config::WellConfig,
}

/// Response after config update attempt
#[derive(Debug, Serialize)]
pub struct UpdateConfigResponse {
    pub success: bool,
    pub message: String,
    /// Validation errors if any
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

/// POST /api/v1/config - Validate and save a new well configuration to disk
///
/// The config is validated but NOT applied to the running system (requires restart).
/// Saves to `./well_config.toml` so it takes effect on next startup.
pub async fn update_config(
    Json(request): Json<UpdateConfigRequest>,
) -> Json<UpdateConfigResponse> {
    // Validate the new config
    match request.config.validate() {
        Ok(()) => {}
        Err(crate::config::ConfigError::Validation(errors)) => {
            return Json(UpdateConfigResponse {
                success: false,
                message: "Configuration validation failed".to_string(),
                errors,
            });
        }
        Err(e) => {
            return Json(UpdateConfigResponse {
                success: false,
                message: format!("Validation error: {}", e),
                errors: vec![],
            });
        }
    }

    // Save to disk
    let save_path = std::path::PathBuf::from("well_config.toml");
    match request.config.save_to_file(&save_path) {
        Ok(()) => Json(UpdateConfigResponse {
            success: true,
            message: "Config saved to well_config.toml. Restart SAIREN-OS to apply changes.".to_string(),
            errors: vec![],
        }),
        Err(e) => Json(UpdateConfigResponse {
            success: false,
            message: format!("Failed to save config: {}", e),
            errors: vec![],
        }),
    }
}

/// GET /api/v1/config/validate - Validate a config without saving
pub async fn validate_config(
    Json(request): Json<UpdateConfigRequest>,
) -> Json<UpdateConfigResponse> {
    match request.config.validate() {
        Ok(()) => Json(UpdateConfigResponse {
            success: true,
            message: "Configuration is valid".to_string(),
            errors: vec![],
        }),
        Err(crate::config::ConfigError::Validation(errors)) => Json(UpdateConfigResponse {
            success: false,
            message: "Configuration validation failed".to_string(),
            errors,
        }),
        Err(e) => Json(UpdateConfigResponse {
            success: false,
            message: format!("Validation error: {}", e),
            errors: vec![],
        }),
    }
}

// ============================================================================
// Advisory Acknowledgment Endpoints
// ============================================================================

/// Request body for acknowledging an advisory
#[derive(Debug, serde::Deserialize)]
pub struct AcknowledgeRequest {
    /// Advisory ticket timestamp (Unix seconds)
    pub ticket_timestamp: u64,
    /// Who acknowledged (crew role or name)
    pub acknowledged_by: String,
    /// Optional notes from the acknowledger
    #[serde(default)]
    pub notes: String,
    /// Action taken (e.g. "monitored", "adjusted_parameters", "shut_in")
    #[serde(default)]
    pub action_taken: String,
}

/// Stored acknowledgment record
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct AcknowledgmentRecord {
    pub ticket_timestamp: u64,
    pub acknowledged_by: String,
    pub acknowledged_at: u64,
    pub notes: String,
    pub action_taken: String,
}

/// Response after acknowledging an advisory
#[derive(Debug, Serialize)]
pub struct AcknowledgeResponse {
    pub success: bool,
    pub message: String,
    pub record: Option<AcknowledgmentRecord>,
}

/// POST /api/v1/advisory/acknowledge - Acknowledge an advisory ticket
///
/// Creates an audit trail of who acknowledged which advisory and what action was taken.
/// Stored in Sled DB for persistence across restarts.
pub async fn acknowledge_advisory(
    State(state): State<DashboardState>,
    Json(request): Json<AcknowledgeRequest>,
) -> Json<AcknowledgeResponse> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let record = AcknowledgmentRecord {
        ticket_timestamp: request.ticket_timestamp,
        acknowledged_by: request.acknowledged_by,
        acknowledged_at: now,
        notes: request.notes,
        action_taken: request.action_taken,
    };

    // Persist to sled before touching the in-memory list so the record
    // survives even if the process is killed immediately after this write.
    if let Err(e) = crate::storage::acks::persist(record.acknowledged_at, &record) {
        warn!("Failed to persist acknowledgment: {}", e);
    }

    // Store in the app state's acknowledgment log (bounded ring buffer)
    let mut app_state = state.app_state.write().await;
    if app_state.acknowledgments.len() >= crate::pipeline::MAX_ACKNOWLEDGMENTS {
        app_state.acknowledgments.pop_front();
    }
    app_state.acknowledgments.push_back(record.clone());

    tracing::info!(
        ticket_ts = record.ticket_timestamp,
        by = %record.acknowledged_by,
        action = %record.action_taken,
        "Advisory acknowledged"
    );

    Json(AcknowledgeResponse {
        success: true,
        message: "Advisory acknowledged and logged".to_string(),
        record: Some(record),
    })
}

/// GET /api/v1/advisory/acknowledgments - List recent acknowledgments
pub async fn get_acknowledgments(
    State(state): State<DashboardState>,
) -> Json<Vec<AcknowledgmentRecord>> {
    let app_state = state.app_state.read().await;
    Json(Vec::from(app_state.acknowledgments.clone()))
}
