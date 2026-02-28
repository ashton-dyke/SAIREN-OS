//! NCP (Neural Circuit Policy) sparse wiring generation.
//!
//! Generates deterministic sparse connectivity for the CfC network using
//! a seeded PRNG. The wiring follows the NCP architecture:
//!
//! - **Sensory neurons** (0..24): receive input features
//! - **Inter neurons** (24..88): hidden processing layer
//! - **Command neurons** (88..120): decision/integration layer
//! - **Motor neurons** (120..128): produce output
//!
//! Input mapping (16 features → 24 sensory neurons):
//! - Features 0-7 (primary): 2 sensory neurons each (16 neurons)
//! - Features 8-15 (supplementary): 1 sensory neuron each (8 neurons)
//!
//! Connections flow forward through groups with ~30% connectivity density.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use crate::cfc::normalizer::NUM_FEATURES;

/// Total number of CfC neurons (legacy 128-neuron layout).
pub const NUM_NEURONS: usize = 128;

/// NCP group boundaries (legacy 128-neuron layout).
pub const SENSORY_START: usize = 0;
pub const SENSORY_END: usize = 24;
pub const INTER_START: usize = 24;
pub const INTER_END: usize = 88;
pub const COMMAND_START: usize = 88;
pub const COMMAND_END: usize = 120;
pub const MOTOR_START: usize = 120;
pub const MOTOR_END: usize = 128;

/// Number of motor neurons (output dimension before projection).
pub const NUM_MOTOR: usize = MOTOR_END - MOTOR_START;

/// Number of network outputs (next-step predictions for each input feature).
pub const NUM_OUTPUTS: usize = NUM_FEATURES;

/// Number of primary features (get 2 sensory neurons each).
const NUM_PRIMARY: usize = 8;

/// Configuration for NCP neuron topology — group boundaries and sizes.
#[derive(Debug, Clone)]
pub struct NcpConfig {
    pub num_neurons: usize,
    pub sensory_end: usize,    // sensory = 0..sensory_end
    pub inter_end: usize,      // inter = sensory_end..inter_end
    pub command_end: usize,    // command = inter_end..command_end
    // motor = command_end..num_neurons
}

impl NcpConfig {
    /// Legacy 128-neuron layout: 24 sensory, 64 inter, 32 command, 8 motor.
    pub fn default_128() -> Self {
        Self {
            num_neurons: 128,
            sensory_end: 24,
            inter_end: 88,
            command_end: 120,
        }
    }

    /// Compact 64-neuron layout for the dual-network architecture.
    /// 24 sensory (dictated by 16 features → 24 input mappings),
    /// 20 inter, 12 command, 8 motor (regime clusterer expects DIM=8).
    pub fn dual_64() -> Self {
        Self {
            num_neurons: 64,
            sensory_end: 24,
            inter_end: 44,   // 24 + 20 inter
            command_end: 56,  // 44 + 12 command
            // motor = 56..64 = 8 motor
        }
    }

    /// Number of motor neurons.
    #[inline]
    pub fn num_motor(&self) -> usize {
        self.num_neurons - self.command_end
    }
}

/// Sparse wiring configuration for the NCP network.
#[derive(Debug, Clone)]
pub struct NcpWiring {
    /// Neuron topology configuration.
    pub config: NcpConfig,

    /// For each neuron, which other neurons connect TO it.
    /// `incoming[i]` = list of source neuron indices that feed into neuron i.
    pub incoming: Vec<Vec<usize>>,

    /// Dense adjacency matrix [N * N], row-major.
    /// adj[src * N + dst] = true if src→dst connection exists.
    pub adj: Vec<bool>,

    /// Input mapping: which sensory neuron each input feature maps to.
    /// input_map[feature_idx] = list of sensory neuron indices.
    /// Primary features (0-7) get 2 neurons, supplementary (8-15) get 1.
    pub input_map: Vec<Vec<usize>>,

    /// Total number of input weight entries (sum of input_map lengths).
    pub total_input_weights: usize,

    /// Total number of active connections.
    pub num_connections: usize,
}

