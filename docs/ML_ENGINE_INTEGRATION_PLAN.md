# ML Engine Integration Plan: Hourly Drilling Efficiency Analysis

## Executive Summary

**Goal**: Add an ML engine that runs hourly to analyze drilling efficiency patterns, identify optimal parameter combinations, and store findings for RAG-enhanced LLM advisories.

**Verdict**: ✅ **This will work well** - The approach leverages existing infrastructure and provides continuous learning that improves advisory quality over time.

---

## Why This Works

### Current System Gaps
1. **Real-time focus**: Current system reacts to anomalies but doesn't learn optimal patterns
2. **No historical correlation**: LLM advisories are based on current state, not learned experience
3. **Generic recommendations**: "Optimize WOB/RPM" without formation-specific insights

### What ML Engine Adds
1. **Pattern Discovery**: Finds which parameter combinations produced best ROP/MSE for each formation type
2. **Formation-Specific Learning**: Different formations require different optimal parameters
3. **Continuous Improvement**: Each hour of drilling adds to the knowledge base
4. **Context-Rich Advisories**: LLM can say "In similar formations at this depth, WOB=28klbs and RPM=110 achieved 65 ft/hr"

---

## Architecture Overview

```
                         SAIREN-OS with ML Engine
    ═══════════════════════════════════════════════════════════════════════

    Real-Time Pipeline (existing)          ML Engine (new, hourly)
    ─────────────────────────────          ────────────────────────

    ┌─────────────┐                        ┌──────────────────────┐
    │ WITS Packets│                        │   Hourly Scheduler   │
    │   (1 Hz)    │                        │   (tokio interval)   │
    └──────┬──────┘                        └──────────┬───────────┘
           │                                          │
           ▼                                          ▼
    ┌─────────────┐     writes to          ┌──────────────────────┐
    │  Pipeline   │ ───────────────────►   │   Drilling History   │
    │ Coordinator │     history_buffer     │   Database (Sled)    │
    └──────┬──────┘                        └──────────┬───────────┘
           │                                          │
           ▼                                          │ reads 1 hour
    ┌─────────────┐                                   │ of data
    │  Strategic  │                                   ▼
    │     LLM     │◄──── RAG query ────── ┌──────────────────────┐
    └─────────────┘                       │     ML Analyzer      │
           │                              │  ┌────────────────┐  │
           │                              │  │ Feature Engine │  │
           │                              │  │ (correlations) │  │
           │                              │  └───────┬────────┘  │
           │                              │          ▼           │
           │                              │  ┌────────────────┐  │
           │                              │  │ Pattern Finder │  │
           │                              │  │ (clustering)   │  │
           │                              │  └───────┬────────┘  │
           │                              │          ▼           │
           │                              │  ┌────────────────┐  │
           │                              │  │ Report Builder │  │
           │                              │  └───────┬────────┘  │
           │                              └──────────┼───────────┘
           │                                         │
           │                                         ▼
           │                              ┌──────────────────────┐
           └────── retrieves ────────────►│   ML Insights DB     │
                   context                │   (Vector Store)     │
                                          │   - Embeddings       │
                                          │   - Reports          │
                                          └──────────────────────┘
    ═══════════════════════════════════════════════════════════════════════
```

---

## Data Flow

### Input: 1 Hour of Drilling Data
```rust
struct HourlyDataset {
    packets: Vec<WitsPacket>,      // ~3600 packets at 1Hz
    metrics: Vec<DrillingMetrics>, // Calculated metrics for each
    time_range: (u64, u64),        // Start/end timestamps
    avg_depth: f64,                // Average depth during period
    formation_estimate: String,    // Estimated formation type
}
```

### Analysis Variables (Drilling Efficiency Focus)
| Category | Variables | Role |
|----------|-----------|------|
| **Inputs** | WOB, RPM, flow_in, mud_weight, bit_diameter | Controllable parameters |
| **Outputs** | ROP, MSE, MSE_efficiency | Efficiency metrics |
| **Context** | depth, d_exponent, formation_hardness | Formation characteristics |
| **Constraints** | torque, SPP, ECD_margin | Safety limits |

### Output: ML Insights Report
```rust
struct MLInsightsReport {
    timestamp: u64,
    depth_range: (f64, f64),
    formation_type: String,

    // Key findings
    optimal_parameters: OptimalParams,
    efficiency_summary: EfficiencySummary,
    correlations: Vec<Correlation>,
    recommendations: Vec<String>,

    // For RAG embedding
    summary_text: String,        // Natural language summary
    embedding: Vec<f32>,         // Vector embedding for similarity search
}

struct OptimalParams {
    best_wob: f64,
    best_rpm: f64,
    best_flow: f64,
    achieved_rop: f64,
    achieved_mse: f64,
    confidence: f64,
}
```

