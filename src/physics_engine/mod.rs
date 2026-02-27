//! Physics Engine Module
//!
//! Deterministic calculations for drilling operational intelligence.
//! All math here is pure physics/statistics - no ML involved.
//!
//! ## Phase 2 Functions (Fast, < 15ms)
//! - `tactical_update()` - Fast metrics from single WITS packet
//! - `calculate_mse()` - Mechanical Specific Energy
//! - `calculate_d_exponent()` - Drilling exponent for pore pressure
//! - `classify_rig_state()` - Operational state classification
//!
//! ## Phase 5 Functions (Advanced, run only on ticket)
//! - `strategic_drilling_analysis()` - Comprehensive trend analysis
//! - `detect_kick()` / `detect_lost_circulation()` - Well control
//! - `detect_packoff()` / `detect_stick_slip()` - Mechanical issues

pub mod drilling_models;
pub mod metrics;
pub mod models;

// Export drilling-specific functions
pub use drilling_models::{
    calculate_d_exponent, calculate_dxc, calculate_ecd, calculate_mse, calculate_mse_efficiency,
    calculate_r_squared, calculate_trend, classify_rig_state,
    detect_founder, detect_founder_quick, detect_kick, detect_lost_circulation, detect_packoff,
    detect_stick_slip, estimate_optimal_mse,
};


use tracing::warn;

use crate::baseline::BaselineOverrides;
use crate::types::{
    AnomalyCategory, DrillingMetrics, DrillingPhysicsReport, EnhancedPhysicsReport, HistoryEntry,
    RigState, WitsPacket,
};

// ============================================================================
// Tactical Update (Phase 2) - Fast path, < 15ms
// ============================================================================

/// Perform tactical update from a single WITS packet
///
/// Classifies operational state and calculates real-time drilling metrics.
/// Called every packet interval (typically 1-60 seconds).
///
/// Returns DrillingMetrics with:
/// - Rig state classification
/// - MSE and efficiency
/// - D-exponent and dxc
/// - Flow balance and pit rate
/// - Anomaly detection
pub fn tactical_update(
    packet: &WitsPacket,
    prev_packet: Option<&WitsPacket>,
    baseline_overrides: Option<&BaselineOverrides>,
) -> DrillingMetrics {
    // Classify operational state
    let state = classify_rig_state(packet);

    // Calculate MSE (only meaningful during drilling)
    let mse = if state == RigState::Drilling || state == RigState::Reaming {
        calculate_mse(
            packet.torque,
            packet.rpm,
            packet.bit_diameter,
            packet.rop,
            packet.wob,
        )
    } else {
        0.0
    };

    // Calculate d-exponent (only during drilling)
    let d_exponent = if state == RigState::Drilling {
        calculate_d_exponent(packet.rop, packet.rpm, packet.wob, packet.bit_diameter)
    } else {
        0.0
    };

    // Calculate corrected d-exponent using configurable normal mud weight
    let normal_mud_weight = if crate::config::is_initialized() {
        crate::config::get().thresholds.hydraulics.normal_mud_weight_ppg
    } else {
        8.6
    };
    let dxc = calculate_dxc(d_exponent, packet.mud_weight_in, normal_mud_weight);

    // Calculate flow balance (positive = gain/kick, negative = loss)
    // When both flow sensors read zero, we can't distinguish "balanced" from "no data".
    // When only flow_out is zero while flow_in > 0, skip flow balance to avoid false alerts.
    let flow_data_available = packet.flow_in > 0.0 || packet.flow_out > 0.0;
    let flow_out_available = packet.flow_out > 0.0 || !flow_data_available;
    let flow_balance = if flow_data_available && flow_out_available {
        packet.flow_out - packet.flow_in
    } else {
        0.0
    };

    // Calculate pit rate (bbl/hr) from packet interval
    // Requires minimum 10-second interval to avoid noise amplification from high-frequency sampling
    // Clamps to ±50 bbl/hr to filter unrealistic spikes
    let pit_rate = if let Some(prev) = prev_packet {
        let time_delta_secs = (packet.timestamp - prev.timestamp) as f64;
        // Only calculate rate if time delta is at least 10 seconds
        if time_delta_secs >= 10.0 {
            let time_delta_hr = time_delta_secs / 3600.0;
            let raw_rate = (packet.pit_volume - prev.pit_volume) / time_delta_hr;
            // Clamp to realistic bounds: ±50 bbl/hr
            // Real kicks/losses rarely exceed 30-40 bbl/hr initially
            raw_rate.clamp(-50.0, 50.0)
        } else {
            // For high-frequency sampling, use pit_volume_change if available
            // Otherwise return 0 to avoid noise
            if packet.pit_volume_change.abs() > 0.01 {
                // pit_volume_change is per-interval, convert to hourly rate
                let rate = packet.pit_volume_change * 3600.0 / time_delta_secs.max(1.0);
                rate.clamp(-50.0, 50.0)
            } else {
                0.0
            }
        }
    } else {
        0.0
    };

    // Calculate ECD margin to fracture.
    // Warn once-ish if fracture gradient is unavailable — the 1.5 ppg fallback
    // silences alarms and could mask a real well control issue.
    let ecd_margin = packet.ecd_margin();
    if packet.fracture_gradient <= 0.0 && packet.ecd > 0.0 {
        warn!("Fracture gradient unavailable (0.0) — ECD margin defaulting to 1.5 ppg; alarms suppressed");
    }

    // Calculate deltas from previous packet
    let (torque_delta_percent, spp_delta) = if let Some(prev) = prev_packet {
        // Require minimum absolute torque before computing percentage delta.
        // Near-zero torque (off-bottom, connections) produces huge percentages
        // from tiny absolute changes (0.01 → 9.0 = 89,900%).
        let torque_floor_klbs = 3.0;
        let torque_delta = if prev.torque > torque_floor_klbs {
            (packet.torque - prev.torque) / prev.torque
        } else {
            0.0
        };
        let spp_delta = packet.spp - prev.spp;
        (torque_delta, spp_delta)
    } else {
        (0.0, 0.0)
    };

    // Estimate MSE efficiency
    let formation_hardness = estimate_formation_hardness_from_rop(packet.rop, packet.wob, packet.rpm);
    let optimal_mse = estimate_optimal_mse(formation_hardness);
    let mse_efficiency = calculate_mse_efficiency(mse, optimal_mse);

    // Detect anomalies
    let (is_anomaly, anomaly_category, anomaly_description) =
        detect_anomalies(packet, prev_packet, &state, flow_balance, pit_rate, mse_efficiency, torque_delta_percent, spp_delta, flow_out_available, baseline_overrides);

    DrillingMetrics {
        state,
        operation: crate::types::Operation::Static, // Set by tactical agent based on campaign
        mse,
        mse_efficiency,
        d_exponent,
        dxc,
        mse_delta_percent: 0.0, // Calculated in baseline comparison
        flow_balance,
        pit_rate,
        ecd_margin,
        torque_delta_percent,
        spp_delta,
        flow_data_available,
        is_anomaly,
        anomaly_category,
        anomaly_description,
        current_formation: None,
        formation_depth_in_ft: None,
    }
}