impl NcpWiring {
    /// Generate NCP wiring with the legacy 128-neuron layout.
    ///
    /// Target density: ~30% within each group-to-group connection layer.
    /// Connections: input→sensory, sensory→inter, inter→command, command→motor.
    pub fn generate(seed: u64) -> Self {
        Self::generate_with_config(seed, &NcpConfig::default_128())
    }

    /// Generate NCP wiring with a given neuron topology configuration.
    pub fn generate_with_config(seed: u64, config: &NcpConfig) -> Self {
        let n = config.num_neurons;
        let mut rng = StdRng::seed_from_u64(seed);
        let mut adj = vec![false; n * n];
        let mut incoming: Vec<Vec<usize>> = vec![Vec::new(); n];

        let density = 0.30;
        let mut num_connections = 0usize;

        let sensory_start = 0;
        let sensory_end = config.sensory_end;
        let inter_start = config.sensory_end;
        let inter_end = config.inter_end;
        let command_start = config.inter_end;
        let command_end = config.command_end;
        let motor_start = config.command_end;
        let motor_end = config.num_neurons;

        // Helper: connect group src_range → dst_range with given density
        let mut connect = |src_start: usize, src_end: usize,
                           dst_start: usize, dst_end: usize,
                           rng: &mut StdRng| -> usize {
            let mut count = 0;
            for dst in dst_start..dst_end {
                for src in src_start..src_end {
                    if rng.gen::<f64>() < density {
                        let idx = src * n + dst;
                        if !adj[idx] {
                            adj[idx] = true;
                            incoming[dst].push(src);
                            count += 1;
                        }
                    }
                }
                // Ensure every neuron has at least one incoming connection
                if incoming[dst].is_empty() || incoming[dst].iter().all(|&s| s < src_start || s >= src_end) {
                    let src = src_start + (rng.gen::<usize>() % (src_end - src_start));
                    let idx = src * n + dst;
                    if !adj[idx] {
                        adj[idx] = true;
                        incoming[dst].push(src);
                        count += 1;
                    }
                }
            }
            count
        };

        // Layer 1: Sensory → Inter
        num_connections += connect(sensory_start, sensory_end, inter_start, inter_end, &mut rng);

        // Layer 2: Inter → Command
        num_connections += connect(inter_start, inter_end, command_start, command_end, &mut rng);

        // Layer 3: Command → Motor
        num_connections += connect(command_start, command_end, motor_start, motor_end, &mut rng);

        // Recurrent connections within inter and command groups (~15% density)
        let recurrent_density = 0.15;
        for group in [(inter_start, inter_end), (command_start, command_end)] {
            for dst in group.0..group.1 {
                for src in group.0..group.1 {
                    if src != dst && rng.gen::<f64>() < recurrent_density {
                        let idx = src * n + dst;
                        if !adj[idx] {
                            adj[idx] = true;
                            incoming[dst].push(src);
                            num_connections += 1;
                        }
                    }
                }
            }
        }

        // Input mapping: distribute features across sensory neurons
        // Primary features (0..8) get 2 sensory neurons each (16 neurons total)
        // Supplementary features (8..16) get 1 sensory neuron each (8 neurons total)
        // Total: 16 + 8 = 24 sensory neurons
        let mut input_map: Vec<Vec<usize>> = vec![Vec::new(); NUM_FEATURES];
        let mut neuron_cursor = sensory_start;

        for (feat_idx, map) in input_map.iter_mut().enumerate() {
            if feat_idx < NUM_PRIMARY {
                // Primary: 2 sensory neurons
                map.push(neuron_cursor);
                map.push(neuron_cursor + 1);
                neuron_cursor += 2;
            } else {
                // Supplementary: 1 sensory neuron
                map.push(neuron_cursor);
                neuron_cursor += 1;
            }
        }
        debug_assert_eq!(neuron_cursor, sensory_end);

        let total_input_weights = input_map.iter().map(|m| m.len()).sum();

        NcpWiring {
            config: config.clone(),
            incoming,
            adj,
            input_map,
            total_input_weights,
            num_connections,
        }
    }

    /// Number of neurons in this wiring configuration.
    #[inline]
    pub fn num_neurons(&self) -> usize {
        self.config.num_neurons
    }

