//! Depth-Ahead CfC Network — formation transition forecasting.
//!
//! Wraps a standard `CfcNetwork` (64 neurons, seed=1042) to predict depth-ahead
//! anomalies using 8 formation-relevant features zero-padded into the 16-feature
//! input array. This avoids any changes to the core CfC infrastructure.
//!
//! ## Feature mapping
//!
//! Slots 0-7 map to depth-ahead features (2x training weight via existing
//! FEATURE_WEIGHTS). Slots 8-15 are zero-filled — the network trivially
//! learns to predict zero for those outputs.

use crate::cfc::network::{CfcNetwork, CfcNetworkConfig};
use crate::cfc::normalizer::NUM_FEATURES;
use crate::cfc::training::TrainingConfig;
use crate::cfc::wiring::NcpConfig;
use crate::types::{DrillingMetrics, WitsPacket};

/// Number of real features used by the depth-ahead network.
pub const DA_NUM_FEATURES: usize = 8;

/// Human-readable names for the 8 depth-ahead features.
pub const DA_FEATURE_NAMES: [&str; DA_NUM_FEATURES] = [
    "wob",
    "rop",
    "rpm",
    "torque",
    "mse",
    "d_exponent",
    "depth_into_formation",
    "formation_hardness",
];

/// Result of a depth-ahead CfC processing step.
#[derive(Debug, Clone)]
pub struct DepthAheadResult {
    /// Prediction confidence: 1.0 - anomaly_score (higher = more confident).
    pub confidence: f64,
    /// Raw anomaly score from the CfC network (0.0 = normal, 1.0 = anomalous).
    pub anomaly_score: f64,
    /// Whether the network has processed enough packets to be calibrated.
    pub is_calibrated: bool,
    /// Training loss for this step (None if first packet).
    pub training_loss: Option<f64>,
}

/// Depth-ahead CfC network for formation transition forecasting.
///
/// Uses a single 64-neuron CfcNetwork with moderate training config.
/// Feature slots 0-7 carry real drilling+formation data; slots 8-15 are zero.
#[derive(Debug, Clone)]
pub struct DepthAheadNetwork {
    network: CfcNetwork,
}

impl DepthAheadNetwork {
    /// Create a new depth-ahead network.
    ///
    /// Uses seed 1042 by default for wiring isolation from the dual main networks
    /// (which use seeds 42 and 142).
    pub fn new(seed: u64) -> Self {
        let config = CfcNetworkConfig {
            ncp: NcpConfig::dual_64(),
            training: TrainingConfig {
                bptt_depth: 6,
                bptt_decay: 0.75,
                max_grad_norm: 5.0,
                initial_lr: 0.0005,
                lr_decay: 0.9999,
                lr_floor: 0.00005,
            },
            error_ema_alpha: 0.008,
            calibration_window: 500,
        };

        Self {
            network: CfcNetwork::with_config(seed, config),
        }
    }

    /// Process one timestep and return the depth-ahead result.
    pub fn process(&mut self, features: &[f64; NUM_FEATURES], dt: f64) -> DepthAheadResult {
        let (_predictions, train_loss) = self.network.process(features, dt);

        DepthAheadResult {
            confidence: 1.0 - self.network.anomaly_score(),
            anomaly_score: self.network.anomaly_score(),
            is_calibrated: self.network.is_calibrated(),
            training_loss: train_loss,
        }
    }

    /// Reset hidden state and cache history (e.g., on formation boundary).
    ///
    /// Keeps learned weights — only clears temporal state so the network
    /// doesn't carry prediction context across formation transitions.
    pub fn reset_state(&mut self) {
        self.network.reset_state();
    }

    /// Prediction confidence: 1.0 - anomaly_score.
    pub fn prediction_confidence(&self) -> f64 {
        1.0 - self.network.anomaly_score()
    }

    /// Number of packets processed.
    pub fn packets_processed(&self) -> u64 {
        self.network.packets_processed()
    }

    /// Whether calibrated (enough packets seen).
    pub fn is_calibrated(&self) -> bool {
        self.network.is_calibrated()
    }
}

