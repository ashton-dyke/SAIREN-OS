//! Shared data structures for WITS-based drilling operational intelligence
//!
//! This module defines the core types for the drilling advisory pipeline:
//! - Phase 1: WitsPacket (WITS Level 0 data)
//! - Phase 2-3: DrillingMetrics, AdvisoryTicket (tactical agent outputs)
//! - Phase 4: HistoryBuffer (packet circular buffer)
//! - Phase 5: DrillingPhysicsReport (drilling physics calculations)
//! - Phase 6: Context snippets from vector DB
//! - Phase 7: LLM advisory (RECOMMENDATION + REASONING)
//! - Phase 8: StrategicAdvisory (orchestrator output with weighted voting)

use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ============================================================================
// Phase 1: WITS Data Ingestion
// ============================================================================

/// Operational state of the drilling rig
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, Hash)]
pub enum RigState {
    Drilling,
    Reaming,
    Circulating,
    Connection,
    TrippingIn,
    TrippingOut,
    #[default]
    Idle,
}

impl std::fmt::Display for RigState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RigState::Drilling => write!(f, "Drilling"),
            RigState::Reaming => write!(f, "Reaming"),
            RigState::Circulating => write!(f, "Circulating"),
            RigState::Connection => write!(f, "Connection"),
            RigState::TrippingIn => write!(f, "Tripping In"),
            RigState::TrippingOut => write!(f, "Tripping Out"),
            RigState::Idle => write!(f, "Idle"),
        }
    }
}

// ============================================================================
// Campaign Type (Production vs P&A)
// ============================================================================

// ============================================================================
// Operation Type (Auto-Classified)
// ============================================================================

/// Operation type for automatic classification of drilling/P&A activities
///
/// Automatically detected from WITS parameters:
/// - **ProductionDrilling**: Standard drilling in Production campaign
/// - **Milling**: High torque, low ROP (cutting casing/cement)
/// - **CementDrillOut**: High WOB, moderate torque (drilling cement)
/// - **Circulating**: Pumps on, no rotation (conditioning mud)
/// - **Static**: No pumps, no rotation (idle/waiting)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, Hash)]
pub enum Operation {
    /// Standard production drilling - optimizing ROP and MSE
    #[default]
    ProductionDrilling,
    /// P&A milling operation - cutting casing/cement with high torque
    Milling,
    /// P&A cement drill-out - drilling through cement plugs
    CementDrillOut,
    /// Circulating mud without drilling (conditioning, cleaning)
    Circulating,
    /// Static/idle - no pumps, no rotation
    Static,
}

impl Operation {
    /// Get display name for UI
    pub fn display_name(&self) -> &'static str {
        match self {
            Operation::ProductionDrilling => "Production Drilling",
            Operation::Milling => "Milling",
            Operation::CementDrillOut => "Cement Drill-Out",
            Operation::Circulating => "Circulating",
            Operation::Static => "Static",
        }
    }

    /// Get short code for logging
    pub fn short_code(&self) -> &'static str {
        match self {
            Operation::ProductionDrilling => "DRILL",
            Operation::Milling => "MILL",
            Operation::CementDrillOut => "CDO",
            Operation::Circulating => "CIRC",
            Operation::Static => "STATIC",
        }
    }

    /// Check if this operation is a P&A-specific operation
    pub fn is_pa_operation(&self) -> bool {
        matches!(self, Operation::Milling | Operation::CementDrillOut)
    }
}

impl std::fmt::Display for Operation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

// ============================================================================
// Campaign Type (Production vs P&A)
// ============================================================================

/// Campaign type determines operational focus and thresholds
///
/// - **Production**: Focus on drilling efficiency, ROP optimization, formation evaluation
/// - **PlugAbandonment**: Focus on cement integrity, pressure containment, barrier verification
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum Campaign {
    /// Production drilling - optimize ROP, MSE, minimize NPT
    #[default]
    Production,
    /// Plug & Abandonment - cement integrity, pressure testing, barrier verification
    PlugAbandonment,
}

impl Campaign {
    /// Get display name for UI
    pub fn display_name(&self) -> &'static str {
        match self {
            Campaign::Production => "Production Drilling",
            Campaign::PlugAbandonment => "Plug & Abandonment",
        }
    }

    /// Get short code for logging
    pub fn short_code(&self) -> &'static str {
        match self {
            Campaign::Production => "PROD",
            Campaign::PlugAbandonment => "P&A",
        }
    }

    /// Parse from string (for API/config)
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "production" | "prod" | "drilling" => Some(Campaign::Production),
            "p&a" | "pa" | "plug_abandonment" | "plugabandonment" | "abandonment" => {
                Some(Campaign::PlugAbandonment)
            }
            _ => None,
        }
    }
}

impl std::fmt::Display for Campaign {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// WITS Level 0 packet containing full drilling parameters
///
/// Contains ~40+ channels covering drilling, hydraulics, mud, and well control data.
/// The `waveform_snapshot` field uses `Arc<Vec<f64>>` to enable zero-copy
/// sharing between threads for high-frequency sensor data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitsPacket {
    pub timestamp: u64,

