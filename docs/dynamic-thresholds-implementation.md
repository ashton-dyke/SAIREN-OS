# Dynamic Thresholds Implementation

This document details the complete implementation of dynamic baseline thresholds for the TDS Guardian multi-agent monitoring system.

## Overview

The system now supports learning equipment-specific baselines during commissioning and using z-score based anomaly detection during operation. This replaces hardcoded thresholds with dynamic, learned thresholds based on each machine's actual behavior.

### Key Features

- **Welford's Online Algorithm**: Numerically stable single-pass mean/variance calculation
- **Z-Score Anomaly Detection**: Warning at 3σ, critical at 5σ deviations
- **Contamination Detection**: Flags baselines with >5% outliers as potentially corrupted
- **Multi-Equipment Architecture**: HashMap-based design with composite keys (`equipment_id:sensor_id`)
- **JSON Persistence**: Thresholds survive restarts with schema versioning
- **Three Operating Modes**: FixedThresholds, BaselineLearning, DynamicThresholds

---

## Architecture

### Design Choice: Extensible Multi-Equipment

The implementation uses "Option 2: Extensible" from the design spec, supporting multiple equipment IDs from day one. This enables immediate scaling to monitor the full drill package (mud pumps, draw works, etc.).

```
ThresholdManager
├── thresholds: HashMap<String, DynamicThresholds>
│   ├── "TDS:vibration_rms" -> DynamicThresholds
│   ├── "TDS:bpfo_amplitude" -> DynamicThresholds
│   ├── "MUD_PUMP_1:pressure" -> DynamicThresholds (future)
│   └── "DRAW_WORKS:current" -> DynamicThresholds (future)
└── accumulators: HashMap<String, BaselineAccumulator>
    └── (active during learning phase only)
```

---

## Components

### 1. DynamicThresholds Struct

**File**: `src/baseline/mod.rs` (lines 50-90)

Stores learned baseline statistics for a single metric on a single piece of equipment.

```rust
pub struct DynamicThresholds {
    pub equipment_id: String,      // e.g., "TDS", "MUD_PUMP_1"
    pub sensor_id: String,         // e.g., "vibration_rms", "bpfo_amplitude"
    pub baseline_mean: f64,        // Learned mean value
    pub baseline_std: f64,         // Learned standard deviation
    pub warning_sigma: f64,        // Z-score for warning (default: 3.0)
    pub critical_sigma: f64,       // Z-score for critical (default: 5.0)
    pub locked: bool,              // True when baseline learning complete
    pub locked_timestamp: Option<u64>,  // When baseline was locked
    pub sample_count: usize,       // Number of samples used
    pub min_value: f64,            // Minimum observed value
    pub max_value: f64,            // Maximum observed value
}
```

**Key Methods**:
- `check_anomaly(value: f64) -> AnomalyCheckResult`: Returns z-score and anomaly level
- `warning_threshold() -> f64`: Returns `mean + (warning_sigma * std)`
- `critical_threshold() -> f64`: Returns `mean + (critical_sigma * std)`

### 2. BaselineAccumulator

**File**: `src/baseline/mod.rs` (lines 130-220)

Collects samples during the learning phase using Welford's algorithm for numerical stability.

```rust
pub struct BaselineAccumulator {
    equipment_id: String,
    sensor_id: String,
    count: usize,
    mean: f64,
    m2: f64,              // Sum of squared differences from mean
    min_value: f64,
    max_value: f64,
    outlier_count: usize, // For contamination detection
    warning_sigma: f64,
    critical_sigma: f64,
}
```

**Why Welford's Algorithm?**

Traditional variance calculation `Σ(x - mean)²` requires two passes and can suffer from catastrophic cancellation. Welford's single-pass algorithm:

```rust
fn add_sample(&mut self, value: f64) {
    self.count += 1;
    let delta = value - self.mean;
    self.mean += delta / self.count as f64;
    let delta2 = value - self.mean;
    self.m2 += delta * delta2;  // Numerically stable
}
```

**Contamination Detection**:
- Tracks samples that exceed 5σ from running mean
- If >5% of samples are outliers, baseline is flagged as potentially contaminated
- Prevents locking bad baselines learned during fault conditions

### 3. ThresholdManager

**File**: `src/baseline/mod.rs` (lines 250-450)

Central coordinator for all baseline operations.

