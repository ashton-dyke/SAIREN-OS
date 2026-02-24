//! Auto-Detection — Infer WellConfig values from live WITS data
//!
//! Observes the first N WITS packets to auto-detect configuration values
//! that the operator didn't explicitly set. Uses robust statistics (median,
//! coefficient of variation) to ensure stable detection.
//!
//! ## Detected Parameters
//!
//! - `normal_mud_weight_ppg`: Median of `mud_weight_in` from first 30 packets
//!
//! ## Usage
//!
//! ```ignore
//! let mut detector = AutoDetector::new();
//! for packet in first_30_packets {
//!     detector.observe(&packet);
//! }
//! if detector.ready() {
//!     let detected = detector.detect();
//!     // Apply detected.normal_mud_weight_ppg if not user-set...
//! }
//! ```

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::types::WitsPacket;

/// Minimum samples before auto-detection is attempted.
const DEFAULT_MIN_SAMPLES: usize = 30;

/// Maximum coefficient of variation for a signal to be considered stable.
/// 0.15 = 15% — if the signal varies more than this, we don't trust it.
const DEFAULT_CONFIDENCE_CV: f64 = 0.15;

/// Observes WITS packets to auto-detect configuration values.
pub struct AutoDetector {
    mud_weight_samples: Vec<f64>,
    min_samples: usize,
    confidence_cv: f64,
}

/// Values auto-detected from WITS stream observation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AutoDetectedValues {
    /// Normal mud weight in ppg, detected from `mud_weight_in` (WITS 0124).
    pub normal_mud_weight_ppg: Option<f64>,
}

impl AutoDetector {
    /// Create a new auto-detector with default thresholds.
    pub fn new() -> Self {
        Self {
            mud_weight_samples: Vec::new(),
            min_samples: DEFAULT_MIN_SAMPLES,
            confidence_cv: DEFAULT_CONFIDENCE_CV,
        }
    }

    /// Observe a single WITS packet, collecting samples for auto-detection.
    pub fn observe(&mut self, packet: &WitsPacket) {
        // Mud weight: skip zeros and non-finite values
        let mw = packet.mud_weight_in;
        if mw > 0.0 && mw.is_finite() {
            self.mud_weight_samples.push(mw);
        }
    }

    /// Check whether enough samples have been collected to attempt detection.
    pub fn ready(&self) -> bool {
        self.mud_weight_samples.len() >= self.min_samples
    }

    /// Number of packets observed so far.
    pub fn sample_count(&self) -> usize {
        self.mud_weight_samples.len()
    }

    /// Attempt to detect values from collected samples.
    ///
    /// Returns `AutoDetectedValues` with `Some(value)` for each parameter
    /// that was detected with sufficient confidence, `None` otherwise.
    pub fn detect(&self) -> AutoDetectedValues {
        let mut result = AutoDetectedValues::default();

        // --- Mud Weight Detection ---
        if self.mud_weight_samples.len() >= self.min_samples {
            let median = Self::median(&self.mud_weight_samples);
            let cv = Self::coefficient_of_variation(&self.mud_weight_samples);

            if cv <= self.confidence_cv {
                // Physical range check: 5–25 ppg
                if (5.0..=25.0).contains(&median) {
                    info!(
                        median = format!("{:.2}", median),
                        cv = format!("{:.4}", cv),
                        samples = self.mud_weight_samples.len(),
                        "Auto-detect: mud weight signal is stable"
                    );
                    result.normal_mud_weight_ppg = Some(median);
                } else {
                    warn!(
                        median = format!("{:.2}", median),
                        "Auto-detect: mud weight median outside physical range (5-25 ppg), skipping"
                    );
                }
            } else {
                warn!(
                    cv = format!("{:.4}", cv),
                    threshold = format!("{:.4}", self.confidence_cv),
                    samples = self.mud_weight_samples.len(),
                    "Auto-detect: mud weight signal unstable (high CV), skipping"
                );
            }
        }

        result
    }

    /// Compute the median of a slice (non-destructive — clones and sorts).
    fn median(values: &[f64]) -> f64 {
        if values.is_empty() {
            return 0.0;
        }
        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mid = sorted.len() / 2;
        if sorted.len() % 2 == 0 {
            (sorted[mid - 1] + sorted[mid]) / 2.0
        } else {
            sorted[mid]
        }
    }

