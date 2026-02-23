//! Statistical Correlation Engine (V2.1)
//!
//! Calculates Pearson correlations with p-value filtering using the statrs crate.
//! Only returns correlations that meet the statistical significance threshold (p < 0.05).
//!
//! ## Key Features
//! - Pearson correlation coefficient calculation
//! - P-value calculation using Student's t-distribution (statrs)
//! - Automatic filtering of non-significant correlations
//! - Analysis of all relevant drilling parameter pairs

use crate::types::{ml_quality_thresholds::SIGNIFICANCE_THRESHOLD, SignificantCorrelation, WitsPacket};
use statrs::distribution::{ContinuousCDF, StudentsT};

/// Correlation analysis engine with statistical significance testing
pub struct CorrelationEngine;

impl CorrelationEngine {
    /// Calculate Pearson correlation with statistical significance testing
    ///
    /// Uses the statrs crate for accurate p-value calculation via Student's t-distribution.
    ///
    /// # Arguments
    /// * `x` - First variable values
    /// * `y` - Second variable values
    /// * `x_name` - Name of first variable (for reporting)
    /// * `y_name` - Name of second variable (for reporting)
    ///
    /// # Returns
    /// Some(SignificantCorrelation) if p < 0.05, None otherwise
    pub fn calculate(
        x: &[f64],
        y: &[f64],
        x_name: &str,
        y_name: &str,
    ) -> Option<SignificantCorrelation> {
        let n = x.len();
        if n < 30 || n != y.len() {
            // Minimum 30 samples for meaningful correlation
            return None;
        }

        let r = Self::pearson(x, y);
        let p_value = Self::p_value_for_r(r, n);

        // V2: Only return if statistically significant
        if p_value >= SIGNIFICANCE_THRESHOLD {
            return None;
        }

        Some(SignificantCorrelation {
            x_param: x_name.to_string(),
            y_param: y_name.to_string(),
            r_value: r,
            r_squared: r * r,
            p_value,
            sample_count: n,
        })
    }

    /// Calculate Pearson correlation coefficient
    ///
    /// Formula: r = Σ[(xi - x̄)(yi - ȳ)] / sqrt(Σ(xi - x̄)² × Σ(yi - ȳ)²)
    fn pearson(x: &[f64], y: &[f64]) -> f64 {
        let n = x.len() as f64;
        let sum_x: f64 = x.iter().sum();
        let sum_y: f64 = y.iter().sum();
        let sum_xy: f64 = x.iter().zip(y.iter()).map(|(a, b)| a * b).sum();
        let sum_x2: f64 = x.iter().map(|a| a * a).sum();
        let sum_y2: f64 = y.iter().map(|a| a * a).sum();

        let numerator = n * sum_xy - sum_x * sum_y;
        let denominator = ((n * sum_x2 - sum_x.powi(2)) * (n * sum_y2 - sum_y.powi(2))).sqrt();

        if denominator == 0.0 {
            0.0
        } else {
            numerator / denominator
        }
    }

    /// Calculate p-value using statrs StudentsT distribution (V2.1)
    ///
    /// Formula: t = r × sqrt(n-2) / sqrt(1-r²)
    /// Then calculate two-tailed p-value from t-distribution with n-2 degrees of freedom
    fn p_value_for_r(r: f64, n: usize) -> f64 {
        if n < 3 {
            return 1.0;
        }

        // Perfect or near-perfect correlation is highly significant
        if r.abs() >= 0.9999 {
            return 0.0;
        }

        let df = (n - 2) as f64;
        let r_squared = r * r;

        let t_stat = r * df.sqrt() / (1.0 - r_squared).sqrt();

        // Use statrs for accurate t-distribution CDF
        match StudentsT::new(0.0, 1.0, df) {
            Ok(t_dist) => {
                // Two-tailed p-value
                2.0 * (1.0 - t_dist.cdf(t_stat.abs()))
            }
            Err(_) => 1.0, // Fallback if distribution creation fails
        }
    }

