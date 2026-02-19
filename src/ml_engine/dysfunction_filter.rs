//! Dysfunction Filter for ML Analysis (V2.2)
//!
//! Filters out drilling data samples where dysfunction indicators were present.
//! This ensures the OptimalFinder only considers stable, sustainable operating points.
//!
//! ## Dysfunction Types Filtered
//! - **Torque Instability**: High coefficient of variation in torque (stick-slip precursor)
//! - **Stick-Slip**: Active torsional oscillation detected
//! - **Pack-Off**: Combined torque + SPP increases indicating restriction
//! - **Founder**: WOB increasing while ROP flat/decreasing (bit inefficiency)
//!
//! This stage runs AFTER quality filtering but BEFORE optimization.

use crate::types::{DrillingMetrics, WitsPacket};

/// Thresholds for dysfunction detection
pub mod dysfunction_thresholds {
    /// Torque coefficient of variation threshold for instability
    pub const TORQUE_CV_UNSTABLE: f64 = 0.12;
    /// Torque delta percent indicating pack-off risk
    pub const TORQUE_DELTA_PACKOFF: f64 = 0.10;
    /// SPP delta (psi) indicating pack-off risk
    pub const SPP_DELTA_PACKOFF: f64 = 75.0;
    /// Combined torque+SPP threshold for pack-off
    pub const COMBINED_PACKOFF_THRESHOLD: f64 = 0.15;
    /// WOB increase percent for founder detection
    pub const WOB_INCREASE_FOUNDER: f64 = 0.03;
    /// ROP response threshold for founder (below this = not responding)
    pub const ROP_RESPONSE_FOUNDER: f64 = 0.01;
    /// MSE efficiency threshold - below this indicates dysfunction
    pub const MSE_EFFICIENCY_UNSTABLE: f64 = 50.0;
    /// Minimum window size for rolling calculations
    pub const ROLLING_WINDOW_SIZE: usize = 10;
}

/// Reasons for dysfunction rejection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DysfunctionReason {
    /// High torque variability (stick-slip precursor)
    TorqueInstability,
    /// Active stick-slip oscillation
    StickSlip,
    /// Pack-off signature (torque + SPP rising)
    PackOff,
    /// Founder condition (WOB up, ROP down)
    Founder,
    /// Very low MSE efficiency
    LowEfficiency,
}

impl std::fmt::Display for DysfunctionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TorqueInstability => write!(f, "Torque instability"),
            Self::StickSlip => write!(f, "Stick-slip"),
            Self::PackOff => write!(f, "Pack-off"),
            Self::Founder => write!(f, "Founder condition"),
            Self::LowEfficiency => write!(f, "Low MSE efficiency"),
        }
    }
}

/// Result of dysfunction filtering
#[derive(Debug, Clone)]
pub struct DysfunctionFilterResult<'a> {
    /// Packets that passed dysfunction check (stable operation)
    pub stable_packets: Vec<&'a WitsPacket>,
    /// Metrics corresponding to stable packets
    pub stable_metrics: Vec<&'a DrillingMetrics>,
    /// Number of samples rejected due to dysfunction
    pub rejected_count: usize,
    /// Breakdown of rejection reasons
    pub rejection_breakdown: DysfunctionBreakdown,
    /// Stability score (0-1, fraction of stable samples)
    pub stability_score: f64,
}

/// Breakdown of dysfunction rejections by type
#[derive(Debug, Clone, Default)]
pub struct DysfunctionBreakdown {
    pub torque_instability: usize,
    pub stick_slip: usize,
    pub pack_off: usize,
    pub founder: usize,
    pub low_efficiency: usize,
}

impl DysfunctionBreakdown {
    pub fn total(&self) -> usize {
        self.torque_instability + self.stick_slip + self.pack_off + self.founder + self.low_efficiency
    }

    pub fn primary_reason(&self) -> Option<(DysfunctionReason, usize)> {
        let reasons = [
            (DysfunctionReason::TorqueInstability, self.torque_instability),
            (DysfunctionReason::StickSlip, self.stick_slip),
            (DysfunctionReason::PackOff, self.pack_off),
            (DysfunctionReason::Founder, self.founder),
            (DysfunctionReason::LowEfficiency, self.low_efficiency),
        ];

        reasons
            .into_iter()
            .filter(|(_, count)| *count > 0)
            .max_by_key(|(_, count)| *count)
    }
}

/// Dysfunction filter for ML analysis
pub struct DysfunctionFilter;

