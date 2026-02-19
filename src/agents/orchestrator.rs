//! Orchestrator - Phase 8 Ensemble Voting for Drilling Intelligence
//!
//! The Orchestrator collects votes from trait-based specialist agents and combines
//! them into a weighted consensus using configurable weights.
//!
//! ## Specialists and Weights
//!
//! Each specialist implements the `Specialist` trait (see `agents::specialists`):
//! 1. **MSE Specialist** (25%): Drilling efficiency analysis
//! 2. **Hydraulic Specialist** (25%): SPP, flow, ECD margin
//! 3. **WellControl Specialist** (30%): Kick/loss, gas, pit volume (safety-critical)
//! 4. **Formation Specialist** (20%): D-exponent, torque trends
//!
//! ## Voting Rules
//!
//! - **WellControl CRITICAL** overrides all others (safety critical)
//! - Otherwise: weighted average of votes converted to severity

use super::specialists::{self, Specialist};
use crate::strategic::advisory::{self, VotingResult};
use crate::types::{
    AdvisoryTicket, DrillingPhysicsReport, FinalSeverity, SpecialistVote, TicketSeverity,
};
use tracing::info;

/// Orchestrator for Phase 8 ensemble voting
pub struct Orchestrator {
    /// Trait-based specialist voters
    specialists: Vec<Box<dyn Specialist>>,
    /// Total reports generated
    reports_generated: u64,
}

impl Orchestrator {
    /// Create a new orchestrator with the default 4 drilling specialists
    pub fn new() -> Self {
        Self {
            specialists: specialists::default_specialists(),
            reports_generated: 0,
        }
    }

    /// Phase 8: Collect specialist votes and produce a weighted consensus
    ///
    /// Returns a `VotingResult` containing all votes, final severity, risk level,
    /// and efficiency score. The caller (typically `AdvisoryComposer`) uses this
    /// to compose the final `StrategicAdvisory`.
    pub fn vote(
        &mut self,
        ticket: &AdvisoryTicket,
        physics: &DrillingPhysicsReport,
    ) -> VotingResult {
        self.reports_generated += 1;

        // Collect votes from all specialists via the trait
        let votes: Vec<SpecialistVote> = self
            .specialists
            .iter()
            .map(|s| s.evaluate(ticket, physics))
            .collect();

        // Check for WellControl CRITICAL override (safety critical)
        let well_control_critical = votes.iter().any(|v| {
            v.specialist == "WellControl" && v.vote == TicketSeverity::Critical
        });

        // Check for any CRITICAL
        let any_critical = votes.iter().any(|v| v.vote == TicketSeverity::Critical);

        // Calculate weighted average
        let weighted_sum: f64 = votes
            .iter()
            .map(|v| (v.vote as u8) as f64 * v.weight)
            .sum();

        // Final severity: WellControl CRITICAL is highest priority override
        let final_severity = if well_control_critical || any_critical {
            FinalSeverity::Critical
        } else {
            FinalSeverity::from(weighted_sum)
        };

        // Calculate efficiency score and risk level
        let efficiency_score =
            advisory::calculate_efficiency_score(&ticket.current_metrics, physics);
        let risk_level =
            advisory::calculate_risk_level(&votes, &ticket.current_metrics);

        // Build voting reasoning string
        let voting_reasoning = build_voting_reasoning(&votes, final_severity);

        info!(
            severity = %final_severity,
            efficiency = efficiency_score,
            risk = %risk_level,
            votes = votes.len(),
            "Orchestrator voting complete"
        );

        VotingResult {
            votes,
            final_severity,
            risk_level,
            efficiency_score,
            voting_reasoning,
        }
    }

    /// Get orchestrator statistics
    pub fn stats(&self) -> u64 {
        self.reports_generated
    }
}

impl Default for Orchestrator {
    fn default() -> Self {
        Self::new()
    }
}

