//! CfC checkpoint types for federated weight sharing.
//!
//! Provides serializable snapshots of dual CfC network state, enabling:
//! - Disk persistence (atomic save/load)
//! - Hub upload for federated averaging
//! - Network restoration from federated models

use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;

use crate::cfc::cell::CfcWeights;
use crate::cfc::network::{CfcNetwork, CfcNetworkConfig};
use crate::cfc::normalizer::OnlineNormalizer;
use crate::cfc::training::AdamOptimizer;

/// Complete snapshot of both CfC networks (fast + slow).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DualCfcCheckpoint {
    /// Format version for forward compatibility.
    pub version: u32,
    /// Fast network checkpoint.
    pub fast: CfcNetworkCheckpoint,
    /// Slow network checkpoint.
    pub slow: CfcNetworkCheckpoint,
    /// Metadata about the source rig and training state.
    pub metadata: CheckpointMetadata,
}

/// Snapshot of a single CfC network's trainable state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfcNetworkCheckpoint {
    /// Network configuration (topology + training hyperparams).
    pub config: CfcNetworkConfig,
    /// Seed used for wiring generation.
    pub seed: u64,
    /// Learned weights and biases.
    pub weights: CfcWeights,
    /// Online normalizer state (Welford running stats).
    pub normalizer: OnlineNormalizer,
    /// Adam optimizer state (momentum vectors).
    pub optimizer: AdamOptimizer,
    /// Number of packets this network has processed.
    pub packets_processed: u64,
    /// EMA of prediction error (anomaly scoring baseline).
    pub error_ema: f64,
}

/// Metadata attached to a checkpoint for provenance tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointMetadata {
    /// Rig identifier that produced this checkpoint.
    pub rig_id: String,
    /// Well identifier.
    pub well_id: String,
    /// Unix timestamp when checkpoint was created.
    pub timestamp: u64,
    /// Total packets processed across both networks.
    pub packets_processed: u64,
    /// Average training loss (fast network).
    pub avg_loss: f64,
    /// Whether the fast network is calibrated.
    pub is_calibrated: bool,
}

impl CfcNetwork {
    /// Create a serializable snapshot of this network's current state.
    pub fn snapshot(&self) -> CfcNetworkCheckpoint {
        CfcNetworkCheckpoint {
            config: self.config().clone(),
            seed: self.seed(),
            weights: self.weights().clone(),
            normalizer: self.normalizer().clone(),
            optimizer: self.optimizer().clone(),
            packets_processed: self.packets_processed(),
            error_ema: self.error_ema(),
        }
    }

    /// Restore network state from a checkpoint.
    ///
    /// Validates that the checkpoint is compatible (same neuron count and
    /// weight dimensions). Resets hidden state after restoration.
    pub fn restore_from(&mut self, cp: &CfcNetworkCheckpoint) -> Result<(), String> {
        // Validate neuron topology matches
        if cp.config.ncp.num_neurons != self.config().ncp.num_neurons {
            return Err(format!(
                "neuron count mismatch: checkpoint has {}, network has {}",
                cp.config.ncp.num_neurons,
                self.config().ncp.num_neurons,
            ));
        }

        // Validate weight dimensions match
        if cp.weights.num_params() != self.weights().num_params() {
            return Err(format!(
                "weight dimension mismatch: checkpoint has {} params, network has {}",
                cp.weights.num_params(),
                self.weights().num_params(),
            ));
        }

        self.set_weights(cp.weights.clone());
        self.set_normalizer(cp.normalizer.clone());
        self.set_optimizer(cp.optimizer.clone());
        self.set_packets_processed(cp.packets_processed);
        self.set_error_ema(cp.error_ema);
        self.reset_state();
        Ok(())
    }
}

/// Save a checkpoint to disk atomically (write temp file, then rename).
pub fn save_to_disk(cp: &DualCfcCheckpoint, path: &Path) -> io::Result<()> {
    let json = serde_json::to_vec(cp)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    // Write to temp file alongside the target
    let tmp_path = path.with_extension("json.tmp");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&tmp_path, &json)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

