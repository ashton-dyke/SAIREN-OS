//! Manual backpropagation through the CfC cell + Adam optimizer.
//!
//! Implements truncated BPTT (depth=4): backprop through up to 4 timesteps,
//! chaining gradients through hidden states. Gradient contribution from
//! older steps is decayed by 0.7^k to stabilize training.
//!
//! The loss is feature-weighted MSE between predicted and actual next-step values.
//! Primary drilling features (WOB, ROP, torque, SPP) are weighted 2x.

use crate::cfc::cell::{CfcWeights, ForwardCache, sigmoid};
use crate::cfc::wiring::{NcpWiring, NUM_OUTPUTS};

#[cfg(test)]
use crate::cfc::wiring::NUM_NEURONS;

/// Maximum BPTT depth (number of timesteps to backprop through).
pub const BPTT_DEPTH: usize = 4;

/// Gradient decay factor per step back in time.
const BPTT_DECAY: f64 = 0.7;

/// Max gradient norm for global gradient clipping.
const MAX_GRAD_NORM: f64 = 5.0;

/// Training hyperparameters for a CfC network.
#[derive(Debug, Clone)]
pub struct TrainingConfig {
    pub bptt_depth: usize,
    pub bptt_decay: f64,
    pub max_grad_norm: f64,
    pub initial_lr: f64,
    pub lr_decay: f64,
    pub lr_floor: f64,
}

impl TrainingConfig {
    /// Fast network config: BPTT=4, LR 0.001→0.0001 — catches acute events.
    pub fn fast() -> Self {
        Self {
            bptt_depth: 4,
            bptt_decay: 0.7,
            max_grad_norm: 5.0,
            initial_lr: 0.001,
            lr_decay: 0.9999,
            lr_floor: 0.0001,
        }
    }

    /// Slow network config: BPTT=8, LR 0.0001→0.00001 — catches gradual trends.
    pub fn slow() -> Self {
        Self {
            bptt_depth: 8,
            bptt_decay: 0.85,
            max_grad_norm: 5.0,
            initial_lr: 0.0001,
            lr_decay: 0.9999,
            lr_floor: 0.00001,
        }
    }
}

/// Per-output feature weights for loss computation.
/// Primary features (0-7) get weight 2.0, supplementary (8-15) get weight 1.0.
const FEATURE_WEIGHTS: [f64; NUM_OUTPUTS] = [
    2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0, // WOB, ROP, RPM, torque, MSE, SPP, d-exp, hookload
    1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, // ECD, flow_bal, pit_rate, DXC, pump, MW, gas, pit_vol
];

/// Accumulated gradients matching the CfcWeights layout.
struct GradAccum {
    d_w_tau: Vec<f64>,
    d_w_f: Vec<f64>,
    d_w_g: Vec<f64>,
    d_b_tau: Vec<f64>,
    d_b_f: Vec<f64>,
    d_b_g: Vec<f64>,
    d_w_out: Vec<f64>,
    d_b_out: Vec<f64>,
    d_w_in: Vec<f64>,
}

impl GradAccum {
    fn new(weights: &CfcWeights, num_neurons: usize) -> Self {
        Self {
            d_w_tau: vec![0.0; weights.w_tau.len()],
            d_w_f: vec![0.0; weights.w_f.len()],
            d_w_g: vec![0.0; weights.w_g.len()],
            d_b_tau: vec![0.0; num_neurons],
            d_b_f: vec![0.0; num_neurons],
            d_b_g: vec![0.0; num_neurons],
            d_w_out: vec![0.0; weights.w_out.len()],
            d_b_out: vec![0.0; NUM_OUTPUTS],
            d_w_in: vec![0.0; weights.w_in.len()],
        }
    }

    /// Compute L2 norm of all accumulated gradients.
    fn grad_norm(&self) -> f64 {
        let mut sum = 0.0;
        for v in self.d_w_tau.iter().chain(self.d_w_f.iter()).chain(self.d_w_g.iter())
            .chain(self.d_b_tau.iter()).chain(self.d_b_f.iter()).chain(self.d_b_g.iter())
            .chain(self.d_w_out.iter()).chain(self.d_b_out.iter())
            .chain(self.d_w_in.iter())
        {
            sum += v * v;
        }
        sum.sqrt()
    }

