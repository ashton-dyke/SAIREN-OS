//! MSE Specialist (25% weight) - Drilling efficiency analysis

use super::Specialist;
use crate::types::{AdvisoryTicket, DrillingPhysicsReport, SpecialistVote, TicketSeverity};

/// MSE Specialist evaluates drilling efficiency via Mechanical Specific Energy
pub struct MseSpecialist;

impl Specialist for MseSpecialist {
    fn name(&self) -> &str {
        "MSE"
    }

    fn evaluate(
        &self,
        _ticket: &AdvisoryTicket,
        physics: &DrillingPhysicsReport,
    ) -> SpecialistVote {
        let efficiency = physics.mse_efficiency;
        let cfg = crate::config::get();

        let (vote, reasoning) = if efficiency <= 0.0 {
            (
                TicketSeverity::Low,
                "MSE not assessable - insufficient drilling data".to_string(),
            )
        } else if efficiency < cfg.thresholds.mse.efficiency_poor_percent {
            (
                TicketSeverity::High,
                format!(
                    "MSE efficiency {:.0}% critically low - significant optimization needed",
                    efficiency
                ),
            )
        } else if efficiency < cfg.thresholds.mse.efficiency_warning_percent {
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
            weight: cfg.ensemble_weights.mse,
            reasoning,
        }
    }
}