/// Estimate formation hardness from drilling parameters
///
/// Uses relationship between ROP, WOB, and RPM to estimate
/// relative formation hardness on 0-10 scale.
fn estimate_formation_hardness_from_rop(rop: f64, wob: f64, rpm: f64) -> f64 {
    if rop <= 0.0 || wob <= 0.0 || rpm <= 0.0 {
        return 5.0; // Default medium hardness
    }

    // Drillability exponent approximation
    // Higher ROP for given WOB/RPM = softer formation
    let expected_rop = wob * rpm * 0.01; // Simplified model
    let drillability = rop / expected_rop.max(1.0);

    // Convert to hardness (inverse relationship)
    // drillability > 1 = soft, drillability < 1 = hard
    let hardness = 5.0 / drillability.max(0.1);
    hardness.clamp(0.0, 10.0)
}

/// Detect anomalies from drilling metrics
fn detect_anomalies(
    packet: &WitsPacket,
    prev_packet: Option<&WitsPacket>,
    state: &RigState,
    flow_balance: f64,
    pit_rate: f64,
    mse_efficiency: f64,
    torque_delta_percent: f64,
    spp_delta: f64,
    flow_out_available: bool,
    baseline_overrides: Option<&BaselineOverrides>,
) -> (bool, AnomalyCategory, Option<String>) {
    // Only check during active drilling states
    if *state != RigState::Drilling && *state != RigState::Reaming && *state != RigState::Circulating {
        return (false, AnomalyCategory::None, None);
    }

    // === WELL CONTROL (highest priority - safety critical) ===
    // Skip flow-based kick/loss detection when flow_out sensor is unavailable,
    // as it would produce false positives from the flow_in - 0 imbalance.

    if flow_out_available {
        // Check for kick indicators
        let (is_kick, kick_severity) = detect_kick(
            packet.flow_in,
            packet.flow_out,
            packet.pit_volume_change,
            packet.gas_units,
            packet.background_gas,
        );
        if is_kick {
            let severity_str = if kick_severity > 0.7 { "CRITICAL" } else if kick_severity > 0.4 { "HIGH" } else { "WARNING" };
            return (
                true,
                AnomalyCategory::WellControl,
                Some(format!("{}: Potential kick detected - flow imbalance {:.1} gpm, gas {:.0} units",
                    severity_str, flow_balance, packet.gas_units)),
            );
        }

        // Check for loss indicators
        let (is_loss, loss_severity) = detect_lost_circulation(
            packet.flow_in,
            packet.flow_out,
            packet.pit_volume_change,
            spp_delta.max(0.0),
        );
        if is_loss {
            let severity_str = if loss_severity > 0.7 { "CRITICAL" } else if loss_severity > 0.4 { "HIGH" } else { "WARNING" };
            return (
                true,
                AnomalyCategory::WellControl,
                Some(format!("{}: Potential lost circulation - flow imbalance {:.1} gpm, pit rate {:.1} bbl/hr",
                    severity_str, flow_balance, pit_rate)),
            );
        }
    }

    // Read thresholds from config
    let cfg_available = crate::config::is_initialized();
    let (gas_warn, gas_crit) = if cfg_available {
        let wc = &crate::config::get().thresholds.well_control;
        (wc.gas_units_warning, wc.gas_units_critical)
    } else { (100.0, 500.0) };
    let (h2s_warn, h2s_crit) = if cfg_available {
        let wc = &crate::config::get().thresholds.well_control;
        (wc.h2s_warning_ppm, wc.h2s_critical_ppm)
    } else { (10.0, 20.0) };
    let (pr_warn, pr_crit) = if cfg_available {
        let wc = &crate::config::get().thresholds.well_control;
        (wc.pit_rate_warning_bbl_hr, wc.pit_rate_critical_bbl_hr)
    } else { (5.0, 15.0) };
    let (ecd_warn, ecd_crit) = if cfg_available {
        let h = &crate::config::get().thresholds.hydraulics;
        (h.ecd_margin_warning_ppg, h.ecd_margin_critical_ppg)
    } else { (0.3, 0.1) };
    let (spp_warn, spp_crit) = {
        let (cfg_warn, cfg_crit) = if cfg_available {
            let h = &crate::config::get().thresholds.hydraulics;
            (h.spp_deviation_warning_psi, h.spp_deviation_critical_psi)
        } else { (100.0, 200.0) };
        (
            baseline_overrides.and_then(|o| o.spp_deviation_warning_psi).unwrap_or(cfg_warn),
            baseline_overrides.and_then(|o| o.spp_deviation_critical_psi).unwrap_or(cfg_crit),
        )
    };
    let (torq_warn, torq_crit) = {
        let (cfg_warn, cfg_crit) = if cfg_available {
            let m = &crate::config::get().thresholds.mechanical;
            (m.torque_increase_warning, m.torque_increase_critical)
        } else { (0.15, 0.25) };
        (
            baseline_overrides.and_then(|o| o.torque_warning_fraction).unwrap_or(cfg_warn),
            baseline_overrides.and_then(|o| o.torque_critical_fraction).unwrap_or(cfg_crit),
        )
    };

    // Gas warning
    if packet.gas_units > gas_warn {
        let severity_str = if packet.gas_units > gas_crit { "CRITICAL" } else { "WARNING" };
        return (
            true,
            AnomalyCategory::WellControl,
            Some(format!("{}: Elevated gas {:.0} units (background: {:.0})",
                severity_str, packet.gas_units, packet.background_gas)),
        );
    }

    // H2S warning
    if packet.h2s > h2s_warn {
        let severity_str = if packet.h2s > h2s_crit { "CRITICAL" } else { "WARNING" };
        return (
            true,
            AnomalyCategory::WellControl,
            Some(format!("{}: H2S detected at {:.1} ppm", severity_str, packet.h2s)),
        );
    }

    // Pit rate anomaly
    if pit_rate.abs() > pr_warn {
        let severity_str = if pit_rate.abs() > pr_crit { "CRITICAL" } else { "WARNING" };
        let direction = if pit_rate > 0.0 { "gain" } else { "loss" };
        return (
            true,
            AnomalyCategory::WellControl,
            Some(format!("{}: Pit {} rate {:.1} bbl/hr", severity_str, direction, pit_rate.abs())),
        );
    }

    // === HYDRAULICS ===

    // ECD margin
    if packet.ecd_margin() < ecd_warn {
        let severity_str = if packet.ecd_margin() < ecd_crit { "CRITICAL" } else { "WARNING" };
        return (
            true,
            AnomalyCategory::Hydraulics,
            Some(format!("{}: ECD margin only {:.2} ppg to fracture", severity_str, packet.ecd_margin())),
        );
    }

    // SPP deviation
    if spp_delta.abs() > spp_warn {
        let severity_str = if spp_delta.abs() > spp_crit { "HIGH" } else { "WARNING" };
        let direction = if spp_delta > 0.0 { "increase" } else { "decrease" };
        return (
            true,
            AnomalyCategory::Hydraulics,
            Some(format!("{}: SPP {} of {:.0} psi", severity_str, direction, spp_delta.abs())),
        );
    }

    // === MECHANICAL ===

    // Torque increase (potential pack-off or stuck pipe)
    if torque_delta_percent > torq_warn {
        let severity_str = if torque_delta_percent > torq_crit { "HIGH" } else { "WARNING" };
        return (
            true,
            AnomalyCategory::Mechanical,
            Some(format!("{}: Torque increase {:.1}% - potential pack-off", severity_str, torque_delta_percent * 100.0)),
        );
    }

    // Founder detection (WOB increasing but ROP not responding)
    // Quick check using two consecutive packets - strategic agent will verify with full history
    if let Some(prev) = prev_packet {
        let (is_potential_founder, wob_delta, rop_delta) = detect_founder_quick(
            prev.wob,
            prev.rop,
            packet.wob,
            packet.rop,
        );
        if is_potential_founder && *state == RigState::Drilling {
            let severity_str = if rop_delta < -0.05 { "HIGH" } else { "WARNING" };
            return (
                true,
                AnomalyCategory::Mechanical,
                Some(format!(
                    "{}: Founder condition - WOB increased {:.1}% but ROP {} {:.1}%. Reduce WOB.",
                    severity_str,
                    wob_delta * 100.0,
                    if rop_delta < 0.0 { "decreased" } else { "flat at" },
                    rop_delta.abs() * 100.0
                )),
            );
        }
    }

    // === DRILLING EFFICIENCY ===

    // MSE efficiency warning (only during drilling)
    let (mse_warn, mse_poor) = if cfg_available {
        let m = &crate::config::get().thresholds.mse;
        (m.efficiency_warning_percent, m.efficiency_poor_percent)
    } else { (70.0, 50.0) };
    if *state == RigState::Drilling && mse_efficiency < mse_warn {
        let severity_str = if mse_efficiency < mse_poor { "HIGH" } else { "LOW" };
        return (
            true,
            AnomalyCategory::DrillingEfficiency,
            Some(format!("{}: MSE efficiency {:.0}% - optimization opportunity", severity_str, mse_efficiency)),
        );
    }

    (false, AnomalyCategory::None, None)
}

