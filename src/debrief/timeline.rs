//! Timeline assembly: correlate advisories with formations by depth

use crate::storage::feedback::{FeedbackOutcome, FeedbackRecord};
use crate::types::{FormationPrognosis, StrategicAdvisory, TimelineEvent};

/// Build a chronological timeline of advisories correlated with formations.
///
/// For each advisory, extracts depth from `physics_report.current_depth`,
/// matches it to a formation via prognosis, and links operator feedback.
pub fn build_timeline(
    advisories: &[StrategicAdvisory],
    feedback_records: &[FeedbackRecord],
    prognosis: Option<&FormationPrognosis>,
) -> Vec<TimelineEvent> {
    let mut events: Vec<TimelineEvent> = advisories
        .iter()
        .map(|adv| {
            let depth = adv.physics_report.current_depth;

            let formation_name = prognosis.and_then(|prog| {
                prog.formation_at_depth(depth).map(|f| f.name.clone())
            });

            let feedback = feedback_records
                .iter()
                .find(|fr| fr.advisory_timestamp == adv.timestamp)
                .map(|fr| match fr.outcome {
                    FeedbackOutcome::Confirmed => "confirmed".to_string(),
                    FeedbackOutcome::FalsePositive => "false_positive".to_string(),
                    FeedbackOutcome::Unclear => "unclear".to_string(),
                });

            TimelineEvent {
                timestamp: adv.timestamp,
                depth_ft: depth,
                formation_name,
                category: adv.category,
                severity: format!("{}", adv.severity),
                recommendation: adv.recommendation.clone(),
                feedback,
            }
        })
        .collect();

    events.sort_by_key(|e| e.timestamp);
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        AnomalyCategory, DrillingPhysicsReport, FinalSeverity, FormationInterval,
        FormationParameters, FormationPrognosis, OffsetPerformance, BestParams,
        ParameterRange, PrognosisWellInfo, StrategicAdvisory,
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

    fn make_prognosis() -> FormationPrognosis {
        FormationPrognosis {
            well: PrognosisWellInfo {
                name: "Well-A".to_string(),
                field: "TestField".to_string(),
                spud_date: String::new(),
                target_depth_ft: 10000.0,
                coordinate_system: String::new(),
            },
            formations: vec![
                FormationInterval {
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
                        avg_rop_ft_hr: 80.0,
                        best_rop_ft_hr: 120.0,
                        avg_mse_psi: 12000.0,
                        best_params: BestParams { wob_klbs: 22.0, rpm: 130.0 },
                        notes: String::new(),
                    },
                },
                FormationInterval {
                    name: "Deep".to_string(),
                    depth_top_ft: 3000.0,
                    depth_base_ft: 6000.0,
                    lithology: "Limestone".to_string(),
                    hardness: 6.0,
                    drillability: "Moderate".to_string(),
                    pore_pressure_ppg: 10.0,
                    fracture_gradient_ppg: 15.0,
                    hazards: Vec::new(),
                    parameters: FormationParameters {
                        wob_klbs: ParameterRange { min: 15.0, optimal: 25.0, max: 35.0 },
                        rpm: ParameterRange { min: 60.0, optimal: 100.0, max: 140.0 },
                        flow_gpm: ParameterRange { min: 450.0, optimal: 550.0, max: 650.0 },
                        mud_weight_ppg: 10.5,
                        bit_type: "PDC".to_string(),
                    },
                    offset_performance: OffsetPerformance {
                        wells: vec!["Offset-1".to_string()],
                        avg_rop_ft_hr: 40.0,
                        best_rop_ft_hr: 60.0,
                        avg_mse_psi: 18000.0,
                        best_params: BestParams { wob_klbs: 28.0, rpm: 110.0 },
                        notes: String::new(),
                    },
                },
            ],
            casings: Vec::new(),
        }
    }

    #[test]
    fn test_timeline_event_building() {
        let advisories = vec![
            make_advisory(1000, 1500.0, AnomalyCategory::DrillingEfficiency),
            make_advisory(2000, 4000.0, AnomalyCategory::Mechanical),
        ];
        let prognosis = make_prognosis();

        let timeline = build_timeline(&advisories, &[], Some(&prognosis));

        assert_eq!(timeline.len(), 2);
        assert_eq!(timeline[0].formation_name.as_deref(), Some("Shallow"));
        assert_eq!(timeline[1].formation_name.as_deref(), Some("Deep"));
        assert_eq!(timeline[0].category, AnomalyCategory::DrillingEfficiency);
        assert_eq!(timeline[1].category, AnomalyCategory::Mechanical);
    }

    #[test]
    fn test_timeline_with_feedback() {
        let advisories = vec![
            make_advisory(1000, 1500.0, AnomalyCategory::DrillingEfficiency),
            make_advisory(2000, 4000.0, AnomalyCategory::Mechanical),
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

        let timeline = build_timeline(&advisories, &feedback, Some(&prognosis));

        assert_eq!(timeline[0].feedback.as_deref(), Some("confirmed"));
        assert!(timeline[1].feedback.is_none());
    }
}
