//! CfC-based Formation Transition Detector
//!
//! Detects formation transitions by monitoring CfC feature surprise z-scores.
//! When multiple features simultaneously show high surprise without an active
//! advisory ticket, it signals a likely formation change — earlier than the
//! d-exponent 15% shift detector.

use crate::types::FormationTransitionEvent;

/// Minimum z-score for a feature to be considered "surprised".
const SIGMA_THRESHOLD: f64 = 2.0;

/// Minimum number of features above threshold to count as multi-feature surprise.
const MIN_SURPRISED_FEATURES: usize = 3;

/// Number of consecutive packets with multi-feature surprise required to fire.
const MIN_CONSECUTIVE_PACKETS: usize = 5;

/// Detects formation transitions from CfC feature surprise patterns.
///
/// Runs alongside (not replacing) the d-exponent segmenter. When >= 3 features
/// show > 2σ surprise for >= 5 consecutive packets with no active advisory,
/// it emits a `FormationTransitionEvent`.
#[derive(Debug, Clone)]
pub struct FormationTransitionDetector {
    consecutive_count: usize,
    last_surprised_features: Vec<String>,
    packets_seen: u64,
}

impl FormationTransitionDetector {
    pub fn new() -> Self {
        Self {
            consecutive_count: 0,
            last_surprised_features: Vec::new(),
            packets_seen: 0,
        }
    }

    /// Check feature sigmas and potentially emit a formation transition event.
    ///
    /// # Arguments
    /// * `feature_sigmas` - z-scores for all 16 CfC features
    /// * `has_active_advisory` - whether an advisory ticket is currently active
    /// * `timestamp` - current packet timestamp
    /// * `bit_depth` - current bit depth in ft
    ///
    /// # Returns
    /// `Some(FormationTransitionEvent)` when the detector fires, `None` otherwise.
    pub fn check(
        &mut self,
        feature_sigmas: &[(usize, &str, f64)],
        has_active_advisory: bool,
        timestamp: u64,
        bit_depth: f64,
    ) -> Option<FormationTransitionEvent> {
        self.packets_seen += 1;

        // Count features with sigma above threshold
        let surprised: Vec<String> = feature_sigmas
            .iter()
            .filter(|(_, _, sigma)| *sigma > SIGMA_THRESHOLD)
            .map(|(_, name, _)| name.to_string())
            .collect();

        if surprised.len() >= MIN_SURPRISED_FEATURES {
            self.consecutive_count += 1;
            self.last_surprised_features = surprised;
        } else {
            self.consecutive_count = 0;
            self.last_surprised_features.clear();
        }

        // Fire if we've seen enough consecutive packets AND no active advisory
        if self.consecutive_count >= MIN_CONSECUTIVE_PACKETS && !has_active_advisory {
            let event = FormationTransitionEvent {
                timestamp,
                bit_depth,
                surprised_features: self.last_surprised_features.clone(),
                packet_index: self.packets_seen,
            };
            // Reset after firing
            self.consecutive_count = 0;
            self.last_surprised_features.clear();
            Some(event)
        } else {
            None
        }
    }
}

impl Default for FormationTransitionDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sigmas(surprised_count: usize, sigma_val: f64) -> Vec<(usize, &'static str, f64)> {
        let names = [
            "wob", "rop", "rpm", "torque", "mse", "spp", "d_exp", "hookload",
            "ecd", "flow_balance", "pit_rate", "dxc", "pump_spm", "mud_weight",
            "gas_units", "pit_volume",
        ];
        names
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let sigma = if i < surprised_count { sigma_val } else { 0.5 };
                (i, *name, sigma)
            })
            .collect()
    }

    #[test]
    fn test_no_fire_below_threshold() {
        let mut detector = FormationTransitionDetector::new();
        // Only 2 features surprised (below MIN_SURPRISED_FEATURES=3)
        let sigmas = make_sigmas(2, 3.0);
        for _ in 0..10 {
            assert!(detector.check(&sigmas, false, 1000, 5000.0).is_none());
        }
    }

    #[test]
    fn test_no_fire_insufficient_consecutive() {
        let mut detector = FormationTransitionDetector::new();
        let high_sigmas = make_sigmas(5, 3.0);
        let low_sigmas = make_sigmas(0, 0.5);

        // 4 consecutive (just under threshold), then reset
        for _ in 0..4 {
            assert!(detector.check(&high_sigmas, false, 1000, 5000.0).is_none());
        }
        // Reset
        assert!(detector.check(&low_sigmas, false, 1000, 5000.0).is_none());
        // 4 more — still shouldn't fire
        for _ in 0..4 {
            assert!(detector.check(&high_sigmas, false, 1000, 5000.0).is_none());
        }
    }

    #[test]
    fn test_fire_at_exact_threshold() {
        let mut detector = FormationTransitionDetector::new();
        // Exactly 3 features at exactly 2.0σ threshold — sigma must be > 2.0
        let at_threshold = make_sigmas(3, 2.0);
        for _ in 0..10 {
            // sigma == 2.0 is NOT > 2.0, so should not fire
            assert!(detector.check(&at_threshold, false, 1000, 5000.0).is_none());
        }

        // Just above threshold
        let above_threshold = make_sigmas(3, 2.01);
        for i in 0..5 {
            let result = detector.check(&above_threshold, false, 1000 + i, 5000.0);
            if i < 4 {
                assert!(result.is_none());
            } else {
                assert!(result.is_some(), "Should fire on 5th consecutive packet");
            }
        }
    }

    #[test]
    fn test_fire_and_reset() {
        let mut detector = FormationTransitionDetector::new();
        let sigmas = make_sigmas(5, 3.0);

        // Build up to firing
        for _ in 0..4 {
            assert!(detector.check(&sigmas, false, 1000, 5000.0).is_none());
        }
        let event = detector.check(&sigmas, false, 1005, 5100.0);
        assert!(event.is_some());
        let event = event.unwrap();
        assert_eq!(event.timestamp, 1005);
        assert!((event.bit_depth - 5100.0).abs() < 0.01);
        assert_eq!(event.surprised_features.len(), 5);

        // After firing, counter resets — next packet should NOT fire
        assert!(detector.check(&sigmas, false, 1006, 5100.0).is_none());
    }

    #[test]
    fn test_suppression_when_advisory_active() {
        let mut detector = FormationTransitionDetector::new();
        let sigmas = make_sigmas(5, 3.0);

        // Build up consecutive count
        for _ in 0..4 {
            assert!(detector.check(&sigmas, false, 1000, 5000.0).is_none());
        }
        // 5th packet with advisory active — should NOT fire
        assert!(detector.check(&sigmas, true, 1005, 5000.0).is_none());
        // 6th packet without advisory — should also not fire (count was 6, but advisory check is separate)
        // Actually count is 6 now, and has_active_advisory is false
        let result = detector.check(&sigmas, false, 1006, 5000.0);
        // Count is 7, >= 5, and no advisory → fires
        assert!(result.is_some());
    }
}
