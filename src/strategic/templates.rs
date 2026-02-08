//! Template-based advisory generation (Phase 7 fallback)
//!
//! Provides structured advisory templates for each `AnomalyCategory` that produce
//! useful recommendations when the LLM is unavailable, timed out, or returned
//! garbage. Templates include actual metric values from the current drilling state.
//!
//! Templates produce advisories with reduced confidence (0.70 vs LLM's 0.85+)
//! and are tagged with `source: "template"` so the dashboard can display a banner.

use crate::types::{
    AdvisoryTicket, AnomalyCategory, Campaign, DrillingPhysicsReport,
};

/// Result of template-based advisory generation
pub struct TemplateAdvisory {
    /// Primary recommendation (actionable advice)
    pub recommendation: String,
    /// Expected benefit if recommendation is followed
    pub expected_benefit: String,
    /// Technical reasoning supporting the recommendation
    pub reasoning: String,
    /// Template-generated advisories have lower confidence
    pub confidence: f64,
    /// Source identifier for dashboard banner
    pub source: &'static str,
}

/// Generate a template advisory for the given anomaly category
///
/// Every `AnomalyCategory` variant has a dedicated template that produces
/// actionable text including actual metric values (e.g., "Torque at 18.5 kft-lb,
/// 23% above baseline").
pub fn template_advisory(
    ticket: &AdvisoryTicket,
    physics: &DrillingPhysicsReport,
    campaign: Campaign,
) -> TemplateAdvisory {
    let (recommendation, expected_benefit, reasoning) = match ticket.category {
        AnomalyCategory::WellControl => well_control_template(ticket, physics, campaign),
        AnomalyCategory::DrillingEfficiency => efficiency_template(ticket, physics),
        AnomalyCategory::Hydraulics => hydraulics_template(ticket, physics),
        AnomalyCategory::Mechanical => mechanical_template(ticket, physics),
        AnomalyCategory::Formation => formation_template(ticket, physics),
        AnomalyCategory::None => normal_template(physics),
    };

    TemplateAdvisory {
        recommendation,
        expected_benefit,
        reasoning,
        confidence: 0.70,
        source: "template",
    }
}

fn well_control_template(
    ticket: &AdvisoryTicket,
    physics: &DrillingPhysicsReport,
    campaign: Campaign,
) -> (String, String, String) {
    let metrics = &ticket.current_metrics;
    let flow = metrics.flow_balance;
    let pit = metrics.pit_rate;
    let ecd = metrics.ecd_margin;

    let campaign_note = match campaign {
        Campaign::PlugAbandonment => " (P&A mode: tighter flow tolerance)",
        _ => "",
    };

    (
        format!(
            "WELL CONTROL: Verify flow balance and pit levels immediately{}. \
             Flow imbalance {:.1} gpm, pit rate {:.1} bbl/hr. \
             Check trip tank, confirm flow out reading, prepare for shut-in if trend continues. \
             Current mud weight {:.1} ppg, ECD {:.1} ppg at {:.0} ft.",
            campaign_note,
            flow, pit,
            physics.current_mud_weight, physics.current_ecd, physics.current_depth
        ),
        "Well control incident prevention — immediate safety priority".to_string(),
        format!(
            "Flow imbalance of {:.1} gpm detected with pit rate {:.1} bbl/hr. \
             ECD margin: {:.2} ppg. Flow balance trend: {:.1} gpm/10min. \
             Gas reading: {:.0} units. Confidence limited — template-based analysis.",
            flow, pit, ecd, physics.flow_balance_trend, physics.current_gas
        ),
    )
}

fn efficiency_template(
    ticket: &AdvisoryTicket,
    physics: &DrillingPhysicsReport,
) -> (String, String, String) {
    let eff = physics.mse_efficiency;
    let optimal = physics.optimal_mse;
    let avg = physics.avg_mse;
    let trend_dir = if physics.mse_trend > 0.0 { "increasing (worsening)" } else { "stable/improving" };

    let action = if eff < 50.0 {
        format!(
            "Significant efficiency loss. Reduce WOB by 5 klbs or increase RPM by 10-15. \
             Current WOB {:.0} klbs, RPM {:.0}. Target MSE: {:.0} psi.",
            physics.current_wob, physics.current_rpm, optimal
        )
    } else {
        format!(
            "Consider fine-tuning WOB/RPM combination. Current efficiency {:.0}%. \
             Current WOB {:.0} klbs, RPM {:.0}, ROP {:.1} ft/hr.",
            eff, physics.current_wob, physics.current_rpm, physics.current_rop
        )
    };

    (
        action,
        format!(
            "Potential {:.0}% efficiency improvement, reduced bit wear, improved ROP",
            (100.0 - eff).min(30.0)
        ),
        format!(
            "MSE {}: avg {:.0} psi vs optimal {:.0} psi ({:.0}% efficiency). \
             Torque {:.1} kft-lb at {:.0} ft depth. Formation hardness {:.1}/10.",
            trend_dir, avg, optimal, eff,
            physics.current_torque, physics.current_depth, physics.formation_hardness
        ),
    )
}

