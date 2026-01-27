# ML Engine Integration Plan V2.1: Campaign-Aware Optimal Drilling Conditions

## Summary

Add an ML engine that analyzes drilling data to find optimal drilling conditions (WOB, RPM, flow) for each formation type and campaign mode. The engine runs on a configurable interval, learns from historical patterns, and enhances LLM advisories with statistically validated, data-driven recommendations.

**Key Features**:
- Campaign-awareness (Production vs P&A optimization goals)
- Composite efficiency scoring (ROP + MSE balance, campaign-specific weights)
- Formation boundary detection and segmentation
- Statistical significance testing (p-value filtering via `statrs`)
- Multi-well/field-level knowledge transfer (future phase)

**V2 Changes**: Nine engineering improvements for production-grade robustness.
**V2.1 Changes**: Three critical corrections for statistical accuracy and campaign safety.

---

## V2 Engineering Improvements

| # | Improvement | Rationale |
|---|-------------|-----------|
| 1 | **Composite Efficiency Scoring** | Prevents recommending aggressive params that destroy bits |
| 2 | **Stricter Confidence Thresholds** | 30 min minimum filters transient noise |
| 3 | **Formation Boundary Segmentation** | Avoids averaging across different rock types |
| 4 | **Data Quality Pre-Filtering** | Rejects connection noise and sensor glitches |
| 5 | **Bit Wear Context** | Optimal params change as bit dulls |
| 6 | **Multi-Well Storage Schema** | Enables cross-well learning in same field |
| 7 | **Statistical Significance Testing** | Only reports correlations where p < 0.05 |
| 8 | **Configurable Intervals** | `ML_INTERVAL_SECS` env var for testing |
| 9 | **Explicit Failure Modes** | LLM knows *why* no advice exists |

---

## V2.1 Critical Corrections

| # | Correction | Change |
|---|------------|--------|
| 1 | **Use `statrs` for Statistics** | Replaced 100+ lines of custom math with `statrs::distribution::StudentsT` for accurate p-values |
| 2 | **Campaign-Specific Weights** | `get_weights(campaign)`: Production (0.6, 0.4), P&A (0.3, 0.7) |
| 3 | **Composite Score Interpretation** | `interpret_composite_score()`: >0.75=EXCELLENT, >0.60=GOOD, >0.45=ACCEPTABLE, else=POOR |

---

## Architecture Overview

```
                    SAIREN-OS ML Engine V2 Architecture
═══════════════════════════════════════════════════════════════════════════════

Real-Time Pipeline (existing)              ML Engine V2 (configurable interval)
─────────────────────────────              ─────────────────────────────────────

┌─────────────┐                            ┌───────────────────────────────────┐
│ WITS Packets│                            │   Configurable Scheduler          │
│   (1 Hz)    │                            │   (ML_INTERVAL_SECS env var)      │
└──────┬──────┘                            └─────────────┬─────────────────────┘
       │                                                 │
       ▼                                                 ▼
┌─────────────┐     writes to              ┌───────────────────────────────────┐
│  Pipeline   │ ─────────────────────────► │   Drilling History Database       │
│ Coordinator │     history_buffer         │   (Sled: well_id + field indexed) │
└──────┬──────┘                            └─────────────┬─────────────────────┘
       │                                                 │
       ▼                                                 │ reads interval data
┌─────────────┐                                          ▼
│  Strategic  │◄──── RAG query ──────────  ┌───────────────────────────────────┐
│     LLM     │                            │         ML Analyzer V2            │
└─────────────┘                            │  ┌─────────────────────────────┐  │
       │                                   │  │  1. Data Quality Filter     │  │
       │                                   │  │     (WOB>5, RPM>40, etc.)   │  │
       │                                   │  └──────────────┬──────────────┘  │
       │                                   │                 ▼                 │
       │                                   │  ┌─────────────────────────────┐  │
       │                                   │  │  2. Formation Segmenter     │  │
       │                                   │  │     (d-exp >15% shift)      │  │
       │                                   │  └──────────────┬──────────────┘  │
       │                                   │                 ▼                 │
       │                                   │  ┌─────────────────────────────┐  │
       │                                   │  │  3. Correlation Engine      │  │
       │                                   │  │     (Pearson + p-value)     │  │
       │                                   │  └──────────────┬──────────────┘  │
       │                                   │                 ▼                 │
       │                                   │  ┌─────────────────────────────┐  │
       │                                   │  │  4. Composite Scorer        │  │
       │                                   │  │     (0.6*ROP + 0.4*MSE_eff) │  │
       │                                   │  └──────────────┬──────────────┘  │
       │                                   │                 ▼                 │
       │                                   │  ┌─────────────────────────────┐  │
       │                                   │  │  5. Report Builder          │  │
       │                                   │  │     (or AnalysisFailure)    │  │
       │                                   │  └──────────────┬──────────────┘  │
       │                                   └────────────────┼──────────────────┘
       │                                                    │
       └──── retrieves ──────────────────► ┌────────────────┴──────────────────┐
             context                       │  ML Insights Store (Sled)         │
                                           │  Keys: well_id/field/campaign/ts  │
                                           └───────────────────────────────────┘
═══════════════════════════════════════════════════════════════════════════════
```

---

## Campaign-Aware ML Analysis

### Different Goals by Campaign

| Aspect | Production | Plug & Abandonment |
|--------|------------|-------------------|
| **Primary Goal** | Maximize ROP while maintaining efficiency | Operational stability, cement integrity |
| **Composite Weights** | **0.6×ROP + 0.4×MSE_efficiency** | **0.3×ROP + 0.7×MSE_efficiency** |
| **Optimization Focus** | Drill fast, but don't destroy bits | Stable operations, consistent parameters |
| **Key Metrics** | MSE efficiency, d-exponent, bit wear | Cement returns, pressure hold, barrier depth |
| **Historical Relevance** | Same field, similar formations | Same well type, similar barriers |

> **V2.1 Safety Note**: P&A uses 70% MSE_efficiency weight because aggressive ROP is dangerous during cementing and plug operations. The heavier stability weighting prevents recommending parameters that could compromise barrier integrity.

---

## Data Structures

