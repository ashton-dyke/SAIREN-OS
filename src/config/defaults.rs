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

/// Cycle-time warning threshold when LLM GPU inference is available (ms).
pub const CYCLE_TARGET_GPU_MS: u128 = 100;

/// Cycle-time warning threshold when using CPU-only inference (ms).
pub const CYCLE_TARGET_CPU_MS: u128 = 60_000;

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
// Fleet Client
// ============================================================================

/// HTTP client timeout for fleet hub requests (seconds).
pub const FLEET_HTTP_TIMEOUT_SECS: u64 = 30;

/// How often the uploader task drains the queue to the hub (seconds).
pub const FLEET_UPLOADER_INTERVAL_SECS: u64 = 60;

/// Base interval for fleet library sync (seconds). 21 600 = 6 hours.
pub const FLEET_LIBRARY_SYNC_INTERVAL_SECS: u64 = 21_600;

/// Random jitter added to the library sync interval (seconds). 1 800 = ±30 min.
pub const FLEET_LIBRARY_SYNC_JITTER_SECS: u64 = 1_800;

/// How often the rig pulls hub intelligence outputs (seconds). 14 400 = 4 hours.
pub const FLEET_INTELLIGENCE_SYNC_INTERVAL_SECS: u64 = 14_400;

/// Random jitter on intelligence sync interval (seconds). 1 800 = ±30 min.
pub const FLEET_INTELLIGENCE_SYNC_JITTER_SECS: u64 = 1_800;

/// Maximum number of intelligence outputs to keep in the local cache file.
pub const FLEET_INTELLIGENCE_MAX_CACHED: usize = 100;

// ============================================================================
// Fleet Event Snapshot
// ============================================================================

/// MSE efficiency denominator used in `HistorySnapshot` calculation.
///
/// `mse_efficiency = 100 - (mse / MSE_EFFICIENCY_DENOMINATOR * 100)`
pub const MSE_EFFICIENCY_DENOMINATOR: f64 = 50_000.0;

/// Reference ECD (ppg) for computing ECD margin in fleet snapshots.
pub const ECD_REFERENCE_PPG: f64 = 14.0;

// ============================================================================
// Simulation
// ============================================================================

/// Base delay denominator for `--speed` flag.
///
/// `delay_ms = SIMULATION_BASE_DELAY_MS / speed`
pub const SIMULATION_BASE_DELAY_MS: u64 = 60_000;

// ============================================================================
// LLM Scheduler
// ============================================================================

/// Maximum time to wait for a single LLM inference before aborting (seconds).
pub const LLM_INFERENCE_TIMEOUT_SECS: u64 = 120;

// ============================================================================
// Fleet Sync Backoff
// ============================================================================

/// Maximum backoff multiplier exponent for fleet sync retries.
///
/// `2^6 = 64× base interval`, capped at 300 s (~5 min).
pub const FLEET_SYNC_MAX_BACKOFF_EXPONENT: u32 = 6;

// ============================================================================
// Pairing Security
// ============================================================================

/// Maximum failed pairing status lookups per IP before returning 429.
pub const MAX_PAIRING_STATUS_FAILURES: u32 = 5;

/// Rolling window for pairing status failure tracking (seconds).
pub const PAIRING_RATE_LIMIT_WINDOW_SECS: u64 = 600;