fn hydraulics_template(
    ticket: &AdvisoryTicket,
    physics: &DrillingPhysicsReport,
) -> (String, String, String) {
    let metrics = &ticket.current_metrics;
    let spp_delta = metrics.spp_delta;
    let ecd = metrics.ecd_margin;

    let action = if ecd < 0.3 {
        format!(
            "ECD margin critically low at {:.2} ppg. Reduce flow rate or ROP immediately. \
             SPP {:.0} psi, flow in {:.0} gpm.",
            ecd, physics.current_spp, physics.current_flow_in
        )
    } else if spp_delta.abs() > 100.0 {
        format!(
            "SPP deviation {:.0} psi — check for washout (drop) or pack-off (rise). \
             Current SPP {:.0} psi, flow {:.0} gpm. Monitor over next 5 minutes.",
            spp_delta, physics.current_spp, physics.current_flow_in
        )
    } else {
        format!(
            "Monitor standpipe pressure and flow rates. SPP deviation {:.0} psi, \
             ECD margin {:.2} ppg. No immediate action required.",
            spp_delta, ecd
        )
    };

    (
        action,
        "Hydraulic efficiency optimization, equipment damage prevention".to_string(),
        format!(
            "Flow balance trend: {:.1} gpm/10min. ECD margin: {:.2} ppg. \
             SPP delta: {:.0} psi. Mud weight in {:.1} ppg, ECD {:.1} ppg.",
            physics.flow_balance_trend, ecd, spp_delta,
            physics.current_mud_weight, physics.current_ecd
        ),
    )
}

fn mechanical_template(
    ticket: &AdvisoryTicket,
    physics: &DrillingPhysicsReport,
) -> (String, String, String) {
    let metrics = &ticket.current_metrics;
    let torque_delta = metrics.torque_delta_percent;

    let action = if physics.founder_detected {
        format!(
            "FOUNDER CONDITION: WOB exceeds optimal ({:.0} klbs, optimal ~{:.0} klbs). \
             ROP no longer responding to WOB increases. Reduce WOB by 5-10 klbs.",
            physics.current_wob, physics.optimal_wob_estimate
        )
    } else if torque_delta > 0.15 {
        format!(
            "Torque elevated {:.0}% above baseline ({:.1} kft-lb). \
             Monitor for pack-off. Consider backreaming if torque continues to rise. \
             Reduce WOB if stick-slip develops.",
            torque_delta * 100.0, physics.current_torque
        )
    } else {
        format!(
            "Mechanical parameter deviation detected. Torque {:.1} kft-lb (delta {:.0}%). \
             Continue monitoring torque and drag trends.",
            physics.current_torque, torque_delta * 100.0
        )
    };

    (
        action,
        "Pack-off/stick-slip prevention, reduced NPT risk".to_string(),
        format!(
            "Torque delta {:.0}% at {:.0} ft. WOB {:.0} klbs, RPM {:.0}. \
             Founder detected: {}. Current ROP {:.1} ft/hr.",
            torque_delta * 100.0, physics.current_depth,
            physics.current_wob, physics.current_rpm,
            physics.founder_detected, physics.current_rop
        ),
    )
}

fn formation_template(
    _ticket: &AdvisoryTicket,
    physics: &DrillingPhysicsReport,
) -> (String, String, String) {
    let dxc_trend = physics.dxc_trend;
    let hardness = physics.formation_hardness;

    let action = if dxc_trend < -0.1 {
        format!(
            "D-exponent DECREASING ({:.3}) — possible abnormal pore pressure. \
             Monitor mud weight vs pore pressure closely. Consider increasing mud weight. \
             Current depth {:.0} ft, formation hardness {:.1}/10.",
            dxc_trend, physics.current_depth, hardness
        )
    } else if dxc_trend.abs() > 0.05 {
        let dir = if dxc_trend > 0.0 { "harder" } else { "softer" };
        format!(
            "Formation transition detected — drilling into {} rock. \
             Adjust WOB/RPM for new formation. D-exponent trend {:.3} at {:.0} ft.",
            dir, dxc_trend, physics.current_depth
        )
    } else {
        format!(
            "Formation change indicated. D-exponent trend {:.3}, hardness {:.1}/10. \
             Continue with current parameters, monitor ROP response.",
            dxc_trend, hardness
        )
    };

    (
        action,
        "Optimized drilling through formation transition, pore pressure awareness".to_string(),
        format!(
            "D-exponent trend: {:.3}. Formation hardness: {:.1}/10. \
             MSE efficiency: {:.0}%. Current ROP: {:.1} ft/hr at {:.0} ft.",
            dxc_trend, hardness, physics.mse_efficiency,
            physics.current_rop, physics.current_depth
        ),
    )
}

