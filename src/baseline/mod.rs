//! Dynamic Thresholds Module - Baseline Learning & Z-Score Anomaly Detection
//!
//! This module implements "Phase 1.5 Baseline Learning" for the multi-agent pipeline.
//! Instead of hardcoded thresholds, it learns each machine's baseline behavior during
//! a commissioning window and uses z-score based anomaly detection.
//!
//! ## Architecture
//!
//! - `DynamicThresholds`: Per-metric thresholds learned from baseline data
//! - `BaselineAccumulator`: Accumulates samples during learning phase
//! - `ThresholdManager`: Manages thresholds for all equipment/sensors
//!
//! ## Key Features
//!
//! - Equipment-agnostic: Each machine learns its own baseline
//! - Z-score detection: warning at 3σ, critical at 5σ
//! - Contamination detection: Flags bad baselines
//! - Persistence: Thresholds survive restarts
//!
//! ## Usage
//!
//! ```ignore
//! // Create manager for a specific equipment
//! let mut manager = ThresholdManager::new();
//!
//! // During commissioning, add samples
//! manager.add_sample("RIG", "mse", 35000.0, timestamp);
//! manager.add_sample("RIG", "flow_balance", 0.0, timestamp);
//! // ... continue for learning window
//!
//! // Lock baseline when ready
//! manager.lock_baseline("RIG", "mse", timestamp)?;
//!
//! // During operation, check anomalies
//! let result = manager.check_anomaly("RIG", "mse", 55000.0);
//! match result.level {
//!     AnomalyLevel::Normal => { /* all good */ }
//!     AnomalyLevel::Warning => { /* create warning ticket */ }
//!     AnomalyLevel::Critical => { /* create critical ticket */ }
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;
use tracing::{debug, info, warn};

// ============================================================================
// Configuration Constants (defaults — overridden by well_config.toml)
// ============================================================================

/// Default sigma multiplier for warning threshold
pub const DEFAULT_WARNING_SIGMA: f64 = 3.0;

/// Default sigma multiplier for critical threshold
pub const DEFAULT_CRITICAL_SIGMA: f64 = 5.0;

/// Minimum samples required before baseline can be locked
pub const MIN_SAMPLES_FOR_LOCK: usize = 100;

/// Minimum standard deviation to avoid divide-by-zero (0.1% of mean or absolute floor)
pub const MIN_STD_FLOOR: f64 = 0.001;

/// Maximum allowed outlier percentage during baseline learning before flagging contamination
pub const MAX_OUTLIER_PERCENTAGE: f64 = 0.05; // 5%

/// Sigma threshold for outlier detection during learning
pub const OUTLIER_SIGMA_THRESHOLD: f64 = 3.0;

/// Schema version for persistence compatibility
pub const SCHEMA_VERSION: u32 = 2; // Updated for WITS metrics

// ============================================================================
// Config-aware accessors (read from well_config.toml when available)
// ============================================================================

fn cfg_warning_sigma() -> f64 {
    if crate::config::is_initialized() {
        crate::config::get().baseline_learning.warning_sigma
    } else {
        DEFAULT_WARNING_SIGMA
    }
}

fn cfg_critical_sigma() -> f64 {
    if crate::config::is_initialized() {
        crate::config::get().baseline_learning.critical_sigma
    } else {
        DEFAULT_CRITICAL_SIGMA
    }
}

fn cfg_min_samples() -> usize {
    if crate::config::is_initialized() {
        crate::config::get().baseline_learning.min_samples_for_lock
    } else {
        MIN_SAMPLES_FOR_LOCK
    }
}

fn cfg_min_std_floor() -> f64 {
    if crate::config::is_initialized() {
        crate::config::get().baseline_learning.min_std_floor
    } else {
        MIN_STD_FLOOR
    }
}

fn cfg_max_outlier_pct() -> f64 {
    if crate::config::is_initialized() {
        crate::config::get().baseline_learning.max_outlier_percentage
    } else {
        MAX_OUTLIER_PERCENTAGE
    }
}

fn cfg_outlier_sigma() -> f64 {
    if crate::config::is_initialized() {
        crate::config::get().baseline_learning.outlier_sigma_threshold
    } else {
        OUTLIER_SIGMA_THRESHOLD
    }
}

// ============================================================================
// Baseline Overrides — sigma-derived thresholds for anomaly detection
// ============================================================================

/// Sigma-derived threshold overrides computed from learned baselines.
///
/// After the `ThresholdManager` locks (100+ samples per metric), these overrides
/// are computed from the learned mean/std and can replace static config defaults
/// in the physics engine's anomaly detection.
///
/// Each field is `Option<f64>` — `None` means the metric wasn't locked or the
/// baseline data was insufficient to derive a meaningful threshold.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BaselineOverrides {
    /// Flow imbalance warning: 3σ of flow_balance std (gpm).
    /// Uses 3σ for warning because kick detection must avoid false alarms.
    pub flow_imbalance_warning_gpm: Option<f64>,
    /// SPP deviation warning: 2σ of spp std (psi).
    pub spp_deviation_warning_psi: Option<f64>,
    /// SPP deviation critical: 3σ of spp std (psi).
    pub spp_deviation_critical_psi: Option<f64>,
    /// Torque warning: mean + 2σ of torque (fraction increase).
    pub torque_warning_fraction: Option<f64>,
    /// Torque critical: mean + 3σ of torque (fraction increase).
    pub torque_critical_fraction: Option<f64>,
}

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, Error)]
pub enum BaselineError {
    #[error("Baseline not locked for metric: {0}")]
    NotLocked(String),