impl DysfunctionFilter {
    /// Filter out samples where dysfunction was detected
    ///
    /// Uses a rolling window approach to detect dysfunction patterns,
    /// then marks individual samples that fall within dysfunction periods.
    ///
    /// # Arguments
    /// * `packets` - Pre-filtered valid WITS packets
    /// * `metrics` - Corresponding drilling metrics
    ///
    /// # Returns
    /// DysfunctionFilterResult with stable samples only
    pub fn filter<'a>(
        packets: &'a [&'a WitsPacket],
        metrics: &'a [&'a DrillingMetrics],
    ) -> DysfunctionFilterResult<'a> {
        use dysfunction_thresholds::*;

        let n = packets.len();
        if n < ROLLING_WINDOW_SIZE {
            // Not enough data for rolling analysis - pass through
            return DysfunctionFilterResult {
                stable_packets: packets.to_vec(),
                stable_metrics: metrics.to_vec(),
                rejected_count: 0,
                rejection_breakdown: DysfunctionBreakdown::default(),
                stability_score: 1.0,
            };
        }

        let mut stable_packets = Vec::with_capacity(n);
        let mut stable_metrics = Vec::with_capacity(n);
        let mut breakdown = DysfunctionBreakdown::default();

        // Pre-compute rolling statistics for efficiency
        let torque_cv = Self::compute_rolling_cv(packets, |p| p.torque, ROLLING_WINDOW_SIZE);
        let founder_flags = Self::detect_founder_periods(packets, ROLLING_WINDOW_SIZE);

        for i in 0..n {
            let packet = packets[i];
            let metric = metrics[i];

            // Check each dysfunction type
            let dysfunction = Self::check_dysfunction(
                i,
                packet,
                metric,
                &torque_cv,
                &founder_flags,
            );

            match dysfunction {
                Some(DysfunctionReason::TorqueInstability) => breakdown.torque_instability += 1,
                Some(DysfunctionReason::StickSlip) => breakdown.stick_slip += 1,
                Some(DysfunctionReason::PackOff) => breakdown.pack_off += 1,
                Some(DysfunctionReason::Founder) => breakdown.founder += 1,
                Some(DysfunctionReason::LowEfficiency) => breakdown.low_efficiency += 1,
                None => {
                    // Sample is stable - keep it
                    stable_packets.push(packet);
                    stable_metrics.push(metric);
                }
            }
        }

        let rejected_count = breakdown.total();
        let stability_score = if n > 0 {
            (n - rejected_count) as f64 / n as f64
        } else {
            1.0
        };

        DysfunctionFilterResult {
            stable_packets,
            stable_metrics,
            rejected_count,
            rejection_breakdown: breakdown,
            stability_score,
        }
    }

    /// Check if a single sample exhibits dysfunction
    fn check_dysfunction(
        index: usize,
        _packet: &WitsPacket,
        metric: &DrillingMetrics,
        torque_cv: &[f64],
        founder_flags: &[bool],
    ) -> Option<DysfunctionReason> {
        use dysfunction_thresholds::*;

        // 1. Check torque instability (stick-slip precursor)
        if index < torque_cv.len() && torque_cv[index] > TORQUE_CV_UNSTABLE {
            return Some(DysfunctionReason::TorqueInstability);
        }

        // 2. Check for pack-off signature (torque + SPP both elevated)
        let torque_elevated = metric.torque_delta_percent > TORQUE_DELTA_PACKOFF;
        let spp_elevated = metric.spp_delta > SPP_DELTA_PACKOFF;
        if torque_elevated && spp_elevated {
            return Some(DysfunctionReason::PackOff);
        }

        // Combined torque/SPP score for subtle pack-off
        let combined_score = metric.torque_delta_percent.max(0.0) * 0.6
            + (metric.spp_delta / 200.0).max(0.0).min(1.0) * 0.4;
        if combined_score > COMBINED_PACKOFF_THRESHOLD {
            return Some(DysfunctionReason::PackOff);
        }

        // 3. Check founder condition
        if index < founder_flags.len() && founder_flags[index] {
            return Some(DysfunctionReason::Founder);
        }

        // 4. Check very low MSE efficiency (indicates something wrong)
        if metric.mse_efficiency < MSE_EFFICIENCY_UNSTABLE {
            return Some(DysfunctionReason::LowEfficiency);
        }

        // 5. Check if anomaly was flagged by tactical agent
        if metric.is_anomaly {
            // Map anomaly category to dysfunction type
            match metric.anomaly_category {
                crate::types::AnomalyCategory::Mechanical => {
                    // Could be stick-slip or pack-off
                    if metric.torque_delta_percent > TORQUE_DELTA_PACKOFF {
                        return Some(DysfunctionReason::PackOff);
                    }
                    return Some(DysfunctionReason::StickSlip);
                }
                crate::types::AnomalyCategory::DrillingEfficiency => {
                    return Some(DysfunctionReason::LowEfficiency);
                }
                _ => {
                    // Well control or hydraulic issues - also reject
                    return Some(DysfunctionReason::LowEfficiency);
                }
            }
        }

        None // Sample is stable
    }

    /// Compute rolling coefficient of variation for a parameter
    fn compute_rolling_cv<F>(packets: &[&WitsPacket], extractor: F, window: usize) -> Vec<f64>
    where
        F: Fn(&WitsPacket) -> f64,
    {
        let n = packets.len();
        let mut cv_values = vec![0.0; n];

        if n < window {
            return cv_values;
        }

        for i in window - 1..n {
            let start = i + 1 - window;
            let values: Vec<f64> = packets[start..=i].iter().map(|p| extractor(p)).collect();

            let mean = values.iter().sum::<f64>() / values.len() as f64;
            if mean > 0.0 {
                let variance =
                    values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
                let std_dev = variance.sqrt();
                cv_values[i] = std_dev / mean;
            }
        }

        cv_values
    }

    /// Detect founder periods (WOB trending up, ROP not responding)
    fn detect_founder_periods(packets: &[&WitsPacket], window: usize) -> Vec<bool> {
        use dysfunction_thresholds::*;

        let n = packets.len();
        let mut founder_flags = vec![false; n];

        if n < window {
            return founder_flags;
        }

        for i in window - 1..n {
            let start = i + 1 - window;

            // Calculate WOB and ROP trends over window
            let wob_values: Vec<f64> = packets[start..=i].iter().map(|p| p.wob).collect();
            let rop_values: Vec<f64> = packets[start..=i].iter().map(|p| p.rop).collect();

            let wob_trend = Self::calculate_trend(&wob_values);
            let rop_trend = Self::calculate_trend(&rop_values);

            // Calculate average WOB and ROP for normalization
            let avg_wob = wob_values.iter().sum::<f64>() / wob_values.len() as f64;
            let avg_rop = rop_values.iter().sum::<f64>() / rop_values.len() as f64;

            if avg_wob > 0.0 && avg_rop > 0.0 {
                let wob_trend_pct = wob_trend / avg_wob;
                let rop_trend_pct = rop_trend / avg_rop;

                // Founder: WOB increasing but ROP not responding or decreasing
                if wob_trend_pct > WOB_INCREASE_FOUNDER && rop_trend_pct < ROP_RESPONSE_FOUNDER {
                    founder_flags[i] = true;
                }
            }
        }

        founder_flags
    }

    /// Calculate linear trend (slope) using least squares
    fn calculate_trend(values: &[f64]) -> f64 {
        let n = values.len() as f64;
        if n < 2.0 {
            return 0.0;
        }

        let x_mean = (n - 1.0) / 2.0;
        let y_mean = values.iter().sum::<f64>() / n;

        let mut numerator = 0.0;
        let mut denominator = 0.0;

        for (i, &y) in values.iter().enumerate() {
            let x = i as f64;
            numerator += (x - x_mean) * (y - y_mean);
            denominator += (x - x_mean).powi(2);
        }

        if denominator > 0.0 {
            numerator / denominator
        } else {
            0.0
        }
    }

    /// Get a stability score for a single operating point
    ///
    /// Returns a score from 0.0 (highly unstable) to 1.0 (very stable)
    /// Used by OptimalFinder for stability penalty calculation
    pub fn calculate_stability_score(
        _packet: &WitsPacket,
        metric: &DrillingMetrics,
        torque_cv: f64,
    ) -> f64 {
        use dysfunction_thresholds::*;

        let mut score = 1.0;

        // Penalize high torque CV (approaching stick-slip)
        if torque_cv > TORQUE_CV_UNSTABLE * 0.5 {
            let cv_factor = (torque_cv - TORQUE_CV_UNSTABLE * 0.5)
                / (TORQUE_CV_UNSTABLE * 0.5);
            score -= 0.3 * cv_factor.min(1.0);
        }

        // Penalize elevated torque delta (approaching pack-off)
        if metric.torque_delta_percent > TORQUE_DELTA_PACKOFF * 0.5 {
            let td_factor = (metric.torque_delta_percent - TORQUE_DELTA_PACKOFF * 0.5)
                / (TORQUE_DELTA_PACKOFF * 0.5);
            score -= 0.25 * td_factor.min(1.0);
        }

        // Penalize low MSE efficiency
        if metric.mse_efficiency < 70.0 {
            let eff_factor = (70.0 - metric.mse_efficiency) / 20.0;
            score -= 0.2 * eff_factor.min(1.0);
        }

        // Penalize elevated SPP delta
        if metric.spp_delta > SPP_DELTA_PACKOFF * 0.5 {
            let spp_factor = (metric.spp_delta - SPP_DELTA_PACKOFF * 0.5)
                / (SPP_DELTA_PACKOFF * 0.5);
            score -= 0.15 * spp_factor.min(1.0);
        }

        score.max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AnomalyCategory, Operation, RigState};
    use std::sync::Arc;

    fn make_packet(wob: f64, rpm: f64, rop: f64, torque: f64, spp: f64) -> WitsPacket {
        WitsPacket {
            timestamp: 1000,
            bit_depth: 5000.0,
            hole_depth: 5000.0,
            rop,
            hook_load: 200.0,
            wob,
            rpm,
            torque,
            bit_diameter: 8.5,
            spp,
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
            waveform_snapshot: Arc::new(Vec::new()),
        }
    }

    fn make_metric(
        mse_efficiency: f64,
        torque_delta: f64,
        spp_delta: f64,
        is_anomaly: bool,
    ) -> DrillingMetrics {
        DrillingMetrics {
            state: RigState::Drilling,
            operation: Operation::ProductionDrilling,
            mse: 20000.0,
            mse_efficiency,
            d_exponent: 1.5,
            dxc: 1.4,
            mse_delta_percent: 0.0,
            flow_balance: 0.0,
            pit_rate: 0.0,
            ecd_margin: 1.0,
            torque_delta_percent: torque_delta,
            spp_delta,
            is_anomaly,
            anomaly_category: if is_anomaly {
                AnomalyCategory::Mechanical
            } else {
                AnomalyCategory::None
            },
            anomaly_description: None,
        }
    }

    #[test]
    fn test_stable_samples_pass_through() {
        // Create stable drilling data
        let packets: Vec<_> = (0..50)
            .map(|_| make_packet(20.0, 100.0, 50.0, 10.0, 3000.0))
            .collect();
        let metrics: Vec<_> = (0..50)
            .map(|_| make_metric(80.0, 0.02, 10.0, false))
            .collect();

        let packet_refs: Vec<_> = packets.iter().collect();
        let metric_refs: Vec<_> = metrics.iter().collect();

        let result = DysfunctionFilter::filter(&packet_refs, &metric_refs);

        assert_eq!(result.stable_packets.len(), 50);
        assert_eq!(result.rejected_count, 0);
        assert!((result.stability_score - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_packoff_samples_rejected() {
        // Create data with pack-off signatures
        let packets: Vec<_> = (0..20)
            .map(|_| make_packet(20.0, 100.0, 50.0, 15.0, 3200.0))
            .collect();
        let metrics: Vec<_> = (0..20)
            .map(|_| make_metric(75.0, 0.15, 100.0, false)) // High torque delta + SPP delta
            .collect();

        let packet_refs: Vec<_> = packets.iter().collect();
        let metric_refs: Vec<_> = metrics.iter().collect();

        let result = DysfunctionFilter::filter(&packet_refs, &metric_refs);

        assert!(result.rejected_count > 0);
        assert!(result.rejection_breakdown.pack_off > 0);
    }

    #[test]
    fn test_low_efficiency_rejected() {
        let packets: Vec<_> = (0..20)
            .map(|_| make_packet(20.0, 100.0, 20.0, 10.0, 3000.0))
            .collect();
        let metrics: Vec<_> = (0..20)
            .map(|_| make_metric(40.0, 0.0, 0.0, false)) // Very low MSE efficiency
            .collect();

        let packet_refs: Vec<_> = packets.iter().collect();
        let metric_refs: Vec<_> = metrics.iter().collect();

        let result = DysfunctionFilter::filter(&packet_refs, &metric_refs);

        assert!(result.rejected_count > 0);
        assert!(result.rejection_breakdown.low_efficiency > 0);
    }

    #[test]
    fn test_stability_score_calculation() {
        let packet = make_packet(20.0, 100.0, 50.0, 10.0, 3000.0);
        let metric = make_metric(80.0, 0.02, 10.0, false);

        let score = DysfunctionFilter::calculate_stability_score(&packet, &metric, 0.05);
        assert!(score > 0.9, "Stable sample should have high score: {}", score);

        // High torque CV should reduce score
        let unstable_score =
            DysfunctionFilter::calculate_stability_score(&packet, &metric, 0.15);
        assert!(
            unstable_score < score,
            "High CV should reduce score: {} vs {}",
            unstable_score,
            score
        );
    }
}