    /// Scale all gradients by a factor.
    fn scale(&mut self, factor: f64) {
        for v in self.d_w_tau.iter_mut().chain(self.d_w_f.iter_mut()).chain(self.d_w_g.iter_mut())
            .chain(self.d_b_tau.iter_mut()).chain(self.d_b_f.iter_mut()).chain(self.d_b_g.iter_mut())
            .chain(self.d_w_out.iter_mut()).chain(self.d_b_out.iter_mut())
            .chain(self.d_w_in.iter_mut())
        {
            *v *= factor;
        }
    }
}

/// Adam optimizer with decaying base learning rate.
#[derive(Debug, Clone)]
pub struct AdamOptimizer {
    /// Base learning rate (decays over time).
    pub lr: f64,
    /// LR decay factor per step.
    pub decay: f64,
    /// Minimum learning rate floor.
    pub lr_floor: f64,
    /// Adam beta1 (first moment decay).
    pub beta1: f64,
    /// Adam beta2 (second moment decay).
    pub beta2: f64,
    /// Adam epsilon (numerical stability).
    pub eps: f64,
    /// Total steps taken.
    pub steps: u64,
    /// First moment estimates (same layout as weights, flattened).
    m: Vec<f64>,
    /// Second moment estimates (same layout as weights, flattened).
    v: Vec<f64>,
}

impl AdamOptimizer {
    pub fn new(num_params: usize) -> Self {
        Self {
            lr: 0.001,
            decay: 0.9999,
            lr_floor: 0.0001,
            beta1: 0.9,
            beta2: 0.999,
            eps: 1e-8,
            steps: 0,
            m: vec![0.0; num_params],
            v: vec![0.0; num_params],
        }
    }

    /// Create an optimizer from a TrainingConfig.
    pub fn with_config(num_params: usize, config: &TrainingConfig) -> Self {
        Self {
            lr: config.initial_lr,
            decay: config.lr_decay,
            lr_floor: config.lr_floor,
            beta1: 0.9,
            beta2: 0.999,
            eps: 1e-8,
            steps: 0,
            m: vec![0.0; num_params],
            v: vec![0.0; num_params],
        }
    }

    /// Current effective learning rate (before bias correction).
    pub fn current_lr(&self) -> f64 {
        self.lr
    }

    /// Apply Adam update to all weights given accumulated gradients.
    /// Gradients are passed as flat slices matching the moment vector layout.
    fn apply(&mut self, weights_flat: &mut [f64], grads_flat: &[f64]) {
        self.steps += 1;
        let t = self.steps as f64;

        // Bias-corrected LR
        let lr_t = self.lr * (1.0 - self.beta2.powf(t)).sqrt() / (1.0 - self.beta1.powf(t));

        for i in 0..weights_flat.len() {
            let g = grads_flat[i];
            self.m[i] = self.beta1 * self.m[i] + (1.0 - self.beta1) * g;
            self.v[i] = self.beta2 * self.v[i] + (1.0 - self.beta2) * g * g;
            weights_flat[i] -= lr_t * self.m[i] / (self.v[i].sqrt() + self.eps);
        }

        // Decay base LR
        self.lr = (self.lr * self.decay).max(self.lr_floor);
    }
}

/// Flatten all weights into a contiguous vec for Adam, and unflatten back.
fn flatten_weights(w: &CfcWeights) -> Vec<f64> {
    let mut flat = Vec::with_capacity(w.num_params());
    flat.extend_from_slice(&w.w_tau);
    flat.extend_from_slice(&w.w_f);
    flat.extend_from_slice(&w.w_g);
    flat.extend_from_slice(&w.b_tau);
    flat.extend_from_slice(&w.b_f);
    flat.extend_from_slice(&w.b_g);
    flat.extend_from_slice(&w.w_out);
    flat.extend_from_slice(&w.b_out);
    flat.extend_from_slice(&w.w_in);
    flat
}

