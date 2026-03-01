//! Advisory types: HistoryEntry, RiskLevel, DrillingPhysicsReport, StrategicAdvisory,
//! SpecialistVote, FinalSeverity

use serde::{Deserialize, Serialize};

use super::{AnomalyCategory, DrillingMetrics, RiskLevel, TicketEvent, TicketSeverity, WitsPacket};

// ============================================================================
// Phase 4: History Buffer
// ============================================================================

/// Wrapper for WITS packet with computed metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub packet: WitsPacket,
    pub metrics: DrillingMetrics,
}

// ============================================================================
// Phase 5: Advanced Physics (Strategic)
// ============================================================================

/// Drilling physics calculations report (Phase 5)
///
/// These calculations run when an advisory ticket is created:
/// - MSE trend analysis
/// - D-exponent trend for pore pressure tracking
/// - Flow balance trend for kick/loss detection
/// - Formation hardness estimation
/// - WOB/ROP trend for founder detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrillingPhysicsReport {
    /// Average MSE over analysis window (psi)
    pub avg_mse: f64,
    /// MSE trend (positive = increasing, negative = decreasing)
    pub mse_trend: f64,
    /// Optimal MSE for current formation (psi)
    pub optimal_mse: f64,
    /// MSE efficiency score (0-100%)
    pub mse_efficiency: f64,
    /// D-exponent trend (positive = hardening/pressure increase)
    pub dxc_trend: f64,
    /// Flow balance trend (positive = increasing gain)
    pub flow_balance_trend: f64,
    /// Average pit rate over window (bbl/hr)
    pub avg_pit_rate: f64,
    /// Estimated formation hardness (relative scale 0-10)
    pub formation_hardness: f64,
    /// Confidence level of the calculations
    pub confidence: f64,
    /// Detected drilling dysfunctions
    pub detected_dysfunctions: Vec<String>,
    // === Founder Detection Fields (Phase 5 enhancement) ===
    /// WOB trend (positive = increasing weight)
    #[serde(default)]
    pub wob_trend: f64,
    /// ROP trend (positive = increasing penetration rate)
    #[serde(default)]
    pub rop_trend: f64,
    /// Whether founder condition is detected
    #[serde(default)]
    pub founder_detected: bool,
    /// Founder severity (0.0-1.0)
    #[serde(default)]
    pub founder_severity: f64,
    /// Estimated optimal WOB (klbs) - where ROP was highest
    #[serde(default)]
    pub optimal_wob_estimate: f64,
    // Snapshot of current drilling parameters for LLM prompt
    /// Current bit depth (ft)
    pub current_depth: f64,
    /// Current ROP (ft/hr)
    pub current_rop: f64,
    /// Current WOB (klbs)
    pub current_wob: f64,
    /// Current RPM
    pub current_rpm: f64,
    /// Current torque (kft-lbs)
    pub current_torque: f64,
    /// Current flow in (gpm)
    pub current_flow_in: f64,
    /// Current flow out (gpm)
    pub current_flow_out: f64,
    /// Current mud weight in (ppg)
    pub current_mud_weight: f64,
    /// Current ECD (ppg)
    pub current_ecd: f64,
    /// Current gas units
    pub current_gas: f64,
    /// Current pit volume (bbl)
    pub current_pit_volume: f64,
    /// Current standpipe pressure (psi)
    pub current_spp: f64,
    /// Current casing pressure (psi)
    pub current_casing_pressure: f64,
}

impl Default for DrillingPhysicsReport {
    fn default() -> Self {
        Self {
            avg_mse: 0.0,
            mse_trend: 0.0,
            optimal_mse: 0.0,
            mse_efficiency: 100.0,
            dxc_trend: 0.0,
            flow_balance_trend: 0.0,
            avg_pit_rate: 0.0,
            formation_hardness: 5.0,
            confidence: 0.0,
            detected_dysfunctions: Vec::new(),
            wob_trend: 0.0,
            rop_trend: 0.0,
            founder_detected: false,
            founder_severity: 0.0,
            optimal_wob_estimate: 0.0,
            current_depth: 0.0,
            current_rop: 0.0,
            current_wob: 0.0,
            current_rpm: 0.0,
            current_torque: 0.0,
            current_flow_in: 0.0,
            current_flow_out: 0.0,
            current_mud_weight: 0.0,
            current_ecd: 0.0,
            current_gas: 0.0,
            current_pit_volume: 0.0,
            current_spp: 0.0,
            current_casing_pressure: 0.0,
        }
    }
}