### HourlyDataset (V2)
```rust
// src/types.rs (add)
use std::collections::HashMap;

/// Dataset for ML analysis over a time window
pub struct HourlyDataset {
    /// Raw WITS packets (after quality filtering)
    pub packets: Vec<WitsPacket>,
    /// Computed metrics for each packet
    pub metrics: Vec<DrillingMetrics>,
    /// Analysis window (start_ts, end_ts)
    pub time_range: (u64, u64),
    /// Average depth during window
    pub avg_depth: f64,
    /// Estimated formation type (from d-exponent clustering)
    pub formation_estimate: String,
    /// Active campaign mode
    pub campaign: Campaign,
    /// Breakdown of rig states in window
    pub rig_states_breakdown: HashMap<RigState, usize>,

    // === V2 Additions ===
    /// Well identifier for multi-well storage
    pub well_id: String,
    /// Field/asset name for cross-well queries
    pub field_name: String,
    /// Cumulative bit hours at window start
    pub bit_hours: f64,
    /// Depth drilled on current bit (ft)
    pub bit_depth: f64,
    /// Number of samples rejected by quality filter
    pub rejected_sample_count: usize,
    /// Detected formation segments (if boundary found)
    pub formation_segments: Vec<FormationSegment>,
}

/// A contiguous segment within a single formation
pub struct FormationSegment {
    /// Index range in packets vec [start, end)
    pub packet_range: (usize, usize),
    /// Estimated formation type
    pub formation_type: String,
    /// Average d-exponent in segment
    pub avg_d_exponent: f64,
    /// Sample count after quality filtering
    pub valid_sample_count: usize,
}
```

### MLInsightsReport (V2)
```rust
// src/types.rs (add)

/// Result of ML analysis - either successful insights or explicit failure
pub struct MLInsightsReport {
    pub timestamp: u64,
    pub campaign: Campaign,
    pub depth_range: (f64, f64),

    // === V2: Multi-well identification ===
    pub well_id: String,
    pub field_name: String,

    // === V2: Bit wear context ===
    pub bit_hours: f64,
    pub bit_depth: f64,

    /// Formation analyzed (or "Mixed" if segmented)
    pub formation_type: String,

    /// Analysis result - Success or Failure with reason
    pub result: AnalysisResult,
}

/// Analysis outcome with explicit failure modes
pub enum AnalysisResult {
    /// Successful analysis with insights
    Success(AnalysisInsights),
    /// Analysis failed - explicit reason for LLM context
    Failure(AnalysisFailure),
}

/// Successful analysis insights
pub struct AnalysisInsights {
    /// Optimal drilling parameters (composite-scored)
    pub optimal_params: OptimalParams,
    /// Statistically significant correlations only (p < 0.05)
    pub correlations: Vec<SignificantCorrelation>,
    /// Natural language summary for LLM
    pub summary_text: String,
    /// Overall confidence level
    pub confidence: ConfidenceLevel,
    /// Number of valid samples used
    pub sample_count: usize,
}

/// V2: Explicit failure reasons for LLM context
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisFailure {
    /// Less than 1800 valid samples (30 min minimum)
    InsufficientData { valid_samples: usize, required: usize },
    /// Formation changed >15% mid-window, segments too small individually
    UnstableFormation { segment_count: usize, max_segment_size: usize },
    /// No correlations met p < 0.05 threshold
    NoSignificantCorrelation { best_p_value: f64 },
    /// All data rejected by quality filter
    AllDataRejected { rejection_reason: &'static str },
    /// Campaign not suitable for optimization (e.g., Idle state)
    NotApplicable { reason: &'static str },
}

impl std::fmt::Display for AnalysisFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InsufficientData { valid_samples, required } =>
                write!(f, "Insufficient data: {} samples (need {})", valid_samples, required),
            Self::UnstableFormation { segment_count, max_segment_size } =>
                write!(f, "Unstable formation: {} segments, largest has {} samples", segment_count, max_segment_size),
            Self::NoSignificantCorrelation { best_p_value } =>
                write!(f, "No significant correlations (best p={:.3})", best_p_value),
            Self::AllDataRejected { rejection_reason } =>
                write!(f, "All data rejected: {}", rejection_reason),
            Self::NotApplicable { reason } =>
                write!(f, "Analysis not applicable: {}", reason),
        }
    }
}
```

### OptimalParams (V2)
```rust
/// Optimal drilling parameters from composite efficiency scoring
pub struct OptimalParams {
    pub best_wob: f64,
    pub best_rpm: f64,
    pub best_flow: f64,
    /// ROP achieved at optimal params
    pub achieved_rop: f64,
    /// MSE achieved at optimal params
    pub achieved_mse: f64,
    /// MSE efficiency (0-100%)
    pub mse_efficiency: f64,
    /// Composite efficiency score used for ranking
    pub composite_score: f64,
    /// Confidence level (requires 1800+ samples for High)
    pub confidence: ConfidenceLevel,
}

/// V2: Stricter confidence levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfidenceLevel {
    /// >= 1800 samples (30+ min of clean data)
    High,
    /// 900-1799 samples (15-30 min)
    Medium,
    /// 360-899 samples (6-15 min) - use with caution
    Low,
    /// < 360 samples - insufficient for any recommendation
    Insufficient,
}

impl ConfidenceLevel {
    pub fn from_sample_count(n: usize) -> Self {
        match n {
            n if n >= 1800 => Self::High,
            n if n >= 900 => Self::Medium,
            n if n >= 360 => Self::Low,
            _ => Self::Insufficient,
        }
    }
}
```

### SignificantCorrelation (V2)
```rust
/// Correlation that passed statistical significance test
pub struct SignificantCorrelation {
    pub x_param: String,
    pub y_param: String,
    /// Pearson correlation coefficient (-1 to 1)
    pub r_value: f64,
    /// Coefficient of determination (r²)
    pub r_squared: f64,
    /// V2: p-value for significance testing
    pub p_value: f64,
    /// Sample count used for calculation
    pub sample_count: usize,
}
```

