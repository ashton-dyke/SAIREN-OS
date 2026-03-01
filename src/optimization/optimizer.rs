//! Core ParameterOptimizer — proactive drilling parameter recommendation engine

use crate::types::{
    DrillingParameter, DrillingPhysicsReport, FormationInterval, FormationPrognosis,
    HistoryEntry, OptimizationAdvisory, OptimizationSkipReason, ParameterRecommendation,
    RigState, WitsPacket,
};

use super::confidence::score_confidence;
use super::look_ahead::check_look_ahead;
use super::rate_limiter::RateLimiter;

/// Minimum confidence threshold (60%) below which advisories are suppressed.
const MIN_CONFIDENCE_PERCENT: u8 = 60;

/// Evaluate optimization every N packets.
const EVALUATE_EVERY_N_PACKETS: u64 = 10;

/// Minimum history entries required.
const MIN_HISTORY_ENTRIES: usize = 10;

/// CfC anomaly score threshold — above this, defer to incident system.
const CFC_ANOMALY_THRESHOLD: f64 = 0.7;

/// Core optimization engine that compares real-time drilling parameters
/// against formation prognosis data and produces bounded recommendations.
pub struct ParameterOptimizer {
    rate_limiter: RateLimiter,
    packet_counter: u64,
    last_advisory: Option<OptimizationAdvisory>,
}

impl ParameterOptimizer {
    /// Create a new optimizer with the given rate-limiter cooldown (seconds).
    pub fn new(cooldown_secs: u64) -> Self {
        Self {
            rate_limiter: RateLimiter::new(cooldown_secs),
            packet_counter: 0,
            last_advisory: None,
        }
    }

    /// Get a reference to the last advisory produced.
    pub fn last_advisory(&self) -> Option<&OptimizationAdvisory> {
        self.last_advisory.as_ref()
    }

