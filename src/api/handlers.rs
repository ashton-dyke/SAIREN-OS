//! API route handlers
//!
//! Request handling logic for all API endpoints including:
//! - Drilling health status and advisory data
//! - System status with WITS drilling parameters
//! - Baseline learning status and dynamic thresholds

use axum::{extract::State, Json};
use tracing::warn;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::baseline::{wits_metrics, LearningStatus, ThresholdManager};
use crate::pipeline::AppState;
use crate::ml_engine::{MLInsightsStorage, OptimalFinder};
use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

// ============================================================================
// API State
// ============================================================================

/// Shared state for API handlers
#[derive(Clone)]
pub struct DashboardState {
    /// Application state from the pipeline
    pub app_state: Arc<RwLock<AppState>>,
    /// Strategic report storage
    pub strategic_storage: Option<crate::storage::StrategicStorage>,
    /// Optional threshold manager for baseline status
    pub threshold_manager: Option<Arc<std::sync::RwLock<ThresholdManager>>>,
    /// Equipment ID for baseline lookups
    pub equipment_id: String,
    /// ML insights storage (V2.1)
    pub ml_storage: Option<Arc<MLInsightsStorage>>,
}

impl DashboardState {
    /// Create a new DashboardState with storage
    /// Create a new DashboardState with thresholds
    pub fn new_with_storage_and_thresholds(
        app_state: Arc<RwLock<AppState>>,
        threshold_manager: Arc<std::sync::RwLock<ThresholdManager>>,
        equipment_id: &str,
    ) -> Self {
        Self {
            app_state,
            strategic_storage: None,
            threshold_manager: Some(threshold_manager),
            equipment_id: equipment_id.to_string(),
            ml_storage: None,
        }
    }

}

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
// Strategic Report Endpoints
// ============================================================================

