//! Bit Wear Tracking
//!
//! Monitors bit degradation by tracking normalized MSE over footage intervals.
//! As the bit wears, more energy is required to cut the same rock, causing
//! normalized MSE to trend upward relative to the reference value at the
//! start of the bit run.
//!
//! ## Algorithm
//!
//! 1. Accumulate drilling data in 100-ft depth buckets
//! 2. Normalize MSE against estimated optimal MSE for formation hardness
//! 3. Compare current normalized MSE against the reference (first bucket)
//! 4. Wear index = (current_normalized - reference_normalized) / reference_normalized

use serde::Serialize;
use std::collections::VecDeque;

/// Depth interval per footage bucket (ft)
const BUCKET_SIZE_FT: f64 = 100.0;

/// Maximum number of buckets retained
const MAX_BUCKETS: usize = 20;

/// A finalized footage bucket
#[derive(Debug, Clone, Serialize)]
pub struct FootageBucket {
    pub depth_start_ft: f64,
    pub depth_end_ft: f64,
    pub avg_mse: f64,
    /// Normalized MSE: avg_mse / estimate_optimal_mse(hardness)
    pub normalized_mse: f64,
    pub sample_count: usize,
}

/// Accumulating bucket (not yet finalized)
#[derive(Clone)]
struct AccumulatingBucket {
    depth_start: f64,
    mse_sum: f64,
    wob_sum: f64,
    rpm_sum: f64,
    hardness: f64,
    count: usize,
}

impl AccumulatingBucket {
    fn new(depth_start: f64, hardness: f64) -> Self {
        Self {
            depth_start,
            mse_sum: 0.0,
            wob_sum: 0.0,
            rpm_sum: 0.0,
            hardness,
            count: 0,
        }
    }

    fn add(&mut self, mse: f64, wob: f64, rpm: f64) {
        self.mse_sum += mse;
        self.wob_sum += wob;
        self.rpm_sum += rpm;
        self.count += 1;
    }

    fn finalize(self, depth_end: f64) -> FootageBucket {
        let avg_mse = if self.count > 0 {
            self.mse_sum / self.count as f64
        } else {
            0.0
        };
        let optimal = estimate_optimal_mse(self.hardness);
        let normalized = if optimal > 0.0 {
            avg_mse / optimal
        } else {
            1.0
        };

        FootageBucket {
            depth_start_ft: self.depth_start,
            depth_end_ft: depth_end,
            avg_mse,
            normalized_mse: normalized,
            sample_count: self.count,
        }
    }
}

/// Estimate optimal MSE for a given formation hardness (0-10 scale).
///
/// Uses the physics engine formula: base + hardness * multiplier.
fn estimate_optimal_mse(hardness: f64) -> f64 {
    if crate::config::is_initialized() {
        let cfg = crate::config::get();
        cfg.physics.formation_hardness_base_psi
            + hardness * cfg.physics.formation_hardness_multiplier
    } else {
        // Fallback defaults
        5000.0 + hardness * 3000.0
    }
}

/// Tracks bit wear via normalized MSE over 100-ft depth intervals.
#[derive(Clone)]
pub struct BitWearTracker {
    buckets: VecDeque<FootageBucket>,
    reference_normalized_mse: Option<f64>,
    /// Current wear index (0.0 = fresh, 1.0 = doubled MSE from reference)
    pub current_wear_index: f64,
    last_depth: f64,
    current_bucket: Option<AccumulatingBucket>,
}

impl Default for BitWearTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for BitWearTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BitWearTracker")
            .field("wear_index", &self.current_wear_index)
            .field("buckets", &self.buckets.len())
            .field("last_depth", &self.last_depth)
            .finish()
    }
}

impl BitWearTracker {
    pub fn new() -> Self {
        Self {
            buckets: VecDeque::with_capacity(MAX_BUCKETS),
            reference_normalized_mse: None,
            current_wear_index: 0.0,
            last_depth: 0.0,
            current_bucket: None,
        }
    }

    /// Update tracker with a new drilling sample.
    ///
    /// Only call during active drilling (Drilling/Reaming rig state).
    /// `hardness` is the current formation hardness (0-10), use 5.0 as default.
    pub fn update(&mut self, depth: f64, mse: f64, wob: f64, rpm: f64, hardness: f64) {
        // Ignore non-positive MSE
        if mse <= 0.0 || depth <= 0.0 {
            return;
        }

        // Initialize or check if we need a new bucket
        let bucket = self.current_bucket.get_or_insert_with(|| {
            AccumulatingBucket::new(depth, hardness)
        });

        bucket.add(mse, wob, rpm);

        // Check if bucket should be finalized (100 ft drilled since bucket start)
        let footage = depth - bucket.depth_start;
        if footage >= BUCKET_SIZE_FT {
            // Safety: current_bucket is guaranteed Some here (we just borrowed it above)
            let Some(bucket) = self.current_bucket.take() else { return; };
            let finalized = bucket.finalize(depth);

            // Set reference from first bucket
            if self.reference_normalized_mse.is_none() {
                self.reference_normalized_mse = Some(finalized.normalized_mse);
            }

            // Update wear index
            if let Some(ref_mse) = self.reference_normalized_mse {
                if ref_mse > 0.0 {
                    self.current_wear_index =
                        ((finalized.normalized_mse - ref_mse) / ref_mse).max(0.0);
                }
            }

            // Store bucket
            if self.buckets.len() >= MAX_BUCKETS {
                self.buckets.pop_front();
            }
            self.buckets.push_back(finalized);
        }

        self.last_depth = depth;
    }