    // === Drilling Parameters ===
    /// Bit depth (ft) - WITS 0108
    pub bit_depth: f64,
    /// Hole depth (ft) - WITS 0110
    pub hole_depth: f64,
    /// Rate of penetration (ft/hr) - WITS 0113
    pub rop: f64,
    /// Hook load (klbs) - WITS 0114
    pub hook_load: f64,
    /// Weight on bit (klbs) - WITS 0116
    pub wob: f64,
    /// Rotary RPM - WITS 0117
    pub rpm: f64,
    /// Surface torque (kft-lbs) - WITS 0118
    pub torque: f64,
    /// Bit diameter (inches)
    pub bit_diameter: f64,

    // === Hydraulics Parameters ===
    /// Standpipe pressure (psi) - WITS 0119
    pub spp: f64,
    /// Pump strokes per minute - WITS 0120
    pub pump_spm: f64,
    /// Flow rate in (gpm) - WITS 0121
    pub flow_in: f64,
    /// Flow rate out (gpm) - WITS 0122
    pub flow_out: f64,
    /// Total pit volume (bbl) - WITS 0123
    pub pit_volume: f64,
    /// Pit volume change from baseline (bbl)
    #[serde(default)]
    pub pit_volume_change: f64,

    // === Mud Parameters ===
    /// Mud weight in (ppg) - WITS 0124
    pub mud_weight_in: f64,
    /// Mud weight out (ppg) - WITS 0125
    pub mud_weight_out: f64,
    /// Equivalent circulating density (ppg) - calculated or from sensor
    pub ecd: f64,
    /// Mud temperature in (°F) - WITS 0126
    pub mud_temp_in: f64,
    /// Mud temperature out (°F) - WITS 0127
    pub mud_temp_out: f64,

    // === Well Control Parameters ===
    /// Total gas units - WITS 0140
    pub gas_units: f64,
    /// Background gas (units)
    #[serde(default)]
    pub background_gas: f64,
    /// Connection gas (units)
    #[serde(default)]
    pub connection_gas: f64,
    /// H2S concentration (ppm) - WITS 0145
    #[serde(default)]
    pub h2s: f64,
    /// CO2 concentration (%) - WITS 0146
    #[serde(default)]
    pub co2: f64,
    /// Casing pressure (psi) - WITS 0130
    #[serde(default)]
    pub casing_pressure: f64,
    /// Annular pressure (psi)
    #[serde(default)]
    pub annular_pressure: f64,

    // === Formation Parameters ===
    /// Formation pore pressure estimate (ppg)
    #[serde(default)]
    pub pore_pressure: f64,
    /// Fracture gradient estimate (ppg)
    #[serde(default)]
    pub fracture_gradient: f64,

    // === Derived/Calculated Parameters ===
    /// Mechanical Specific Energy (psi) - calculated
    #[serde(default)]
    pub mse: f64,
    /// D-exponent - calculated
    #[serde(default)]
    pub d_exponent: f64,
    /// Corrected d-exponent (dxc)
    #[serde(default)]
    pub dxc: f64,
    /// ROP change from previous packet (ft/hr)
    #[serde(default)]
    pub rop_delta: f64,
    /// Torque change from baseline (%)
    #[serde(default)]
    pub torque_delta_percent: f64,
    /// SPP change from baseline (psi)
    #[serde(default)]
    pub spp_delta: f64,

    // === Rig State ===
    /// Current operational state of the rig
    #[serde(default)]
    pub rig_state: RigState,

    // === High-Frequency Data (Optional) ===
    /// High-frequency waveform snapshot for vibration analysis
    /// 1024 samples at 10 kHz = 100ms window
    /// Used for stick-slip and vibration analysis
    #[serde(default = "default_arc_vec", serialize_with = "serialize_arc_vec", deserialize_with = "deserialize_arc_vec")]
    pub waveform_snapshot: Arc<Vec<f64>>,
}

/// Default for Arc<Vec<f64>> - creates an empty Arc-wrapped vector
fn default_arc_vec() -> Arc<Vec<f64>> {
    Arc::new(Vec::new())
}

/// Serialize Arc<Vec<f64>> as just Vec<f64>
fn serialize_arc_vec<S>(data: &Arc<Vec<f64>>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    data.as_ref().serialize(serializer)
}

/// Deserialize Vec<f64> into Arc<Vec<f64>>
fn deserialize_arc_vec<'de, D>(deserializer: D) -> Result<Arc<Vec<f64>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let vec = Vec::<f64>::deserialize(deserializer)?;
    Ok(Arc::new(vec))
}

