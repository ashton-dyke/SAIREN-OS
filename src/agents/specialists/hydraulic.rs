//! Hydraulic Specialist (25% weight) - SPP, flow, ECD margin analysis

use super::Specialist;
use crate::types::{AdvisoryTicket, DrillingPhysicsReport, SpecialistVote, TicketSeverity};

/// Hydraulic Specialist evaluates standpipe pressure, flow rates, and ECD margin
pub struct HydraulicSpecialist;

impl Specialist for HydraulicSpecialist {
    fn name(&self) -> &str {
        "Hydraulic"
    }

    fn evaluate(
        &self,
        ticket: &AdvisoryTicket,
        _physics: &DrillingPhysicsReport,
    ) -> SpecialistVote {
        let metrics = &ticket.current_metrics;
        let ecd_margin = metrics.ecd_margin;
        let spp_delta = if metrics.spp_delta.is_finite() { metrics.spp_delta.abs() } else { 0.0 };

        let cfg = crate::config::get();

        let (vote, reasoning) = if ecd_margin < cfg.thresholds.hydraulics.ecd_margin_critical_ppg {
            (
                TicketSeverity::Critical,
                format!(
                    "ECD margin {:.2} ppg critically low - fracture risk",
                    ecd_margin
                ),
            )
        } else if ecd_margin < cfg.thresholds.hydraulics.ecd_margin_warning_ppg {
            (
                TicketSeverity::High,
                format!(
                    "ECD margin {:.2} ppg low - reduce flow rate or ROP",
                    ecd_margin
                ),
            )
        } else if spp_delta > cfg.thresholds.hydraulics.spp_deviation_critical_psi {
            (
                TicketSeverity::High,
                format!(
                    "SPP deviation {:.0} psi significant - check for washout/pack-off",
                    spp_delta
                ),
            )
        } else if spp_delta > cfg.thresholds.hydraulics.spp_deviation_warning_psi {
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
            weight: cfg.ensemble_weights.hydraulic,
            reasoning,
        }
    }
}
