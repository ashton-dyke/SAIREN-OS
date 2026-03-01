//! Reports & summaries: strategic, critical, shift

use axum::extract::{Query, State};
use axum::Json;
use serde::Serialize;

use super::DashboardState;

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
    let limit = query.limit.unwrap_or(24).min(1000);

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
    let limit = query.limit.unwrap_or(7).min(1000);

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
        category: crate::types::AnomalyCategory::WellControl,
        trigger_parameter: "flow_balance".to_string(),
        trigger_value: 85.0,
        threshold_value: 20.0,
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
