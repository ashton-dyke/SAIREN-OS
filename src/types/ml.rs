//! ML Engine types: ml_quality_thresholds, HourlyDataset, MLInsightsReport, etc.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{Campaign, DrillingMetrics, RigState, WitsPacket};

/// V2: Data quality filter thresholds for ML analysis
pub mod ml_quality_thresholds {
    /// Minimum WOB to consider "drilling" (klbs)
    pub const MIN_WOB: f64 = 5.0;
    /// Minimum RPM to consider "rotating"
    pub const MIN_RPM: f64 = 40.0;
    /// Maximum plausible MSE (psi) - reject sensor glitches
    pub const MAX_PLAUSIBLE_MSE: f64 = 500_000.0;
    /// Minimum plausible MSE (psi)
    pub const MIN_PLAUSIBLE_MSE: f64 = 1_000.0;
    /// Minimum ROP to consider "making hole" (ft/hr)
    pub const MIN_ROP: f64 = 1.0;
    /// Maximum plausible ROP (ft/hr)
    pub const MAX_PLAUSIBLE_ROP: f64 = 500.0;
    /// D-exponent shift threshold for formation boundary (%)
    pub const FORMATION_BOUNDARY_SHIFT: f64 = 0.15;
    /// Minimum samples for high confidence
    pub const HIGH_CONFIDENCE_SAMPLES: usize = 1800;
    /// Minimum samples for medium confidence
    pub const MEDIUM_CONFIDENCE_SAMPLES: usize = 900;
    /// Minimum samples for low confidence
    pub const LOW_CONFIDENCE_SAMPLES: usize = 360;
    /// Minimum samples for any analysis
    pub const MIN_ANALYSIS_SAMPLES: usize = 360;
    /// P-value threshold for statistical significance
    pub const SIGNIFICANCE_THRESHOLD: f64 = 0.05;
}

/// Dataset for ML analysis over a time window
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HourlyDataset {
    /// Raw WITS packets (after quality filtering)
    pub packets: Vec<WitsPacket>,
    /// Computed metrics for each packet
    pub metrics: Vec<DrillingMetrics>,
    /// Analysis window (start_ts, end_ts)
    pub time_range: (u64, u64),
    /// Average depth during window
    pub avg_depth: f64,
    /// Estimated formation type (from d-exponent clustering)
    pub formation_estimate: String,
    /// Active campaign mode
    pub campaign: Campaign,
    /// Breakdown of rig states in window
    pub rig_states_breakdown: HashMap<RigState, usize>,

    // === V2 Additions ===
    /// Well identifier for multi-well storage
    pub well_id: String,
    /// Field/asset name for cross-well queries
    pub field_name: String,
    /// Cumulative bit hours at window start
    pub bit_hours: f64,
    /// Depth drilled on current bit (ft)
    pub bit_depth: f64,
    /// Number of samples rejected by quality filter
    pub rejected_sample_count: usize,
    /// Detected formation segments (if boundary found)
    pub formation_segments: Vec<FormationSegment>,
    /// CfC-detected formation transition timestamps (for dual-source segmentation)
    #[serde(default)]
    pub cfc_transition_timestamps: Vec<u64>,
    /// Regime centroids from CfC motor output clustering (k=4, dim=8)
    #[serde(default)]
    pub regime_centroids: [[f64; 8]; 4],
}

impl Default for HourlyDataset {
    fn default() -> Self {
        Self {
            packets: Vec::new(),
            metrics: Vec::new(),
            time_range: (0, 0),
            avg_depth: 0.0,
            formation_estimate: "Unknown".to_string(),
            campaign: Campaign::Production,
            rig_states_breakdown: HashMap::new(),
            well_id: String::new(),
            field_name: String::new(),
            bit_hours: 0.0,
            bit_depth: 0.0,
            rejected_sample_count: 0,
            formation_segments: Vec::new(),
            cfc_transition_timestamps: Vec::new(),
            regime_centroids: [[0.0; 8]; 4],
        }
    }
}

/// A contiguous segment within a single formation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormationSegment {
    /// Index range in packets vec [start, end)
    pub packet_range: (usize, usize),
    /// Estimated formation type
    pub formation_type: String,
    /// Average d-exponent in segment
    pub avg_d_exponent: f64,
    /// Sample count after quality filtering
    pub valid_sample_count: usize,
}

/// CfC-detected formation transition event.
///
/// Emitted when >= 3 CfC features show surprise > 2.0σ for >= 5 consecutive
/// packets with no active advisory ticket. Complements the d-exponent 15%
/// shift detector as an early indicator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormationTransitionEvent {
    pub timestamp: u64,
    pub bit_depth: f64,
    pub surprised_features: Vec<String>,
    pub packet_index: u64,
}

/// Result of ML analysis - either successful insights or explicit failure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MLInsightsReport {
    pub timestamp: u64,
    pub campaign: Campaign,
    pub depth_range: (f64, f64),

    // === V2: Multi-well identification ===
    pub well_id: String,
    pub field_name: String,

    // === V2: Bit wear context ===
    pub bit_hours: f64,
    pub bit_depth: f64,

    /// Formation analyzed (or "Mixed" if segmented)
    pub formation_type: String,

    /// Analysis result - Success or Failure with reason
    pub result: AnalysisResult,
}

/// Analysis outcome with explicit failure modes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnalysisResult {
    /// Successful analysis with insights
    Success(AnalysisInsights),
    /// Analysis failed - explicit reason for LLM context
    Failure(AnalysisFailure),
}