/// Load a checkpoint from disk.
pub fn load_from_disk(path: &Path) -> io::Result<DualCfcCheckpoint> {
    let data = std::fs::read(path)?;
    serde_json::from_slice(&data)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfc::{CfcNetwork, CfcNetworkConfig, DualCfcNetwork};
    use crate::cfc::normalizer::NUM_FEATURES;

    #[test]
    fn test_serde_round_trip() {
        let mut net = CfcNetwork::with_config(42, CfcNetworkConfig::fast());
        let features = [10.0; NUM_FEATURES];
        for _ in 0..5 {
            net.process(&features, 1.0);
        }

        let cp = net.snapshot();
        let json = serde_json::to_string(&cp).expect("serialize");
        let restored: CfcNetworkCheckpoint = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(restored.packets_processed, 5);
        assert_eq!(restored.config.ncp.num_neurons, cp.config.ncp.num_neurons);
        assert_eq!(restored.weights.w_tau.len(), cp.weights.w_tau.len());
    }

    #[test]
    fn test_restore_produces_same_output() {
        let mut net_a = CfcNetwork::with_config(42, CfcNetworkConfig::fast());
        let features = [10.0, 50.0, 120.0, 15.0, 30000.0, 3000.0,
                        1.5, 200.0, 10.5, 5.0, 0.1, 1.3,
                        60.0, 10.5, 20.0, 800.0];
        for _ in 0..20 {
            net_a.process(&features, 1.0);
        }

        let cp = net_a.snapshot();

        // Must use same seed so NCP wiring matches (wiring is deterministic from seed)
        let mut net_b = CfcNetwork::with_config(42, CfcNetworkConfig::fast());
        net_b.restore_from(&cp).expect("restore should succeed");

        // Process same packet on both — predictions should be identical
        let (pred_a, _) = net_a.process(&features, 1.0);
        let (pred_b, _) = net_b.process(&features, 1.0);

        for (a, b) in pred_a.iter().zip(pred_b.iter()) {
            assert!(
                (a - b).abs() < 1e-10,
                "predictions diverged: {} vs {}",
                a, b,
            );
        }
    }

    #[test]
    fn test_reject_mismatched_neuron_count() {
        let net_64 = CfcNetwork::with_config(42, CfcNetworkConfig::fast());
        let cp = net_64.snapshot();

        let mut net_128 = CfcNetwork::new(42); // 128-neuron legacy
        let result = net_128.restore_from(&cp);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("neuron count mismatch"));
    }

    #[test]
    fn test_dual_checkpoint_round_trip() {
        let mut dual = DualCfcNetwork::new(42);
        let features = [10.0; NUM_FEATURES];
        let metrics = crate::cfc::tests::make_test_metrics();
        let packet = crate::cfc::tests::make_test_packet();
        for _ in 0..10 {
            crate::cfc::update_dual_from_drilling(&mut dual, &packet, &metrics, 1.0);
        }

        let cp = dual.snapshot("rig-1", "well-1");
        assert_eq!(cp.version, 1);
        assert_eq!(cp.metadata.rig_id, "rig-1");
        assert_eq!(cp.metadata.packets_processed, 10);

        let json = serde_json::to_string(&cp).expect("serialize");
        let restored: DualCfcCheckpoint = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.fast.packets_processed, 10);
        assert_eq!(restored.slow.packets_processed, 10);
    }

    #[test]
    fn test_disk_persistence() {
        let dual = DualCfcNetwork::new(42);
        let cp = dual.snapshot("rig-1", "well-1");

        let dir = tempfile::tempdir().expect("tmpdir");
        let path = dir.path().join("test_checkpoint.json");

        save_to_disk(&cp, &path).expect("save");
        let loaded = load_from_disk(&path).expect("load");

        assert_eq!(loaded.version, cp.version);
        assert_eq!(loaded.metadata.rig_id, "rig-1");
        assert_eq!(loaded.fast.config.ncp.num_neurons, cp.fast.config.ncp.num_neurons);
    }
}
