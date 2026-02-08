//! Advisory Composer - Phase 9 advisory composition with CRITICAL cooldown
//!
//! Merges voting results, physics, context, and LLM recommendation into a
//! final `StrategicAdvisory`. Enforces a 30-second cooldown between CRITICAL
//! advisories to prevent dashboard spam.

use crate::types::{
    AdvisoryTicket, AnomalyCategory, DrillingMetrics, DrillingPhysicsReport, FinalSeverity,
    RiskLevel, SpecialistVote, StrategicAdvisory, TicketSeverity,
};
use std::time::{Duration, Instant};
use tracing::debug;

/// Default cooldown between CRITICAL advisories (30 seconds)
const CRITICAL_COOLDOWN_SECS: u64 = 30;

/// Result of orchestrator ensemble voting (before advisory composition)
#[derive(Debug, Clone)]
pub struct VotingResult {
    /// Individual specialist votes
    pub votes: Vec<SpecialistVote>,
    /// Final severity from weighted voting
    pub final_severity: FinalSeverity,
    /// Overall risk level
    pub risk_level: RiskLevel,
    /// Drilling efficiency score (0-100)
    pub efficiency_score: u8,
    /// Human-readable voting reasoning
    pub voting_reasoning: String,
}

/// Composes final `StrategicAdvisory` from pipeline outputs
///
/// Tracks CRITICAL advisory cooldown to prevent dashboard spam.
pub struct AdvisoryComposer {
    /// Timestamp of last CRITICAL advisory
    last_critical: Option<Instant>,
    /// Cooldown duration between CRITICAL advisories
    cooldown: Duration,
}

impl AdvisoryComposer {
    /// Create a new composer with default 30-second CRITICAL cooldown
    pub fn new() -> Self {
        Self {
            last_critical: None,
            cooldown: Duration::from_secs(CRITICAL_COOLDOWN_SECS),
        }
    }

    /// Compose a strategic advisory from pipeline outputs
    ///
    /// Returns `None` if a CRITICAL advisory is within the cooldown window.
    /// Non-CRITICAL advisories are never suppressed by cooldown.
    pub fn compose(
        &mut self,
        ticket: &AdvisoryTicket,
        physics: &DrillingPhysicsReport,
        context: &[String],
        recommendation: &str,
        expected_benefit: &str,
        reasoning: &str,
        voting: &VotingResult,
    ) -> Option<StrategicAdvisory> {
        // Check CRITICAL cooldown
        if voting.final_severity == FinalSeverity::Critical {
            if let Some(last) = self.last_critical {
                if last.elapsed() < self.cooldown {
                    debug!(
                        cooldown_remaining_ms = (self.cooldown - last.elapsed()).as_millis(),
                        "CRITICAL advisory suppressed by cooldown"
                    );
                    return None;
                }
            }
            self.last_critical = Some(Instant::now());
        }

        // Combine reasoning
        let full_reasoning = if reasoning.is_empty() {
            voting.voting_reasoning.clone()
        } else {
            format!("{}\n\nVoting: {}", reasoning, voting.voting_reasoning)
        };

        Some(StrategicAdvisory {
            timestamp: ticket.timestamp,
            efficiency_score: voting.efficiency_score,
            risk_level: voting.risk_level,
            severity: voting.final_severity,
            recommendation: recommendation.to_string(),
            expected_benefit: expected_benefit.to_string(),
            reasoning: full_reasoning,
            votes: voting.votes.clone(),
            physics_report: physics.clone(),
            context_used: context.to_vec(),
            trace_log: ticket.trace_log.clone(),
        })
    }
}

impl Default for AdvisoryComposer {
    fn default() -> Self {
        Self::new()
    }
}

/// Calculate efficiency score from metrics and physics (0-100)
pub fn calculate_efficiency_score(
    metrics: &DrillingMetrics,
    physics: &DrillingPhysicsReport,
) -> u8 {
    let mse_score = physics.mse_efficiency;

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

    let trend_bonus = if physics.mse_trend < 0.0 && physics.mse_efficiency > 70.0 {
        5.0
    } else {
        0.0
    };

    ((mse_score - anomaly_penalty + trend_bonus).clamp(0.0, 100.0)) as u8
}

/// Calculate risk level from specialist votes and metrics
pub fn calculate_risk_level(
    votes: &[SpecialistVote],
    metrics: &DrillingMetrics,
) -> RiskLevel {
    let critical_count = votes
        .iter()
        .filter(|v| v.vote == TicketSeverity::Critical)
        .count();
    let high_count = votes
        .iter()
        .filter(|v| v.vote == TicketSeverity::High)
        .count();

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DrillingMetrics, RigState, Operation};

    fn make_voting_result(severity: FinalSeverity) -> VotingResult {
        VotingResult {
            votes: vec![],
            final_severity: severity,
            risk_level: RiskLevel::Low,
            efficiency_score: 80,
            voting_reasoning: "test".to_string(),
        }
    }

    fn make_test_ticket() -> AdvisoryTicket {
        AdvisoryTicket {
            timestamp: 1000,
            ticket_type: crate::types::TicketType::Optimization,
            category: AnomalyCategory::DrillingEfficiency,
            severity: TicketSeverity::Low,
            current_metrics: DrillingMetrics {
                state: RigState::Drilling,
                operation: Operation::ProductionDrilling,
                ..DrillingMetrics::default()
            },
            trigger_parameter: "test".to_string(),
            trigger_value: 0.0,
            threshold_value: 0.0,
            description: "test".to_string(),
            context: None,
            depth: 10000.0,
            trace_log: Vec::new(),
        }
    }

    #[test]
    fn test_critical_cooldown() {
        let mut composer = AdvisoryComposer::new();
        let ticket = make_test_ticket();
        let physics = DrillingPhysicsReport::default();
        let critical_voting = make_voting_result(FinalSeverity::Critical);

        // First CRITICAL should pass
        let result = composer.compose(
            &ticket, &physics, &[], "rec", "ben", "rea", &critical_voting,
        );
        assert!(result.is_some());

        // Second CRITICAL within cooldown should be suppressed
        let result = composer.compose(
            &ticket, &physics, &[], "rec", "ben", "rea", &critical_voting,
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_non_critical_not_suppressed() {
        let mut composer = AdvisoryComposer::new();
        let ticket = make_test_ticket();
        let physics = DrillingPhysicsReport::default();
        let critical_voting = make_voting_result(FinalSeverity::Critical);
        let low_voting = make_voting_result(FinalSeverity::Low);

        // Send a CRITICAL
        composer.compose(&ticket, &physics, &[], "rec", "ben", "rea", &critical_voting);

        // Non-CRITICAL should still pass even within cooldown
        let result = composer.compose(
            &ticket, &physics, &[], "rec", "ben", "rea", &low_voting,
        );
        assert!(result.is_some());
    }
}