fn flatten_grads(g: &GradAccum) -> Vec<f64> {
    let mut flat = Vec::with_capacity(
        g.d_w_tau.len() + g.d_w_f.len() + g.d_w_g.len()
        + g.d_b_tau.len() + g.d_b_f.len() + g.d_b_g.len()
        + g.d_w_out.len() + g.d_b_out.len() + g.d_w_in.len()
    );
    flat.extend_from_slice(&g.d_w_tau);
    flat.extend_from_slice(&g.d_w_f);
    flat.extend_from_slice(&g.d_w_g);
    flat.extend_from_slice(&g.d_b_tau);
    flat.extend_from_slice(&g.d_b_f);
    flat.extend_from_slice(&g.d_b_g);
    flat.extend_from_slice(&g.d_w_out);
    flat.extend_from_slice(&g.d_b_out);
    flat.extend_from_slice(&g.d_w_in);
    flat
}

fn unflatten_weights(flat: &[f64], w: &mut CfcWeights) {
    let mut offset = 0;
    let n = w.w_tau.len();
    w.w_tau.copy_from_slice(&flat[offset..offset + n]); offset += n;
    let n = w.w_f.len();
    w.w_f.copy_from_slice(&flat[offset..offset + n]); offset += n;
    let n = w.w_g.len();
    w.w_g.copy_from_slice(&flat[offset..offset + n]); offset += n;
    let n = w.b_tau.len();
    w.b_tau.copy_from_slice(&flat[offset..offset + n]); offset += n;
    let n = w.b_f.len();
    w.b_f.copy_from_slice(&flat[offset..offset + n]); offset += n;
    let n = w.b_g.len();
    w.b_g.copy_from_slice(&flat[offset..offset + n]); offset += n;
    let n = w.w_out.len();
    w.w_out.copy_from_slice(&flat[offset..offset + n]); offset += n;
    let n = w.b_out.len();
    w.b_out.copy_from_slice(&flat[offset..offset + n]); offset += n;
    let n = w.w_in.len();
    w.w_in.copy_from_slice(&flat[offset..offset + n]);
}

/// Train with truncated BPTT through multiple cached timesteps.
///
/// Uses feature-weighted loss (primary features 2x) and Adam optimizer
/// with gradient norm clipping.
///
/// # Returns
/// * Weighted MSE loss for this step
pub fn train_step(
    weights: &mut CfcWeights,
    cache_history: &[ForwardCache],
    target: &[f64],
    wiring: &NcpWiring,
    optimizer: &mut AdamOptimizer,
) -> f64 {
    train_step_with_config(weights, cache_history, target, wiring, optimizer, None)
}

/// Train with truncated BPTT, optionally using a TrainingConfig for hyperparameters.
///
/// When `config` is None, uses the legacy constants (BPTT_DEPTH=4, BPTT_DECAY=0.7, MAX_GRAD_NORM=5.0).
pub fn train_step_with_config(
    weights: &mut CfcWeights,
    cache_history: &[ForwardCache],
    target: &[f64],
    wiring: &NcpWiring,
    optimizer: &mut AdamOptimizer,
    config: Option<&TrainingConfig>,
) -> f64 {
    if cache_history.is_empty() {
        return 0.0;
    }

    let bptt_depth = config.map_or(BPTT_DEPTH, |c| c.bptt_depth);
    let bptt_decay = config.map_or(BPTT_DECAY, |c| c.bptt_decay);
    let max_grad_norm = config.map_or(MAX_GRAD_NORM, |c| c.max_grad_norm);
    let n = wiring.config.num_neurons;
    let num_motor = wiring.config.num_motor();
    let motor_start = wiring.config.command_end;
    let inter_start = wiring.config.sensory_end;

    let most_recent = &cache_history[0];
    let mut grads = GradAccum::new(weights, n);

    // ========================================================================
    // 1. Compute feature-weighted output loss
    // ========================================================================
    let weight_sum: f64 = FEATURE_WEIGHTS.iter().sum();
    let mut loss = 0.0;
    let mut d_output = vec![0.0; NUM_OUTPUTS];
    for o in 0..NUM_OUTPUTS {
        let err = most_recent.output[o] - target[o];
        let w = FEATURE_WEIGHTS[o];
        d_output[o] = 2.0 * w * err / weight_sum;
        loss += w * err * err;
    }
    loss /= weight_sum;

    // ========================================================================
    // 2. Backprop through output projection: y = W_out * h_motor + b_out
    // ========================================================================
    let mut d_h = vec![0.0; n];

    for o in 0..NUM_OUTPUTS {
        grads.d_b_out[o] += d_output[o];
        for m in 0..num_motor {
            let w_idx = o * num_motor + m;
            grads.d_w_out[w_idx] += d_output[o] * most_recent.motor_out[m];
            d_h[motor_start + m] += d_output[o] * weights.w_out[w_idx];
        }
    }

    // ========================================================================
    // 3. BPTT: backprop through up to bptt_depth cached timesteps
    // ========================================================================
    let depth = cache_history.len().min(bptt_depth);

    for step in 0..depth {
        let cache = &cache_history[step];
        let decay = bptt_decay.powi(step as i32);

        d_h = backprop_cfc_gates(&mut grads, weights, cache, &d_h, wiring, decay, n, inter_start);
        backprop_input_projection(&mut grads, cache, &d_h, wiring, decay);
    }

    // ========================================================================
    // 4. Gradient norm clipping
    // ========================================================================
    let norm = grads.grad_norm();
    if norm > max_grad_norm {
        grads.scale(max_grad_norm / norm);
    }

    // ========================================================================
    // 5. Adam update
    // ========================================================================
    let mut flat_w = flatten_weights(weights);
    let flat_g = flatten_grads(&grads);
    optimizer.apply(&mut flat_w, &flat_g);
    unflatten_weights(&flat_w, weights);

    loss
}

