//! Optimal Parameter Finder with Binned Grid Search and Stability Penalty (V2.2)
//!
//! Finds optimal drilling parameters using:
//! - **Grid-based binning** instead of "top 10% averaging" to avoid mixing disjoint modes
//! - **Stability penalty** that penalizes operating points near dysfunction thresholds
//! - **Campaign-aware composite scoring** (ROP vs MSE efficiency balance)
//! - **Safe operating ranges** from the winning bin (not just point estimates)
//!
//! The key insight is that "optimal" must mean both fast AND robust.
//! A high-ROP operating point that sits on the edge of stick-slip is not truly optimal.

use crate::types::{
    ml_quality_thresholds::MIN_ANALYSIS_SAMPLES, Campaign, ConfidenceLevel, DrillingMetrics,
    OptimalParams, WitsPacket,
};

use super::dysfunction_filter::DysfunctionFilter;

/// Grid search configuration
pub mod grid_config {
    /// Number of bins for WOB dimension
    pub const WOB_BINS: usize = 8;
    /// Number of bins for RPM dimension
    pub const RPM_BINS: usize = 6;
    /// Minimum samples in a bin to consider it valid
    pub const MIN_BIN_SAMPLES: usize = 10;
    /// Stability weight in composite score (0-1)
    /// Higher = more penalty for operating near dysfunction thresholds
    pub const STABILITY_WEIGHT: f64 = 0.25;
}

/// V2.2: Campaign-specific composite weights
/// Returns (rop_weight, mse_efficiency_weight, stability_weight)
fn get_weights(campaign: Campaign) -> (f64, f64, f64) {
    match campaign {
        // Production: ROP-focused but with stability consideration
        // 50% ROP + 30% MSE efficiency + 20% stability
        Campaign::Production => (0.50, 0.30, 0.20),
        // P&A: Stability-focused (MSE efficiency = operational stability)
        // 25% ROP + 45% MSE efficiency + 30% stability
        Campaign::PlugAbandonment => (0.25, 0.45, 0.30),
    }
}

/// A single bin in the WOB/RPM grid
#[derive(Debug, Clone)]
struct ParameterBin {
    /// Indices of samples falling in this bin
    sample_indices: Vec<usize>,
    /// WOB range for this bin
    wob_range: (f64, f64),
    /// RPM range for this bin
    rpm_range: (f64, f64),
    /// Composite score (higher = better)
    composite_score: f64,
    /// Average stability score
    stability_score: f64,
    /// Statistics for this bin
    stats: BinStats,
}

/// Statistics calculated for a bin
#[derive(Debug, Clone, Default)]
struct BinStats {
    wob_median: f64,
    wob_min: f64,
    wob_max: f64,
    rpm_median: f64,
    rpm_min: f64,
    rpm_max: f64,
    flow_median: f64,
    flow_min: f64,
    flow_max: f64,
    rop_median: f64,
    mse_median: f64,
    mse_efficiency_median: f64,
}

/// Optimal drilling parameter finder using binned grid search with stability penalty
pub struct OptimalFinder;

