//! Online feature normalization using Welford's algorithm.
//!
//! Each of the 16 CfC input features is independently tracked with a running
//! mean and variance, enabling zero-mean unit-variance normalization without
//! needing to store historical data.

/// Number of CfC input features.
pub const NUM_FEATURES: usize = 16;

/// Feature names (matches extraction order in `mod.rs`).
///
/// Features 0-7 are "primary" (2 sensory neurons each).
/// Features 8-15 are "supplementary" (1 sensory neuron each).
pub const FEATURE_NAMES: [&str; NUM_FEATURES] = [
    // Primary (8 features × 2 sensory neurons = 16 neurons)
    "wob", "rop", "rpm", "torque", "mse", "spp",
    "d_exponent", "hookload",
    // Supplementary (8 features × 1 sensory neuron = 8 neurons)
    "ecd", "flow_balance", "pit_rate", "dxc",
    "pump_spm", "mud_weight_in", "gas_units", "pit_volume",
];

/// Online normalizer using Welford's algorithm for numerically stable
/// incremental mean and variance computation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OnlineNormalizer {
    count: u64,
    mean: [f64; NUM_FEATURES],
    m2: [f64; NUM_FEATURES],
}

impl OnlineNormalizer {
    pub fn new() -> Self {
        Self {
            count: 0,
            mean: [0.0; NUM_FEATURES],
            m2: [0.0; NUM_FEATURES],
        }
    }

    /// Update running statistics with a new raw feature vector and return
    /// the normalized (zero-mean, unit-variance) version.
    pub fn normalize_and_update(&mut self, raw: &[f64; NUM_FEATURES]) -> [f64; NUM_FEATURES] {
        self.count += 1;
        let n = self.count as f64;

        let mut normalized = [0.0_f64; NUM_FEATURES];

        for i in 0..NUM_FEATURES {
            let x = raw[i];
            let delta = x - self.mean[i];
            self.mean[i] += delta / n;
            let delta2 = x - self.mean[i];
            self.m2[i] += delta * delta2;

            // Normalize: (x - mean) / std, with a floor on std
            if self.count >= 2 {
                let variance = self.m2[i] / (n - 1.0);
                let std = variance.sqrt().max(1e-8);
                normalized[i] = (x - self.mean[i]) / std;
            }
            // For count < 2, normalized stays 0.0 (no meaningful stats yet)
        }

        normalized
    }

    /// Normalize without updating statistics (for inference-only paths).
    pub fn normalize(&self, raw: &[f64; NUM_FEATURES]) -> [f64; NUM_FEATURES] {
        let mut normalized = [0.0_f64; NUM_FEATURES];
        if self.count < 2 {
            return normalized;
        }
        let n = self.count as f64;
        for i in 0..NUM_FEATURES {
            let variance = self.m2[i] / (n - 1.0);
            let std = variance.sqrt().max(1e-8);
            normalized[i] = (raw[i] - self.mean[i]) / std;
        }
        normalized
    }

    /// Number of samples seen so far.
    pub fn count(&self) -> u64 {
        self.count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_welford_basic() {
        let mut norm = OnlineNormalizer::new();

        // Feed constant values — std should be ~0, normalized should be ~0
        let constant = [1.0; NUM_FEATURES];
        for _ in 0..100 {
            let n = norm.normalize_and_update(&constant);
            for &v in &n {
                assert!(v.abs() < 1e-6, "constant input should normalize to ~0");
            }
        }
        assert_eq!(norm.count(), 100);
    }

    #[test]
    fn test_welford_varying() {
        let mut norm = OnlineNormalizer::new();

        // Feed linearly increasing values
        for i in 0..1000 {
            let mut raw = [0.0; NUM_FEATURES];
            raw[0] = i as f64;
            norm.normalize_and_update(&raw);
        }

        // After 1000 samples, mean should be ~499.5
        let n = norm.count() as f64;
        let expected_mean = (n - 1.0) / 2.0;
        assert!((norm.mean[0] - expected_mean).abs() < 0.1);
    }
}