// ============================================================================
// Active Damping Types
// ============================================================================

/// Classification of torque oscillation type based on frequency analysis.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum OscillationType {
    /// Classic stick-slip: low-frequency torsional oscillation (<1 Hz)
    StickSlip,
    /// Torsional oscillation with indeterminate frequency (insufficient data or ambiguous)
    TorsionalGeneral,
}

/// Result of torque oscillation characterization from the history buffer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OscillationAnalysis {
    /// Classified oscillation type
    pub oscillation_type: OscillationType,
    /// Torque coefficient of variation (std_dev / mean)
    pub torque_cv: f64,
    /// Estimated oscillation frequency (Hz) from zero-crossing analysis
    pub estimated_frequency_hz: f64,
    /// Amplitude ratio: (peak - trough) / mean_torque
    pub amplitude_ratio: f64,
    /// Severity: 0.0 (mild) to 1.0 (severe)
    pub severity: f64,
    /// Number of torque samples used in analysis
    pub sample_count: usize,
}

/// Specific parameter change recommendation to dampen torque oscillation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DampingRecommendation {
    /// Oscillation analysis that triggered this recommendation
    pub analysis: OscillationAnalysis,
    /// Current WOB (klbs)
    pub current_wob: f64,
    /// Recommended WOB (klbs) — always within safe envelope
    pub recommended_wob: f64,
    /// WOB change percentage (negative = reduction)
    pub wob_change_pct: f64,
    /// Current RPM
    pub current_rpm: f64,
    /// Recommended RPM — always within safe envelope
    pub recommended_rpm: f64,
    /// RPM change percentage (positive = increase for stick-slip)
    pub rpm_change_pct: f64,
    /// Human-readable rationale for the recommendation
    pub rationale: String,
}

// ============================================================================
// Damping Feedback Monitoring Types
// ============================================================================

/// Outcome of monitoring a damping recommendation's effectiveness.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DampingOutcome {
    /// Torque CV reduced by ≥ success threshold — recommendation worked
    Success,
    /// Monitoring window expired without significant improvement — escalate
    Escalated,
    /// Torque CV increased by ≥ retract threshold — recommendation made things worse
    Retracted,
}

/// A successful damping recipe stored per formation for future reuse.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DampingRecipe {
    /// Formation name where this recipe succeeded
    pub formation_name: String,
    /// WOB change percentage that was applied
    pub wob_change_pct: f64,
    /// RPM change percentage that was applied
    pub rpm_change_pct: f64,
    /// Torque CV before damping action
    pub baseline_cv: f64,
    /// Torque CV achieved after damping action
    pub achieved_cv: f64,
    /// CV reduction percentage (positive = improvement)
    pub cv_reduction_pct: f64,
    /// Depth (ft) at time of recommendation
    pub depth_ft: f64,
    /// Unix timestamp when recipe was recorded
    pub recorded_at: u64,
}

/// Snapshot of the damping monitor state for API/dashboard visibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DampingMonitorSnapshot {
    /// Whether the monitor is currently tracking a recommendation
    pub active: bool,
    /// Baseline torque CV when monitoring started
    pub baseline_cv: Option<f64>,
    /// Current torque CV (latest computation)
    pub current_cv: Option<f64>,
    /// CV change percentage since baseline (negative = improvement)
    pub cv_change_pct: Option<f64>,
    /// Seconds elapsed since monitoring started
    pub elapsed_secs: Option<u64>,
    /// Monitoring window duration (from config)
    pub window_secs: u64,
    /// Formation being monitored
    pub formation_name: Option<String>,
    /// Most recent outcome (if monitoring just concluded)
    pub last_outcome: Option<DampingOutcome>,
}

// ============================================================================
// Phase 8: Orchestrator Voting
// ============================================================================