impl OptimalFinder {
    /// Find optimal parameters using binned grid search with stability penalty
    ///
    /// # Algorithm
    /// 1. Create a WOB × RPM grid (bins)
    /// 2. Assign each sample to its bin based on WOB/RPM values
    /// 3. For each bin with enough samples:
    ///    - Calculate performance score (ROP, MSE efficiency)
    ///    - Calculate stability score (proximity to dysfunction thresholds)
    ///    - Compute weighted composite score
    /// 4. Select the bin with highest composite score
    /// 5. Return median values and ranges from winning bin
    ///
    /// # Arguments
    /// * `packets` - Valid WITS packets (post quality + dysfunction filter)
    /// * `metrics` - Corresponding drilling metrics
    /// * `campaign` - Current campaign mode for weight selection
    /// * `dysfunction_filtered` - Whether dysfunction filtering was applied
    ///
    /// # Returns
    /// Some(OptimalParams) if sufficient data, None otherwise
    pub fn find_optimal(
        packets: &[&WitsPacket],
        metrics: &[&DrillingMetrics],
        campaign: Campaign,
        dysfunction_filtered: bool,
    ) -> Option<OptimalParams> {
        use grid_config::*;

        let n = packets.len();
        if n < MIN_ANALYSIS_SAMPLES {
            return None;
        }

        // Get campaign-specific weights
        let (rop_weight, mse_weight, stability_weight) = get_weights(campaign);

        // Calculate data ranges for bin boundaries
        let (wob_min, wob_max) = Self::get_range(packets, |p| p.wob);
        let (rpm_min, rpm_max) = Self::get_range(packets, |p| p.rpm);

        // Avoid degenerate cases
        if wob_max - wob_min < 1.0 || rpm_max - rpm_min < 5.0 {
            // Not enough variation - fall back to simple median
            return Some(Self::fallback_median(packets, metrics, n, dysfunction_filtered));
        }

        // Create bins
        let wob_step = (wob_max - wob_min) / WOB_BINS as f64;
        let rpm_step = (rpm_max - rpm_min) / RPM_BINS as f64;

        let mut bins: Vec<ParameterBin> = Vec::new();

        for wob_idx in 0..WOB_BINS {
            for rpm_idx in 0..RPM_BINS {
                let wob_lo = wob_min + wob_idx as f64 * wob_step;
                let wob_hi = wob_lo + wob_step;
                let rpm_lo = rpm_min + rpm_idx as f64 * rpm_step;
                let rpm_hi = rpm_lo + rpm_step;

                bins.push(ParameterBin {
                    sample_indices: Vec::new(),
                    wob_range: (wob_lo, wob_hi),
                    rpm_range: (rpm_lo, rpm_hi),
                    composite_score: 0.0,
                    stability_score: 0.0,
                    stats: BinStats::default(),
                });
            }
        }

        // Pre-compute stability scores for all samples (need rolling torque CV)
        let torque_cv_values = Self::compute_rolling_torque_cv(packets, 10);

        // Assign samples to bins
        for i in 0..n {
            let wob = packets[i].wob;
            let rpm = packets[i].rpm;

            let wob_idx = ((wob - wob_min) / wob_step).floor() as usize;
            let rpm_idx = ((rpm - rpm_min) / rpm_step).floor() as usize;

            // Clamp to valid range
            let wob_idx = wob_idx.min(WOB_BINS - 1);
            let rpm_idx = rpm_idx.min(RPM_BINS - 1);

            let bin_idx = wob_idx * RPM_BINS + rpm_idx;
            if bin_idx < bins.len() {
                bins[bin_idx].sample_indices.push(i);
            }
        }

        // Calculate scores for each valid bin
        let mut valid_bin_count = 0;
        for bin in &mut bins {
            if bin.sample_indices.len() >= MIN_BIN_SAMPLES {
                valid_bin_count += 1;
                Self::calculate_bin_score(
                    bin,
                    packets,
                    metrics,
                    &torque_cv_values,
                    rop_weight,
                    mse_weight,
                    stability_weight,
                );
            }
        }

        if valid_bin_count == 0 {
            // No bins have enough samples - fall back to median
            return Some(Self::fallback_median(packets, metrics, n, dysfunction_filtered));
        }

        // Find best bin
        let best_bin = bins
            .iter()
            .filter(|b| b.sample_indices.len() >= MIN_BIN_SAMPLES)
            .max_by(|a, b| {
                a.composite_score
                    .partial_cmp(&b.composite_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })?;

        // Build result from best bin
        let confidence = ConfidenceLevel::from_sample_count(n);

        Some(OptimalParams {
            best_wob: best_bin.stats.wob_median,
            best_rpm: best_bin.stats.rpm_median,
            best_flow: best_bin.stats.flow_median,
            wob_min: best_bin.stats.wob_min,
            wob_max: best_bin.stats.wob_max,
            rpm_min: best_bin.stats.rpm_min,
            rpm_max: best_bin.stats.rpm_max,
            flow_min: best_bin.stats.flow_min,
            flow_max: best_bin.stats.flow_max,
            achieved_rop: best_bin.stats.rop_median,
            achieved_mse: best_bin.stats.mse_median,
            mse_efficiency: best_bin.stats.mse_efficiency_median,
            composite_score: best_bin.composite_score,
            confidence,
            stability_score: best_bin.stability_score,
            bin_sample_count: best_bin.sample_indices.len(),
            bins_evaluated: valid_bin_count,
            dysfunction_filtered,
        })
    }

    /// Calculate composite score for a bin
    fn calculate_bin_score(
        bin: &mut ParameterBin,
        packets: &[&WitsPacket],
        metrics: &[&DrillingMetrics],
        torque_cv_values: &[f64],
        rop_weight: f64,
        mse_weight: f64,
        stability_weight: f64,
    ) {
        let indices = &bin.sample_indices;
        if indices.is_empty() {
            return;
        }

        // Collect values for this bin
        let mut wob_values: Vec<f64> = indices.iter().map(|&i| packets[i].wob).collect();
        let mut rpm_values: Vec<f64> = indices.iter().map(|&i| packets[i].rpm).collect();
        let mut flow_values: Vec<f64> = indices.iter().map(|&i| packets[i].flow_in).collect();
        let mut rop_values: Vec<f64> = indices.iter().map(|&i| packets[i].rop).collect();
        let mut mse_values: Vec<f64> = indices.iter().map(|&i| packets[i].mse).collect();
        let mut mse_eff_values: Vec<f64> = indices.iter().map(|&i| metrics[i].mse_efficiency).collect();

        // Sort for median/min/max calculation
        wob_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        rpm_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        flow_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        rop_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        mse_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        mse_eff_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // Calculate statistics
        bin.stats = BinStats {
            wob_median: Self::median(&wob_values),
            wob_min: *wob_values.first().unwrap_or(&0.0),
            wob_max: *wob_values.last().unwrap_or(&0.0),
            rpm_median: Self::median(&rpm_values),
            rpm_min: *rpm_values.first().unwrap_or(&0.0),
            rpm_max: *rpm_values.last().unwrap_or(&0.0),
            flow_median: Self::median(&flow_values),
            flow_min: *flow_values.first().unwrap_or(&0.0),
            flow_max: *flow_values.last().unwrap_or(&0.0),
            rop_median: Self::median(&rop_values),
            mse_median: Self::median(&mse_values),
            mse_efficiency_median: Self::median(&mse_eff_values),
        };

        // Calculate average stability score for samples in this bin
        let stability_scores: Vec<f64> = indices
            .iter()
            .map(|&i| {
                let torque_cv = if i < torque_cv_values.len() {
                    torque_cv_values[i]
                } else {
                    0.0
                };
                DysfunctionFilter::calculate_stability_score(packets[i], metrics[i], torque_cv)
            })
            .collect();
        bin.stability_score = stability_scores.iter().sum::<f64>() / stability_scores.len() as f64;

        // Normalize values for composite score calculation
        // Use typical ranges for normalization
        let norm_rop = (bin.stats.rop_median / 100.0).min(1.0); // Assume 100 ft/hr is very good
        let norm_mse_eff = bin.stats.mse_efficiency_median / 100.0; // Already 0-100%

        // Calculate composite score with stability penalty
        bin.composite_score = rop_weight * norm_rop
            + mse_weight * norm_mse_eff
            + stability_weight * bin.stability_score;
    }

    /// Compute rolling coefficient of variation for torque
    fn compute_rolling_torque_cv(packets: &[&WitsPacket], window: usize) -> Vec<f64> {
        let n = packets.len();
        let mut cv_values = vec![0.0; n];

        if n < window {
            return cv_values;
        }

        for i in window - 1..n {
            let start = i + 1 - window;
            let values: Vec<f64> = packets[start..=i].iter().map(|p| p.torque).collect();

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

    /// Get min/max range for a parameter
    fn get_range<F>(packets: &[&WitsPacket], extractor: F) -> (f64, f64)
    where
        F: Fn(&WitsPacket) -> f64,
    {
        let values: Vec<f64> = packets.iter().map(|p| extractor(p)).collect();
        let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        (min, max)
    }

    /// Calculate median of sorted values
    fn median(sorted_values: &[f64]) -> f64 {
        if sorted_values.is_empty() {
            return 0.0;
        }
        let mid = sorted_values.len() / 2;
        if sorted_values.len() % 2 == 0 {
            (sorted_values[mid - 1] + sorted_values[mid]) / 2.0
        } else {
            sorted_values[mid]
        }
    }

    /// Fallback to simple median when binning isn't possible
    fn fallback_median(
        packets: &[&WitsPacket],
        metrics: &[&DrillingMetrics],
        n: usize,
        dysfunction_filtered: bool,
    ) -> OptimalParams {
        let mut wob_values: Vec<f64> = packets.iter().map(|p| p.wob).collect();
        let mut rpm_values: Vec<f64> = packets.iter().map(|p| p.rpm).collect();
        let mut flow_values: Vec<f64> = packets.iter().map(|p| p.flow_in).collect();
        let mut rop_values: Vec<f64> = packets.iter().map(|p| p.rop).collect();
        let mut mse_values: Vec<f64> = packets.iter().map(|p| p.mse).collect();
        let mut mse_eff_values: Vec<f64> = metrics.iter().map(|m| m.mse_efficiency).collect();

        wob_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        rpm_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        flow_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        rop_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        mse_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        mse_eff_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let confidence = ConfidenceLevel::from_sample_count(n);

        OptimalParams {
            best_wob: Self::median(&wob_values),
            best_rpm: Self::median(&rpm_values),
            best_flow: Self::median(&flow_values),
            wob_min: *wob_values.first().unwrap_or(&0.0),
            wob_max: *wob_values.last().unwrap_or(&0.0),
            rpm_min: *rpm_values.first().unwrap_or(&0.0),
            rpm_max: *rpm_values.last().unwrap_or(&0.0),
            flow_min: *flow_values.first().unwrap_or(&0.0),
            flow_max: *flow_values.last().unwrap_or(&0.0),
            achieved_rop: Self::median(&rop_values),
            achieved_mse: Self::median(&mse_values),
            mse_efficiency: Self::median(&mse_eff_values),
            composite_score: 0.5, // Default middle score for fallback
            confidence,
            stability_score: 1.0, // Assume stable for fallback
            bin_sample_count: n,
            bins_evaluated: 0, // No binning was done
            dysfunction_filtered,
        }
    }

    /// V2.2: Interpret composite score for LLM context
    ///
    /// Provides human-readable assessment of drilling efficiency:
    /// - > 0.75: EXCELLENT drilling conditions (fast and stable)
    /// - > 0.60: GOOD efficiency
    /// - > 0.45: ACCEPTABLE
    /// - <= 0.45: POOR efficiency - optimization needed
    pub fn interpret_composite_score(score: f64) -> &'static str {
        match score {
            s if s > 0.75 => "EXCELLENT (fast and stable)",
            s if s > 0.60 => "GOOD efficiency",
            s if s > 0.45 => "ACCEPTABLE",
            _ => "POOR - optimization needed",
        }
    }

    /// V2.2: Interpret stability score
    pub fn interpret_stability_score(score: f64) -> &'static str {
        match score {
            s if s > 0.85 => "Very stable operation",
            s if s > 0.70 => "Stable with minor variations",
            s if s > 0.50 => "Moderate stability - monitor closely",
            _ => "Unstable - recommend parameter adjustment",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AnomalyCategory, Operation, RigState};
    use std::sync::Arc;

    fn make_packet(wob: f64, rpm: f64, flow: f64, rop: f64, mse: f64, torque: f64) -> WitsPacket {
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
            spp: 3000.0,
            pump_spm: 60.0,
            flow_in: flow,
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
            mse,
            d_exponent: 1.5,
            dxc: 1.4,
            rop_delta: 0.0,
            torque_delta_percent: 0.0,
            spp_delta: 0.0,
            rig_state: RigState::Drilling,
            waveform_snapshot: Arc::new(Vec::new()),
        }
    }

    fn make_metric(mse: f64, mse_efficiency: f64) -> DrillingMetrics {
        DrillingMetrics {
            state: RigState::Drilling,
            operation: Operation::ProductionDrilling,
            mse,
            mse_efficiency,
            d_exponent: 1.5,
            dxc: 1.4,
            mse_delta_percent: 0.0,
            flow_balance: 0.0,
            pit_rate: 0.0,
            ecd_margin: 1.0,
            torque_delta_percent: 0.02,
            spp_delta: 10.0,
            is_anomaly: false,
            anomaly_category: AnomalyCategory::None,
            anomaly_description: None,
        }
    }

    #[test]
    fn test_insufficient_samples_rejected() {
        let packets: Vec<_> = (0..100)
            .map(|_| make_packet(20.0, 100.0, 500.0, 50.0, 20000.0, 10.0))
            .collect();
        let metrics: Vec<_> = (0..100).map(|_| make_metric(20000.0, 75.0)).collect();
        let packet_refs: Vec<_> = packets.iter().collect();
        let metric_refs: Vec<_> = metrics.iter().collect();

        let result = OptimalFinder::find_optimal(&packet_refs, &metric_refs, Campaign::Production, false);
        assert!(result.is_none());
    }

    #[test]
    fn test_binned_optimization() {
        // Create data with two distinct operating modes
        let mut packets = Vec::new();
        let mut metrics = Vec::new();

        // Mode 1: Low WOB, moderate ROP, stable
        for i in 0..200 {
            packets.push(make_packet(
                15.0 + (i % 5) as f64,
                90.0 + (i % 10) as f64,
                480.0,
                40.0 + (i % 5) as f64,
                22000.0,
                8.0,
            ));
            metrics.push(make_metric(22000.0, 70.0));
        }

        // Mode 2: High WOB, high ROP, but less stable
        for i in 0..200 {
            packets.push(make_packet(
                28.0 + (i % 5) as f64,
                130.0 + (i % 10) as f64,
                550.0,
                70.0 + (i % 10) as f64,
                18000.0,
                15.0,
            ));
            metrics.push(make_metric(18000.0, 85.0));
        }

        let packet_refs: Vec<_> = packets.iter().collect();
        let metric_refs: Vec<_> = metrics.iter().collect();

        let result = OptimalFinder::find_optimal(&packet_refs, &metric_refs, Campaign::Production, true);
        assert!(result.is_some());

        let optimal = result.unwrap();

        // Should return ranges, not just point estimates
        assert!(optimal.wob_min < optimal.wob_max);
        assert!(optimal.rpm_min < optimal.rpm_max);
        assert!(optimal.bins_evaluated > 0);
        assert!(optimal.stability_score > 0.0);
        assert!(optimal.dysfunction_filtered);
    }

    #[test]
    fn test_stability_included_in_score() {
        // Create stable data
        let stable_packets: Vec<_> = (0..400)
            .map(|i| make_packet(20.0 + (i % 5) as f64, 100.0, 500.0, 50.0, 20000.0, 10.0))
            .collect();
        let stable_metrics: Vec<_> = (0..400).map(|_| make_metric(20000.0, 80.0)).collect();

        let packet_refs: Vec<_> = stable_packets.iter().collect();
        let metric_refs: Vec<_> = stable_metrics.iter().collect();

        let result = OptimalFinder::find_optimal(&packet_refs, &metric_refs, Campaign::Production, false);
        assert!(result.is_some());

        let optimal = result.unwrap();
        assert!(
            optimal.stability_score > 0.7,
            "Stable data should have high stability score: {}",
            optimal.stability_score
        );
    }

    #[test]
    fn test_pa_campaign_weights_stability_more() {
        let packets: Vec<_> = (0..400)
            .map(|i| make_packet(20.0 + (i % 10) as f64, 100.0 + (i % 20) as f64, 500.0, 50.0, 20000.0, 10.0))
            .collect();
        let metrics: Vec<_> = (0..400).map(|_| make_metric(20000.0, 75.0)).collect();

        let packet_refs: Vec<_> = packets.iter().collect();
        let metric_refs: Vec<_> = metrics.iter().collect();

        let prod_result =
            OptimalFinder::find_optimal(&packet_refs, &metric_refs, Campaign::Production, false);
        let pa_result =
            OptimalFinder::find_optimal(&packet_refs, &metric_refs, Campaign::PlugAbandonment, false);

        assert!(prod_result.is_some());
        assert!(pa_result.is_some());

        // P&A should weight stability more heavily
        let (_, _, prod_stability_w) = get_weights(Campaign::Production);
        let (_, _, pa_stability_w) = get_weights(Campaign::PlugAbandonment);
        assert!(pa_stability_w > prod_stability_w);
    }

    #[test]
    fn test_ranges_calculated_correctly() {
        let packets: Vec<_> = (0..400)
            .map(|i| {
                make_packet(
                    15.0 + (i as f64 * 0.05), // WOB from 15 to 35
                    90.0 + (i as f64 * 0.1),  // RPM from 90 to 130
                    500.0,
                    50.0,
                    20000.0,
                    10.0,
                )
            })
            .collect();
        let metrics: Vec<_> = (0..400).map(|_| make_metric(20000.0, 75.0)).collect();

        let packet_refs: Vec<_> = packets.iter().collect();
        let metric_refs: Vec<_> = metrics.iter().collect();

        let result = OptimalFinder::find_optimal(&packet_refs, &metric_refs, Campaign::Production, false);
        assert!(result.is_some());

        let optimal = result.unwrap();

        // Ranges should be within the winning bin, not the full data range
        assert!(optimal.wob_min >= 15.0);
        assert!(optimal.wob_max <= 35.0);
        assert!(optimal.wob_min < optimal.best_wob);
        assert!(optimal.best_wob < optimal.wob_max);
    }
}
