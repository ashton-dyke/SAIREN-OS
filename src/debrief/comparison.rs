//! Planned vs actual formation comparison

use crate::types::{
    AnomalyCategory, FormationComparison, FormationPrognosis, PostWellSummary, TimelineEvent,
};

/// Compare planned formation parameters with actual post-well performance.
///
/// For each formation in the post-well summary, looks up planned parameters
/// from the prognosis, computes ROP deltas, counts advisories in the depth
/// range, and assigns an assessment.
pub fn compare_formations(
    prognosis: Option<&FormationPrognosis>,
    post_well: &PostWellSummary,
    timeline: &[TimelineEvent],
) -> Vec<FormationComparison> {
    post_well
        .formations
        .iter()
        .map(|perf| {
            // Find matching prognosis formation by name
            let planned = prognosis.and_then(|prog| {
                prog.formations
                    .iter()
                    .find(|f| f.name == perf.formation_name)
            });

            let planned_wob = planned.map(|f| f.parameters.wob_klbs.optimal);
            let planned_rpm = planned.map(|f| f.parameters.rpm.optimal);
            let planned_rop = planned.map(|f| f.offset_performance.avg_rop_ft_hr);

            let rop_delta_pct = planned_rop.and_then(|p| {
                if p > 0.0 {
                    Some((perf.avg_rop_ft_hr - p) / p * 100.0)
                } else {
                    None
                }
            });

            // Count advisories in this formation's depth range
            let events_in_range: Vec<&TimelineEvent> = timeline
                .iter()
                .filter(|e| e.depth_ft >= perf.depth_top_ft && e.depth_ft < perf.depth_base_ft)
                .collect();

            let advisory_count = events_in_range.len();
            let critical_count = events_in_range
                .iter()
                .filter(|e| e.severity == "CRITICAL")
                .count();
            let mechanical_count = events_in_range
                .iter()
                .filter(|e| e.category == AnomalyCategory::Mechanical)
                .count();

            let assessment = match rop_delta_pct {
                Some(delta) if delta > 10.0 => "exceeded_plan".to_string(),
                Some(delta) if delta < -10.0 => "below_plan".to_string(),
                Some(_) => "met_plan".to_string(),
                None => "no_plan_data".to_string(),
            };

            FormationComparison {
                formation_name: perf.formation_name.clone(),
                depth_top_ft: perf.depth_top_ft,
                depth_base_ft: perf.depth_base_ft,
                planned_wob_klbs: planned_wob,
                planned_rpm,
                planned_rop_ft_hr: planned_rop,
                actual_avg_rop_ft_hr: perf.avg_rop_ft_hr,
                actual_best_rop_ft_hr: perf.best_rop_ft_hr,
                actual_avg_mse_psi: perf.avg_mse_psi,
                actual_best_wob_klbs: perf.best_params.wob_klbs,
                actual_best_rpm: perf.best_params.rpm,
                rop_delta_pct,
                advisory_count,
                critical_count,
                mechanical_count,
                assessment,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        BestParams, FormationInterval, FormationParameters, FormationPrognosis,
        OffsetPerformance, ParameterRange, PostWellFormationPerformance, PostWellSummary,
        PrognosisWellInfo, TimelineEvent,
    };

    fn make_post_well(formations: Vec<PostWellFormationPerformance>) -> PostWellSummary {
        PostWellSummary {
            well_id: "Well-A".to_string(),
            field: "TestField".to_string(),
            completion_date: "2025-01-01".to_string(),
            total_depth_ft: 6000.0,
            total_bit_hours: 100.0,
            formations,
        }
    }

    fn make_perf(name: &str, avg_rop: f64, top: f64, base: f64) -> PostWellFormationPerformance {
        PostWellFormationPerformance {
            well_id: "Well-A".to_string(),
            field: "TestField".to_string(),
            formation_name: name.to_string(),
            depth_top_ft: top,
            depth_base_ft: base,
            avg_rop_ft_hr: avg_rop,
            best_rop_ft_hr: avg_rop * 1.2,
            avg_mse_psi: 15000.0,
            best_params: BestParams { wob_klbs: 25.0, rpm: 120.0 },
            avg_wob_range: ParameterRange { min: 15.0, optimal: 25.0, max: 35.0 },
            avg_rpm_range: ParameterRange { min: 80.0, optimal: 120.0, max: 160.0 },
            avg_flow_range: ParameterRange { min: 400.0, optimal: 500.0, max: 600.0 },
            total_snapshots: 100,
            avg_confidence: 0.8,
            avg_stability: 0.9,
            notes: String::new(),
            completed_timestamp: 1700000000,
            sustained_only: None,
        }
    }

    fn make_prognosis_formation(name: &str, avg_rop: f64, top: f64, base: f64) -> FormationInterval {
        FormationInterval {
            name: name.to_string(),
            depth_top_ft: top,
            depth_base_ft: base,
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
                avg_rop_ft_hr: avg_rop,
                best_rop_ft_hr: avg_rop * 1.5,
                avg_mse_psi: 12000.0,
                best_params: BestParams { wob_klbs: 22.0, rpm: 130.0 },
                notes: String::new(),
            },
        }
    }

    fn make_prognosis(formations: Vec<FormationInterval>) -> FormationPrognosis {
        FormationPrognosis {
            well: PrognosisWellInfo {
                name: "Well-A".to_string(),
                field: "TestField".to_string(),
                spud_date: String::new(),
                target_depth_ft: 10000.0,
                coordinate_system: String::new(),
            },
            formations,
            casings: Vec::new(),
        }
    }

    #[test]
    fn test_formation_comparison_exceeded() {
        // Planned ROP 60, actual 80 → +33% → exceeded_plan
        let post_well = make_post_well(vec![make_perf("Shallow", 80.0, 0.0, 3000.0)]);
        let prognosis = make_prognosis(vec![
            make_prognosis_formation("Shallow", 60.0, 0.0, 3000.0),
        ]);

        let comparisons = compare_formations(Some(&prognosis), &post_well, &[]);

        assert_eq!(comparisons.len(), 1);
        assert_eq!(comparisons[0].assessment, "exceeded_plan");
        let delta = comparisons[0].rop_delta_pct.unwrap();
        assert!((delta - 33.33).abs() < 1.0);
    }

    #[test]
    fn test_formation_comparison_below() {
        // Planned ROP 100, actual 60 → -40% → below_plan
        let post_well = make_post_well(vec![make_perf("Deep", 60.0, 3000.0, 6000.0)]);
        let prognosis = make_prognosis(vec![
            make_prognosis_formation("Deep", 100.0, 3000.0, 6000.0),
        ]);

        let comparisons = compare_formations(Some(&prognosis), &post_well, &[]);

        assert_eq!(comparisons[0].assessment, "below_plan");
    }

    #[test]
    fn test_formation_comparison_no_prognosis() {
        let post_well = make_post_well(vec![make_perf("Shallow", 80.0, 0.0, 3000.0)]);

        let comparisons = compare_formations(None, &post_well, &[]);

        assert_eq!(comparisons.len(), 1);
        assert_eq!(comparisons[0].assessment, "no_plan_data");
        assert!(comparisons[0].planned_rop_ft_hr.is_none());
    }

    #[test]
    fn test_advisory_counts_in_formation() {
        let post_well = make_post_well(vec![
            make_perf("Shallow", 80.0, 0.0, 3000.0),
            make_perf("Deep", 60.0, 3000.0, 6000.0),
        ]);

        let timeline = vec![
            TimelineEvent {
                timestamp: 1000,
                depth_ft: 1500.0,
                formation_name: Some("Shallow".to_string()),
                category: AnomalyCategory::DrillingEfficiency,
                severity: "MEDIUM".to_string(),
                recommendation: "test".to_string(),
                feedback: None,
            },
            TimelineEvent {
                timestamp: 2000,
                depth_ft: 2500.0,
                formation_name: Some("Shallow".to_string()),
                category: AnomalyCategory::Mechanical,
                severity: "CRITICAL".to_string(),
                recommendation: "test".to_string(),
                feedback: None,
            },
            TimelineEvent {
                timestamp: 3000,
                depth_ft: 4000.0,
                formation_name: Some("Deep".to_string()),
                category: AnomalyCategory::WellControl,
                severity: "HIGH".to_string(),
                recommendation: "test".to_string(),
                feedback: None,
            },
        ];

        let comparisons = compare_formations(None, &post_well, &timeline);

        assert_eq!(comparisons[0].advisory_count, 2);
        assert_eq!(comparisons[0].critical_count, 1);
        assert_eq!(comparisons[0].mechanical_count, 1);
        assert_eq!(comparisons[1].advisory_count, 1);
        assert_eq!(comparisons[1].critical_count, 0);
    }
}