impl Default for WitsPacket {
    fn default() -> Self {
        Self {
            timestamp: 0,
            bit_depth: 0.0,
            hole_depth: 0.0,
            rop: 0.0,
            hook_load: 0.0,
            wob: 0.0,
            rpm: 0.0,
            torque: 0.0,
            bit_diameter: 8.5, // Common default
            spp: 0.0,
            pump_spm: 0.0,
            flow_in: 0.0,
            flow_out: 0.0,
            pit_volume: 0.0,
            pit_volume_change: 0.0,
            mud_weight_in: 0.0,
            mud_weight_out: 0.0,
            ecd: 0.0,
            mud_temp_in: 0.0,
            mud_temp_out: 0.0,
            gas_units: 0.0,
            background_gas: 0.0,
            connection_gas: 0.0,
            h2s: 0.0,
            co2: 0.0,
            casing_pressure: 0.0,
            annular_pressure: 0.0,
            pore_pressure: 0.0,
            fracture_gradient: 0.0,
            mse: 0.0,
            d_exponent: 0.0,
            dxc: 0.0,
            rop_delta: 0.0,
            torque_delta_percent: 0.0,
            spp_delta: 0.0,
            rig_state: RigState::Idle,
            waveform_snapshot: Arc::new(Vec::new()),
        }
    }
}

impl WitsPacket {
    /// Check if this packet has a valid waveform snapshot for vibration analysis
    pub fn has_waveform(&self) -> bool {
        !self.waveform_snapshot.is_empty()
    }

    /// Calculate flow balance (positive = gain, negative = loss)
    pub fn flow_balance(&self) -> f64 {
        self.flow_out - self.flow_in
    }

    /// Get ECD margin to fracture gradient
    /// Returns the margin in ppg, or a safe default (1.5 ppg) if fracture gradient unavailable
    pub fn ecd_margin(&self) -> f64 {
        if self.fracture_gradient > 0.0 && self.ecd > 0.0 {
            self.fracture_gradient - self.ecd
        } else {
            // Return safe default when fracture gradient unavailable
            // 1.5 ppg is a typical comfortable margin
            1.5
        }
    }

    /// Check if drilling (RPM > 0 and WOB > 0)
    pub fn is_drilling(&self) -> bool {
        self.rpm > 5.0 && self.wob > 1.0
    }

    /// Check if circulating (flow > 0 but not drilling)
    pub fn is_circulating(&self) -> bool {
        self.flow_in > 50.0 && !self.is_drilling()
    }

    /// Calculate mud weight delta (in vs out)
    pub fn mud_weight_delta(&self) -> f64 {
        self.mud_weight_out - self.mud_weight_in
    }

    /// Calculate mud temperature delta (out vs in)
    pub fn mud_temp_delta(&self) -> f64 {
        self.mud_temp_out - self.mud_temp_in
    }
}

// ============================================================================
// Phase 2-3: Tactical Agent Types
// ============================================================================

/// Category of detected anomaly for drilling operations
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum AnomalyCategory {
    #[default]
    None,
    /// MSE inefficiency, ROP optimization opportunities
    DrillingEfficiency,
    /// SPP, ECD, flow rate anomalies
    Hydraulics,
    /// Kick, loss, gas, pit volume changes
    WellControl,
    /// Torque, vibration, stick-slip issues
    Mechanical,
    /// D-exponent trends, hard/soft stringers
    Formation,
}

impl std::fmt::Display for AnomalyCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnomalyCategory::None => write!(f, "None"),
            AnomalyCategory::DrillingEfficiency => write!(f, "Drilling Efficiency"),
            AnomalyCategory::Hydraulics => write!(f, "Hydraulics"),
            AnomalyCategory::WellControl => write!(f, "Well Control"),
            AnomalyCategory::Mechanical => write!(f, "Mechanical"),
            AnomalyCategory::Formation => write!(f, "Formation"),
        }
    }
}

/// Output from the tactical agent's drilling physics calculations (Phase 2)
///
/// Contains fast metrics computed in < 15ms:
/// - MSE (Mechanical Specific Energy)
/// - D-exponent and corrected dxc
/// - Flow balance and pit rate
/// - ECD margin to fracture pressure
/// - Anomaly detection flags
/// - Operation classification (auto-detected)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrillingMetrics {
    /// Current rig operational state
    pub state: RigState,
    /// Auto-classified operation type (ProductionDrilling, Milling, CementDrillOut, etc.)
    pub operation: Operation,
    /// Mechanical Specific Energy (psi)
    pub mse: f64,
    /// MSE efficiency (0-100%)
    pub mse_efficiency: f64,
    /// D-exponent (normalized drilling parameter)
    pub d_exponent: f64,
    /// Corrected d-exponent (adjusted for mud weight)
    pub dxc: f64,
    /// MSE deviation from baseline (%)
    pub mse_delta_percent: f64,
    /// Flow balance: flow_out - flow_in (gpm)
    /// Positive = potential kick, Negative = potential loss
    pub flow_balance: f64,
    /// Pit volume rate of change (bbl/hr)
    /// Positive = gain, Negative = loss
    pub pit_rate: f64,
    /// ECD margin to fracture pressure (ppg)
    pub ecd_margin: f64,
    /// Torque delta from baseline (%)
    pub torque_delta_percent: f64,
    /// SPP delta from baseline (psi)
    pub spp_delta: f64,
    /// Whether metrics indicate an anomaly
    pub is_anomaly: bool,
    /// Category of detected anomaly
    pub anomaly_category: AnomalyCategory,
    /// Description of detected anomaly
    pub anomaly_description: Option<String>,
}