```rust
pub struct ThresholdManager {
    thresholds: HashMap<String, DynamicThresholds>,
    accumulators: HashMap<String, BaselineAccumulator>,
    schema_version: u32,  // Currently v1
}
```

**Key Methods**:

| Method | Purpose |
|--------|---------|
| `start_learning(equipment_id, sensor_id)` | Begin accumulating samples for a metric |
| `add_sample(equipment_id, sensor_id, value)` | Feed a sample during learning |
| `finalize_learning(equipment_id, sensor_id)` | Lock baseline and compute final stats |
| `check_anomaly(equipment_id, sensor_id, value)` | Check value against learned baseline |
| `get_thresholds(equipment_id, sensor_id)` | Retrieve current thresholds |
| `save_to_file(path)` | Persist to JSON |
| `load_from_file(path)` | Restore from JSON |

### 4. AnomalyCheckResult

**File**: `src/baseline/mod.rs` (lines 95-125)

Return type for anomaly checks with full context.

```rust
pub struct AnomalyCheckResult {
    pub value: f64,
    pub z_score: f64,
    pub baseline_mean: f64,
    pub baseline_std: f64,
    pub level: AnomalyLevel,
    pub threshold_used: f64,  // The threshold that was exceeded (if any)
}

pub enum AnomalyLevel {
    Normal,
    Warning,   // |z| >= 3.0
    Critical,  // |z| >= 5.0
}
```

### 5. TDS-Specific Metrics

**File**: `src/baseline/mod.rs` (lines 470-520)

Pre-defined metric configurations for Top Drive Systems:

```rust
pub mod tds_metrics {
    pub const VIBRATION_RMS: &str = "vibration_rms";
    pub const BPFO_AMPLITUDE: &str = "bpfo_amplitude";
    pub const BPFI_AMPLITUDE: &str = "bpfi_amplitude";
    pub const BSF_AMPLITUDE: &str = "bsf_amplitude";
    pub const FTF_AMPLITUDE: &str = "ftf_amplitude";
    pub const KURTOSIS: &str = "kurtosis";
    pub const CREST_FACTOR: &str = "crest_factor";
    pub const TEMPERATURE: &str = "temperature";

    pub fn all_metrics() -> Vec<&'static str> { ... }
    pub fn default_sigmas(metric: &str) -> (f64, f64) { ... }
}
```

---

## TacticalAgent Integration

**File**: `src/agents/tactical.rs`

### Operating Modes

```rust
pub enum TacticalMode {
    FixedThresholds,    // Original hardcoded behavior
    BaselineLearning,   // Accumulating samples, no anomaly detection
    DynamicThresholds,  // Using learned baselines for detection
}
```

### New Fields

```rust
pub struct TacticalAgent {
    // ... existing fields ...
    mode: TacticalMode,
    equipment_id: String,
    threshold_manager: Option<Arc<RwLock<ThresholdManager>>>,
}
```

### New Constructor

```rust
pub fn new_with_thresholds(
    threshold_manager: Arc<RwLock<ThresholdManager>>,
    equipment_id: String,
    mode: TacticalMode,
) -> Self
```

### Baseline Sample Feeding

During learning mode, the `feed_baseline_samples()` method extracts metrics from sensor packets:

```rust
pub fn feed_baseline_samples(&self, packet: &SensorPacket) {
    if let Some(ref manager) = self.threshold_manager {
        let mut mgr = manager.write().unwrap();

        // Feed RMS vibration
        mgr.add_sample(&self.equipment_id, "vibration_rms", packet.rms);

        // Feed bearing frequencies if available
        if let Some(bpfo) = packet.bpfo_amplitude {
            mgr.add_sample(&self.equipment_id, "bpfo_amplitude", bpfo);
        }
        // ... other metrics
    }
}
```

### Dynamic Anomaly Detection

In `DynamicThresholds` mode, `check_dynamic_anomaly()` replaces hardcoded thresholds:

```rust
fn check_dynamic_anomaly(&self, packet: &SensorPacket) -> Option<(String, AnomalyCheckResult)> {
    let manager = self.threshold_manager.as_ref()?.read().ok()?;

    // Check vibration RMS
    if let Some(result) = manager.check_anomaly(&self.equipment_id, "vibration_rms", packet.rms) {
        if result.level != AnomalyLevel::Normal {
            return Some(("vibration_rms".to_string(), result));
        }
    }

    // Check BPFO amplitude
    if let Some(bpfo) = packet.bpfo_amplitude {
        if let Some(result) = manager.check_anomaly(&self.equipment_id, "bpfo_amplitude", bpfo) {
            if result.level != AnomalyLevel::Normal {
                return Some(("bpfo_amplitude".to_string(), result));
            }
        }
    }

    // ... other metrics
    None
}
```

