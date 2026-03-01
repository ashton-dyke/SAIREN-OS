//! Post-well AI debrief: structured narrative correlating advisory history
//! with formation transitions and planned vs actual performance.

pub mod comparison;
pub mod narrative;
pub mod timeline;

use crate::storage::feedback::{FeedbackOutcome, FeedbackRecord};
use crate::storage::suggestions;
use crate::types::{
    CategoryFeedbackRate, FeedbackSummary, FormationPrognosis, PostWellSummary,
    StrategicAdvisory, WellDebrief,
};

/// Generate a complete well debrief from post-well data, advisories, and feedback.
pub fn generate_debrief(
    post_well: &PostWellSummary,
    advisories: &[StrategicAdvisory],
    feedback_records: &[FeedbackRecord],
    prognosis: Option<&FormationPrognosis>,
    well_start_ts: u64,
) -> WellDebrief {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // 1. Build timeline
    let timeline = timeline::build_timeline(advisories, feedback_records, prognosis);

    // 2. Compare formations
    let formation_comparisons =
        comparison::compare_formations(prognosis, post_well, &timeline);

    // 3. Build feedback summary
    let feedback_summary = build_feedback_summary(advisories.len(), feedback_records);

    // 4. Generate narrative
    let narrative = narrative::generate_narrative(
        &post_well.well_id,
        post_well.total_depth_ft,
        post_well.total_bit_hours,
        &formation_comparisons,
        &feedback_summary,
        &timeline,
    );

    WellDebrief {
        well_id: post_well.well_id.clone(),
        field: post_well.field.clone(),
        generated_at: now,
        well_start_ts,
        well_end_ts: now,
        total_depth_ft: post_well.total_depth_ft,
        total_bit_hours: post_well.total_bit_hours,
        timeline,
        formation_comparisons,
        feedback_summary,
        narrative,
    }
}

