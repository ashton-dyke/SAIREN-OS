//! Statistical metrics for vibration and drilling analysis

use crate::types::RigState;

/// Calculate excess kurtosis (Fisher's definition) of a signal
///
/// Excess kurtosis measures the "tailedness" relative to a normal distribution.
/// Formula: (μ₄ / σ⁴) - 3
///
/// ## Expected Values
/// - Gaussian noise: ~0.0 (normal distribution baseline)
/// - Uniform distribution: ~-1.2 (platykurtic, flatter than normal)
/// - Bearing impacts (impulsive): >3.0 (leptokurtic, heavy tails)
/// - Severe faults: >6.0 (highly impulsive)
///
/// ## Input
/// - `signal`: Should be a waveform snapshot (e.g., 1024 samples at 10 kHz)
///   for statistically meaningful results. N < 30 gives unreliable estimates.
///
/// ## Returns
/// Excess kurtosis value. Returns 0.0 if signal length < 4 or variance ≈ 0.
pub fn kurtosis(signal: &[f64]) -> f64 {
    if signal.len() < 4 {
        return 0.0;
    }

    let n = signal.len() as f64;

    // Calculate mean
    let mean = signal.iter().sum::<f64>() / n;

    // Calculate variance (2nd central moment)
    let variance = signal.iter()
        .map(|x| (x - mean).powi(2))
        .sum::<f64>() / n;

    if variance < 1e-10 {
        return 0.0; // Avoid division by zero
    }

    let std_dev = variance.sqrt();

    // Calculate 4th central moment
    let fourth_moment = signal.iter()
        .map(|x| (x - mean).powi(4))
        .sum::<f64>() / n;

    // Kurtosis = 4th moment / variance^2
    // Excess kurtosis subtracts 3 (normal distribution has kurtosis of 3)
    (fourth_moment / std_dev.powi(4)) - 3.0
}

/// Calculate shock factor based on hookload and rig state
///
/// Returns a multiplier indicating shock severity:
/// - Drilling/Reaming: 1.2 (moderate continuous load)
/// - Other: 1.0 (baseline)
pub fn shock_factor(hookload: f64, state: &RigState) -> f64 {
    let _ = hookload; // Reserved for future load-based adjustments

    match state {
        RigState::Drilling | RigState::Reaming => 1.2,
        RigState::Circulating => 1.0,
        RigState::Connection => 1.0,
        RigState::TrippingIn | RigState::TrippingOut => 1.0,
        RigState::Idle => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kurtosis_uniform_distribution() {
        // Uniform distribution should have negative excess kurtosis (~-1.2)
        let signal: Vec<f64> = (0..1000).map(|x| x as f64).collect();
        let k = kurtosis(&signal);
        assert!(k < 0.0, "Uniform signal should have negative excess kurtosis");
        assert!(k > -1.5 && k < -1.0, "Uniform excess kurtosis should be around -1.2, got {}", k);
    }

    #[test]
    fn test_kurtosis_gaussian_approximation() {
        // Approximate Gaussian using Box-Muller-like distribution
        // For a proper Gaussian, excess kurtosis should be ~0.0
        // We use a simple sinusoidal sum to approximate
        let signal: Vec<f64> = (0..1024)
            .map(|i| {
                let t = i as f64 / 1024.0;
                // Sum of sines approximates Gaussian-ish distribution
                (t * 2.0 * std::f64::consts::PI).sin()
                    + (t * 5.0 * std::f64::consts::PI).sin() * 0.5
                    + (t * 11.0 * std::f64::consts::PI).sin() * 0.25
            })
            .collect();
        let k = kurtosis(&signal);
        // Sinusoidal sum gives roughly normal-ish kurtosis (within -1 to 1)
        assert!(k.abs() < 2.0, "Sinusoidal mix should have low excess kurtosis, got {}", k);
    }

    #[test]
    fn test_kurtosis_impulse() {
        // Signal with outliers should have high kurtosis (leptokurtic)
        let mut signal: Vec<f64> = vec![0.0; 1024];
        signal[50] = 100.0; // Single impulse
        let k = kurtosis(&signal);
        assert!(k > 10.0, "Impulse signal should have high kurtosis, got {}", k);
    }

    #[test]
    fn test_kurtosis_multiple_impacts() {
        // Simulate bearing fault: multiple impacts at regular intervals
        let mut signal: Vec<f64> = vec![0.015; 1024]; // Baseline noise level
        // Add 10 impacts (simulating ~100 Hz BPFO in 100ms window)
        for i in 0..10 {
            let pos = 50 + i * 100;
            if pos < 1024 {
                signal[pos] = 0.5; // Impact amplitude
            }
        }
        let k = kurtosis(&signal);
        assert!(k > 3.0, "Multiple impacts should have kurtosis > 3.0, got {}", k);
    }

    #[test]
    fn test_shock_factor_states() {
        assert_eq!(shock_factor(1000.0, &RigState::Drilling), 1.2);
        assert_eq!(shock_factor(1000.0, &RigState::Reaming), 1.2);
        assert_eq!(shock_factor(1000.0, &RigState::Idle), 1.0);
        assert_eq!(shock_factor(1000.0, &RigState::Circulating), 1.0);
    }
}
