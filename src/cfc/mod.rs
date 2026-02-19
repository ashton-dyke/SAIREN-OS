//! CfC (Closed-form Continuous-time) Neural Network Operations Specialist.
//!
//! A 128-neuron CfC/NCP neural network that runs in **shadow mode**:
//! it logs predictions alongside the rule-based system but does not
//! influence ticket severity decisions.
//!
//! The network is **self-supervised** — it predicts next-timestep sensor
//! values and treats prediction error as an anomaly signal. No labeled
//! training data is needed.
//!
//! ## Architecture
//!
//! - 128 CfC neurons with NCP sparse wiring (~30% connectivity)
//! - 12 input features: WOB, ROP, RPM, torque, MSE, SPP, d-exponent,
//!   hookload, ECD, flow_balance, pit_rate, DXC
//! - NCP groups: 24 sensory → 64 inter → 32 command → 8 motor
//! - Outputs: 12 next-step predictions, anomaly score (0-1), health score (0-1)
//! - Online training: forward → predict → compare → backprop → SGD, every packet

pub mod normalizer;
pub mod wiring;
pub mod cell;
pub mod training;
pub mod network;

pub use network::{CfcNetwork, FeatureSurprise};
pub use normalizer::NUM_FEATURES;

use crate::types::{DrillingMetrics, WitsPacket};

/// Result of CfC processing for one packet (shadow mode output).
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

/// Update the CfC network with a new packet and return the shadow result.
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
            is_anomaly: false,
            anomaly_category: AnomalyCategory::None,
            anomaly_description: None,
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
}
