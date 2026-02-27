//! Tactical agent types: AnomalyCategory, DrillingMetrics, TicketSeverity, TicketType

use serde::{Deserialize, Serialize};

use super::{Operation, RigState};

// ============================================================================
// Phase 2-3: Tactical Agent Types
// ============================================================================

/// Category of detected anomaly for drilling operations
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
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
    /// Whether flow sensor data is available (at least one of flow_in/flow_out > 0)
    pub flow_data_available: bool,
    /// Whether metrics indicate an anomaly
    pub is_anomaly: bool,
    /// Category of detected anomaly
    pub anomaly_category: AnomalyCategory,
    /// Description of detected anomaly
    pub anomaly_description: Option<String>,
    /// Current formation name (from prognosis lookup)
    #[serde(default)]
    pub current_formation: Option<String>,
    /// Depth into current formation in feet (for progress tracking)
    #[serde(default)]
    pub formation_depth_in_ft: Option<f64>,
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
            flow_data_available: false,
            is_anomaly: false,
            anomaly_category: AnomalyCategory::None,
            anomaly_description: None,
            current_formation: None,
            formation_depth_in_ft: None,
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
