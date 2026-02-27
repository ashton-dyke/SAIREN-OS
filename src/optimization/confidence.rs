//! 5-factor confidence scoring for optimization recommendations

use crate::types::{
    ConfidenceBreakdown, DrillingPhysicsReport, FormationInterval, HistoryEntry,
};

/// Score confidence across 5 weighted factors.
///
/// Weights: offset wells 30%, parameter gap 25%, trend consistency 20%,
/// sensor quality 15%, CfC agreement 10%.
pub fn score_confidence(
    formation: &FormationInterval,
    physics: &DrillingPhysicsReport,
    history: &[HistoryEntry],
    cfc_anomaly_score: Option<f64>,
    sensor_quality: f64,
) -> ConfidenceBreakdown {
    let offset_wells = score_offset_wells(formation);
    let parameter_gap = score_parameter_gap(formation, physics);
    let trend_consistency = score_trend_consistency(history);
    let cfc_agreement = score_cfc_agreement(cfc_anomaly_score);

    ConfidenceBreakdown {
        offset_wells,
        parameter_gap,
        trend_consistency,
        sensor_quality: sensor_quality.clamp(0.0, 1.0),
        cfc_agreement,
    }
}

/// Offset wells: 0 wells=0.0, 1=0.4, 2=0.7, 3+=1.0
fn score_offset_wells(formation: &FormationInterval) -> f64 {
    match formation.offset_performance.wells.len() {
        0 => 0.0,
        1 => 0.4,
        2 => 0.7,
        _ => 1.0,
    }
}

/// Parameter gap: normalized distance from optimal across WOB/RPM/flow, averaged.
/// Larger gaps → higher confidence that there's room to improve.
fn score_parameter_gap(formation: &FormationInterval, physics: &DrillingPhysicsReport) -> f64 {
    let params = &formation.parameters;

    let gaps = [
        normalized_gap(physics.current_wob, &params.wob_klbs),
        normalized_gap(physics.current_rpm, &params.rpm),
        normalized_gap(physics.current_flow_in, &params.flow_gpm),
    ];

    let avg = gaps.iter().sum::<f64>() / gaps.len() as f64;
    avg.clamp(0.0, 1.0)
}

/// Compute normalized distance from optimal within the [min, max] range.
fn normalized_gap(current: f64, range: &crate::types::ParameterRange) -> f64 {
    let span = (range.max - range.min).abs().max(1e-6);
    let distance = (current - range.optimal).abs();
    (distance / span).clamp(0.0, 1.0)
}

/// Trend consistency: check last 10 history entries for sustained underperformance.
/// If MSE efficiency is consistently low, we're more confident a change is needed.
fn score_trend_consistency(history: &[HistoryEntry]) -> f64 {
    let recent: Vec<&HistoryEntry> = history.iter().rev().take(10).collect();
    if recent.len() < 5 {
        return 0.3; // Low confidence with insufficient trend data
    }

    // Count entries with low MSE efficiency (below 70%)
    let low_efficiency_count = recent
        .iter()
        .filter(|e| e.metrics.mse_efficiency < 70.0)
        .count();

    let ratio = low_efficiency_count as f64 / recent.len() as f64;
    // Sustained underperformance → high confidence that optimization is needed
    ratio.clamp(0.0, 1.0)
}

