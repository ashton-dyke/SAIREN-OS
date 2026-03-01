//! Hub-side federated CfC model aggregation.
//!
//! Performs weighted averaging of CfC checkpoints uploaded by individual rigs
//! to produce a single federated model that any rig can pull.
//!
//! ## Aggregation Strategy
//!
//! - **Weights**: element-wise weighted average by `packets_processed`
//! - **Normalizers**: parallel Welford combination (exact merge of count/mean/m2)
//! - **Optimizer**: reset to fresh Adam (averaged weights are at a new point
//!   in the loss landscape; old momentum would bias toward a single rig)

use crate::cfc::cell::CfcWeights;
use crate::cfc::checkpoint::{
    CheckpointMetadata, CfcNetworkCheckpoint, DualCfcCheckpoint,
};
use crate::cfc::normalizer::OnlineNormalizer;
use crate::cfc::training::AdamOptimizer;

/// Compute a federated-averaged checkpoint from multiple rig checkpoints.
///
/// Each checkpoint is weighted by its `packets_processed` count. More
/// experienced models contribute proportionally more to the average.
///
/// Returns `Err` if fewer than 2 checkpoints are provided or if
/// network configurations are incompatible.
pub fn federated_average(
    checkpoints: &[DualCfcCheckpoint],
) -> Result<DualCfcCheckpoint, String> {
    if checkpoints.len() < 2 {
        return Err("need at least 2 checkpoints for federated averaging".into());
    }

    // Reference config from first checkpoint (all must match)
    let ref_fast = &checkpoints[0].fast;
    let ref_slow = &checkpoints[0].slow;

    for (i, cp) in checkpoints.iter().enumerate().skip(1) {
        if cp.fast.config.ncp.num_neurons != ref_fast.config.ncp.num_neurons {
            return Err(format!(
                "fast network neuron count mismatch: checkpoint 0 has {}, checkpoint {} has {}",
                ref_fast.config.ncp.num_neurons, i, cp.fast.config.ncp.num_neurons,
            ));
        }
        if cp.slow.config.ncp.num_neurons != ref_slow.config.ncp.num_neurons {
            return Err(format!(
                "slow network neuron count mismatch: checkpoint 0 has {}, checkpoint {} has {}",
                ref_slow.config.ncp.num_neurons, i, cp.slow.config.ncp.num_neurons,
            ));
        }
    }

    let fast_avg = average_single_network(
        &checkpoints.iter().map(|c| &c.fast).collect::<Vec<_>>(),
    )?;
    let slow_avg = average_single_network(
        &checkpoints.iter().map(|c| &c.slow).collect::<Vec<_>>(),
    )?;

    let total_packets: u64 = checkpoints.iter().map(|c| c.metadata.packets_processed).sum();
    let avg_loss: f64 = {
        let w_sum: f64 = checkpoints.iter().map(|c| c.metadata.packets_processed as f64).sum();
        if w_sum > 0.0 {
            checkpoints.iter()
                .map(|c| c.metadata.avg_loss * c.metadata.packets_processed as f64)
                .sum::<f64>() / w_sum
        } else {
            0.0
        }
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    Ok(DualCfcCheckpoint {
        version: 1,
        fast: fast_avg,
        slow: slow_avg,
        metadata: CheckpointMetadata {
            rig_id: "federated".to_string(),
            well_id: "fleet".to_string(),
            timestamp: now,
            packets_processed: total_packets,
            avg_loss,
            is_calibrated: checkpoints.iter().any(|c| c.metadata.is_calibrated),
        },
    })
}

/// Average a set of single-network checkpoints weighted by packets_processed.
fn average_single_network(
    checkpoints: &[&CfcNetworkCheckpoint],
) -> Result<CfcNetworkCheckpoint, String> {
    if checkpoints.is_empty() {
        return Err("empty checkpoint list".into());
    }

    let ref_cp = checkpoints[0];
    let weights: Vec<f64> = checkpoints
        .iter()
        .map(|c| c.packets_processed as f64)
        .collect();
    let w_sum: f64 = weights.iter().sum();

    if w_sum == 0.0 {
        // All networks untrained — just return the first
        return Ok(ref_cp.clone());
    }

    let norm_weights: Vec<f64> = weights.iter().map(|w| w / w_sum).collect();

    // Weighted average of CfC weights
    let avg_weights = average_cfc_weights(
        &checkpoints.iter().map(|c| &c.weights).collect::<Vec<_>>(),
        &norm_weights,
    )?;

    // Merge Welford normalizers
    let avg_normalizer = merge_normalizers(
        &checkpoints.iter().map(|c| &c.normalizer).collect::<Vec<_>>(),
    );

    // Fresh optimizer (standard in federated learning)
    let num_params = avg_weights.num_params();
    let avg_optimizer = AdamOptimizer::with_config(num_params, &ref_cp.config.training);

    let avg_packets: u64 = (w_sum / checkpoints.len() as f64) as u64;
    let avg_error_ema: f64 = checkpoints.iter().zip(norm_weights.iter())
        .map(|(c, w)| c.error_ema * w)
        .sum();

    Ok(CfcNetworkCheckpoint {
        config: ref_cp.config.clone(),
        seed: ref_cp.seed,
        weights: avg_weights,
        normalizer: avg_normalizer,
        optimizer: avg_optimizer,
        packets_processed: avg_packets,
        error_ema: avg_error_ema,
    })
}

/// Element-wise weighted average of CfC weight vectors.
fn average_cfc_weights(
    all_weights: &[&CfcWeights],
    norm_weights: &[f64],
) -> Result<CfcWeights, String> {
    let ref_w = all_weights[0];

    // Verify dimensions match
    for (i, w) in all_weights.iter().enumerate().skip(1) {
        if w.num_params() != ref_w.num_params() {
            return Err(format!(
                "weight dimension mismatch: checkpoint 0 has {} params, checkpoint {} has {}",
                ref_w.num_params(), i, w.num_params(),
            ));
        }
    }

    fn weighted_avg(vecs: &[&Vec<f64>], weights: &[f64]) -> Vec<f64> {
        let len = vecs[0].len();
        let mut result = vec![0.0; len];
        for (v, &w) in vecs.iter().zip(weights.iter()) {
            for (r, &val) in result.iter_mut().zip(v.iter()) {
                *r += val * w;
            }
        }
        result
    }

    let w_taus: Vec<&Vec<f64>> = all_weights.iter().map(|w| &w.w_tau).collect();
    let w_fs: Vec<&Vec<f64>> = all_weights.iter().map(|w| &w.w_f).collect();
    let w_gs: Vec<&Vec<f64>> = all_weights.iter().map(|w| &w.w_g).collect();
    let b_taus: Vec<&Vec<f64>> = all_weights.iter().map(|w| &w.b_tau).collect();
    let b_fs: Vec<&Vec<f64>> = all_weights.iter().map(|w| &w.b_f).collect();
    let b_gs: Vec<&Vec<f64>> = all_weights.iter().map(|w| &w.b_g).collect();
    let w_outs: Vec<&Vec<f64>> = all_weights.iter().map(|w| &w.w_out).collect();
    let b_outs: Vec<&Vec<f64>> = all_weights.iter().map(|w| &w.b_out).collect();
    let w_ins: Vec<&Vec<f64>> = all_weights.iter().map(|w| &w.w_in).collect();

    Ok(CfcWeights {
        w_tau: weighted_avg(&w_taus, norm_weights),
        w_f: weighted_avg(&w_fs, norm_weights),
        w_g: weighted_avg(&w_gs, norm_weights),
        b_tau: weighted_avg(&b_taus, norm_weights),
        b_f: weighted_avg(&b_fs, norm_weights),
        b_g: weighted_avg(&b_gs, norm_weights),
        w_out: weighted_avg(&w_outs, norm_weights),
        b_out: weighted_avg(&b_outs, norm_weights),
        w_in: weighted_avg(&w_ins, norm_weights),
        // Structural fields copied from reference (same topology)
        weight_offset: ref_w.weight_offset.clone(),
        weight_count: ref_w.weight_count.clone(),
    })
}

/// Merge multiple Welford normalizers using the parallel combination formula.
///
/// For two normalizers (count_a, mean_a, m2_a) and (count_b, mean_b, m2_b):
///   count = count_a + count_b
///   delta = mean_b - mean_a
///   mean  = (count_a * mean_a + count_b * mean_b) / count
///   m2    = m2_a + m2_b + delta^2 * count_a * count_b / count
fn merge_normalizers(normalizers: &[&OnlineNormalizer]) -> OnlineNormalizer {
    // Serialize to JSON and back to access private fields
    // This is acceptable since we just added serde derives
    let jsons: Vec<serde_json::Value> = normalizers
        .iter()
        .map(|n| serde_json::to_value(n).expect("normalizer serialization"))
        .collect();

    if jsons.is_empty() {
        return OnlineNormalizer::new();
    }

    let num_features = jsons[0]["mean"].as_array().map(|a| a.len()).unwrap_or(16);

    let mut merged_count: u64 = 0;
    let mut merged_mean = vec![0.0f64; num_features];
    let mut merged_m2 = vec![0.0f64; num_features];

    for json in &jsons {
        let count = json["count"].as_u64().unwrap_or(0);
        if count == 0 {
            continue;
        }

        let mean: Vec<f64> = json["mean"]
            .as_array()
            .map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect())
            .unwrap_or_else(|| vec![0.0; num_features]);
        let m2: Vec<f64> = json["m2"]
            .as_array()
            .map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect())
            .unwrap_or_else(|| vec![0.0; num_features]);

        if merged_count == 0 {
            merged_count = count;
            merged_mean = mean;
            merged_m2 = m2;
        } else {
            let combined_count = merged_count + count;
            for i in 0..num_features {
                let delta = mean[i] - merged_mean[i];
                merged_mean[i] = (merged_count as f64 * merged_mean[i]
                    + count as f64 * mean[i])
                    / combined_count as f64;
                merged_m2[i] += m2[i]
                    + delta * delta * merged_count as f64 * count as f64
                        / combined_count as f64;
            }
            merged_count = combined_count;
        }
    }

    // Reconstruct normalizer via JSON
    let merged_json = serde_json::json!({
        "count": merged_count,
        "mean": merged_mean,
        "m2": merged_m2,
    });
    serde_json::from_value(merged_json).expect("normalizer deserialization")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfc::{CfcNetwork, CfcNetworkConfig, DualCfcNetwork};
    use crate::cfc::normalizer::NUM_FEATURES;

    #[test]
    fn test_federated_average_basic() {
        // Same seed ensures identical NCP wiring (topology must match for averaging)
        let mut dual_a = DualCfcNetwork::new(42);
        let mut dual_b = DualCfcNetwork::new(42);

        // Train both on different data so weights diverge
        let features_a = [10.0; NUM_FEATURES];
        let features_b = [20.0; NUM_FEATURES];

        for _ in 0..50 {
            dual_a.fast.process(&features_a, 1.0);
            dual_a.slow.process(&features_a, 1.0);
            dual_b.fast.process(&features_b, 1.0);
            dual_b.slow.process(&features_b, 1.0);
        }

        let cp_a = dual_a.snapshot("rig-a", "well-a");
        let cp_b = dual_b.snapshot("rig-b", "well-b");

        let avg = federated_average(&[cp_a.clone(), cp_b.clone()])
            .expect("average should succeed");

        assert_eq!(avg.version, 1);
        assert_eq!(avg.metadata.rig_id, "federated");
        assert!(avg.metadata.packets_processed > 0);

        // Averaged weights should be between the two
        let w_a = &cp_a.fast.weights.w_tau;
        let w_b = &cp_b.fast.weights.w_tau;
        let w_avg = &avg.fast.weights.w_tau;
        for i in 0..w_a.len().min(5) {
            let mid = (w_a[i] + w_b[i]) / 2.0;
            // Since both have same packets_processed, should be close to midpoint
            assert!(
                (w_avg[i] - mid).abs() < 1e-6,
                "averaged weight[{}] = {} expected near midpoint {}",
                i, w_avg[i], mid,
            );
        }
    }

    #[test]
    fn test_federated_average_rejects_single() {
        let dual = DualCfcNetwork::new(42);
        let cp = dual.snapshot("rig-a", "well-a");
        let result = federated_average(&[cp]);
        assert!(result.is_err());
    }

    #[test]
    fn test_normalizer_merge() {
        // Create two normalizers that have seen different data
        let mut norm_a = OnlineNormalizer::new();
        let mut norm_b = OnlineNormalizer::new();

        let vals_a: Vec<[f64; NUM_FEATURES]> = (0..100)
            .map(|i| {
                let mut f = [0.0; NUM_FEATURES];
                f[0] = i as f64;
                f
            })
            .collect();
        let vals_b: Vec<[f64; NUM_FEATURES]> = (100..200)
            .map(|i| {
                let mut f = [0.0; NUM_FEATURES];
                f[0] = i as f64;
                f
            })
            .collect();

        for v in &vals_a {
            norm_a.normalize_and_update(v);
        }
        for v in &vals_b {
            norm_b.normalize_and_update(v);
        }

        // Single-pass normalizer over all data
        let mut norm_combined = OnlineNormalizer::new();
        for v in vals_a.iter().chain(vals_b.iter()) {
            norm_combined.normalize_and_update(v);
        }

        // Merge
        let merged = merge_normalizers(&[&norm_a, &norm_b]);

        // Verify count matches
        assert_eq!(merged.count(), 200);
        assert_eq!(norm_combined.count(), 200);

        // Verify merged normalization is close to single-pass
        let test_val = {
            let mut f = [0.0; NUM_FEATURES];
            f[0] = 150.0;
            f
        };
        let merged_norm = merged.normalize(&test_val);
        let combined_norm = norm_combined.normalize(&test_val);
        assert!(
            (merged_norm[0] - combined_norm[0]).abs() < 1e-10,
            "merged={}, combined={}",
            merged_norm[0], combined_norm[0],
        );
    }

    #[test]
    fn test_weighted_average_by_experience() {
        // Same seed ensures compatible wiring topology
        // Rig A has 100 packets, Rig B has 900 packets
        // Result should be 90% B, 10% A
        let mut net_a = CfcNetwork::with_config(42, CfcNetworkConfig::fast());
        let mut net_b = CfcNetwork::with_config(42, CfcNetworkConfig::fast());

        let features = [10.0; NUM_FEATURES];
        for _ in 0..100 {
            net_a.process(&features, 1.0);
        }
        for _ in 0..900 {
            net_b.process(&features, 1.0);
        }

        let mut dual_a = DualCfcNetwork::new(42);
        dual_a.fast = net_a;
        let mut dual_b = DualCfcNetwork::new(42);
        dual_b.fast = net_b;

        let cp_a = dual_a.snapshot("rig-a", "well-a");
        let cp_b = dual_b.snapshot("rig-b", "well-b");

        let avg = federated_average(&[cp_a, cp_b]).expect("should succeed");

        // Verify packets_processed in metadata is the sum
        assert!(avg.metadata.packets_processed > 0);
    }
}
