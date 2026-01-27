//! Orchestrator - Phase 8 Ensemble Voting for Drilling Intelligence
//!
//! The Orchestrator collects votes from 4 drilling specialist agents and combines them
//! into a final decision using weighted voting.
//!
//! ## Specialists and Weights
//!
//! 1. **MSE Specialist** (25%): Drilling efficiency analysis
//!    - MSE efficiency < 50% → HIGH
//!    - MSE efficiency 50-70% → MEDIUM
//!    - MSE efficiency > 70% → LOW
//!
//! 2. **Hydraulic Specialist** (25%): SPP, flow, ECD margin
//!    - ECD margin < 0.1 ppg → CRITICAL
//!    - ECD margin 0.1-0.3 ppg → HIGH
//!    - SPP deviation > 100 psi → MEDIUM
//!    - Otherwise → LOW
//!
//! 3. **WellControl Specialist** (30%): Kick/loss, gas, pit volume (HIGHEST for safety)
//!    - Flow imbalance > 20 gpm OR pit rate > 15 bbl/hr → CRITICAL
//!    - Flow imbalance > 10 gpm OR gas > 200 units → HIGH
//!    - Any well control indicator → MEDIUM
//!    - Otherwise → LOW
//!
//! 4. **Formation Specialist** (20%): D-exponent, torque trends
//!    - D-exponent decreasing (abnormal pressure) → HIGH
//!    - Significant formation change → MEDIUM
//!    - Otherwise → LOW
//!
//! ## Voting Rules
//!
//! - **WellControl CRITICAL** overrides all others (safety critical)
//! - Otherwise: weighted average of votes converted to severity

use crate::types::{
    drilling_thresholds, weights, AdvisoryTicket, AnomalyCategory, DrillingMetrics,
    DrillingPhysicsReport, FinalSeverity, RiskLevel, SpecialistVote, StrategicAdvisory,
    TicketEvent, TicketSeverity, TicketStage,
};
use tracing::info;

/// Orchestrator for Phase 8 ensemble voting
pub struct Orchestrator {
    /// Total reports generated
    reports_generated: u64,
}

impl Orchestrator {
    /// Create a new orchestrator
    pub fn new() -> Self {
        Self {
            reports_generated: 0,
        }
    }

    /// Phase 8: Collect votes and generate final strategic advisory
    ///
    /// Inputs:
    /// - `ticket`: The advisory ticket from Phase 3
    /// - `physics`: The drilling physics report from Phase 5
    /// - `context`: Context snippets from Phase 6
    /// - `recommendation`: Recommendation string from Phase 7 LLM
    /// - `reasoning`: Reasoning string from Phase 7 LLM
    ///
    /// Output: Complete StrategicAdvisory with final severity from ensemble voting
    pub fn vote(
        &mut self,
        ticket: &AdvisoryTicket,
        physics: &DrillingPhysicsReport,
        context: &[String],
        recommendation: &str,
        expected_benefit: &str,
        reasoning: &str,
    ) -> StrategicAdvisory {
        self.reports_generated += 1;

        // Collect votes from all 4 specialists
        let votes = vec![
            self.mse_specialist_vote(&ticket.current_metrics, physics),
            self.hydraulic_specialist_vote(&ticket.current_metrics),
            self.well_control_specialist_vote(&ticket.current_metrics, ticket.category),
            self.formation_specialist_vote(&ticket.current_metrics, physics),
        ];

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
        let final_severity = if well_control_critical {
            FinalSeverity::Critical
        } else if any_critical {
            FinalSeverity::Critical
        } else {
            FinalSeverity::from(weighted_sum)
        };

        // Calculate efficiency score
        let efficiency_score = self.calculate_efficiency_score(&ticket.current_metrics, physics);

        // Calculate risk level
        let risk_level = self.calculate_risk_level(&votes, &ticket.current_metrics);

        // Build voting reasoning string
        let voting_reasoning = self.build_voting_reasoning(&votes, final_severity);

        // Combine reasoning
        let full_reasoning = if reasoning.is_empty() {
            voting_reasoning
        } else {
            format!("{}\n\nVoting: {}", reasoning, voting_reasoning)
        };

        info!(
            severity = %final_severity,
            efficiency = efficiency_score,
            risk = %risk_level,
            votes = votes.len(),
            "Orchestrator voting complete"
        );

        StrategicAdvisory {
            timestamp: ticket.timestamp,
            efficiency_score,
            risk_level,
            severity: final_severity,
            recommendation: recommendation.to_string(),
            expected_benefit: expected_benefit.to_string(),
            reasoning: full_reasoning,
            votes,
            physics_report: physics.clone(),
            context_used: context.to_vec(),
            trace_log: ticket.trace_log.clone(),
        }
    }