/// Build a feedback summary from advisory count and feedback records.
///
/// Reuses `storage::suggestions::compute_stats()` for per-category rates.
fn build_feedback_summary(
    total_advisories: usize,
    feedback_records: &[FeedbackRecord],
) -> FeedbackSummary {
    let stats = suggestions::compute_stats(feedback_records);

    let confirmed: usize = feedback_records
        .iter()
        .filter(|r| r.outcome == FeedbackOutcome::Confirmed)
        .count();
    let false_positives: usize = feedback_records
        .iter()
        .filter(|r| r.outcome == FeedbackOutcome::FalsePositive)
        .count();
    let unclear: usize = feedback_records
        .iter()
        .filter(|r| r.outcome == FeedbackOutcome::Unclear)
        .count();

    let denominator = confirmed + false_positives;
    let confirmation_rate = if denominator > 0 {
        confirmed as f64 / denominator as f64
    } else {
        0.0
    };

    let category_rates = stats
        .into_iter()
        .map(|s| CategoryFeedbackRate {
            category: s.category,
            total: s.total,
            confirmed: s.confirmed,
            false_positives: s.false_positives,
            confirmation_rate: s.confirmation_rate,
        })
        .collect();

    FeedbackSummary {
        total_advisories,
        total_feedback: feedback_records.len(),
        confirmed,
        false_positives,
        unclear,
        confirmation_rate,
        category_rates,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        AnomalyCategory, BestParams, DrillingPhysicsReport, FinalSeverity,
        FormationInterval, FormationParameters, FormationPrognosis, OffsetPerformance,
        ParameterRange, PostWellFormationPerformance, PostWellSummary, PrognosisWellInfo,
        StrategicAdvisory,
    };

    fn make_advisory(ts: u64, depth: f64, category: AnomalyCategory) -> StrategicAdvisory {
        StrategicAdvisory {
            timestamp: ts,
            category,
            severity: FinalSeverity::Medium,
            recommendation: format!("Advisory at depth {:.0}", depth),
            physics_report: DrillingPhysicsReport {
                current_depth: depth,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn make_post_well() -> PostWellSummary {
        PostWellSummary {
            well_id: "Well-A".to_string(),
            field: "TestField".to_string(),
            completion_date: "2025-01-01".to_string(),
            total_depth_ft: 6000.0,
            total_bit_hours: 100.0,
            formations: vec![PostWellFormationPerformance {
                well_id: "Well-A".to_string(),
                field: "TestField".to_string(),
                formation_name: "Shallow".to_string(),
                depth_top_ft: 0.0,
                depth_base_ft: 3000.0,
                avg_rop_ft_hr: 90.0,
                best_rop_ft_hr: 120.0,
                avg_mse_psi: 14000.0,
                best_params: BestParams { wob_klbs: 24.0, rpm: 125.0 },
                avg_wob_range: ParameterRange { min: 15.0, optimal: 24.0, max: 30.0 },
                avg_rpm_range: ParameterRange { min: 90.0, optimal: 125.0, max: 150.0 },
                avg_flow_range: ParameterRange { min: 400.0, optimal: 500.0, max: 600.0 },
                total_snapshots: 100,
                avg_confidence: 0.8,
                avg_stability: 0.9,
                notes: String::new(),
                completed_timestamp: 1700000000,
                sustained_only: None,
            }],
        }
    }

    fn make_prognosis() -> FormationPrognosis {
        FormationPrognosis {
            well: PrognosisWellInfo {
                name: "Well-A".to_string(),
                field: "TestField".to_string(),
                spud_date: String::new(),
                target_depth_ft: 10000.0,
                coordinate_system: String::new(),
            },
            formations: vec![FormationInterval {
                name: "Shallow".to_string(),
                depth_top_ft: 0.0,
                depth_base_ft: 3000.0,
                lithology: "Shale".to_string(),
                hardness: 3.0,
                drillability: "Easy".to_string(),
                pore_pressure_ppg: 9.0,
                fracture_gradient_ppg: 14.0,
                hazards: Vec::new(),
                parameters: FormationParameters {
                    wob_klbs: ParameterRange { min: 10.0, optimal: 20.0, max: 30.0 },
                    rpm: ParameterRange { min: 80.0, optimal: 120.0, max: 160.0 },
                    flow_gpm: ParameterRange { min: 400.0, optimal: 500.0, max: 600.0 },
                    mud_weight_ppg: 9.5,
                    bit_type: "PDC".to_string(),
                },
                offset_performance: OffsetPerformance {
                    wells: vec!["Offset-1".to_string()],
                    avg_rop_ft_hr: 70.0,
                    best_rop_ft_hr: 100.0,
                    avg_mse_psi: 12000.0,
                    best_params: BestParams { wob_klbs: 22.0, rpm: 130.0 },
                    notes: String::new(),
                },
            }],
            casings: Vec::new(),
        }
    }

    #[test]
    fn test_feedback_summary_computation() {
        let records = vec![
            FeedbackRecord {
                advisory_timestamp: 1000,
                outcome: FeedbackOutcome::Confirmed,
                category: AnomalyCategory::DrillingEfficiency,
                trigger_parameter: "mse".to_string(),
                trigger_value: 30000.0,
                threshold_value: 25000.0,
                submitted_by: "driller".to_string(),
                submitted_at: 1100,
                notes: String::new(),
            },
            FeedbackRecord {
                advisory_timestamp: 2000,
                outcome: FeedbackOutcome::FalsePositive,
                category: AnomalyCategory::DrillingEfficiency,
                trigger_parameter: "mse".to_string(),
                trigger_value: 26000.0,
                threshold_value: 25000.0,
                submitted_by: "driller".to_string(),
                submitted_at: 2100,
                notes: String::new(),
            },
            FeedbackRecord {
                advisory_timestamp: 3000,
                outcome: FeedbackOutcome::Confirmed,
                category: AnomalyCategory::WellControl,
                trigger_parameter: "flow".to_string(),
                trigger_value: 50.0,
                threshold_value: 30.0,
                submitted_by: "driller".to_string(),
                submitted_at: 3100,
                notes: String::new(),
            },
            FeedbackRecord {
                advisory_timestamp: 4000,
                outcome: FeedbackOutcome::Unclear,
                category: AnomalyCategory::Mechanical,
                trigger_parameter: "torque".to_string(),
                trigger_value: 10.0,
                threshold_value: 8.0,
                submitted_by: "driller".to_string(),
                submitted_at: 4100,
                notes: String::new(),
            },
        ];

        let summary = build_feedback_summary(10, &records);

        assert_eq!(summary.total_advisories, 10);
        assert_eq!(summary.total_feedback, 4);
        assert_eq!(summary.confirmed, 2);
        assert_eq!(summary.false_positives, 1);
        assert_eq!(summary.unclear, 1);
        // confirmation_rate = 2 / (2 + 1) = 0.6667
        assert!((summary.confirmation_rate - 0.6667).abs() < 0.01);
        assert!(!summary.category_rates.is_empty());
    }

    #[test]
    fn test_debrief_serde_roundtrip() {
        let post_well = make_post_well();
        let advisories = vec![
            make_advisory(1000, 1500.0, AnomalyCategory::DrillingEfficiency),
        ];
        let prognosis = make_prognosis();

        let debrief = generate_debrief(&post_well, &advisories, &[], Some(&prognosis), 1000);

        let json = serde_json::to_string_pretty(&debrief).unwrap();
        let decoded: WellDebrief = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.well_id, "Well-A");
        assert_eq!(decoded.field, "TestField");
        assert_eq!(decoded.timeline.len(), 1);
        assert_eq!(decoded.formation_comparisons.len(), 1);
        assert!(!decoded.narrative.is_empty());
    }

    #[test]
    fn test_debrief_orchestrator() {
        let post_well = make_post_well();
        let advisories = vec![
            make_advisory(1000, 1500.0, AnomalyCategory::DrillingEfficiency),
            make_advisory(2000, 2500.0, AnomalyCategory::Mechanical),
        ];
        let feedback = vec![FeedbackRecord {
            advisory_timestamp: 1000,
            outcome: FeedbackOutcome::Confirmed,
            category: AnomalyCategory::DrillingEfficiency,
            trigger_parameter: "mse".to_string(),
            trigger_value: 30000.0,
            threshold_value: 25000.0,
            submitted_by: "driller".to_string(),
            submitted_at: 1100,
            notes: String::new(),
        }];
        let prognosis = make_prognosis();

        let debrief = generate_debrief(&post_well, &advisories, &feedback, Some(&prognosis), 1000);

        // Verify all components assembled
        assert_eq!(debrief.well_id, "Well-A");
        assert_eq!(debrief.total_depth_ft, 6000.0);
        assert_eq!(debrief.timeline.len(), 2);
        assert_eq!(debrief.timeline[0].feedback.as_deref(), Some("confirmed"));
        assert!(debrief.timeline[1].feedback.is_none());
        assert_eq!(debrief.formation_comparisons.len(), 1);
        assert_eq!(debrief.formation_comparisons[0].assessment, "exceeded_plan");
        assert_eq!(debrief.feedback_summary.total_feedback, 1);
        assert!(debrief.narrative.contains("## Summary"));
        assert!(debrief.narrative.contains("EXCEEDED PLAN"));
    }
}