### Data Quality Thresholds
```rust
/// V2: Data quality filter thresholds
pub mod ml_quality_thresholds {
    /// Minimum WOB to consider "drilling" (klbs)
    pub const MIN_WOB: f64 = 5.0;
    /// Minimum RPM to consider "rotating"
    pub const MIN_RPM: f64 = 40.0;
    /// Maximum plausible MSE (psi) - reject sensor glitches
    pub const MAX_PLAUSIBLE_MSE: f64 = 500_000.0;
    /// Minimum plausible MSE (psi)
    pub const MIN_PLAUSIBLE_MSE: f64 = 1_000.0;
    /// Minimum ROP to consider "making hole" (ft/hr)
    pub const MIN_ROP: f64 = 1.0;
    /// Maximum plausible ROP (ft/hr)
    pub const MAX_PLAUSIBLE_ROP: f64 = 500.0;
    /// D-exponent shift threshold for formation boundary (%)
    pub const FORMATION_BOUNDARY_SHIFT: f64 = 0.15;
    /// Minimum samples for high confidence
    pub const HIGH_CONFIDENCE_SAMPLES: usize = 1800;
    /// Minimum samples for any analysis
    pub const MIN_ANALYSIS_SAMPLES: usize = 360;
    /// P-value threshold for statistical significance
    pub const SIGNIFICANCE_THRESHOLD: f64 = 0.05;
}
```

---

## Implementation Phases

### Phase 1: Data Collection & Quality Filtering
**Files**: `src/storage/history.rs`, `src/types.rs`, `src/ml_engine/quality_filter.rs`

**Tasks**:
1. Add all V2 data structures to `types.rs`
2. Add `well_id` and `field_name` to AppState (configurable via env/config)
3. Implement `DataQualityFilter`:

```rust
// src/ml_engine/quality_filter.rs

use crate::types::{WitsPacket, DrillingMetrics, ml_quality_thresholds::*};

pub struct DataQualityFilter;

impl DataQualityFilter {
    /// Filter packets to only valid drilling data
    /// Returns (valid_packets, valid_metrics, rejected_count)
    pub fn filter(
        packets: &[WitsPacket],
        metrics: &[DrillingMetrics],
    ) -> (Vec<&WitsPacket>, Vec<&DrillingMetrics>, usize) {
        let mut valid_packets = Vec::new();
        let mut valid_metrics = Vec::new();
        let mut rejected = 0;

        for (packet, metric) in packets.iter().zip(metrics.iter()) {
            if Self::is_valid(packet, metric) {
                valid_packets.push(packet);
                valid_metrics.push(metric);
            } else {
                rejected += 1;
            }
        }

        (valid_packets, valid_metrics, rejected)
    }

    fn is_valid(packet: &WitsPacket, metric: &DrillingMetrics) -> bool {
        // Reject connection/idle data
        if packet.wob < MIN_WOB {
            return false;
        }
        if packet.rpm < MIN_RPM {
            return false;
        }

        // Reject sensor glitches
        if metric.mse < MIN_PLAUSIBLE_MSE || metric.mse > MAX_PLAUSIBLE_MSE {
            return false;
        }
        if packet.rop < MIN_ROP || packet.rop > MAX_PLAUSIBLE_ROP {
            return false;
        }

        // Reject non-drilling states
        matches!(packet.rig_state, RigState::Drilling | RigState::Reaming)
    }
}
```

4. Add `export_dataset_for_ml()` method to HistoryStorage:

```rust
// src/storage/history.rs (add method)

impl HistoryStorage {
    pub fn export_dataset_for_ml(
        &self,
        duration_secs: u64,
        well_id: &str,
        field_name: &str,
        campaign: Campaign,
        bit_hours: f64,
        bit_depth: f64,
    ) -> HourlyDataset {
        // 1. Get raw data from history buffer
        // 2. Apply DataQualityFilter
        // 3. Run FormationSegmenter
        // 4. Build HourlyDataset with V2 fields
    }
}
```

**Verification**: Unit test that quality filter rejects WOB < 5 samples

---

### Phase 2: Formation Boundary Segmentation
**Files**: `src/ml_engine/formation_segmenter.rs`

**Tasks**:
1. Implement formation boundary detection:

```rust
// src/ml_engine/formation_segmenter.rs

use crate::types::{WitsPacket, FormationSegment, ml_quality_thresholds::*};

pub struct FormationSegmenter;

impl FormationSegmenter {
    /// Detect formation boundaries and split into segments
    /// A boundary is detected when d-exponent shifts >15% over 60 samples
    pub fn segment(packets: &[&WitsPacket]) -> Vec<FormationSegment> {
        if packets.len() < 120 {
            // Too few samples to detect boundaries reliably
            return vec![Self::single_segment(packets)];
        }

        let mut segments = Vec::new();
        let mut segment_start = 0;
        let window_size = 60; // 1-minute rolling window

        // Calculate rolling average d-exponent
        let d_exp_values: Vec<f64> = packets.iter().map(|p| p.d_exponent).collect();

        for i in window_size..packets.len() {
            let prev_avg = Self::mean(&d_exp_values[i - window_size..i - window_size / 2]);
            let curr_avg = Self::mean(&d_exp_values[i - window_size / 2..i]);

            // Check for >15% shift
            if prev_avg > 0.0 {
                let shift = (curr_avg - prev_avg).abs() / prev_avg;
                if shift > FORMATION_BOUNDARY_SHIFT {
                    // Formation boundary detected
                    segments.push(FormationSegment {
                        packet_range: (segment_start, i - window_size / 2),
                        formation_type: Self::estimate_formation(prev_avg),
                        avg_d_exponent: prev_avg,
                        valid_sample_count: i - window_size / 2 - segment_start,
                    });
                    segment_start = i - window_size / 2;
                }
            }
        }

        // Add final segment
        segments.push(FormationSegment {
            packet_range: (segment_start, packets.len()),
            formation_type: Self::estimate_formation(
                Self::mean(&d_exp_values[segment_start..])
            ),
            avg_d_exponent: Self::mean(&d_exp_values[segment_start..]),
            valid_sample_count: packets.len() - segment_start,
        });

        segments
    }

    fn estimate_formation(avg_d_exp: f64) -> String {
        // Formation hardness estimation from d-exponent
        match avg_d_exp {
            d if d < 1.2 => "Soft Shale".to_string(),
            d if d < 1.6 => "Medium Shale/Siltstone".to_string(),
            d if d < 2.0 => "Hard Sandstone".to_string(),
            _ => "Very Hard Formation".to_string(),
        }
    }

    fn mean(values: &[f64]) -> f64 {
        if values.is_empty() { 0.0 } else { values.iter().sum::<f64>() / values.len() as f64 }
    }

    fn single_segment(packets: &[&WitsPacket]) -> FormationSegment {
        let d_exp_values: Vec<f64> = packets.iter().map(|p| p.d_exponent).collect();
        let avg = Self::mean(&d_exp_values);
        FormationSegment {
            packet_range: (0, packets.len()),
            formation_type: Self::estimate_formation(avg),
            avg_d_exponent: avg,
            valid_sample_count: packets.len(),
        }
    }
}
```

