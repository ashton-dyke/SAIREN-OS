//! CfC Network: the top-level struct that orchestrates forward pass,
//! online training with BPTT depth=4, anomaly scoring, and calibration.
//!
//! The network is self-supervised: it predicts next-timestep sensor values
//! and treats prediction error as an anomaly signal.

use std::collections::VecDeque;
use crate::cfc::cell::{CfcCell, CfcWeights, ForwardCache};
use crate::cfc::normalizer::{OnlineNormalizer, NUM_FEATURES, FEATURE_NAMES};
use crate::cfc::training::{AdamOptimizer, train_step, BPTT_DEPTH};
use crate::cfc::wiring::{NcpWiring, NUM_NEURONS, NUM_OUTPUTS};

/// Calibration window: number of packets before the network is considered
/// calibrated enough to produce meaningful anomaly scores.
const CALIBRATION_WINDOW: u64 = 500;

/// EMA decay for anomaly score tracking.
const ERROR_EMA_ALPHA: f64 = 0.01;

/// Per-feature surprise: which features the network predicted badly.
#[derive(Debug, Clone)]
pub struct FeatureSurprise {
    /// Feature index (0..NUM_FEATURES).
    pub index: usize,
    /// Feature name (e.g., "torque", "spp").
    pub name: &'static str,
    /// Signed prediction error in normalized units (positive = actual > predicted).
    pub error: f64,
    /// Absolute error magnitude.
    pub magnitude: f64,
}

/// CfC neural network for drilling anomaly detection.
#[derive(Debug, Clone)]
pub struct CfcNetwork {
    /// NCP sparse wiring topology.
    wiring: NcpWiring,
    /// Network weights and biases.
    weights: CfcWeights,
    /// Online feature normalizer (Welford's).
    normalizer: OnlineNormalizer,
    /// Adam optimizer with decaying LR.
    optimizer: AdamOptimizer,
    /// Current hidden state [NUM_NEURONS].
    hidden: Vec<f64>,
    /// Ring buffer of recent forward caches for BPTT (most recent first).
    cache_history: VecDeque<ForwardCache>,
    /// Whether we have at least one cached step to train on.
    has_prev: bool,
    /// Packets processed.
    packets_processed: u64,
    /// EMA of prediction error (for anomaly scoring).
    error_ema: f64,
    /// EMA of squared prediction error (for anomaly scoring).
    error_sq_ema: f64,
    /// Cumulative training loss (for diagnostics).
    total_loss: f64,
    /// Number of training steps completed.
    train_steps: u64,
    /// Most recent per-step RMSE (for anomaly scoring).
    last_rmse: f64,
    /// Per-feature error EMA (for relative surprise scoring).
    feature_error_ema: [f64; NUM_FEATURES],
    /// Per-feature squared error EMA (for variance estimation).
    feature_error_sq_ema: [f64; NUM_FEATURES],
    /// Most recent per-feature signed errors (prediction - actual, normalized).
    last_feature_errors: [f64; NUM_FEATURES],
}

impl CfcNetwork {
    /// Create a new CfC network with deterministic initialization.
    pub fn new(seed: u64) -> Self {
        let wiring = NcpWiring::generate(seed);
        let weights = CfcWeights::init(&wiring, seed.wrapping_add(1));
        let num_params = weights.num_params();

        Self {
            wiring,
            weights,
            normalizer: OnlineNormalizer::new(),
            optimizer: AdamOptimizer::new(num_params),
            hidden: vec![0.0; NUM_NEURONS],
            cache_history: VecDeque::with_capacity(BPTT_DEPTH + 1),
            has_prev: false,
            packets_processed: 0,
            error_ema: 0.0,
            error_sq_ema: 0.0,
            total_loss: 0.0,
            train_steps: 0,
            last_rmse: 0.0,
            feature_error_ema: [0.0; NUM_FEATURES],
            feature_error_sq_ema: [0.0; NUM_FEATURES],
            last_feature_errors: [0.0; NUM_FEATURES],
        }
    }

