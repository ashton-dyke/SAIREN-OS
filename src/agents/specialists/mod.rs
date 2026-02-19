//! Specialist trait and implementations for Phase 8 ensemble voting
//!
//! Each specialist evaluates an advisory ticket from a specific domain perspective
//! and returns a vote with severity, confidence weight, and reasoning.
//!
//! ## Specialists
//!
//! 1. **MSE** (default 25%) - Drilling efficiency analysis
//! 2. **Hydraulic** (default 25%) - SPP, flow, ECD margin
//! 3. **WellControl** (default 30%) - Kick/loss, gas, pit volume (safety-critical)
//! 4. **Formation** (default 20%) - D-exponent, torque trends

pub mod mse;
pub mod hydraulic;
pub mod well_control;
pub mod formation;

pub use mse::MseSpecialist;
pub use hydraulic::HydraulicSpecialist;
pub use well_control::WellControlSpecialist;
pub use formation::FormationSpecialist;

use crate::types::{AdvisoryTicket, DrillingPhysicsReport, SpecialistVote};

/// Trait for specialist voting agents
///
/// Each specialist evaluates an advisory ticket from its domain perspective
/// and produces a weighted vote for the orchestrator ensemble.
pub trait Specialist: Send + Sync {
    /// Specialist name (e.g., "MSE", "Hydraulic", "WellControl", "Formation")
    fn name(&self) -> &str;

    /// Evaluate the ticket and physics report, returning a vote
    fn evaluate(
        &self,
        ticket: &AdvisoryTicket,
        physics: &DrillingPhysicsReport,
    ) -> SpecialistVote;
}

/// Create the default set of 4 drilling specialists
pub fn default_specialists() -> Vec<Box<dyn Specialist>> {
    vec![
        Box::new(MseSpecialist),
        Box::new(HydraulicSpecialist),
        Box::new(WellControlSpecialist),
        Box::new(FormationSpecialist),
    ]
}