**Verification**: Unit test detects boundary when d-exp jumps from 1.5 to 2.0

---

### Phase 3: Statistical Correlation Engine
**Files**: `src/ml_engine/correlations.rs`

**Tasks**:
1. Implement Pearson correlation with p-value calculation:

```rust
// src/ml_engine/correlations.rs

use crate::types::{SignificantCorrelation, WitsPacket, ml_quality_thresholds::*};
use statrs::distribution::{StudentsT, ContinuousCDF};

pub struct CorrelationEngine;

impl CorrelationEngine {
    /// Calculate Pearson correlation with statistical significance
    /// V2.1: Uses statrs for accurate p-value calculation
    pub fn calculate(
        x: &[f64],
        y: &[f64],
        x_name: &str,
        y_name: &str,
    ) -> Option<SignificantCorrelation> {
        let n = x.len();
        if n < 30 {
            return None; // Minimum for meaningful correlation
        }

        let r = Self::pearson(x, y);
        let p_value = Self::p_value_for_r(r, n);

        // V2: Only return if statistically significant
        if p_value >= SIGNIFICANCE_THRESHOLD {
            return None;
        }

        Some(SignificantCorrelation {
            x_param: x_name.to_string(),
            y_param: y_name.to_string(),
            r_value: r,
            r_squared: r * r,
            p_value,
            sample_count: n,
        })
    }

    /// Pearson correlation coefficient
    fn pearson(x: &[f64], y: &[f64]) -> f64 {
        let n = x.len() as f64;
        let sum_x: f64 = x.iter().sum();
        let sum_y: f64 = y.iter().sum();
        let sum_xy: f64 = x.iter().zip(y.iter()).map(|(a, b)| a * b).sum();
        let sum_x2: f64 = x.iter().map(|a| a * a).sum();
        let sum_y2: f64 = y.iter().map(|a| a * a).sum();

        let numerator = n * sum_xy - sum_x * sum_y;
        let denominator = ((n * sum_x2 - sum_x.powi(2)) * (n * sum_y2 - sum_y.powi(2))).sqrt();

        if denominator == 0.0 { 0.0 } else { numerator / denominator }
    }

    /// V2.1: Calculate p-value using statrs StudentsT distribution
    /// Formula: t = r * sqrt(n-2) / sqrt(1-r²), then two-tailed p-value
    fn p_value_for_r(r: f64, n: usize) -> f64 {
        if n < 3 || r.abs() >= 1.0 {
            return 1.0;
        }

        let df = (n - 2) as f64;
        let t_stat = r * df.sqrt() / (1.0 - r * r).sqrt();

        // Use statrs for accurate t-distribution CDF
        match StudentsT::new(0.0, 1.0, df) {
            Ok(t_dist) => {
                // Two-tailed p-value
                2.0 * (1.0 - t_dist.cdf(t_stat.abs()))
            }
            Err(_) => 1.0, // Fallback if distribution creation fails
        }
    }

    /// Analyze all relevant parameter correlations
    pub fn analyze_drilling_correlations(
        packets: &[&WitsPacket],
    ) -> (Vec<SignificantCorrelation>, f64) {
        let wob: Vec<f64> = packets.iter().map(|p| p.wob).collect();
        let rpm: Vec<f64> = packets.iter().map(|p| p.rpm).collect();
        let flow: Vec<f64> = packets.iter().map(|p| p.flow_in).collect();
        let rop: Vec<f64> = packets.iter().map(|p| p.rop).collect();
        let mse: Vec<f64> = packets.iter().map(|p| p.mse).collect();

        let mut correlations = Vec::new();
        let mut best_p = 1.0;

        // WOB correlations
        if let Some(c) = Self::calculate(&wob, &rop, "WOB", "ROP") {
            best_p = best_p.min(c.p_value);
            correlations.push(c);
        }
        if let Some(c) = Self::calculate(&wob, &mse, "WOB", "MSE") {
            best_p = best_p.min(c.p_value);
            correlations.push(c);
        }

        // RPM correlations
        if let Some(c) = Self::calculate(&rpm, &rop, "RPM", "ROP") {
            best_p = best_p.min(c.p_value);
            correlations.push(c);
        }
        if let Some(c) = Self::calculate(&rpm, &mse, "RPM", "MSE") {
            best_p = best_p.min(c.p_value);
            correlations.push(c);
        }

        // Flow correlations
        if let Some(c) = Self::calculate(&flow, &rop, "Flow", "ROP") {
            best_p = best_p.min(c.p_value);
            correlations.push(c);
        }

        (correlations, best_p)
    }
}
```

**Dependencies** (Cargo.toml):
```toml
ndarray = "0.15"
statrs = "0.16"   # V2.1: Required for accurate p-value calculations
```

**Verification**: Unit test that r=0.2 with n=100 has p > 0.05 (rejected)

---

### Phase 4: Composite Efficiency Scoring
**Files**: `src/ml_engine/optimal_finder.rs`

