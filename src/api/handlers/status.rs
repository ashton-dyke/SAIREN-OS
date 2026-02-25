//! System state endpoints: health, status, diagnosis, baseline

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Serialize;
use tracing::warn;

use crate::baseline::{wits_metrics, LearningStatus};

use super::DashboardState;

// ============================================================================
// Health Endpoint
// ============================================================================

/// Health assessment response from LLM analysis
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// Overall health score (0-100)
    pub health_score: f64,
    /// Severity classification
    pub severity: String,
    /// Natural language diagnosis
    pub diagnosis: String,
    /// Recommended action
    pub recommended_action: String,
    /// Analysis timestamp
    pub timestamp: DateTime<Utc>,
    /// Confidence level (0.0-1.0)
    pub confidence: f64,
    /// Current RPM during analysis
    pub rpm: f64,
    /// MSE efficiency percentage (0-100) from strategic advisory
    pub mse_efficiency: Option<f64>,
    /// Risk level from strategic advisory
    pub risk_level: Option<String>,
}

/// GET /api/v1/health - Get current health assessment
pub async fn get_health(State(state): State<DashboardState>) -> Json<HealthResponse> {
    let app_state = state.app_state.read().await;

    // Derive health from strategic advisory
    let (mse_efficiency, risk_level) = match &app_state.latest_advisory {
        Some(advisory) => (
            Some(advisory.efficiency_score as f64),
            Some(format!("{:?}", advisory.risk_level)),
        ),
        None => (None, None),
    };

    let (diagnosis, action) = match &app_state.latest_advisory {
        Some(advisory) => (advisory.reasoning.clone(), advisory.recommendation.clone()),
        None => (
            "System initializing, no analysis performed yet. Collecting baseline data...".to_string(),
            "Wait for learning phase to complete.".to_string(),
        ),
    };

    Json(HealthResponse {
        health_score: mse_efficiency.unwrap_or(100.0),
        severity: risk_level.clone().unwrap_or_else(|| "Healthy".to_string()),
        diagnosis,
        recommended_action: action,
        timestamp: Utc::now(),
        confidence: 0.0,
        rpm: app_state.current_rpm,
        mse_efficiency,
        risk_level,
    })
}

// ============================================================================
// Status Endpoint
// ============================================================================

/// System status response with WITS drilling parameters
#[derive(Debug, Serialize)]
pub struct StatusResponse {
    /// Current system status / rig state
    pub system_status: String,
    /// Rig state (Drilling, Reaming, Circulating, etc.)
    pub rig_state: String,
    /// Auto-classified operation type (ProductionDrilling, Milling, CementDrillOut, etc.)
    pub operation: String,
    /// Short code for operation (DRILL, MILL, CDO, CIRC, STATIC)
    pub operation_code: String,
    /// Total analyses performed
    pub total_analyses: u64,
    /// Uptime in seconds
    pub uptime_secs: u64,
    /// Samples collected
    pub samples_collected: usize,
    /// Last analysis timestamp
    pub last_analysis_time: Option<DateTime<Utc>>,
    /// Analysis interval in seconds
    pub analysis_interval_secs: u64,

    // === WITS Drilling Parameters ===
    /// Bit depth in feet
    pub bit_depth: f64,
    /// Rate of penetration in ft/hr
    pub rop: f64,
    /// Weight on bit in klbs
    pub wob: f64,
    /// Rotary RPM
    pub rpm: f64,
    /// Torque in kft-lbs
    pub torque: f64,
    /// Standpipe pressure in psi
    pub spp: f64,
    /// Hook load in klbs
    pub hook_load: f64,

    // === Well Control Parameters ===
    /// Flow in (pump output) in gpm
    pub flow_in: f64,
    /// Flow out (return flow) in gpm
    pub flow_out: f64,
    /// Pit volume in bbl
    pub pit_volume: f64,
    /// Mud weight in ppg
    pub mud_weight: f64,
    /// Equivalent circulating density in ppg
    pub ecd: f64,
    /// Gas units
    pub gas_units: f64,
    /// ECD margin to fracture pressure in ppg
    pub ecd_margin: f64,

}