    /// Get current wear index (0.0 = fresh bit, >0.5 = consider POOH)
    pub fn wear_index(&self) -> f64 {
        self.current_wear_index
    }

    /// Get wear advisory string based on current wear index
    pub fn wear_advisory(&self) -> Option<&'static str> {
        if self.current_wear_index > 0.80 {
            Some("Bit dulled — POOH recommended")
        } else if self.current_wear_index > 0.50 {
            Some("Consider POOH for bit inspection")
        } else if self.current_wear_index > 0.30 {
            Some("Monitor bit — wear increasing")
        } else {
            None
        }
    }

    /// Get finalized footage buckets
    pub fn buckets(&self) -> &VecDeque<FootageBucket> {
        &self.buckets
    }

    /// Notify the tracker of a formation change.
    ///
    /// Resets the current accumulating bucket so that the new formation's
    /// hardness is used for normalization. Preserves `reference_normalized_mse`
    /// (bit-run-level metric) and all finalized buckets.
    pub fn notify_formation_change(&mut self) {
        self.current_bucket = None;
    }

    /// Reset tracker (e.g., after bit trip / POOH)
    pub fn reset(&mut self) {
        self.buckets.clear();
        self.reference_normalized_mse = None;
        self.current_wear_index = 0.0;
        self.last_depth = 0.0;
        self.current_bucket = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_mse_zero_wear() {
        let mut tracker = BitWearTracker::new();

        // Drill 500 ft at constant MSE
        for i in 0..500 {
            let depth = 5000.0 + i as f64;
            tracker.update(depth, 20000.0, 20.0, 120.0, 5.0);
        }

        // Should have ~4 completed buckets (500 ft / 100 ft = 5, but last may not be done)
        assert!(tracker.buckets().len() >= 4);
        // Wear index should be near zero (constant MSE)
        assert!(
            tracker.wear_index() < 0.05,
            "Constant MSE should produce near-zero wear: {}",
            tracker.wear_index()
        );
    }

    #[test]
    fn test_doubling_mse_high_wear() {
        let mut tracker = BitWearTracker::new();

        // Drill 500 ft with MSE doubling over the interval
        for i in 0..500 {
            let depth = 5000.0 + i as f64;
            let progress = i as f64 / 500.0;
            let mse = 20000.0 * (1.0 + progress); // 20000 → 40000
            tracker.update(depth, mse, 20.0, 120.0, 5.0);
        }

        // Wear index should be significant
        assert!(
            tracker.wear_index() > 0.3,
            "Doubling MSE should produce high wear: {}",
            tracker.wear_index()
        );
    }

    #[test]
    fn test_notify_formation_change_resets_current_bucket() {
        let mut tracker = BitWearTracker::new();

        // Start accumulating in a bucket
        for i in 0..50 {
            tracker.update(5000.0 + i as f64, 20000.0, 20.0, 120.0, 5.0);
        }
        assert!(tracker.current_bucket.is_some(), "Should have active bucket");

        tracker.notify_formation_change();
        assert!(
            tracker.current_bucket.is_none(),
            "Formation change should reset current bucket"
        );
    }

    #[test]
    fn test_notify_formation_change_preserves_reference_mse() {
        let mut tracker = BitWearTracker::new();

        // Drill enough for at least one completed bucket
        for i in 0..150 {
            tracker.update(5000.0 + i as f64, 20000.0, 20.0, 120.0, 5.0);
        }
        let ref_before = tracker.reference_normalized_mse;
        let buckets_before = tracker.buckets().len();
        assert!(ref_before.is_some(), "Should have reference MSE");

        tracker.notify_formation_change();
        assert_eq!(
            tracker.reference_normalized_mse, ref_before,
            "Reference MSE should be preserved across formation change"
        );
        assert_eq!(
            tracker.buckets().len(),
            buckets_before,
            "Finalized buckets should be preserved"
        );
    }

    #[test]
    fn test_reset_clears_state() {
        let mut tracker = BitWearTracker::new();

        for i in 0..200 {
            tracker.update(5000.0 + i as f64, 20000.0, 20.0, 120.0, 5.0);
        }
        assert!(!tracker.buckets().is_empty());

        tracker.reset();
        assert!(tracker.buckets().is_empty());
        assert_eq!(tracker.wear_index(), 0.0);
    }
}
