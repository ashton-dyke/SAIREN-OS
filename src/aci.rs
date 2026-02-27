//! Adaptive Conformal Inference (ACI)
//!
//! Implements the ACI algorithm from Gibbs & Candès (NeurIPS 2021) to wrap
//! every drilling prediction with calibrated confidence intervals that
//! self-correct under distribution shift.
//!
//! Standard conformal prediction assumes exchangeable data and drops to
//! 81-84% coverage under distribution shift. ACI maintains ~90% coverage
//! even as drilling conditions change mid-operation.
//!
//! ## Core mechanism
//!
//! Single scalar update per prediction:
//!
//! ```text
//! α_{t+1} = α_t + γ(α_target − err_t)
//! ```
//!
//! Where `err_t = 1` if the true value fell outside the interval, `0` otherwise.
//! This dynamically widens intervals when coverage drops and narrows them
//! when the model is well-calibrated.
//!
//! ## Usage
//!
//! ```ignore
//! let mut tracker = AciTracker::new(AciConfig::default());
//!
//! // Each timestep: get interval, then update with actual value
//! let interval = tracker.predict("mse", current_mse);
//! // ... later, or immediately for non-predictive mode:
//! tracker.update("mse", current_mse);
//! ```

/// Configuration for ACI tracker
#[derive(Debug, Clone)]
pub struct AciConfig {
    /// Target coverage probability (e.g. 0.90 = 90%)
    pub target_coverage: f64,
    /// Learning rate γ for alpha adaptation
    pub gamma: f64,
    /// Sliding window size for residual history
    pub window_size: usize,
    /// Minimum samples before producing intervals
    pub min_samples: usize,
}

impl Default for AciConfig {
    fn default() -> Self {
        Self {
            target_coverage: 0.90,
            gamma: 0.005,
            window_size: 200,
            min_samples: 20,
        }
    }
}

/// A calibrated prediction interval
#[derive(Debug, Clone, Copy)]
pub struct ConformalInterval {
    /// Point estimate (current value or prediction)
    pub value: f64,
    /// Lower bound of interval (physically bounded: ≥ 0 for non-negative metrics)
    pub lower: f64,
    /// Upper bound of interval
    pub upper: f64,
    /// Current empirical coverage (0.0-1.0)
    pub coverage: f64,
    /// Whether the current value is outside the interval (anomalous)
    pub is_outlier: bool,
    /// How many sigma-equivalents from the median (for severity grading)
    pub deviation_score: f64,
}

/// Tracks ACI state for a single metric
#[derive(Debug, Clone)]
struct MetricTracker {
    /// Sorted residuals (absolute deviations from running median)
    residuals: Vec<f64>,
    /// Recent values for running median
    values: Vec<f64>,
    /// Adaptive miscoverage rate
    alpha: f64,
    /// Target miscoverage rate (1 - target_coverage)
    alpha_target: f64,
    /// Running hit count for empirical coverage
    hits: u64,
    /// Total predictions made
    total: u64,
    /// Physical lower bound (e.g. 0.0 for non-negative metrics like MSE, ROP)
    floor: Option<f64>,
    /// Minimum interval half-width (prevents zero-width intervals for constant metrics)
    min_half_width: f64,
}

impl MetricTracker {
    fn new(target_coverage: f64) -> Self {
        let alpha_target = 1.0 - target_coverage;
        Self {
            residuals: Vec::new(),
            values: Vec::new(),
            alpha: alpha_target,
            alpha_target,
            hits: 0,
            total: 0,
            floor: None,
            min_half_width: 0.0,
        }
    }

    /// Get the running median of recent values
    fn median(&self) -> f64 {
        if self.values.is_empty() {
            return 0.0;
        }
        let mut sorted = self.values.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mid = sorted.len() / 2;
        if sorted.len() % 2 == 0 {
            (sorted[mid - 1] + sorted[mid]) / 2.0
        } else {
            sorted[mid]
        }
    }

    /// Compute the conformal quantile from residual history
    fn quantile_radius(&self) -> f64 {
        if self.residuals.is_empty() {
            return 0.0;
        }
        let mut sorted = self.residuals.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // Quantile level: 1 - alpha (clamped to valid range)
        let level = (1.0 - self.alpha).clamp(0.01, 0.999);
        let idx = ((sorted.len() as f64) * level).ceil() as usize;
        let idx = idx.min(sorted.len() - 1);
        sorted[idx]
    }