impl Default for DrillingMetrics {
    fn default() -> Self {
        Self {
            state: RigState::Idle,
            operation: Operation::Static,
            mse: 0.0,
            mse_efficiency: 100.0,
            d_exponent: 0.0,
            dxc: 0.0,
            mse_delta_percent: 0.0,
            flow_balance: 0.0,
            pit_rate: 0.0,
            ecd_margin: 1.5, // Safe default margin
            torque_delta_percent: 0.0,
            spp_delta: 0.0,
            is_anomaly: false,
            anomaly_category: AnomalyCategory::None,
            anomaly_description: None,
        }
    }
}

/// Severity level for advisory tickets
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum TicketSeverity {
    #[default]
    Low = 1,
    Medium = 2,
    High = 3,
    Critical = 4,
}

impl std::fmt::Display for TicketSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TicketSeverity::Low => write!(f, "LOW"),
            TicketSeverity::Medium => write!(f, "MEDIUM"),
            TicketSeverity::High => write!(f, "HIGH"),
            TicketSeverity::Critical => write!(f, "CRITICAL"),
        }
    }
}

/// Type of advisory ticket
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TicketType {
    /// Suggestion to improve drilling efficiency
    Optimization,
    /// Warning about potential risk/hazard
    RiskWarning,
    /// Recommended intervention to prevent problem
    Intervention,
}

impl std::fmt::Display for TicketType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TicketType::Optimization => write!(f, "OPTIMIZATION"),
            TicketType::RiskWarning => write!(f, "RISK_WARNING"),
            TicketType::Intervention => write!(f, "INTERVENTION"),
        }
    }
}

// ============================================================================
// Phase 4: History Buffer
// ============================================================================

/// Wrapper for WITS packet with computed metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub packet: WitsPacket,
    pub metrics: DrillingMetrics,
    /// Cumulative MSE-hours contribution from this packet
    pub mse_contribution: f64,
}

// ============================================================================
// Phase 5: Advanced Physics (Strategic)
// ============================================================================

/// Risk level assessment for drilling operations
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskLevel {
    Low = 0,
    Elevated = 1,
    High = 2,
    Critical = 3,
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RiskLevel::Low => write!(f, "LOW"),
            RiskLevel::Elevated => write!(f, "ELEVATED"),
            RiskLevel::High => write!(f, "HIGH"),
            RiskLevel::Critical => write!(f, "CRITICAL"),
        }
    }
}

impl Default for RiskLevel {
    fn default() -> Self {
        RiskLevel::Low
    }
}

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
    /// Convert weighted score (0-4) to severity level
    fn from(score: f64) -> Self {
        if score >= 3.5 {
            FinalSeverity::Critical
        } else if score >= 2.5 {
            FinalSeverity::High
        } else if score >= 1.5 {
            FinalSeverity::Medium
        } else if score >= 0.5 {
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
        }
    }
}

// ============================================================================
// Drilling Thresholds
// ============================================================================

/// Thresholds for drilling anomaly detection
pub mod drilling_thresholds {
    // === MSE Efficiency Thresholds ===
    /// MSE efficiency above this is optimal (%)
    pub const MSE_EFFICIENCY_OPTIMAL: f64 = 85.0;
    /// MSE efficiency below this is poor (%)
    pub const MSE_EFFICIENCY_POOR: f64 = 50.0;
    /// MSE efficiency below this warrants optimization advisory (%)
    pub const MSE_EFFICIENCY_WARNING: f64 = 70.0;

    // === Well Control Thresholds (SAFETY-CRITICAL) ===
    /// Flow imbalance warning threshold (gpm)
    pub const FLOW_IMBALANCE_WARNING: f64 = 10.0;
    /// Flow imbalance critical threshold (gpm)
    pub const FLOW_IMBALANCE_CRITICAL: f64 = 20.0;
    /// Pit gain warning threshold (bbl)
    pub const PIT_GAIN_WARNING: f64 = 5.0;
    /// Pit gain critical threshold (bbl)
    pub const PIT_GAIN_CRITICAL: f64 = 10.0;
    /// Pit rate warning threshold (bbl/hr)
    pub const PIT_RATE_WARNING: f64 = 5.0;
    /// Pit rate critical threshold (bbl/hr)
    pub const PIT_RATE_CRITICAL: f64 = 15.0;
    /// Gas units warning threshold
    pub const GAS_UNITS_WARNING: f64 = 100.0;
    /// Gas units critical threshold
    pub const GAS_UNITS_CRITICAL: f64 = 500.0;
    /// H2S warning threshold (ppm)
    pub const H2S_WARNING: f64 = 10.0;
    /// H2S critical threshold (ppm)
    pub const H2S_CRITICAL: f64 = 20.0;

    // === Hydraulics Thresholds ===
    /// Normal hydrostatic mud weight gradient (ppg) - used for Dxc correction
    /// Represents typical pore pressure gradient (8.6 ppg = 0.465 psi/ft)
    pub const NORMAL_MUD_WEIGHT: f64 = 8.6;
    /// Typical fracture gradient (ppg) - used for ECD margin calculation
    pub const FRACTURE_GRADIENT_TYPICAL: f64 = 14.0;
    /// ECD margin warning threshold (ppg to fracture)
    pub const ECD_MARGIN_WARNING: f64 = 0.3;
    /// ECD margin critical threshold (ppg)
    pub const ECD_MARGIN_CRITICAL: f64 = 0.1;
    /// SPP deviation warning threshold (psi)
    pub const SPP_DEVIATION_WARNING: f64 = 100.0;
    /// SPP deviation critical threshold (psi)
    pub const SPP_DEVIATION_CRITICAL: f64 = 200.0;