---

## ML Approach: Pragmatic Statistics + Simple ML

### Why Not Deep Learning?
- Hourly batches are small (~3600 samples)
- Drilling physics are well-understood
- Interpretability matters (drillers need to trust recommendations)
- Computational efficiency (runs alongside real-time system)

### Recommended Approach: Hybrid Statistical + Decision Tree

#### Step 1: Data Preprocessing
```rust
fn preprocess(packets: &[WitsPacket]) -> CleanedDataset {
    // 1. Filter to drilling state only
    // 2. Remove outliers (>3 sigma)
    // 3. Smooth with rolling average (10-sample window)
    // 4. Normalize to comparable scales
}
```

#### Step 2: Correlation Analysis
```rust
fn analyze_correlations(data: &CleanedDataset) -> Vec<Correlation> {
    // Pearson correlation between each input and ROP/MSE
    // Example output:
    // - WOB vs ROP: r=0.72 (strong positive)
    // - RPM vs ROP: r=0.45 (moderate positive)
    // - WOB vs MSE: r=-0.38 (moderate negative - good!)
}
```

#### Step 3: Optimal Parameter Identification
```rust
fn find_optimal_params(data: &CleanedDataset) -> OptimalParams {
    // Method 1: Percentile analysis
    // - Find top 10% ROP periods
    // - Extract average parameters during those periods

    // Method 2: Decision tree
    // - Train shallow tree (depth=3) predicting ROP
    // - Extract rules: "IF WOB > 26 AND RPM > 115 THEN ROP_high"

    // Method 3: Grid search (if compute allows)
    // - Bin parameters into ranges
    // - Find bin combination with highest avg ROP
}
```

#### Step 4: Report Generation
```rust
fn generate_report(
    optimal: &OptimalParams,
    correlations: &[Correlation],
    data: &CleanedDataset,
) -> MLInsightsReport {
    // Natural language summary for RAG
    let summary = format!(
        "Drilling analysis for {} to {} ft in {} formation. \
         Optimal parameters: WOB={:.1} klbs, RPM={:.0}, Flow={:.0} gpm. \
         Achieved ROP={:.1} ft/hr with MSE efficiency {:.0}%. \
         Key finding: {} showed strongest correlation with ROP (r={:.2}).",
        depth_start, depth_end, formation,
        optimal.best_wob, optimal.best_rpm, optimal.best_flow,
        optimal.achieved_rop, optimal.mse_efficiency,
        strongest_correlate.name, strongest_correlate.r
    );

    // Generate embedding for vector similarity search
    let embedding = embed_text(&summary);

    MLInsightsReport { summary_text: summary, embedding, ... }
}
```

---

## RAG Integration

### How It Works
1. **Storage**: ML reports stored with vector embeddings in Sled DB
2. **Query**: When LLM generates advisory, query similar past conditions
3. **Context**: Retrieved reports added to LLM prompt as context

### Vector Similarity Search
```rust
// In context/vector_db.rs (extend existing)
pub fn search_ml_insights(
    current_depth: f64,
    formation_type: &str,
    limit: usize,
) -> Vec<MLInsightsReport> {
    // Query by:
    // 1. Depth similarity (±500 ft)
    // 2. Formation type match
    // 3. Recency (prefer recent insights)
}
```

### Enhanced LLM Prompt
```
You are the Strategic AI for rig operational intelligence.

### CURRENT CONDITIONS
State: Drilling | Depth: 10,500 ft | ROP: 45 ft/hr
WOB: 24 klbs | RPM: 110 | MSE: 42,000 psi (efficiency: 72%)

### HISTORICAL ML INSIGHTS (from similar conditions)
[1] At 10,200-10,400 ft in similar formation:
    Optimal: WOB=28 klbs, RPM=115 achieved ROP=58 ft/hr
    Finding: Increasing WOB showed 0.72 correlation with ROP

[2] At 10,600-10,800 ft yesterday:
    Optimal: WOB=26 klbs, RPM=120 achieved ROP=52 ft/hr
    Finding: RPM increase from 100→120 improved ROP 15%

### YOUR TASK
Provide specific recommendations based on current conditions AND historical learnings.
```

---

## Implementation Plan

### Phase 1: Data Collection Enhancement (Week 1)
**Files to modify:**
- `src/storage/history.rs` - Add hourly data export
- `src/types.rs` - Add `HourlyDataset` struct

**Tasks:**
1. Add method to export last N hours of packets from history
2. Create hourly dataset struct with all required fields
3. Add formation estimation based on d-exponent trends