/// Successful analysis insights
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisInsights {
    /// Optimal drilling parameters (composite-scored)
    pub optimal_params: OptimalParams,
    /// Statistically significant correlations only (p < 0.05)
    pub correlations: Vec<SignificantCorrelation>,
    /// Natural language summary for LLM
    pub summary_text: String,
    /// Overall confidence level
    pub confidence: ConfidenceLevel,
    /// Number of valid samples used
    pub sample_count: usize,
}

/// V2: Explicit failure reasons for LLM context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnalysisFailure {
    /// Less than 360 valid samples
    InsufficientData { valid_samples: usize, required: usize },
    /// Formation changed >15% mid-window, segments too small individually
    UnstableFormation { segment_count: usize, max_segment_size: usize },
    /// No correlations met p < 0.05 threshold
    NoSignificantCorrelation { best_p_value: f64 },
    /// All data rejected by quality filter
    AllDataRejected { rejection_reason: String },
    /// Campaign not suitable for optimization (e.g., Idle state)
    NotApplicable { reason: String },
}

impl std::fmt::Display for AnalysisFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InsufficientData { valid_samples, required } =>
                write!(f, "Insufficient data: {} samples (need {})", valid_samples, required),
            Self::UnstableFormation { segment_count, max_segment_size } =>
                write!(f, "Unstable formation: {} segments, largest has {} samples", segment_count, max_segment_size),
            Self::NoSignificantCorrelation { best_p_value } =>
                write!(f, "No significant correlations (best p={:.3})", best_p_value),
            Self::AllDataRejected { rejection_reason } =>
                write!(f, "All data rejected: {}", rejection_reason),
            Self::NotApplicable { reason } =>
                write!(f, "Analysis not applicable: {}", reason),
        }
    }
}

/// V2: Stricter confidence levels based on sample count
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConfidenceLevel {
    /// >= 1800 samples (30+ min of clean data)
    High,
    /// 900-1799 samples (15-30 min)
    Medium,
    /// 360-899 samples (6-15 min) - use with caution
    Low,
    /// < 360 samples - insufficient for any recommendation
    Insufficient,
}

impl ConfidenceLevel {
    /// Create confidence level from sample count
    pub fn from_sample_count(n: usize) -> Self {
        use ml_quality_thresholds::*;
        match n {
            n if n >= HIGH_CONFIDENCE_SAMPLES => Self::High,
            n if n >= MEDIUM_CONFIDENCE_SAMPLES => Self::Medium,
            n if n >= LOW_CONFIDENCE_SAMPLES => Self::Low,
            _ => Self::Insufficient,
        }
    }

    /// Get display string for confidence level
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::High => "HIGH",
            Self::Medium => "MEDIUM",
            Self::Low => "LOW",
            Self::Insufficient => "INSUFFICIENT",
        }
    }
}

impl std::fmt::Display for ConfidenceLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Optimal drilling parameters from dysfunction-aware binned optimization (V2.2)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimalParams {
    // === Point Estimates (median of winning bin) ===
    pub best_wob: f64,
    pub best_rpm: f64,
    pub best_flow: f64,

    // === Safe Operating Ranges (from winning bin) ===
    /// Minimum safe WOB in the optimal region
    #[serde(default)]
    pub wob_min: f64,
    /// Maximum safe WOB in the optimal region
    #[serde(default)]
    pub wob_max: f64,
    /// Minimum safe RPM in the optimal region
    #[serde(default)]
    pub rpm_min: f64,
    /// Maximum safe RPM in the optimal region
    #[serde(default)]
    pub rpm_max: f64,
    /// Minimum safe flow in the optimal region
    #[serde(default)]
    pub flow_min: f64,
    /// Maximum safe flow in the optimal region
    #[serde(default)]
    pub flow_max: f64,

    // === Performance Metrics ===
    /// ROP achieved at optimal params
    pub achieved_rop: f64,
    /// MSE achieved at optimal params
    pub achieved_mse: f64,
    /// MSE efficiency (0-100%)
    pub mse_efficiency: f64,
    /// Composite efficiency score used for ranking (0-1)
    pub composite_score: f64,
    /// Confidence level (requires 1800+ samples for High)
    pub confidence: ConfidenceLevel,

    // === Stability Metrics (V2.2) ===
    /// Average stability score of samples in winning bin (0-1)
    #[serde(default)]
    pub stability_score: f64,
    /// Number of samples in the winning bin
    #[serde(default)]
    pub bin_sample_count: usize,
    /// Total bins evaluated in grid search
    #[serde(default)]
    pub bins_evaluated: usize,
    /// Whether dysfunction filtering was applied
    #[serde(default)]
    pub dysfunction_filtered: bool,
    /// Regime ID from CfC motor output clustering (None if not partitioned)
    #[serde(default)]
    pub regime_id: Option<u8>,
}

impl Default for OptimalParams {
    fn default() -> Self {
        Self {
            best_wob: 0.0,
            best_rpm: 0.0,
            best_flow: 0.0,
            wob_min: 0.0,
            wob_max: 0.0,
            rpm_min: 0.0,
            rpm_max: 0.0,
            flow_min: 0.0,
            flow_max: 0.0,
            achieved_rop: 0.0,
            achieved_mse: 0.0,
            mse_efficiency: 0.0,
            composite_score: 0.0,
            confidence: ConfidenceLevel::Insufficient,
            stability_score: 0.0,
            bin_sample_count: 0,
            bins_evaluated: 0,
            dysfunction_filtered: false,
            regime_id: None,
        }
    }
}

/// Correlation that passed statistical significance test
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignificantCorrelation {
    pub x_param: String,
    pub y_param: String,
    /// Pearson correlation coefficient (-1 to 1)
    pub r_value: f64,
    /// Coefficient of determination (r²)
    pub r_squared: f64,
    /// V2: p-value for significance testing
    pub p_value: f64,
    /// Sample count used for calculation
    pub sample_count: usize,
}