    /// MSE Specialist vote (25% weight) - Drilling efficiency
    fn mse_specialist_vote(
        &self,
        metrics: &DrillingMetrics,
        physics: &DrillingPhysicsReport,
    ) -> SpecialistVote {
        let efficiency = physics.mse_efficiency;

        let (vote, reasoning) = if efficiency < drilling_thresholds::MSE_EFFICIENCY_POOR {
            (
                TicketSeverity::High,
                format!(
                    "MSE efficiency {:.0}% critically low - significant optimization needed",
                    efficiency
                ),
            )
        } else if efficiency < drilling_thresholds::MSE_EFFICIENCY_WARNING {
            (
                TicketSeverity::Medium,
                format!(
                    "MSE efficiency {:.0}% below optimal - consider parameter adjustment",
                    efficiency
                ),
            )
        } else {
            (
                TicketSeverity::Low,
                format!("MSE efficiency {:.0}% adequate for current formation", efficiency),
            )
        };

        SpecialistVote {
            specialist: "MSE".to_string(),
            vote,
            weight: weights::MSE,
            reasoning,
        }
    }

    /// Hydraulic Specialist vote (25% weight) - SPP, flow, ECD margin
    fn hydraulic_specialist_vote(&self, metrics: &DrillingMetrics) -> SpecialistVote {
        let ecd_margin = metrics.ecd_margin;
        let spp_delta = metrics.spp_delta.abs();

        let (vote, reasoning) = if ecd_margin < drilling_thresholds::ECD_MARGIN_CRITICAL {
            (
                TicketSeverity::Critical,
                format!(
                    "ECD margin {:.2} ppg critically low - fracture risk",
                    ecd_margin
                ),
            )
        } else if ecd_margin < drilling_thresholds::ECD_MARGIN_WARNING {
            (
                TicketSeverity::High,
                format!(
                    "ECD margin {:.2} ppg low - reduce flow rate or ROP",
                    ecd_margin
                ),
            )
        } else if spp_delta > drilling_thresholds::SPP_DEVIATION_CRITICAL {
            (
                TicketSeverity::High,
                format!("SPP deviation {:.0} psi significant - check for washout/pack-off", spp_delta),
            )
        } else if spp_delta > drilling_thresholds::SPP_DEVIATION_WARNING {
            (
                TicketSeverity::Medium,
                format!("SPP deviation {:.0} psi elevated - monitor", spp_delta),
            )
        } else {
            (
                TicketSeverity::Low,
                format!(
                    "Hydraulics normal - ECD margin {:.2} ppg, SPP stable",
                    ecd_margin
                ),
            )
        };

        SpecialistVote {
            specialist: "Hydraulic".to_string(),
            vote,
            weight: weights::HYDRAULIC,
            reasoning,
        }
    }