/// GET /api/v1/status - Get system status with WITS drilling parameters
pub async fn get_status(State(state): State<DashboardState>) -> Json<StatusResponse> {
    let app_state = state.app_state.read().await;

    // Extract WITS drilling parameters from latest packet if available
    let (bit_depth, rop, wob, rpm, torque, spp, hook_load, flow_in, flow_out,
         pit_volume, mud_weight, ecd, gas_units, ecd_margin, rig_state) =
        match &app_state.latest_wits_packet {
            Some(packet) => (
                packet.bit_depth,
                packet.rop,
                packet.wob,
                packet.rpm,
                packet.torque,
                packet.spp,
                packet.hook_load,
                packet.flow_in,
                packet.flow_out,
                packet.pit_volume,
                packet.mud_weight_in,
                packet.ecd,
                packet.gas_units,
                // Use actual ECD margin from packet (fracture_gradient - ecd)
                packet.ecd_margin(),
                format!("{:?}", packet.rig_state),
            ),
            None => (0.0, 0.0, 0.0, app_state.current_rpm, 0.0, 0.0, 0.0,
                     0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, "Unknown".to_string()),
        };

    // Extract operation from latest drilling metrics
    let (operation, operation_code) = match &app_state.latest_drilling_metrics {
        Some(metrics) => (
            metrics.operation.display_name().to_string(),
            metrics.operation.short_code().to_string(),
        ),
        None => ("Static".to_string(), "STATIC".to_string()),
    };

    Json(StatusResponse {
        system_status: format!("{:?}", app_state.status),
        rig_state,
        operation,
        operation_code,
        total_analyses: app_state.total_analyses,
        uptime_secs: app_state.uptime_secs(),
        samples_collected: app_state.samples_collected,
        last_analysis_time: app_state.last_analysis_time,
        analysis_interval_secs: app_state.analysis_interval_secs,
        // WITS drilling parameters
        bit_depth,
        rop,
        wob,
        rpm,
        torque,
        spp,
        hook_load,
        // Well control parameters
        flow_in,
        flow_out,
        pit_volume,
        mud_weight,
        ecd,
        gas_units,
        ecd_margin,
    })
}


// ============================================================================
// Legacy Endpoints (kept for compatibility)
// ============================================================================

/// Legacy health check response
#[derive(Debug, Serialize)]
pub struct LegacyHealthResponse {
    pub status: String,
    pub version: String,
    pub uptime_seconds: u64,
}

/// GET /health - Legacy health check
pub async fn legacy_health_check(
    State(state): State<DashboardState>,
) -> Json<LegacyHealthResponse> {
    let app_state = state.app_state.read().await;
    Json(LegacyHealthResponse {
        status: format!("{:?}", app_state.status),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: app_state.uptime_secs(),
    })
}

// ============================================================================
// Current Advisory Endpoint
// ============================================================================

/// Structured advisory response from Strategic analysis
#[derive(Debug, Serialize)]
pub struct DiagnosisResponse {
    /// Current status assessment (e.g., "Drilling efficiency low")
    pub status: String,
    /// Affected component/category (e.g., "Drilling", "Well Control", "Hydraulics")
    pub component: String,
    /// Confidence level (0-100)
    pub confidence: u8,
    /// Recommended action to take
    pub recommended_action: String,
    /// Reasoning behind the advisory
    pub reasoning: String,
    /// Unix timestamp of advisory
    pub timestamp: u64,
}

impl From<&crate::types::StrategicAdvisory> for DiagnosisResponse {
    fn from(adv: &crate::types::StrategicAdvisory) -> Self {
        Self {
            status: format!("{} - Risk: {}", adv.severity, adv.risk_level),
            component: "Drilling Operations".to_string(),
            confidence: adv.efficiency_score,
            recommended_action: adv.recommendation.clone(),
            reasoning: adv.reasoning.clone(),
            timestamp: adv.timestamp,
        }
    }
}