    /// Compute the coefficient of variation (std / mean) of a slice.
    fn coefficient_of_variation(values: &[f64]) -> f64 {
        if values.is_empty() {
            return f64::INFINITY;
        }
        let n = values.len() as f64;
        let mean = values.iter().sum::<f64>() / n;
        if mean.abs() < 1e-10 {
            return f64::INFINITY;
        }
        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);
        variance.sqrt() / mean.abs()
    }
}

impl Default for AutoDetector {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Persistence — cache auto-detected values across restarts
// ============================================================================

const AUTO_DETECTED_PATH: &str = "data/auto_detected.json";

impl AutoDetectedValues {
    /// Persist auto-detected values to disk.
    pub fn save(&self) -> Result<(), std::io::Error> {
        let path = std::path::Path::new(AUTO_DETECTED_PATH);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, json)?;
        info!("Auto-detected values saved to {}", AUTO_DETECTED_PATH);
        Ok(())
    }

    /// Load cached auto-detected values from disk.
    pub fn load_cached() -> Option<Self> {
        let path = std::path::Path::new(AUTO_DETECTED_PATH);
        let json = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&json).ok()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_packet(mud_weight_in: f64) -> WitsPacket {
        WitsPacket {
            mud_weight_in,
            ..Default::default()
        }
    }

    #[test]
    fn test_stable_mud_weight_detected() {
        let mut detector = AutoDetector::new();
        // Feed 30 packets with stable mud weight around 9.5 ppg
        for i in 0..35 {
            let mw = 9.5 + (i as f64 % 3.0) * 0.05; // small variation: 9.5, 9.55, 9.6
            detector.observe(&make_packet(mw));
        }
        assert!(detector.ready());
        let detected = detector.detect();
        let mw = detected.normal_mud_weight_ppg.expect("should detect mud weight");
        assert!((mw - 9.55).abs() < 0.1, "median should be ~9.55, got {}", mw);
    }

    #[test]
    fn test_unstable_mud_weight_not_detected() {
        let mut detector = AutoDetector::new();
        // Feed packets with wildly varying mud weight
        for i in 0..40 {
            let mw = if i % 2 == 0 { 8.0 } else { 16.0 }; // 100% CV
            detector.observe(&make_packet(mw));
        }
        assert!(detector.ready());
        let detected = detector.detect();
        assert!(
            detected.normal_mud_weight_ppg.is_none(),
            "unstable signal should not be detected"
        );
    }

    #[test]
    fn test_zeros_skipped() {
        let mut detector = AutoDetector::new();
        // Feed 20 zeros and 30 valid packets
        for _ in 0..20 {
            detector.observe(&make_packet(0.0));
        }
        for _ in 0..30 {
            detector.observe(&make_packet(10.0));
        }
        assert!(detector.ready());
        let detected = detector.detect();
        let mw = detected.normal_mud_weight_ppg.expect("should detect from valid samples");
        assert!((mw - 10.0).abs() < 0.01);
    }

    #[test]
    fn test_not_ready_without_enough_samples() {
        let mut detector = AutoDetector::new();
        for _ in 0..10 {
            detector.observe(&make_packet(9.0));
        }
        assert!(!detector.ready());
    }

    #[test]
    fn test_median_even_count() {
        let values = vec![1.0, 2.0, 3.0, 4.0];
        assert!((AutoDetector::median(&values) - 2.5).abs() < 0.001);
    }

    #[test]
    fn test_median_odd_count() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert!((AutoDetector::median(&values) - 3.0).abs() < 0.001);
    }

    #[test]
    fn test_coefficient_of_variation_zero_mean() {
        let values = vec![0.0, 0.0, 0.0];
        assert!(AutoDetector::coefficient_of_variation(&values).is_infinite());
    }

    #[test]
    fn test_out_of_range_mud_weight_rejected() {
        let mut detector = AutoDetector::new();
        // Feed 30 packets with physically impossible mud weight
        for _ in 0..35 {
            detector.observe(&make_packet(30.0)); // > 25 ppg
        }
        assert!(detector.ready());
        let detected = detector.detect();
        assert!(
            detected.normal_mud_weight_ppg.is_none(),
            "out-of-range mud weight should not be detected"
        );
    }
}