    /// WellControl Specialist vote (30% weight) - SAFETY CRITICAL
    fn well_control_specialist_vote(
        &self,
        metrics: &DrillingMetrics,
        category: AnomalyCategory,
    ) -> SpecialistVote {
        let flow_balance = metrics.flow_balance.abs();
        let pit_rate = metrics.pit_rate.abs();

        // Direct category override for well control
        if category == AnomalyCategory::WellControl {
            // Already flagged as well control issue
            if flow_balance > drilling_thresholds::FLOW_IMBALANCE_CRITICAL
                || pit_rate > drilling_thresholds::PIT_RATE_CRITICAL
            {
                return SpecialistVote {
                    specialist: "WellControl".to_string(),
                    vote: TicketSeverity::Critical,
                    weight: weights::WELL_CONTROL,
                    reasoning: format!(
                        "CRITICAL: Flow imbalance {:.1} gpm, pit rate {:.1} bbl/hr - immediate action",
                        metrics.flow_balance, metrics.pit_rate
                    ),
                };
            }
        }

        let (vote, reasoning) = if flow_balance > drilling_thresholds::FLOW_IMBALANCE_CRITICAL
            || pit_rate > drilling_thresholds::PIT_RATE_CRITICAL
        {
            (
                TicketSeverity::Critical,
                format!(
                    "Flow imbalance {:.1} gpm, pit rate {:.1} bbl/hr - well control event",
                    metrics.flow_balance, metrics.pit_rate
                ),
            )
        } else if flow_balance > drilling_thresholds::FLOW_IMBALANCE_WARNING
            || pit_rate > drilling_thresholds::PIT_RATE_WARNING
        {
            (
                TicketSeverity::High,
                format!(
                    "Flow imbalance {:.1} gpm, pit rate {:.1} bbl/hr - monitor closely",
                    metrics.flow_balance, metrics.pit_rate
                ),
            )
        } else if flow_balance > 5.0 || pit_rate.abs() > 2.0 {
            (
                TicketSeverity::Medium,
                format!(
                    "Minor flow imbalance {:.1} gpm - continue monitoring",
                    metrics.flow_balance
                ),
            )
        } else {
            (
                TicketSeverity::Low,
                "Well control parameters normal - flow balanced".to_string(),
            )
        };

        SpecialistVote {
            specialist: "WellControl".to_string(),
            vote,
            weight: weights::WELL_CONTROL,
            reasoning,
        }
    }

    /// Formation Specialist vote (20% weight) - D-exponent, torque trends
    fn formation_specialist_vote(
        &self,
        metrics: &DrillingMetrics,
        physics: &DrillingPhysicsReport,
    ) -> SpecialistVote {
        let dxc_trend = physics.dxc_trend;
        let formation_hardness = physics.formation_hardness;

        let (vote, reasoning) = if dxc_trend < -0.15 {
            // Decreasing d-exponent can indicate abnormal pore pressure
            (
                TicketSeverity::High,
                format!(
                    "D-exponent decreasing trend ({:.3}) - possible abnormal pressure",
                    dxc_trend
                ),
            )
        } else if dxc_trend.abs() > 0.1 {
            // Significant formation change
            let change_type = if dxc_trend > 0.0 { "harder" } else { "softer" };
            (
                TicketSeverity::Medium,
                format!(
                    "Formation change detected - drilling into {} rock (hardness {:.1}/10)",
                    change_type, formation_hardness
                ),
            )
        } else {
            (
                TicketSeverity::Low,
                format!(
                    "Formation stable (hardness {:.1}/10) - no significant changes",
                    formation_hardness
                ),
            )
        };

        SpecialistVote {
            specialist: "Formation".to_string(),
            vote,
            weight: weights::FORMATION,
            reasoning,
        }
    }

    /// Calculate efficiency score from metrics and physics (0-100)
    fn calculate_efficiency_score(
        &self,
        metrics: &DrillingMetrics,
        physics: &DrillingPhysicsReport,
    ) -> u8 {
        // Base score from MSE efficiency
        let mse_score = physics.mse_efficiency;

        // Penalty for anomalies
        let anomaly_penalty = if metrics.is_anomaly {
            match metrics.anomaly_category {
                AnomalyCategory::WellControl => 30.0,
                AnomalyCategory::Hydraulics => 20.0,
                AnomalyCategory::Mechanical => 15.0,
                AnomalyCategory::DrillingEfficiency => 10.0,
                AnomalyCategory::Formation => 5.0,
                AnomalyCategory::None => 0.0,
            }
        } else {
            0.0
        };

        // Bonus for good trends
        let trend_bonus = if physics.mse_trend < 0.0 && physics.mse_efficiency > 70.0 {
            5.0 // MSE decreasing while efficiency good
        } else {
            0.0
        };

        ((mse_score - anomaly_penalty + trend_bonus).clamp(0.0, 100.0)) as u8
    }

