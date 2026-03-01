//! Template-based conversion from OptimizationAdvisory → StrategicAdvisory

use crate::types::{
    AnomalyCategory, DrillingPhysicsReport, FinalSeverity, LookAheadAdvisory,
    OptimizationAdvisory, RiskLevel, StrategicAdvisory,
};

/// Convert an `OptimizationAdvisory` into a `StrategicAdvisory` for pipeline output.
///
/// Produces slot-filled text following the architecture doc §7.1 patterns.
pub fn format_optimization_advisory(
    advisory: &OptimizationAdvisory,
    physics: &DrillingPhysicsReport,
) -> StrategicAdvisory {
    let mut recommendation_parts: Vec<String> = Vec::new();

    // Main efficiency context
    recommendation_parts.push(format!(
        "Current ROP {:.1} ft/hr is {:.0}% below offset average in {}.\n\
         MSE efficiency: {:.0}% (optimal MSE: {:.0} psi)",
        physics.current_rop,
        (1.0 - advisory.rop_ratio).max(0.0) * 100.0,
        advisory.formation,
        advisory.mse_efficiency,
        physics.optimal_mse,
    ));

    // Parameter recommendations
    for rec in &advisory.recommendations {
        let delta = (rec.recommended_value - rec.current_value).abs();
        let direction = if rec.recommended_value > rec.current_value {
            "increase"
        } else {
            "decrease"
        };
        recommendation_parts.push(format!(
            "Recommended: {}: {:.1} → {:.1} ({} by {:.1}) — Basis: {}",
            rec.parameter,
            rec.current_value,
            rec.recommended_value,
            direction,
            delta,
            rec.evidence,
        ));
    }

    // Look-ahead text
    if let Some(ref la) = advisory.look_ahead {
        let changes = if la.parameter_changes.is_empty() {
            "no parameter changes needed".to_string()
        } else {
            la.parameter_changes.join("; ")
        };
        let hazards = if la.hazards.is_empty() {
            "none identified".to_string()
        } else {
            la.hazards.join(", ")
        };
        let notes = if la.offset_notes.is_empty() {
            "no notes".to_string()
        } else {
            la.offset_notes.clone()
        };
        recommendation_parts.push(format!(
            "LOOK-AHEAD: In approximately {:.0} minutes you'll enter {}. \
             Recommended: {}. Known hazards: {}. Offset note: {}",
            la.estimated_minutes, la.formation_name, changes, hazards, notes,
        ));
    }

    let recommendation = recommendation_parts.join("\n");

    let expected_benefit = if advisory.rop_ratio < 0.8 {
        format!(
            "Potential ROP improvement of {:.0}% toward offset average in {}",
            (1.0 - advisory.rop_ratio) * 100.0,
            advisory.formation,
        )
    } else {
        "Maintain current performance near offset benchmarks".to_string()
    };

    let reasoning = format!(
        "Optimization engine analysis at {:.0} ft in {} (confidence: {}%). \
         ROP ratio: {:.2}, MSE efficiency: {:.0}%. Source: {}",
        advisory.depth_ft,
        advisory.formation,
        advisory.confidence.percent(),
        advisory.rop_ratio,
        advisory.mse_efficiency,
        advisory.source,
    );

    let efficiency_score = (advisory.mse_efficiency.clamp(0.0, 100.0)) as u8;

    StrategicAdvisory {
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        efficiency_score,
        risk_level: RiskLevel::Low,
        severity: FinalSeverity::Low,
        recommendation,
        expected_benefit,
        reasoning,
        votes: Vec::new(),
        physics_report: physics.clone(),
        context_used: vec![format!("formation_prognosis:{}", advisory.formation)],
        trace_log: Vec::new(),
        category: crate::types::AnomalyCategory::DrillingEfficiency,
        trigger_parameter: String::new(),
        trigger_value: 0.0,
        threshold_value: 0.0,
    }
}