    // === Mechanical Thresholds ===
    /// Torque increase warning threshold (%)
    pub const TORQUE_INCREASE_WARNING: f64 = 0.15;
    /// Torque increase critical threshold (%)
    pub const TORQUE_INCREASE_CRITICAL: f64 = 0.25;
    /// Stick-slip coefficient of variation warning threshold
    pub const STICK_SLIP_CV_WARNING: f64 = 0.15;
    /// Stick-slip coefficient of variation critical threshold
    pub const STICK_SLIP_CV_CRITICAL: f64 = 0.25;

    // === Founder Detection Thresholds ===
    /// Minimum WOB increase (%) to consider as "increasing"
    pub const FOUNDER_WOB_INCREASE_MIN: f64 = 0.05;
    /// ROP response threshold - below this % increase, ROP is "not responding"
    pub const FOUNDER_ROP_RESPONSE_MIN: f64 = 0.01;
    /// Founder severity threshold for warning
    pub const FOUNDER_SEVERITY_WARNING: f64 = 0.3;
    /// Founder severity threshold for high severity
    pub const FOUNDER_SEVERITY_HIGH: f64 = 0.7;
    /// Minimum samples needed for reliable founder detection
    pub const FOUNDER_MIN_SAMPLES: usize = 5;

    // === Formation Change Thresholds ===
    /// D-exponent increase rate warning (per 100ft)
    pub const DEXP_INCREASE_WARNING: f64 = 0.1;
    /// D-exponent decrease (soft stringer) warning
    pub const DEXP_DECREASE_WARNING: f64 = -0.15;

    // === Cooldown and Timing ===
    /// Default cooldown between tickets (seconds)
    pub const DEFAULT_COOLDOWN_SECONDS: u64 = 60;
    /// Critical bypass - no cooldown for critical issues
    pub const CRITICAL_BYPASS_COOLDOWN: bool = true;
}

/// Weights for Phase 8 ensemble voting - drilling specialists
pub mod weights {
    /// MSE/Drilling efficiency specialist weight (25%)
    pub const MSE: f64 = 0.25;
    /// Hydraulic specialist weight (25%)
    pub const HYDRAULIC: f64 = 0.25;
    /// Well Control specialist weight (30%) - highest for safety
    pub const WELL_CONTROL: f64 = 0.30;
    /// Formation specialist weight (20%)
    pub const FORMATION: f64 = 0.20;
}

// ============================================================================
// Campaign-Specific Thresholds
// ============================================================================

/// Campaign-specific threshold configuration
///
/// Different campaigns have different operational priorities:
/// - Production: Optimize ROP, minimize MSE
/// - P&A: Focus on pressure integrity, cement quality
#[derive(Debug, Clone)]
pub struct CampaignThresholds {
    // MSE/Efficiency (less relevant for P&A)
    pub mse_efficiency_warning: f64,
    pub mse_efficiency_poor: f64,

    // Pressure thresholds (more critical for P&A)
    pub pressure_test_tolerance: f64,     // psi deviation allowed
    pub cement_pressure_hold: f64,        // psi for cement test
    pub barrier_pressure_margin: f64,     // psi margin for barrier

    // Flow thresholds (tighter for P&A due to cement/plug operations)
    pub flow_imbalance_warning: f64,
    pub flow_imbalance_critical: f64,

    // Specialist weights (different focus per campaign)
    pub weight_mse: f64,
    pub weight_hydraulic: f64,
    pub weight_well_control: f64,
    pub weight_formation: f64,

    // P&A specific
    pub cement_returns_expected: bool,
    pub plug_depth_tolerance: f64,        // ft tolerance for plug depth
}

impl CampaignThresholds {
    /// Get thresholds for Production drilling
    pub fn production() -> Self {
        Self {
            // Production focuses on efficiency
            mse_efficiency_warning: 70.0,
            mse_efficiency_poor: 50.0,

            // Standard pressure thresholds
            pressure_test_tolerance: 50.0,
            cement_pressure_hold: 0.0,      // N/A for production
            barrier_pressure_margin: 0.0,   // N/A for production

            // Standard flow thresholds
            flow_imbalance_warning: 10.0,
            flow_imbalance_critical: 20.0,

            // Balanced weights for production
            weight_mse: 0.25,
            weight_hydraulic: 0.25,
            weight_well_control: 0.30,
            weight_formation: 0.20,

            // Not applicable for production
            cement_returns_expected: false,
            plug_depth_tolerance: 0.0,
        }
    }