fn normal_template(physics: &DrillingPhysicsReport) -> (String, String, String) {
    (
        format!(
            "Continue monitoring drilling parameters. ROP {:.1} ft/hr, \
             efficiency {:.0}% at {:.0} ft.",
            physics.current_rop, physics.mse_efficiency, physics.current_depth
        ),
        "Maintained operational efficiency".to_string(),
        "Normal drilling operations — periodic summary.".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DrillingMetrics, DrillingPhysicsReport, Operation, RigState, TicketSeverity, TicketType};

    fn make_ticket(category: AnomalyCategory) -> AdvisoryTicket {
        AdvisoryTicket {
            timestamp: 1000,
            ticket_type: TicketType::Optimization,
            category,
            severity: TicketSeverity::Medium,
            current_metrics: DrillingMetrics {
                state: RigState::Drilling,
                operation: Operation::ProductionDrilling,
                flow_balance: 15.0,
                pit_rate: 8.0,
                ecd_margin: 0.4,
                spp_delta: 120.0,
                torque_delta_percent: 0.2,
                mse_efficiency: 60.0,
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

    fn make_physics() -> DrillingPhysicsReport {
        DrillingPhysicsReport {
            avg_mse: 30000.0,
            optimal_mse: 20000.0,
            mse_efficiency: 60.0,
            mse_trend: 0.05,
            dxc_trend: -0.15,
            flow_balance_trend: 2.0,
            formation_hardness: 6.0,
            current_depth: 10000.0,
            current_rop: 45.0,
            current_wob: 30.0,
            current_rpm: 120.0,
            current_torque: 18.5,
            current_spp: 2800.0,
            current_flow_in: 500.0,
            current_flow_out: 515.0,
            current_mud_weight: 12.0,
            current_ecd: 12.4,
            current_gas: 80.0,
            current_pit_volume: 500.0,
            founder_detected: false,
            founder_severity: 0.0,
            optimal_wob_estimate: 25.0,
            ..DrillingPhysicsReport::default()
        }
    }

    #[test]
    fn test_all_categories_produce_output() {
        let physics = make_physics();
        for cat in [
            AnomalyCategory::WellControl,
            AnomalyCategory::DrillingEfficiency,
            AnomalyCategory::Hydraulics,
            AnomalyCategory::Mechanical,
            AnomalyCategory::Formation,
            AnomalyCategory::None,
        ] {
            let ticket = make_ticket(cat.clone());
            let result = template_advisory(&ticket, &physics, Campaign::Production);
            assert!(!result.recommendation.is_empty(), "Empty recommendation for {:?}", cat);
            assert!(!result.reasoning.is_empty(), "Empty reasoning for {:?}", cat);
            assert_eq!(result.confidence, 0.70);
            assert_eq!(result.source, "template");
        }
    }

    #[test]
    fn test_well_control_includes_metrics() {
        let ticket = make_ticket(AnomalyCategory::WellControl);
        let physics = make_physics();
        let result = template_advisory(&ticket, &physics, Campaign::Production);
        assert!(result.recommendation.contains("15.0"), "Should include flow balance value");
        assert!(result.recommendation.contains("WELL CONTROL"));
    }

    #[test]
    fn test_pa_campaign_note() {
        let ticket = make_ticket(AnomalyCategory::WellControl);
        let physics = make_physics();
        let result = template_advisory(&ticket, &physics, Campaign::PlugAbandonment);
        assert!(result.recommendation.contains("P&A"), "Should include P&A campaign note");
    }

    #[test]
    fn test_founder_template() {
        let ticket = make_ticket(AnomalyCategory::Mechanical);
        let mut physics = make_physics();
        physics.founder_detected = true;
        physics.optimal_wob_estimate = 25.0;
        let result = template_advisory(&ticket, &physics, Campaign::Production);
        assert!(result.recommendation.contains("FOUNDER"), "Should detect founder condition");
    }
}