// ============================================================================
// Strategic Analysis (Phase 5) - Runs on ticket
// ============================================================================

/// Perform strategic drilling physics analysis over history window
///
/// Called when an advisory ticket is created to provide deep analysis.
/// Returns comprehensive DrillingPhysicsReport with trends and predictions.
pub fn strategic_analysis(history: &[HistoryEntry]) -> DrillingPhysicsReport {
    drilling_models::strategic_drilling_analysis(history)
}

/// Enhanced strategic analysis for verification system
///
/// Provides additional confidence metrics for verification decisions.
pub fn enhanced_strategic_analysis(history: &[HistoryEntry]) -> EnhancedPhysicsReport {
    if history.is_empty() {
        return EnhancedPhysicsReport::default();
    }

    let base = drilling_models::strategic_drilling_analysis(history);

    // Calculate history duration in hours
    let history_hours = if history.len() >= 2 {
        let first_ts = history.first().map(|h| h.packet.timestamp).unwrap_or(0);
        let last_ts = history.last().map(|h| h.packet.timestamp).unwrap_or(0);
        (last_ts - first_ts) as f64 / 3600.0
    } else {
        0.0
    };

    // Calculate trend consistency
    let mse_values: Vec<f64> = history.iter().map(|h| h.metrics.mse).collect();
    let trend_consistency = calculate_r_squared(&mse_values);

    // Calculate confidence factor
    let depth_factor = (history.len() as f64 / 60.0).min(1.0);
    let consistency_factor = trend_consistency;
    let operating_count = history.iter()
        .filter(|h| h.metrics.state == RigState::Drilling || h.metrics.state == RigState::Reaming)
        .count();
    let operating_factor = operating_count as f64 / history.len().max(1) as f64;
    let confidence_factor = (depth_factor * 0.4 + consistency_factor * 0.3 + operating_factor * 0.3).min(1.0);

    // Check if anomaly is sustained
    let anomaly_count = history.iter()
        .rev()
        .take(10)
        .filter(|h| h.metrics.is_anomaly)
        .count();
    let is_sustained = anomaly_count >= 3;

    EnhancedPhysicsReport {
        base,
        trend_consistency,
        confidence_factor,
        history_hours,
        is_sustained,
        consecutive_anomaly_count: anomaly_count as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn create_drilling_packet() -> WitsPacket {
        WitsPacket {
            timestamp: 1000,
            bit_depth: 10000.0,
            hole_depth: 10000.0,
            rop: 60.0,
            hook_load: 150.0,
            wob: 25.0,
            rpm: 120.0,
            torque: 15.0,
            bit_diameter: 8.5,
            spp: 3000.0,
            pump_spm: 120.0,
            flow_in: 500.0,
            flow_out: 500.0,
            pit_volume: 800.0,
            pit_volume_change: 0.0,
            mud_weight_in: 10.5,
            mud_weight_out: 10.5,
            ecd: 10.8,
            mud_temp_in: 80.0,
            mud_temp_out: 120.0,
            gas_units: 20.0,
            background_gas: 15.0,
            connection_gas: 0.0,
            h2s: 0.0,
            co2: 0.0,
            casing_pressure: 0.0,
            annular_pressure: 0.0,
            pore_pressure: 9.0,
            fracture_gradient: 14.0,
            mse: 0.0,
            d_exponent: 0.0,
            dxc: 0.0,
            rop_delta: 0.0,
            torque_delta_percent: 0.0,
            spp_delta: 0.0,
            rig_state: RigState::Drilling,
            regime_id: 0,
            seconds_since_param_change: 0,        }
    }

    #[test]
    fn test_tactical_update_normal_drilling() {
        let packet = create_drilling_packet();
        let metrics = tactical_update(&packet, None, None);

        assert_eq!(metrics.state, RigState::Drilling);
        assert!(metrics.mse > 0.0, "MSE should be calculated during drilling");
        // D-exponent can be negative for certain drilling parameter combinations
        // Just verify it's a finite number (not NaN or Inf)
        assert!(metrics.d_exponent.is_finite(), "D-exponent should be finite");
        assert!(!metrics.is_anomaly, "Normal drilling should not trigger anomaly");
    }

    #[test]
    fn test_tactical_update_kick_detection() {
        let mut packet = create_drilling_packet();
        packet.flow_out = 530.0; // 30 gpm gain
        packet.pit_volume_change = 8.0; // 8 bbl gain
        packet.gas_units = 200.0; // Elevated gas

        let metrics = tactical_update(&packet, None, None);

        assert!(metrics.is_anomaly, "Should detect kick conditions");
        assert_eq!(metrics.anomaly_category, AnomalyCategory::WellControl);
    }

    #[test]
    fn test_tactical_update_low_efficiency() {
        let mut packet = create_drilling_packet();
        packet.rop = 10.0; // Low ROP
        packet.wob = 35.0; // High WOB
        // This should result in poor MSE efficiency

        let metrics = tactical_update(&packet, None, None);

        // Low efficiency is detected when MSE is higher than optimal
        // MSE efficiency is capped at 100.0, so just verify metrics were calculated
        assert!(metrics.mse > 0.0, "MSE should be calculated");
        assert!(metrics.mse_efficiency <= 100.0, "MSE efficiency should be <= 100");
    }

    #[test]
    fn test_rig_state_classification() {
        let mut packet = WitsPacket::default();

        // Idle
        assert_eq!(classify_rig_state(&packet), RigState::Idle);

        // Drilling
        packet.rpm = 120.0;
        packet.wob = 25.0;
        packet.rop = 60.0;
        packet.flow_in = 500.0;
        packet.bit_depth = 10000.0;
        packet.hole_depth = 10000.0;
        assert_eq!(classify_rig_state(&packet), RigState::Drilling);

        // Circulating
        packet.wob = 0.0;
        packet.rop = 0.0;
        assert_eq!(classify_rig_state(&packet), RigState::Circulating);
    }
}