---

## StrategicAgent Integration

**File**: `src/agents/strategic.rs`

### New Fields

```rust
pub struct StrategicAgent {
    // ... existing fields ...
    equipment_id: String,
    threshold_manager: Option<Arc<std::sync::RwLock<ThresholdManager>>>,
}
```

### Z-Score Consistency Checking

The `check_zscore_consistency()` method analyzes historical packets to find sustained anomalies:

```rust
fn check_zscore_consistency(
    &self,
    history: &[SensorPacket],
    metric_sensor_id: &str,
    min_packets: usize,
) -> Option<(usize, f64, f64)>  // (count_above_warning, max_z, avg_z)
```

This helps the strategic agent distinguish:
- **Transient spikes**: Single high z-score readings (likely noise)
- **Sustained anomalies**: Multiple consecutive readings above threshold (likely real fault)

### Baseline Context in Verification

The `get_baseline_context()` method provides rich context for LLM verification:

```rust
fn get_baseline_context(&self, history: &[SensorPacket]) -> String {
    // Returns formatted string like:
    // "Baseline Context:
    //  - vibration_rms: baseline=2.45±0.32, current z-scores show
    //    5/10 packets above warning (max z=4.2, avg z=3.5)
    //  - bpfo_amplitude: baseline=0.0023±0.0008, current z-scores show
    //    8/10 packets above warning (max z=6.1, avg z=4.8)"
}
```

### Enhanced Verification Logic

The `apply_verification_logic_with_history()` method now includes baseline evidence:

```rust
fn apply_verification_logic_with_history(&self, ticket: &Ticket, history: &[SensorPacket]) -> VerificationResult {
    let baseline_context = self.get_baseline_context(history);

    // Check z-score consistency for the flagged metric
    if let Some((count, max_z, _avg_z)) = self.check_zscore_consistency(history, &metric_id, 3) {
        if count >= 3 && max_z > 4.0 {
            reasoning.push_str(&format!(
                "Z-score analysis: {}/{} packets above warning threshold (max z={:.1}). ",
                count, history.len(), max_z
            ));
            confidence_boost += 0.1;
        }
    }

    // Include baseline context in LLM prompt
    // ...
}
```

---

## Dashboard API

**File**: `src/api/handlers.rs`

### Updated DashboardState

```rust
pub struct DashboardState {
    pub app_state: Arc<RwLock<AppState>>,
    pub storage: Option<crate::storage::AnalysisStorage>,
    pub strategic_storage: Option<crate::storage::StrategicStorage>,
    pub threshold_manager: Option<Arc<std::sync::RwLock<ThresholdManager>>>,
    pub equipment_id: String,
}
```

### New Endpoint: GET /api/v1/baseline

**File**: `src/api/routes.rs` (line 27)

```rust
.route("/baseline", get(handlers::get_baseline_status))
```

**Response Structure**:

```rust
pub struct BaselineStatusResponse {
    pub equipment_id: String,
    pub learning_active: bool,
    pub metrics: Vec<MetricBaselineStatus>,
}

pub struct MetricBaselineStatus {
    pub sensor_id: String,
    pub locked: bool,
    pub sample_count: usize,
    pub baseline_mean: Option<f64>,
    pub baseline_std: Option<f64>,
    pub warning_threshold: Option<f64>,
    pub critical_threshold: Option<f64>,
    pub contamination_detected: bool,
}
```

**Example Response**:

```json
{
  "equipment_id": "TDS",
  "learning_active": false,
  "metrics": [
    {
      "sensor_id": "vibration_rms",
      "locked": true,
      "sample_count": 1000,
      "baseline_mean": 2.45,
      "baseline_std": 0.32,
      "warning_threshold": 3.41,
      "critical_threshold": 4.05,
      "contamination_detected": false
    },
    {
      "sensor_id": "bpfo_amplitude",
      "locked": true,
      "sample_count": 1000,
      "baseline_mean": 0.0023,
      "baseline_std": 0.0008,
      "warning_threshold": 0.0047,
      "critical_threshold": 0.0063,
      "contamination_detected": false
    }
  ]
}
```

---

## Persistence

### File Format

