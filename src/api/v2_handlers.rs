//! v2 API handlers — consistent envelope, typed responses, ISO-8601 timestamps.
//!
//! All handlers return `Response` via [`ApiResponse::ok`] or [`ApiErrorResponse`].

use axum::extract::{Path, Query, State};
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
    ApiResponse::ok((*crate::config::get_arc()).clone())
}

/// POST /api/v2/config — update config (save to disk and hot-reload).
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

    let save_path = crate::config::config_path()
        .cloned()
        .unwrap_or_else(|| std::path::PathBuf::from("well_config.toml"));

    if let Err(e) = request.config.save_to_file(&save_path) {
        return ApiErrorResponse::internal(format!("Failed to save: {e}"));
    }

    // Hot-reload the saved config
    match crate::config::reload() {
        Ok(changes) => {
            let warnings = crate::config::check_non_reloadable(&changes);
            ApiResponse::ok(serde_json::json!({
                "reloaded": true,
                "changes": changes,
                "warnings": warnings,
                "message": format!("{} field(s) updated", changes.len())
            }))
        }
        Err(e) => ApiResponse::ok(serde_json::json!({
            "reloaded": false,
            "message": format!("Config saved but reload failed: {e}. Restart to apply.")
        })),
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

/// POST /api/v2/config/reload — trigger a hot-reload from disk.
pub async fn reload_config() -> Response {
    match crate::config::reload() {
        Ok(changes) => {
            let warnings = crate::config::check_non_reloadable(&changes);
            ApiResponse::ok(serde_json::json!({
                "reloaded": true,
                "changes": changes,
                "warnings": warnings,
                "message": format!("{} field(s) updated", changes.len())
            }))
        }
        Err(e) => ApiErrorResponse::internal(format!("Reload failed: {e}")),
    }
}

// ============================================================================
// Feedback endpoints
// ============================================================================

/// Request body for submitting feedback on an advisory.
#[derive(Debug, Deserialize)]
pub struct SubmitFeedbackRequest {
    pub outcome: crate::storage::feedback::FeedbackOutcome,
    #[serde(default)]
    pub submitted_by: String,
    #[serde(default)]
    pub notes: String,
}

/// POST /api/v2/advisory/feedback/:timestamp — submit operator feedback on an advisory.
pub async fn submit_feedback(
    Path(timestamp): Path<u64>,
    axum::Json(body): axum::Json<SubmitFeedbackRequest>,
) -> Response {
    // Look up the advisory to denormalize its fields
    let report = match crate::storage::history::get_by_timestamp(timestamp) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return ApiErrorResponse::not_found(format!(
                "No advisory found with timestamp {}",
                timestamp
            ));
        }
        Err(e) => {
            return ApiErrorResponse::internal(format!("Storage error: {}", e));
        }
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let record = crate::storage::feedback::FeedbackRecord {
        advisory_timestamp: timestamp,
        outcome: body.outcome,
        category: report.category,
        trigger_parameter: report.trigger_parameter.clone(),
        trigger_value: report.trigger_value,
        threshold_value: report.threshold_value,
        submitted_by: if body.submitted_by.is_empty() {
            "anonymous".to_string()
        } else {
            body.submitted_by
        },
        submitted_at: now,
        notes: body.notes,
    };

    if let Err(e) = crate::storage::feedback::persist(&record) {
        return ApiErrorResponse::internal(format!("Failed to persist feedback: {}", e));
    }

    ApiResponse::ok(record)
}

/// GET /api/v2/advisory/feedback/stats — per-category confirmation rates.
pub async fn feedback_stats() -> Response {
    let records = crate::storage::feedback::load_all();
    let stats = crate::storage::suggestions::compute_stats(&records);
    ApiResponse::ok(stats)
}

/// GET /api/v2/config/suggestions — threshold adjustment suggestions based on feedback.
pub async fn config_suggestions() -> Response {
    let records = crate::storage::feedback::load_all();
    let config = crate::config::get();
    let suggestions = crate::storage::suggestions::compute_suggestions(&records, &config);
    ApiResponse::ok(suggestions)
}