    #[error("Baseline already locked for metric: {0}")]
    AlreadyLocked(String),

    #[error("Insufficient samples for metric {0}: have {1}, need {2}")]
    InsufficientSamples(String, usize, usize),

    #[error("Baseline contaminated for metric {0}: {1:.1}% outliers detected (max {2:.1}%)")]
    Contaminated(String, f64, f64),

    #[error("Metric not found: {0}")]
    MetricNotFound(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Schema version mismatch: file has v{0}, expected v{1}")]
    SchemaMismatch(u32, u32),
}

// ============================================================================
// Anomaly Detection Results
// ============================================================================

/// Severity level from z-score check
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnomalyLevel {
    /// Value is within normal range (z < warning_sigma)
    Normal,
    /// Value exceeds warning threshold (warning_sigma <= z < critical_sigma)
    Warning,
    /// Value exceeds critical threshold (z >= critical_sigma)
    Critical,
}

impl std::fmt::Display for AnomalyLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnomalyLevel::Normal => write!(f, "NORMAL"),
            AnomalyLevel::Warning => write!(f, "WARNING"),
            AnomalyLevel::Critical => write!(f, "CRITICAL"),
        }
    }
}

/// Result of checking a value against dynamic thresholds
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyCheckResult {
    /// The metric ID that was checked
    pub metric_id: String,
    /// Current value being checked
    pub current_value: f64,
    /// Z-score: (current_value - baseline_mean) / baseline_std
    pub z_score: f64,
    /// Anomaly level based on z-score
    pub level: AnomalyLevel,
    /// Baseline mean used for comparison
    pub baseline_mean: f64,
    /// Baseline std used for comparison
    pub baseline_std: f64,
    /// Warning threshold (baseline_mean + warning_sigma * baseline_std)
    pub warning_threshold: f64,
    /// Critical threshold (baseline_mean + critical_sigma * baseline_std)
    pub critical_threshold: f64,
}

// ============================================================================
// Dynamic Thresholds
// ============================================================================

/// Build a "equipment:sensor" composite ID string.
fn make_composite_id(equipment_id: &str, sensor_id: &str) -> String {
    format!("{}:{}", equipment_id, sensor_id)
}

/// Dynamic thresholds learned from baseline data for a single metric
///
/// Each metric (e.g., "RIG:mse", "RIG:flow_balance", "RIG:torque")
/// has its own thresholds based on learned baseline behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicThresholds {
    /// Equipment identifier (e.g., "RIG", "MudPump1", "DrawWorks")
    pub equipment_id: String,

    /// Sensor/metric identifier (e.g., "mse", "flow_balance", "torque")
    pub sensor_id: String,

    /// Cached "equipment:sensor" composite ID (avoids repeated format! allocations)
    #[serde(default)]
    pub composite_id: String,

    /// Mean value during baseline period
    pub baseline_mean: f64,

    /// Standard deviation during baseline period
    pub baseline_std: f64,

    /// Sigma multiplier for warning threshold (default: 3.0)
    pub warning_sigma: f64,

    /// Sigma multiplier for critical threshold (default: 5.0)
    pub critical_sigma: f64,

    /// Whether baseline is locked (learning complete)
    pub locked: bool,

    /// Timestamp when baseline was locked (Unix timestamp)
    pub locked_timestamp: Option<u64>,

    /// Number of samples used to compute baseline
    pub sample_count: usize,

    /// Minimum value seen during baseline
    pub min_value: f64,

    /// Maximum value seen during baseline
    pub max_value: f64,
}

impl DynamicThresholds {
    /// Create new thresholds (unlocked, awaiting baseline learning)
    pub fn new(equipment_id: &str, sensor_id: &str) -> Self {
        Self {
            composite_id: make_composite_id(equipment_id, sensor_id),
            equipment_id: equipment_id.to_string(),
            sensor_id: sensor_id.to_string(),
            baseline_mean: 0.0,
            baseline_std: 0.0,
            warning_sigma: cfg_warning_sigma(),
            critical_sigma: cfg_critical_sigma(),
            locked: false,
            locked_timestamp: None,
            sample_count: 0,
            min_value: f64::MAX,
            max_value: f64::MIN,
        }
    }

    /// Get cached composite ID (equipment:sensor) — zero allocation.
    pub fn composite_id(&self) -> &str {
        &self.composite_id
    }

    /// Calculate warning threshold
    pub fn warning_threshold(&self) -> f64 {
        self.baseline_mean + self.warning_sigma * self.effective_std()
    }

    /// Calculate critical threshold
    pub fn critical_threshold(&self) -> f64 {
        self.baseline_mean + self.critical_sigma * self.effective_std()
    }

    /// Get effective standard deviation (with floor to avoid divide-by-zero)
    pub fn effective_std(&self) -> f64 {
        let floor = cfg_min_std_floor();
        if !self.baseline_mean.is_finite() {
            return floor;
        }
        let min_std = (self.baseline_mean.abs() * floor).max(floor);
        self.baseline_std.max(min_std)
    }

    /// Calculate z-score for a value
    pub fn z_score(&self, value: f64) -> f64 {
        (value - self.baseline_mean) / self.effective_std()
    }

