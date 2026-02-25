//! ML engine endpoints

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

use crate::ml_engine::OptimalFinder;

use super::DashboardState;

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
            .get_well_history(&app_state.field_name, &app_state.well_id, None, 10)
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
    let limit = query.limit.unwrap_or(24).min(1000);
    let campaign = query.campaign.as_ref().and_then(|c| crate::types::Campaign::from_str(c));

    if let Some(storage) = &state.ml_storage {
        let history: Vec<MLReportSummary> = storage
            .get_well_history(&app_state.field_name, &app_state.well_id, campaign, limit)
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
        match storage.find_by_depth(&app_state.field_name, &app_state.well_id, depth, 500.0, 5) {
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