// ============================================================================
// Lookahead endpoint
// ============================================================================

/// Current formation lookahead status.
#[derive(Debug, Serialize)]
pub struct LookaheadStatus {
    pub enabled: bool,
    pub next_formation: Option<String>,
    pub depth_remaining_ft: Option<f64>,
    pub estimated_minutes: Option<f64>,
    pub hazards: Vec<String>,
    pub parameter_changes: Vec<String>,
    pub offset_notes: Option<String>,
}

/// GET /api/v2/lookahead/status — current formation lookahead state.
pub async fn lookahead_status(State(state): State<DashboardState>) -> Response {
    let config = crate::config::get();
    let enabled = config.lookahead.enabled;

    if !enabled {
        return ApiResponse::ok(LookaheadStatus {
            enabled: false,
            next_formation: None,
            depth_remaining_ft: None,
            estimated_minutes: None,
            hazards: Vec::new(),
            parameter_changes: Vec::new(),
            offset_notes: None,
        });
    }

    let app = state.app_state.read().await;
    let (bit_depth, rop) = match &app.latest_wits_packet {
        Some(pkt) => (pkt.bit_depth, pkt.rop),
        None => {
            return ApiResponse::ok(LookaheadStatus {
                enabled: true,
                next_formation: None,
                depth_remaining_ft: None,
                estimated_minutes: None,
                hazards: Vec::new(),
                parameter_changes: Vec::new(),
                offset_notes: None,
            });
        }
    };
    drop(app);

    // Load prognosis (same path as coordinator)
    let kb = crate::knowledge_base::KnowledgeBase::init();
    let prognosis = if let Some(ref kb) = kb {
        kb.prognosis()
    } else {
        crate::types::FormationPrognosis::load()
    };

    let Some(prognosis) = prognosis else {
        return ApiResponse::ok(LookaheadStatus {
            enabled: true,
            next_formation: None,
            depth_remaining_ft: None,
            estimated_minutes: None,
            hazards: Vec::new(),
            parameter_changes: Vec::new(),
            offset_notes: None,
        });
    };

    let formation = prognosis.formation_at_depth(bit_depth);
    let la = formation.and_then(|fm| {
        crate::optimization::look_ahead::check_look_ahead(
            &prognosis,
            bit_depth,
            rop,
            fm,
            config.lookahead.window_minutes,
        )
    });

    match la {
        Some(la) => ApiResponse::ok(LookaheadStatus {
            enabled: true,
            next_formation: Some(la.formation_name),
            depth_remaining_ft: Some(la.depth_remaining_ft),
            estimated_minutes: Some(la.estimated_minutes),
            hazards: la.hazards,
            parameter_changes: la.parameter_changes,
            offset_notes: if la.offset_notes.is_empty() { None } else { Some(la.offset_notes) },
        }),
        None => ApiResponse::ok(LookaheadStatus {
            enabled: true,
            next_formation: None,
            depth_remaining_ft: None,
            estimated_minutes: None,
            hazards: Vec::new(),
            parameter_changes: Vec::new(),
            offset_notes: None,
        }),
    }
}

// ============================================================================
// Damping status endpoint
// ============================================================================

/// Current active damping analysis status.
#[derive(Debug, Serialize)]
pub struct DampingStatus {
    pub enabled: bool,
    pub current_torque_cv: Option<f64>,
    pub oscillation_type: Option<String>,
    pub estimated_frequency_hz: Option<f64>,
    pub severity: Option<f64>,
    pub recommendation: Option<DampingRecommendationResponse>,
    pub monitor: crate::types::DampingMonitorSnapshot,
}

#[derive(Debug, Serialize)]
pub struct DampingRecommendationResponse {
    pub wob_current: f64,
    pub wob_recommended: f64,
    pub wob_change_pct: f64,
    pub rpm_current: f64,
    pub rpm_recommended: f64,
    pub rpm_change_pct: f64,
    pub rationale: String,
}