    /// Get thresholds for Plug & Abandonment
    pub fn plug_abandonment() -> Self {
        Self {
            // MSE less important for P&A (not drilling for ROP)
            mse_efficiency_warning: 50.0,   // Relaxed
            mse_efficiency_poor: 30.0,      // Relaxed

            // Pressure integrity is critical for P&A
            pressure_test_tolerance: 25.0,  // Tighter tolerance
            cement_pressure_hold: 500.0,    // Hold 500 psi for cement test
            barrier_pressure_margin: 100.0, // 100 psi margin on barriers

            // Tighter flow control for cement operations
            flow_imbalance_warning: 5.0,    // Tighter - cement returns matter
            flow_imbalance_critical: 15.0,  // Tighter

            // P&A weights: well control and hydraulics dominate
            weight_mse: 0.10,               // Reduced - efficiency less critical
            weight_hydraulic: 0.35,         // Increased - cement/mud critical
            weight_well_control: 0.40,      // Increased - barrier integrity
            weight_formation: 0.15,         // Reduced

            // P&A specific
            cement_returns_expected: true,
            plug_depth_tolerance: 5.0,      // 5ft tolerance for plug setting
        }
    }

    /// Get thresholds for a campaign type
    pub fn for_campaign(campaign: Campaign) -> Self {
        match campaign {
            Campaign::Production => Self::production(),
            Campaign::PlugAbandonment => Self::plug_abandonment(),
        }
    }
}

impl Default for CampaignThresholds {
    fn default() -> Self {
        Self::production()
    }
}

// ============================================================================
// Flight Recorder / Ticket Tracker System
// ============================================================================

/// Processing stage for advisory trace logging
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TicketStage {
    /// Advisory ticket created by tactical agent
    TacticalCreation,
    /// Physics calculations in strategic agent
    StrategicPhysics,
    /// MSE trend analysis
    MseAnalysis,
    /// D-exponent/formation analysis
    FormationAnalysis,
    /// Flow balance / kick-loss check
    WellControlCheck,
    /// Hydraulics analysis
    HydraulicsCheck,
    /// Ensemble voting
    EnsembleVoting,
    /// LLM advisory generation
    LlmAdvisory,
    /// Final decision
    FinalDecision,
}

impl std::fmt::Display for TicketStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TicketStage::TacticalCreation => write!(f, "TACTICAL"),
            TicketStage::StrategicPhysics => write!(f, "PHYSICS"),
            TicketStage::MseAnalysis => write!(f, "MSE"),
            TicketStage::FormationAnalysis => write!(f, "FORMATION"),
            TicketStage::WellControlCheck => write!(f, "WELL_CONTROL"),
            TicketStage::HydraulicsCheck => write!(f, "HYDRAULICS"),
            TicketStage::EnsembleVoting => write!(f, "VOTING"),
            TicketStage::LlmAdvisory => write!(f, "LLM"),
            TicketStage::FinalDecision => write!(f, "FINAL"),
        }
    }
}

/// Check status for advisory trace events
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CheckStatus {
    /// Check passed (contributes to confirmation)
    Passed,
    /// Check failed (contributes to rejection)
    Failed,
    /// Check inconclusive (doesn't affect decision)
    Inconclusive,
    /// Informational event (no pass/fail)
    Info,
}

impl std::fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckStatus::Passed => write!(f, "PASSED"),
            CheckStatus::Failed => write!(f, "FAILED"),
            CheckStatus::Inconclusive => write!(f, "INCONCLUSIVE"),
            CheckStatus::Info => write!(f, "INFO"),
        }
    }
}

/// Individual event in the advisory trace log (Flight Recorder)
///
/// Each event records a decision point or check in the advisory pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketEvent {
    /// Unix timestamp (milliseconds for high precision)
    pub timestamp_ms: u64,
    /// Processing stage where this event occurred
    pub stage: TicketStage,
    /// Status of this check (PASSED, FAILED, INCONCLUSIVE, INFO)
    pub status: CheckStatus,
    /// Human-readable message describing the event
    pub message: String,
}

impl TicketEvent {
    /// Create a new ticket event with current timestamp
    pub fn new(stage: TicketStage, status: CheckStatus, message: impl Into<String>) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        Self {
            timestamp_ms,
            stage,
            status,
            message: message.into(),
        }
    }

    /// Create an info event (no pass/fail status)
    pub fn info(stage: TicketStage, message: impl Into<String>) -> Self {
        Self::new(stage, CheckStatus::Info, message)
    }

    /// Create a passed check event
    pub fn passed(stage: TicketStage, message: impl Into<String>) -> Self {
        Self::new(stage, CheckStatus::Passed, message)
    }

    /// Create a failed check event
    pub fn failed(stage: TicketStage, message: impl Into<String>) -> Self {
        Self::new(stage, CheckStatus::Failed, message)
    }

    /// Format as a single-line log entry
    pub fn to_log_line(&self) -> String {
        format!(
            "[{}] {} - {}: {}",
            self.timestamp_ms, self.stage, self.status, self.message
        )
    }
}

// ============================================================================
// Ticket Context (Structured Routing - replaces tactical LLM)
// ============================================================================