/// CfC agreement: low anomaly score means CfC agrees conditions are stable.
fn score_cfc_agreement(cfc_anomaly_score: Option<f64>) -> f64 {
    match cfc_anomaly_score {
        Some(score) if score < 0.3 => 1.0,
        Some(score) if score < 0.5 => 0.7,
        Some(score) if score < 0.7 => 0.3,
        Some(_) => 0.0, // Should not reach here (gated at 0.7)
        None => 0.5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use std::sync::Arc;

    fn make_formation(num_wells: usize) -> FormationInterval {
        FormationInterval {
            name: "TestFm".to_string(),
            depth_top_ft: 5000.0,
            depth_base_ft: 6000.0,
            lithology: "Sandstone".to_string(),
            hardness: 5.0,
            drillability: "medium".to_string(),
            pore_pressure_ppg: 10.0,
            fracture_gradient_ppg: 14.0,
            hazards: vec![],
            parameters: FormationParameters {
                wob_klbs: ParameterRange { min: 15.0, optimal: 25.0, max: 35.0 },
                rpm: ParameterRange { min: 80.0, optimal: 120.0, max: 160.0 },
                flow_gpm: ParameterRange { min: 400.0, optimal: 500.0, max: 600.0 },
                mud_weight_ppg: 12.0,
                bit_type: "PDC".to_string(),
            },
            offset_performance: OffsetPerformance {
                wells: (0..num_wells).map(|i| format!("W-{}", i + 1)).collect(),
                avg_rop_ft_hr: 80.0,
                best_rop_ft_hr: 100.0,
                avg_mse_psi: 20000.0,
                best_params: BestParams { wob_klbs: 28.0, rpm: 130.0 },
                notes: String::new(),
            },
        }
    }

    fn make_physics(wob: f64, rpm: f64, flow_in: f64) -> DrillingPhysicsReport {
        DrillingPhysicsReport {
            current_wob: wob,
            current_rpm: rpm,
            current_flow_in: flow_in,
            ..Default::default()
        }
    }

    fn make_history(count: usize, mse_eff: f64) -> Vec<HistoryEntry> {
        (0..count)
            .map(|_| HistoryEntry {
                packet: WitsPacket {
                    timestamp: 0,
                    bit_depth: 5500.0,
                    hole_depth: 5500.0,
                    rop: 50.0,
                    hook_load: 200.0,
                    wob: 25.0,
                    rpm: 120.0,
                    torque: 15.0,
                    bit_diameter: 8.5,
                    spp: 2800.0,
                    pump_spm: 120.0,
                    flow_in: 500.0,
                    flow_out: 500.0,
                    pit_volume: 500.0,
                    pit_volume_change: 0.0,
                    mud_weight_in: 12.0,
                    mud_weight_out: 12.0,
                    ecd: 12.4,
                    mud_temp_in: 100.0,
                    mud_temp_out: 120.0,
                    gas_units: 50.0,
                    background_gas: 40.0,
                    connection_gas: 10.0,
                    h2s: 0.0,
                    co2: 0.1,
                    casing_pressure: 0.0,
                    annular_pressure: 0.0,
                    pore_pressure: 10.5,
                    fracture_gradient: 14.0,
                    mse: 30000.0,
                    d_exponent: 1.5,
                    dxc: 1.45,
                    rop_delta: 0.0,
                    torque_delta_percent: 0.0,
                    spp_delta: 0.0,
                    rig_state: RigState::Drilling,
                    regime_id: 0,
                    seconds_since_param_change: 0,                },
                metrics: DrillingMetrics {
                    state: RigState::Drilling,
                    operation: Operation::ProductionDrilling,
                    mse: 30000.0,
                    mse_efficiency: mse_eff,
                    d_exponent: 1.5,
                    dxc: 1.45,
                    mse_delta_percent: 0.0,
                    flow_balance: 0.0,
                    pit_rate: 0.0,
                    ecd_margin: 1.5,
                    torque_delta_percent: 0.0,
                    spp_delta: 0.0,
                    flow_data_available: true,
                    is_anomaly: false,
                    anomaly_category: AnomalyCategory::None,
                    anomaly_description: None,
                    current_formation: None,
                    formation_depth_in_ft: None,
                },
            })
            .collect()
    }

    #[test]
    fn offset_wells_scoring() {
        assert_eq!(score_offset_wells(&make_formation(0)), 0.0);
        assert_eq!(score_offset_wells(&make_formation(1)), 0.4);
        assert_eq!(score_offset_wells(&make_formation(2)), 0.7);
        assert_eq!(score_offset_wells(&make_formation(3)), 1.0);
        assert_eq!(score_offset_wells(&make_formation(5)), 1.0);
    }

    #[test]
    fn parameter_gap_at_optimal_is_zero() {
        let fm = make_formation(3);
        let physics = make_physics(25.0, 120.0, 500.0); // all at optimal
        let gap = score_parameter_gap(&fm, &physics);
        assert!(gap < 0.01, "Gap should be ~0 at optimal, got {gap}");
    }

    #[test]
    fn parameter_gap_away_from_optimal() {
        let fm = make_formation(3);
        let physics = make_physics(15.0, 80.0, 400.0); // all at min
        let gap = score_parameter_gap(&fm, &physics);
        assert!(gap > 0.3, "Gap should be significant at min, got {gap}");
    }

    #[test]
    fn cfc_agreement_scoring() {
        assert_eq!(score_cfc_agreement(Some(0.1)), 1.0);
        assert_eq!(score_cfc_agreement(Some(0.4)), 0.7);
        assert_eq!(score_cfc_agreement(Some(0.6)), 0.3);
        assert_eq!(score_cfc_agreement(None), 0.5);
    }

    #[test]
    fn trend_consistency_with_low_efficiency() {
        let history = make_history(10, 50.0); // all below 70%
        let score = score_trend_consistency(&history);
        assert!(score > 0.9, "All low-efficiency entries should score high: {score}");
    }

    #[test]
    fn trend_consistency_with_high_efficiency() {
        let history = make_history(10, 90.0); // all above 70%
        let score = score_trend_consistency(&history);
        assert!(score < 0.1, "All high-efficiency entries should score low: {score}");
    }

    #[test]
    fn full_confidence_computation() {
        let fm = make_formation(3);
        let physics = make_physics(15.0, 80.0, 400.0);
        let history = make_history(10, 50.0);
        let breakdown = score_confidence(&fm, &physics, &history, Some(0.1), 1.0);
        let pct = breakdown.percent();
        assert!(pct > 50, "Should have reasonable confidence, got {pct}%");
    }
}
