//! Formation Specialist (20% weight) - D-exponent and torque trend analysis

use super::Specialist;
use crate::types::{AdvisoryTicket, DrillingPhysicsReport, SpecialistVote, TicketSeverity};

/// Formation Specialist evaluates d-exponent trends and formation changes
pub struct FormationSpecialist;

impl Specialist for FormationSpecialist {
    fn name(&self) -> &str {
        "Formation"
    }

    fn evaluate(
        &self,
        _ticket: &AdvisoryTicket,
        physics: &DrillingPhysicsReport,
    ) -> SpecialistVote {
        let dxc_trend = physics.dxc_trend;
        let formation_hardness = physics.formation_hardness;

        let cfg = crate::config::get();
        let dexp_decrease = cfg.thresholds.formation.dexp_decrease_warning;
        let dxc_threshold = cfg.thresholds.strategic_verification.dxc_change_threshold;

        let (vote, reasoning) = if dxc_trend < dexp_decrease {
            // Decreasing d-exponent can indicate abnormal pore pressure
            (
                TicketSeverity::High,
                format!(
                    "D-exponent decreasing trend ({:.3}) - possible abnormal pressure",
                    dxc_trend
                ),
            )
        } else if dxc_trend.abs() > dxc_threshold {
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
            weight: cfg.ensemble_weights.formation,
            reasoning,
        }
    }
}