/// Convert a standalone `LookAheadAdvisory` into a `StrategicAdvisory`.
///
/// Used when the lookahead fires independently of the optimizer (coordinator-level).
pub fn format_lookahead_advisory(
    look_ahead: &LookAheadAdvisory,
    current_depth_ft: f64,
    current_rop: f64,
) -> StrategicAdvisory {
    let changes = if look_ahead.parameter_changes.is_empty() {
        "no parameter changes needed".to_string()
    } else {
        look_ahead.parameter_changes.join("; ")
    };
    let hazards = if look_ahead.hazards.is_empty() {
        "none identified".to_string()
    } else {
        look_ahead.hazards.join(", ")
    };
    let notes = if look_ahead.offset_notes.is_empty() {
        "no notes".to_string()
    } else {
        look_ahead.offset_notes.clone()
    };

    let recommendation = format!(
        "FORMATION LOOKAHEAD: Approaching {} in ~{:.0} min ({:.0} ft at {:.0} ft/hr).\n\
         Parameter changes: {}\n\
         Known hazards: {}\n\
         Offset notes: {}",
        look_ahead.formation_name,
        look_ahead.estimated_minutes,
        look_ahead.depth_remaining_ft,
        current_rop,
        changes,
        hazards,
        notes,
    );

    let risk_level = if look_ahead.hazards.is_empty() {
        RiskLevel::Low
    } else {
        RiskLevel::Elevated
    };

    let reasoning = format!(
        "Formation lookahead at {:.0} ft (ROP {:.0} ft/hr). \
         Next formation: {} at {:.0} ft remaining. Source: formation_prognosis",
        current_depth_ft,
        current_rop,
        look_ahead.formation_name,
        look_ahead.depth_remaining_ft,
    );

    StrategicAdvisory {
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        efficiency_score: 100,
        risk_level,
        severity: FinalSeverity::Low,
        recommendation,
        expected_benefit: format!(
            "Proactive parameter adjustment before entering {}",
            look_ahead.formation_name,
        ),
        reasoning,
        votes: Vec::new(),
        physics_report: DrillingPhysicsReport::default(),
        context_used: vec![format!("formation_prognosis:{}", look_ahead.formation_name)],
        trace_log: Vec::new(),
        category: AnomalyCategory::Formation,
        trigger_parameter: "formation_lookahead".to_string(),
        trigger_value: look_ahead.estimated_minutes,
        threshold_value: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    fn make_advisory() -> OptimizationAdvisory {
        OptimizationAdvisory {
            formation: "Utsira".to_string(),
            depth_ft: 3500.0,
            recommendations: vec![ParameterRecommendation {
                parameter: DrillingParameter::Rpm,
                current_value: 80.0,
                recommended_value: 120.0,
                safe_min: 60.0,
                safe_max: 160.0,
                expected_impact: 0.5,
                evidence: "Offset wells averaged 130 RPM".to_string(),
            }],
            confidence: ConfidenceBreakdown {
                offset_wells: 1.0,
                parameter_gap: 0.5,
                trend_consistency: 0.8,
                sensor_quality: 1.0,
                cfc_agreement: 1.0,
            },
            rop_ratio: 0.6,
            mse_efficiency: 65.0,
            look_ahead: None,
            source: "optimization_engine".to_string(),
        }
    }

    #[test]
    fn template_includes_formation_name() {
        let adv = make_advisory();
        let physics = DrillingPhysicsReport {
            current_rop: 48.0,
            optimal_mse: 18000.0,
            ..Default::default()
        };
        let result = format_optimization_advisory(&adv, &physics);
        assert!(result.recommendation.contains("Utsira"));
    }

    #[test]
    fn template_includes_actual_values() {
        let adv = make_advisory();
        let physics = DrillingPhysicsReport {
            current_rop: 48.0,
            optimal_mse: 18000.0,
            ..Default::default()
        };
        let result = format_optimization_advisory(&adv, &physics);
        assert!(result.recommendation.contains("48.0"));
        assert!(result.recommendation.contains("80.0"));
        assert!(result.recommendation.contains("120.0"));
    }

    #[test]
    fn template_with_look_ahead() {
        let mut adv = make_advisory();
        adv.look_ahead = Some(LookAheadAdvisory {
            formation_name: "Balder".to_string(),
            estimated_minutes: 15.0,
            depth_remaining_ft: 30.0,
            parameter_changes: vec!["WOB: 20 → 30 klbs".into()],
            hazards: vec!["Lost circulation risk".into()],
            offset_notes: "Reduce RPM before entering".into(),
            cfc_confidence: None,
        });
        let physics = DrillingPhysicsReport::default();
        let result = format_optimization_advisory(&adv, &physics);
        assert!(result.recommendation.contains("LOOK-AHEAD"));
        assert!(result.recommendation.contains("Balder"));
        assert!(result.recommendation.contains("15 minutes"));
    }

    #[test]
    fn source_is_optimization_engine() {
        let adv = make_advisory();
        let physics = DrillingPhysicsReport::default();
        let result = format_optimization_advisory(&adv, &physics);
        assert_eq!(result.risk_level, RiskLevel::Low);
        assert_eq!(result.severity, FinalSeverity::Low);
        assert!(result.reasoning.contains("optimization_engine"));
    }

    #[test]
    fn lookahead_advisory_has_correct_fields() {
        let la = LookAheadAdvisory {
            formation_name: "Balder".to_string(),
            estimated_minutes: 15.0,
            depth_remaining_ft: 30.0,
            parameter_changes: vec!["WOB: 20 → 30 klbs".into()],
            hazards: vec!["Lost circulation risk".into()],
            offset_notes: "Reduce RPM before entering".into(),
            cfc_confidence: None,
        };
        let result = format_lookahead_advisory(&la, 3950.0, 120.0);
        assert_eq!(result.category, AnomalyCategory::Formation);
        assert_eq!(result.trigger_parameter, "formation_lookahead");
        assert_eq!(result.risk_level, RiskLevel::Elevated);
        assert!(result.recommendation.contains("Balder"));
        assert!(result.recommendation.contains("FORMATION LOOKAHEAD"));
        assert!(result.recommendation.contains("Lost circulation risk"));
    }

    #[test]
    fn lookahead_advisory_low_risk_without_hazards() {
        let la = LookAheadAdvisory {
            formation_name: "Utsira".to_string(),
            estimated_minutes: 20.0,
            depth_remaining_ft: 40.0,
            parameter_changes: vec![],
            hazards: vec![],
            offset_notes: String::new(),
            cfc_confidence: None,
        };
        let result = format_lookahead_advisory(&la, 3960.0, 120.0);
        assert_eq!(result.risk_level, RiskLevel::Low);
        assert_eq!(result.efficiency_score, 100);
    }
}