    /// Check if a connection from src to dst exists.
    #[inline]
    pub fn is_connected(&self, src: usize, dst: usize) -> bool {
        self.adj[src * self.config.num_neurons + dst]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wiring_deterministic() {
        let w1 = NcpWiring::generate(42);
        let w2 = NcpWiring::generate(42);
        assert_eq!(w1.num_connections, w2.num_connections);
        assert_eq!(w1.adj, w2.adj);
    }

    #[test]
    fn test_wiring_connectivity() {
        let w = NcpWiring::generate(42);
        assert!(w.num_connections > 500, "too few connections: {}", w.num_connections);
        assert!(w.num_connections < 10000, "too many connections: {}", w.num_connections);

        for i in INTER_START..INTER_END {
            assert!(!w.incoming[i].is_empty(), "inter neuron {} has no inputs", i);
        }
        for i in MOTOR_START..MOTOR_END {
            assert!(!w.incoming[i].is_empty(), "motor neuron {} has no inputs", i);
        }
    }

    #[test]
    fn test_input_mapping() {
        let w = NcpWiring::generate(42);
        assert_eq!(w.input_map.len(), NUM_FEATURES);

        // Primary features get 2 sensory neurons
        for i in 0..NUM_PRIMARY {
            assert_eq!(w.input_map[i].len(), 2, "primary feature {} should map to 2 neurons", i);
        }
        // Supplementary features get 1 sensory neuron
        for i in NUM_PRIMARY..NUM_FEATURES {
            assert_eq!(w.input_map[i].len(), 1, "supplementary feature {} should map to 1 neuron", i);
        }

        // All mapped neurons should be in the sensory range
        for map in &w.input_map {
            for &neuron in map {
                assert!(neuron >= SENSORY_START && neuron < SENSORY_END);
            }
        }

        // Total input weights: 8*2 + 8*1 = 24
        assert_eq!(w.total_input_weights, 24);
    }

    #[test]
    fn test_ncp_config_dual_64() {
        let cfg = NcpConfig::dual_64();
        assert_eq!(cfg.num_neurons, 64);
        assert_eq!(cfg.sensory_end, 24);
        assert_eq!(cfg.inter_end, 44);   // 24 + 20 inter
        assert_eq!(cfg.command_end, 56);  // 44 + 12 command
        assert_eq!(cfg.num_motor(), 8);   // 64 - 56 = 8 motor
    }

    #[test]
    fn test_generate_with_config_64() {
        let cfg = NcpConfig::dual_64();
        let w = NcpWiring::generate_with_config(42, &cfg);

        assert_eq!(w.num_neurons(), 64);
        assert_eq!(w.adj.len(), 64 * 64);
        assert_eq!(w.incoming.len(), 64);

        // Every inter neuron should have at least one input
        for i in cfg.sensory_end..cfg.inter_end {
            assert!(!w.incoming[i].is_empty(), "inter neuron {} has no inputs", i);
        }
        // Every motor neuron should have at least one input
        for i in cfg.command_end..cfg.num_neurons {
            assert!(!w.incoming[i].is_empty(), "motor neuron {} has no inputs", i);
        }

        // Connection count should be reasonable
        assert!(w.num_connections > 50, "too few connections: {}", w.num_connections);
        assert!(w.num_connections < 5000, "too many connections: {}", w.num_connections);

        // Input mapping still works (24 sensory neurons, same as 128)
        assert_eq!(w.total_input_weights, 24);
    }

    #[test]
    fn test_generate_with_config_deterministic() {
        let cfg = NcpConfig::dual_64();
        let w1 = NcpWiring::generate_with_config(42, &cfg);
        let w2 = NcpWiring::generate_with_config(42, &cfg);
        assert_eq!(w1.num_connections, w2.num_connections);
        assert_eq!(w1.adj, w2.adj);
    }

    #[test]
    fn test_is_connected_uses_config_size() {
        let cfg = NcpConfig::dual_64();
        let w = NcpWiring::generate_with_config(42, &cfg);

        // Should not panic for any valid neuron pair
        for src in 0..64 {
            for dst in 0..64 {
                let _ = w.is_connected(src, dst);
            }
        }
    }
}