    /// Check a value against thresholds
    pub fn check(&self, value: f64) -> AnomalyCheckResult {
        let z = self.z_score(value);
        let level = if z >= self.critical_sigma {
            AnomalyLevel::Critical
        } else if z >= self.warning_sigma {
            AnomalyLevel::Warning
        } else {
            AnomalyLevel::Normal
        };

        AnomalyCheckResult {
            metric_id: self.composite_id.clone(),
            current_value: value,
            z_score: z,
            level,
            baseline_mean: self.baseline_mean,
            baseline_std: self.effective_std(),
            warning_threshold: self.warning_threshold(),
            critical_threshold: self.critical_threshold(),
        }
    }

    /// Check a value against thresholds in both directions (for signed metrics
    /// like flow_balance or SPP delta where deviations in either direction are
    /// anomalous).
    pub fn check_bidirectional(&self, value: f64) -> AnomalyCheckResult {
        let z = self.z_score(value);
        let abs_z = z.abs();
        let level = if abs_z >= self.critical_sigma {
            AnomalyLevel::Critical
        } else if abs_z >= self.warning_sigma {
            AnomalyLevel::Warning
        } else {
            AnomalyLevel::Normal
        };

        AnomalyCheckResult {
            metric_id: self.composite_id.clone(),
            current_value: value,
            z_score: z,
            level,
            baseline_mean: self.baseline_mean,
            baseline_std: self.effective_std(),
            warning_threshold: self.warning_threshold(),
            critical_threshold: self.critical_threshold(),
        }
    }

    /// Check if a value exceeds warning threshold
    pub fn is_warning(&self, value: f64) -> bool {
        self.z_score(value) >= self.warning_sigma
    }

    /// Check if a value exceeds critical threshold
    pub fn is_critical(&self, value: f64) -> bool {
        self.z_score(value) >= self.critical_sigma
    }
}

// ============================================================================
// Baseline Accumulator
// ============================================================================

/// Accumulates samples during baseline learning phase
///
/// Uses Welford's online algorithm for numerically stable mean/variance calculation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineAccumulator {
    /// Equipment identifier
    pub equipment_id: String,

    /// Sensor/metric identifier
    pub sensor_id: String,

    /// Cached "equipment:sensor" composite ID (avoids repeated format! allocations)
    #[serde(default)]
    pub composite_id: String,

    /// Number of samples accumulated
    pub count: usize,

    /// Running mean (Welford's algorithm)
    pub mean: f64,

    /// Running M2 for variance (Welford's algorithm)
    pub m2: f64,

    /// Minimum value seen
    pub min_value: f64,

    /// Maximum value seen
    pub max_value: f64,

    /// Count of outliers detected (for contamination check)
    pub outlier_count: usize,

    /// Timestamp when learning started
    pub started_at: u64,
}

impl BaselineAccumulator {
    /// Create new accumulator for a metric
    pub fn new(equipment_id: &str, sensor_id: &str, started_at: u64) -> Self {
        Self {
            composite_id: make_composite_id(equipment_id, sensor_id),
            equipment_id: equipment_id.to_string(),
            sensor_id: sensor_id.to_string(),
            count: 0,
            mean: 0.0,
            m2: 0.0,
            min_value: f64::MAX,
            max_value: f64::MIN,
            outlier_count: 0,
            started_at,
        }
    }

    /// Get cached composite ID — zero allocation.
    pub fn composite_id(&self) -> &str {
        &self.composite_id
    }

    /// Add a sample using Welford's online algorithm
    ///
    /// Returns true if sample was an outlier (for contamination tracking).
    /// Non-finite values (NaN, Infinity) are rejected to prevent poisoning
    /// the Welford accumulator — a single NaN permanently corrupts mean/m2.
    pub fn add_sample(&mut self, value: f64) -> bool {
        if !value.is_finite() {
            return false;
        }
        self.count += 1;

        // Update min/max
        self.min_value = self.min_value.min(value);
        self.max_value = self.max_value.max(value);

        // Welford's online algorithm for mean and variance
        let delta = value - self.mean;
        self.mean += delta / self.count as f64;
        let delta2 = value - self.mean;
        self.m2 += delta * delta2;

        // Check for outlier (after we have enough samples for meaningful std)
        let is_outlier = if self.count > 10 {
            let std = self.variance().sqrt();
            let z = if std > cfg_min_std_floor() {
                (value - self.mean).abs() / std
            } else {
                0.0
            };
            if z > cfg_outlier_sigma() {
                self.outlier_count += 1;
                true
            } else {
                false
            }
        } else {
            false
        };

        is_outlier
    }

    /// Get current variance
    pub fn variance(&self) -> f64 {
        if self.count < 2 {
            0.0
        } else {
            self.m2 / (self.count - 1) as f64
        }
    }

    /// Get current standard deviation
    pub fn std_dev(&self) -> f64 {
        self.variance().sqrt()
    }

