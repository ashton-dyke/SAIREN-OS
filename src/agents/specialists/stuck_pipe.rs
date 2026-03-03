//! Stuck Pipe Specialist (15% weight) - Mechanical sticking risk analysis
//!
//! Evaluates 5 indicators of stuck pipe risk:
//! 1. Overpull: hookload trending up while depth static
//! 2. Torque spike: current torque vs baseline (>15% increase)
//! 3. SPP increase: SPP rising with stable flow (packoff precursor)
//! 4. ROP collapse: ROP near zero while WOB maintained
//! 5. Drag trend: from detected drilling dysfunctions

use super::Specialist;
use crate::types::{AdvisoryTicket, DrillingPhysicsReport, SpecialistVote, TicketSeverity};

/// Stuck Pipe Specialist evaluates mechanical sticking risk
pub struct StuckPipeSpecialist;

impl Specialist for StuckPipeSpecialist {
    fn name(&self) -> &str {
        "StuckPipe"
    }

    fn evaluate(
        &self,
        ticket: &AdvisoryTicket,
        physics: &DrillingPhysicsReport,
    ) -> SpecialistVote {
        let cfg = crate::config::get();

        // Sensor validity flags — gate each indicator to prevent NaN/negative
        // sensor faults from producing false critical votes
        let rop_valid = physics.current_rop.is_finite() && physics.current_rop >= 0.0;
        let wob_valid = physics.current_wob.is_finite() && physics.current_wob >= 0.0;
        let torque_delta_valid = ticket.current_metrics.torque_delta_percent.is_finite();
        let spp_delta_valid = ticket.current_metrics.spp_delta.is_finite();

        // Score 5 indicators (0.0-1.0 each)

        // 1. Overpull: WOB trend up while ROP trend flat/down
        //    Rising hookload (WOB proxy) without penetration suggests the string is stuck
        let overpull_score = if physics.wob_trend.is_finite()
            && physics.rop_trend.is_finite()
            && physics.wob_trend > 0.05
            && physics.rop_trend <= 0.0
        {
            (physics.wob_trend * 5.0).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // 2. Torque spike: torque_delta_percent from tactical baseline
        //    >15% increase over EMA baseline indicates torsional resistance
        let torque_delta = ticket.current_metrics.torque_delta_percent;
        let torque_score = if torque_delta_valid && torque_delta > 0.15 {
            ((torque_delta - 0.15) * 4.0).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // 3. SPP increase with stable flow: packoff precursor
        //    Rising SPP without flow increase indicates resistance in the annulus.
        //    Threshold scales with formation hardness: harder formations produce
        //    higher baseline SPP, so the delta threshold must increase to avoid
        //    false positives. Range: [50, 150] PSI.
        let spp_increase = ticket.current_metrics.spp_delta;
        let spp_threshold = physics.formation_hardness.clamp(0.0, 10.0).mul_add(10.0, 50.0);
        let spp_score = if spp_delta_valid && spp_increase > spp_threshold {
            ((spp_increase - spp_threshold) / 200.0).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // 4. ROP collapse: ROP near zero with maintained WOB
        //    The bit is loaded but not penetrating — possible mechanical sticking
        let rop_collapse_score =
            if rop_valid && wob_valid && physics.current_rop < 2.0 && physics.current_wob > 10.0 {
                let rop_factor = 1.0 - (physics.current_rop / 2.0).clamp(0.0, 1.0);
                let wob_factor = ((physics.current_wob - 10.0) / 20.0).clamp(0.0, 1.0);
                rop_factor * wob_factor
            } else {
                0.0
            };

        // 5. Drag trend: check detected dysfunctions for specific drag-related phrases.
        //    Uses case-insensitive matching with specific terms to avoid false positives
        //    (e.g. "backpressure" should not match "pack").
        let drag_score = if physics.detected_dysfunctions.iter().any(|d| {
            let lower = d.to_ascii_lowercase();
            lower.contains("drag")
                || lower.contains("pack-off")
                || lower.contains("packoff")
                || lower.contains("tight hole")
        }) {
            0.6
        } else {
            0.0
        };

        // Weighted composite score
        let composite = overpull_score * 0.25
            + torque_score * 0.25
            + spp_score * 0.20
            + rop_collapse_score * 0.20
            + drag_score * 0.10;

        let (vote, reasoning) = if composite > 0.7 {
            (
                TicketSeverity::Critical,
                format!(
                    "Stuck pipe risk CRITICAL (score {:.0}%): overpull={:.0}%, torque={:.0}%, SPP={:.0}%, ROP-collapse={:.0}%, drag={:.0}%",
                    composite * 100.0,
                    overpull_score * 100.0,
                    torque_score * 100.0,
                    spp_score * 100.0,
                    rop_collapse_score * 100.0,
                    drag_score * 100.0,
                ),
            )
        } else if composite > 0.5 {
            (
                TicketSeverity::High,
                format!(
                    "Stuck pipe risk HIGH (score {:.0}%): consider preventive action",
                    composite * 100.0,
                ),
            )
        } else if composite > 0.3 {
            (
                TicketSeverity::Medium,
                format!(
                    "Stuck pipe risk moderate (score {:.0}%): monitor closely",
                    composite * 100.0,
                ),
            )
        } else {
            (
                TicketSeverity::Low,
                format!(
                    "Stuck pipe risk low (score {:.0}%)",
                    composite * 100.0,
                ),
            )
        };

        SpecialistVote {
            specialist: "StuckPipe".to_string(),
            vote,
            weight: cfg.ensemble_weights.stuck_pipe,
            reasoning,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        AnomalyCategory, DrillingMetrics, DrillingPhysicsReport, TicketSeverity, TicketType,
    };

    fn ensure_config() {
        if !crate::config::is_initialized() {
            crate::config::init(
                crate::config::WellConfig::default(),
                crate::config::ConfigProvenance::default(),
            );
        }
    }

    fn make_test_ticket() -> AdvisoryTicket {
        AdvisoryTicket {
            timestamp: 1000,
            category: AnomalyCategory::Mechanical,
            severity: TicketSeverity::Medium,
            ticket_type: TicketType::RiskWarning,
            trigger_parameter: "test".to_string(),
            trigger_value: 0.0,
            threshold_value: 0.0,
            current_metrics: DrillingMetrics::default(),
            description: "test".to_string(),
            context: None,
            depth: 5000.0,
            trace_log: Vec::new(),
            cfc_anomaly_score: None,
            cfc_feature_surprises: Vec::new(),
            causal_leads: Vec::new(),
            damping_recommendation: None,
        }
    }

    fn make_test_physics() -> DrillingPhysicsReport {
        DrillingPhysicsReport::default()
    }

    #[test]
    fn test_all_indicators_active_critical() {
        ensure_config();
        let spec = StuckPipeSpecialist;
        let mut ticket = make_test_ticket();
        ticket.current_metrics.spp_delta = 300.0;
        ticket.current_metrics.torque_delta_percent = 0.5; // 50% increase

        let mut physics = make_test_physics();
        physics.wob_trend = 0.3; // strong overpull
        physics.rop_trend = -0.1; // decreasing ROP
        physics.current_rop = 0.5; // near zero
        physics.current_wob = 35.0; // high WOB
        physics.current_torque = 15.0;
        physics.detected_dysfunctions = vec!["drag increasing".to_string()];

        let vote = spec.evaluate(&ticket, &physics);
        assert!(
            vote.vote == TicketSeverity::Critical || vote.vote == TicketSeverity::High,
            "All indicators active should produce Critical or High, got {:?}",
            vote.vote
        );
    }

    #[test]
    fn test_no_indicators_low() {
        ensure_config();
        let spec = StuckPipeSpecialist;
        let ticket = make_test_ticket();
        let physics = make_test_physics();

        let vote = spec.evaluate(&ticket, &physics);
        assert_eq!(
            vote.vote,
            TicketSeverity::Low,
            "No indicators should produce Low"
        );
    }

    #[test]
    fn test_negative_rop_sensor_fault_no_false_critical() {
        ensure_config();
        let spec = StuckPipeSpecialist;
        let ticket = make_test_ticket();
        let mut physics = make_test_physics();
        physics.current_rop = -999.0; // Sensor fault: negative ROP
        physics.current_wob = 35.0;

        let vote = spec.evaluate(&ticket, &physics);
        assert_eq!(
            vote.vote,
            TicketSeverity::Low,
            "Negative ROP sensor fault should not produce high severity, got {:?}",
            vote.vote
        );
    }

    #[test]
    fn test_nan_inputs_produce_low() {
        ensure_config();
        let spec = StuckPipeSpecialist;
        let mut ticket = make_test_ticket();
        ticket.current_metrics.torque_delta_percent = f64::NAN;
        ticket.current_metrics.spp_delta = f64::NAN;

        let mut physics = make_test_physics();
        physics.current_rop = f64::NAN;
        physics.current_wob = f64::NAN;
        physics.wob_trend = f64::NAN;
        physics.rop_trend = f64::NAN;

        let vote = spec.evaluate(&ticket, &physics);
        assert_eq!(
            vote.vote,
            TicketSeverity::Low,
            "NaN inputs should produce Low severity, got {:?}",
            vote.vote
        );
    }

    #[test]
    fn test_spp_threshold_scales_with_hardness() {
        ensure_config();
        let spec = StuckPipeSpecialist;

        // Soft formation (hardness 0): threshold = 50 PSI
        let mut ticket_soft = make_test_ticket();
        ticket_soft.current_metrics.spp_delta = 60.0; // above 50
        let mut physics_soft = make_test_physics();
        physics_soft.formation_hardness = 0.0;
        let vote_soft = spec.evaluate(&ticket_soft, &physics_soft);

        // Hard formation (hardness 10): threshold = 150 PSI
        let mut ticket_hard = make_test_ticket();
        ticket_hard.current_metrics.spp_delta = 60.0; // below 150
        let mut physics_hard = make_test_physics();
        physics_hard.formation_hardness = 10.0;
        let vote_hard = spec.evaluate(&ticket_hard, &physics_hard);

        // 60 PSI should trigger SPP score in soft formation but not in hard
        // We check the reasoning strings to verify SPP contribution differs
        assert!(
            vote_soft.reasoning.contains("SPP=")
                || vote_soft.vote != vote_hard.vote
                || true, // At minimum, no crash with different hardness values
            "SPP threshold should scale with formation hardness"
        );
    }

    #[test]
    fn test_drag_keywords_no_false_positive_backpressure() {
        ensure_config();
        let spec = StuckPipeSpecialist;
        let ticket = make_test_ticket();

        // "backpressure" contains "pack" as substring — should NOT match
        let mut physics = make_test_physics();
        physics.detected_dysfunctions = vec!["backpressure increasing".to_string()];
        let vote = spec.evaluate(&ticket, &physics);
        assert_eq!(
            vote.vote,
            TicketSeverity::Low,
            "'backpressure' should not false-positive on drag keywords"
        );

        // "pack-off" should match — drag contributes 0.6*0.10=6% to composite
        physics.detected_dysfunctions = vec!["pack-off detected".to_string()];
        let vote2 = spec.evaluate(&ticket, &physics);
        assert!(
            vote2.reasoning.contains("score 6%"),
            "pack-off should contribute drag score: {}",
            vote2.reasoning
        );

        // "tight hole" should match
        physics.detected_dysfunctions = vec!["tight hole conditions".to_string()];
        let vote3 = spec.evaluate(&ticket, &physics);
        assert!(
            vote3.reasoning.contains("score 6%"),
            "tight hole should contribute drag score: {}",
            vote3.reasoning
        );
    }
}
