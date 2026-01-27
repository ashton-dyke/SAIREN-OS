# ML Engine - Optimal Drilling Conditions Analysis

## Overview

The ML Engine (v2.2) is a campaign-aware, dysfunction-aware machine learning system that analyzes historical drilling data to find optimal parameters (WOB, RPM, Flow) for each formation type. It runs periodically (default: every hour) and provides statistically-validated recommendations that prevent both inefficient drilling and aggressive parameters that destroy bits.

**Key Capabilities:**
- Campaign-aware composite scoring (Production vs P&A goals)
- **Dysfunction filtering** - Rejects samples with stick-slip, pack-off, or founder conditions (V2.2)
- **Stability-aware optimization** - Penalizes operating points near instability thresholds (V2.2)
- **Grid-based binning** - Replaces top-10% averaging to avoid mixing disjoint modes (V2.2)
- **Safe operating ranges** - Returns parameter ranges, not just point estimates (V2.2)
- Formation boundary detection via d-exponent analysis
- Statistical significance testing (Pearson correlation)
- Multi-well/field-level knowledge storage and retrieval
- Integration with LLM advisory generation

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        ML ENGINE PIPELINE (V2.2)                            │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│   ┌──────────────┐     ┌───────────────────┐     ┌────────────────────┐    │
│   │   WITS Data  │────>│  Quality Filter   │────>│ Dysfunction Filter │    │
│   │  (1-2 hours) │     │  (WOB>5, RPM>40)  │     │  (V2.2 - NEW)      │    │
│   └──────────────┘     └───────────────────┘     └────────────────────┘    │
│                                │                          │                 │
│                                │ Rejected: sensor         │ Rejected:       │
│                                │ glitches, connections    │ stick-slip,     │
│                                ▼                          │ pack-off,       │
│                                                           │ founder         │
│                                                           ▼                 │
│                        ┌───────────────────┐     ┌────────────────────┐    │
│                        │ Formation Segmenter│<───│  Stable Samples    │    │
│                        │ (d-exp boundaries) │     │  Only              │    │
│                        └───────────────────┘     └────────────────────┘    │
│                                │                                            │
│                                │ Largest segment                            │
│                                ▼                                            │
│                        ┌───────────────────┐                               │
│                        │ Correlation Engine│                               │
│                        │ (Pearson r, p-val)│                               │
│                        └───────────────────┘                               │
│                                │                                            │
│                                │ V2.2: Relaxed requirements                 │
│                                │ (proceeds even if p > 0.05)               │
│                                ▼                                            │
│                        ┌───────────────────┐                               │
│                        │  Optimal Finder   │                               │
│                        │  (V2.2 - Binning) │                               │
│                        │  8×6 WOB×RPM grid │                               │
│                        │  + stability score│                               │
│                        └───────────────────┘                               │
│                                │                                            │
│                                │ Campaign-weighted composite:               │
│                                │ ROP + MSE_eff + Stability                  │
│                                ▼                                            │
│                        ┌───────────────────┐     ┌────────────────────┐    │
│                        │  MLInsightsReport │────>│  Sled Database     │    │
│                        │  + Operating Ranges     │  (MLInsightsStorage)│    │
│                        └───────────────────┘     └────────────────────┘    │
│                                │                          │                 │
│                                │                          │ Query by:       │
│                                ▼                          │ - well_id       │
│                        ┌───────────────────┐              │ - depth         │
│                        │   LLM Context     │<─────────────│ - campaign      │
│                        │  (build_ml_context)              │ - field         │
│                        └───────────────────┘              │                 │
│                                │                          │                 │
│                                ▼                          ▼                 │
│                        ┌───────────────────┐     ┌────────────────────┐    │
│                        │ Strategic Agent   │     │   REST API         │    │
│                        │ (LLM prompts)     │     │   /api/v1/ml/*     │    │
│                        └───────────────────┘     └────────────────────┘    │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Components

### 1. Scheduler (`scheduler.rs`)

**Purpose:** Manages the periodic execution of ML analysis.

**Configuration:**
```bash
# Environment variable (default: 3600 = 1 hour)
ML_INTERVAL_SECS=3600

# For faster testing (5 minutes)
ML_INTERVAL_SECS=300 ./target/release/sairen-os --wits-tcp localhost:5000
```

**Key Functions:**
- `get_interval_secs()` - Returns configured interval from environment
- `build_dataset()` - Constructs HourlyDataset from raw packets/metrics
- `run_analysis()` - Executes the full analysis pipeline

**Integration with Main Loop:**
The scheduler runs as an async task in `main.rs`, triggered every `ML_INTERVAL_SECS`:
```rust
// From main.rs (simplified)
loop {
    tokio::time::sleep(get_interval()).await;

    if packets.len() >= 100 {
        let dataset = MLScheduler::build_dataset(packets, metrics, well_id, ...);
        let report = MLScheduler::run_analysis(&dataset);
        storage.store_report(&report)?;
    }
}
```

---

### 2. Quality Filter (`quality_filter.rs`)

**Purpose:** Ensures only valid drilling data enters the analysis pipeline.

**Rejection Criteria:**

| Criterion | Threshold | Reason |
|-----------|-----------|--------|
| WOB | < 5 klbs | Connection/idle data |
| RPM | < 40 | Not rotating (stationary) |
| MSE | < 1,000 or > 500,000 psi | Sensor glitch |
| ROP | < 1 or > 500 ft/hr | Invalid value |
| Rig State | Not Drilling/Reaming | Non-drilling operation |

**Output:**
```rust
pub struct FilterResult<'a> {
    pub valid_packets: Vec<&'a WitsPacket>,
    pub valid_metrics: Vec<&'a DrillingMetrics>,
    pub rejected_count: usize,
    pub rejection_reason: Option<String>,  // e.g., "WOB < 5 klbs (connection/idle) (142 samples)"
}
```

**Why It Matters:**
- Connection data (low WOB/RPM) would skew optimal parameter calculations
- Sensor glitches could produce statistically significant but meaningless correlations
- Non-drilling states have different physics than rotary drilling

---

### 3. Dysfunction Filter (`dysfunction_filter.rs`) - V2.2 NEW

**Purpose:** Ensures optimal parameters are calculated only from stable, sustainable operating conditions. Rejects samples where drilling dysfunctions were occurring.

**Dysfunction Types Filtered:**

| Dysfunction | Detection Method | Why It's Filtered |
|-------------|------------------|-------------------|
| **Torque Instability** | Rolling CV > 12% | Precursor to stick-slip; unstable operation |
| **Pack-Off** | Torque delta > 10% AND SPP delta > 75 psi | Restriction forming; parameters are reactive |
| **Founder** | WOB trending up > 3%, ROP trend < 1% | Beyond optimal WOB; inefficient operation |
| **Low Efficiency** | MSE efficiency < 50% | Something fundamentally wrong |
| **Flagged Anomaly** | Tactical agent flagged as anomaly | Already detected as problematic |

**Rolling Window Analysis:**
- Uses 10-sample rolling window for trend detection
- Calculates coefficient of variation (CV) for torque stability
- Linear regression for WOB/ROP trend analysis

**Output:**
```rust
pub struct DysfunctionFilterResult<'a> {
    pub stable_packets: Vec<&'a WitsPacket>,
    pub stable_metrics: Vec<&'a DrillingMetrics>,
    pub rejected_count: usize,
    pub rejection_breakdown: DysfunctionBreakdown,
    pub stability_score: f64,  // 0-1, fraction of stable samples
}

pub struct DysfunctionBreakdown {
    pub torque_instability: usize,
    pub stick_slip: usize,
    pub pack_off: usize,
    pub founder: usize,
    pub low_efficiency: usize,
}
```

**Stability Score Calculation:**

The filter also provides a per-sample stability score (0.0-1.0) used by OptimalFinder:

```
score = 1.0
  - 0.30 × (torque_cv / threshold)      // Penalize high torque variability
  - 0.25 × (torque_delta / threshold)   // Penalize elevated torque
  - 0.20 × (efficiency_deficit / 20%)   // Penalize low efficiency
  - 0.15 × (spp_delta / threshold)      // Penalize elevated SPP
```

**Why It Matters:**
- "Optimal" must mean both **fast AND robust**
- A high-ROP operating point on the edge of stick-slip is not truly optimal
- Parameters selected during dysfunction would not be sustainable
- Ensures recommendations work in normal drilling conditions

---

### 4. Formation Segmenter (`formation_segmenter.rs`)

**Purpose:** Detects formation boundaries to prevent averaging parameters across different rock types.

**Detection Method:**
- Uses rolling 60-sample (1-minute) window
- Compares d-exponent between previous half-window and current half-window
- Boundary detected when d-exponent shifts > 15%

**Formation Type Estimation:**

| D-Exponent | Formation Type |
|------------|----------------|
| < 1.2 | Soft Shale |
| 1.2 - 1.6 | Medium Shale/Siltstone |
| 1.6 - 2.0 | Hard Sandstone |
| > 2.0 | Very Hard Formation |

**Output:**
```rust
pub struct FormationSegment {
    pub packet_range: (usize, usize),      // Index range in dataset
    pub formation_type: String,             // e.g., "Hard Sandstone"
    pub avg_d_exponent: f64,
    pub valid_sample_count: usize,
}
```

**Unstable Formation Detection:**
If multiple formation boundaries are detected and the largest segment has fewer than `MIN_ANALYSIS_SAMPLES` (360), the analysis fails with `UnstableFormation` status.

---

### 5. Correlation Engine (`correlations.rs`)

**Purpose:** Identifies statistically significant relationships between drilling parameters.

**Correlations Calculated:**

| X Parameter | Y Parameter | What It Tells Us |
|-------------|-------------|------------------|
| WOB | ROP | Weight-on-bit effect on penetration rate |
| WOB | MSE | Weight effect on drilling efficiency |
| RPM | ROP | Rotation speed effect on penetration |
| RPM | MSE | Rotation effect on efficiency |
| Flow | ROP | Hydraulics effect on hole cleaning/ROP |

**Statistical Significance Testing:**
- Uses Pearson correlation coefficient (r)
- Calculates p-value via Student's t-distribution (`statrs` crate)
- **Only correlations with p < 0.05 are returned**

**Formula:**
```
t = r × sqrt(n-2) / sqrt(1-r²)
p-value = 2 × (1 - CDF(|t|))  // Two-tailed
```

**Output:**
```rust
pub struct SignificantCorrelation {
    pub x_param: String,        // e.g., "WOB"
    pub y_param: String,        // e.g., "ROP"
    pub r_value: f64,           // Correlation coefficient (-1 to 1)
    pub r_squared: f64,         // Coefficient of determination
    pub p_value: f64,           // Statistical significance
    pub sample_count: usize,
}
```

**V2.2 Relaxed Requirements:**
- Pipeline no longer fails if no correlations meet p < 0.05
- Instead, proceeds with optimization and flags as "low confidence"
- Allows optimization even in variable drilling conditions

**Minimum Samples:** 30 (for meaningful correlation analysis)

---

### 6. Optimal Finder (`optimal_finder.rs`) - V2.2 REWRITTEN

**Purpose:** Identifies the best drilling parameters using campaign-aware composite scoring with **stability penalty** and **grid-based binning**.

**V2.2 Key Changes:**
1. **Binning replaces averaging** - No longer averages top 10%; uses 8×6 WOB×RPM grid
2. **Stability penalty** - Operating points near dysfunction thresholds are penalized
3. **Three-factor composite** - Now includes ROP, MSE efficiency, AND stability
4. **Range output** - Returns safe operating ranges, not just point estimates

**Campaign Weights (V2.2):**

| Campaign | ROP Weight | MSE Efficiency Weight | Stability Weight | Focus |
|----------|------------|----------------------|------------------|-------|
| Production | 50% | 30% | 20% | Drill fast, but stably |
| P&A | 25% | 45% | 30% | Operational stability first |

**Grid-Based Binning Algorithm:**

```
1. Determine WOB and RPM ranges from data
2. Create 8×6 grid (8 WOB bins × 6 RPM bins)
3. Assign each sample to a bin based on its WOB/RPM
4. For each bin with ≥10 samples:
   a. Calculate median ROP, MSE, flow for the bin
   b. Calculate stability score (from DysfunctionFilter)
   c. Normalize all metrics to 0-1
   d. Compute composite: rop_w × ROP + mse_w × MSE_eff + stab_w × stability
5. Select bin with highest composite score
6. Return median values and min/max ranges from winning bin
```

**Why Binning > Top 10% Averaging:**
- Averaging can mix disjoint operating modes (e.g., 15 klbs @ 80 RPM + 25 klbs @ 140 RPM → 20 klbs @ 110 RPM, which nobody actually used)
- Binning ensures recommended parameters were actually used together
- Ranges show the actual operating envelope, not a statistical phantom

**Output (V2.2):**
```rust
pub struct OptimalParams {
    // Point Estimates (median of winning bin)
    pub best_wob: f64,          // Optimal weight-on-bit (klbs)
    pub best_rpm: f64,          // Optimal rotation speed
    pub best_flow: f64,         // Optimal flow rate (gpm)

    // Safe Operating Ranges (V2.2)
    pub wob_min: f64,           // Minimum WOB in winning bin
    pub wob_max: f64,           // Maximum WOB in winning bin
    pub rpm_min: f64,           // Minimum RPM in winning bin
    pub rpm_max: f64,           // Maximum RPM in winning bin
    pub flow_min: f64,          // Minimum flow in winning bin
    pub flow_max: f64,          // Maximum flow in winning bin

    // Performance Metrics
    pub achieved_rop: f64,      // Median ROP at these params
    pub achieved_mse: f64,      // Median MSE at these params
    pub mse_efficiency: f64,    // Efficiency percentage
    pub composite_score: f64,   // 0-1 score (ROP + MSE + stability)
    pub confidence: ConfidenceLevel,

    // Stability Metrics (V2.2)
    pub stability_score: f64,   // 0-1, how far from dysfunction thresholds
    pub bin_sample_count: usize,// Samples in winning bin
    pub bins_evaluated: usize,  // Total valid bins evaluated
    pub dysfunction_filtered: bool, // Whether samples were filtered
}
```

**Composite Score Interpretation:**

| Score | Rating |
|-------|--------|
| > 0.75 | EXCELLENT - fast, efficient, AND stable |
| 0.60 - 0.75 | GOOD efficiency and stability |
| 0.45 - 0.60 | ACCEPTABLE - some compromise |
| < 0.45 | POOR - optimization needed |

**Stability Score Interpretation:**

| Score | Stability |
|-------|-----------|
| > 0.90 | Excellent - far from all thresholds |
| 0.75 - 0.90 | Good - comfortable margins |
| 0.60 - 0.75 | Moderate - some dysfunction filtered |
| < 0.60 | Marginal - significant dysfunction filtered |

**Confidence Levels:**

| Sample Count | Confidence |
|--------------|------------|
| < 360 | Insufficient |
| 360 - 720 | Low |
| 720 - 1800 | Medium |
| > 1800 | High |

---

### 7. Analyzer (`analyzer.rs`)

**Purpose:** Orchestrates the full ML pipeline and builds the final report.

**Pipeline Steps (V2.2):**
1. **Quality Filter** → Reject invalid samples (WOB < 5, RPM < 40, etc.)
2. **Dysfunction Filter** → Reject unstable samples (stick-slip, pack-off, founder)
3. **Formation Segmentation** → Detect boundaries, use largest stable segment
4. **Correlation Analysis** → Calculate relationships (relaxed - doesn't fail)
5. **Optimal Finding** → Grid-based binning with stability penalty
6. **Report Building** → Generate success or failure report with ranges

**Failure Modes:**

| Failure | Cause | Example |
|---------|-------|---------|
| `AllDataRejected` | No samples passed quality filter | All data during connections |
| `AllDataRejected` | Dysfunction filtered too many samples | Persistent stick-slip |
| `InsufficientData` | < 360 stable samples | Short drilling interval or high dysfunction |
| `UnstableFormation` | Too many formation changes | Drilling through interbedded layers |

**Note:** `NoSignificantCorrelation` is no longer a failure mode in V2.2. The pipeline proceeds with low confidence instead.

**Success Report Includes (V2.2):**
- Optimal parameters (WOB, RPM, Flow) with **safe operating ranges**
- Significant correlations with p-values (if any)
- **Stability score** and dysfunction filtering statistics
- Formation type and depth range
- Natural language summary for LLM context (includes stability info)

---

### 8. Storage (`storage.rs`)

**Purpose:** Persistent multi-well storage using Sled embedded database.

**Key Format:**
```
{field_name}/{well_id}/{campaign}/{timestamp}
```

Example: `NORTH_SEA/WELL-042/Production/1704067200`

**Query Capabilities:**

| Method | Purpose |
|--------|---------|
| `get_latest(well_id, campaign)` | Most recent report for a well |
| `get_well_history(well_id, campaign, limit)` | Historical reports for a well |
| `get_field_history(field_name, campaign, limit)` | Cross-well field analysis |
| `find_by_depth(well_id, depth, tolerance, limit)` | Reports near a specific depth |

**LLM Context Builder:**
```rust
pub fn build_ml_context(reports: &[MLInsightsReport], current_depth: f64) -> String
```

Generates formatted context for LLM prompts:
```
### HISTORICAL ML INSIGHTS
[1] WELL-042 at 4950-5050ft in Hard Sandstone (bit: 24hrs, HIGH confidence):
    Optimal: WOB=22.5 klbs, RPM=115, Flow=520 gpm
    Achieved: ROP=65.2 ft/hr, MSE_eff=82%
    Correlation: WOB vs ROP r=0.85 (p=0.0001)
```

---

## Data Flow Integration

### Input: WITS Packets
The ML engine receives accumulated WITS packets from the main processing loop:
- Packets are collected in a circular buffer (up to 2 hours at 1 Hz = 7,200 packets)
- Every `ML_INTERVAL_SECS`, the buffer is snapshot and passed to the analyzer
- Minimum 100 packets required to trigger analysis

### Output: Three Consumers

1. **Sled Database** - Persistent storage for historical queries
2. **REST API** - Real-time access via `/api/v1/ml/latest` and `/api/v1/ml/history`
3. **LLM Context** - Injected into strategic agent prompts for informed recommendations

---

## API Endpoints

### GET /api/v1/ml/latest

Returns the most recent ML insights report.

**Response (Success - V2.2):**
```json
{
  "timestamp": 1704067200,
  "campaign": "Production",
  "depth_range": [4950.0, 5050.0],
  "well_id": "WELL-042",
  "field_name": "NORTH_SEA",
  "formation_type": "Hard Sandstone",
  "result": {
    "Success": {
      "optimal_params": {
        "best_wob": 22.5,
        "best_rpm": 115.0,
        "best_flow": 520.0,
        "wob_min": 20.0,
        "wob_max": 25.0,
        "rpm_min": 100.0,
        "rpm_max": 130.0,
        "flow_min": 480.0,
        "flow_max": 560.0,
        "achieved_rop": 65.2,
        "achieved_mse": 18500.0,
        "mse_efficiency": 82.0,
        "composite_score": 0.78,
        "confidence": "High",
        "stability_score": 0.88,
        "bin_sample_count": 145,
        "bins_evaluated": 32,
        "dysfunction_filtered": true
      },
      "correlations": [
        {
          "x_param": "WOB",
          "y_param": "ROP",
          "r_value": 0.85,
          "r_squared": 0.72,
          "p_value": 0.0001,
          "sample_count": 2400
        }
      ],
      "summary_text": "ML Analysis for WELL-042 in Hard Sandstone (HIGH confidence). Optimal: WOB=22.5 klbs [20.0-25.0], RPM=115 [100-130], Flow=520 gpm [480-560]. Achieved ROP=65.2 ft/hr with MSE efficiency 82% (composite score: 0.78 - EXCELLENT, good stability)...",
      "confidence": "High",
      "sample_count": 2400
    }
  }
}
```

**Response (Failure):**
```json
{
  "timestamp": 1704067200,
  "result": {
    "Failure": {
      "InsufficientData": {
        "valid_samples": 180,
        "required": 360
      }
    }
  }
}
```

### GET /api/v1/ml/history

Returns historical ML reports.

**Query Parameters:**
- `hours` (optional) - How far back to look (default: 24)
- `limit` (optional) - Maximum reports to return (default: 10)

---

## Configuration Reference

| Environment Variable | Default | Description |
|---------------------|---------|-------------|
| `ML_INTERVAL_SECS` | 3600 | Analysis interval in seconds |
| `WELL_ID` | WELL-001 | Well identifier for storage |
| `FIELD_NAME` | DEFAULT | Field/asset name for grouping |

| Threshold | Value | Source |
|-----------|-------|--------|
| `MIN_ANALYSIS_SAMPLES` | 360 | ~6 minutes of valid drilling data |
| `SIGNIFICANCE_THRESHOLD` | 0.05 | p-value cutoff for correlations (relaxed in V2.2) |
| `FORMATION_BOUNDARY_SHIFT` | 0.15 | 15% d-exponent change = boundary |
| `MIN_WOB` | 5.0 klbs | Quality filter threshold |
| `MIN_RPM` | 40 | Quality filter threshold |
| `MIN_PLAUSIBLE_MSE` | 1,000 psi | Sensor glitch filter |
| `MAX_PLAUSIBLE_MSE` | 500,000 psi | Sensor glitch filter |

**Dysfunction Filter Thresholds (V2.2):**

| Threshold | Value | Purpose |
|-----------|-------|---------|
| `TORQUE_CV_UNSTABLE` | 0.12 (12%) | Torque coefficient of variation for instability |
| `TORQUE_DELTA_PACKOFF` | 0.10 (10%) | Torque delta for pack-off detection |
| `SPP_DELTA_PACKOFF` | 75 psi | SPP delta for pack-off detection |
| `WOB_INCREASE_FOUNDER` | 0.03 (3%) | WOB trend for founder detection |
| `ROP_RESPONSE_FOUNDER` | 0.01 (1%) | ROP response threshold for founder |
| `MSE_EFFICIENCY_UNSTABLE` | 50% | Minimum efficiency threshold |
| `ROLLING_WINDOW_SIZE` | 10 samples | Window for rolling calculations |

**Optimal Finder Grid Configuration (V2.2):**

| Parameter | Value | Purpose |
|-----------|-------|---------|
| `WOB_BINS` | 8 | Number of WOB bins in grid |
| `RPM_BINS` | 6 | Number of RPM bins in grid |
| `MIN_BIN_SAMPLES` | 10 | Minimum samples for valid bin |

---

## Example Analysis Flow (V2.2)

```
1. [00:00:00] ML Scheduler starts, collects packets for 1 hour

2. [01:00:00] Analysis triggered with 3,200 packets
   ├── Quality Filter: 2,850 valid (350 rejected: connections)
   ├── Dysfunction Filter: 2,650 stable (200 rejected)
   │   ├── Torque instability: 85 samples
   │   ├── Pack-off signatures: 45 samples
   │   ├── Founder conditions: 40 samples
   │   └── Low efficiency: 30 samples
   │   └── Stability score: 0.93 (93% stable)
   ├── Formation Segmenter: 1 segment detected (stable Sandstone)
   ├── Correlation Engine: 3 significant correlations found
   │   ├── WOB vs ROP: r=0.82, p=0.0001
   │   ├── RPM vs ROP: r=0.65, p=0.003
   │   └── WOB vs MSE: r=-0.45, p=0.012
   └── Optimal Finder: 8×6 grid binning
       ├── 32 valid bins (≥10 samples each)
       ├── Best bin: WOB[21-24], RPM[110-125]
       ├── Bin stability: 0.88
       └── Result: WOB=23.1 [21.0-24.0], RPM=118 [110-125], Flow=515 [490-540]

3. [01:00:00] Report stored in Sled DB
   Key: "NORTH_SEA/WELL-042/Production/1704070800"

4. [01:00:01] LLM generates advisory with ML context:
   "Based on ML analysis (good stability), optimal parameters for current
    Hard Sandstone formation are WOB=23 klbs [21-24], RPM=118 [110-125].
    Current WOB=18 is below optimal range. Consider increasing WOB to 21+
    for improved ROP while staying within stable operating envelope."
```

---

## Troubleshooting

### "InsufficientData" failures
- **Cause:** Less than 360 valid drilling samples
- **Fix:** Wait for more drilling time, or reduce `MIN_ANALYSIS_SAMPLES` for testing

### "AllDataRejected" failures
- **Cause:** All packets failed quality filter (e.g., during connections/tripping)
- **Fix:** Normal during non-drilling operations; will resolve when drilling resumes

### "NoSignificantCorrelation" failures (V2.1 only)
- **Note:** In V2.2, this is no longer a failure. The pipeline proceeds with low confidence.
- **Cause:** Data is too noisy or parameters don't correlate
- **Possible reasons:**
  - Sensor issues
  - Highly variable formation
  - Parameters at constant values (no variation to correlate)

### High dysfunction rejection rate
- **Cause:** Many samples rejected by DysfunctionFilter
- **Check:**
  - Is stick-slip active? (high torque CV)
  - Is there a pack-off developing? (torque + SPP rising)
  - Are you past the founder point? (WOB up, ROP not responding)
- **Interpretation:** A high dysfunction rate is actually useful information - it indicates the drilling operation was unstable during the analysis period
- **The stability_score in the report tells you what fraction was stable

### "UnstableFormation" failures
- **Cause:** Multiple formation boundaries detected, largest segment too small
- **Interpretation:** Drilling through interbedded layers; optimal parameters would be unreliable

### No ML reports appearing
1. Check `ML_INTERVAL_SECS` - default is 1 hour
2. Verify enough packets are being collected (need 100+ to trigger)
3. Check logs for `[MLScheduler]` messages
4. Verify drilling state (analysis only runs during Drilling/Reaming)
