//! CfC (Closed-form Continuous-time) cell implementation.
//!
//! Implements the continuous-time RNN cell with time-gated updates:
//!
//! ```text
//! For each neuron i (masked by NCP adjacency):
//!   tau[i] = softplus(W_tau * [x; h] + b_tau)
//!   f[i]   = sigmoid(-(dt * tau[i]) * (W_f * [x; h] + b_f))
//!   g[i]   = tanh(W_g * [x; h] + b_g)
//!   h_new[i] = f[i] * g[i] + (1 - f[i]) * h[i]
//! ```

use crate::cfc::wiring::{NcpWiring, NUM_OUTPUTS};
use crate::cfc::normalizer::NUM_FEATURES;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Serialize, Deserialize};

/// CfC cell weights and biases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfcWeights {
    // Per-neuron weight vectors for incoming connections (sparse).
    // For neuron i, w_tau[i], w_f[i], w_g[i] have length = number of incoming connections.
    // These are stored flat with an index map.

    /// Flat weight storage: 3 gates (tau, f, g) x NUM_NEURONS x max_incoming.
    /// Indexed via `weight_offset[neuron]` and `weight_count[neuron]`.
    pub w_tau: Vec<f64>,
    pub w_f: Vec<f64>,
    pub w_g: Vec<f64>,

    /// Bias per neuron per gate.
    pub b_tau: Vec<f64>,
    pub b_f: Vec<f64>,
    pub b_g: Vec<f64>,

    /// Output projection: [NUM_OUTPUTS x NUM_MOTOR].
    pub w_out: Vec<f64>,
    pub b_out: Vec<f64>,

    /// Input projection: [NUM_NEURONS] — weight for injecting each feature into
    /// its mapped sensory neurons.
    pub w_in: Vec<f64>,

    /// Offset into flat weight arrays for each neuron's incoming connections.
    pub weight_offset: Vec<usize>,
    /// Number of incoming connections for each neuron.
    pub weight_count: Vec<usize>,
}

impl CfcWeights {
    /// Initialize weights with Xavier/Glorot-style random initialization.
    pub fn init(wiring: &NcpWiring, seed: u64) -> Self {
        let mut rng = StdRng::seed_from_u64(seed);
        let n = wiring.config.num_neurons;
        let num_motor = wiring.config.num_motor();

        // Compute offsets for flat weight storage
        let mut weight_offset = vec![0usize; n];
        let mut weight_count = vec![0usize; n];
        let mut total_weights = 0usize;

        for i in 0..n {
            weight_offset[i] = total_weights;
            weight_count[i] = wiring.incoming[i].len();
            total_weights += weight_count[i];
        }

        // Xavier init: std = sqrt(2 / (fan_in + fan_out))
        let init_weights = |total: usize, rng: &mut StdRng| -> Vec<f64> {
            let mut w = vec![0.0; total];
            for i in 0..n {
                let fan_in = weight_count[i].max(1);
                let std = (2.0 / (fan_in + 1) as f64).sqrt();
                for j in 0..weight_count[i] {
                    w[weight_offset[i] + j] = rng.gen::<f64>() * 2.0 * std - std;
                }
            }
            w
        };

        let w_tau = init_weights(total_weights, &mut rng);
        let w_f = init_weights(total_weights, &mut rng);
        let w_g = init_weights(total_weights, &mut rng);

        // Biases initialized to small values
        let b_tau = vec![0.5; n]; // Bias toward moderate time constants
        let b_f = vec![0.0; n];
        let b_g = vec![0.0; n];

        // Output projection: Xavier init
        let out_std = (2.0 / (num_motor + NUM_OUTPUTS) as f64).sqrt();
        let w_out: Vec<f64> = (0..NUM_OUTPUTS * num_motor)
            .map(|_| rng.gen::<f64>() * 2.0 * out_std - out_std)
            .collect();
        let b_out = vec![0.0; NUM_OUTPUTS];

        // Input weights: one per input→sensory mapping (variable per feature)
        let in_std = (1.0 / NUM_FEATURES as f64).sqrt();
        let w_in: Vec<f64> = (0..wiring.total_input_weights)
            .map(|_| rng.gen::<f64>() * 2.0 * in_std - in_std)
            .collect();

        Self {
            w_tau, w_f, w_g,
            b_tau, b_f, b_g,
            w_out, b_out,
            w_in,
            weight_offset, weight_count,
        }
    }