    /// Get outlier percentage
    pub fn outlier_percentage(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.outlier_count as f64 / self.count as f64
        }
    }

    /// Check if baseline is contaminated
    pub fn is_contaminated(&self) -> bool {
        self.outlier_percentage() > cfg_max_outlier_pct()
    }

    /// Check if we have enough samples to lock
    pub fn has_enough_samples(&self) -> bool {
        self.count >= cfg_min_samples()
    }

    /// Finalize into DynamicThresholds
    ///
    /// Returns error if contaminated or insufficient samples.
    pub fn finalize(self, timestamp: u64) -> Result<DynamicThresholds, BaselineError> {
        // Check minimum samples
        if !self.has_enough_samples() {
            return Err(BaselineError::InsufficientSamples(
                self.composite_id.clone(),
                self.count,
                cfg_min_samples(),
            ));
        }

        // Check contamination
        let outlier_pct = self.outlier_percentage() * 100.0;
        if self.is_contaminated() {
            return Err(BaselineError::Contaminated(
                self.composite_id.clone(),
                outlier_pct,
                cfg_max_outlier_pct() * 100.0,
            ));
        }

        // Compute std_dev before moving values
        let std_dev = self.std_dev();

        Ok(DynamicThresholds {
            composite_id: self.composite_id,
            equipment_id: self.equipment_id,
            sensor_id: self.sensor_id,
            baseline_mean: self.mean,
            baseline_std: std_dev,
            warning_sigma: cfg_warning_sigma(),
            critical_sigma: cfg_critical_sigma(),
            locked: true,
            locked_timestamp: Some(timestamp),
            sample_count: self.count,
            min_value: self.min_value,
            max_value: self.max_value,
        })
    }

    /// Force finalize even if contaminated (for manual override)
    pub fn force_finalize(self, timestamp: u64) -> DynamicThresholds {
        let outlier_pct = self.outlier_percentage() * 100.0;
        let std_dev = self.std_dev();

        warn!(
            metric = %self.composite_id,
            outlier_pct = outlier_pct,
            "Force-finalizing potentially contaminated baseline"
        );

        DynamicThresholds {
            composite_id: self.composite_id,
            equipment_id: self.equipment_id,
            sensor_id: self.sensor_id,
            baseline_mean: self.mean,
            baseline_std: std_dev,
            warning_sigma: cfg_warning_sigma(),
            critical_sigma: cfg_critical_sigma(),
            locked: true,
            locked_timestamp: Some(timestamp),
            sample_count: self.count,
            min_value: self.min_value,
            max_value: self.max_value,
        }
    }
}

// ============================================================================
// Learning Status
// ============================================================================

/// Status of baseline learning for a metric
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LearningStatus {
    /// Still learning - accumulating samples
    Learning {
        samples_collected: usize,
        samples_needed: usize,
        outlier_percentage: f64,
        current_mean: f64,
        current_std: f64,
    },
    /// Baseline locked and ready for use
    Locked {
        mean: f64,
        std: f64,
        warning_threshold: f64,
        critical_threshold: f64,
        sample_count: usize,
        locked_at: u64,
    },
    /// Contamination detected, needs attention
    Contaminated {
        outlier_percentage: f64,
        samples_collected: usize,
    },
}

// ============================================================================
// Baseline State Persistence
// ============================================================================

/// Default file path for baseline state persistence
pub const DEFAULT_STATE_PATH: &str = "data/baseline_state.json";

/// Serializable baseline state for crash-safe persistence.
///
/// Only locked thresholds are persisted.  In-progress accumulators are
/// intentionally excluded so that learning restarts cleanly after a reboot
/// rather than resuming from potentially stale partial data.
#[derive(Serialize, Deserialize)]
struct BaselineState {
    schema_version: u32,
    thresholds: HashMap<String, DynamicThresholds>,
    /// Sigma-derived overrides computed at lock time (Phase 2 auto-detection).
    #[serde(default)]
    overrides: Option<BaselineOverrides>,
}

// ============================================================================
// Threshold Manager
// ============================================================================

/// Manages dynamic thresholds for all equipment and sensors
///
/// This is the main interface for the baseline learning system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdManager {
    /// Locked thresholds ready for use
    thresholds: HashMap<String, DynamicThresholds>,

    /// Accumulators for metrics still learning
    accumulators: HashMap<String, BaselineAccumulator>,

    /// Schema version for persistence compatibility
    schema_version: u32,

    /// Sigma-derived overrides computed at lock time, used by physics engine.
    /// Populated after baselines lock; persisted alongside thresholds.
    pub overrides: Option<BaselineOverrides>,
}

impl Default for ThresholdManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ThresholdManager {
    /// Create new empty manager
    pub fn new() -> Self {
        Self {
            thresholds: HashMap::new(),
            accumulators: HashMap::new(),
            schema_version: SCHEMA_VERSION,
            overrides: None,
        }
    }

    /// Start learning baseline for a metric
    pub fn start_learning(&mut self, equipment_id: &str, sensor_id: &str, timestamp: u64) {
        let composite_id = format!("{}:{}", equipment_id, sensor_id);

        // Don't restart if already locked
        if self.thresholds.contains_key(&composite_id) {
            debug!(metric = %composite_id, "Skipping - baseline already locked");
            return;
        }

        // Don't restart if already learning
        if self.accumulators.contains_key(&composite_id) {
            debug!(metric = %composite_id, "Already learning");
            return;
        }

        info!(metric = %composite_id, "Starting baseline learning");
        self.accumulators.insert(
            composite_id,
            BaselineAccumulator::new(equipment_id, sensor_id, timestamp),
        );
    }

    /// Add a sample to an accumulator
    ///
    /// Automatically starts learning if not already started.
    /// Returns None if baseline is already locked.
    pub fn add_sample(
        &mut self,
        equipment_id: &str,
        sensor_id: &str,
        value: f64,
        timestamp: u64,
    ) -> Option<bool> {
        let composite_id = format!("{}:{}", equipment_id, sensor_id);

        // If already locked, ignore samples
        if self.thresholds.contains_key(&composite_id) {
            return None;
        }

        // Start learning if needed
        if !self.accumulators.contains_key(&composite_id) {
            self.start_learning(equipment_id, sensor_id, timestamp);
        }

        // Add sample
        let acc = self.accumulators.get_mut(&composite_id)?;
        let is_outlier = acc.add_sample(value);
        Some(is_outlier)
    }

