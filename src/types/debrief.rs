//! Post-well debrief types

use serde::{Deserialize, Serialize};

use super::AnomalyCategory;

/// Complete post-well debrief report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WellDebrief {
    pub well_id: String,
    pub field: String,
    /// Unix timestamp when the debrief was generated
    pub generated_at: u64,
    /// Unix timestamp of the first advisory (well start proxy)
    pub well_start_ts: u64,
    /// Unix timestamp of debrief generation (well end proxy)
    pub well_end_ts: u64,
    pub total_depth_ft: f64,
    pub total_bit_hours: f64,
    pub timeline: Vec<TimelineEvent>,
    pub formation_comparisons: Vec<FormationComparison>,
    pub feedback_summary: FeedbackSummary,
    pub narrative: String,
}

/// A single event on the well timeline (advisory correlated with depth/formation)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEvent {
    pub timestamp: u64,
    pub depth_ft: f64,
    pub formation_name: Option<String>,
    pub category: AnomalyCategory,
    pub severity: String,
    pub recommendation: String,
    /// "confirmed" / "false_positive" / "unclear" / None
    pub feedback: Option<String>,
}

/// Planned vs actual comparison for a single formation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormationComparison {
    pub formation_name: String,
    pub depth_top_ft: f64,
    pub depth_base_ft: f64,
    // Planned (from prognosis)
    pub planned_wob_klbs: Option<f64>,
    pub planned_rpm: Option<f64>,
    pub planned_rop_ft_hr: Option<f64>,
    // Actual (from post-well performance)
    pub actual_avg_rop_ft_hr: f64,
    pub actual_best_rop_ft_hr: f64,
    pub actual_avg_mse_psi: f64,
    pub actual_best_wob_klbs: f64,
    pub actual_best_rpm: f64,
    // Delta
    pub rop_delta_pct: Option<f64>,
    // Advisory counts in this formation's depth range
    pub advisory_count: usize,
    pub critical_count: usize,
    pub mechanical_count: usize,
    /// "exceeded_plan", "met_plan", or "below_plan"
    pub assessment: String,
}

/// Summary of operator feedback across the well
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackSummary {
    pub total_advisories: usize,
    pub total_feedback: usize,
    pub confirmed: usize,
    pub false_positives: usize,
    pub unclear: usize,
    pub confirmation_rate: f64,
    pub category_rates: Vec<CategoryFeedbackRate>,
}

/// Per-category feedback rate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryFeedbackRate {
    pub category: AnomalyCategory,
    pub total: usize,
    pub confirmed: usize,
    pub false_positives: usize,
    pub confirmation_rate: f64,
}