/// Build reasoning string from all votes
fn build_voting_reasoning(votes: &[SpecialistVote], final_severity: FinalSeverity) -> String {
    let vote_summaries: Vec<String> = votes
        .iter()
        .map(|v| format!("{}={} ({:.0}%)", v.specialist, v.vote, v.weight * 100.0))
        .collect();

    format!(
        "Final severity {} from ensemble: {}",
        final_severity,
        vote_summaries.join(", ")
    )
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AnomalyCategory, DrillingMetrics, DrillingPhysicsReport, RigState, RiskLevel, TicketType, TicketSeverity};

    fn ensure_config() {
        if !crate::config::is_initialized() {
            crate::config::init(crate::config::WellConfig::default());
        }
    }

    fn create_test_ticket() -> AdvisoryTicket {
        AdvisoryTicket {
            timestamp: 1000,
            ticket_type: TicketType::RiskWarning,
            category: AnomalyCategory::DrillingEfficiency,
            severity: TicketSeverity::Medium,
            current_metrics: DrillingMetrics {
                state: RigState::Drilling,
                operation: crate::types::Operation::ProductionDrilling,
                mse: 30000.0,
                mse_efficiency: 65.0,
                d_exponent: 1.5,
                dxc: 1.4,
                mse_delta_percent: 0.1,
                flow_balance: 2.0,
                pit_rate: 1.0,
                ecd_margin: 0.5,
                torque_delta_percent: 0.05,
                spp_delta: 30.0,
                is_anomaly: true,
                anomaly_category: AnomalyCategory::DrillingEfficiency,
                anomaly_description: Some("MSE efficiency below optimal".to_string()),
            },
            trigger_parameter: "mse_efficiency".to_string(),
            trigger_value: 65.0,
            threshold_value: 70.0,
            description: "MSE efficiency below optimal".to_string(),
            context: None,
            depth: 10000.0,
            trace_log: Vec::new(),
            cfc_anomaly_score: None,
            cfc_feature_surprises: Vec::new(),
        }
    }

    fn create_test_physics() -> DrillingPhysicsReport {
        DrillingPhysicsReport {
            avg_mse: 30000.0,
            mse_trend: 0.01,
            optimal_mse: 20000.0,
            mse_efficiency: 66.7,
            dxc_trend: 0.02,
            flow_balance_trend: 0.0,
            avg_pit_rate: 0.5,
            formation_hardness: 5.0,
            confidence: 0.9,
            detected_dysfunctions: Vec::new(),
            current_depth: 10000.0,
            current_rop: 50.0,
            current_wob: 25.0,
            current_rpm: 120.0,
            current_torque: 15.0,
            current_spp: 2500.0,
            current_casing_pressure: 0.0,
            current_flow_in: 500.0,
            current_flow_out: 505.0,
            current_mud_weight: 12.0,
            current_ecd: 12.4,
            current_gas: 50.0,
            current_pit_volume: 500.0,
            wob_trend: 0.0,
            rop_trend: 0.0,
            founder_detected: false,
            founder_severity: 0.0,
            optimal_wob_estimate: 0.0,
        }
    }

    #[test]
    fn test_orchestrator_voting() {
        ensure_config();
        let mut orchestrator = Orchestrator::new();
        let ticket = create_test_ticket();
        let physics = create_test_physics();

        let result = orchestrator.vote(&ticket, &physics);

        assert_eq!(result.votes.len(), 4);
        assert!(result.efficiency_score > 0 && result.efficiency_score <= 100);
    }

    #[test]
    fn test_well_control_critical_override() {
        ensure_config();
        let mut orchestrator = Orchestrator::new();
        let mut ticket = create_test_ticket();

        // Set well control critical conditions
        ticket.category = AnomalyCategory::WellControl;
        ticket.current_metrics.flow_balance = 25.0;
        ticket.current_metrics.pit_rate = 20.0;
        ticket.current_metrics.anomaly_category = AnomalyCategory::WellControl;

        let physics = create_test_physics();
        let result = orchestrator.vote(&ticket, &physics);

        // WellControl CRITICAL should override
        assert_eq!(result.final_severity, FinalSeverity::Critical);
        assert_eq!(result.risk_level, RiskLevel::Critical);
    }

    #[test]
    fn test_all_specialists_vote() {
        ensure_config();
        let mut orchestrator = Orchestrator::new();
        let ticket = create_test_ticket();
        let physics = create_test_physics();

        let result = orchestrator.vote(&ticket, &physics);

        // Verify all 4 specialists voted
        let specialists: Vec<&str> = result.votes.iter().map(|v| v.specialist.as_str()).collect();
        assert!(specialists.contains(&"MSE"));
        assert!(specialists.contains(&"Hydraulic"));
        assert!(specialists.contains(&"WellControl"));
        assert!(specialists.contains(&"Formation"));

        // Verify weights sum to 1.0
        let total_weight: f64 = result.votes.iter().map(|v| v.weight).sum();
        assert!(
            (total_weight - 1.0).abs() < 0.001,
            "Weights should sum to 1.0, got {}",
            total_weight
        );
    }

    #[test]
    fn test_efficiency_score() {
        ensure_config();
        let mut orchestrator = Orchestrator::new();

        let mut ticket = create_test_ticket();
        ticket.current_metrics.mse_efficiency = 85.0;
        ticket.current_metrics.is_anomaly = false;

        let mut physics = create_test_physics();
        physics.mse_efficiency = 85.0;

        let result = orchestrator.vote(&ticket, &physics);

        assert!(
            result.efficiency_score >= 80,
            "Good efficiency should result in high score, got {}",
            result.efficiency_score
        );
    }
}