/// Individual specialist vote for ensemble decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecialistVote {
    /// Name of the specialist (e.g., "MSE", "Hydraulic", "WellControl", "Formation")
    pub specialist: String,
    /// Vote value: LOW=1, MEDIUM=2, HIGH=3, CRITICAL=4
    pub vote: TicketSeverity,
    /// Weight for this specialist (0.0 to 1.0, all should sum to 1.0)
    pub weight: f64,
    /// Reasoning for this vote
    pub reasoning: String,
}

/// Final severity levels for strategic advisories
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum FinalSeverity {
    Healthy = 0,
    Low = 1,
    Medium = 2,
    High = 3,
    Critical = 4,
}

impl std::fmt::Display for FinalSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FinalSeverity::Healthy => write!(f, "HEALTHY"),
            FinalSeverity::Low => write!(f, "LOW"),
            FinalSeverity::Medium => write!(f, "MEDIUM"),
            FinalSeverity::High => write!(f, "HIGH"),
            FinalSeverity::Critical => write!(f, "CRITICAL"),
        }
    }
}

impl From<f64> for FinalSeverity {
    /// Convert weighted score (0-4) to severity level.
    ///
    /// Uses `>` (not `>=`) at boundaries so exact half-points round down:
    /// e.g. score 3.5 → High (not Critical). This avoids boundary inflation
    /// when vote distributions land on exact half-integer sums.
    fn from(score: f64) -> Self {
        if score > 3.5 {
            FinalSeverity::Critical
        } else if score > 2.5 {
            FinalSeverity::High
        } else if score > 1.5 {
            FinalSeverity::Medium
        } else if score > 0.5 {
            FinalSeverity::Low
        } else {
            FinalSeverity::Healthy
        }
    }
}

// ============================================================================
// Phase 8-9: Strategic Advisory (Final Output)
// ============================================================================

/// Strategic advisory output from the drilling intelligence system (Phase 8-9)
///
/// This is the complete output of the processing pipeline, containing:
/// - Efficiency score and risk level
/// - Actionable recommendation with expected benefit
/// - Reasoning from physics and specialist analysis
/// - All specialist votes for transparency
/// - Flight Recorder trace log for debugging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategicAdvisory {
    /// Unix timestamp
    pub timestamp: u64,
    /// Drilling efficiency score (0-100, higher is better)
    pub efficiency_score: u8,
    /// Overall risk level assessment
    pub risk_level: RiskLevel,
    /// Final severity from ensemble voting
    pub severity: FinalSeverity,
    /// Primary recommendation (actionable advice)
    pub recommendation: String,
    /// Expected benefit if recommendation is followed
    pub expected_benefit: String,
    /// Technical reasoning supporting the recommendation
    pub reasoning: String,
    /// Individual votes from all specialists
    pub votes: Vec<SpecialistVote>,
    /// Physics report from strategic analysis
    pub physics_report: DrillingPhysicsReport,
    /// Context snippets used (from vector DB)
    pub context_used: Vec<String>,
    /// Flight Recorder trace log from the advisory ticket
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trace_log: Vec<TicketEvent>,
    /// Anomaly category from the originating ticket
    #[serde(default)]
    pub category: AnomalyCategory,
    /// Parameter that triggered the advisory (e.g. "flow_balance", "mse_efficiency")
    #[serde(default)]
    pub trigger_parameter: String,
    /// Measured value of the trigger parameter at detection time
    #[serde(default)]
    pub trigger_value: f64,
    /// Threshold value that was exceeded
    #[serde(default)]
    pub threshold_value: f64,
}

impl Default for StrategicAdvisory {
    fn default() -> Self {
        Self {
            timestamp: 0,
            efficiency_score: 100,
            risk_level: RiskLevel::Low,
            severity: FinalSeverity::Healthy,
            recommendation: "Continue current drilling parameters".to_string(),
            expected_benefit: "Maintain optimal performance".to_string(),
            reasoning: String::new(),
            votes: Vec::new(),
            physics_report: DrillingPhysicsReport::default(),
            context_used: Vec::new(),
            trace_log: Vec::new(),
            category: AnomalyCategory::None,
            trigger_parameter: String::new(),
            trigger_value: 0.0,
            threshold_value: 0.0,
        }
    }
}
