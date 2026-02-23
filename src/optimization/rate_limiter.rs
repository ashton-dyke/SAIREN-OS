//! Per-parameter recommendation cooldown tracker

use crate::types::DrillingParameter;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Rate limiter that prevents recommendation spam by enforcing per-parameter cooldowns.
pub struct RateLimiter {
    cooldown: Duration,
    last_recommendation: HashMap<DrillingParameter, (Instant, f64)>,
}

impl RateLimiter {
    /// Create a new rate limiter with the given cooldown in seconds.
    pub fn new(cooldown_secs: u64) -> Self {
        Self {
            cooldown: Duration::from_secs(cooldown_secs),
            last_recommendation: HashMap::new(),
        }
    }

    /// Check if a new recommendation can be issued for this parameter.
    ///
    /// Returns true if:
    /// - No prior recommendation exists for this parameter
    /// - The cooldown has expired
    /// - The new value differs from the last recommendation by >10%
    pub fn can_recommend(&self, param: DrillingParameter, new_value: f64) -> bool {
        match self.last_recommendation.get(&param) {
            None => true,
            Some((last_time, last_value)) => {
                if last_time.elapsed() >= self.cooldown {
                    return true;
                }
                // Allow if value changed >10%
                let change = ((new_value - last_value) / last_value.abs().max(1e-6)).abs();
                change > 0.10
            }
        }
    }

    /// Record that a recommendation was issued for this parameter.
    pub fn record(&mut self, param: DrillingParameter, value: f64) {
        self.last_recommendation.insert(param, (Instant::now(), value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_first_recommendation() {
        let limiter = RateLimiter::new(300);
        assert!(limiter.can_recommend(DrillingParameter::Rpm, 120.0));
    }

    #[test]
    fn suppresses_rapid_re_recommendations() {
        let mut limiter = RateLimiter::new(300);
        limiter.record(DrillingParameter::Rpm, 120.0);
        // Same value within cooldown → suppressed
        assert!(!limiter.can_recommend(DrillingParameter::Rpm, 120.0));
    }

    #[test]
    fn allows_when_value_changes_significantly() {
        let mut limiter = RateLimiter::new(300);
        limiter.record(DrillingParameter::Rpm, 100.0);
        // >10% change → allowed even within cooldown
        assert!(limiter.can_recommend(DrillingParameter::Rpm, 115.0));
    }

    #[test]
    fn allows_different_parameter() {
        let mut limiter = RateLimiter::new(300);
        limiter.record(DrillingParameter::Rpm, 120.0);
        // Different parameter → always allowed
        assert!(limiter.can_recommend(DrillingParameter::Wob, 25.0));
    }

    #[test]
    fn allows_after_cooldown_expires() {
        let mut limiter = RateLimiter::new(0); // 0-second cooldown
        limiter.record(DrillingParameter::Rpm, 120.0);
        // Cooldown of 0s means it's already expired
        assert!(limiter.can_recommend(DrillingParameter::Rpm, 120.0));
    }
}