/// Query parameters for strategic endpoints
#[derive(Debug, serde::Deserialize)]
pub struct StrategicQuery {
    /// Maximum number of reports to return (default: 24 for hourly, 7 for daily)
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Hourly strategic report response
#[derive(Debug, Serialize)]
pub struct HourlyReportResponse {
    pub health_score: f64,
    pub severity: String,
    pub diagnosis: String,
    pub action: String,
    pub raw: String,
}

impl From<crate::strategic::HourlyReport> for HourlyReportResponse {
    fn from(report: crate::strategic::HourlyReport) -> Self {
        Self {
            health_score: report.health_score,
            severity: report.severity,
            diagnosis: report.diagnosis,
            action: report.action,
            raw: report.raw,
        }
    }
}

/// Daily strategic report response with optional details
#[derive(Debug, Serialize)]
pub struct DailyReportResponse {
    pub health_score: f64,
    pub severity: String,
    pub diagnosis: String,
    pub action: String,
    pub details: Option<DetailsResponse>,
    pub raw: String,
}

/// Details section for daily reports
#[derive(Debug, Serialize)]
pub struct DetailsResponse {
    pub trend: String,
    pub top_drivers: String,
    pub confidence: String,
    pub next_check: String,
}

impl From<crate::strategic::DailyReport> for DailyReportResponse {
    fn from(report: crate::strategic::DailyReport) -> Self {
        Self {
            health_score: report.health_score,
            severity: report.severity,
            diagnosis: report.diagnosis,
            action: report.action,
            details: report.details.map(|d| DetailsResponse {
                trend: d.trend,
                top_drivers: d.top_drivers,
                confidence: d.confidence,
                next_check: d.next_check,
            }),
            raw: report.raw,
        }
    }
}

/// GET /api/v1/strategic/hourly?limit=24
///
/// Returns recent hourly strategic reports
pub async fn get_hourly_reports(
    State(state): State<DashboardState>,
    Query(query): Query<StrategicQuery>,
) -> Json<Vec<HourlyReportResponse>> {
    let limit = query.limit.unwrap_or(24);

    if let Some(storage) = &state.strategic_storage {
        match storage.get_hourly(limit) {
            Ok(reports) => {
                let responses: Vec<HourlyReportResponse> =
                    reports.into_iter().map(HourlyReportResponse::from).collect();
                Json(responses)
            }
            Err(e) => {
                tracing::error!("Failed to retrieve hourly reports: {}", e);
                Json(Vec::new())
            }
        }
    } else {
        tracing::warn!("Strategic storage not available");
        Json(Vec::new())
    }
}

/// GET /api/v1/strategic/daily?limit=7
///
/// Returns recent daily strategic reports
pub async fn get_daily_reports(
    State(state): State<DashboardState>,
    Query(query): Query<StrategicQuery>,
) -> Json<Vec<DailyReportResponse>> {
    let limit = query.limit.unwrap_or(7);

    if let Some(storage) = &state.strategic_storage {
        match storage.get_daily(limit) {
            Ok(reports) => {
                let responses: Vec<DailyReportResponse> =
                    reports.into_iter().map(DailyReportResponse::from).collect();
                Json(responses)
            }
            Err(e) => {
                tracing::error!("Failed to retrieve daily reports: {}", e);
                Json(Vec::new())
            }
        }
    } else {
        tracing::warn!("Strategic storage not available");
        Json(Vec::new())
    }
}

// ============================================================================
// Drilling Metrics Endpoint
// ============================================================================

/// Drilling metrics response for MSE efficiency and formation analysis
#[derive(Debug, Serialize)]
pub struct DrillingMetricsResponse {
    /// Current MSE value in psi
    pub mse: f64,
    /// MSE efficiency percentage (0-100)
    pub mse_efficiency: f64,
    /// Baseline MSE for comparison
    pub mse_baseline: f64,
    /// MSE deviation from baseline (percentage)
    pub mse_deviation: f64,
    /// D-exponent value
    pub d_exponent: f64,
    /// Corrected d-exponent (Dxc)
    pub dxc: f64,
    /// Detected formation type
    pub formation_type: String,
    /// Whether formation change was detected
    pub formation_change: bool,
    /// Trend direction (Stable, Increasing, Decreasing)
    pub trend: String,
    /// Specialist votes (if available)
    pub votes: Option<SpecialistVotesResponse>,
    /// CfC-detected formation transition (if recent)
    pub cfc_formation_transition: Option<FormationTransitionInfo>,
}

/// CfC formation transition detection info
#[derive(Debug, Serialize)]
pub struct FormationTransitionInfo {
    pub timestamp: u64,
    pub bit_depth: f64,
    pub surprised_features: Vec<String>,
}

/// Specialist votes for advisory panel
#[derive(Debug, Serialize)]
pub struct SpecialistVotesResponse {
    pub mse: String,
    pub hydraulic: String,
    pub well_control: String,
    pub formation: String,
}

/// GET /api/v1/drilling - Get drilling metrics (MSE, formation analysis)
pub async fn get_drilling_metrics(State(state): State<DashboardState>) -> Json<DrillingMetricsResponse> {
    let app_state = state.app_state.read().await;

    // Get drilling metrics from latest_drilling_metrics or strategic report
    let (mse, mse_efficiency, mse_delta_percent, d_exponent, dxc) = match &app_state.latest_drilling_metrics {
        Some(metrics) => (
            metrics.mse,
            metrics.mse_efficiency,
            metrics.mse_delta_percent,
            metrics.d_exponent,
            metrics.dxc,
        ),
        None => (0.0, 100.0, 0.0, 1.0, 1.0),
    };

    // Get actual MSE baseline from threshold manager if available
    let mse_baseline = if let Some(ref manager) = state.threshold_manager {
        if let Ok(mgr) = manager.read() {
            mgr.get_threshold(&state.equipment_id, wits_metrics::MSE)
                .map(|t| t.baseline_mean)
                .unwrap_or(0.0)
        } else {
            0.0
        }
    } else {
        0.0
    };

    // Use actual baseline if available, otherwise estimate from current MSE
    let (final_baseline, final_deviation) = if mse_baseline > 0.0 {
        // Use real baseline and calculated deviation
        (mse_baseline, mse_delta_percent * 100.0)
    } else if mse > 0.0 {
        // Fallback: estimate baseline as 85% of current (assumes slight inefficiency)
        let estimated_baseline = mse * 0.85;
        let deviation = ((mse - estimated_baseline) / estimated_baseline) * 100.0;
        (estimated_baseline, deviation)
    } else {
        (0.0, 0.0)
    };

    // Get specialist votes from strategic advisory if available
    let votes = app_state.latest_advisory.as_ref().map(|advisory| {
        // Map votes by specialist name
        let mut mse_vote = "--".to_string();
        let mut hydraulic_vote = "--".to_string();
        let mut well_control_vote = "--".to_string();
        let mut formation_vote = "--".to_string();

        for v in &advisory.votes {
            let vote_str = format!("{:?}", v.vote);
            match v.specialist.to_lowercase().as_str() {
                "mse" => mse_vote = vote_str,
                "hydraulic" => hydraulic_vote = vote_str,
                "wellcontrol" | "well_control" => well_control_vote = vote_str,
                "formation" => formation_vote = vote_str,
                _ => {}
            }
        }

        SpecialistVotesResponse {
            mse: mse_vote,
            hydraulic: hydraulic_vote,
            well_control: well_control_vote,
            formation: formation_vote,
        }
    });

    // Check for CfC formation transition (within last 60 seconds)
    let cfc_transition = app_state.latest_formation_transition.as_ref().and_then(|event| {
        let latest_ts = app_state.latest_wits_packet.as_ref().map(|p| p.timestamp).unwrap_or(0);
        if latest_ts.saturating_sub(event.timestamp) <= 60 {
            Some(FormationTransitionInfo {
                timestamp: event.timestamp,
                bit_depth: event.bit_depth,
                surprised_features: event.surprised_features.clone(),
            })
        } else {
            None
        }
    });
    let formation_change = cfc_transition.is_some();

    Json(DrillingMetricsResponse {
        mse,
        mse_efficiency,
        mse_baseline: final_baseline,
        mse_deviation: final_deviation,
        d_exponent,
        dxc,
        formation_type: "Normal".to_string(),
        formation_change,
        trend: "Stable".to_string(),
        votes,
        cfc_formation_transition: cfc_transition,
    })
}

// ============================================================================
// Verification Endpoints
// ============================================================================

/// Response for verification endpoint
#[derive(Serialize)]
pub struct VerificationResponse {
    /// Whether there is a latest verification result
    pub has_verification: bool,
    /// Verification status (Confirmed, Rejected, Uncertain, Pending)
    pub status: Option<String>,
    /// The suspected fault type
    pub suspected_fault: Option<String>,
    /// Reasoning for the verification decision
    pub reasoning: Option<String>,
    /// Final severity (if confirmed)
    pub final_severity: Option<String>,
    /// Whether to alert dashboard
    pub send_to_dashboard: Option<bool>,
    /// Original ticket timestamp (seconds since epoch)
    pub ticket_timestamp: Option<u64>,
    /// Trigger value that caused the ticket
    pub trigger_value: Option<f64>,
    /// Confidence from tactical agent
    pub confidence: Option<f64>,
    /// Sensor that triggered the fault
    pub sensor_name: Option<String>,
    /// Count of verified (confirmed) faults
    pub verified_count: u64,
    /// Count of rejected faults
    pub rejected_count: u64,
}

/// GET /api/v1/verification - Get latest verification result
pub async fn get_verification(State(state): State<DashboardState>) -> Json<VerificationResponse> {
    let app_state = state.app_state.read().await;

    match &app_state.latest_verification {
        Some(v) => Json(VerificationResponse {
            has_verification: true,
            status: Some(v.status.to_string()),
            suspected_fault: Some(v.ticket.description.clone()),
            reasoning: Some(v.reasoning.clone()),
            final_severity: Some(format!("{:?}", v.final_severity)),
            send_to_dashboard: Some(v.send_to_dashboard),
            ticket_timestamp: Some(v.ticket.timestamp),
            trigger_value: Some(v.ticket.trigger_value),
            confidence: Some(v.ticket.severity as u8 as f64 / 4.0 * 100.0), // Severity as confidence proxy
            sensor_name: Some(v.ticket.trigger_parameter.clone()),
            verified_count: app_state.verified_faults,
            rejected_count: app_state.rejected_faults,
        }),
        None => Json(VerificationResponse {
            has_verification: false,
            status: None,
            suspected_fault: None,
            reasoning: None,
            final_severity: None,
            send_to_dashboard: None,
            ticket_timestamp: None,
            trigger_value: None,
            confidence: None,
            sensor_name: None,
            verified_count: app_state.verified_faults,
            rejected_count: app_state.rejected_faults,
        }),
    }
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

// ============================================================================
// Campaign Management Endpoints
// ============================================================================

/// Response for campaign status
#[derive(Debug, Serialize)]
pub struct CampaignResponse {
    /// Current campaign type
    pub campaign: String,
    /// Campaign short code
    pub code: String,
    /// Display name for UI
    pub display_name: String,
    /// Campaign-specific threshold info
    pub thresholds: CampaignThresholdInfo,
}

/// Threshold information for campaign response
#[derive(Debug, Serialize)]
pub struct CampaignThresholdInfo {
    /// MSE efficiency warning threshold
    pub mse_efficiency_warning: f64,
    /// Flow imbalance warning threshold
    pub flow_imbalance_warning: f64,
    /// Flow imbalance critical threshold
    pub flow_imbalance_critical: f64,
    /// Specialist weight distribution
    pub weights: SpecialistWeights,
}

/// Specialist weights for campaign response
#[derive(Debug, Serialize)]
pub struct SpecialistWeights {
    pub mse: f64,
    pub hydraulic: f64,
    pub well_control: f64,
    pub formation: f64,
}

/// GET /api/v1/campaign - Get current campaign type and thresholds
pub async fn get_campaign(State(state): State<DashboardState>) -> Json<CampaignResponse> {
    let app_state = state.app_state.read().await;

    let thresholds = &app_state.campaign_thresholds;

    Json(CampaignResponse {
        campaign: format!("{:?}", app_state.campaign),
        code: app_state.campaign.short_code().to_string(),
        display_name: app_state.campaign.display_name().to_string(),
        thresholds: CampaignThresholdInfo {
            mse_efficiency_warning: thresholds.mse_efficiency_warning,
            flow_imbalance_warning: thresholds.flow_imbalance_warning,
            flow_imbalance_critical: thresholds.flow_imbalance_critical,
            weights: SpecialistWeights {
                mse: thresholds.weight_mse,
                hydraulic: thresholds.weight_hydraulic,
                well_control: thresholds.weight_well_control,
                formation: thresholds.weight_formation,
            },
        },
    })
}

/// Request body for setting campaign
#[derive(Debug, serde::Deserialize)]
pub struct SetCampaignRequest {
    /// Campaign type: "production" or "p&a"
    pub campaign: String,
}

/// Response after setting campaign
#[derive(Debug, Serialize)]
pub struct SetCampaignResponse {
    /// Whether the change was successful
    pub success: bool,
    /// New campaign type
    pub campaign: String,
    /// Message
    pub message: String,
}

/// POST /api/v1/campaign - Set campaign type
///
/// Changes the operational campaign, which updates thresholds and LLM behavior.
/// Valid values: "production", "prod", "p&a", "pa", "plug_abandonment"
pub async fn set_campaign(
    State(state): State<DashboardState>,
    Json(request): Json<SetCampaignRequest>,
) -> Json<SetCampaignResponse> {
    let campaign = match crate::types::Campaign::from_str(&request.campaign) {
        Some(c) => c,
        None => {
            return Json(SetCampaignResponse {
                success: false,
                campaign: request.campaign,
                message: "Invalid campaign type. Use 'production' or 'p&a'".to_string(),
            });
        }
    };

    let mut app_state = state.app_state.write().await;
    app_state.set_campaign(campaign);

    Json(SetCampaignResponse {
        success: true,
        campaign: campaign.display_name().to_string(),
        message: format!(
            "Campaign switched to {}. Thresholds and LLM behavior updated.",
            campaign.display_name()
        ),
    })
}

// ============================================================================
// ML Engine Endpoints (V2.1)
// ============================================================================

/// ML insights query parameters
#[derive(Debug, serde::Deserialize)]
pub struct MLQuery {
    /// Maximum number of reports to return (default: 10)
    #[serde(default)]
    pub limit: Option<usize>,
    /// Depth to search near (for find_by_depth)
    #[serde(default)]
    pub depth: Option<f64>,
    /// Filter by campaign type
    #[serde(default)]
    pub campaign: Option<String>,
}

/// Response for ML insights
#[derive(Debug, Serialize)]
pub struct MLInsightsResponse {
    /// Whether there is ML data available
    pub has_data: bool,
    /// Latest ML report (if successful)
    pub latest: Option<MLReportSummary>,
    /// Recent history of reports
    pub history: Vec<MLReportSummary>,
    /// Total count of stored reports
    pub total_count: usize,
    /// Count of successful analyses
    pub successful_count: usize,
}

/// Summary of an ML report
#[derive(Debug, Serialize)]
pub struct MLReportSummary {
    pub timestamp: u64,
    pub campaign: String,
    pub well_id: String,
    pub field_name: String,
    pub formation_type: String,
    pub depth_range: (f64, f64),
    pub bit_hours: f64,
    pub success: bool,
    pub failure_reason: Option<String>,
    pub optimal_params: Option<MLOptimalParams>,
    pub confidence: Option<String>,
    pub sample_count: Option<usize>,
    pub correlations: Option<Vec<MLCorrelation>>,
}

/// Optimal parameters from ML analysis
#[derive(Debug, Serialize)]
pub struct MLOptimalParams {
    pub best_wob: f64,
    pub best_rpm: f64,
    pub best_flow: f64,
    pub achieved_rop: f64,
    pub mse_efficiency: f64,
    pub composite_score: f64,
    pub efficiency_rating: String,
    pub regime_id: Option<u8>,
}

/// Correlation info for API
#[derive(Debug, Serialize)]
pub struct MLCorrelation {
    pub x_param: String,
    pub y_param: String,
    pub r_value: f64,
    pub p_value: f64,
}

impl From<&crate::types::MLInsightsReport> for MLReportSummary {
    fn from(report: &crate::types::MLInsightsReport) -> Self {
        match &report.result {
            crate::types::AnalysisResult::Success(insights) => Self {
                timestamp: report.timestamp,
                campaign: format!("{:?}", report.campaign),
                well_id: report.well_id.clone(),
                field_name: report.field_name.clone(),
                formation_type: report.formation_type.clone(),
                depth_range: report.depth_range,
                bit_hours: report.bit_hours,
                success: true,
                failure_reason: None,
                optimal_params: Some(MLOptimalParams {
                    best_wob: insights.optimal_params.best_wob,
                    best_rpm: insights.optimal_params.best_rpm,
                    best_flow: insights.optimal_params.best_flow,
                    achieved_rop: insights.optimal_params.achieved_rop,
                    mse_efficiency: insights.optimal_params.mse_efficiency,
                    composite_score: insights.optimal_params.composite_score,
                    efficiency_rating: OptimalFinder::interpret_composite_score(
                        insights.optimal_params.composite_score,
                    )
                    .to_string(),
                    regime_id: insights.optimal_params.regime_id,
                }),
                confidence: Some(insights.confidence.to_string()),
                sample_count: Some(insights.sample_count),
                correlations: Some(
                    insights
                        .correlations
                        .iter()
                        .map(|c| MLCorrelation {
                            x_param: c.x_param.clone(),
                            y_param: c.y_param.clone(),
                            r_value: c.r_value,
                            p_value: c.p_value,
                        })
                        .collect(),
                ),
            },
            crate::types::AnalysisResult::Failure(failure) => Self {
                timestamp: report.timestamp,
                campaign: format!("{:?}", report.campaign),
                well_id: report.well_id.clone(),
                field_name: report.field_name.clone(),
                formation_type: report.formation_type.clone(),
                depth_range: report.depth_range,
                bit_hours: report.bit_hours,
                success: false,
                failure_reason: Some(failure.to_string()),
                optimal_params: None,
                confidence: None,
                sample_count: None,
                correlations: None,
            },
        }
    }
}

/// GET /api/v1/ml/latest - Get latest ML insights
pub async fn get_ml_latest(State(state): State<DashboardState>) -> Json<MLInsightsResponse> {
    let app_state = state.app_state.read().await;

    // First check AppState for latest report
    let latest = app_state.latest_ml_report.as_ref().map(MLReportSummary::from);

    // If we have ML storage, get history and counts
    if let Some(storage) = &state.ml_storage {
        let history: Vec<MLReportSummary> = storage
            .get_well_history(&app_state.well_id, None, 10)
            .ok()
            .unwrap_or_default()
            .iter()
            .map(MLReportSummary::from)
            .collect();

        let total_count = storage.count();
        let successful_count = storage.count_successful().unwrap_or(0);

        Json(MLInsightsResponse {
            has_data: latest.is_some() || !history.is_empty(),
            latest,
            history,
            total_count,
            successful_count,
        })
    } else {
        Json(MLInsightsResponse {
            has_data: latest.is_some(),
            latest,
            history: Vec::new(),
            total_count: 0,
            successful_count: 0,
        })
    }
}

/// GET /api/v1/ml/history - Get ML insights history
pub async fn get_ml_history(
    State(state): State<DashboardState>,
    Query(query): Query<MLQuery>,
) -> Json<Vec<MLReportSummary>> {
    let app_state = state.app_state.read().await;
    let limit = query.limit.unwrap_or(24);
    let campaign = query.campaign.as_ref().and_then(|c| crate::types::Campaign::from_str(c));

    if let Some(storage) = &state.ml_storage {
        let history: Vec<MLReportSummary> = storage
            .get_well_history(&app_state.well_id, campaign, limit)
            .ok()
            .unwrap_or_default()
            .iter()
            .map(MLReportSummary::from)
            .collect();
        Json(history)
    } else {
        Json(Vec::new())
    }
}

/// GET /api/v1/ml/optimal - Get optimal parameters for current depth
pub async fn get_ml_optimal(
    State(state): State<DashboardState>,
    Query(query): Query<MLQuery>,
) -> Response {
    let app_state = state.app_state.read().await;

    // Get depth from query or use current depth from WITS
    let depth = query.depth.unwrap_or_else(|| {
        app_state
            .latest_wits_packet
            .as_ref()
            .map(|p| p.bit_depth)
            .unwrap_or(0.0)
    });

    if let Some(storage) = &state.ml_storage {
        match storage.find_by_depth(&app_state.well_id, depth, 500.0, 5) {
            Ok(reports) if !reports.is_empty() => {
                // Find first successful report
                for report in &reports {
                    if let crate::types::AnalysisResult::Success(insights) = &report.result {
                        let response = MLOptimalParams {
                            best_wob: insights.optimal_params.best_wob,
                            best_rpm: insights.optimal_params.best_rpm,
                            best_flow: insights.optimal_params.best_flow,
                            achieved_rop: insights.optimal_params.achieved_rop,
                            mse_efficiency: insights.optimal_params.mse_efficiency,
                            composite_score: insights.optimal_params.composite_score,
                            efficiency_rating: OptimalFinder::interpret_composite_score(
                                insights.optimal_params.composite_score,
                            )
                            .to_string(),
                            regime_id: insights.optimal_params.regime_id,
                        };
                        return (StatusCode::OK, Json(response)).into_response();
                    }
                }
                (StatusCode::NOT_FOUND, "No successful analysis for this depth").into_response()
            }
            Ok(_) => (StatusCode::NOT_FOUND, "No ML data for this depth").into_response(),
            Err(e) => {
                tracing::error!("ML storage error: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Storage error").into_response()
            }
        }
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "ML storage not available").into_response()
    }
}

// ============================================================================
// Critical Reports API
// ============================================================================

/// Critical report entry for the reports page
#[derive(Debug, Serialize)]
pub struct CriticalReportEntry {
    /// Unique report ID (timestamp-based)
    pub report_id: String,
    /// Unix timestamp
    pub timestamp: u64,
    /// Formatted timestamp for display
    pub timestamp_formatted: String,
    /// Efficiency score (0-100)
    pub efficiency_score: u8,
    /// Risk level
    pub risk_level: String,
    /// Primary recommendation
    pub recommendation: String,
    /// Expected benefit
    pub expected_benefit: String,
    /// Technical reasoning
    pub reasoning: String,
    /// Trigger parameter (what caused the alert)
    pub trigger_parameter: String,
    /// Trigger value
    pub trigger_value: f64,
    /// Threshold that was exceeded
    pub threshold_value: f64,
    /// Current drilling parameters at time of alert
    pub drilling_params: CriticalDrillingParams,
    /// Specialist votes summary
    pub votes_summary: Vec<String>,
    /// Digital signature (SHA256 hash of report content)
    pub digital_signature: String,
    /// Signature timestamp
    pub signature_timestamp: String,
}

/// Drilling parameters snapshot for critical reports
#[derive(Debug, Serialize)]
pub struct CriticalDrillingParams {
    pub bit_depth: f64,
    pub rop: f64,
    pub wob: f64,
    pub rpm: f64,
    pub torque: f64,
    pub flow_in: f64,
    pub flow_out: f64,
    pub flow_balance: f64,
    pub spp: f64,
    pub mud_weight: f64,
    pub ecd: f64,
    pub pit_volume: f64,
    pub mse: f64,
    pub mse_efficiency: f64,
}

/// GET /api/v1/reports/critical - Get critical severity reports
pub async fn get_critical_reports(
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Json<Vec<CriticalReportEntry>> {
    let limit = query
        .get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(50);

    let reports = crate::storage::history::get_critical_reports(limit);

    let entries: Vec<CriticalReportEntry> = reports
        .into_iter()
        .map(|report| {
            let physics = &report.physics_report;

            // Generate digital signature from report content
            let signature_content = format!(
                "{}:{}:{}:{}:{}",
                report.timestamp,
                report.efficiency_score,
                report.recommendation,
                physics.current_depth,
                physics.current_rop
            );
            let digital_signature = format!("MD5-{:x}", md5::compute(signature_content.as_bytes()));

            // Format timestamp
            let dt = chrono::DateTime::from_timestamp(report.timestamp as i64, 0)
                .unwrap_or_default();
            let timestamp_formatted = dt.format("%Y-%m-%d %H:%M:%S UTC").to_string();
            let signature_timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();

            // Generate report ID
            let report_id = format!("RPT-{}", report.timestamp);

            // Determine trigger from votes or trace log
            let (trigger_param, trigger_val, threshold_val) = extract_trigger_info(&report);

            // Summarize votes
            let votes_summary: Vec<String> = report.votes.iter()
                .map(|v| format!("{} ({}%): {}", v.specialist, (v.weight * 100.0).clamp(0.0, 100.0) as u8, v.vote))
                .collect();

            CriticalReportEntry {
                report_id,
                timestamp: report.timestamp,
                timestamp_formatted,
                efficiency_score: report.efficiency_score,
                risk_level: format!("{:?}", report.risk_level),
                recommendation: report.recommendation,
                expected_benefit: report.expected_benefit,
                reasoning: report.reasoning,
                trigger_parameter: trigger_param,
                trigger_value: trigger_val,
                threshold_value: threshold_val,
                drilling_params: CriticalDrillingParams {
                    bit_depth: physics.current_depth,
                    rop: physics.current_rop,
                    wob: physics.current_wob,
                    rpm: physics.current_rpm,
                    torque: physics.current_torque,
                    flow_in: physics.current_flow_in,
                    flow_out: physics.current_flow_out,
                    flow_balance: physics.current_flow_out - physics.current_flow_in,
                    spp: physics.current_spp,
                    mud_weight: physics.current_mud_weight,
                    ecd: physics.current_ecd,
                    pit_volume: physics.current_pit_volume,
                    mse: physics.avg_mse,
                    mse_efficiency: physics.mse_efficiency,
                },
                votes_summary,
                digital_signature,
                signature_timestamp,
            }
        })
        .collect();

    Json(entries)
}

/// Extract trigger information from report votes and trace log
pub fn extract_trigger_info(report: &crate::types::StrategicReport) -> (String, f64, f64) {
    // Look for WellControl vote as it often has the trigger
    for vote in &report.votes {
        if vote.specialist == "WellControl" && vote.vote == crate::types::TicketSeverity::Critical {
            // Parse flow balance from reasoning
            if let Some(fb) = extract_flow_balance(&vote.reasoning) {
                return ("Flow Balance".to_string(), fb, 10.0);
            }
        }
        if vote.specialist == "MSE" && vote.vote == crate::types::TicketSeverity::Critical {
            return ("MSE Efficiency".to_string(), report.physics_report.mse_efficiency, 70.0);
        }
    }

    // Default to flow balance from physics
    let flow_balance = report.physics_report.current_flow_out - report.physics_report.current_flow_in;
    if flow_balance.abs() > 10.0 {
        return ("Flow Balance".to_string(), flow_balance, 10.0);
    }

    ("Unknown".to_string(), 0.0, 0.0)
}

/// Extract flow balance value from reasoning text
fn extract_flow_balance(reasoning: &str) -> Option<f64> {
    // Look for patterns like "flow imbalance: -15.2" or "Flow imbalance -15.2 gpm"
    let patterns = ["flow imbalance", "Flow imbalance", "flow balance"];
    for pattern in patterns {
        if let Some(idx) = reasoning.find(pattern) {
            let after = &reasoning[idx + pattern.len()..];
            // Find the number
            let num_str: String = after
                .chars()
                .skip_while(|c| !c.is_ascii_digit() && *c != '-')
                .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
                .collect();
            if let Ok(val) = num_str.parse::<f64>() {
                return Some(val);
            }
        }
    }
    None
}

/// POST /api/v1/reports/test - Create a test critical report for UI testing
pub async fn create_test_critical_report() -> Json<serde_json::Value> {
    use crate::types::{DrillingPhysicsReport, FinalSeverity, RiskLevel, StrategicReport, SpecialistVote, TicketSeverity};

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let test_report = StrategicReport {
        timestamp,
        efficiency_score: 35,
        risk_level: RiskLevel::Critical,
        severity: FinalSeverity::Critical,
        recommendation: "IMMEDIATE ACTION REQUIRED: Shut in well and initiate well control procedures. Flow imbalance of 85 gpm indicates active kick.".to_string(),
        expected_benefit: "Prevent blowout and protect personnel safety".to_string(),
        reasoning: "Severe flow imbalance detected (85 gpm gain). Pit volume increasing at 25 bbl/hr. Gas units elevated to 450 units. Multiple well control indicators confirm active kick scenario.".to_string(),
        votes: vec![
            SpecialistVote {
                specialist: "WellControl".to_string(),
                vote: TicketSeverity::Critical,
                weight: 0.30,
                reasoning: "CRITICAL: Flow imbalance 85 gpm, pit rate 25 bbl/hr - immediate well control response required".to_string(),
            },
            SpecialistVote {
                specialist: "MSE".to_string(),
                vote: TicketSeverity::High,
                weight: 0.25,
                reasoning: "ROP dropped to 15 m/hr, drilling efficiency severely impacted".to_string(),
            },
            SpecialistVote {
                specialist: "Hydraulic".to_string(),
                vote: TicketSeverity::Critical,
                weight: 0.25,
                reasoning: "SPP dropped 400 psi, ECD margin critical at 0.1 ppg".to_string(),
            },
            SpecialistVote {
                specialist: "Formation".to_string(),
                vote: TicketSeverity::High,
                weight: 0.20,
                reasoning: "Formation fluid influx detected, pore pressure exceeded".to_string(),
            },
        ],
        physics_report: DrillingPhysicsReport {
            current_depth: 3250.5,
            current_rop: 15.2,
            current_wob: 18.5,
            current_rpm: 95.0,
            current_torque: 12.3,
            current_flow_in: 450.0,
            current_flow_out: 535.0,
            current_spp: 2400.0,
            current_mud_weight: 11.8,
            current_ecd: 13.9,
            current_pit_volume: 525.0,
            avg_mse: 950.0,
            mse_efficiency: 45.0,
            ..Default::default()
        },
        context_used: vec!["Historical kick event at similar depth".to_string()],
        trace_log: vec![
            crate::types::TicketEvent {
                timestamp_ms: timestamp * 1000,
                stage: crate::types::TicketStage::WellControlCheck,
                status: crate::types::CheckStatus::Failed,
                message: "Flow imbalance 85 gpm exceeds critical threshold (20 gpm)".to_string(),
            },
            crate::types::TicketEvent {
                timestamp_ms: timestamp * 1000 + 100,
                stage: crate::types::TicketStage::WellControlCheck,
                status: crate::types::CheckStatus::Failed,
                message: "Pit rate 25 bbl/hr exceeds critical threshold (15 bbl/hr)".to_string(),
            },
            crate::types::TicketEvent {
                timestamp_ms: timestamp * 1000 + 200,
                stage: crate::types::TicketStage::FinalDecision,
                status: crate::types::CheckStatus::Failed,
                message: "CONFIRMED: Active kick - immediate response required".to_string(),
            },
        ],
    };

    // Store the test report
    match crate::storage::history::store_report(&test_report) {
        Ok(_) => Json(serde_json::json!({
            "success": true,
            "message": "Test critical report created",
            "timestamp": timestamp
        })),
        Err(e) => Json(serde_json::json!({
            "success": false,
            "error": e.to_string()
        })),
    }
}

// ============================================================================
// Well Configuration Endpoints
// ============================================================================

/// GET /api/v1/config - Return the active well configuration
///
/// Returns the complete WellConfig as JSON including all thresholds,
/// physics parameters, baseline learning settings, and campaign overrides.
pub async fn get_config() -> Json<serde_json::Value> {
    let cfg = crate::config::get();
    match serde_json::to_value(cfg) {
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

    // Store in the app state's acknowledgment log
    let mut app_state = state.app_state.write().await;
    app_state.acknowledgments.push(record.clone());

    // Keep only last 1000 acknowledgments in memory
    if app_state.acknowledgments.len() > 1000 {
        let drain_count = app_state.acknowledgments.len() - 1000;
        app_state.acknowledgments.drain(..drain_count);
    }

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
    Json(app_state.acknowledgments.clone())
}

// ============================================================================
// Shift Summary Endpoint
// ============================================================================

/// Query parameters for shift summary
#[derive(Debug, serde::Deserialize)]
pub struct ShiftSummaryQuery {
    /// Start time (Unix timestamp seconds)
    pub from: Option<u64>,
    /// End time (Unix timestamp seconds)
    pub to: Option<u64>,
    /// Duration in hours (alternative to from/to — last N hours)
    pub hours: Option<f64>,
}

/// Shift summary response
#[derive(Debug, Serialize)]
pub struct ShiftSummaryResponse {
    /// Time range covered
    pub from_timestamp: u64,
    pub to_timestamp: u64,
    pub duration_hours: f64,
    /// Packet statistics
    pub packets_processed: u64,
    pub tickets_created: u64,
    pub tickets_verified: u64,
    pub tickets_rejected: u64,
    /// Advisory breakdown by category
    pub advisories_by_category: std::collections::HashMap<String, u64>,
    /// Peak severity during shift
    pub peak_severity: String,
    /// Average MSE efficiency during shift
    pub avg_mse_efficiency: Option<f64>,
    /// Acknowledgment count during shift
    pub acknowledgments_in_period: usize,
    /// Well name from config
    pub well_name: String,
}

/// GET /api/v1/shift/summary - Get shift summary for a time range
///
/// Query params:
/// - `from` + `to`: Unix timestamps for custom range
/// - `hours`: Alternative — last N hours (default: 12)
pub async fn get_shift_summary(
    State(state): State<DashboardState>,
    axum::extract::Query(query): axum::extract::Query<ShiftSummaryQuery>,
) -> Json<ShiftSummaryResponse> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let (from_ts, to_ts) = if let (Some(from), Some(to)) = (query.from, query.to) {
        (from, to)
    } else {
        let hours = query.hours.unwrap_or(12.0);
        let duration_secs = (hours.max(0.0) * 3600.0) as u64;
        (now.saturating_sub(duration_secs), now)
    };

    let duration_hours = (to_ts.saturating_sub(from_ts)) as f64 / 3600.0;

    let app_state = state.app_state.read().await;

    // Count acknowledgments in the time range
    let acks_in_period = app_state.acknowledgments.iter()
        .filter(|a| a.acknowledged_at >= from_ts && a.acknowledged_at <= to_ts)
        .count();

    let well_name = if crate::config::is_initialized() {
        crate::config::get().well.name.clone()
    } else {
        "DEFAULT".to_string()
    };

    // Build category breakdown from recent history
    let mut by_category: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    if let Some(ref latest) = app_state.latest_advisory {
        // Count from the stored advisory votes as a proxy
        for vote in &latest.votes {
            *by_category.entry(vote.specialist.clone()).or_insert(0) += 1;
        }
    }

    Json(ShiftSummaryResponse {
        from_timestamp: from_ts,
        to_timestamp: to_ts,
        duration_hours,
        packets_processed: app_state.packets_processed,
        tickets_created: app_state.tickets_created,
        tickets_verified: app_state.tickets_verified,
        tickets_rejected: app_state.tickets_rejected,
        advisories_by_category: by_category,
        peak_severity: format!("{:?}", app_state.peak_severity),
        avg_mse_efficiency: app_state.avg_mse_efficiency,
        acknowledgments_in_period: acks_in_period,
        well_name,
    })
}

// ============================================================================
// Prometheus Metrics Endpoint (Item 4.1)
// ============================================================================

/// GET /api/v1/metrics
///
/// Returns runtime counters in Prometheus text format (version 0.0.4).
/// No external crate required — gauges and counters are hand-formatted from
/// `AppState` fields that are already maintained by the processing loop.
///
/// Exposed metrics:
/// - `sairen_packets_total`       — cumulative WITS packets processed
/// - `sairen_tickets_created_total` — advisory tickets generated
/// - `sairen_tickets_verified_total` — tickets confirmed by strategic agent
/// - `sairen_tickets_rejected_total` — tickets rejected as transient
/// - `sairen_uptime_seconds`       — process uptime in seconds
/// - `sairen_avg_mse_efficiency`   — current rolling MSE efficiency (gauge)
pub async fn get_metrics(State(state): State<DashboardState>) -> impl IntoResponse {
    let app_state = state.app_state.read().await;

    let mut body = String::with_capacity(1024);

    body.push_str("# HELP sairen_packets_total Total WITS packets processed\n");
    body.push_str("# TYPE sairen_packets_total counter\n");
    body.push_str(&format!("sairen_packets_total {}\n", app_state.packets_processed));

    body.push_str("# HELP sairen_tickets_created_total Advisory tickets generated\n");
    body.push_str("# TYPE sairen_tickets_created_total counter\n");
    body.push_str(&format!("sairen_tickets_created_total {}\n", app_state.tickets_created));

    body.push_str("# HELP sairen_tickets_verified_total Tickets confirmed by strategic agent\n");
    body.push_str("# TYPE sairen_tickets_verified_total counter\n");
    body.push_str(&format!("sairen_tickets_verified_total {}\n", app_state.tickets_verified));

    body.push_str("# HELP sairen_tickets_rejected_total Tickets rejected as transient\n");
    body.push_str("# TYPE sairen_tickets_rejected_total counter\n");
    body.push_str(&format!("sairen_tickets_rejected_total {}\n", app_state.tickets_rejected));

    body.push_str("# HELP sairen_uptime_seconds Process uptime in seconds\n");
    body.push_str("# TYPE sairen_uptime_seconds gauge\n");
    body.push_str(&format!("sairen_uptime_seconds {}\n", app_state.uptime_secs()));

    if let Some(eff) = app_state.avg_mse_efficiency {
        body.push_str("# HELP sairen_avg_mse_efficiency Rolling average MSE efficiency (0-100)\n");
        body.push_str("# TYPE sairen_avg_mse_efficiency gauge\n");
        body.push_str(&format!("sairen_avg_mse_efficiency {eff:.2}\n"));
    }

    (
        axum::http::StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        body,
    )
}

// ─── Fleet Intelligence ───────────────────────────────────────────────────────

/// GET /api/v1/fleet/intelligence
///
/// Returns locally cached hub intelligence outputs.  The cache is populated by
/// `run_intelligence_sync` from `fleet/sync.rs` and written to
/// `./data/fleet_intelligence.json`.
///
/// Query params:
/// - `?type=benchmark` — filter by output_type
/// - `?formation=Ekofisk` — filter by formation name

pub async fn get_fleet_intelligence(
    Query(params): Query<FleetIntelligenceQuery>,
) -> Json<Vec<crate::fleet::types::IntelligenceOutput>> {
    let path = std::path::Path::new("./data/fleet_intelligence.json");

    let outputs: Vec<crate::fleet::types::IntelligenceOutput> = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let filtered: Vec<_> = outputs
        .into_iter()
        .filter(|o| {
            if let Some(ref t) = params.r#type {
                if o.output_type != *t {
                    return false;
                }
            }
            if let Some(ref f) = params.formation {
                if o.formation_name.as_deref() != Some(f.as_str()) {
                    return false;
                }
            }
            true
        })
        .collect();

    Json(filtered)
}


#[derive(serde::Deserialize)]
pub struct FleetIntelligenceQuery {
    /// Filter by output_type: `benchmark`, `fingerprint`, `report`, `advisory`
    pub r#type: Option<String>,
    /// Filter by formation name
    pub formation: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::AppState;

    fn create_test_state() -> DashboardState {
        DashboardState {
            app_state: Arc::new(RwLock::new(AppState::default())),
            strategic_storage: None,
            threshold_manager: None,
            equipment_id: "RIG".to_string(),
            ml_storage: None,
        }
    }

    #[tokio::test]
    async fn test_health_check() {
        let state = create_test_state();
        let response = legacy_health_check(State(state)).await;
        assert!(response.uptime_seconds >= 0);
    }

    #[tokio::test]
    async fn test_get_status() {
        let state = create_test_state();
        let response = get_status(State(state)).await;
        assert_eq!(response.total_analyses, 0);
    }

    #[tokio::test]
    async fn test_get_health_no_analysis() {
        let state = create_test_state();
        let response = get_health(State(state)).await;
        assert_eq!(response.health_score, 100.0);
        assert_eq!(response.confidence, 0.0);
    }

    #[tokio::test]
    async fn test_get_baseline_not_configured() {
        let state = create_test_state();
        let response = get_baseline_status(State(state)).await;
        assert_eq!(response.overall_status, "Not configured");
        assert_eq!(response.locked_count, 0);
        assert_eq!(response.learning_count, 0);
    }
}
