//! Core state types: RigState, Operation, Campaign

use serde::{Deserialize, Serialize};

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