    /// Attempt to lock baseline for a metric
    pub fn lock_baseline(
        &mut self,
        equipment_id: &str,
        sensor_id: &str,
        timestamp: u64,
    ) -> Result<&DynamicThresholds, BaselineError> {
        let composite_id = format!("{}:{}", equipment_id, sensor_id);

        // Check if already locked
        if self.thresholds.contains_key(&composite_id) {
            return Err(BaselineError::AlreadyLocked(composite_id));
        }

        // Get accumulator
        let acc = self
            .accumulators
            .remove(&composite_id)
            .ok_or_else(|| BaselineError::MetricNotFound(composite_id.clone()))?;

        // Finalize
        let thresholds = acc.finalize(timestamp)?;

        info!(
            metric = %composite_id,
            mean = thresholds.baseline_mean,
            std = thresholds.baseline_std,
            warning = thresholds.warning_threshold(),
            critical = thresholds.critical_threshold(),
            samples = thresholds.sample_count,
            "Baseline locked"
        );

        self.thresholds.insert(composite_id.clone(), thresholds);

        // Auto-persist after successful lock
        if let Err(e) = self.save_to_file(Path::new(DEFAULT_STATE_PATH)) {
            warn!(error = %e, "Failed to auto-persist baseline state after lock");
        }

        self.thresholds.get(&composite_id)
            .ok_or_else(|| BaselineError::MetricNotFound(format!("{composite_id} (insert failed)")))
    }

    /// Force lock baseline even if contaminated
    pub fn force_lock_baseline(
        &mut self,
        equipment_id: &str,
        sensor_id: &str,
        timestamp: u64,
    ) -> Result<&DynamicThresholds, BaselineError> {
        let composite_id = format!("{}:{}", equipment_id, sensor_id);

        // Check if already locked
        if self.thresholds.contains_key(&composite_id) {
            return Err(BaselineError::AlreadyLocked(composite_id));
        }

        // Get accumulator
        let acc = self
            .accumulators
            .remove(&composite_id)
            .ok_or_else(|| BaselineError::MetricNotFound(composite_id.clone()))?;

        // Force finalize
        let thresholds = acc.force_finalize(timestamp);

        warn!(
            metric = %composite_id,
            mean = thresholds.baseline_mean,
            std = thresholds.baseline_std,
            "Baseline force-locked (may be contaminated)"
        );

        self.thresholds.insert(composite_id.clone(), thresholds);

        // Auto-persist after successful force-lock
        if let Err(e) = self.save_to_file(Path::new(DEFAULT_STATE_PATH)) {
            warn!(error = %e, "Failed to auto-persist baseline state after force-lock");
        }

        self.thresholds.get(&composite_id)
            .ok_or_else(|| BaselineError::MetricNotFound(format!("{composite_id} (insert failed)")))
    }

    /// Check a value against dynamic thresholds
    ///
    /// Returns None if baseline is not yet locked.
    pub fn check_anomaly(
        &self,
        equipment_id: &str,
        sensor_id: &str,
        value: f64,
    ) -> Option<AnomalyCheckResult> {
        let composite_id = format!("{}:{}", equipment_id, sensor_id);
        self.thresholds.get(&composite_id).map(|t| t.check(value))
    }

    /// Get learning status for a metric
    pub fn get_status(&self, equipment_id: &str, sensor_id: &str) -> Option<LearningStatus> {
        let composite_id = format!("{}:{}", equipment_id, sensor_id);

        // Check if locked
        if let Some(t) = self.thresholds.get(&composite_id) {
            return Some(LearningStatus::Locked {
                mean: t.baseline_mean,
                std: t.baseline_std,
                warning_threshold: t.warning_threshold(),
                critical_threshold: t.critical_threshold(),
                sample_count: t.sample_count,
                locked_at: t.locked_timestamp.unwrap_or(0),
            });
        }

        // Check if learning
        if let Some(acc) = self.accumulators.get(&composite_id) {
            if acc.is_contaminated() {
                return Some(LearningStatus::Contaminated {
                    outlier_percentage: acc.outlier_percentage() * 100.0,
                    samples_collected: acc.count,
                });
            }
            return Some(LearningStatus::Learning {
                samples_collected: acc.count,
                samples_needed: cfg_min_samples(),
                outlier_percentage: acc.outlier_percentage() * 100.0,
                current_mean: acc.mean,
                current_std: acc.std_dev(),
            });
        }

        None
    }

    /// Check if baseline is locked for a metric
    pub fn is_locked(&self, equipment_id: &str, sensor_id: &str) -> bool {
        let composite_id = format!("{}:{}", equipment_id, sensor_id);
        self.thresholds.contains_key(&composite_id)
    }

    /// Check if baseline is learning for a metric
    pub fn is_learning(&self, equipment_id: &str, sensor_id: &str) -> bool {
        let composite_id = format!("{}:{}", equipment_id, sensor_id);
        self.accumulators.contains_key(&composite_id)
    }

    /// Get all locked thresholds
    pub fn get_all_thresholds(&self) -> &HashMap<String, DynamicThresholds> {
        &self.thresholds
    }

    /// Get threshold for a specific metric
    pub fn get_threshold(&self, equipment_id: &str, sensor_id: &str) -> Option<&DynamicThresholds> {
        let composite_id = format!("{}:{}", equipment_id, sensor_id);
        self.thresholds.get(&composite_id)
    }

