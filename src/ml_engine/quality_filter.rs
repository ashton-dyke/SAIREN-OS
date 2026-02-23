//! Data Quality Filter for ML Analysis (V2)
//!
//! Filters drilling data to ensure only valid samples are used for ML analysis.
//! Rejects:
//! - Connection/idle data (WOB < 5 klbs, RPM < 40)
//! - Sensor glitches (MSE/ROP out of plausible range)
//! - Non-drilling rig states

use crate::types::{
    ml_quality_thresholds::*, DrillingMetrics, RigState, WitsPacket,
};

/// Result of quality filtering
#[derive(Debug, Clone)]
pub struct FilterResult<'a> {
    /// Valid packets that passed all quality checks
    pub valid_packets: Vec<&'a WitsPacket>,
    /// Valid metrics corresponding to valid packets
    pub valid_metrics: Vec<&'a DrillingMetrics>,
    /// Primary reason for rejections (if any)
    pub rejection_reason: Option<String>,
}

/// Data quality filter for ML analysis
pub struct DataQualityFilter;

impl DataQualityFilter {
    /// Filter packets to only valid drilling data
    ///
    /// Returns a FilterResult containing:
    /// - Valid packets/metrics that passed all quality checks
    /// - Count of rejected samples
    /// - Primary rejection reason (if applicable)
    ///
    /// # Quality Criteria
    /// - WOB >= 5 klbs (drilling, not connections)
    /// - RPM >= 40 (rotating, not stationary)
    /// - MSE between 1,000 and 500,000 psi (rejects sensor glitches)
    /// - ROP between 1 and 500 ft/hr (rejects unrealistic values)
    /// - Rig state is Drilling or Reaming
    pub fn filter<'a>(
        packets: &'a [WitsPacket],
        metrics: &'a [DrillingMetrics],
    ) -> FilterResult<'a> {
        let mut valid_packets = Vec::new();
        let mut valid_metrics = Vec::new();
        let mut rejected = 0;

        // Track rejection reasons for reporting
        let mut low_wob_count = 0;
        let mut low_rpm_count = 0;
        let mut bad_mse_count = 0;
        let mut bad_rop_count = 0;
        let mut wrong_state_count = 0;

        for (packet, metric) in packets.iter().zip(metrics.iter()) {
            match Self::validate(packet, metric) {
                Ok(()) => {
                    valid_packets.push(packet);
                    valid_metrics.push(metric);
                }
                Err(reason) => {
                    rejected += 1;
                    match reason {
                        RejectionReason::LowWob => low_wob_count += 1,
                        RejectionReason::LowRpm => low_rpm_count += 1,
                        RejectionReason::InvalidMse => bad_mse_count += 1,
                        RejectionReason::InvalidRop => bad_rop_count += 1,
                        RejectionReason::WrongRigState => wrong_state_count += 1,
                    }
                }
            }
        }

        // Determine primary rejection reason
        let rejection_reason = if rejected > 0 {
            let max_reason = [
                (low_wob_count, "WOB < 5 klbs (connection/idle)"),
                (low_rpm_count, "RPM < 40 (not rotating)"),
                (bad_mse_count, "MSE out of range (sensor glitch)"),
                (bad_rop_count, "ROP out of range (invalid)"),
                (wrong_state_count, "Non-drilling rig state"),
            ]
            .into_iter()
            .max_by_key(|(count, _)| *count)
            .filter(|(count, _)| *count > 0)
            .map(|(count, reason)| format!("{} ({} samples)", reason, count));
            max_reason
        } else {
            None
        };

        FilterResult {
            valid_packets,
            valid_metrics,
            rejection_reason,
        }
    }

    /// Validate a single packet/metric pair
    fn validate(packet: &WitsPacket, metric: &DrillingMetrics) -> Result<(), RejectionReason> {
        // Check WOB (reject connection/idle data)
        if packet.wob < MIN_WOB {
            return Err(RejectionReason::LowWob);
        }

        // Check RPM (reject stationary pipe)
        if packet.rpm < MIN_RPM {
            return Err(RejectionReason::LowRpm);
        }

        // Check MSE (reject sensor glitches)
        if metric.mse < MIN_PLAUSIBLE_MSE || metric.mse > MAX_PLAUSIBLE_MSE {
            return Err(RejectionReason::InvalidMse);
        }

        // Check ROP (reject unrealistic values)
        if packet.rop < MIN_ROP || packet.rop > MAX_PLAUSIBLE_ROP {
            return Err(RejectionReason::InvalidRop);
        }

        // Check rig state (only analyze drilling/reaming)
        if !matches!(packet.rig_state, RigState::Drilling | RigState::Reaming) {
            return Err(RejectionReason::WrongRigState);
        }

        Ok(())
    }

    /// Quick check if a packet is valid for ML analysis
    #[cfg(test)]
    pub fn is_valid(packet: &WitsPacket, metric: &DrillingMetrics) -> bool {
        Self::validate(packet, metric).is_ok()
    }
}