/// GET /api/v1/diagnosis - Get current strategic advisory
///
/// Returns the latest strategic advisory from drilling analysis.
/// Returns 204 No Content if no advisory is available.
pub async fn get_current_diagnosis(State(state): State<DashboardState>) -> Response {
    let app_state = state.app_state.read().await;

    // Check if we have a strategic advisory
    match &app_state.latest_advisory {
        Some(advisory) => {
            let response = DiagnosisResponse::from(advisory);
            (StatusCode::OK, Json(response)).into_response()
        }
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

// ============================================================================
// Baseline Status Endpoint
// ============================================================================

/// Status of a single metric's baseline learning
#[derive(Debug, Serialize)]
pub struct MetricBaselineStatus {
    /// Metric identifier (e.g., "vibration_rms")
    pub metric_id: String,
    /// Learning status
    pub status: String,
    /// Samples collected
    pub samples_collected: usize,
    /// Samples needed for lock
    pub samples_needed: usize,
    /// Current mean (if learning or locked)
    pub mean: Option<f64>,
    /// Current std deviation (if learning or locked)
    pub std: Option<f64>,
    /// Warning threshold (if locked)
    pub warning_threshold: Option<f64>,
    /// Critical threshold (if locked)
    pub critical_threshold: Option<f64>,
    /// Outlier percentage during learning
    pub outlier_percentage: Option<f64>,
    /// Timestamp when locked (if locked)
    pub locked_at: Option<u64>,
}

/// Response for baseline status endpoint
#[derive(Debug, Serialize)]
pub struct BaselineStatusResponse {
    /// Equipment ID
    pub equipment_id: String,
    /// Overall baseline status
    pub overall_status: String,
    /// Number of metrics locked
    pub locked_count: usize,
    /// Number of metrics still learning
    pub learning_count: usize,
    /// Per-metric status
    pub metrics: Vec<MetricBaselineStatus>,
}

/// GET /api/v1/baseline - Get baseline learning status and thresholds
///
/// Returns the current state of baseline learning for all metrics.
/// Includes learned thresholds for locked metrics.
pub async fn get_baseline_status(State(state): State<DashboardState>) -> Json<BaselineStatusResponse> {
    let manager = match &state.threshold_manager {
        Some(m) => m.read().unwrap_or_else(|e| {
            warn!("RwLock poisoned on ThresholdManager read, recovering");
            e.into_inner()
        }),
        None => {
            // No threshold manager configured - return empty status
            return Json(BaselineStatusResponse {
                equipment_id: state.equipment_id.clone(),
                overall_status: "Not configured".to_string(),
                locked_count: 0,
                learning_count: 0,
                metrics: Vec::new(),
            });
        }
    };

    // Use WITS drilling metrics for baseline tracking
    let metrics_to_check = [
        wits_metrics::MSE,
        wits_metrics::D_EXPONENT,
        wits_metrics::DXC,
        wits_metrics::FLOW_BALANCE,
        wits_metrics::SPP,
        wits_metrics::TORQUE,
        wits_metrics::ROP,
        wits_metrics::WOB,
        wits_metrics::RPM,
        wits_metrics::ECD,
        wits_metrics::PIT_VOLUME,
        wits_metrics::GAS_UNITS,
    ];

    let mut metrics = Vec::new();
    let mut locked_count = 0;
    let mut learning_count = 0;

    for metric_id in metrics_to_check {
        if let Some(status) = manager.get_status(&state.equipment_id, metric_id) {
            let metric_status = match status {
                LearningStatus::Learning {
                    samples_collected,
                    samples_needed,
                    outlier_percentage,
                    current_mean,
                    current_std,
                } => {
                    learning_count += 1;
                    MetricBaselineStatus {
                        metric_id: metric_id.to_string(),
                        status: "Learning".to_string(),
                        samples_collected,
                        samples_needed,
                        mean: Some(current_mean),
                        std: Some(current_std),
                        warning_threshold: None,
                        critical_threshold: None,
                        outlier_percentage: Some(outlier_percentage),
                        locked_at: None,
                    }
                }
                LearningStatus::Locked {
                    mean,
                    std,
                    warning_threshold,
                    critical_threshold,
                    sample_count,
                    locked_at,
                } => {
                    locked_count += 1;
                    MetricBaselineStatus {
                        metric_id: metric_id.to_string(),
                        status: "Locked".to_string(),
                        samples_collected: sample_count,
                        samples_needed: 0,
                        mean: Some(mean),
                        std: Some(std),
                        warning_threshold: Some(warning_threshold),
                        critical_threshold: Some(critical_threshold),
                        outlier_percentage: None,
                        locked_at: Some(locked_at),
                    }
                }
                LearningStatus::Contaminated {
                    outlier_percentage,
                    samples_collected,
                } => {
                    learning_count += 1;
                    MetricBaselineStatus {
                        metric_id: metric_id.to_string(),
                        status: "Contaminated".to_string(),
                        samples_collected,
                        samples_needed: 100,
                        mean: None,
                        std: None,
                        warning_threshold: None,
                        critical_threshold: None,
                        outlier_percentage: Some(outlier_percentage),
                        locked_at: None,
                    }
                }
            };
            metrics.push(metric_status);
        }
    }

    let overall_status = if locked_count == metrics_to_check.len() {
        "All baselines locked - monitoring active"
    } else if learning_count > 0 {
        "Learning in progress"
    } else {
        "Not started"
    }
    .to_string();

    Json(BaselineStatusResponse {
        equipment_id: state.equipment_id.clone(),
        overall_status,
        locked_count,
        learning_count,
        metrics,
    })
}