    /// Process one packet: normalize, train on previous predictions via BPTT, forward pass.
    ///
    /// Returns (predictions, training_loss).
    pub fn process(&mut self, raw_features: &[f64; NUM_FEATURES], dt: f64) -> (Vec<f64>, Option<f64>) {
        self.packets_processed += 1;

        // Normalize current features (and update running stats)
        let normalized = self.normalizer.normalize_and_update(raw_features);

        // ====================================================================
        // Train: backprop through cached timesteps (BPTT depth up to 4)
        // ====================================================================
        let train_loss = if self.has_prev {
            // Target is current normalized values (what we should have predicted)
            let target: Vec<f64> = normalized.to_vec();

            // Compute per-feature errors before training (prediction - actual)
            if let Some(prev_cache) = self.cache_history.front() {
                for i in 0..NUM_OUTPUTS.min(NUM_FEATURES) {
                    let err = prev_cache.output[i] - target[i];
                    self.last_feature_errors[i] = err;

                    // Update per-feature error EMAs
                    let abs_err = err.abs();
                    self.feature_error_ema[i] = self.feature_error_ema[i] * (1.0 - ERROR_EMA_ALPHA)
                        + abs_err * ERROR_EMA_ALPHA;
                    self.feature_error_sq_ema[i] = self.feature_error_sq_ema[i] * (1.0 - ERROR_EMA_ALPHA)
                        + (abs_err * abs_err) * ERROR_EMA_ALPHA;
                }
            }

            // Build cache slice for BPTT (most recent first)
            let cache_vec: Vec<ForwardCache> = self.cache_history.iter().cloned().collect();

            let loss = train_step(
                &mut self.weights,
                &cache_vec,
                &target,
                &self.wiring,
                &mut self.optimizer,
            );

            self.total_loss += loss;
            self.train_steps += 1;

            // Update error EMA for anomaly scoring
            let rmse = loss.sqrt();
            self.last_rmse = rmse;
            self.error_ema = self.error_ema * (1.0 - ERROR_EMA_ALPHA)
                + rmse * ERROR_EMA_ALPHA;
            self.error_sq_ema = self.error_sq_ema * (1.0 - ERROR_EMA_ALPHA)
                + (rmse * rmse) * ERROR_EMA_ALPHA;

            Some(loss)
        } else {
            None
        };

        // ====================================================================
        // Forward: predict next timestep
        // ====================================================================
        let (h_new, predictions, cache) = CfcCell::forward(
            &normalized,
            &self.hidden,
            dt,
            &self.weights,
            &self.wiring,
        );

        self.hidden = h_new;

        // Push to cache history (most recent at front)
        self.cache_history.push_front(cache);
        if self.cache_history.len() > BPTT_DEPTH {
            self.cache_history.pop_back();
        }
        self.has_prev = true;

        (predictions, train_loss)
    }

    /// Compute anomaly score (0-1) based on adaptive z-score of prediction error.
    ///
    /// Uses EMA of RMSE + EMA of squared RMSE → z-score → sigmoid(z-2) → 0-1.
    /// z=2 maps to 0.5, z=4 maps to 0.88.
    pub fn anomaly_score(&self) -> f64 {
        if !self.is_calibrated() || self.train_steps < 2 {
            return 0.0;
        }

        let variance = (self.error_sq_ema - self.error_ema * self.error_ema).max(1e-12);
        let std = variance.sqrt();

        if std < 1e-10 {
            return 0.0;
        }

        // Use actual prediction error (RMSE from last training step)
        let z = (self.last_rmse - self.error_ema) / std;
        crate::cfc::cell::sigmoid(z - 2.0)
    }

    /// Get per-feature surprises sorted by magnitude (most surprising first).
    ///
    /// Each surprise is the absolute prediction error for that feature.
    /// Only returns features where the current error exceeds 1.5x the
    /// running average error for that feature (genuinely surprising).
    pub fn feature_surprises(&self) -> Vec<FeatureSurprise> {
        if self.train_steps < 10 {
            return Vec::new();
        }

        let mut surprises: Vec<FeatureSurprise> = (0..NUM_FEATURES)
            .filter_map(|i| {
                let abs_err = self.last_feature_errors[i].abs();
                let avg_err = self.feature_error_ema[i];

                // Only include if current error is notably above average
                if avg_err > 1e-10 && abs_err > avg_err * 1.5 {
                    Some(FeatureSurprise {
                        index: i,
                        name: FEATURE_NAMES[i],
                        error: self.last_feature_errors[i],
                        magnitude: abs_err,
                    })
                } else {
                    None
                }
            })
            .collect();

        surprises.sort_by(|a, b| b.magnitude.partial_cmp(&a.magnitude).unwrap_or(std::cmp::Ordering::Equal));
        surprises
    }

    /// Compute health score (0-1), inverse of anomaly.
    pub fn health_score(&self) -> f64 {
        1.0 - self.anomaly_score()
    }

    /// Whether the network has processed enough data to be calibrated.
    pub fn is_calibrated(&self) -> bool {
        self.packets_processed >= CALIBRATION_WINDOW
    }

    /// Number of packets processed.
    pub fn packets_processed(&self) -> u64 {
        self.packets_processed
    }

    /// Average training loss (MSE) over all steps.
    pub fn avg_loss(&self) -> f64 {
        if self.train_steps > 0 {
            self.total_loss / self.train_steps as f64
        } else {
            0.0
        }
    }