/// Structured context attached to each advisory ticket.
///
/// Built by the tactical agent's deterministic pattern matcher. Provides the
/// strategic agent with precise, parseable information about which thresholds
/// were breached, replacing the previously planned tactical LLM description.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketContext {
    /// Which specific thresholds were breached (ordered by severity)
    pub triggers: Vec<ThresholdBreach>,
    /// Detected pattern name (e.g. "Kick Warning", "Pack-off", "MSE Inefficiency")
    pub pattern: String,
    /// Rig state at time of ticket (Drilling, Tripping, etc.)
    pub rig_state: RigState,
    /// Current operation (Production, Milling, CementDrillOut, etc.)
    pub operation: Operation,
    /// Current campaign (Production or P&A)
    pub campaign: Campaign,
}

impl TicketContext {
    /// Format as structured text for the strategic LLM prompt
    pub fn to_prompt_section(&self) -> String {
        let mut s = format!("TICKET: {} ({})\n", self.pattern, self.triggers.first()
            .map(|t| t.threshold_type.as_str()).unwrap_or("INFO"));
        s.push_str("TRIGGERS:\n");
        for t in &self.triggers {
            s.push_str(&format!(
                "  - {}: {:.2} {} (threshold: {:.2} {}, {})\n",
                t.parameter, t.actual_value, t.unit, t.threshold_value, t.unit, t.threshold_type
            ));
        }
        s.push_str(&format!("RIG STATE: {:?}\n", self.rig_state));
        s.push_str(&format!("OPERATION: {:?}\n", self.operation));
        s.push_str(&format!("CAMPAIGN: {:?}\n", self.campaign));
        s
    }
}

/// A single threshold breach recorded by the tactical pattern matcher.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdBreach {
    /// Parameter name (e.g. "flow_balance", "torque_cv", "mse_efficiency")
    pub parameter: String,
    /// Measured value at time of detection
    pub actual_value: f64,
    /// Threshold that was breached
    pub threshold_value: f64,
    /// "WARNING" or "CRITICAL"
    pub threshold_type: String,
    /// Engineering unit (e.g. "gpm", "%", "ppg", "psi")
    pub unit: String,
}

// ============================================================================
// CfC Neural Network Types (serializable)
// ============================================================================

/// Serializable version of per-feature surprise from the CfC neural network.
/// The CfC internal `FeatureSurprise` uses `&'static str` for names, so this
/// owned variant is used on tickets and for JSON serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfcFeatureSurpriseInfo {
    pub name: String,
    pub error: f64,
    pub magnitude: f64,
}

// ============================================================================
// Advisory Ticket System
// ============================================================================

/// Advisory ticket generated by tactical agent for strategic validation
///
/// This replaces direct alert generation - tactical agent creates tickets
/// that must be validated by the strategic agent using physics engine analysis.
///
/// The `trace_log` field implements the "Flight Recorder" pattern, tracking
/// every decision point in the ticket's lifecycle for debugging and auditing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvisoryTicket {
    /// Unix timestamp when the ticket was created
    pub timestamp: u64,
    /// Type of advisory (Optimization, RiskWarning, Intervention)
    pub ticket_type: TicketType,
    /// Category of the detected anomaly
    pub category: AnomalyCategory,
    /// Initial severity assessment from tactical agent
    pub severity: TicketSeverity,
    /// Current drilling metrics at time of detection
    pub current_metrics: DrillingMetrics,
    /// The parameter that triggered this ticket (e.g., "MSE", "flow_balance")
    pub trigger_parameter: String,
    /// The value of the trigger parameter
    pub trigger_value: f64,
    /// Threshold that was exceeded
    pub threshold_value: f64,
    /// Description of the detected issue
    pub description: String,
    /// Structured context from deterministic pattern matcher (replaces tactical LLM)
    #[serde(default)]
    pub context: Option<TicketContext>,
    /// Current depth at time of detection (ft)
    pub depth: f64,
    /// Flight Recorder trace log - tracks all decision points
    #[serde(default)]
    pub trace_log: Vec<TicketEvent>,
    /// CfC neural network anomaly score (0.0 = normal, 1.0 = highly anomalous)
    #[serde(default)]
    pub cfc_anomaly_score: Option<f64>,
    /// CfC per-feature surprise decomposition (top contributors to anomaly)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cfc_feature_surprises: Vec<CfcFeatureSurpriseInfo>,
}

impl AdvisoryTicket {
    /// Add an event to the trace log
    pub fn log_event(&mut self, event: TicketEvent) {
        self.trace_log.push(event);
    }

    /// Add an info event to the trace log
    pub fn log_info(&mut self, stage: TicketStage, message: impl Into<String>) {
        self.trace_log.push(TicketEvent::info(stage, message));
    }

    /// Add a passed check event to the trace log
    pub fn log_passed(&mut self, stage: TicketStage, message: impl Into<String>) {
        self.trace_log.push(TicketEvent::passed(stage, message));
    }

    /// Add a failed check event to the trace log
    pub fn log_failed(&mut self, stage: TicketStage, message: impl Into<String>) {
        self.trace_log.push(TicketEvent::failed(stage, message));
    }

    /// Get a summary of the trace log for LLM context
    pub fn trace_summary(&self) -> String {
        if self.trace_log.is_empty() {
            return "No trace events recorded".to_string();
        }

        self.trace_log
            .iter()
            .map(|e| format!("[{}] {}", e.stage, e.message))
            .collect::<Vec<_>>()
            .join(" -> ")
    }