/// Default (idle) monitor snapshot for when no coordinator data is available.
fn default_monitor_snapshot() -> crate::types::DampingMonitorSnapshot {
    crate::types::DampingMonitorSnapshot {
        active: false,
        baseline_cv: None,
        current_cv: None,
        cv_change_pct: None,
        elapsed_secs: None,
        window_secs: crate::config::get().damping.monitor_window_secs,
        formation_name: None,
        last_outcome: None,
    }
}

/// GET /api/v2/damping/status — current active damping analysis.
pub async fn damping_status(State(state): State<DashboardState>) -> Response {
    let config = crate::config::get();
    let enabled = config.damping.enabled;

    if !enabled {
        return ApiResponse::ok(DampingStatus {
            enabled: false,
            current_torque_cv: None,
            oscillation_type: None,
            estimated_frequency_hz: None,
            severity: None,
            recommendation: None,
            monitor: default_monitor_snapshot(),
        });
    }

    let app = state.app_state.read().await;
    let monitor = app.damping_monitor_snapshot.clone().unwrap_or_else(default_monitor_snapshot);
    let (wob, rpm) = match &app.latest_wits_packet {
        Some(pkt) => (pkt.wob, pkt.rpm),
        None => {
            return ApiResponse::ok(DampingStatus {
                enabled: true,
                current_torque_cv: None,
                oscillation_type: None,
                estimated_frequency_hz: None,
                severity: None,
                recommendation: None,
                monitor,
            });
        }
    };

    // Collect torque values from the WITS packet history buffer
    let torques: Vec<f64> = app
        .wits_history
        .iter()
        .map(|pkt| pkt.torque)
        .collect();
    drop(app);

    if torques.len() < config.damping.min_samples {
        return ApiResponse::ok(DampingStatus {
            enabled: true,
            current_torque_cv: None,
            oscillation_type: None,
            estimated_frequency_hz: None,
            severity: None,
            recommendation: None,
            monitor,
        });
    }

    let analysis = crate::physics_engine::characterize_oscillation(
        &torques,
        config.damping.min_samples,
    );

    match analysis {
        Some(a) => {
            let rec = crate::physics_engine::recommend_damping(
                &a,
                wob,
                rpm,
                config.damping.max_wob_reduction_pct,
                config.damping.max_rpm_change_pct,
            );

            ApiResponse::ok(DampingStatus {
                enabled: true,
                current_torque_cv: Some(a.torque_cv),
                oscillation_type: Some(format!("{:?}", a.oscillation_type)),
                estimated_frequency_hz: Some(a.estimated_frequency_hz),
                severity: Some(a.severity),
                recommendation: rec.map(|r| DampingRecommendationResponse {
                    wob_current: r.current_wob,
                    wob_recommended: r.recommended_wob,
                    wob_change_pct: r.wob_change_pct,
                    rpm_current: r.current_rpm,
                    rpm_recommended: r.recommended_rpm,
                    rpm_change_pct: r.rpm_change_pct,
                    rationale: r.rationale,
                }),
                monitor,
            })
        }
        None => ApiResponse::ok(DampingStatus {
            enabled: true,
            current_torque_cv: None,
            oscillation_type: None,
            estimated_frequency_hz: None,
            severity: None,
            recommendation: None,
            monitor,
        }),
    }
}

// ============================================================================
// Damping recipe endpoints
// ============================================================================

/// Per-formation recipe summary.
#[derive(Debug, Serialize)]
pub struct FormationRecipes {
    pub formation_name: String,
    pub recipe_count: usize,
    pub best_cv_reduction_pct: f64,
    pub recipes: Vec<crate::types::DampingRecipe>,
}

/// Response for recipe listing endpoint.
#[derive(Debug, Serialize)]
pub struct RecipeListResponse {
    pub formations: Vec<FormationRecipes>,
}

