//! WellControl Specialist (30% weight) - SAFETY CRITICAL
//!
//! Highest-weighted specialist because well control events (kicks, losses)
//! are the most dangerous situations on a drilling rig.

use super::Specialist;
use crate::types::{
    AdvisoryTicket, AnomalyCategory, DrillingPhysicsReport, SpecialistVote, TicketSeverity,
};

/// WellControl Specialist evaluates kick/loss indicators, gas, and pit volume
pub struct WellControlSpecialist;

impl Specialist for WellControlSpecialist {
    fn name(&self) -> &str {
        "WellControl"
    }

    fn evaluate(
        &self,
        ticket: &AdvisoryTicket,
        _physics: &DrillingPhysicsReport,
    ) -> SpecialistVote {
        let metrics = &ticket.current_metrics;
        let flow_balance = metrics.flow_balance.abs();
        let pit_rate = metrics.pit_rate.abs();

        let cfg = crate::config::get();
        let wc = &cfg.thresholds.well_control;

        // Direct category override for well control
        if ticket.category == AnomalyCategory::WellControl
            && (flow_balance > wc.flow_imbalance_critical_gpm
                || pit_rate > wc.pit_rate_critical_bbl_hr)
        {
            return SpecialistVote {
                specialist: "WellControl".to_string(),
                vote: TicketSeverity::Critical,
                weight: cfg.ensemble_weights.well_control,
                reasoning: format!(
                    "CRITICAL: Flow imbalance {:.1} gpm, pit rate {:.1} bbl/hr - immediate action",
                    metrics.flow_balance, metrics.pit_rate
                ),
            };
        }

        let (vote, reasoning) = if flow_balance > wc.flow_imbalance_critical_gpm
            || pit_rate > wc.pit_rate_critical_bbl_hr
        {
            (
                TicketSeverity::Critical,
                format!(
                    "Flow imbalance {:.1} gpm, pit rate {:.1} bbl/hr - well control event",
                    metrics.flow_balance, metrics.pit_rate
                ),
            )
        } else if flow_balance > wc.flow_imbalance_warning_gpm
            || pit_rate > wc.pit_rate_warning_bbl_hr
        {
            (
                TicketSeverity::High,
                format!(
                    "Flow imbalance {:.1} gpm, pit rate {:.1} bbl/hr - monitor closely",
                    metrics.flow_balance, metrics.pit_rate
                ),
            )
        } else if flow_balance > 5.0 || pit_rate > 2.0 {
            (
                TicketSeverity::Medium,
                format!(
                    "Minor flow imbalance {:.1} gpm - continue monitoring",
                    metrics.flow_balance
                ),
            )
        } else if !metrics.flow_data_available {
            (
                TicketSeverity::Low,
                "Flow sensor data unavailable - cannot assess flow balance".to_string(),
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
            weight: cfg.ensemble_weights.well_control,
            reasoning,
        }
    }
}