    /// Get only the passed/failed events for decision summary
    pub fn decision_summary(&self) -> String {
        let decisions: Vec<_> = self
            .trace_log
            .iter()
            .filter(|e| e.status == CheckStatus::Passed || e.status == CheckStatus::Failed)
            .map(|e| format!("{}: {}", e.stage, e.status))
            .collect();

        if decisions.is_empty() {
            "No decision events".to_string()
        } else {
            decisions.join(", ")
        }
    }
}

/// Verification status returned by strategic agent after physics validation
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum VerificationStatus {
    /// Advisory is awaiting strategic analysis
    Pending,
    /// Physics confirms the issue - generate dashboard advisory
    Confirmed,
    /// Physics rejects the issue (e.g., transient spike, returned to baseline)
    Rejected,
    /// Insufficient data or conflicting signals - monitor closely
    Uncertain,
}

impl std::fmt::Display for VerificationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerificationStatus::Pending => write!(f, "PENDING"),
            VerificationStatus::Confirmed => write!(f, "CONFIRMED"),
            VerificationStatus::Rejected => write!(f, "REJECTED"),
            VerificationStatus::Uncertain => write!(f, "UNCERTAIN"),
        }
    }
}

/// Result of strategic verification with detailed analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// The original advisory ticket being verified
    pub ticket: AdvisoryTicket,
    /// Verification status (Confirmed, Rejected, Uncertain)
    pub status: VerificationStatus,
    /// Physics report from strategic analysis
    pub physics_report: DrillingPhysicsReport,
    /// Reasoning for the verification decision
    pub reasoning: String,
    /// Final severity (only meaningful if Confirmed)
    pub final_severity: FinalSeverity,
    /// Whether to send advisory to dashboard
    pub send_to_dashboard: bool,
}

/// Enhanced physics report for strategic verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedPhysicsReport {
    /// Base physics calculations
    pub base: DrillingPhysicsReport,
    /// Trend consistency from regression analysis (0.0 to 1.0)
    /// Higher values indicate consistent trend
    pub trend_consistency: f64,
    /// Confidence factor based on data quality and history depth
    pub confidence_factor: f64,
    /// Hours of history used for analysis
    pub history_hours: f64,
    /// Whether the anomaly is sustained (not transient)
    pub is_sustained: bool,
    /// Number of consecutive packets showing anomaly
    pub consecutive_anomaly_count: u32,
}

impl Default for EnhancedPhysicsReport {
    fn default() -> Self {
        Self {
            base: DrillingPhysicsReport::default(),
            trend_consistency: 0.0,
            confidence_factor: 0.0,
            history_hours: 0.0,
            is_sustained: false,
            consecutive_anomaly_count: 0,
        }
    }
}

/// Thresholds for verification decision logic
pub mod verification_thresholds {
    /// Minimum trend consistency for confirmation (0.0 to 1.0)
    pub const MIN_TREND_CONSISTENCY: f64 = 0.7;
    /// Minimum confidence factor for certain decisions
    pub const MIN_CONFIDENCE_FACTOR: f64 = 0.6;
    /// Minimum consecutive anomaly packets for sustained classification
    pub const MIN_CONSECUTIVE_FOR_SUSTAINED: u32 = 3;
    /// Minimum history required for verification (minutes)
    pub const MIN_HISTORY_MINUTES: f64 = 5.0;
}

// ============================================================================
// Legacy Compatibility Types (for gradual migration)
// ============================================================================

/// Alias for backward compatibility during migration
pub type SensorPacket = WitsPacket;
/// Alias for backward compatibility during migration
pub type TacticalMetrics = DrillingMetrics;
/// Alias for backward compatibility during migration
pub type OperationalState = RigState;
/// Alias for backward compatibility during migration
pub type PhysicsReport = DrillingPhysicsReport;
/// Alias for backward compatibility during migration
pub type StrategicReport = StrategicAdvisory;
/// Alias for backward compatibility during migration
pub type VerificationTicket = AdvisoryTicket;

// Legacy thresholds module for backward compatibility
pub mod thresholds {
    pub use super::drilling_thresholds::*;
    // Legacy TDS thresholds mapped to drilling equivalents where possible
    pub const KURTOSIS_WARNING: f64 = 3.0;
    pub const KURTOSIS_CRITICAL: f64 = 6.0;
    pub const BPFO_WARNING: f64 = 0.15;
    pub const BPFO_CRITICAL: f64 = 0.3;
    pub const TEMP_DELTA_WARNING: f64 = 8.0;
    pub const TEMP_DELTA_HIGH: f64 = 15.0;
    pub const L10_CRITICAL: f64 = 24.0;
    pub const L10_HIGH: f64 = 168.0;
    pub const L10_MEDIUM: f64 = 720.0;
    pub const BEARING_RATING: f64 = 120_000.0;
    pub const VIB_AMPLITUDE_WARNING: f64 = 0.12;
    pub const VIB_AMPLITUDE_CRITICAL: f64 = 0.4;
}

// ============================================================================
// ML Engine Types (V2.1)
// ============================================================================

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

use std::collections::HashMap;

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