/// Extract depth-ahead features from a packet, metrics, and formation context.
///
/// Maps 8 real features into positions 0-7 of the 16-element array.
/// Positions 8-15 are zero-filled.
///
/// `formation_ctx` is `(depth_into_formation_ft, formation_hardness)`.
/// If None, those features default to 0.0.
pub fn extract_da_features(
    packet: &WitsPacket,
    metrics: &DrillingMetrics,
    formation_ctx: Option<(f64, f64)>,
) -> [f64; NUM_FEATURES] {
    let (depth_into, hardness) = formation_ctx.unwrap_or((0.0, 0.0));

    let mut features = [0.0f64; NUM_FEATURES];
    features[0] = packet.wob;
    features[1] = packet.rop;
    features[2] = packet.rpm;
    features[3] = packet.torque;
    features[4] = metrics.mse;
    features[5] = metrics.d_exponent;
    features[6] = depth_into;
    features[7] = hardness;
    // 8-15 remain 0.0

    features
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfc::tests::{make_test_metrics, make_test_packet};

    #[test]
    fn test_da_network_creation() {
        let da = DepthAheadNetwork::new(1042);
        assert_eq!(da.packets_processed(), 0);
        assert!(!da.is_calibrated());
        assert!((da.prediction_confidence() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_extract_da_features() {
        let packet = make_test_packet();
        let metrics = make_test_metrics();
        let features = extract_da_features(&packet, &metrics, Some((150.0, 5.0)));

        assert_eq!(features[0], 25.0); // wob
        assert_eq!(features[1], 60.0); // rop
        assert_eq!(features[2], 120.0); // rpm
        assert_eq!(features[3], 15.0); // torque
        assert_eq!(features[4], 30000.0); // mse
        assert_eq!(features[5], 1.5); // d_exponent
        assert_eq!(features[6], 150.0); // depth_into_formation
        assert_eq!(features[7], 5.0); // formation_hardness

        // Slots 8-15 should be zero
        for i in 8..NUM_FEATURES {
            assert_eq!(features[i], 0.0, "Slot {} should be zero", i);
        }
    }

    #[test]
    fn test_extract_da_features_no_formation() {
        let packet = make_test_packet();
        let metrics = make_test_metrics();
        let features = extract_da_features(&packet, &metrics, None);

        assert_eq!(features[6], 0.0);
        assert_eq!(features[7], 0.0);
    }

    #[test]
    fn test_da_process() {
        let mut da = DepthAheadNetwork::new(1042);
        let packet = make_test_packet();
        let metrics = make_test_metrics();
        let features = extract_da_features(&packet, &metrics, Some((100.0, 3.0)));

        let result = da.process(&features, 1.0);
        assert!(!result.is_calibrated);
        assert!(result.training_loss.is_none()); // First packet
        assert_eq!(result.anomaly_score, 0.0); // Not calibrated
        assert_eq!(result.confidence, 1.0);
        assert_eq!(da.packets_processed(), 1);
    }

    #[test]
    fn test_da_multi_step() {
        let mut da = DepthAheadNetwork::new(1042);
        let packet = make_test_packet();
        let metrics = make_test_metrics();

        for i in 0..10 {
            let features = extract_da_features(&packet, &metrics, Some((100.0 + i as f64, 3.0)));
            let result = da.process(&features, 1.0);
            assert!(result.anomaly_score >= 0.0 && result.anomaly_score <= 1.0);
            assert!(result.confidence >= 0.0 && result.confidence <= 1.0);
        }
        assert_eq!(da.packets_processed(), 10);
    }

    #[test]
    fn test_da_reset_state() {
        let mut da = DepthAheadNetwork::new(1042);
        let packet = make_test_packet();
        let metrics = make_test_metrics();
        let features = extract_da_features(&packet, &metrics, Some((100.0, 3.0)));

        for _ in 0..5 {
            da.process(&features, 1.0);
        }

        da.reset_state();
        // After reset_state, packets_processed is preserved (weights kept)
        // but hidden state is cleared
        assert_eq!(da.packets_processed(), 5);
    }
}