    /// Get accumulator for a specific metric (for status checks)
    pub fn get_accumulator(
        &self,
        equipment_id: &str,
        sensor_id: &str,
    ) -> Option<&BaselineAccumulator> {
        let composite_id = format!("{}:{}", equipment_id, sensor_id);
        self.accumulators.get(&composite_id)
    }

    /// Reset learning for a metric (for re-commissioning)
    pub fn reset_learning(&mut self, equipment_id: &str, sensor_id: &str) {
        let composite_id = format!("{}:{}", equipment_id, sensor_id);
        self.thresholds.remove(&composite_id);
        self.accumulators.remove(&composite_id);
        info!(metric = %composite_id, "Baseline reset for re-commissioning");
    }

    /// Compute sigma-derived overrides from locked baselines.
    ///
    /// Called after baselines lock to derive anomaly detection thresholds
    /// from the learned mean/std of each WITS metric.
    pub fn compute_overrides(&self, equipment_id: &str) -> BaselineOverrides {
        let mut overrides = BaselineOverrides::default();

        // Flow imbalance: 3σ of flow_balance std (absolute deviation from zero)
        if let Some(t) = self.get_threshold(equipment_id, wits_metrics::FLOW_BALANCE) {
            let sigma3 = 3.0 * t.effective_std();
            if sigma3 > 0.0 {
                overrides.flow_imbalance_warning_gpm = Some(sigma3);
            }
        }

        // SPP deviation: 2σ for warning, 3σ for critical
        if let Some(t) = self.get_threshold(equipment_id, wits_metrics::SPP) {
            let sigma2 = 2.0 * t.effective_std();
            let sigma3 = 3.0 * t.effective_std();
            if sigma2 > 0.0 {
                overrides.spp_deviation_warning_psi = Some(sigma2.max(30.0));
                overrides.spp_deviation_critical_psi = Some(sigma3.max(50.0));
            }
        }

        // Torque: fraction-based increase thresholds
        // Anomaly detection checks torque_delta_percent (fractional change).
        // We derive threshold from (std / mean), scaled by 2σ and 3σ.
        if let Some(t) = self.get_threshold(equipment_id, wits_metrics::TORQUE) {
            if t.baseline_mean > 0.0 {
                let cv = (t.effective_std() / t.baseline_mean).clamp(0.0, 10.0);
                let warn = (2.0 * cv).max(0.10); // Never below 10%
                let crit = (3.0 * cv).max(0.20); // Never below 20%
                if warn > 0.0 {
                    overrides.torque_warning_fraction = Some(warn);
                    overrides.torque_critical_fraction = Some(crit);
                }
            }
        }

        overrides
    }

    /// Count of locked thresholds
    pub fn locked_count(&self) -> usize {
        self.thresholds.len()
    }

    /// Count of metrics still learning
    pub fn learning_count(&self) -> usize {
        self.accumulators.len()
    }

    // ========================================================================
    // Persistence
    // ========================================================================

    /// Save locked thresholds to a JSON file via [`BaselineState`].
    ///
    /// Only locked thresholds are persisted; in-progress accumulators are
    /// intentionally omitted so that learning restarts cleanly after a reboot.
    /// Parent directories are created automatically if they do not exist.
    pub fn save_to_file(&self, path: &Path) -> Result<(), BaselineError> {
        let state = BaselineState {
            schema_version: SCHEMA_VERSION,
            thresholds: self.thresholds.clone(),
            overrides: self.overrides.clone(),
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&state)?;
        std::fs::write(path, json)?;
        info!(
            path = %path.display(),
            locked = self.thresholds.len(),
            "Baseline state saved"
        );
        Ok(())
    }

    /// Load locked thresholds from a JSON file.
    ///
    /// Returns `None` when the file is missing, corrupt, or has an
    /// incompatible schema version.  Accumulators are always empty on load
    /// so learning restarts from scratch after a crash.
    pub fn load_from_file(path: &Path) -> Option<Self> {
        let json = match std::fs::read_to_string(path) {
            Ok(j) => j,
            Err(e) => {
                debug!(path = %path.display(), error = %e, "No baseline state file found");
                return None;
            }
        };
        let state: BaselineState = match serde_json::from_str(&json) {
            Ok(s) => s,
            Err(e) => {
                warn!(path = %path.display(), error = %e, "Corrupt baseline state file, ignoring");
                return None;
            }
        };
        if state.schema_version != SCHEMA_VERSION {
            warn!(
                file_version = state.schema_version,
                expected = SCHEMA_VERSION,
                "Schema version mismatch, ignoring saved baseline state"
            );
            return None;
        }
        let locked = state.thresholds.len();
        let has_overrides = state.overrides.is_some();
        info!(path = %path.display(), locked, has_overrides, "Baseline state loaded");

        // Reconstruct cached composite_id for entries deserialized from old state
        // files that predate the composite_id field.
        let thresholds: HashMap<String, DynamicThresholds> = state.thresholds.into_iter()
            .map(|(k, mut t)| {
                if t.composite_id.is_empty() {
                    t.composite_id = make_composite_id(&t.equipment_id, &t.sensor_id);
                }
                (k, t)
            })
            .collect();

        Some(Self {
            thresholds,
            accumulators: HashMap::new(),
            schema_version: SCHEMA_VERSION,
            overrides: state.overrides,
        })
    }

    /// Load from file if it exists and is valid, otherwise create new.
    pub fn load_or_new(path: &Path) -> Self {
        Self::load_from_file(path).unwrap_or_else(|| {
            debug!("No valid baseline state found, starting fresh");
            Self::new()
        })
    }
}