    /// Total number of trainable parameters.
    pub fn num_params(&self) -> usize {
        self.w_tau.len() + self.w_f.len() + self.w_g.len()
            + self.b_tau.len() + self.b_f.len() + self.b_g.len()
            + self.w_out.len() + self.b_out.len()
            + self.w_in.len()
    }
}

/// Cached intermediate values from forward pass, needed for backprop.
#[derive(Debug, Clone)]
pub struct ForwardCache {
    /// Input values at each neuron (after aggregation).
    pub pre_tau: Vec<f64>,
    pub pre_f: Vec<f64>,
    pub pre_g: Vec<f64>,

    /// Gate outputs.
    pub tau: Vec<f64>,
    pub f_gate: Vec<f64>,
    pub g_gate: Vec<f64>,

    /// Hidden state before and after update.
    pub h_prev: Vec<f64>,
    pub h_new: Vec<f64>,

    /// Motor neuron outputs (before output projection).
    pub motor_out: Vec<f64>,

    /// Network output (predictions).
    pub output: Vec<f64>,

    /// Input features used (for input weight gradients).
    pub input_features: [f64; NUM_FEATURES],

    /// Sensory neuron activations (input projected values).
    pub sensory_activations: Vec<f64>,

    /// Delta-t used in this forward pass.
    pub dt: f64,
}

/// CfC cell: forward pass producing new hidden state and output.
pub struct CfcCell;

impl CfcCell {
    /// Run forward pass through the CfC cell.
    ///
    /// # Arguments
    /// * `input` - Normalized feature vector [NUM_FEATURES]
    /// * `h` - Current hidden state [NUM_NEURONS]
    /// * `dt` - Time step (seconds since last update)
    /// * `weights` - Network weights
    /// * `wiring` - NCP connectivity
    ///
    /// # Returns
    /// * Updated hidden state [NUM_NEURONS]
    /// * Network output [NUM_OUTPUTS]
    /// * ForwardCache for backprop
    pub fn forward(
        input: &[f64; NUM_FEATURES],
        h: &[f64],
        dt: f64,
        weights: &CfcWeights,
        wiring: &NcpWiring,
    ) -> (Vec<f64>, Vec<f64>, ForwardCache) {
        let n = wiring.config.num_neurons;
        let inter_start = wiring.config.sensory_end;
        let motor_start = wiring.config.command_end;
        let num_motor = wiring.config.num_motor();
        let mut h_new = h.to_vec();

        // Inject input features into sensory neurons (variable mapping)
        let mut sensory_activations = vec![0.0; n];
        let mut w_in_idx = 0;
        for (feat_idx, &val) in input.iter().enumerate() {
            for &neuron_idx in &wiring.input_map[feat_idx] {
                sensory_activations[neuron_idx] = val * weights.w_in[w_in_idx];
                h_new[neuron_idx] = sensory_activations[neuron_idx];
                w_in_idx += 1;
            }
        }

        // Allocate cache storage
        let mut pre_tau = vec![0.0; n];
        let mut pre_f = vec![0.0; n];
        let mut pre_g = vec![0.0; n];
        let mut tau = vec![0.0; n];
        let mut f_gate = vec![0.0; n];
        let mut g_gate = vec![0.0; n];
        let h_prev = h.to_vec();

        // Process neurons in group order (sensory neurons are set from input)
        // Inter, command, and motor neurons use gated update
        for neuron in inter_start..n {
            let n_in = weights.weight_count[neuron];
            if n_in == 0 {
                continue;
            }

            let offset = weights.weight_offset[neuron];

            // Compute weighted sum of incoming connections for each gate
            let mut sum_tau = weights.b_tau[neuron];
            let mut sum_f = weights.b_f[neuron];
            let mut sum_g = weights.b_g[neuron];

            for (j, &src) in wiring.incoming[neuron].iter().enumerate() {
                let h_src = h_new[src]; // Use updated h for feedforward
                sum_tau += weights.w_tau[offset + j] * h_src;
                sum_f += weights.w_f[offset + j] * h_src;
                sum_g += weights.w_g[offset + j] * h_src;
            }

            pre_tau[neuron] = sum_tau;
            pre_f[neuron] = sum_f;
            pre_g[neuron] = sum_g;

            // tau = softplus(sum_tau)
            tau[neuron] = softplus(sum_tau);

            // f = sigmoid(-(dt * tau) * sum_f)
            let f_input = -(dt * tau[neuron]) * sum_f;
            f_gate[neuron] = sigmoid(f_input);

            // g = tanh(sum_g)
            g_gate[neuron] = sum_g.tanh();

            // h_new = f * g + (1 - f) * h_prev
            h_new[neuron] = f_gate[neuron] * g_gate[neuron]
                + (1.0 - f_gate[neuron]) * h_prev[neuron];
        }

        // Output projection: y = W_out * h_motor + b_out
        let mut output = vec![0.0; NUM_OUTPUTS];
        let motor_out: Vec<f64> = h_new[motor_start..motor_start + num_motor].to_vec();
        for o in 0..NUM_OUTPUTS {
            let mut sum = weights.b_out[o];
            for m in 0..num_motor {
                sum += weights.w_out[o * num_motor + m] * motor_out[m];
            }
            output[o] = sum;
        }

        let cache = ForwardCache {
            pre_tau,
            pre_f,
            pre_g,
            tau,
            f_gate,
            g_gate,
            h_prev,
            h_new: h_new.clone(),
            motor_out,
            output: output.clone(),
            input_features: *input,
            sensory_activations,
            dt,
        };

        (h_new, output, cache)
    }
}

