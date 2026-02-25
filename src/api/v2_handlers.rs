//! v2 API handlers — consistent envelope, typed responses, ISO-8601 timestamps.
//!
//! All handlers return `Response` via [`ApiResponse::ok`] or [`ApiErrorResponse`].

use axum::extract::{Query, State};
use axum::response::Response;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use axum::response::IntoResponse;

use super::envelope::{ApiErrorResponse, ApiResponse};
use super::handlers::DashboardState;
use crate::baseline::{wits_metrics, LearningStatus};
use crate::ml_engine::OptimalFinder;

// ============================================================================
// Response types
// ============================================================================

/// Component-level health for `/api/v2/system/health`.
#[derive(Debug, Serialize)]
pub struct HealthV2 {
    pub overall_score: f64,
    pub severity: String,
    pub diagnosis: String,
    pub recommendation: String,
    pub confidence: f64,
    pub timestamp: DateTime<Utc>,
    pub components: ComponentHealth,
}

#[derive(Debug, Serialize)]
pub struct ComponentHealth {
    pub pipeline: bool,
    pub baseline: String,
    pub ml: bool,
    pub fleet: bool,
    pub storage: bool,
}

/// System status fields for the consolidated live endpoint.
#[derive(Debug, Serialize)]
pub struct StatusV2 {
    pub system_status: String,
    pub rig_state: String,
    pub operation: String,
    pub operation_code: String,
    pub uptime_secs: u64,
    pub total_analyses: u64,
    pub packets_processed: u64,
    pub campaign: String,
    pub campaign_code: String,
}

/// Drilling metrics for v2.
#[derive(Debug, Serialize)]
pub struct DrillingV2 {
    pub bit_depth: f64,
    pub rop: f64,
    pub wob: f64,
    pub rpm: f64,
    pub torque: f64,
    pub spp: f64,
    pub hook_load: f64,
    pub flow_in: f64,
    pub flow_out: f64,
    pub flow_balance: f64,
    pub pit_volume: f64,
    pub mud_weight: f64,
    pub ecd: f64,
    pub ecd_margin: f64,
    pub gas_units: f64,
    pub mse: f64,
    pub mse_efficiency: f64,
    pub mse_baseline: f64,
    pub d_exponent: f64,
    pub dxc: f64,
    pub formation_type: String,
    pub formation_change: bool,
    pub trend: String,
    pub votes: Option<SpecialistVotesV2>,
}

#[derive(Debug, Serialize)]
pub struct SpecialistVotesV2 {
    pub mse: String,
    pub hydraulic: String,
    pub well_control: String,
    pub formation: String,
}

/// Verification status.
#[derive(Debug, Serialize)]
pub struct VerificationV2 {
    pub has_verification: bool,
    pub status: Option<String>,
    pub suspected_fault: Option<String>,
    pub reasoning: Option<String>,
    pub final_severity: Option<String>,
    pub verified_count: u64,
    pub rejected_count: u64,
}

/// Baseline summary for live endpoint.
#[derive(Debug, Serialize)]
pub struct BaselineSummaryV2 {
    pub overall_status: String,
    pub locked_count: usize,
    pub learning_count: usize,
    pub total_metrics: usize,
}

/// ML summary for live endpoint.
#[derive(Debug, Serialize)]
pub struct MLSummaryV2 {
    pub has_data: bool,
    pub timestamp: Option<u64>,
    pub confidence: Option<String>,
    pub composite_score: Option<f64>,
    pub best_wob: Option<f64>,
    pub best_rpm: Option<f64>,
    pub best_flow: Option<f64>,
}

/// Shift summary for live endpoint.
#[derive(Debug, Serialize)]
pub struct ShiftSummaryV2 {
    pub duration_hours: f64,
    pub packets_processed: u64,
    pub tickets_created: u64,
    pub tickets_verified: u64,
    pub tickets_rejected: u64,
    pub peak_severity: String,
    pub avg_mse_efficiency: Option<f64>,
    pub acknowledgments: usize,
}

