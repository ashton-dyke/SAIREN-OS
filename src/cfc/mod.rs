//! CfC (Closed-form Continuous-time) Neural Network — Dual Architecture.
//!
//! Two 64-neuron CfC/NCP networks running in parallel:
//!
//! - **Fast network** (LR 0.001→0.0001, BPTT=4): catches acute events
//!   (kicks, sudden losses) — adapts quickly to step changes.
//! - **Slow network** (LR 0.0001→0.00001, BPTT=8): catches gradual trends
//!   (pack-offs, washouts) — maintains a stable baseline so prediction error
//!   stays elevated for slow-moving anomalies.
//!
//! Combined scoring: `max(fast_score, slow_score)` — either network can
//! trigger detection. Two 64-neuron networks cost ~77% of a single 128-neuron
//! network while providing fundamentally better coverage.
//!
//! The networks are **self-supervised** — they predict next-timestep sensor
//! values and treat prediction error as an anomaly signal. No labeled
//! training data is needed.
//!
//! ## Architecture (per network)
//!
//! - 64 CfC neurons with NCP sparse wiring (~30% connectivity)
//! - 16 input features: WOB, ROP, RPM, torque, MSE, SPP, d-exponent,
//!   hookload, ECD, flow_balance, pit_rate, DXC, pump_spm, MW, gas, pit_vol
//! - NCP groups: 24 sensory → 20 inter → 12 command → 8 motor
//! - Outputs: 16 next-step predictions, anomaly score (0-1), health score (0-1)
//! - Online training: forward → predict → compare → backprop → Adam, every packet

pub mod normalizer;
pub mod wiring;
pub mod cell;
pub mod training;
pub mod network;
pub mod formation_detector;
pub mod regime_clusterer;

pub use network::{CfcNetwork, CfcNetworkConfig, FeatureSurprise};
pub use normalizer::NUM_FEATURES;
pub use regime_clusterer::RegimeClusterer;

use crate::types::{DrillingMetrics, WitsPacket};

/// Result of CfC processing for one packet (single-network output).
#[derive(Debug, Clone)]
pub struct CfcDrillingResult {
    /// Anomaly score from CfC (0.0 = normal, 1.0 = highly anomalous).
    pub anomaly_score: f64,
    /// Health score (1.0 - anomaly_score).
    pub health_score: f64,
    /// Training loss for this step (None if first packet).
    pub training_loss: Option<f64>,
    /// Whether the network is calibrated (>500 packets processed).
    pub is_calibrated: bool,
    /// Current learning rate.
    pub learning_rate: f64,
    /// Total packets processed by the network.
    pub packets_processed: u64,
    /// Average training loss since start.
    pub avg_loss: f64,
    /// Per-feature surprises (most surprising first), only features exceeding
    /// 1.5x their running average error. Empty if not calibrated or too early.
    pub feature_surprises: Vec<FeatureSurprise>,
    /// Z-scores for ALL 16 features (for formation transition detection).
    pub feature_sigmas: Vec<(usize, &'static str, f64)>,
    /// Motor neuron outputs (8-dimensional) from the CfC NCP motor layer.
    /// Used for regime clustering. Empty if no forward pass has been computed.
    pub motor_outputs: Vec<f64>,
}

/// Combined result from the dual CfC network architecture.
#[derive(Debug, Clone)]
pub struct DualCfcResult {
    /// Combined anomaly score: max(fast, slow).
    pub anomaly_score: f64,
    /// Combined health score: 1.0 - anomaly_score.
    pub health_score: f64,
    /// Per-network results.
    pub fast: CfcDrillingResult,
    pub slow: CfcDrillingResult,
    /// Either network is calibrated.
    pub is_calibrated: bool,
    /// Per-feature surprises from whichever network scored higher.
    pub feature_surprises: Vec<FeatureSurprise>,
    /// Feature sigmas from slow network (stable baseline for formation detection).
    pub feature_sigmas: Vec<(usize, &'static str, f64)>,
    /// Motor outputs from fast network (responsive for regime clustering).
    pub motor_outputs: Vec<f64>,
    /// Fast network's learning rate.
    pub learning_rate: f64,
    /// Total packets processed (from fast network — both see same count).
    pub packets_processed: u64,
}

/// Dual CfC network: fast + slow running in parallel.
#[derive(Debug, Clone)]
pub struct DualCfcNetwork {
    pub fast: CfcNetwork,
    pub slow: CfcNetwork,
}

impl DualCfcNetwork {
    /// Create a new dual network with fast and slow configs.
    /// Uses offset seeds so the two networks have different initial weights.
    pub fn new(seed: u64) -> Self {
        Self {
            fast: CfcNetwork::with_config(seed, CfcNetworkConfig::fast()),
            slow: CfcNetwork::with_config(seed + 100, CfcNetworkConfig::slow()),
        }
    }