### Phase 2: ML Analyzer Module (Week 2)
**New files:**
- `src/ml_engine/mod.rs` - Module exports
- `src/ml_engine/analyzer.rs` - Core analysis logic
- `src/ml_engine/correlations.rs` - Statistical functions
- `src/ml_engine/patterns.rs` - Optimal parameter finding

**Dependencies to add (Cargo.toml):**
```toml
[dependencies]
ndarray = "0.15"           # Numerical arrays
linregress = "0.5"         # Linear regression/correlation
```

**Tasks:**
1. Implement correlation analysis
2. Implement percentile-based optimal parameter finding
3. Implement report generation with natural language summary

### Phase 3: Scheduler Integration (Week 3)
**Files to modify:**
- `src/main.rs` - Add hourly task to JoinSet
- `src/pipeline/coordinator.rs` - Expose data access for ML

**Tasks:**
1. Add tokio interval task (1 hour)
2. Integrate with existing supervisor pattern
3. Add graceful shutdown handling

### Phase 4: RAG Integration (Week 4)
**Files to modify:**
- `src/context/vector_db.rs` - Add ML insights storage/retrieval
- `src/llm/strategic_llm.rs` - Query and inject ML context

**Tasks:**
1. Add vector storage for ML reports
2. Implement similarity search
3. Modify LLM prompt to include historical insights

### Phase 5: Testing & Tuning (Week 5)
- Run with 8-hour simulation
- Verify reports are generated and stored
- Validate RAG retrieval improves advisory quality
- Tune correlation thresholds and report format

---

## File Structure

```
src/
  ml_engine/
    mod.rs              # Module exports
    analyzer.rs         # HourlyAnalyzer struct, main analysis loop
    correlations.rs     # Pearson correlation, R², feature importance
    patterns.rs         # Optimal parameter finder, clustering
    report.rs           # MLInsightsReport generation
    scheduler.rs        # Hourly scheduling with tokio

  storage/
    ml_insights.rs      # Sled storage for ML reports (new)

  context/
    vector_db.rs        # Extended with ML insights queries
```

---

## API Additions

### New Endpoints
| Endpoint | Description |
|----------|-------------|
| `GET /api/v1/ml/latest` | Most recent ML insights report |
| `GET /api/v1/ml/history?hours=24` | Last 24 hours of reports |
| `GET /api/v1/ml/optimal?depth=10500` | Optimal params for depth |

### Dashboard Widget
- Add "ML Insights" panel showing:
  - Current optimal parameters
  - Top correlations
  - Trend chart: efficiency over time

---

## Risks & Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Insufficient data (new well) | Medium | Medium | Fall back to physics-only mode until 4+ hours accumulated |
| Formation changes within hour | Medium | Low | Segment analysis by d-exponent clusters |
| Overfitting to noise | Low | Medium | Use robust statistics (median, IQR) not just mean |
| LLM context overflow | Low | Low | Limit to 3 most relevant insights per query |
| Compute overhead | Low | Low | Analysis runs async, doesn't block real-time pipeline |

---

## Success Metrics

| Metric | Target | How to Measure |
|--------|--------|----------------|
| Report generation | 100% hourly | Log monitoring |
| RAG retrieval relevance | >80% useful | Manual review of 20 advisories |
| Advisory specificity | 50% cite ML insights | Count advisories with specific param recommendations |
| ROP improvement (long-term) | 5-10% | Compare wells with/without ML engine |

---

## Alternative Approaches Considered

### 1. Online Learning (Rejected)
- Update model with every packet
- **Why rejected**: Adds latency to real-time pipeline, prone to drift

### 2. Deep Learning (Rejected)
- LSTM/Transformer for sequence modeling
- **Why rejected**: Overkill for hourly batch, hard to interpret, needs more data

### 3. External ML Service (Rejected)
- Send data to Python/TensorFlow service
- **Why rejected**: Adds complexity, network dependency, harder to deploy

### 4. Recommended: Embedded Statistical + Light ML ✅
- Pure Rust, runs in-process
- Interpretable results
- Fast enough for hourly batch
- Scales to larger datasets later

---

## Next Steps

1. **Review this plan** - Provide feedback on approach
2. **Approve scope** - Confirm Phase 1-5 timeline acceptable
3. **Begin Phase 1** - Data collection enhancement
4. **Iterate** - Adjust based on initial results

---

## Questions for Review

1. **Report frequency**: Is hourly the right interval? (Could be 30min or 2hr)
2. **Formation estimation**: Should we use d-exponent or external formation tops data?
3. **RAG limit**: How many historical insights should inform each advisory? (Suggest: 3)
4. **Dashboard priority**: Is the ML insights widget needed in v1?

---

*Plan created: 2026-01-26*
*Author: Claude (SAIREN-OS Development)*
