//! Drilling thresholds, ensemble weights, and campaign-specific thresholds

use serde::{Deserialize, Serialize};

use super::Campaign;

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

// Legacy thresholds module for backward compatibility
#[allow(unused_imports, dead_code)]
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