    /// Main evaluation entry point.
    ///
    /// Runs every `EVALUATE_EVERY_N_PACKETS` packets. Returns either an advisory
    /// or a skip reason.
    pub fn evaluate(
        &mut self,
        packet: &WitsPacket,
        physics: &DrillingPhysicsReport,
        formation: &FormationInterval,
        prognosis: &FormationPrognosis,
        history: &[HistoryEntry],
        cfc_anomaly_score: Option<f64>,
        sensor_quality: f64,
    ) -> Result<OptimizationAdvisory, OptimizationSkipReason> {
        // 1. Counter gate: only run every N packets
        self.packet_counter += 1;
        if self.packet_counter % EVALUATE_EVERY_N_PACKETS != 0 {
            return Err(OptimizationSkipReason::RateLimited);
        }

        // 2. CfC anomaly gate
        if let Some(score) = cfc_anomaly_score {
            if score > CFC_ANOMALY_THRESHOLD {
                return Err(OptimizationSkipReason::AnomalyActive);
            }
        }

        // 3. Rig state gate
        if !matches!(packet.rig_state, RigState::Drilling) {
            return Err(OptimizationSkipReason::NotDrilling);
        }

        // 4. History gate
        if history.len() < MIN_HISTORY_ENTRIES {
            return Err(OptimizationSkipReason::InsufficientHistory);
        }

        // 5. Calculate MSE efficiency vs formation offset data
        let mse_efficiency = if formation.offset_performance.avg_mse_psi > 0.0 {
            (formation.offset_performance.avg_mse_psi / physics.avg_mse.max(1.0) * 100.0)
                .min(100.0)
        } else {
            physics.mse_efficiency
        };

        // 6. Calculate ROP ratio
        let rop_ratio = if formation.offset_performance.best_rop_ft_hr > 0.0 {
            packet.rop / formation.offset_performance.best_rop_ft_hr
        } else {
            1.0
        };

        // 7. Evaluate each parameter
        let mut recommendations = Vec::new();

        if let Some(rec) = self.evaluate_parameter(
            DrillingParameter::Wob,
            physics.current_wob,
            &formation.parameters.wob_klbs,
            formation.offset_performance.best_params.wob_klbs,
            formation,
        ) {
            recommendations.push(rec);
        }

        if let Some(rec) = self.evaluate_parameter(
            DrillingParameter::Rpm,
            physics.current_rpm,
            &formation.parameters.rpm,
            formation.offset_performance.best_params.rpm,
            formation,
        ) {
            recommendations.push(rec);
        }

        // Flow rate: no offset best_params for flow, use optimal
        if let Some(rec) = self.evaluate_parameter(
            DrillingParameter::FlowRate,
            physics.current_flow_in,
            &formation.parameters.flow_gpm,
            formation.parameters.flow_gpm.optimal,
            formation,
        ) {
            recommendations.push(rec);
        }

        // 8. Sort by expected impact (highest first)
        recommendations.sort_by(|a, b| {
            b.expected_impact
                .partial_cmp(&a.expected_impact)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // 9. Filter through rate limiter
        recommendations.retain(|rec| {
            self.rate_limiter
                .can_recommend(rec.parameter, rec.recommended_value)
        });

        // 10. Score confidence
        let confidence =
            score_confidence(formation, physics, history, cfc_anomaly_score, sensor_quality);

        if confidence.percent() < MIN_CONFIDENCE_PERCENT {
            return Err(OptimizationSkipReason::LowConfidence);
        }

        // 11. Check look-ahead
        let look_ahead = check_look_ahead(prognosis, packet.bit_depth, packet.rop, formation, super::look_ahead::LOOK_AHEAD_THRESHOLD_MINUTES);

        // 12. Need at least one recommendation or a look-ahead to produce an advisory
        if recommendations.is_empty() && look_ahead.is_none() {
            return Err(OptimizationSkipReason::LowConfidence);
        }

        // 13. Record rate limiter state for accepted recommendations
        for rec in &recommendations {
            self.rate_limiter.record(rec.parameter, rec.recommended_value);
        }

        let advisory = OptimizationAdvisory {
            formation: formation.name.clone(),
            depth_ft: packet.bit_depth,
            recommendations,
            confidence,
            rop_ratio,
            mse_efficiency,
            look_ahead,
            source: "optimization_engine".to_string(),
        };

        self.last_advisory = Some(advisory.clone());
        Ok(advisory)
    }

    /// Evaluate a single drilling parameter against prognosis range and offset data.
    fn evaluate_parameter(
        &self,
        param: DrillingParameter,
        current: f64,
        range: &crate::types::ParameterRange,
        offset_best: f64,
        formation: &FormationInterval,
    ) -> Option<ParameterRecommendation> {
        let span = (range.max - range.min).abs().max(1e-6);

        // Determine recommended value
        let recommended = if current < range.min {
            // Below safe minimum → return to minimum
            range.min
        } else if current > range.max {
            // Above safe maximum → return to maximum
            range.max
        } else {
            // Within range — target offset best if within bounds, else optimal
            let target = if offset_best >= range.min && offset_best <= range.max {
                offset_best
            } else {
                range.optimal
            };

            // Only recommend if gap is meaningful (>5% of range)
            let gap = (current - target).abs();
            if gap / span < 0.05 {
                return None;
            }
            target
        };

        // Calculate expected impact: normalized gap × sensitivity
        let gap = (current - recommended).abs();
        let expected_impact = (gap / span).clamp(0.0, 1.0);

        // Build evidence string
        let evidence = if current < range.min || current > range.max {
            format!(
                "{} outside safe range [{:.1}–{:.1}] in {}",
                param, range.min, range.max, formation.name
            )
        } else {
            let wells = &formation.offset_performance.wells;
            if wells.is_empty() {
                format!(
                    "Prognosis optimal {:.1} for {} in {}",
                    range.optimal, param, formation.name
                )
            } else {
                format!(
                    "Offset wells ({}) best: {:.1} for {} in {}",
                    wells.join(", "),
                    offset_best,
                    param,
                    formation.name
                )
            }
        };

        Some(ParameterRecommendation {
            parameter: param,
            current_value: current,
            recommended_value: recommended,
            safe_min: range.min,
            safe_max: range.max,
            expected_impact,
            evidence,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use std::sync::Arc;

    fn make_formation() -> FormationInterval {
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
                wells: vec!["W-1".into(), "W-2".into(), "W-3".into()],
                avg_rop_ft_hr: 80.0,
                best_rop_ft_hr: 100.0,
                avg_mse_psi: 20000.0,
                best_params: BestParams { wob_klbs: 28.0, rpm: 130.0 },
                notes: String::new(),
            },
        }
    }

    fn make_prognosis(formation: &FormationInterval) -> FormationPrognosis {
        FormationPrognosis {
            well: PrognosisWellInfo {
                name: "Test-1".into(),
                field: "TestField".into(),
                spud_date: String::new(),
                target_depth_ft: 8000.0,
                coordinate_system: String::new(),
            },
            formations: vec![formation.clone()],
            casings: vec![],
        }
    }

    fn make_packet(rig_state: RigState) -> WitsPacket {
        WitsPacket {
            timestamp: 1705564800,
            bit_depth: 5500.0,
            hole_depth: 5550.0,
            rop: 50.0,
            hook_load: 200.0,
            wob: 20.0,  // Below offset best of 28
            rpm: 90.0,  // Below offset best of 130
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
            mse: 35000.0,
            d_exponent: 1.5,
            dxc: 1.45,
            rop_delta: 0.0,
            torque_delta_percent: 0.0,
            spp_delta: 0.0,
            rig_state,
            regime_id: 0,
            seconds_since_param_change: 0,        }
    }

    fn make_physics(wob: f64, rpm: f64, flow_in: f64) -> DrillingPhysicsReport {
        DrillingPhysicsReport {
            avg_mse: 35000.0,
            mse_efficiency: 57.0,
            optimal_mse: 20000.0,
            current_wob: wob,
            current_rpm: rpm,
            current_flow_in: flow_in,
            current_rop: 50.0,
            ..Default::default()
        }
    }

    fn make_history(count: usize) -> Vec<HistoryEntry> {
        (0..count)
            .map(|_| HistoryEntry {
                packet: make_packet(RigState::Drilling),
                metrics: DrillingMetrics {
                    state: RigState::Drilling,
                    operation: Operation::ProductionDrilling,
                    mse: 35000.0,
                    mse_efficiency: 57.0,
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
    fn skips_when_not_drilling() {
        let mut opt = ParameterOptimizer::new(300);
        let fm = make_formation();
        let prognosis = make_prognosis(&fm);
        let packet = make_packet(RigState::Idle);
        let physics = make_physics(20.0, 90.0, 500.0);
        let history = make_history(15);

        // Advance counter to evaluation point
        for _ in 0..9 {
            opt.packet_counter += 1;
        }

        let result = opt.evaluate(&packet, &physics, &fm, &prognosis, &history, None, 1.0);
        assert_eq!(result.unwrap_err(), OptimizationSkipReason::NotDrilling);
    }

    #[test]
    fn skips_when_cfc_anomaly_active() {
        let mut opt = ParameterOptimizer::new(300);
        let fm = make_formation();
        let prognosis = make_prognosis(&fm);
        let packet = make_packet(RigState::Drilling);
        let physics = make_physics(20.0, 90.0, 500.0);
        let history = make_history(15);

        for _ in 0..9 {
            opt.packet_counter += 1;
        }

        let result = opt.evaluate(&packet, &physics, &fm, &prognosis, &history, Some(0.8), 1.0);
        assert_eq!(result.unwrap_err(), OptimizationSkipReason::AnomalyActive);
    }

    #[test]
    fn skips_with_insufficient_history() {
        let mut opt = ParameterOptimizer::new(300);
        let fm = make_formation();
        let prognosis = make_prognosis(&fm);
        let packet = make_packet(RigState::Drilling);
        let physics = make_physics(20.0, 90.0, 500.0);
        let history = make_history(5); // Too few

        for _ in 0..9 {
            opt.packet_counter += 1;
        }

        let result = opt.evaluate(&packet, &physics, &fm, &prognosis, &history, None, 1.0);
        assert_eq!(
            result.unwrap_err(),
            OptimizationSkipReason::InsufficientHistory
        );
    }

    #[test]
    fn produces_recommendation_when_rpm_below_optimal() {
        let mut opt = ParameterOptimizer::new(0); // No cooldown for test
        let fm = make_formation();
        let prognosis = make_prognosis(&fm);
        let packet = make_packet(RigState::Drilling);
        let physics = make_physics(20.0, 90.0, 500.0); // RPM=90, offset best=130

        let history = make_history(15);

        // Advance to evaluation point
        for _ in 0..9 {
            opt.packet_counter += 1;
        }

        let result = opt.evaluate(&packet, &physics, &fm, &prognosis, &history, Some(0.1), 1.0);
        assert!(result.is_ok(), "Should produce advisory: {:?}", result);

        let adv = result.unwrap();
        assert!(!adv.recommendations.is_empty());

        // Should recommend RPM increase
        let rpm_rec = adv
            .recommendations
            .iter()
            .find(|r| r.parameter == DrillingParameter::Rpm);
        assert!(rpm_rec.is_some(), "Should have RPM recommendation");
        let rpm_rec = rpm_rec.unwrap();
        assert!(
            rpm_rec.recommended_value > rpm_rec.current_value,
            "Should recommend increasing RPM"
        );
    }

    #[test]
    fn recommendations_bounded_by_prognosis() {
        let mut opt = ParameterOptimizer::new(0);
        let fm = make_formation();
        let prognosis = make_prognosis(&fm);
        let mut packet = make_packet(RigState::Drilling);
        packet.wob = 10.0; // Below min of 15
        let physics = make_physics(10.0, 90.0, 500.0);
        let history = make_history(15);

        for _ in 0..9 {
            opt.packet_counter += 1;
        }

        let result = opt.evaluate(&packet, &physics, &fm, &prognosis, &history, Some(0.1), 1.0);
        let adv = result.unwrap();

        let wob_rec = adv
            .recommendations
            .iter()
            .find(|r| r.parameter == DrillingParameter::Wob);
        assert!(wob_rec.is_some());
        let wob_rec = wob_rec.unwrap();
        // Should recommend at least min, not below it
        assert!(
            wob_rec.recommended_value >= wob_rec.safe_min,
            "Recommendation {:.1} should be >= safe_min {:.1}",
            wob_rec.recommended_value,
            wob_rec.safe_min
        );
    }

    #[test]
    fn no_recommendation_when_near_optimal() {
        let mut opt = ParameterOptimizer::new(0);
        let mut fm = make_formation();
        // Set offset best same as optimal so target == current
        fm.offset_performance.best_params.wob_klbs = 25.0;
        fm.offset_performance.best_params.rpm = 120.0;

        let prognosis = make_prognosis(&fm);
        let mut packet = make_packet(RigState::Drilling);
        packet.wob = 25.0;
        packet.rpm = 120.0;
        packet.flow_in = 500.0;
        let physics = make_physics(25.0, 120.0, 500.0); // All at optimal
        let history = make_history(15);

        for _ in 0..9 {
            opt.packet_counter += 1;
        }

        let result = opt.evaluate(&packet, &physics, &fm, &prognosis, &history, Some(0.1), 1.0);
        // Should either have no recommendations (LowConfidence skip) or empty recommendations
        match result {
            Err(OptimizationSkipReason::LowConfidence) => {} // Expected: no recs, no look-ahead
            Ok(adv) => {
                // If we got an advisory, recommendations should be empty
                // (could have look-ahead)
                assert!(
                    adv.recommendations.is_empty(),
                    "Should have no parameter recommendations when at optimal"
                );
            }
            Err(other) => panic!("Unexpected skip reason: {other:?}"),
        }
    }

    #[test]
    fn rate_limiter_suppresses_rapid_recommendations() {
        let mut opt = ParameterOptimizer::new(300);
        let fm = make_formation();
        let prognosis = make_prognosis(&fm);
        let packet = make_packet(RigState::Drilling);
        let physics = make_physics(20.0, 90.0, 500.0);
        let history = make_history(15);

        // First evaluation
        for _ in 0..9 {
            opt.packet_counter += 1;
        }
        let first = opt.evaluate(&packet, &physics, &fm, &prognosis, &history, Some(0.1), 1.0);
        assert!(first.is_ok());

        // Second evaluation (same values, within cooldown)
        for _ in 0..9 {
            opt.packet_counter += 1;
        }
        let second = opt.evaluate(&packet, &physics, &fm, &prognosis, &history, Some(0.1), 1.0);
        // Should be suppressed: same values within cooldown → no recommendations
        match second {
            Err(OptimizationSkipReason::LowConfidence) => {} // Rate limiter filtered all recs
            Ok(adv) => {
                // All parameter recs should be filtered, maybe only look-ahead remains
                assert!(
                    adv.recommendations.is_empty(),
                    "Rate limiter should suppress same-value re-recommendations"
                );
            }
            Err(other) => panic!("Unexpected: {other:?}"),
        }
    }
}