/// Consolidated live data — replaces 7 independent polling intervals.
#[derive(Debug, Serialize)]
pub struct LiveDataResponse {
    pub health: HealthV2,
    pub status: StatusV2,
    pub drilling: DrillingV2,
    pub verification: VerificationV2,
    pub baseline_summary: BaselineSummaryV2,
    pub ml_latest: Option<MLSummaryV2>,
    pub shift: ShiftSummaryV2,
}

// ============================================================================
// Query types
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct LimitQuery {
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct DepthQuery {
    #[serde(default)]
    pub depth: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct ShiftQuery {
    #[serde(default)]
    pub hours: Option<f64>,
}

// ============================================================================
// Internal helpers
// ============================================================================

fn build_health(state: &crate::pipeline::AppState, dashboard: &DashboardState) -> HealthV2 {
    let (score, severity, diagnosis, recommendation, confidence) =
        match &state.latest_advisory {
            Some(adv) => (
                adv.efficiency_score as f64,
                format!("{:?}", adv.severity),
                adv.reasoning.clone(),
                adv.recommendation.clone(),
                0.85,
            ),
            None => (
                100.0,
                "Healthy".to_string(),
                "System initializing, collecting baseline data.".to_string(),
                "Wait for learning phase to complete.".to_string(),
                0.0,
            ),
        };

    let baseline_status = dashboard
        .threshold_manager
        .as_ref()
        .and_then(|m| m.read().ok())
        .map(|mgr| {
            if mgr.locked_count() > 0 {
                "locked"
            } else {
                "learning"
            }
        })
        .unwrap_or("unavailable")
        .to_string();

    let has_ml = state.latest_ml_report.is_some();
    let has_fleet = std::env::var("FLEET_HUB_URL").is_ok();

    HealthV2 {
        overall_score: score,
        severity,
        diagnosis,
        recommendation,
        confidence,
        timestamp: Utc::now(),
        components: ComponentHealth {
            pipeline: true,
            baseline: baseline_status,
            ml: has_ml,
            fleet: has_fleet,
            storage: true,
        },
    }
}

fn build_status(state: &crate::pipeline::AppState) -> StatusV2 {
    let (rig_state, operation, operation_code) = match (&state.latest_wits_packet, &state.latest_drilling_metrics) {
        (Some(pkt), Some(metrics)) => (
            format!("{:?}", pkt.rig_state),
            metrics.operation.display_name().to_string(),
            metrics.operation.short_code().to_string(),
        ),
        (Some(pkt), None) => (format!("{:?}", pkt.rig_state), "Static".to_string(), "STATIC".to_string()),
        _ => ("Unknown".to_string(), "Static".to_string(), "STATIC".to_string()),
    };

    StatusV2 {
        system_status: format!("{:?}", state.status),
        rig_state,
        operation,
        operation_code,
        uptime_secs: state.uptime_secs(),
        total_analyses: state.total_analyses,
        packets_processed: state.packets_processed,
        campaign: state.campaign.display_name().to_string(),
        campaign_code: state.campaign.short_code().to_string(),
    }
}

fn build_drilling(state: &crate::pipeline::AppState, dashboard: &DashboardState) -> DrillingV2 {
    let (bit_depth, rop, wob, rpm, torque, spp, hook_load, flow_in, flow_out, pit_volume, mud_weight, ecd, gas_units, ecd_margin) =
        match &state.latest_wits_packet {
            Some(pkt) => (
                pkt.bit_depth, pkt.rop, pkt.wob, pkt.rpm, pkt.torque,
                pkt.spp, pkt.hook_load, pkt.flow_in, pkt.flow_out,
                pkt.pit_volume, pkt.mud_weight_in, pkt.ecd, pkt.gas_units,
                pkt.ecd_margin(),
            ),
            None => (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
        };

    let flow_balance = flow_out - flow_in;

    let (mse, mse_efficiency, mse_delta, d_exponent, dxc) = match &state.latest_drilling_metrics {
        Some(m) => (m.mse, m.mse_efficiency, m.mse_delta_percent, m.d_exponent, m.dxc),
        None => (0.0, 100.0, 0.0, 1.0, 1.0),
    };

    let mse_baseline = dashboard
        .threshold_manager
        .as_ref()
        .and_then(|mgr| mgr.read().ok())
        .map(|mgr| {
            mgr.get_threshold(&dashboard.equipment_id, wits_metrics::MSE)
                .map(|t| t.baseline_mean)
                .unwrap_or(0.0)
        })
        .unwrap_or(0.0);

    let mse_baseline_display = if mse_baseline > 0.0 {
        mse_baseline
    } else if mse > 0.0 {
        mse * 0.85
    } else {
        0.0
    };

    // Formation transition check
    let formation_change = state.latest_formation_transition.as_ref().map_or(false, |event| {
        let latest_ts = state.latest_wits_packet.as_ref().map(|p| p.timestamp).unwrap_or(0);
        latest_ts.saturating_sub(event.timestamp) <= 60
    });

    let votes = state.latest_advisory.as_ref().map(|adv| {
        let mut mv = "--".to_string();
        let mut hv = "--".to_string();
        let mut wv = "--".to_string();
        let mut fv = "--".to_string();
        for v in &adv.votes {
            let vs = format!("{:?}", v.vote);
            match v.specialist.to_lowercase().as_str() {
                "mse" => mv = vs,
                "hydraulic" => hv = vs,
                "wellcontrol" | "well_control" => wv = vs,
                "formation" => fv = vs,
                _ => {}
            }
        }
        SpecialistVotesV2 { mse: mv, hydraulic: hv, well_control: wv, formation: fv }
    });

    DrillingV2 {
        bit_depth, rop, wob, rpm, torque, spp, hook_load,
        flow_in, flow_out, flow_balance, pit_volume, mud_weight,
        ecd, ecd_margin, gas_units, mse, mse_efficiency,
        mse_baseline: mse_baseline_display,
        d_exponent, dxc,
        formation_type: "Normal".to_string(),
        formation_change,
        trend: if mse_delta.abs() < 0.05 { "Stable" } else if mse_delta > 0.0 { "Increasing" } else { "Decreasing" }.to_string(),
        votes,
    }
}

fn build_verification(state: &crate::pipeline::AppState) -> VerificationV2 {
    match &state.latest_verification {
        Some(v) => VerificationV2 {
            has_verification: true,
            status: Some(v.status.to_string()),
            suspected_fault: Some(v.ticket.description.clone()),
            reasoning: Some(v.reasoning.clone()),
            final_severity: Some(format!("{:?}", v.final_severity)),
            verified_count: state.verified_faults,
            rejected_count: state.rejected_faults,
        },
        None => VerificationV2 {
            has_verification: false,
            status: None,
            suspected_fault: None,
            reasoning: None,
            final_severity: None,
            verified_count: state.verified_faults,
            rejected_count: state.rejected_faults,
        },
    }
}

fn build_baseline_summary(dashboard: &DashboardState) -> BaselineSummaryV2 {
    let metrics_to_check = [
        wits_metrics::MSE, wits_metrics::D_EXPONENT, wits_metrics::DXC,
        wits_metrics::FLOW_BALANCE, wits_metrics::SPP, wits_metrics::TORQUE,
        wits_metrics::ROP, wits_metrics::WOB, wits_metrics::RPM,
        wits_metrics::ECD, wits_metrics::PIT_VOLUME, wits_metrics::GAS_UNITS,
    ];

    let mgr = match &dashboard.threshold_manager {
        Some(m) => match m.read() {
            Ok(g) => Some(g),
            Err(e) => Some(e.into_inner()),
        },
        None => None,
    };

    let (mut locked, mut learning) = (0usize, 0usize);
    if let Some(ref mgr) = mgr {
        for metric_id in metrics_to_check {
            if let Some(status) = mgr.get_status(&dashboard.equipment_id, metric_id) {
                match status {
                    LearningStatus::Locked { .. } => locked += 1,
                    _ => learning += 1,
                }
            }
        }
    }

    let overall = if locked == metrics_to_check.len() {
        "All baselines locked"
    } else if learning > 0 || locked > 0 {
        "Learning in progress"
    } else {
        "Not started"
    };

    BaselineSummaryV2 {
        overall_status: overall.to_string(),
        locked_count: locked,
        learning_count: learning,
        total_metrics: metrics_to_check.len(),
    }
}

fn build_ml_summary(state: &crate::pipeline::AppState) -> Option<MLSummaryV2> {
    let report = state.latest_ml_report.as_ref()?;
    match &report.result {
        crate::types::AnalysisResult::Success(insights) => Some(MLSummaryV2 {
            has_data: true,
            timestamp: Some(report.timestamp),
            confidence: Some(insights.confidence.to_string()),
            composite_score: Some(insights.optimal_params.composite_score),
            best_wob: Some(insights.optimal_params.best_wob),
            best_rpm: Some(insights.optimal_params.best_rpm),
            best_flow: Some(insights.optimal_params.best_flow),
        }),
        crate::types::AnalysisResult::Failure(_) => Some(MLSummaryV2 {
            has_data: false,
            timestamp: Some(report.timestamp),
            confidence: None,
            composite_score: None,
            best_wob: None,
            best_rpm: None,
            best_flow: None,
        }),
    }
}

fn build_shift(state: &crate::pipeline::AppState) -> ShiftSummaryV2 {
    ShiftSummaryV2 {
        duration_hours: state.uptime_secs() as f64 / 3600.0,
        packets_processed: state.packets_processed,
        tickets_created: state.tickets_created,
        tickets_verified: state.tickets_verified,
        tickets_rejected: state.tickets_rejected,
        peak_severity: format!("{:?}", state.peak_severity),
        avg_mse_efficiency: state.avg_mse_efficiency,
        acknowledgments: state.acknowledgments.len(),
    }
}

// ============================================================================
// Handlers
// ============================================================================

/// GET /api/v2/system/health
pub async fn system_health(State(state): State<DashboardState>) -> Response {
    let app = state.app_state.read().await;
    ApiResponse::ok(build_health(&app, &state))
}

/// GET /api/v2/live — consolidated endpoint replacing 7 v1 polls.
pub async fn live_data(State(state): State<DashboardState>) -> Response {
    let app = state.app_state.read().await;
    let response = LiveDataResponse {
        health: build_health(&app, &state),
        status: build_status(&app),
        drilling: build_drilling(&app, &state),
        verification: build_verification(&app),
        baseline_summary: build_baseline_summary(&state),
        ml_latest: build_ml_summary(&app),
        shift: build_shift(&app),
    };
    ApiResponse::ok(response)
}

/// GET /api/v2/drilling
pub async fn drilling(State(state): State<DashboardState>) -> Response {
    let app = state.app_state.read().await;
    ApiResponse::ok(build_drilling(&app, &state))
}

/// GET /api/v2/reports/hourly?limit=24
pub async fn reports_hourly(
    State(state): State<DashboardState>,
    Query(q): Query<LimitQuery>,
) -> Response {
    let limit = q.limit.unwrap_or(24).min(1000);
    if let Some(storage) = &state.strategic_storage {
        match storage.get_hourly(limit) {
            Ok(reports) => {
                let items: Vec<super::handlers::HourlyReportResponse> =
                    reports.into_iter().map(Into::into).collect();
                ApiResponse::ok(items)
            }
            Err(e) => ApiErrorResponse::internal(format!("Storage error: {e}")),
        }
    } else {
        ApiResponse::ok(Vec::<super::handlers::HourlyReportResponse>::new())
    }
}

/// GET /api/v2/reports/daily?limit=7
pub async fn reports_daily(
    State(state): State<DashboardState>,
    Query(q): Query<LimitQuery>,
) -> Response {
    let limit = q.limit.unwrap_or(7).min(1000);
    if let Some(storage) = &state.strategic_storage {
        match storage.get_daily(limit) {
            Ok(reports) => {
                let items: Vec<super::handlers::DailyReportResponse> =
                    reports.into_iter().map(Into::into).collect();
                ApiResponse::ok(items)
            }
            Err(e) => ApiErrorResponse::internal(format!("Storage error: {e}")),
        }
    } else {
        ApiResponse::ok(Vec::<super::handlers::DailyReportResponse>::new())
    }
}

/// GET /api/v2/reports/critical?limit=50
pub async fn reports_critical(
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let limit: usize = params.get("limit").and_then(|s| s.parse().ok()).unwrap_or(50).min(1000);
    let reports = crate::storage::history::get_critical_reports(limit);

    let entries: Vec<super::handlers::CriticalReportEntry> = reports
        .into_iter()
        .map(|report| {
            let physics = &report.physics_report;
            let sig_content = format!(
                "{}:{}:{}:{}:{}",
                report.timestamp, report.efficiency_score, report.recommendation,
                physics.current_depth, physics.current_rop
            );
            let digital_signature = format!("MD5-{:x}", md5::compute(sig_content.as_bytes()));

            let dt = chrono::DateTime::from_timestamp(report.timestamp as i64, 0).unwrap_or_default();
            let timestamp_formatted = dt.to_rfc3339();
            let signature_timestamp = Utc::now().to_rfc3339();
            let report_id = format!("RPT-{}", report.timestamp);

            let (trigger_param, trigger_val, threshold_val) =
                super::handlers::extract_trigger_info(&report);

            let votes_summary: Vec<String> = report.votes.iter()
                .map(|v| format!("{} ({}%): {:?}", v.specialist, (v.weight * 100.0).clamp(0.0, 100.0) as u8, v.vote))
                .collect();

            super::handlers::CriticalReportEntry {
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
                drilling_params: super::handlers::CriticalDrillingParams {
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

    ApiResponse::ok(entries)
}

/// GET /api/v2/ml/latest
pub async fn ml_latest(State(state): State<DashboardState>) -> Response {
    let app = state.app_state.read().await;
    let latest = app.latest_ml_report.as_ref().map(super::handlers::MLReportSummary::from);

    if let Some(storage) = &state.ml_storage {
        let history: Vec<super::handlers::MLReportSummary> = storage
            .get_well_history(&app.field_name, &app.well_id, None, 10)
            .ok()
            .unwrap_or_default()
            .iter()
            .map(super::handlers::MLReportSummary::from)
            .collect();

        let total_count = storage.count();
        let successful_count = storage.count_successful().unwrap_or(0);

        ApiResponse::ok(super::handlers::MLInsightsResponse {
            has_data: latest.is_some() || !history.is_empty(),
            latest,
            history,
            total_count,
            successful_count,
        })
    } else {
        ApiResponse::ok(super::handlers::MLInsightsResponse {
            has_data: latest.is_some(),
            latest,
            history: Vec::new(),
            total_count: 0,
            successful_count: 0,
        })
    }
}

/// GET /api/v2/ml/optimal?depth=X
pub async fn ml_optimal(
    State(state): State<DashboardState>,
    Query(q): Query<DepthQuery>,
) -> Response {
    let app = state.app_state.read().await;

    let depth = q.depth.unwrap_or_else(|| {
        app.latest_wits_packet.as_ref().map(|p| p.bit_depth).unwrap_or(0.0)
    });

    let Some(storage) = &state.ml_storage else {
        return ApiErrorResponse::service_unavailable("ML storage not available");
    };

    match storage.find_by_depth(&app.field_name, &app.well_id, depth, 500.0, 5) {
        Ok(reports) if !reports.is_empty() => {
            for report in &reports {
                if let crate::types::AnalysisResult::Success(insights) = &report.result {
                    return ApiResponse::ok(super::handlers::MLOptimalParams {
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
                    });
                }
            }
            ApiErrorResponse::not_found("No successful analysis for this depth")
        }
        Ok(_) => ApiErrorResponse::not_found("No ML data for this depth"),
        Err(e) => ApiErrorResponse::internal(format!("Storage error: {e}")),
    }
}

/// GET /api/v2/config — returns typed WellConfig.
pub async fn get_config() -> Response {
    ApiResponse::ok(crate::config::get().clone())
}

/// POST /api/v2/config — update config (save to disk).
pub async fn update_config(
    axum::Json(request): axum::Json<super::handlers::UpdateConfigRequest>,
) -> Response {
    match request.config.validate() {
        Ok(()) => {}
        Err(crate::config::ConfigError::Validation(errors)) => {
            return ApiErrorResponse::bad_request(
                format!("Validation failed: {}", errors.join("; ")),
            );
        }
        Err(e) => {
            return ApiErrorResponse::bad_request(format!("Validation error: {e}"));
        }
    }

    let save_path = std::path::PathBuf::from("well_config.toml");
    match request.config.save_to_file(&save_path) {
        Ok(()) => ApiResponse::ok(serde_json::json!({
            "success": true,
            "message": "Config saved. Restart SAIREN-OS to apply."
        })),
        Err(e) => ApiErrorResponse::internal(format!("Failed to save: {e}")),
    }
}

/// POST /api/v2/config/validate
pub async fn validate_config(
    axum::Json(request): axum::Json<super::handlers::UpdateConfigRequest>,
) -> Response {
    match request.config.validate() {
        Ok(()) => ApiResponse::ok(serde_json::json!({
            "valid": true,
            "message": "Configuration is valid"
        })),
        Err(crate::config::ConfigError::Validation(errors)) => {
            ApiResponse::ok(serde_json::json!({
                "valid": false,
                "errors": errors
            }))
        }
        Err(e) => ApiErrorResponse::bad_request(format!("Validation error: {e}")),
    }
}

/// GET /api/v2/campaign
pub async fn get_campaign(State(state): State<DashboardState>) -> Response {
    let app = state.app_state.read().await;
    let thresholds = &app.campaign_thresholds;

    ApiResponse::ok(serde_json::json!({
        "campaign": app.campaign.display_name(),
        "code": app.campaign.short_code(),
        "thresholds": {
            "mse_efficiency_warning": thresholds.mse_efficiency_warning,
            "flow_imbalance_warning": thresholds.flow_imbalance_warning,
            "flow_imbalance_critical": thresholds.flow_imbalance_critical,
            "weights": {
                "mse": thresholds.weight_mse,
                "hydraulic": thresholds.weight_hydraulic,
                "well_control": thresholds.weight_well_control,
                "formation": thresholds.weight_formation,
            }
        }
    }))
}

/// POST /api/v2/campaign
pub async fn set_campaign(
    State(state): State<DashboardState>,
    axum::Json(request): axum::Json<super::handlers::SetCampaignRequest>,
) -> Response {
    let campaign = match crate::types::Campaign::from_str(&request.campaign) {
        Some(c) => c,
        None => {
            return ApiErrorResponse::bad_request(
                "Invalid campaign type. Use 'production' or 'p&a'.",
            );
        }
    };

    let mut app = state.app_state.write().await;
    app.set_campaign(campaign);

    ApiResponse::ok(serde_json::json!({
        "campaign": campaign.display_name(),
        "code": campaign.short_code(),
        "message": format!("Campaign switched to {}.", campaign.display_name())
    }))
}

/// POST /api/v2/advisory/acknowledge
pub async fn acknowledge_advisory(
    State(state): State<DashboardState>,
    axum::Json(request): axum::Json<super::handlers::AcknowledgeRequest>,
) -> Response {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let record = super::handlers::AcknowledgmentRecord {
        ticket_timestamp: request.ticket_timestamp,
        acknowledged_by: request.acknowledged_by,
        acknowledged_at: now,
        notes: request.notes,
        action_taken: request.action_taken,
    };

    if let Err(e) = crate::storage::acks::persist(record.acknowledged_at, &record) {
        tracing::warn!("Failed to persist acknowledgment: {}", e);
    }

    let mut app = state.app_state.write().await;
    if app.acknowledgments.len() >= crate::pipeline::MAX_ACKNOWLEDGMENTS {
        app.acknowledgments.pop_front();
    }
    app.acknowledgments.push_back(record.clone());

    ApiResponse::ok(record)
}

/// GET /api/v2/advisory/acknowledgments
pub async fn get_acknowledgments(State(state): State<DashboardState>) -> Response {
    let app = state.app_state.read().await;
    ApiResponse::ok(app.acknowledgments.clone())
}

/// GET /api/v2/shift/summary?hours=12
pub async fn shift_summary(
    State(state): State<DashboardState>,
    Query(q): Query<ShiftQuery>,
) -> Response {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let hours = q.hours.unwrap_or(12.0);
    let duration_secs = (hours.max(0.0) * 3600.0) as u64;
    let from_ts = now.saturating_sub(duration_secs);

    let app = state.app_state.read().await;
    let acks_in_period = app.acknowledgments.iter()
        .filter(|a| a.acknowledged_at >= from_ts && a.acknowledged_at <= now)
        .count();

    let well_name = if crate::config::is_initialized() {
        crate::config::get().well.name.clone()
    } else {
        "DEFAULT".to_string()
    };

    ApiResponse::ok(serde_json::json!({
        "from_timestamp": from_ts,
        "to_timestamp": now,
        "duration_hours": hours,
        "packets_processed": app.packets_processed,
        "tickets_created": app.tickets_created,
        "tickets_verified": app.tickets_verified,
        "tickets_rejected": app.tickets_rejected,
        "peak_severity": format!("{:?}", app.peak_severity),
        "avg_mse_efficiency": app.avg_mse_efficiency,
        "acknowledgments_in_period": acks_in_period,
        "well_name": well_name,
    }))
}

// ============================================================================
// Debug endpoints
// ============================================================================

/// GET /api/v2/debug/baseline — full per-metric Welford internals.
pub async fn debug_baseline(State(state): State<DashboardState>) -> Response {
    // Re-use the v1 handler logic but wrap in envelope
    let app = state.app_state.read().await;
    drop(app); // Release the lock before calling the handler logic
    let resp = super::handlers::get_baseline_status(State(state)).await;
    ApiResponse::ok(resp.0)
}

/// GET /api/v2/debug/ml/history
pub async fn debug_ml_history(
    State(state): State<DashboardState>,
    Query(q): Query<LimitQuery>,
) -> Response {
    let limit = q.limit.unwrap_or(24).min(1000);
    let app = state.app_state.read().await;
    if let Some(storage) = &state.ml_storage {
        let history: Vec<super::handlers::MLReportSummary> = storage
            .get_well_history(&app.field_name, &app.well_id, None, limit)
            .ok()
            .unwrap_or_default()
            .iter()
            .map(super::handlers::MLReportSummary::from)
            .collect();
        ApiResponse::ok(history)
    } else {
        ApiResponse::ok(Vec::<super::handlers::MLReportSummary>::new())
    }
}

/// GET /api/v2/debug/fleet/intelligence
pub async fn debug_fleet_intelligence(
    Query(params): Query<super::handlers::FleetIntelligenceQuery>,
) -> Response {
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

    ApiResponse::ok(filtered)
}

/// GET /api/v2/metrics — Prometheus text format (unchanged from v1).
pub async fn metrics(State(state): State<DashboardState>) -> Response {
    super::handlers::get_metrics(State(state)).await.into_response()
}