    /// Calculate risk level from votes and metrics
    fn calculate_risk_level(
        &self,
        votes: &[SpecialistVote],
        metrics: &DrillingMetrics,
    ) -> RiskLevel {
        // Check for critical votes
        let critical_count = votes.iter().filter(|v| v.vote == TicketSeverity::Critical).count();
        let high_count = votes.iter().filter(|v| v.vote == TicketSeverity::High).count();

        // WellControl issues are always elevated
        if metrics.anomaly_category == AnomalyCategory::WellControl {
            if critical_count > 0 {
                return RiskLevel::Critical;
            }
            return RiskLevel::High;
        }

        if critical_count > 0 {
            RiskLevel::Critical
        } else if high_count >= 2 {
            RiskLevel::High
        } else if high_count >= 1 {
            RiskLevel::Elevated
        } else {
            RiskLevel::Low
        }
    }

    /// Build reasoning string from all votes
    fn build_voting_reasoning(&self, votes: &[SpecialistVote], final_severity: FinalSeverity) -> String {
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{RigState, TicketType};

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
            depth: 10000.0,
            trace_log: Vec::new(),
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
            // Founder detection fields (V0.6)
            wob_trend: 0.0,
            rop_trend: 0.0,
            founder_detected: false,
            founder_severity: 0.0,
            optimal_wob_estimate: 0.0,
        }
    }

    #[test]
    fn test_orchestrator_voting() {
        let mut orchestrator = Orchestrator::new();
        let ticket = create_test_ticket();
        let physics = create_test_physics();
        let context = vec!["Formation: Medium hardite".to_string()];

        let advisory = orchestrator.vote(
            &ticket,
            &physics,
            &context,
            "Optimize WOB/RPM combination",
            "Expected 20-30% ROP improvement",
            "MSE analysis indicates sub-optimal drilling parameters",
        );

        assert_eq!(advisory.votes.len(), 4);
        assert!(advisory.efficiency_score > 0 && advisory.efficiency_score <= 100);
        assert!(!advisory.recommendation.is_empty());
    }

    #[test]
    fn test_well_control_critical_override() {
        let mut orchestrator = Orchestrator::new();
        let mut ticket = create_test_ticket();

        // Set well control critical conditions
        ticket.category = AnomalyCategory::WellControl;
        ticket.current_metrics.flow_balance = 25.0;
        ticket.current_metrics.pit_rate = 20.0;
        ticket.current_metrics.anomaly_category = AnomalyCategory::WellControl;

        let physics = create_test_physics();

        let advisory = orchestrator.vote(
            &ticket,
            &physics,
            &[],
            "Initiate well control procedures",
            "Prevent blowout",
            "Critical flow imbalance detected",
        );

        // WellControl CRITICAL should override
        assert_eq!(advisory.severity, FinalSeverity::Critical);
        assert_eq!(advisory.risk_level, RiskLevel::Critical);
    }

    #[test]
    fn test_all_specialists_vote() {
        let mut orchestrator = Orchestrator::new();
        let ticket = create_test_ticket();
        let physics = create_test_physics();

        let advisory = orchestrator.vote(
            &ticket,
            &physics,
            &[],
            "Test recommendation",
            "Test benefit",
            "Test reasoning",
        );

        // Verify all 4 specialists voted
        let specialists: Vec<&str> = advisory.votes.iter().map(|v| v.specialist.as_str()).collect();
        assert!(specialists.contains(&"MSE"));
        assert!(specialists.contains(&"Hydraulic"));
        assert!(specialists.contains(&"WellControl"));
        assert!(specialists.contains(&"Formation"));

        // Verify weights sum to 1.0
        let total_weight: f64 = advisory.votes.iter().map(|v| v.weight).sum();
        assert!(
            (total_weight - 1.0).abs() < 0.001,
            "Weights should sum to 1.0, got {}",
            total_weight
        );
    }

    #[test]
    fn test_efficiency_score() {
        let mut orchestrator = Orchestrator::new();

        // Test with good efficiency
        let mut ticket = create_test_ticket();
        ticket.current_metrics.mse_efficiency = 85.0;
        ticket.current_metrics.is_anomaly = false;

        let mut physics = create_test_physics();
        physics.mse_efficiency = 85.0;

        let advisory = orchestrator.vote(
            &ticket,
            &physics,
            &[],
            "Continue current parameters",
            "Maintain optimal performance",
            "",
        );

        assert!(
            advisory.efficiency_score >= 80,
            "Good efficiency should result in high score, got {}",
            advisory.efficiency_score
        );
    }
}