    /// Record a new value, update residuals and alpha
    fn update(&mut self, value: f64, window_size: usize, gamma: f64) {
        let median = self.median();
        let residual = (value - median).abs();

        // Check if value was within the previous interval
        let radius = self.quantile_radius();
        let was_covered = residual <= radius;

        // ACI alpha update: α_{t+1} = α_t + γ(α_target − err_t)
        let err_t = if was_covered { 0.0 } else { 1.0 };
        self.alpha = (self.alpha + gamma * (self.alpha_target - err_t)).clamp(0.001, 0.5);

        // Track empirical coverage
        self.total += 1;
        if was_covered {
            self.hits += 1;
        }

        // Add to windows (bounded)
        self.values.push(value);
        if self.values.len() > window_size {
            self.values.remove(0);
        }

        self.residuals.push(residual);
        if self.residuals.len() > window_size {
            self.residuals.remove(0);
        }
    }

    /// Get the current interval for a value
    fn interval(&self, value: f64) -> ConformalInterval {
        let median = self.median();
        let radius = self.quantile_radius().max(self.min_half_width);
        let deviation = (value - median).abs();
        let is_outlier = deviation > radius && self.residuals.len() >= 10;

        // Deviation score: how far outside the interval (in radius units)
        let deviation_score = if radius > 1e-10 {
            deviation / radius
        } else {
            0.0
        };

        let coverage = if self.total > 0 {
            self.hits as f64 / self.total as f64
        } else {
            0.0
        };

        let mut lower = median - radius;
        let upper = median + radius;

        // Apply physical floor bound
        if let Some(floor) = self.floor {
            if lower < floor {
                lower = floor;
            }
        }

        ConformalInterval {
            value,
            lower,
            upper,
            coverage,
            is_outlier,
            deviation_score,
        }
    }
}

// ============================================================================
// Public API
// ============================================================================

/// ACI tracker managing multiple drilling metrics
#[derive(Debug, Clone)]
pub struct AciTracker {
    config: AciConfig,
    trackers: Vec<(String, MetricTracker)>,
}

/// Metric IDs for the standard drilling metrics tracked by ACI
pub mod metrics {
    pub const MSE: &str = "mse";
    pub const D_EXPONENT: &str = "d_exponent";
    pub const DXC: &str = "dxc";
    pub const FLOW_BALANCE: &str = "flow_balance";
    pub const SPP: &str = "spp";
    pub const TORQUE: &str = "torque";
    pub const ROP: &str = "rop";
    pub const WOB: &str = "wob";
    pub const RPM: &str = "rpm";
    pub const ECD: &str = "ecd";
    pub const PIT_RATE: &str = "pit_rate";
}

impl AciTracker {
    /// Create a new ACI tracker with default metrics
    pub fn new(config: AciConfig) -> Self {
        // Metrics with physical floor = 0.0 (can't be negative)
        let non_negative: &[&str] = &[
            metrics::MSE, metrics::D_EXPONENT, metrics::DXC,
            metrics::SPP, metrics::TORQUE, metrics::ROP,
            metrics::WOB, metrics::RPM, metrics::ECD,
        ];
        // Metrics that can be negative (flow balance = loss, pit rate = loss)
        let allow_negative: &[&str] = &[
            metrics::FLOW_BALANCE, metrics::PIT_RATE,
        ];

        let mut trackers = Vec::new();
        for id in non_negative {
            let mut t = MetricTracker::new(config.target_coverage);
            t.floor = Some(0.0);
            trackers.push((id.to_string(), t));
        }
        for id in allow_negative {
            trackers.push((id.to_string(), MetricTracker::new(config.target_coverage)));
        }

        // Set per-metric minimum half-widths (~1-2% of typical operating range)
        // to prevent zero-width intervals when a metric is held constant.
        let min_half_widths: &[(&str, f64)] = &[
            (metrics::MSE,          5.0),   // ksi² (typical 10-500)
            (metrics::SPP,          10.0),  // psi (typical 500-5000)
            (metrics::TORQUE,       0.5),   // kft·lb (typical 5-50)
            (metrics::ROP,          1.0),   // ft/hr (typical 0-300)
            (metrics::RPM,          2.0),   // rpm (typical 60-200)
            (metrics::WOB,          0.5),   // klbs (typical 5-50)
            (metrics::ECD,          0.02),  // ppg (typical 8-18)
            (metrics::D_EXPONENT,   0.05),  // dimensionless (typical 0-3)
            (metrics::DXC,          0.05),  // dimensionless (typical 0-3)
            (metrics::FLOW_BALANCE, 5.0),   // gpm (typically ±30)
            (metrics::PIT_RATE,     1.0),   // bbl/hr (typically ±10)
        ];
        for &(id, mhw) in min_half_widths {
            if let Some(pos) = trackers.iter().position(|(name, _)| name == id) {
                trackers[pos].1.min_half_width = mhw;
            }
        }

        Self { config, trackers }
    }