// ============================================================================
// Activation functions
// ============================================================================

#[inline]
pub fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

#[inline]
pub fn softplus(x: f64) -> f64 {
    if x > 20.0 {
        x // Avoid overflow
    } else {
        (1.0 + x.exp()).ln()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfc::wiring::{NcpWiring, NUM_NEURONS};

    #[test]
    fn test_forward_pass_shape() {
        let wiring = NcpWiring::generate(42);
        let weights = CfcWeights::init(&wiring, 123);
        let input = [0.1; NUM_FEATURES];
        let h = vec![0.0; NUM_NEURONS];

        let (h_new, output, cache) = CfcCell::forward(&input, &h, 1.0, &weights, &wiring);

        assert_eq!(h_new.len(), NUM_NEURONS);
        assert_eq!(output.len(), NUM_OUTPUTS);
        assert_eq!(cache.h_new.len(), NUM_NEURONS);
        assert_eq!(cache.output.len(), NUM_OUTPUTS);
    }

    #[test]
    fn test_forward_pass_finite() {
        let wiring = NcpWiring::generate(42);
        let weights = CfcWeights::init(&wiring, 123);
        let input = [1.0; NUM_FEATURES];
        let h = vec![0.0; NUM_NEURONS];

        let (h_new, output, _) = CfcCell::forward(&input, &h, 1.0, &weights, &wiring);

        for &v in &h_new {
            assert!(v.is_finite(), "h_new contains non-finite: {}", v);
        }
        for &v in &output {
            assert!(v.is_finite(), "output contains non-finite: {}", v);
        }
    }

    #[test]
    fn test_sigmoid_bounds() {
        assert!((sigmoid(0.0) - 0.5).abs() < 1e-10);
        assert!(sigmoid(100.0) > 0.999);
        assert!(sigmoid(-100.0) < 0.001);
    }

    #[test]
    fn test_softplus_positive() {
        for x in [-10.0, -1.0, 0.0, 1.0, 10.0, 100.0] {
            assert!(softplus(x) >= 0.0);
            assert!(softplus(x).is_finite());
        }
    }

    #[test]
    fn test_forward_pass_64_neurons() {
        use crate::cfc::wiring::NcpConfig;
        let cfg = NcpConfig::dual_64();
        let wiring = NcpWiring::generate_with_config(42, &cfg);
        let weights = CfcWeights::init(&wiring, 123);
        let input = [0.5; NUM_FEATURES];
        let h = vec![0.0; 64];

        let (h_new, output, cache) = CfcCell::forward(&input, &h, 1.0, &weights, &wiring);

        assert_eq!(h_new.len(), 64);
        assert_eq!(output.len(), NUM_OUTPUTS);
        assert_eq!(cache.motor_out.len(), 8); // 8 motor neurons

        for &v in &h_new {
            assert!(v.is_finite(), "h_new contains non-finite: {}", v);
        }
        for &v in &output {
            assert!(v.is_finite(), "output contains non-finite: {}", v);
        }
    }
}