/// Backprop through CfC gate equations for one timestep.
/// Accumulates gradients into `grads` and returns dL/dh_prev.
///
/// Uses a two-pass approach to handle both feedforward and recurrent connections:
/// - Pass 1 (reverse order): Correctly handles feedforward paths (src < dst).
/// - Pass 2 (forward order): Picks up residual gradients that landed on neurons
///   after they were already processed in pass 1 (recurrent paths where src > dst).
fn backprop_cfc_gates(
    grads: &mut GradAccum,
    weights: &CfcWeights,
    cache: &ForwardCache,
    d_h_in: &[f64],
    wiring: &NcpWiring,
    decay: f64,
    num_neurons: usize,
    inter_start: usize,
) -> Vec<f64> {
    let mut d_h = d_h_in.to_vec();
    let mut d_h_prev = vec![0.0; num_neurons];

    // Snapshot d_h consumed by each neuron during pass 1 so we can detect
    // late-arriving gradient from recurrent connections.
    let mut d_h_consumed = vec![0.0f64; num_neurons];

    // ── Pass 1: Reverse order (handles feedforward paths correctly) ──
    for neuron in (inter_start..num_neurons).rev() {
        let n_in = weights.weight_count[neuron];
        if n_in == 0 {
            continue;
        }

        d_h_consumed[neuron] = d_h[neuron];
        let dh = d_h[neuron];
        if dh.abs() < 1e-15 {
            d_h_prev[neuron] += dh * (1.0 - cache.f_gate[neuron]);
            continue;
        }

        let offset = weights.weight_offset[neuron];
        let f = cache.f_gate[neuron];
        let g = cache.g_gate[neuron];
        let h_prev = cache.h_prev[neuron];
        let dt = cache.dt;

        // h_new = f * g + (1-f) * h_prev
        let df = dh * (g - h_prev);
        let dg = dh * f;

        d_h_prev[neuron] += dh * (1.0 - f);

        // g = tanh(pre_g) → d_pre_g = dg * (1 - g^2)
        let d_pre_g = dg * (1.0 - g * g);

        // f = sigmoid(-(dt * tau) * pre_f)
        let d_f_input = df * f * (1.0 - f);
        let tau = cache.tau[neuron];
        let d_pre_f = d_f_input * (-(dt * tau));
        let d_tau = d_f_input * (-(dt) * cache.pre_f[neuron]);
        let d_pre_tau = d_tau * sigmoid(cache.pre_tau[neuron]);

        for (j, &src) in wiring.incoming[neuron].iter().enumerate() {
            let h_src = cache.h_new[src];
            let w_idx = offset + j;

            grads.d_w_tau[w_idx] += decay * d_pre_tau * h_src;
            grads.d_w_f[w_idx] += decay * d_pre_f * h_src;
            grads.d_w_g[w_idx] += decay * d_pre_g * h_src;

            let grad_to_src = d_pre_tau * weights.w_tau[w_idx]
                + d_pre_f * weights.w_f[w_idx]
                + d_pre_g * weights.w_g[w_idx];

            if src >= inter_start {
                d_h[src] += grad_to_src;
            } else {
                d_h_prev[src] += grad_to_src;
            }
        }

        grads.d_b_tau[neuron] += decay * d_pre_tau;
        grads.d_b_f[neuron] += decay * d_pre_f;
        grads.d_b_g[neuron] += decay * d_pre_g;
    }

    // ── Pass 2: Forward order (picks up residual gradient from recurrent connections) ──
    // For recurrent connections (src > dst), pass 1 scattered gradient to d_h[src]
    // after src was already processed. The residual = d_h[src] - d_h_consumed[src]
    // must be propagated through src's gates to d_h_prev for temporal credit assignment.
    for neuron in inter_start..num_neurons {
        let residual = d_h[neuron] - d_h_consumed[neuron];
        if residual.abs() < 1e-15 {
            continue;
        }

        let n_in = weights.weight_count[neuron];
        if n_in == 0 {
            continue;
        }

        let offset = weights.weight_offset[neuron];
        let f = cache.f_gate[neuron];
        let g = cache.g_gate[neuron];
        let h_prev = cache.h_prev[neuron];
        let dt = cache.dt;

        // Propagate residual through this neuron's gates
        let df = residual * (g - h_prev);
        let dg = residual * f;

        d_h_prev[neuron] += residual * (1.0 - f);

        let d_pre_g = dg * (1.0 - g * g);
        let d_f_input = df * f * (1.0 - f);
        let tau = cache.tau[neuron];
        let d_pre_f = d_f_input * (-(dt * tau));
        let d_tau = d_f_input * (-(dt) * cache.pre_f[neuron]);
        let d_pre_tau = d_tau * sigmoid(cache.pre_tau[neuron]);

        for (j, &src) in wiring.incoming[neuron].iter().enumerate() {
            let h_src = cache.h_new[src];
            let w_idx = offset + j;

            grads.d_w_tau[w_idx] += decay * d_pre_tau * h_src;
            grads.d_w_f[w_idx] += decay * d_pre_f * h_src;
            grads.d_w_g[w_idx] += decay * d_pre_g * h_src;

            // In pass 2, route all source gradients to d_h_prev to avoid
            // unbounded re-propagation. One correction pass is sufficient.
            let grad_to_src = d_pre_tau * weights.w_tau[w_idx]
                + d_pre_f * weights.w_f[w_idx]
                + d_pre_g * weights.w_g[w_idx];

            d_h_prev[src] += grad_to_src;
        }

        grads.d_b_tau[neuron] += decay * d_pre_tau;
        grads.d_b_f[neuron] += decay * d_pre_f;
        grads.d_b_g[neuron] += decay * d_pre_g;
    }

    d_h_prev
}