    /// Analyze all relevant drilling parameter correlations
    ///
    /// Calculates correlations between:
    /// - WOB vs ROP, MSE
    /// - RPM vs ROP, MSE
    /// - Flow vs ROP
    ///
    /// # Returns
    /// Tuple of (significant correlations, best p-value found)
    /// The best p-value is useful for reporting when no correlations are significant
    pub fn analyze_drilling_correlations(
        packets: &[&WitsPacket],
    ) -> (Vec<SignificantCorrelation>, f64) {
        let wob: Vec<f64> = packets.iter().map(|p| p.wob).collect();
        let rpm: Vec<f64> = packets.iter().map(|p| p.rpm).collect();
        let flow: Vec<f64> = packets.iter().map(|p| p.flow_in).collect();
        let rop: Vec<f64> = packets.iter().map(|p| p.rop).collect();
        let mse: Vec<f64> = packets.iter().map(|p| p.mse).collect();

        let mut correlations = Vec::new();
        let mut best_p: f64 = 1.0;

        // Helper to check and add correlation
        let mut check_correlation = |x: &[f64], y: &[f64], x_name: &str, y_name: &str| {
            if let Some(c) = Self::calculate(x, y, x_name, y_name) {
                best_p = best_p.min(c.p_value);
                correlations.push(c);
            } else {
                // Calculate p-value even for non-significant correlations to track best
                let r = Self::pearson(x, y);
                let p = Self::p_value_for_r(r, x.len());
                best_p = best_p.min(p);
            }
        };

        // WOB correlations
        check_correlation(&wob, &rop, "WOB", "ROP");
        check_correlation(&wob, &mse, "WOB", "MSE");

        // RPM correlations
        check_correlation(&rpm, &rop, "RPM", "ROP");
        check_correlation(&rpm, &mse, "RPM", "MSE");

        // Flow correlations
        check_correlation(&flow, &rop, "Flow", "ROP");

        // Sort by absolute r-value (strongest correlations first)
        correlations.sort_by(|a, b| {
            b.r_value
                .abs()
                .partial_cmp(&a.r_value.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        (correlations, best_p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RigState;
    use std::sync::Arc;

    fn make_packet(wob: f64, rpm: f64, flow: f64, rop: f64, mse: f64) -> WitsPacket {
        WitsPacket {
            timestamp: 1000,
            bit_depth: 5000.0,
            hole_depth: 5000.0,
            rop,
            hook_load: 200.0,
            wob,
            rpm,
            torque: 10.0,
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
            regime_id: 0,
            seconds_since_param_change: 0,        }
    }

    #[test]
    fn test_perfect_positive_correlation() {
        // Perfect positive correlation: y = x
        let x: Vec<f64> = (0..100).map(|i| i as f64).collect();
        let y: Vec<f64> = (0..100).map(|i| i as f64).collect();

        let result = CorrelationEngine::calculate(&x, &y, "X", "Y");
        assert!(result.is_some());

        let corr = result.unwrap();
        assert!((corr.r_value - 1.0).abs() < 0.001);
        assert!(corr.p_value < 0.05);
    }

    #[test]
    fn test_perfect_negative_correlation() {
        // Perfect negative correlation: y = -x
        let x: Vec<f64> = (0..100).map(|i| i as f64).collect();
        let y: Vec<f64> = (0..100).map(|i| 100.0 - i as f64).collect();

        let result = CorrelationEngine::calculate(&x, &y, "X", "Y");
        assert!(result.is_some());

        let corr = result.unwrap();
        assert!((corr.r_value + 1.0).abs() < 0.001);
        assert!(corr.p_value < 0.05);
    }

    #[test]
    fn test_no_correlation_random() {
        use rand::Rng;
        let mut rng = rand::thread_rng();

        // Random data should have low correlation
        let x: Vec<f64> = (0..100).map(|_| rng.gen_range(0.0..100.0)).collect();
        let y: Vec<f64> = (0..100).map(|_| rng.gen_range(0.0..100.0)).collect();

        let r = CorrelationEngine::pearson(&x, &y);
        // Random data typically has |r| < 0.3
        assert!(
            r.abs() < 0.5,
            "Random data should have weak correlation, got r={}",
            r
        );
    }

    #[test]
    fn test_weak_correlation_rejected() {
        // Test with deterministic data that has weak, non-significant correlation
        // Using alternating pattern creates very weak correlation
        let x: Vec<f64> = (0..100).map(|i| i as f64).collect();
        let y: Vec<f64> = (0..100)
            .map(|i| {
                // Add large noise to create weak correlation
                if i % 2 == 0 { 50.0 } else { 51.0 }
            })
            .collect();

        // This should produce a very weak correlation (r close to 0)
        let r = CorrelationEngine::pearson(&x, &y);
        let p = CorrelationEngine::p_value_for_r(r, 100);

        // Weak correlation should not be significant
        assert!(
            r.abs() < 0.1,
            "Test data should produce weak correlation, got r={}",
            r
        );
        assert!(
            p > 0.05,
            "Weak correlation should have p > 0.05, got p={}",
            p
        );

        // Should not pass the significance filter
        let result = CorrelationEngine::calculate(&x, &y, "X", "Y");
        assert!(
            result.is_none(),
            "Weak correlation should be rejected"
        );
    }

    #[test]
    fn test_insufficient_samples_rejected() {
        // Less than 30 samples should be rejected
        let x: Vec<f64> = (0..20).map(|i| i as f64).collect();
        let y: Vec<f64> = (0..20).map(|i| i as f64 * 2.0).collect();

        let result = CorrelationEngine::calculate(&x, &y, "X", "Y");
        assert!(result.is_none());
    }

    #[test]
    fn test_drilling_correlations_analysis() {
        // Create packets with strong WOB-ROP correlation
        let packets: Vec<_> = (0..100)
            .map(|i| {
                let wob = 10.0 + (i as f64 * 0.2);
                let rop = wob * 3.0 + 10.0; // Strong positive correlation
                make_packet(wob, 100.0, 500.0, rop, 20000.0)
            })
            .collect();
        let packet_refs: Vec<_> = packets.iter().collect();

        let (correlations, best_p) = CorrelationEngine::analyze_drilling_correlations(&packet_refs);

        // Should find significant WOB-ROP correlation
        let wob_rop = correlations
            .iter()
            .find(|c| c.x_param == "WOB" && c.y_param == "ROP");
        assert!(wob_rop.is_some(), "Should find WOB-ROP correlation");

        let wob_rop = wob_rop.unwrap();
        assert!(
            wob_rop.r_value > 0.9,
            "WOB-ROP correlation should be strong"
        );
        assert!(wob_rop.p_value < 0.05, "WOB-ROP should be significant");

        assert!(best_p < 0.05, "Best p-value should be significant");
    }

    #[test]
    fn test_p_value_calculation_accuracy() {
        // Known test case: r=0.5, n=30 should give p ≈ 0.005
        let p = CorrelationEngine::p_value_for_r(0.5, 30);
        assert!(
            p < 0.01,
            "r=0.5, n=30 should have p < 0.01, got {}",
            p
        );
        assert!(
            p > 0.001,
            "r=0.5, n=30 should have p > 0.001, got {}",
            p
        );

        // Known test case: r=0.2, n=30 should give p ≈ 0.29
        let p = CorrelationEngine::p_value_for_r(0.2, 30);
        assert!(
            p > 0.2,
            "r=0.2, n=30 should have p > 0.2, got {}",
            p
        );
    }

    #[test]
    fn test_correlations_sorted_by_strength() {
        // Create packets with varying correlation strengths
        let packets: Vec<_> = (0..100)
            .map(|i| {
                let wob = 10.0 + (i as f64 * 0.3);
                let rpm = 80.0 + (i as f64 * 0.1);
                let rop = wob * 2.0 + rpm * 0.5 + 10.0;
                make_packet(wob, rpm, 500.0, rop, 20000.0)
            })
            .collect();
        let packet_refs: Vec<_> = packets.iter().collect();

        let (correlations, _) = CorrelationEngine::analyze_drilling_correlations(&packet_refs);

        // Correlations should be sorted by absolute r-value (strongest first)
        for i in 1..correlations.len() {
            assert!(
                correlations[i - 1].r_value.abs() >= correlations[i].r_value.abs(),
                "Correlations should be sorted by |r|"
            );
        }
    }
}
