//! System-wide default constants.
//!
//! Centralises magic numbers that were previously scattered across the codebase.
//! Grouped by subsystem for easy discovery.

// ============================================================================
// Pipeline
// ============================================================================

/// History buffer size for the strategic agent (packets).
///
/// 60 packets at 1 Hz = 1 minute of recent context.
pub const HISTORY_BUFFER_SIZE: usize = 60;

/// Interval between periodic advisory summaries (seconds).
pub const PERIODIC_SUMMARY_INTERVAL_SECS: u64 = 600;

/// Minimum packets in the history buffer before a periodic summary is generated.
pub const MIN_PACKETS_FOR_PERIODIC_SUMMARY: usize = 10;

// ============================================================================
// ML Engine
// ============================================================================

/// ML history ring-buffer capacity (packets).
///
/// 7 200 = 2 hours at 1 Hz.
pub const ML_HISTORY_BUFFER_SIZE: usize = 7_200;

/// Minimum number of WITS packets required to run an ML analysis cycle.
pub const MIN_PACKETS_FOR_ML_ANALYSIS: usize = 100;

// ============================================================================
// Simulation
// ============================================================================

/// Base delay denominator for `--speed` flag.
///
/// `delay_ms = SIMULATION_BASE_DELAY_MS / speed`
pub const SIMULATION_BASE_DELAY_MS: u64 = 60_000;

// ============================================================================
// P2P Mesh Gossip
// ============================================================================

/// Default gossip broadcast interval (seconds).
pub const MESH_GOSSIP_INTERVAL_SECS: u64 = 60;

/// Default maximum events per gossip exchange.
pub const MESH_GOSSIP_MAX_EVENTS: usize = 50;

/// Default per-peer HTTP timeout for gossip exchanges (seconds).
pub const MESH_GOSSIP_TIMEOUT_SECS: u64 = 10;