    /// Current learning rate.
    pub fn learning_rate(&self) -> f64 {
        self.optimizer.current_lr()
    }

    /// Number of trainable parameters.
    pub fn num_params(&self) -> usize {
        self.weights.num_params()
    }

    /// Number of active connections in the NCP wiring.
    pub fn num_connections(&self) -> usize {
        self.wiring.num_connections
    }

    /// Number of training steps completed.
    pub fn train_steps(&self) -> u64 {
        self.train_steps
    }

    /// Current BPTT depth being used (actual cache size, up to BPTT_DEPTH).
    pub fn current_bptt_depth(&self) -> usize {
        self.cache_history.len()
    }

    /// Reset network state (hidden state and training history, but keep weights).
    pub fn reset_state(&mut self) {
        self.hidden = vec![0.0; NUM_NEURONS];
        self.cache_history.clear();
        self.has_prev = false;
        self.error_ema = 0.0;
        self.error_sq_ema = 0.0;
        self.last_rmse = 0.0;
        self.feature_error_ema = [0.0; NUM_FEATURES];
        self.feature_error_sq_ema = [0.0; NUM_FEATURES];
        self.last_feature_errors = [0.0; NUM_FEATURES];
    }

    /// Full reset (new network from scratch).
    pub fn reset(&mut self) {
        *self = Self::new(42);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfc::wiring::NUM_OUTPUTS;

    #[test]
    fn test_network_process() {
        let mut net = CfcNetwork::new(42);
        let features = [10.0, 50.0, 120.0, 15.0, 30000.0, 3000.0,
                        1.5, 200.0, 10.5, 5.0, 0.1, 1.3,
                        60.0, 10.5, 20.0, 800.0];

        let (preds, loss) = net.process(&features, 1.0);
        assert_eq!(preds.len(), NUM_OUTPUTS);
        assert!(loss.is_none()); // No training on first packet

        // Second packet — should train
        let features2 = [10.5, 52.0, 121.0, 15.5, 31000.0, 3050.0,
                         1.6, 202.0, 10.6, 4.5, 0.2, 1.35,
                         61.0, 10.6, 22.0, 801.0];
        let (preds2, loss2) = net.process(&features2, 1.0);
        assert_eq!(preds2.len(), NUM_OUTPUTS);
        assert!(loss2.is_some());
        assert!(loss2.unwrap().is_finite());
    }

    #[test]
    fn test_bptt_depth_ramps_up() {
        let mut net = CfcNetwork::new(42);
        let features = [10.0; NUM_FEATURES];

        net.process(&features, 1.0);
        assert_eq!(net.current_bptt_depth(), 1);

        net.process(&features, 1.0);
        assert_eq!(net.current_bptt_depth(), 2);

        net.process(&features, 1.0);
        assert_eq!(net.current_bptt_depth(), 3);

        net.process(&features, 1.0);
        assert_eq!(net.current_bptt_depth(), 4);

        // Should cap at BPTT_DEPTH
        net.process(&features, 1.0);
        assert_eq!(net.current_bptt_depth(), 4);
    }

    #[test]
    fn test_calibration() {
        let mut net = CfcNetwork::new(42);
        assert!(!net.is_calibrated());

        let features = [10.0, 50.0, 120.0, 15.0, 30000.0, 3000.0,
                        1.5, 200.0, 10.5, 5.0, 0.1, 1.3,
                        60.0, 10.5, 20.0, 800.0];
        for _ in 0..CALIBRATION_WINDOW {
            net.process(&features, 1.0);
        }
        assert!(net.is_calibrated());
    }

    #[test]
    fn test_anomaly_score_bounds() {
        let mut net = CfcNetwork::new(42);
        let features = [10.0, 50.0, 120.0, 15.0, 30000.0, 3000.0,
                        1.5, 200.0, 10.5, 5.0, 0.1, 1.3,
                        60.0, 10.5, 20.0, 800.0];

        assert_eq!(net.anomaly_score(), 0.0);

        for i in 0..600 {
            let mut f = features;
            f[0] += (i as f64) * 0.01;
            net.process(&f, 1.0);
        }

        let score = net.anomaly_score();
        assert!(score >= 0.0 && score <= 1.0, "score out of bounds: {}", score);
    }

    #[test]
    fn test_learning_rate_decay() {
        let mut net = CfcNetwork::new(42);
        let lr0 = net.learning_rate();

        let features = [10.0; NUM_FEATURES];
        for _ in 0..100 {
            net.process(&features, 1.0);
        }

        assert!(net.learning_rate() < lr0);
    }
}