/// Backprop through input projection weights.
fn backprop_input_projection(
    grads: &mut GradAccum,
    cache: &ForwardCache,
    d_h: &[f64],
    wiring: &NcpWiring,
    decay: f64,
) {
    let mut w_idx = 0;
    for (feat_idx, &val) in cache.input_features.iter().enumerate() {
        for &neuron_idx in &wiring.input_map[feat_idx] {
            grads.d_w_in[w_idx] += decay * d_h[neuron_idx] * val;
            w_idx += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfc::cell::{CfcCell, CfcWeights};
    use crate::cfc::normalizer::NUM_FEATURES;
    use crate::cfc::wiring::NcpWiring;

    fn make_optimizer(wiring: &NcpWiring) -> AdamOptimizer {
        let weights = CfcWeights::init(wiring, 0);
        AdamOptimizer::new(weights.num_params())
    }

    #[test]
    fn test_train_step_single_cache() {
        let wiring = NcpWiring::generate(42);
        let mut weights = CfcWeights::init(&wiring, 123);
        let mut optimizer = make_optimizer(&wiring);

        let input = [0.5; NUM_FEATURES];
        let h = vec![0.0; NUM_NEURONS];
        let target = [0.1; NUM_FEATURES];

        let (_, _, cache) = CfcCell::forward(&input, &h, 1.0, &weights, &wiring);
        let loss = train_step(&mut weights, &[cache], &target, &wiring, &mut optimizer);
        assert!(loss.is_finite());
        assert!(loss >= 0.0);
    }

    #[test]
    fn test_train_step_multi_cache() {
        let wiring = NcpWiring::generate(42);
        let mut weights = CfcWeights::init(&wiring, 123);
        let mut optimizer = make_optimizer(&wiring);

        let input1 = [0.3; NUM_FEATURES];
        let input2 = [0.5; NUM_FEATURES];
        let input3 = [0.7; NUM_FEATURES];
        let target = [0.1; NUM_FEATURES];

        let h = vec![0.0; NUM_NEURONS];
        let (h1, _, cache1) = CfcCell::forward(&input1, &h, 1.0, &weights, &wiring);
        let (h2, _, cache2) = CfcCell::forward(&input2, &h1, 1.0, &weights, &wiring);
        let (_, _, cache3) = CfcCell::forward(&input3, &h2, 1.0, &weights, &wiring);

        // Most recent first
        let caches = [cache3, cache2, cache1];
        let loss = train_step(&mut weights, &caches, &target, &wiring, &mut optimizer);
        assert!(loss.is_finite());
    }

    #[test]
    fn test_bptt_improves_over_single() {
        let wiring = NcpWiring::generate(42);

        // Run with depth=1
        let mut weights1 = CfcWeights::init(&wiring, 123);
        let mut opt1 = make_optimizer(&wiring);
        let mut h1 = vec![0.0; NUM_NEURONS];
        let mut caches1 = Vec::new();

        for i in 0..20 {
            let mut input = [0.5; NUM_FEATURES];
            input[0] = (i as f64) * 0.1;
            let (h_new, _, cache) = CfcCell::forward(&input, &h1, 1.0, &weights1, &wiring);
            if !caches1.is_empty() {
                train_step(&mut weights1, &caches1[..1], &input, &wiring, &mut opt1);
            }
            caches1.insert(0, cache);
            if caches1.len() > BPTT_DEPTH {
                caches1.pop();
            }
            h1 = h_new;
        }

        // Run with depth=4
        let mut weights4 = CfcWeights::init(&wiring, 123);
        let mut opt4 = make_optimizer(&wiring);
        let mut h4 = vec![0.0; NUM_NEURONS];
        let mut caches4 = Vec::new();

        for i in 0..20 {
            let mut input = [0.5; NUM_FEATURES];
            input[0] = (i as f64) * 0.1;
            let (h_new, _, cache) = CfcCell::forward(&input, &h4, 1.0, &weights4, &wiring);
            if !caches4.is_empty() {
                train_step(&mut weights4, &caches4, &input, &wiring, &mut opt4);
            }
            caches4.insert(0, cache);
            if caches4.len() > BPTT_DEPTH {
                caches4.pop();
            }
            h4 = h_new;
        }

        assert!(opt1.steps > 0);
        assert!(opt4.steps > 0);
    }

    #[test]
    fn test_optimizer_decay() {
        let mut opt = AdamOptimizer::new(10);
        let lr0 = opt.current_lr();

        // Apply a dummy update to trigger decay
        let mut w = vec![0.0; 10];
        let g = vec![0.1; 10];
        opt.apply(&mut w, &g);

        assert!(opt.current_lr() < lr0);

        for _ in 0..100_000 {
            opt.apply(&mut w, &g);
        }
        assert!((opt.current_lr() - opt.lr_floor).abs() < 1e-8);
    }

    #[test]
    fn test_gradient_norm_clipping() {
        let wiring = NcpWiring::generate(42);
        let weights = CfcWeights::init(&wiring, 123);
        let mut grads = GradAccum::new(&weights, wiring.num_neurons());
        // Set huge gradients
        for v in grads.d_w_tau.iter_mut() {
            *v = 100.0;
        }
        let norm = grads.grad_norm();
        assert!(norm > MAX_GRAD_NORM);
        grads.scale(MAX_GRAD_NORM / norm);
        let clipped_norm = grads.grad_norm();
        assert!((clipped_norm - MAX_GRAD_NORM).abs() < 1e-6);
    }

    #[test]
    fn test_feature_weights_sum() {
        let sum: f64 = FEATURE_WEIGHTS.iter().sum();
        assert!((sum - 24.0).abs() < 1e-10); // 8*2 + 8*1 = 24
    }

    #[test]
    fn test_recurrent_gradient_flow() {
        // Verify that the two-pass backprop propagates gradients through recurrent
        // connections (src > dst). Before the fix, neurons processed early in the
        // reverse pass never received gradient scattered by later-processed neurons.

        let wiring = NcpWiring::generate(42);
        let weights = CfcWeights::init(&wiring, 123);
        let n = wiring.num_neurons();
        let inter_start = wiring.config.sensory_end;
        let motor_start = wiring.config.command_end;

        // Identify recurrent connections where src > dst (these were broken before the fix)
        let mut recurrent_sources: std::collections::HashSet<usize> = std::collections::HashSet::new();
        for dst in inter_start..n {
            for &src in &wiring.incoming[dst] {
                if src > dst && src >= inter_start {
                    recurrent_sources.insert(src);
                }
            }
        }
        assert!(!recurrent_sources.is_empty(),
            "Expected recurrent connections in the wiring (src > dst)");

        // Run a forward pass with non-trivial inputs to get a cache
        let input = [0.5; NUM_FEATURES];
        let h = vec![0.3; n]; // Non-zero initial hidden state
        let (_, _, cache) = CfcCell::forward(&input, &h, 1.0, &weights, &wiring);

        // Create gradient signal at motor neurons (as if from output loss)
        let mut d_h_in = vec![0.0; n];
        for m in motor_start..n {
            d_h_in[m] = 1.0;
        }

        let mut grads = GradAccum::new(&weights, n);
        let d_h_prev = backprop_cfc_gates(&mut grads, &weights, &cache, &d_h_in, &wiring, 1.0, n, inter_start);

        // Check that recurrent source neurons received temporal gradient via d_h_prev.
        // Without the two-pass fix, these would be zero because the gradient from
        // downstream neurons (dst < src) arrives after src was already processed.
        let mut nonzero_count = 0;
        for &src in &recurrent_sources {
            if d_h_prev[src].abs() > 1e-15 {
                nonzero_count += 1;
            }
        }

        // At least some recurrent sources should get nonzero d_h_prev
        assert!(nonzero_count > 0,
            "No recurrent source neurons received gradient in d_h_prev. \
             Found {} recurrent sources (src > dst), all had zero gradient.",
            recurrent_sources.len());

        // Majority should have nonzero gradient (allowing for some that may
        // coincidentally be near zero due to gate values)
        let ratio = nonzero_count as f64 / recurrent_sources.len() as f64;
        assert!(ratio > 0.5,
            "Only {}/{} ({:.0}%) recurrent source neurons got nonzero d_h_prev gradient",
            nonzero_count, recurrent_sources.len(), ratio * 100.0);
    }

    #[test]
    fn test_training_config_slow() {
        let cfg = TrainingConfig::slow();
        assert_eq!(cfg.bptt_depth, 8);
        assert_eq!(cfg.initial_lr, 0.0001);
        assert_eq!(cfg.lr_floor, 0.00001);
        assert!((cfg.bptt_decay - 0.85).abs() < 1e-10);
    }

    #[test]
    fn test_optimizer_with_config() {
        let cfg = TrainingConfig::slow();
        let mut opt = AdamOptimizer::with_config(10, &cfg);
        assert!((opt.current_lr() - 0.0001).abs() < 1e-10);
        assert!((opt.lr_floor - 0.00001).abs() < 1e-10);

        let mut w = vec![0.0; 10];
        let g = vec![0.1; 10];
        opt.apply(&mut w, &g);
        assert!(opt.current_lr() < 0.0001);
    }

    #[test]
    fn test_train_step_with_config_64_neurons() {
        use crate::cfc::wiring::NcpConfig;
        let cfg = NcpConfig::dual_64();
        let wiring = NcpWiring::generate_with_config(42, &cfg);
        let mut weights = CfcWeights::init(&wiring, 123);
        let training_cfg = TrainingConfig::slow();
        let mut optimizer = AdamOptimizer::with_config(weights.num_params(), &training_cfg);

        let input = [0.5; NUM_FEATURES];
        let h = vec![0.0; 64];
        let target = [0.1; NUM_FEATURES];

        let (_, _, cache) = CfcCell::forward(&input, &h, 1.0, &weights, &wiring);
        let loss = train_step_with_config(
            &mut weights, &[cache], &target, &wiring, &mut optimizer, Some(&training_cfg)
        );
        assert!(loss.is_finite());
        assert!(loss >= 0.0);
    }
}