// ============================================================================
// WITS Drilling Metrics (replacing TDS metrics)
// ============================================================================

/// Standard WITS drilling metric IDs
pub mod wits_metrics {
    /// Mechanical Specific Energy (psi)
    pub const MSE: &str = "mse";
    /// D-exponent (normalized drilling parameter)
    pub const D_EXPONENT: &str = "d_exponent";
    /// Corrected d-exponent
    pub const DXC: &str = "dxc";
    /// Flow balance (bbl/hr) - positive = gain, negative = loss
    pub const FLOW_BALANCE: &str = "flow_balance";
    /// Standpipe pressure (psi)
    pub const SPP: &str = "spp";
    /// Torque (kft-lbs)
    pub const TORQUE: &str = "torque";
    /// Rate of penetration (ft/hr)
    pub const ROP: &str = "rop";
    /// Weight on bit (klbs)
    pub const WOB: &str = "wob";
    /// Rotary RPM
    pub const RPM: &str = "rpm";
    /// Equivalent circulating density (ppg)
    pub const ECD: &str = "ecd";
    /// Pit volume (bbl)
    pub const PIT_VOLUME: &str = "pit_volume";
    /// Gas units (total gas)
    pub const GAS_UNITS: &str = "gas_units";
}

/// Legacy TDS metric IDs (for backward compatibility)
pub mod tds_metrics {
    pub const VIBRATION_RMS: &str = "vibration_rms";
    pub const VIBRATION_KURTOSIS: &str = "vibration_kurtosis";
    pub const BPFO_AMPLITUDE: &str = "bpfo_amplitude";
    pub const BPFI_AMPLITUDE: &str = "bpfi_amplitude";
    pub const MOTOR_TEMP: &str = "motor_temp";
    pub const GEARBOX_TEMP: &str = "gearbox_temp";
}

impl ThresholdManager {
    /// Convenience: Add all standard WITS drilling metrics for learning
    pub fn start_wits_learning(&mut self, equipment_id: &str, timestamp: u64) {
        self.start_learning(equipment_id, wits_metrics::MSE, timestamp);
        self.start_learning(equipment_id, wits_metrics::D_EXPONENT, timestamp);
        self.start_learning(equipment_id, wits_metrics::DXC, timestamp);
        self.start_learning(equipment_id, wits_metrics::FLOW_BALANCE, timestamp);
        self.start_learning(equipment_id, wits_metrics::SPP, timestamp);
        self.start_learning(equipment_id, wits_metrics::TORQUE, timestamp);
        self.start_learning(equipment_id, wits_metrics::ROP, timestamp);
        self.start_learning(equipment_id, wits_metrics::WOB, timestamp);
        self.start_learning(equipment_id, wits_metrics::RPM, timestamp);
        self.start_learning(equipment_id, wits_metrics::ECD, timestamp);
        self.start_learning(equipment_id, wits_metrics::PIT_VOLUME, timestamp);
        self.start_learning(equipment_id, wits_metrics::GAS_UNITS, timestamp);
    }

    /// Convenience: Lock all WITS metrics that have enough samples
    pub fn try_lock_all_wits(&mut self, equipment_id: &str, timestamp: u64) -> Vec<String> {
        let metrics = [
            wits_metrics::MSE,
            wits_metrics::D_EXPONENT,
            wits_metrics::DXC,
            wits_metrics::FLOW_BALANCE,
            wits_metrics::SPP,
            wits_metrics::TORQUE,
            wits_metrics::ROP,
            wits_metrics::WOB,
            wits_metrics::RPM,
            wits_metrics::ECD,
            wits_metrics::PIT_VOLUME,
            wits_metrics::GAS_UNITS,
        ];

        let mut locked = Vec::new();
        for sensor_id in metrics {
            if self.lock_baseline(equipment_id, sensor_id, timestamp).is_ok() {
                locked.push(format!("{}:{}", equipment_id, sensor_id));
            }
        }
        locked
    }

    /// Check if all WITS metrics are locked
    pub fn all_wits_locked(&self, equipment_id: &str) -> bool {
        let metrics = [
            wits_metrics::MSE,
            wits_metrics::D_EXPONENT,
            wits_metrics::DXC,
            wits_metrics::FLOW_BALANCE,
            wits_metrics::SPP,
            wits_metrics::TORQUE,
            wits_metrics::ROP,
            wits_metrics::WOB,
            wits_metrics::RPM,
            wits_metrics::ECD,
            wits_metrics::PIT_VOLUME,
            wits_metrics::GAS_UNITS,
        ];

        metrics
            .iter()
            .all(|sensor_id| self.is_locked(equipment_id, sensor_id))
    }

    /// Legacy: Add all standard TDS metrics for learning
    pub fn start_tds_learning(&mut self, equipment_id: &str, timestamp: u64) {
        self.start_learning(equipment_id, tds_metrics::VIBRATION_RMS, timestamp);
        self.start_learning(equipment_id, tds_metrics::VIBRATION_KURTOSIS, timestamp);
        self.start_learning(equipment_id, tds_metrics::BPFO_AMPLITUDE, timestamp);
        self.start_learning(equipment_id, tds_metrics::MOTOR_TEMP, timestamp);
        self.start_learning(equipment_id, tds_metrics::GEARBOX_TEMP, timestamp);
    }