/// GET /api/v2/damping/recipes — list all stored formation damping recipes.
pub async fn damping_recipes(
    State(_state): State<DashboardState>,
) -> Response {
    let formations = crate::storage::damping_recipes::list_formations();
    let result: Vec<FormationRecipes> = formations
        .iter()
        .map(|f| {
            let recipes = crate::storage::damping_recipes::get_by_formation(f);
            let best = recipes
                .iter()
                .map(|r| r.cv_reduction_pct)
                .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap_or(0.0);
            FormationRecipes {
                formation_name: f.clone(),
                recipe_count: recipes.len(),
                best_cv_reduction_pct: best,
                recipes,
            }
        })
        .collect();
    ApiResponse::ok(RecipeListResponse { formations: result })
}

// ============================================================================
// Debug endpoints
// ============================================================================

// ============================================================================
// Well debrief endpoints
// ============================================================================

/// POST /api/v2/well/debrief — generate and persist a post-well debrief.
pub async fn generate_debrief_handler(
    State(_state): State<DashboardState>,
) -> Response {
    // Init knowledge base from env vars
    let kb = match crate::knowledge_base::KnowledgeBase::init() {
        Some(kb) => kb,
        None => {
            return ApiErrorResponse::service_unavailable(
                "Knowledge base not configured. Set SAIREN_KB and SAIREN_KB_FIELD env vars.",
            );
        }
    };

    // Generate post-well summary
    let post_well = match kb.complete_well() {
        Ok(summary) => summary,
        Err(e) => {
            return ApiErrorResponse::internal(format!(
                "Failed to generate post-well summary: {}",
                e
            ));
        }
    };

    // Get advisory history from global history DB
    let advisories = crate::storage::history::get_all_reports();

    // Get operator feedback
    let feedback_records = crate::storage::feedback::load_all();

    // Get prognosis
    let prognosis = kb.prognosis();

    // Determine well start timestamp from first advisory
    let well_start_ts = advisories.first().map(|a| a.timestamp).unwrap_or(0);

    // Generate debrief
    let debrief = crate::debrief::generate_debrief(
        &post_well,
        &advisories,
        &feedback_records,
        prognosis.as_ref(),
        well_start_ts,
    );

    // Persist to KB post-well directory
    let post_well_dir = kb.config().post_well_dir(&kb.config().well);
    if let Err(e) = std::fs::create_dir_all(&post_well_dir) {
        tracing::warn!("Failed to create post-well dir: {}", e);
    }

    let debrief_path = post_well_dir.join("debrief.json");
    match serde_json::to_string_pretty(&debrief) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&debrief_path, &json) {
                tracing::warn!("Failed to write debrief.json: {}", e);
            } else {
                tracing::info!(path = ?debrief_path, "Wrote post-well debrief");
            }
        }
        Err(e) => {
            tracing::warn!("Failed to serialize debrief: {}", e);
        }
    }

    ApiResponse::ok(debrief)
}

/// GET /api/v2/well/debrief — read persisted debrief.
pub async fn get_debrief_handler(
    State(_state): State<DashboardState>,
) -> Response {
    let kb = match crate::knowledge_base::KnowledgeBase::init() {
        Some(kb) => kb,
        None => {
            return ApiErrorResponse::service_unavailable(
                "Knowledge base not configured. Set SAIREN_KB and SAIREN_KB_FIELD env vars.",
            );
        }
    };

    let debrief_path = kb.config().post_well_dir(&kb.config().well).join("debrief.json");

    let json = match std::fs::read_to_string(&debrief_path) {
        Ok(s) => s,
        Err(_) => {
            return ApiErrorResponse::not_found(
                "No debrief found. Generate one with POST /api/v2/well/debrief.",
            );
        }
    };

    match serde_json::from_str::<crate::types::WellDebrief>(&json) {
        Ok(debrief) => ApiResponse::ok(debrief),
        Err(e) => ApiErrorResponse::internal(format!("Failed to parse debrief.json: {}", e)),
    }
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