/// Reasons for sample rejection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RejectionReason {
    LowWob,
    LowRpm,
    InvalidMse,
    InvalidRop,
    WrongRigState,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn make_valid_packet() -> WitsPacket {
        WitsPacket {
            timestamp: 1000,
            bit_depth: 5000.0,
            hole_depth: 5000.0,
            rop: 50.0,
            hook_load: 200.0,
            wob: 20.0,  // Valid: > 5 klbs
            rpm: 120.0, // Valid: > 40
            torque: 10.0,
            bit_diameter: 8.5,
            spp: 3000.0,
            pump_spm: 60.0,
            flow_in: 500.0,
            flow_out: 495.0,
            pit_volume: 800.0,
            pit_volume_change: 0.0,
            mud_weight_in: 10.0,
            mud_weight_out: 10.1,
            ecd: 10.5,
            mud_temp_in: 80.0,
            mud_temp_out: 95.0,
            gas_units: 10.0,
            background_gas: 5.0,
            connection_gas: 0.0,
            h2s: 0.0,
            co2: 0.0,
            casing_pressure: 100.0,
            annular_pressure: 150.0,
            pore_pressure: 8.6,
            fracture_gradient: 14.0,
            mse: 20000.0,
            d_exponent: 1.5,
            dxc: 1.4,
            rop_delta: 0.0,
            torque_delta_percent: 0.0,
            spp_delta: 0.0,
            rig_state: RigState::Drilling,
            regime_id: 0,
            seconds_since_param_change: 0,        }
    }

    fn make_valid_metric() -> DrillingMetrics {
        DrillingMetrics {
            state: RigState::Drilling,
            operation: crate::types::Operation::ProductionDrilling,
            mse: 20000.0, // Valid: between 1k and 500k
            mse_efficiency: 75.0,
            d_exponent: 1.5,
            dxc: 1.4,
            mse_delta_percent: 0.0,
            flow_balance: 0.0,
            pit_rate: 0.0,
            ecd_margin: 1.0,
            torque_delta_percent: 0.0,
            spp_delta: 0.0,
            flow_data_available: true,
            is_anomaly: false,
            anomaly_category: crate::types::AnomalyCategory::None,
            anomaly_description: None,
            current_formation: None,
            formation_depth_in_ft: None,
        }
    }

    #[test]
    fn test_valid_sample_passes() {
        let packet = make_valid_packet();
        let metric = make_valid_metric();

        assert!(DataQualityFilter::is_valid(&packet, &metric));
    }

    #[test]
    fn test_low_wob_rejected() {
        let mut packet = make_valid_packet();
        packet.wob = 3.0; // Below 5 klbs threshold
        let metric = make_valid_metric();

        assert!(!DataQualityFilter::is_valid(&packet, &metric));
    }

    #[test]
    fn test_low_rpm_rejected() {
        let mut packet = make_valid_packet();
        packet.rpm = 30.0; // Below 40 threshold
        let metric = make_valid_metric();

        assert!(!DataQualityFilter::is_valid(&packet, &metric));
    }

    #[test]
    fn test_invalid_mse_rejected() {
        let packet = make_valid_packet();
        let mut metric = make_valid_metric();
        metric.mse = 500.0; // Below 1000 threshold (sensor glitch)

        assert!(!DataQualityFilter::is_valid(&packet, &metric));
    }

    #[test]
    fn test_high_mse_rejected() {
        let packet = make_valid_packet();
        let mut metric = make_valid_metric();
        metric.mse = 600_000.0; // Above 500k threshold (sensor glitch)

        assert!(!DataQualityFilter::is_valid(&packet, &metric));
    }

    #[test]
    fn test_connection_state_rejected() {
        let mut packet = make_valid_packet();
        packet.rig_state = RigState::Connection;
        let metric = make_valid_metric();

        assert!(!DataQualityFilter::is_valid(&packet, &metric));
    }

    #[test]
    fn test_filter_returns_correct_counts() {
        let valid_packet = make_valid_packet();
        let valid_metric = make_valid_metric();

        let mut low_wob_packet = make_valid_packet();
        low_wob_packet.wob = 2.0;
        let low_wob_metric = make_valid_metric();

        let packets = vec![valid_packet.clone(), low_wob_packet, valid_packet.clone()];
        let metrics = vec![valid_metric.clone(), low_wob_metric, valid_metric];

        let result = DataQualityFilter::filter(&packets, &metrics);

        assert_eq!(result.valid_packets.len(), 2);
        assert_eq!(result.valid_metrics.len(), 2);
        assert!(result.rejection_reason.is_some());
    }
}