    /// Legacy: Lock all TDS metrics that have enough samples
    pub fn try_lock_all_tds(&mut self, equipment_id: &str, timestamp: u64) -> Vec<String> {
        let metrics = [
            tds_metrics::VIBRATION_RMS,
            tds_metrics::VIBRATION_KURTOSIS,
            tds_metrics::BPFO_AMPLITUDE,
            tds_metrics::MOTOR_TEMP,
            tds_metrics::GEARBOX_TEMP,
        ];

        let mut locked = Vec::new();
        for sensor_id in metrics {
            if self.lock_baseline(equipment_id, sensor_id, timestamp).is_ok() {
                locked.push(format!("{}:{}", equipment_id, sensor_id));
            }
        }
        locked
    }

    /// Legacy: Check if all TDS metrics are locked
    pub fn all_tds_locked(&self, equipment_id: &str) -> bool {
        let metrics = [
            tds_metrics::VIBRATION_RMS,
            tds_metrics::VIBRATION_KURTOSIS,
            tds_metrics::BPFO_AMPLITUDE,
            tds_metrics::MOTOR_TEMP,
            tds_metrics::GEARBOX_TEMP,
        ];

        metrics
            .iter()
            .all(|sensor_id| self.is_locked(equipment_id, sensor_id))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_welford_algorithm() {
        let mut acc = BaselineAccumulator::new("RIG", "test", 0);

        // Add known values
        let values = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        for v in values {
            acc.add_sample(v);
        }

        // Mean should be 5.0
        assert!((acc.mean - 5.0).abs() < 0.001);

        // Sample variance = sum((x - mean)^2) / (n-1)
        // Sum of squared deviations = 9+1+1+1+0+0+4+16 = 32
        // Sample variance = 32/7 ≈ 4.571
        assert!((acc.variance() - 4.571).abs() < 0.01);

        // Std should be sqrt(4.571) ≈ 2.138
        assert!((acc.std_dev() - 2.138).abs() < 0.01);
    }

    #[test]
    fn test_z_score_calculation() {
        let threshold = DynamicThresholds {
            equipment_id: "RIG".to_string(),
            sensor_id: "mse".to_string(),
            composite_id: "RIG:mse".to_string(),
            baseline_mean: 35000.0,
            baseline_std: 5000.0,
            warning_sigma: 3.0,
            critical_sigma: 5.0,
            locked: true,
            locked_timestamp: Some(1000),
            sample_count: 100,
            min_value: 25000.0,
            max_value: 45000.0,
        };

        // Normal value (z = 1)
        let result = threshold.check(40000.0);
        assert_eq!(result.level, AnomalyLevel::Normal);
        assert!((result.z_score - 1.0).abs() < 0.001);

        // Warning value (z = 3.5)
        let result = threshold.check(52500.0);
        assert_eq!(result.level, AnomalyLevel::Warning);

        // Critical value (z = 5.5)
        let result = threshold.check(62500.0);
        assert_eq!(result.level, AnomalyLevel::Critical);
    }

    #[test]
    fn test_threshold_manager_workflow() {
        let mut manager = ThresholdManager::new();

        // Start learning
        manager.start_learning("RIG", "mse", 0);
        assert!(manager.is_learning("RIG", "mse"));
        assert!(!manager.is_locked("RIG", "mse"));

        // Add samples
        for i in 0..150 {
            let value = 35000.0 + (i as f64 * 10.0); // Small variation
            manager.add_sample("RIG", "mse", value, i as u64);
        }

        // Lock baseline
        let result = manager.lock_baseline("RIG", "mse", 1000);
        assert!(result.is_ok());
        assert!(manager.is_locked("RIG", "mse"));
        assert!(!manager.is_learning("RIG", "mse"));

        // Check anomaly
        let check = manager.check_anomaly("RIG", "mse", 100000.0);
        assert!(check.is_some());
        assert_eq!(check.unwrap().level, AnomalyLevel::Critical);
    }

    #[test]
    fn test_wits_metrics_learning() {
        let mut manager = ThresholdManager::new();

        manager.start_wits_learning("RIG", 0);

        // Verify all metrics are learning
        assert!(manager.is_learning("RIG", wits_metrics::MSE));
        assert!(manager.is_learning("RIG", wits_metrics::FLOW_BALANCE));
        assert!(manager.is_learning("RIG", wits_metrics::TORQUE));
        assert!(manager.is_learning("RIG", wits_metrics::ROP));
    }

    #[test]
    fn test_min_std_floor() {
        let threshold = DynamicThresholds {
            equipment_id: "RIG".to_string(),
            sensor_id: "test".to_string(),
            composite_id: "RIG:test".to_string(),
            baseline_mean: 0.0,
            baseline_std: 0.0, // Zero std!
            warning_sigma: 3.0,
            critical_sigma: 5.0,
            locked: true,
            locked_timestamp: Some(1000),
            sample_count: 100,
            min_value: 0.0,
            max_value: 0.0,
        };

        // Should use floor instead of zero
        assert!(threshold.effective_std() > 0.0);

        // Should not panic or produce infinity
        let result = threshold.check(0.05);
        assert!(result.z_score.is_finite());
    }

    #[test]
    fn test_contamination_detection() {
        let mut acc = BaselineAccumulator::new("RIG", "test", 0);

        // Add mostly normal values
        for _ in 0..80 {
            acc.add_sample(35000.0);
        }

        // Add too many outliers (> 5%)
        for _ in 0..20 {
            acc.add_sample(500000.0); // Very high outlier
        }

        assert!(acc.is_contaminated());

        // Finalize should fail
        let result = acc.finalize(1000);
        assert!(matches!(result, Err(BaselineError::Contaminated(_, _, _))));
    }
}