    /// Update a metric with a new observed value and return its conformal interval
    pub fn update(&mut self, metric_id: &str, value: f64) -> ConformalInterval {
        let window_size = self.config.window_size;
        let gamma = self.config.gamma;
        let tracker = self.get_or_create(metric_id);
        tracker.update(value, window_size, gamma);
        tracker.interval(value)
    }

    /// Get the current interval for a metric without updating
    pub fn interval(&self, metric_id: &str) -> Option<ConformalInterval> {
        self.trackers
            .iter()
            .find(|(id, _)| id == metric_id)
            .map(|(_, t)| t.interval(t.median()))
    }

    /// Check if a metric value is an outlier relative to its conformal interval
    pub fn is_outlier(&self, metric_id: &str, value: f64) -> bool {
        self.trackers
            .iter()
            .find(|(id, _)| id == metric_id)
            .map(|(_, t)| {
                let interval = t.interval(value);
                interval.is_outlier
            })
            .unwrap_or(false)
    }

    /// Get number of tracked samples for a metric
    pub fn sample_count(&self, metric_id: &str) -> usize {
        self.trackers
            .iter()
            .find(|(id, _)| id == metric_id)
            .map(|(_, t)| t.values.len())
            .unwrap_or(0)
    }

    /// Check if tracker has enough samples for reliable intervals
    pub fn is_calibrated(&self, metric_id: &str) -> bool {
        self.sample_count(metric_id) >= self.config.min_samples
    }