**Tasks**:
1. Implement composite scoring (V2 improvement #1):

```rust
// src/ml_engine/optimal_finder.rs

use crate::types::{
    Campaign, WitsPacket, DrillingMetrics, OptimalParams, ConfidenceLevel,
    ml_quality_thresholds::*,
};

/// V2.1: Campaign-specific composite weights
/// Returns (rop_weight, mse_efficiency_weight)
fn get_weights(campaign: Campaign) -> (f64, f64) {
    match campaign {
        // Production: ROP-focused (drill fast, but efficiently)
        Campaign::Production => (0.6, 0.4),
        // P&A: Stability-focused (MSE efficiency = operational stability)
        Campaign::PlugAbandonment => (0.3, 0.7),
    }
}

pub struct OptimalFinder;

impl OptimalFinder {
    /// Find optimal parameters using campaign-specific composite efficiency scoring
    /// V2.1: Now accepts campaign parameter for appropriate weight selection
    pub fn find_optimal(
        packets: &[&WitsPacket],
        metrics: &[&DrillingMetrics],
        campaign: Campaign,
    ) -> Option<OptimalParams> {
        let n = packets.len();
        if n < MIN_ANALYSIS_SAMPLES {
            return None;
        }

        // V2.1: Get campaign-specific weights
        let (rop_weight, mse_weight) = get_weights(campaign);

        // Calculate composite scores for each sample
        let scores: Vec<(usize, f64)> = Self::calculate_composite_scores(
            packets, metrics, rop_weight, mse_weight
        );

        // Sort by composite score descending
        let mut sorted_scores = scores.clone();
        sorted_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Take top 10% performers
        let top_count = (n as f64 * 0.10).max(10.0) as usize;
        let top_indices: Vec<usize> = sorted_scores.iter().take(top_count).map(|(i, _)| *i).collect();

        // Average parameters from top performers
        let avg_wob = Self::mean_at_indices(packets, &top_indices, |p| p.wob);
        let avg_rpm = Self::mean_at_indices(packets, &top_indices, |p| p.rpm);
        let avg_flow = Self::mean_at_indices(packets, &top_indices, |p| p.flow_in);
        let avg_rop = Self::mean_at_indices(packets, &top_indices, |p| p.rop);
        let avg_mse = Self::mean_at_indices(packets, &top_indices, |p| p.mse);
        let avg_mse_eff = Self::mean_at_indices_metrics(metrics, &top_indices, |m| m.mse_efficiency);
        let avg_composite = sorted_scores.iter().take(top_count).map(|(_, s)| s).sum::<f64>() / top_count as f64;

        // V2: Stricter confidence based on sample count
        let confidence = ConfidenceLevel::from_sample_count(n);

        Some(OptimalParams {
            best_wob: avg_wob,
            best_rpm: avg_rpm,
            best_flow: avg_flow,
            achieved_rop: avg_rop,
            achieved_mse: avg_mse,
            mse_efficiency: avg_mse_eff,
            composite_score: avg_composite,
            confidence,
        })
    }

    /// Calculate composite efficiency score for each sample
    /// V2.1: Uses campaign-specific weights
    fn calculate_composite_scores(
        packets: &[&WitsPacket],
        metrics: &[&DrillingMetrics],
        rop_weight: f64,
        mse_weight: f64,
    ) -> Vec<(usize, f64)> {
        // Find min/max for normalization
        let rop_values: Vec<f64> = packets.iter().map(|p| p.rop).collect();
        let mse_eff_values: Vec<f64> = metrics.iter().map(|m| m.mse_efficiency).collect();

        let rop_min = rop_values.iter().cloned().fold(f64::INFINITY, f64::min);
        let rop_max = rop_values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let rop_range = (rop_max - rop_min).max(1.0);

        let mse_min = mse_eff_values.iter().cloned().fold(f64::INFINITY, f64::min);
        let mse_max = mse_eff_values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let mse_range = (mse_max - mse_min).max(1.0);

        packets.iter().zip(metrics.iter()).enumerate().map(|(i, (p, m))| {
            let norm_rop = (p.rop - rop_min) / rop_range;
            let norm_mse_eff = (m.mse_efficiency - mse_min) / mse_range;
            let composite = rop_weight * norm_rop + mse_weight * norm_mse_eff;
            (i, composite)
        }).collect()
    }

    fn mean_at_indices<F>(packets: &[&WitsPacket], indices: &[usize], f: F) -> f64
    where
        F: Fn(&WitsPacket) -> f64,
    {
        let sum: f64 = indices.iter().map(|&i| f(packets[i])).sum();
        sum / indices.len() as f64
    }

    fn mean_at_indices_metrics<F>(metrics: &[&DrillingMetrics], indices: &[usize], f: F) -> f64
    where
        F: Fn(&DrillingMetrics) -> f64,
    {
        let sum: f64 = indices.iter().map(|&i| f(metrics[i])).sum();
        sum / indices.len() as f64
    }
}
```

**Verification**: Unit test that high-ROP/low-MSE-efficiency samples rank lower than balanced performers

---

### Phase 5: Core Analyzer & Report Builder
**Files**: `src/ml_engine/analyzer.rs`, `src/ml_engine/report.rs`

**Tasks**:
1. Implement main analyzer orchestrating all V2 components:

```rust
// src/ml_engine/analyzer.rs

use crate::types::{
    HourlyDataset, MLInsightsReport, AnalysisResult, AnalysisInsights,
    AnalysisFailure, ConfidenceLevel, ml_quality_thresholds::*,
};
use super::{
    quality_filter::DataQualityFilter,
    formation_segmenter::FormationSegmenter,
    correlations::CorrelationEngine,
    optimal_finder::OptimalFinder,
};

pub struct HourlyAnalyzer;

impl HourlyAnalyzer {
    /// V2: Full analysis pipeline with explicit failure handling
    pub fn analyze(dataset: &HourlyDataset) -> MLInsightsReport {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Step 1: Quality filtering
        let (valid_packets, valid_metrics, rejected) =
            DataQualityFilter::filter(&dataset.packets, &dataset.metrics);

        // Check for all data rejected
        if valid_packets.is_empty() {
            return Self::build_failure_report(
                dataset,
                timestamp,
                AnalysisFailure::AllDataRejected {
                    rejection_reason: "No samples passed quality filter (WOB<5 or RPM<40)",
                },
            );
        }

        // V2: Check minimum sample requirement (1800 for high confidence)
        if valid_packets.len() < MIN_ANALYSIS_SAMPLES {
            return Self::build_failure_report(
                dataset,
                timestamp,
                AnalysisFailure::InsufficientData {
                    valid_samples: valid_packets.len(),
                    required: MIN_ANALYSIS_SAMPLES,
                },
            );
        }

        // Step 2: Formation segmentation
        let segments = FormationSegmenter::segment(&valid_packets);

        // V2: Check for unstable formation
        if segments.len() > 1 {
            let max_segment = segments.iter().map(|s| s.valid_sample_count).max().unwrap_or(0);
            if max_segment < MIN_ANALYSIS_SAMPLES {
                return Self::build_failure_report(
                    dataset,
                    timestamp,
                    AnalysisFailure::UnstableFormation {
                        segment_count: segments.len(),
                        max_segment_size: max_segment,
                    },
                );
            }
            // Use largest segment for analysis
            // TODO: Future - analyze each segment separately
        }

        // Use largest segment (or only segment)
        let best_segment = segments.iter().max_by_key(|s| s.valid_sample_count).unwrap();
        let (start, end) = best_segment.packet_range;
        let segment_packets: Vec<_> = valid_packets[start..end].to_vec();
        let segment_metrics: Vec<_> = valid_metrics[start..end].to_vec();

        // Step 3: Correlation analysis with p-value filtering
        let (correlations, best_p) = CorrelationEngine::analyze_drilling_correlations(&segment_packets);

        // V2: Check for no significant correlations
        if correlations.is_empty() {
            return Self::build_failure_report(
                dataset,
                timestamp,
                AnalysisFailure::NoSignificantCorrelation { best_p_value: best_p },
            );
        }

        // Step 4: Optimal parameter finding with campaign-specific composite scoring
        // V2.1: Pass campaign to get appropriate weights
        let optimal_params = match OptimalFinder::find_optimal(
            &segment_packets,
            &segment_metrics,
            dataset.campaign,
        ) {
            Some(params) => params,
            None => {
                return Self::build_failure_report(
                    dataset,
                    timestamp,
                    AnalysisFailure::InsufficientData {
                        valid_samples: segment_packets.len(),
                        required: MIN_ANALYSIS_SAMPLES,
                    },
                );
            }
        };

        // Step 5: Build success report
        let confidence = ConfidenceLevel::from_sample_count(segment_packets.len());
        let summary = Self::build_summary(
            &optimal_params,
            &correlations,
            &best_segment.formation_type,
            dataset,
            confidence,
        );

        MLInsightsReport {
            timestamp,
            campaign: dataset.campaign,
            depth_range: (
                segment_packets.first().map(|p| p.bit_depth).unwrap_or(0.0),
                segment_packets.last().map(|p| p.bit_depth).unwrap_or(0.0),
            ),
            well_id: dataset.well_id.clone(),
            field_name: dataset.field_name.clone(),
            bit_hours: dataset.bit_hours,
            bit_depth: dataset.bit_depth,
            formation_type: best_segment.formation_type.clone(),
            result: AnalysisResult::Success(AnalysisInsights {
                optimal_params,
                correlations,
                summary_text: summary,
                confidence,
                sample_count: segment_packets.len(),
            }),
        }
    }

    fn build_failure_report(
        dataset: &HourlyDataset,
        timestamp: u64,
        failure: AnalysisFailure,
    ) -> MLInsightsReport {
        MLInsightsReport {
            timestamp,
            campaign: dataset.campaign,
            depth_range: (dataset.avg_depth, dataset.avg_depth),
            well_id: dataset.well_id.clone(),
            field_name: dataset.field_name.clone(),
            bit_hours: dataset.bit_hours,
            bit_depth: dataset.bit_depth,
            formation_type: dataset.formation_estimate.clone(),
            result: AnalysisResult::Failure(failure),
        }
    }

    fn build_summary(
        params: &OptimalParams,
        correlations: &[SignificantCorrelation],
        formation: &str,
        dataset: &HourlyDataset,
        confidence: ConfidenceLevel,
    ) -> String {
        let confidence_str = match confidence {
            ConfidenceLevel::High => "HIGH confidence",
            ConfidenceLevel::Medium => "MEDIUM confidence",
            ConfidenceLevel::Low => "LOW confidence (use with caution)",
            ConfidenceLevel::Insufficient => "INSUFFICIENT data",
        };

        // V2.1: Interpret composite score for LLM context
        let efficiency_rating = Self::interpret_composite_score(params.composite_score);

        let strongest_corr = correlations.iter()
            .max_by(|a, b| a.r_value.abs().partial_cmp(&b.r_value.abs()).unwrap())
            .map(|c| format!("{} shows r={:.2} correlation with {} (p={:.3})",
                c.x_param, c.r_value, c.y_param, c.p_value))
            .unwrap_or_else(|| "No significant correlations".to_string());

        format!(
            "ML Analysis for {} in {} formation ({} confidence, bit: {:.0}hrs/{:.0}ft). \
             Optimal: WOB={:.1} klbs, RPM={:.0}, Flow={:.0} gpm. \
             Achieved ROP={:.1} ft/hr with MSE efficiency {:.0}% \
             (composite score: {:.2} - {}). \
             Key finding: {}.",
            dataset.well_id,
            formation,
            confidence_str,
            dataset.bit_hours,
            dataset.bit_depth,
            params.best_wob,
            params.best_rpm,
            params.best_flow,
            params.achieved_rop,
            params.mse_efficiency,
            params.composite_score,
            efficiency_rating,
            strongest_corr
        )
    }

    /// V2.1: Interpret composite score for LLM context
    /// Provides human-readable assessment of drilling efficiency
    fn interpret_composite_score(score: f64) -> &'static str {
        match score {
            s if s > 0.75 => "EXCELLENT drilling conditions",
            s if s > 0.60 => "GOOD efficiency",
            s if s > 0.45 => "ACCEPTABLE",
            _ => "POOR efficiency - optimization needed",
        }
    }
}
```

---

### Phase 6: Scheduler with Configurable Interval
**Files**: `src/ml_engine/scheduler.rs`, `src/main.rs`

**Tasks**:
1. Implement configurable scheduler (V2 improvement #8):

```rust
// src/ml_engine/scheduler.rs

use std::time::Duration;
use tokio::time::interval;
use tracing::{info, warn};

use crate::types::{Campaign, MLInsightsReport};
use crate::pipeline::processor::AppState;
use crate::storage::ml_insights::MLInsightsStorage;
use super::analyzer::HourlyAnalyzer;

/// Default interval: 1 hour
const DEFAULT_INTERVAL_SECS: u64 = 3600;

/// V2: Read interval from environment variable
pub fn get_interval_secs() -> u64 {
    std::env::var("ML_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_INTERVAL_SECS)
}

/// Run the ML analysis scheduler
pub async fn run_scheduler(
    app_state: std::sync::Arc<tokio::sync::RwLock<AppState>>,
    storage: MLInsightsStorage,
) {
    let interval_secs = get_interval_secs();
    info!(
        "ML Engine scheduler started with interval: {}s (env: ML_INTERVAL_SECS)",
        interval_secs
    );

    let mut interval = interval(Duration::from_secs(interval_secs));

    // Skip first tick (don't run immediately on startup)
    interval.tick().await;

    loop {
        interval.tick().await;

        let (campaign, well_id, field_name, bit_hours, bit_depth) = {
            let state = app_state.read().await;
            (
                state.campaign,
                state.well_id.clone(),
                state.field_name.clone(),
                state.bit_hours,
                state.bit_depth,
            )
        };

        info!(
            "Running ML analysis for well={}, field={}, campaign={:?}",
            well_id, field_name, campaign
        );

        match run_analysis(&app_state, campaign, &well_id, &field_name, bit_hours, bit_depth).await {
            Ok(report) => {
                match &report.result {
                    crate::types::AnalysisResult::Success(insights) => {
                        info!(
                            "ML analysis complete: {} samples, confidence={:?}",
                            insights.sample_count, insights.confidence
                        );
                    }
                    crate::types::AnalysisResult::Failure(failure) => {
                        warn!("ML analysis failed: {}", failure);
                    }
                }

                if let Err(e) = storage.store_report(&report) {
                    warn!("Failed to store ML report: {}", e);
                }
            }
            Err(e) => {
                warn!("ML analysis error: {}", e);
            }
        }
    }
}

async fn run_analysis(
    app_state: &std::sync::Arc<tokio::sync::RwLock<AppState>>,
    campaign: Campaign,
    well_id: &str,
    field_name: &str,
    bit_hours: f64,
    bit_depth: f64,
) -> Result<MLInsightsReport, String> {
    // Get dataset from history storage
    let dataset = {
        let state = app_state.read().await;
        state.history_storage.export_dataset_for_ml(
            get_interval_secs(),
            well_id,
            field_name,
            campaign,
            bit_hours,
            bit_depth,
        )
    };

    Ok(HourlyAnalyzer::analyze(&dataset))
}
```

2. Update `main.rs`:

```rust
// In main.rs, add to JoinSet

let ml_interval = ml_engine::scheduler::get_interval_secs();
tracing::info!("ML Engine interval: {}s", ml_interval);

let ml_handle = {
    let app_state = processor_app_state.clone();
    let ml_storage = MLInsightsStorage::open("./data/ml_insights.db")?;
    spawn(async move {
        ml_engine::scheduler::run_scheduler(app_state, ml_storage).await;
    })
};
```

**Verification**: Set `ML_INTERVAL_SECS=60` and verify analysis runs every minute

---

### Phase 7: Multi-Well Storage & RAG Integration
**Files**: `src/storage/ml_insights.rs`, `src/context/vector_db.rs`, `src/llm/strategic_llm.rs`

**Tasks**:

1. Implement multi-well storage schema:

```rust
// src/storage/ml_insights.rs

use crate::types::{MLInsightsReport, Campaign, AnalysisResult};
use sled::Db;

pub struct MLInsightsStorage {
    db: Db,
}

impl MLInsightsStorage {
    pub fn open(path: &str) -> Result<Self, sled::Error> {
        let db = sled::open(path)?;
        Ok(Self { db })
    }

    /// Store report with multi-well key structure
    /// Key format: {field_name}/{well_id}/{campaign}/{timestamp}
    pub fn store_report(&self, report: &MLInsightsReport) -> Result<(), StorageError> {
        let key = format!(
            "{}/{}/{:?}/{}",
            report.field_name,
            report.well_id,
            report.campaign,
            report.timestamp
        );
        let value = serde_json::to_vec(report)?;
        self.db.insert(key.as_bytes(), value)?;
        Ok(())
    }

    /// Query reports for same well
    pub fn get_well_history(
        &self,
        well_id: &str,
        campaign: Campaign,
        limit: usize,
    ) -> Vec<MLInsightsReport> {
        // Scan for matching prefix
        self.db.scan_prefix(format!("/{}/", well_id).as_bytes())
            .filter_map(|r| r.ok())
            .filter_map(|(_, v)| serde_json::from_slice(&v).ok())
            .filter(|r: &MLInsightsReport| r.campaign == campaign)
            .take(limit)
            .collect()
    }

    /// V2 Future: Query reports from same field (cross-well)
    pub fn get_field_history(
        &self,
        field_name: &str,
        campaign: Campaign,
        limit: usize,
    ) -> Vec<MLInsightsReport> {
        self.db.scan_prefix(format!("{}/", field_name).as_bytes())
            .filter_map(|r| r.ok())
            .filter_map(|(_, v)| serde_json::from_slice(&v).ok())
            .filter(|r: &MLInsightsReport| r.campaign == campaign)
            .take(limit)
            .collect()
    }
}
```

2. Update LLM prompt injection (V2: include failure context):

```rust
// src/llm/strategic_llm.rs (addition to prompt building)

fn build_ml_context(reports: &[MLInsightsReport], depth: f64) -> String {
    let mut context = String::from("### HISTORICAL ML INSIGHTS\n");

    for (i, report) in reports.iter().enumerate() {
        match &report.result {
            AnalysisResult::Success(insights) => {
                context.push_str(&format!(
                    "[{}] {} at {:.0}-{:.0}ft in {} (bit: {:.0}hrs, {} confidence):\n",
                    i + 1,
                    report.well_id,
                    report.depth_range.0,
                    report.depth_range.1,
                    report.formation_type,
                    report.bit_hours,
                    match insights.confidence {
                        ConfidenceLevel::High => "HIGH",
                        ConfidenceLevel::Medium => "MEDIUM",
                        ConfidenceLevel::Low => "LOW",
                        ConfidenceLevel::Insufficient => "INSUFFICIENT",
                    }
                ));
                context.push_str(&format!(
                    "    Optimal: WOB={:.1} klbs, RPM={:.0}, Flow={:.0} gpm\n",
                    insights.optimal_params.best_wob,
                    insights.optimal_params.best_rpm,
                    insights.optimal_params.best_flow,
                ));
                context.push_str(&format!(
                    "    Achieved: ROP={:.1} ft/hr, MSE_eff={:.0}%\n",
                    insights.optimal_params.achieved_rop,
                    insights.optimal_params.mse_efficiency,
                ));

                // Include strongest correlation
                if let Some(corr) = insights.correlations.first() {
                    context.push_str(&format!(
                        "    Correlation: {} vs {} r={:.2} (p={:.4})\n",
                        corr.x_param, corr.y_param, corr.r_value, corr.p_value
                    ));
                }
            }
            AnalysisResult::Failure(failure) => {
                // V2: Include failure context so LLM knows data quality issues
                context.push_str(&format!(
                    "[{}] {} at {:.0}ft: Analysis unavailable - {}\n",
                    i + 1,
                    report.well_id,
                    depth,
                    failure
                ));
            }
        }
        context.push('\n');
    }

    context
}
```

---

### Phase 8: API & Dashboard
**Files**: `src/api/handlers.rs`, `src/api/routes.rs`, `static/index.html`

**New Endpoints**:

| Endpoint | Description |
|----------|-------------|
| `GET /api/v1/ml/latest` | Most recent ML insights |
| `GET /api/v1/ml/latest?well_id=X` | Latest for specific well |
| `GET /api/v1/ml/history?hours=24` | Last N hours of reports |
| `GET /api/v1/ml/field/{field_name}` | Cross-well field insights (future) |
| `GET /api/v1/ml/optimal?depth=X&campaign=production` | Optimal params for conditions |

**Dashboard Widget** updates:
- Show confidence level badge (HIGH/MEDIUM/LOW)
- Display bit wear context (hours, depth drilled)
- Show failure reasons when analysis unavailable
- List statistically significant correlations with p-values

---

## Files to Create/Modify

| File | Action | Description |
|------|--------|-------------|
| `src/ml_engine/mod.rs` | **CREATE** | Module exports |
| `src/ml_engine/quality_filter.rs` | **CREATE** | V2: Data quality filtering |
| `src/ml_engine/formation_segmenter.rs` | **CREATE** | V2: Formation boundary detection |
| `src/ml_engine/correlations.rs` | **CREATE** | V2: Pearson + p-value |
| `src/ml_engine/optimal_finder.rs` | **CREATE** | V2: Composite scoring |
| `src/ml_engine/analyzer.rs` | **CREATE** | Main orchestrator |
| `src/ml_engine/scheduler.rs` | **CREATE** | V2: Configurable interval |
| `src/storage/ml_insights.rs` | **CREATE** | V2: Multi-well storage |
| `src/types.rs` | **MODIFY** | All V2 data structures |
| `src/storage/history.rs` | **MODIFY** | Add export_dataset_for_ml() |
| `src/context/vector_db.rs` | **MODIFY** | Add search_ml_insights() |
| `src/llm/strategic_llm.rs` | **MODIFY** | V2: Inject ML context with failures |
| `src/api/handlers.rs` | **MODIFY** | Add ML endpoints |
| `src/api/routes.rs` | **MODIFY** | Register ML routes |
| `src/main.rs` | **MODIFY** | Add ML scheduler |
| `src/pipeline/processor.rs` | **MODIFY** | Add well_id, field_name, bit_hours to AppState |
| `static/index.html` | **MODIFY** | ML insights widget |
| `Cargo.toml` | **MODIFY** | Add ndarray (optionally statrs) |

---

## Testing Strategy

1. **Unit Tests**:
   - Quality filter rejects WOB < 5, RPM < 40
   - Formation segmenter detects 20% d-exp shift
   - Correlation p-value: r=0.2, n=100 → p > 0.05 (rejected)
   - Composite scorer ranks balanced samples higher

2. **Integration**:
   ```bash
   # Test with short interval
   ML_INTERVAL_SECS=300 cargo run --release

   # Run simulator for 30+ minutes
   python wits_simulator.py --campaign production
   ```

3. **Failure Mode Testing**:
   - Connection-heavy data → `InsufficientData` failure
   - Formation change mid-window → `UnstableFormation` failure
   - Weak correlations → `NoSignificantCorrelation` failure

4. **Cross-Well** (Future):
   - Store insights from Well A
   - Query from Well B in same field

---

## Success Metrics

| Metric | Target |
|--------|--------|
| Report generation | 100% at configured interval |
| High confidence reports | >70% (when drilling steadily) |
| Correlation p-values | All reported < 0.05 |
| Failure mode coverage | 100% explicit reasons |
| Composite vs ROP-only | Measurable MSE improvement |

---

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `ML_INTERVAL_SECS` | 3600 | Analysis interval in seconds |
| `WELL_ID` | "WELL-001" | Current well identifier |
| `FIELD_NAME` | "DEFAULT" | Field/asset name |

### Config File (future)
```toml
[ml_engine]
interval_secs = 3600
min_samples_high_confidence = 1800
composite_rop_weight = 0.6
composite_mse_weight = 0.4
significance_threshold = 0.05
```

---

## Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| Insufficient data | Explicit `InsufficientData` failure + physics fallback |
| Formation changes | Segmentation + `UnstableFormation` failure |
| Weak correlations | p-value filtering + `NoSignificantCorrelation` |
| Bit wear ignored | bit_hours/bit_depth context in reports |
| Cross-well noise | Field-level storage schema (future filtering) |
| Sensor glitches | Quality filter (WOB, RPM, MSE bounds) |

---

## Future Phases

### Phase F1: Cross-Well Learning
- Query similar formations from other wells in same field
- Weight by formation similarity and recency
- Build field-level "playbook" of optimal parameters

### Phase F2: Bit Wear Normalization
- Track optimal param drift vs bit hours
- Recommend bit pull timing
- Adjust recommendations based on bit wear state

### Phase F3: Real-Time Streaming
- Sliding window analysis (not just hourly batch)
- Trigger on formation change detection
- Sub-hour insights during rapid drilling

---

*Plan V2 created: 2026-01-26*
*Plan V2.1 updated: 2026-01-26*
*Author: Claude (SAIREN-OS Development)*
*V2: 9 engineering improvements for production robustness*
*V2.1: 3 critical corrections (statrs, campaign weights, score interpretation)*