Thresholds are saved as JSON with schema versioning:

```json
{
  "schema_version": 1,
  "thresholds": {
    "TDS:vibration_rms": {
      "equipment_id": "TDS",
      "sensor_id": "vibration_rms",
      "baseline_mean": 2.45,
      "baseline_std": 0.32,
      "warning_sigma": 3.0,
      "critical_sigma": 5.0,
      "locked": true,
      "locked_timestamp": 1706054400,
      "sample_count": 1000,
      "min_value": 1.82,
      "max_value": 3.21
    }
  }
}
```

### Usage

```rust
// Save
manager.save_to_file("thresholds.json")?;

// Load
let manager = ThresholdManager::load_from_file("thresholds.json")?;
```

---

## Usage Workflow

### 1. Commissioning (Baseline Learning)

```rust
// Create manager and start learning
let manager = Arc::new(RwLock::new(ThresholdManager::new()));

{
    let mut mgr = manager.write().unwrap();
    mgr.start_learning("TDS", "vibration_rms");
    mgr.start_learning("TDS", "bpfo_amplitude");
    // ... other metrics
}

// Create tactical agent in learning mode
let tactical = TacticalAgent::new_with_thresholds(
    manager.clone(),
    "TDS".to_string(),
    TacticalMode::BaselineLearning,
);

// Process healthy baseline data (typically 1-4 hours of normal operation)
for packet in baseline_packets {
    tactical.feed_baseline_samples(&packet);
}

// Finalize learning
{
    let mut mgr = manager.write().unwrap();
    for metric in tds_metrics::all_metrics() {
        if let Err(e) = mgr.finalize_learning("TDS", metric) {
            warn!("Could not finalize {}: {}", metric, e);
        }
    }
    mgr.save_to_file("thresholds.json")?;
}
```

### 2. Normal Operation (Dynamic Thresholds)

```rust
// Load saved thresholds
let manager = Arc::new(RwLock::new(
    ThresholdManager::load_from_file("thresholds.json")?
));

// Create tactical agent in dynamic mode
let tactical = TacticalAgent::new_with_thresholds(
    manager.clone(),
    "TDS".to_string(),
    TacticalMode::DynamicThresholds,
);

// Process live data - anomalies detected via z-score
for packet in live_packets {
    let (ticket, metrics, history_entry) = tactical.process(&packet);
    if let Some(t) = ticket {
        // Ticket generated based on z-score exceeding threshold
        strategic.verify(t);
    }
}
```

### 3. Fallback to Fixed Thresholds

```rust
// If no baseline available, use original behavior
let tactical = TacticalAgent::new();  // Uses TacticalMode::FixedThresholds
```

---

## Test Coverage

New tests added in `src/baseline/mod.rs`:

| Test | Purpose |
|------|---------|
| `test_welford_algorithm` | Verifies mean/variance calculation accuracy |
| `test_z_score_calculation` | Validates z-score computation |
| `test_contamination_detection` | Ensures outliers are detected |
| `test_min_std_floor` | Confirms minimum std of 1e-10 prevents division by zero |
| `test_threshold_manager_workflow` | End-to-end learning and checking |

Existing tests updated in:
- `src/api/handlers.rs` - Added `test_get_baseline_not_configured`
- `src/api/routes.rs` - Added `test_api_routes_baseline`, updated `create_test_state()`

**Test Results**: All 133 tests pass.

---

## Files Modified

| File | Changes |
|------|---------|
| `src/baseline/mod.rs` | **NEW** - Core baseline learning module (~500 lines) |
| `src/lib.rs` | Added `pub mod baseline;` |
| `src/main.rs` | Added `pub mod baseline;` |
| `src/agents/tactical.rs` | Added TacticalMode, threshold_manager, z-score detection |
| `src/agents/strategic.rs` | Added baseline context, z-score consistency checking |
| `src/api/handlers.rs` | Added threshold_manager to DashboardState, baseline endpoint |
| `src/api/routes.rs` | Added `/baseline` route |

---

## Future Considerations

1. **Automatic Re-learning**: Detect when baselines become stale and trigger re-learning
2. **Per-State Baselines**: Different baselines for Idle vs Drilling vs Circulating
3. **Adaptive Sigma**: Automatically tune warning/critical sigma based on false positive rate
4. **Baseline Comparison**: Compare current equipment baseline to fleet-wide norms
5. **Trend Detection**: Track baseline drift over time for predictive maintenance
