//! Rolling-window tracker for significant WOB/RPM parameter changes.
//!
//! Used to stamp each `WitsPacket` with `seconds_since_param_change` so that
//! post-well analysis can distinguish sustained-state samples from transient noise
//! caused by drillstring elasticity and mud compressibility (60-120s delay).

use std::collections::VecDeque;

const WINDOW_SECS: u64 = 30;
const WOB_THRESHOLD_KLBS: f64 = 2.0;
const RPM_THRESHOLD: f64 = 10.0;

/// Tracks a rolling 30-second window of WOB/RPM to detect significant changes.
pub struct ParamChangeTracker {
    window: VecDeque<(u64, f64, f64)>, // (timestamp, wob, rpm)
    last_change_ts: u64,
}

impl ParamChangeTracker {
    pub fn new() -> Self {
        Self {
            window: VecDeque::new(),
            last_change_ts: 0,
        }
    }

    /// Update with a new sample and return seconds since the last significant change.
    ///
    /// A "significant change" is when the current WOB or RPM deviates from the
    /// rolling 30-second average by more than the threshold.
    pub fn update(&mut self, timestamp: u64, wob: f64, rpm: f64) -> u64 {
        // Trim entries older than the window
        let cutoff = timestamp.saturating_sub(WINDOW_SECS);
        while self.window.front().map_or(false, |&(ts, _, _)| ts < cutoff) {
            self.window.pop_front();
        }

        // Compute rolling averages from the window
        if !self.window.is_empty() {
            let n = self.window.len() as f64;
            let avg_wob: f64 = self.window.iter().map(|&(_, w, _)| w).sum::<f64>() / n;
            let avg_rpm: f64 = self.window.iter().map(|&(_, _, r)| r).sum::<f64>() / n;

            if (wob - avg_wob).abs() > WOB_THRESHOLD_KLBS
                || (rpm - avg_rpm).abs() > RPM_THRESHOLD
            {
                self.last_change_ts = timestamp;
            }
        } else if self.last_change_ts == 0 {
            // Very first sample — treat as a change
            self.last_change_ts = timestamp;
        }

        self.window.push_back((timestamp, wob, rpm));
        timestamp.saturating_sub(self.last_change_ts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_params_grow_monotonically() {
        let mut tracker = ParamChangeTracker::new();
        // First sample is always a "change"
        assert_eq!(tracker.update(100, 20.0, 100.0), 0);
        // Subsequent stable samples grow
        assert_eq!(tracker.update(101, 20.0, 100.0), 1);
        assert_eq!(tracker.update(110, 20.0, 100.0), 10);
        assert_eq!(tracker.update(200, 20.0, 100.0), 100);
        assert_eq!(tracker.update(300, 20.0, 100.0), 200);
    }

    #[test]
    fn wob_spike_resets_counter() {
        let mut tracker = ParamChangeTracker::new();
        // Build up stable history
        for t in 100..140 {
            tracker.update(t, 20.0, 100.0);
        }
        let before = tracker.update(140, 20.0, 100.0);
        assert_eq!(before, 40);

        // WOB spike of +5 klbs (exceeds 2.0 threshold)
        let after = tracker.update(141, 25.0, 100.0);
        assert_eq!(after, 0);

        // Window still contains old 20.0 values, so 25.0 keeps triggering change
        // Counter only grows once old samples fall out of the 30s window
        assert_eq!(tracker.update(142, 25.0, 100.0), 0);

        // After the window flushes (30s), new stable setpoint is established
        for t in 143..175 {
            tracker.update(t, 25.0, 100.0);
        }
        // Now all old samples are out of window, counter starts growing
        let stable = tracker.update(175, 25.0, 100.0);
        assert!(stable > 0, "counter should grow after window flushes");
    }

    #[test]
    fn rpm_spike_resets_counter() {
        let mut tracker = ParamChangeTracker::new();
        for t in 100..140 {
            tracker.update(t, 20.0, 100.0);
        }
        assert_eq!(tracker.update(140, 20.0, 100.0), 40);

        // RPM spike of +15 (exceeds 10.0 threshold)
        let after = tracker.update(141, 20.0, 115.0);
        assert_eq!(after, 0);

        // Window still has old 100.0 RPM values — keeps triggering
        assert_eq!(tracker.update(142, 20.0, 115.0), 0);
    }

    #[test]
    fn sub_threshold_changes_dont_reset() {
        let mut tracker = ParamChangeTracker::new();
        for t in 100..140 {
            tracker.update(t, 20.0, 100.0);
        }
        let before = tracker.update(140, 20.0, 100.0);
        assert_eq!(before, 40);

        // Small WOB change (+1.5, under 2.0 threshold)
        let after = tracker.update(141, 21.5, 100.0);
        assert_eq!(after, 41);

        // Small RPM change (+8, under 10.0 threshold)
        let after = tracker.update(142, 21.5, 108.0);
        assert_eq!(after, 42);
    }
}