    /// Reset both networks from scratch.
    pub fn reset(&mut self) {
        self.fast.reset();
        self.slow.reset();
    }
}

/// Extract the 16 CfC input features from a WITS packet and drilling metrics.
///
/// Primary features (2 sensory neurons each):
/// 0: WOB (klbs)
/// 1: ROP (ft/hr)
/// 2: RPM
/// 3: Torque (kft-lbs)
/// 4: MSE (psi)
/// 5: SPP (psi)
/// 6: D-exponent
/// 7: Hookload (klbs)
///
/// Supplementary features (1 sensory neuron each):
/// 8: ECD (ppg)
/// 9: Flow balance (gpm) = flow_out - flow_in
/// 10: Pit rate (bbl/hr) = pit_volume_change
/// 11: DXC (corrected d-exponent)
/// 12: Pump SPM (strokes/min)
/// 13: Mud weight in (ppg)
/// 14: Gas units
/// 15: Pit volume (bbl)
pub fn extract_features(packet: &WitsPacket, metrics: &DrillingMetrics) -> [f64; NUM_FEATURES] {
    [
        // Primary
        packet.wob,
        packet.rop,
        packet.rpm,
        packet.torque,
        metrics.mse,
        packet.spp,
        metrics.d_exponent,
        packet.hook_load,
        // Supplementary
        packet.ecd,
        packet.flow_balance(),
        metrics.pit_rate,
        metrics.dxc,
        packet.pump_spm,
        packet.mud_weight_in,
        packet.gas_units,
        packet.pit_volume,
    ]
}

/// Update a single CfC network with a new packet and return the result.
pub fn update_from_drilling(
    network: &mut CfcNetwork,
    packet: &WitsPacket,
    metrics: &DrillingMetrics,
    dt: f64,
) -> CfcDrillingResult {
    let features = extract_features(packet, metrics);
    let (_predictions, train_loss) = network.process(&features, dt);

    CfcDrillingResult {
        anomaly_score: network.anomaly_score(),
        health_score: network.health_score(),
        training_loss: train_loss,
        is_calibrated: network.is_calibrated(),
        learning_rate: network.learning_rate(),
        packets_processed: network.packets_processed(),
        avg_loss: network.avg_loss(),
        feature_surprises: network.feature_surprises(),
        feature_sigmas: network.all_feature_sigmas(),
        motor_outputs: network.latest_motor_outputs().map(|s| s.to_vec()).unwrap_or_default(),
    }
}

/// Update the dual CfC network and return the combined result.
pub fn update_dual_from_drilling(
    dual: &mut DualCfcNetwork,
    packet: &WitsPacket,
    metrics: &DrillingMetrics,
    dt: f64,
) -> DualCfcResult {
    let (fast_result, slow_result) = rayon::join(
        || update_from_drilling(&mut dual.fast, packet, metrics, dt),
        || update_from_drilling(&mut dual.slow, packet, metrics, dt),
    );

    let combined_anomaly = fast_result.anomaly_score.max(slow_result.anomaly_score);

    // Feature surprises from whichever network scored higher
    let feature_surprises = if fast_result.anomaly_score >= slow_result.anomaly_score {
        fast_result.feature_surprises.clone()
    } else {
        slow_result.feature_surprises.clone()
    };

    // Feature sigmas from slow network (stable baseline for formation detection)
    let feature_sigmas = slow_result.feature_sigmas.clone();

    // Motor outputs from fast network (responsive for regime clustering)
    let motor_outputs = fast_result.motor_outputs.clone();

    let is_calibrated = fast_result.is_calibrated || slow_result.is_calibrated;

    DualCfcResult {
        anomaly_score: combined_anomaly,
        health_score: 1.0 - combined_anomaly,
        fast: fast_result,
        slow: slow_result,
        is_calibrated,
        feature_surprises,
        feature_sigmas,
        motor_outputs,
        learning_rate: dual.fast.learning_rate(),
        packets_processed: dual.fast.packets_processed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{RigState, Operation, AnomalyCategory};

    fn make_test_packet() -> WitsPacket {
        let mut p = WitsPacket::default();
        p.wob = 25.0;
        p.rop = 60.0;
        p.rpm = 120.0;
        p.torque = 15.0;
        p.spp = 3000.0;
        p.hook_load = 200.0;
        p.ecd = 10.5;
        p.flow_in = 500.0;
        p.flow_out = 505.0;
        p.pit_volume_change = 0.1;
        p.pump_spm = 60.0;
        p.mud_weight_in = 10.5;
        p.gas_units = 20.0;
        p.pit_volume = 800.0;
        p.rig_state = RigState::Drilling;
        p
    }

    fn make_test_metrics() -> DrillingMetrics {
        DrillingMetrics {
            state: RigState::Drilling,
            operation: Operation::ProductionDrilling,
            mse: 30000.0,
            mse_efficiency: 85.0,
            d_exponent: 1.5,
            dxc: 1.3,
            mse_delta_percent: 0.0,
            flow_balance: 5.0,
            pit_rate: 0.1,
            ecd_margin: 3.5,
            torque_delta_percent: 0.0,
            spp_delta: 0.0,
            flow_data_available: true,
            is_anomaly: false,
            anomaly_category: AnomalyCategory::None,
            anomaly_description: None,
            current_formation: None,
            formation_depth_in_ft: None,
        }
    }

    #[test]
    fn test_extract_features() {
        let packet = make_test_packet();
        let metrics = make_test_metrics();
        let features = extract_features(&packet, &metrics);

        assert_eq!(features.len(), NUM_FEATURES);
        assert_eq!(features[0], 25.0); // WOB
        assert_eq!(features[1], 60.0); // ROP
        assert_eq!(features[4], 30000.0); // MSE
        assert_eq!(features[9], 5.0); // flow_balance
        assert_eq!(features[12], 60.0); // pump_spm
        assert_eq!(features[13], 10.5); // mud_weight_in
        assert_eq!(features[14], 20.0); // gas_units
        assert_eq!(features[15], 800.0); // pit_volume
    }

    #[test]
    fn test_update_from_drilling() {
        let mut net = CfcNetwork::new(42);
        let packet = make_test_packet();
        let metrics = make_test_metrics();

        let result = update_from_drilling(&mut net, &packet, &metrics, 1.0);

        assert!(!result.is_calibrated);
        assert_eq!(result.anomaly_score, 0.0); // Not calibrated yet
        assert_eq!(result.health_score, 1.0);
        assert!(result.training_loss.is_none()); // First packet
        assert_eq!(result.packets_processed, 1);
    }

    #[test]
    fn test_cfc_end_to_end() {
        let mut net = CfcNetwork::new(42);
        let packet = make_test_packet();
        let metrics = make_test_metrics();

        // Process multiple packets
        for _ in 0..10 {
            let result = update_from_drilling(&mut net, &packet, &metrics, 1.0);
            assert!(result.anomaly_score >= 0.0 && result.anomaly_score <= 1.0);
        }

        assert_eq!(net.packets_processed(), 10);
        assert!(net.avg_loss().is_finite());
    }

    #[test]
    fn test_dual_network_basic() {
        let mut dual = DualCfcNetwork::new(42);
        let packet = make_test_packet();
        let metrics = make_test_metrics();

        let result = update_dual_from_drilling(&mut dual, &packet, &metrics, 1.0);

        assert!(!result.is_calibrated);
        assert_eq!(result.anomaly_score, 0.0); // Not calibrated yet
        assert_eq!(result.health_score, 1.0);
        assert_eq!(result.packets_processed, 1);
    }

    #[test]
    fn test_dual_network_processes_both() {
        let mut dual = DualCfcNetwork::new(42);
        let packet = make_test_packet();
        let metrics = make_test_metrics();

        for _ in 0..10 {
            let result = update_dual_from_drilling(&mut dual, &packet, &metrics, 1.0);
            assert!(result.anomaly_score >= 0.0 && result.anomaly_score <= 1.0);
            assert_eq!(result.fast.packets_processed, result.slow.packets_processed);
        }

        assert_eq!(dual.fast.packets_processed(), 10);
        assert_eq!(dual.slow.packets_processed(), 10);
    }

    #[test]
    fn test_dual_network_combined_score_is_max() {
        let mut dual = DualCfcNetwork::new(42);
        let packet = make_test_packet();
        let metrics = make_test_metrics();

        // Process enough packets so scores can be meaningful
        for _ in 0..10 {
            let result = update_dual_from_drilling(&mut dual, &packet, &metrics, 1.0);
            // Combined score should be max of the two (both 0.0 when uncalibrated)
            assert!(
                (result.anomaly_score - result.fast.anomaly_score.max(result.slow.anomaly_score)).abs() < 1e-12,
                "Combined score should be max(fast, slow)"
            );
        }
    }

    #[test]
    fn test_dual_network_reset() {
        let mut dual = DualCfcNetwork::new(42);
        let packet = make_test_packet();
        let metrics = make_test_metrics();

        for _ in 0..5 {
            update_dual_from_drilling(&mut dual, &packet, &metrics, 1.0);
        }

        dual.reset();
        assert_eq!(dual.fast.packets_processed(), 0);
        assert_eq!(dual.slow.packets_processed(), 0);
    }
}
