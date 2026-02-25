//! Drilling operations: metrics, verification, campaign management

use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::baseline::wits_metrics;

use super::DashboardState;

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
