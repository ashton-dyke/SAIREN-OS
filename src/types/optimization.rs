//! Optimization engine types for proactive drilling parameter recommendations

use serde::{Deserialize, Serialize};

/// Drilling parameters that can be optimized
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DrillingParameter {
    /// Weight on bit (klbs)
    Wob,
    /// Rotary speed (RPM)
    Rpm,
    /// Flow rate (GPM)
    FlowRate,
}

impl std::fmt::Display for DrillingParameter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DrillingParameter::Wob => write!(f, "WOB"),
            DrillingParameter::Rpm => write!(f, "RPM"),
            DrillingParameter::FlowRate => write!(f, "Flow Rate"),
        }
    }
}

/// A single parameter recommendation with current/recommended values and bounds
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterRecommendation {
    /// Which parameter to adjust
    pub parameter: DrillingParameter,
    /// Current measured value
    pub current_value: f64,
    /// Recommended target value
    pub recommended_value: f64,
    /// Safe minimum bound (from formation prognosis)
    pub safe_min: f64,
    /// Safe maximum bound (from formation prognosis)
    pub safe_max: f64,
    /// Expected normalized impact (0.0–1.0, higher = more impactful)
    pub expected_impact: f64,
    /// Evidence string tracing recommendation basis
    pub evidence: String,
}

/// 5-factor weighted confidence breakdown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceBreakdown {
    /// Offset well data quality (0.0–1.0), weight: 30%
    pub offset_wells: f64,
    /// Normalized parameter gap from optimal (0.0–1.0), weight: 25%
    pub parameter_gap: f64,
    /// Trend consistency score (0.0–1.0), weight: 20%
    pub trend_consistency: f64,
    /// Sensor quality score (0.0–1.0), weight: 15%
    pub sensor_quality: f64,
    /// CfC agreement score (0.0–1.0), weight: 10%
    pub cfc_agreement: f64,
}

impl ConfidenceBreakdown {
    /// Compute weighted confidence score (0.0–1.0)
    pub fn compute(&self) -> f64 {
        self.offset_wells * 0.30
            + self.parameter_gap * 0.25
            + self.trend_consistency * 0.20
            + self.sensor_quality * 0.15
            + self.cfc_agreement * 0.10
    }

    /// Confidence as a percentage (0–100)
    pub fn percent(&self) -> u8 {
        (self.compute() * 100.0).round().clamp(0.0, 100.0) as u8
    }
}

/// Look-ahead advisory for upcoming formation transitions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LookAheadAdvisory {
    /// Name of the upcoming formation
    pub formation_name: String,
    /// Estimated minutes until entering the formation
    pub estimated_minutes: f64,
    /// Remaining depth to the formation boundary (ft)
    pub depth_remaining_ft: f64,
    /// Parameter changes recommended for the transition
    pub parameter_changes: Vec<String>,
    /// Known hazards in the upcoming formation
    pub hazards: Vec<String>,
    /// Offset well notes for the upcoming formation
    pub offset_notes: String,
    /// CfC depth-ahead prediction confidence (0.0–1.0), if available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cfc_confidence: Option<f64>,
}

/// Full optimization advisory output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationAdvisory {
    /// Current formation name
    pub formation: String,
    /// Current bit depth (ft)
    pub depth_ft: f64,
    /// Ranked parameter recommendations (highest impact first)
    pub recommendations: Vec<ParameterRecommendation>,
    /// 5-factor confidence breakdown
    pub confidence: ConfidenceBreakdown,
    /// ROP ratio: current_rop / offset_best_rop
    pub rop_ratio: f64,
    /// MSE efficiency percentage (0–100)
    pub mse_efficiency: f64,
    /// Look-ahead advisory (if near a formation boundary)
    pub look_ahead: Option<LookAheadAdvisory>,
    /// Source tag for traceability
    pub source: String,
}

/// Reasons why the optimization engine may skip producing an advisory
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OptimizationSkipReason {
    /// No formation prognosis loaded
    NoPrognosis,
    /// Rig is not currently drilling
    NotDrilling,
    /// CfC anomaly score too high — deferring to incident system
    AnomalyActive,
    /// Rate-limited (recent recommendation still active)
    RateLimited,
    /// Insufficient history buffer entries
    InsufficientHistory,
    /// Computed confidence below threshold
    LowConfidence,
}

impl std::fmt::Display for OptimizationSkipReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoPrognosis => write!(f, "NoPrognosis"),
            Self::NotDrilling => write!(f, "NotDrilling"),
            Self::AnomalyActive => write!(f, "AnomalyActive"),
            Self::RateLimited => write!(f, "RateLimited"),
            Self::InsufficientHistory => write!(f, "InsufficientHistory"),
            Self::LowConfidence => write!(f, "LowConfidence"),
        }
    }
}