    /// Get a summary of all outliers across all metrics
    pub fn outlier_summary(&self) -> Vec<(&str, ConformalInterval)> {
        self.trackers
            .iter()
            .filter_map(|(id, t)| {
                let interval = t.interval(*t.values.last().unwrap_or(&0.0));
                if interval.is_outlier {
                    Some((id.as_str(), interval))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get or create a tracker for a metric
    fn get_or_create(&mut self, metric_id: &str) -> &mut MetricTracker {
        let pos = self.trackers.iter().position(|(id, _)| id == metric_id);
        match pos {
            Some(idx) => &mut self.trackers[idx].1,
            None => {
                self.trackers.push((
                    metric_id.to_string(),
                    MetricTracker::new(self.config.target_coverage),
                ));
                let last = self.trackers.len() - 1;
                &mut self.trackers[last].1
            }
        }
    }
}

/// Convenience: update all standard drilling metrics from a packet + metrics pair
pub fn update_from_drilling(
    aci: &mut AciTracker,
    packet: &crate::types::WitsPacket,
    drill_metrics: &crate::types::DrillingMetrics,
) -> AciDrillingResult {
    // Physical bounds are built into the tracker (floor=0 for non-negative metrics)
    let mse = aci.update(metrics::MSE, drill_metrics.mse);
    let d_exp = aci.update(metrics::D_EXPONENT, drill_metrics.d_exponent);
    let dxc = aci.update(metrics::DXC, drill_metrics.dxc);
    let flow = aci.update(metrics::FLOW_BALANCE, drill_metrics.flow_balance);
    let spp = aci.update(metrics::SPP, packet.spp);
    let torque = aci.update(metrics::TORQUE, packet.torque);
    let rop = aci.update(metrics::ROP, packet.rop);
    let wob = aci.update(metrics::WOB, packet.wob);
    let rpm = aci.update(metrics::RPM, packet.rpm);
    let ecd = aci.update(metrics::ECD, packet.ecd);
    let pit_rate = aci.update(metrics::PIT_RATE, drill_metrics.pit_rate);

    let outlier_count = [&mse, &d_exp, &dxc, &flow, &spp, &torque, &rop, &wob, &rpm, &ecd, &pit_rate]
        .iter()
        .filter(|i| i.is_outlier)
        .count();

    AciDrillingResult {
        mse,
        d_exponent: d_exp,
        dxc,
        flow_balance: flow,
        spp,
        torque,
        rop,
        wob,
        rpm,
        ecd,
        pit_rate,
        outlier_count,
    }
}

/// Result of ACI analysis across all standard drilling metrics
#[derive(Debug, Clone)]
pub struct AciDrillingResult {
    pub mse: ConformalInterval,
    pub d_exponent: ConformalInterval,
    pub dxc: ConformalInterval,
    pub flow_balance: ConformalInterval,
    pub spp: ConformalInterval,
    pub torque: ConformalInterval,
    pub rop: ConformalInterval,
    pub wob: ConformalInterval,
    pub rpm: ConformalInterval,
    pub ecd: ConformalInterval,
    pub pit_rate: ConformalInterval,
    /// Number of metrics currently outside their conformal interval
    pub outlier_count: usize,
}

impl AciDrillingResult {
    /// Get all metrics that are currently outliers
    pub fn outliers(&self) -> Vec<(&str, &ConformalInterval)> {
        let all = [
            ("MSE", &self.mse),
            ("D-Exp", &self.d_exponent),
            ("DXC", &self.dxc),
            ("Flow", &self.flow_balance),
            ("SPP", &self.spp),
            ("Torque", &self.torque),
            ("ROP", &self.rop),
            ("WOB", &self.wob),
            ("RPM", &self.rpm),
            ("ECD", &self.ecd),
            ("PitRate", &self.pit_rate),
        ];
        all.into_iter().filter(|(_, i)| i.is_outlier).collect()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aci_basic_coverage() {
        let config = AciConfig {
            target_coverage: 0.90,
            gamma: 0.01,
            window_size: 100,
            min_samples: 10,
        };
        let mut tracker = AciTracker::new(config);

        // Feed 50 stable values around 100
        for i in 0..50 {
            let value = 100.0 + (i as f64 * 0.1).sin() * 5.0;
            tracker.update("test", value);
        }

        // Value within range should not be outlier
        let interval = tracker.update("test", 102.0);
        assert!(!interval.is_outlier, "102.0 should be within interval: [{:.1}, {:.1}]",
            interval.lower, interval.upper);

        // Check outlier detection: query interval before updating
        // 500 is far outside the [~95, ~105] range built from 50 samples around 100
        assert!(tracker.is_outlier("test", 500.0),
            "500.0 should be outside interval built from values ~100");

        // After updating with the extreme value, deviation score should be high
        let interval = tracker.update("test", 500.0);
        assert!(interval.deviation_score > 1.0,
            "deviation_score should be > 1.0 for extreme value, got {}", interval.deviation_score);
    }

    #[test]
    fn test_aci_adapts_to_shift() {
        let config = AciConfig {
            target_coverage: 0.90,
            gamma: 0.01,
            window_size: 50,
            min_samples: 10,
        };
        let mut tracker = AciTracker::new(config);

        // Phase 1: stable around 100
        for _ in 0..50 {
            tracker.update("test", 100.0);
        }

        // Phase 2: shift to 200 — initially outliers, then adapts
        let mut outlier_count = 0;
        for _ in 0..50 {
            let interval = tracker.update("test", 200.0);
            if interval.is_outlier {
                outlier_count += 1;
            }
        }

        // Should have adapted — later values at 200 should NOT be outliers
        let final_interval = tracker.update("test", 200.0);
        assert!(!final_interval.is_outlier,
            "After adaptation, 200.0 should be within interval: [{:.1}, {:.1}]",
            final_interval.lower, final_interval.upper);

        // But the first few at 200 should have been outliers
        assert!(outlier_count > 0, "Should have detected initial shift as outliers");
    }

    #[test]
    fn test_aci_coverage_near_target() {
        let config = AciConfig {
            target_coverage: 0.90,
            gamma: 0.005,
            window_size: 200,
            min_samples: 10,
        };
        let mut tracker = AciTracker::new(config);

        // Feed noisy data for a while
        let mut hits = 0u64;
        let total = 500u64;
        for i in 0..total {
            let value = 50.0 + (i as f64 * 0.3).sin() * 10.0;
            let interval = tracker.update("test", value);
            if !interval.is_outlier {
                hits += 1;
            }
        }

        let empirical_coverage = hits as f64 / total as f64;
        // Coverage should be roughly near target (±10% tolerance for finite sample)
        assert!(empirical_coverage > 0.75,
            "Coverage {:.1}% too low (target 90%)", empirical_coverage * 100.0);
    }

    #[test]
    fn test_aci_multi_metric() {
        let mut tracker = AciTracker::new(AciConfig::default());

        // Feed different metrics
        for i in 0..30 {
            tracker.update("mse", 500.0 + i as f64);
            tracker.update("rop", 80.0 + (i as f64 * 0.5).sin() * 5.0);
        }

        assert!(tracker.is_calibrated("mse"));
        assert!(tracker.is_calibrated("rop"));
        assert!(!tracker.is_calibrated("nonexistent"));
    }

    #[test]
    fn test_conformal_interval_fields() {
        let mut tracker = AciTracker::new(AciConfig::default());

        for i in 0..30 {
            tracker.update("test", 100.0 + i as f64 * 0.1);
        }

        let interval = tracker.update("test", 101.0);
        assert!(interval.lower < interval.value);
        assert!(interval.upper > interval.value);
        assert!(interval.coverage > 0.0);
    }
}
