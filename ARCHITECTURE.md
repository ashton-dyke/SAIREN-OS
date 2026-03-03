# SAIREN-OS Architecture

Developer and contributor reference for SAIREN-OS internals. For operator documentation (installation, configuration, running, troubleshooting), see [README.md](README.md).

---

## Table of Contents

1. [System Architecture](#system-architecture)
2. [CfC Neural Network](#cfc-neural-network)
3. [ML Engine V2.2](#ml-engine-v22)
4. [Causal Inference](#causal-inference)
5. [Regime-Aware Orchestrator Weighting](#regime-aware-orchestrator-weighting)
6. [Knowledge Base](#knowledge-base)
7. [P2P Mesh Gossip](#p2p-mesh-gossip)
8. [Trait Architecture](#trait-architecture)
9. [Advisory Composition](#advisory-composition)
10. [Background Services](#background-services)
11. [Operation Classification](#operation-classification)
12. [Performance Benchmarks](#performance-benchmarks)
13. [Project Structure](#project-structure)
14. [Developer Glossary](#developer-glossary)

---

## System Architecture

SAIREN-OS uses a two-stage multi-agent architecture where a fast **Tactical Agent** handles real-time anomaly detection via deterministic pattern-matched routing, and a deeper **Strategic Agent** performs comprehensive drilling physics analysis only when anomalies are detected. Structured `TicketContext` (all threshold breaches, pattern name, rig state, operation, campaign) travels with each ticket and is used by the template advisory system.

The **Orchestrator** uses trait-based `Specialist` implementations for domain-specific evaluation, returning a `VotingResult`. The **AdvisoryComposer** then assembles the final `StrategicAdvisory` with CRITICAL cooldown (30s) to prevent alert spam. Knowledge lookup uses the `KnowledgeStore` trait, allowing swappable backends (static DB, RAMRecall, or NoOp for pilot mode).

**Fleet topology**: A decentralized **P2P mesh** where every Pi node gossips operational reports (anomaly events, outcomes, formation context) directly to all peers. No central server — each node runs identical software with identical capabilities. CfC models are rig-specific and not shared.

```
                           P2P Mesh Topology
    ===============================================================================

    +----------+          +----------+          +----------+
    |  RIG-001 |◄--gossip--►|  RIG-002 |◄--gossip--►|  RIG-003 |
    |   (Pi)   |          |   (Pi)   |          |   (Pi)   |
    +-----┬----+          +----------+          +----┬-----+
          │                                          │
          └──────────────gossip──────────────────────┘

    Every node: CfC inference + health scoring + template advisories + gossip
    No central server, no special roles, no single point of failure
    ===============================================================================
```

```
                              SAIREN-OS Multi-Agent Pipeline
    ===============================================================================

    PHASE 1              PHASE 2-3                PHASE 4              PHASE 5
    --------             ---------                --------             -------

    +------------+      +------------------+     +------------+      +------------------+
    |    WITS    |      |  Tactical Agent  |     |  History   |      | Strategic Agent  |
    |  Ingestion |----->|  Fast Physics    |---->|  Buffer    |--+-->| verify_ticket()  |
    | (TCP/JSON) |      |  + Ticket Gate   |     |  (60 pkt)  |  |   | Drilling Physics |
    +------------+      +------------------+     +------------+  |   +------------------+
                               |                                |            |
                               | No Ticket?                     |     +------+------+
                               | Continue Loop                  |     |      |      |
                               v                                |  REJECT  UNCERTAIN  CONFIRM
                         [Next Packet]                          |     |      |      |
                                                                |     v      v      v
                                                                | [Discard] [Log] [Phase 6-9]
                                                                |                      |
                                                                |            +---------+
                                                                |            v
                                                                |   +------------------+
                                                                |   | KnowledgeStore   |
                                                                |   | (precedent query)|
                                                                |   +------------------+
                                                                |            |
                                                                |            v
                                                                |   +------------------+
                                                                |   |   Orchestrator   |
                                                                |   | 4 Specialists    |
                                                                |   | (trait-based)    |
                                                                |   +------------------+
                                                                |            |
                                                                |            v
                                                                |   +------------------+
                                                                |   | AdvisoryComposer |
                                                                |   | (CRITICAL cooldown)|
                                                                |   +------------------+
                                                                |            |
                                                                |            v
                                                                |   +------------------+
                                                                +-->|  Dashboard API   |
                                                                    |  :8080           |
                                                                    +------------------+
    ===============================================================================
```

### 9-Phase Processing Pipeline

| Phase | Component | Function |
|-------|-----------|----------|
| 1 | WITS Ingestion | Receive 40+ channel WITS Level 0 packets, classify rig state |
| 2 | Tactical Physics | Calculate MSE, d-exponent, flow balance, pit rate (<15ms) |
| 2.8.1 | CfC Network | Self-supervised neural network: predict, compare, train, score, modulate severity; stamp `regime_id` via k-means clusterer |
| 2.8.2 | Depth-Ahead CfC | Secondary 64-neuron CfC (seed=1042) predicting formation transition response; resets on formation boundaries; annotates `LookAheadAdvisory` with confidence |
| 3 | Decision Gate | 6-rule ticket gate: rig state, anomaly, per-category cooldown, ACI corroboration, CfC corroboration, founder debounce |
| 4 | History Buffer | Store last 60 packets for trend analysis |
| 4.5 | Causal Inference | Cross-correlate WOB/RPM/Torque/SPP/ROP against MSE at lags 1-20s; attach `CausalLead` results to ticket |
| 5 | Advanced Physics | Strategic verification of tickets (CfC tiebreaker on Uncertain) |
| 6 | Context Lookup | Query KnowledgeStore (StaticKnowledgeBase, RAMRecall, or NoOp) |
| 7 | Template Advisory | Campaign-aware template recommendations with causal leads appended |
| 8 | Orchestrator Voting | 4 trait-based specialists vote with regime-adjusted weights -> VotingResult |
| 9 | Advisory Composition | AdvisoryComposer assembles StrategicAdvisory (CRITICAL cooldown) |

### Phase 3 Ticket Gate (6 Rules)

The tactical agent's `decide_advisory_ticket()` applies 6 sequential rules before creating a ticket. Failing any rule returns `None` without consuming cooldown (except Rule 3 which is the cooldown itself). WellControl tickets bypass Rules 4, 5, and 6 — safety is never gated.

| Rule | Gate | Purpose |
|------|------|---------|
| 1 | **Rig state** | Only Drilling or Reaming |
| 2 | **Anomaly detected** | `metrics.is_anomaly` must be true |
| 3 | **Per-category cooldown** | Packet count AND depth change AND time elapsed must all be met to suppress (per-category, not global) |
| 4 | **ACI corroboration** | Trigger metric must be outside its ACI conformal interval; category-specific metric mapping |
| 5 | **CfC corroboration** | Neural network anomaly score must be >= 0.3; suppresses all non-safety tickets during CfC warm-up |
| 6 | **Founder debounce** | Mechanical/Founder tickets require N consecutive founder-positive packets (default 3); filters transient WOB spikes |

Delta calculations use `prev_active_packet` (last packet from Drilling/Reaming/Circulating state) rather than the raw previous packet, preventing false positives from Idle-to-Drilling state transitions.

---

## CfC Neural Network

A dual 64-neuron Closed-form Continuous-time (CfC) neural network with Neural Circuit Policy (NCP) sparse wiring that actively participates in the decision pipeline. Two independent networks — **fast** and **slow** — run in parallel via `rayon::join()` and are combined via `max(fast, slow)`. The networks are **self-supervised** — they predict next-timestep sensor values and treat prediction error as an anomaly signal. No labeled training data needed.

### Properties

| Property | Fast Network | Slow Network |
|----------|-------------|-------------|
| **Neurons** | 64 CfC with NCP wiring | 64 CfC with NCP wiring |
| **Parameters** | ~1,308 trainable | ~1,326 trainable |
| **NCP Connections** | ~316 (~30% sparse) | ~322 (~30% sparse) |
| **Learning Rate** | 0.001 → 0.0001 | 0.0001 → 0.00001 |
| **BPTT Depth** | 4 (fast adaptation) | 8 (trend tracking) |
| **Role** | Catches acute events (kicks, sudden losses) | Catches gradual trends (pack-offs, washouts) |
| **Input Features** | 16 (WOB, ROP, RPM, torque, MSE, SPP, d-exp, hookload, ECD, flow balance, pit rate, DXC, pump SPM, mud weight, gas, pit volume) |
| **Outputs** | 16 next-step predictions, anomaly score (0-1), health score (0-1), per-feature surprise decomposition |
| **Combined Scoring** | `max(fast_score, slow_score)` — either network can trigger detection |
| **Parallelism** | Both networks run concurrently via `rayon::join()` (zero shared mutable state) |
| **Calibration** | 500 packets before producing anomaly scores |

### How It Works

1. Each packet: normalize features (Welford's online mean/variance)
2. Train: compare previous predictions against current reality (feature-weighted MSE loss, primary features 2x)
3. Forward: predict next timestep through CfC gates (tau/f/g time-gated update)
4. Score: adaptive z-score of prediction RMSE -> sigmoid -> anomaly score (0-1)

### Active Integration

CfC participates in three pipeline stages:

| Stage | Role | Mechanism |
|-------|------|-----------|
| **Severity modulation** (Phase 3) | Adjusts ticket severity like ACI | Score < 0.3 -> downgrade, >= 0.7 -> escalate |
| **Template context** (Phase 7) | CfC data in advisory text | Anomaly score + top 5 surprised features |
| **Tiebreaker** (Phase 5) | Resolves Uncertain verifications | Score >= 0.7 -> Confirmed, < 0.2 -> Rejected |

Safety rule: WellControl tickets are never downgraded below High severity.

### Per-Feature Surprise Decomposition

When the CfC detects anomalies, it reports which specific features deviated most from prediction (e.g., "SPP up 2.36 sigma, torque up 0.48 sigma"), giving operators interpretable context for the anomaly signal.

### Validation Results

| Well | Packets | Fast Avg Loss | Slow Avg Loss | Tickets | Confirmation Rate | Notes |
|------|---------|--------------|--------------|---------|-------------------|-------|
| **F-5** | 181,617 | 0.254 | 0.658 | 271 | 97% confirmed, 1% rejected | Extensive fluid loss detection across full depth |
| **F-9A** | 133,557 | 0.519 | 1.130 | 8 | 100% confirmed | Hydraulics issues in narrow depth band |
| **F-12** | 2,423,467 | 1.072 | 1.872 | 70 | 81% confirmed | Long-duration well, CfC calibrated online |

### Implementation Details

| Module | Description |
|--------|-------------|
| `src/cfc/mod.rs` | Public API, CfcDrillingResult, feature extraction |
| `src/cfc/normalizer.rs` | Welford's online mean/variance normalization (16 features) |
| `src/cfc/wiring.rs` | NCP sparse connectivity generation (deterministic from seed) |
| `src/cfc/cell.rs` | CfC cell forward pass, gate equations, ForwardCache |
| `src/cfc/training.rs` | Manual BPTT (depth=4), Adam optimizer, gradient norm clipping |
| `src/cfc/network.rs` | CfcNetwork: process(), anomaly scoring, calibration |
| `src/cfc/regime_clusterer.rs` | K-means clustering of 8 motor outputs -> regime_id (0-3) |
| `src/cfc/formation_detector.rs` | Motor-output pattern analysis for formation boundary detection |
| `src/cfc/checkpoint.rs` | `DualCfcCheckpoint`, `CfcNetworkCheckpoint`, atomic disk save/load |
| `src/cfc/depth_ahead.rs` | Depth-ahead CfC network (64 neurons, seed=1042, BPTT=6), 8-feature extraction, formation-boundary reset |

**Adam optimizer**: Decaying base LR, beta1=0.9, beta2=0.999 — 64% lower loss vs SGD baseline. Fast network: 0.001 → 0.0001; Slow network: 0.0001 → 0.00001.

**Truncated BPTT**: Fast network depth=4 (responsive), slow network depth=8 (trend-sensitive). Backprop through cached timesteps with 0.7^k gradient decay per step.

**Gradient norm clipping** (max norm=5.0): Preserves gradient direction while preventing explosion; replaces per-element hard clipping.

**Feature-weighted MSE loss**: Primary drilling features weighted 2x to focus learning on the signals that matter for anomaly detection.

**Adaptive anomaly scoring**: EMA of RMSE -> z-score -> sigmoid(z-2) -> 0-1 score; calibrates after 500 packets.

---

## ML Engine V2.2

Hourly analysis finds optimal drilling conditions for each formation type using **dysfunction-aware** optimization.

### Campaign Weights

| Campaign | ROP Weight | MSE Weight | Stability Weight | Focus |
|----------|------------|------------|------------------|-------|
| **Production** | 50% | 30% | 20% | Drill fast, but stably |
| **Plug & Abandonment** | 25% | 45% | 30% | Operational stability first |

### Pipeline

1. **Data Collection** - Accumulates WITS packets (up to 2 hours at 1 Hz)
2. **Quality Filtering** - Rejects sensor glitches and out-of-range values
3. **Dysfunction Filtering** - Rejects stick-slip, pack-off, founder samples
4. **Formation Segmentation** - Detects boundaries via d-exponent shifts (>15%)
5. **Correlation Analysis** - Pearson correlations (relaxed requirements)
6. **Grid-Based Binning** - 8x6 WOB/RPM grid with stability penalty
7. **Report Generation** - Stores optimal parameters with safe operating ranges

### V2.2 Key Features

- **Dysfunction filtering** - Only analyzes stable, sustainable operating points
  - Torque instability (stick-slip precursor, CV > 12%)
  - Pack-off signatures (torque + SPP both elevated)
  - Founder conditions (WOB up, ROP not responding)
  - Low MSE efficiency (< 50%)
- **Grid-based binning** - 8x6 WOB/RPM grid ensures recommended parameters were actually used together
- **Stability penalty** - Penalizes parameters near dysfunction thresholds
- **Safe operating ranges** - Returns WOB/RPM/Flow min-max ranges, not just point estimates
- **Relaxed correlations** - Proceeds even if p > 0.05 (flags as low confidence)
- **Stability metrics** - `stability_score`, `bin_sample_count`, `bins_evaluated`, `dysfunction_filtered`

### Implementation

| Module | Description |
|--------|-------------|
| `src/ml_engine/analyzer.rs` | Core ML analysis |
| `src/ml_engine/correlations.rs` | Pearson correlation with p-value testing |
| `src/ml_engine/optimal_finder.rs` | Campaign-aware composite scoring |
| `src/ml_engine/dysfunction_filter.rs` | Stick-slip, pack-off, founder sample rejection |
| `src/ml_engine/formation_segmenter.rs` | D-exponent shift detection |
| `src/ml_engine/quality_filter.rs` | Sensor glitch / out-of-range rejection |
| `src/ml_engine/scheduler.rs` | Configurable interval scheduler |
| `src/ml_engine/storage.rs` | ML report storage |

---

## Causal Inference

Detects which drilling parameters causally precede MSE spikes in the real-time history buffer using lightweight Granger-style cross-correlation. No external crates — pure Rust, < 1 ms per packet.

### Properties

| Property | Value |
|----------|-------|
| **Method** | Pearson cross-correlation at lags 1-20 seconds |
| **Target series** | MSE (the efficiency metric being predicted) |
| **Candidate inputs** | WOB, RPM, Torque, SPP, ROP |
| **Threshold** | \|r\| >= 0.45 to report a causal lead |
| **Output** | Up to 3 `CausalLead` results, sorted by \|r\| descending |
| **Min history** | 20 packets required before analysis runs |

### Pipeline Integration

- Phase 4.5 runs causal detection immediately after the history buffer and before advanced physics verification
- `CausalLead` attached to every `AdvisoryTicket` as `causal_leads: Vec<CausalLead>`
- Surfaced in advisory text: *"increase WOB precedes MSE by 12s (r=+0.73); decrease SPP precedes MSE by 4s (r=-0.61)"*
- Causal leads window excludes current packet to prevent self-correlation

### Implementation

| Module | Description |
|--------|-------------|
| `src/causal/mod.rs` | `detect_leads()` -> Vec<CausalLead>; pearson_r() pure-std, no deps |
| `src/types/ticket.rs` | `CausalLead` type: parameter, lag_seconds, pearson_r, direction |
| `src/strategic/templates.rs` | `format_causal_block()` appends leads to template advisories |

---

## Regime-Aware Orchestrator Weighting

Specialist voting weights are dynamically adjusted based on the current drilling regime (0-3) detected by the CfC k-means clusterer (`src/cfc/regime_clusterer.rs`). This tilts expert attention toward the most relevant specialist for the current operating condition while preserving the operator-configured baseline.

### Regime Multiplier Table

| Regime | Label | MSE mult | Hydraulic mult | WellControl mult | Formation mult |
|--------|-------|----------|----------------|------------------|----------------|
| 0 | baseline | x1.0 | x1.0 | x1.0 | x1.0 |
| 1 | hydraulic-stress | x0.8 | x1.4 | x1.0 | x0.8 |
| 2 | high-wob | x1.4 | x0.8 | x0.9 | x1.1 |
| 3 | unstable | x0.7 | x1.0 | x1.5 | x0.8 |

Multipliers are applied on top of `[ensemble_weights]` from `well_config.toml`, then re-normalised so the total always sums to 1.0. Advisory reasoning includes the active regime label (e.g., `[regime 1:hydraulic-stress]`). The WellControl CRITICAL severity override is applied after re-normalisation and is unaffected by regime weighting.

### Implementation

- `RegimeProfile` — struct with four per-specialist multiplicative weight adjustments; `&'static str label` allows `const` array definition with no heap allocation
- `REGIME_PROFILES: [RegimeProfile; 4]` — static table
- `apply_regime_weights(votes, regime_id)` — multiplies each `SpecialistVote.weight` by the regime multiplier then re-normalises
- Out-of-range `regime_id` clamps to regime 3
- Equal-weight fallback when regime scaling zeros all specialist weights

---

## Knowledge Base

Per-well directory-based knowledge base that separates geologist-authored geology from ML-generated performance data and auto-populates offset well performance across the fleet.

### Directory Layout

```
{SAIREN_KB}/
  {field}/
    geology.toml                              # Field-level geological data
    wells/
      {well}/
        pre-spud/
          prognosis.toml                      # Engineering ranges, casings
        mid-well/
          snapshot_{timestamp}.toml            # Recent ML snapshots (plain)
          snapshot_{timestamp}.toml.zst        # Older snapshots (compressed)
        post-well/
          summary.toml                        # Overall well summary
          performance_{formation}.toml         # Per-formation offset data
```

### Components

| Component | Module | Function |
|-----------|--------|----------|
| **Assembler** | `knowledge_base/assembler.rs` | Merges geology + pre-spud + N offset wells into `FormationPrognosis` at runtime |
| **Mid-Well Writer** | `knowledge_base/mid_well.rs` | Writes hourly ML snapshots during drilling, enforces cap (168 hot, then compress, then delete) |
| **Post-Well Generator** | `knowledge_base/post_well.rs` | Aggregates snapshots into per-formation performance files on well completion |
| **Watcher** | `knowledge_base/watcher.rs` | Polls directories for changes, hot-reloads assembled prognosis via `Arc<RwLock>` |
| **Migration** | `knowledge_base/migration.rs` | Converts flat `well_prognosis.toml` into KB directory structure |
| **Compressor** | `knowledge_base/compressor.rs` | Transparent zstd read/write for `.toml` and `.toml.zst` files |

### Assembly Algorithm

1. Load field geology (formations, depths, lithology, hazards)
2. Load well-specific pre-spud engineering parameters
3. Scan sibling wells for post-well performance files
4. Aggregate offset data (weighted average by snapshot count)
5. Merge into `FormationPrognosis` — geologist sets safety envelope, offset wells set target within it

**Legacy fallback:** When `SAIREN_KB` is not set, the system falls back to `FormationPrognosis::load()` from a flat `well_prognosis.toml` file.

---

## P2P Mesh Gossip

Decentralized event sharing between Pi nodes via a broadcast-all gossip protocol. Replaces the previous hub-and-spoke fleet topology. Every node is identical — no central server, no special roles.

### What Gets Shared

- **Anomaly events** — "Rig 1 saw stick-slip at 8,000ft in shale" with full sensor context
- **Outcomes** — "This alert was resolved by reducing WOB 10%" or "This was a false positive"
- **Formation context** — Depth ranges, formation types, risk levels from other rigs' experience

CfC weights are **not** shared — each rig's models are trained on its specific equipment's sensor signatures.

### Components

| Component | Module | Function |
|-----------|--------|----------|
| **GossipEnvelope** | `gossip/protocol.rs` | Wire format: sender_id, version, timestamp, recent_events, known_peers; zstd-compressed JSON |
| **EventStore** | `gossip/store.rs` | SQLite (WAL mode) with indexed columns + zstd-compressed `data` blob; formation/depth/category queries |
| **GossipClient** | `gossip/client.rs` | Async broadcast loop: contacts all peers concurrently each round |
| **GossipServer** | `gossip/server.rs` | Axum handlers: `POST /api/mesh/gossip`, `GET /api/mesh/status`, `GET /api/mesh/fleet` |
| **MeshState** | `gossip/state.rs` | Sled-backed per-peer sync cursors with exponential backoff |
| **FleetEvent** | `fleet/types.rs` | Full advisory + history window + outcome metadata (reused from fleet module) |
| **RAMRecall** | `context/ram_recall.rs` | In-memory episode search with metadata filtering + scoring |

### SQLite Event Store Schema

```sql
CREATE TABLE events (
    id            TEXT PRIMARY KEY,
    rig_id        TEXT NOT NULL,
    well_id       TEXT NOT NULL,
    timestamp     INTEGER NOT NULL,
    last_modified INTEGER NOT NULL,
    formation     TEXT,
    depth_ft      REAL,
    category      TEXT NOT NULL,
    severity      TEXT NOT NULL,
    risk_level    TEXT,
    outcome       TEXT DEFAULT 'Pending',
    action_taken  TEXT,
    data          BLOB NOT NULL              -- zstd-compressed full FleetEvent JSON
);
```

Indexed on `formation`, `category`, `timestamp`, `depth_ft`, `last_modified`. The `last_modified` column is the sync cursor — outcome updates on old events propagate to peers.

### Gossip Protocol

Every `interval_secs` (default 60), each node:

1. For each peer concurrently:
   - Build envelope with events where `last_modified > peer's last_sync_cursor`
   - POST zstd-compressed `GossipEnvelope` to `{peer.address}/api/mesh/gossip`
   - Receive response envelope, upsert events into local store
   - Update per-peer sync cursor

With broadcast-all, every event reaches every reachable node in **one round**. Unreachable peers are skipped with exponential backoff (cap at 1 hour).

### Fleet Dashboard

`GET /api/mesh/fleet` performs server-side aggregation: queries all peers' `/api/mesh/status` concurrently (5s timeout each) and returns a combined fleet view. The browser only talks to one node.

### Retention

Each node independently enforces:
- Max 50,000 events (prune oldest first)
- Age limit: 12 months
- False positives cleaned after 3 months
- Stale pending outcomes scored to 0.05 after 30 days

---

## Trait Architecture

Core system boundaries are abstracted behind traits for swappable backends, testability, and graceful degradation.

| Trait | Module | Implementations | Purpose |
|-------|--------|-----------------|---------|
| **KnowledgeStore** | `context/knowledge_store.rs` | `StaticKnowledgeBase`, `NoOpStore`, `RAMRecall` | Precedent lookup for fleet memory |
| **Specialist** | `agents/specialists/mod.rs` | `MseSpecialist`, `HydraulicSpecialist`, `WellControlSpecialist`, `FormationSpecialist` | Domain-specific risk evaluation |
| **HealthCheck** | `background/self_healer.rs` | `WitsHealthCheck`, `DiskHealthCheck` | Background health monitoring |
| **PersistenceLayer** | `storage/persistence.rs` | `InMemoryDAL` | Advisory and ML report storage |

---

## Advisory Composition

The orchestrator voting and advisory generation are decoupled:

1. **Orchestrator** evaluates all specialists and returns a `VotingResult` (votes, severity, risk level, efficiency score)
2. **AdvisoryComposer** assembles the final `StrategicAdvisory` with a 30-second CRITICAL cooldown to prevent alert spam
3. **Template system** provides campaign-aware advisories with actual metric values and causal leads (confidence: 0.70)

### VotingResult Decoupling

The orchestrator returns `VotingResult` (votes, severity, risk level, efficiency score) instead of directly composing advisories. This separation allows:
- Independent testing of voting logic and advisory formatting
- Template-based advisory system for all recommendations
- CRITICAL cooldown enforcement at the composition layer

### Template System

Campaign-aware template advisories per `AnomalyCategory` with actual metric values and CfC anomaly context. P&A-specific notes for well control. Causal leads appended when present via `format_causal_block()`.

---

## Background Services

Background services run independently of the hot packet pipeline:

| Service | Module | Function |
|---------|--------|----------|
| **SelfHealer** | `background/self_healer.rs` | 30s health check loop with automatic healing |
| **WitsHealthCheck** | `background/self_healer.rs` | Monitors last packet time (30s timeout) |
| **DiskHealthCheck** | `background/self_healer.rs` | Monitors free disk space (warns at 500MB) |

---

## Operation Classification

Automatic detection of current drilling operation based on parameters.

| Operation | Detection Criteria | Campaign |
|-----------|-------------------|----------|
| **Production Drilling** | Default when drilling | Any |
| **Milling** | Torque > 15 kft-lb, ROP < 5 ft/hr | P&A only |
| **Cement Drill-Out** | WOB > 15 klbs, Torque > 12 kft-lb, ROP < 20 ft/hr | P&A only |
| **Circulating** | Flow > 50 gpm, WOB < 5 klbs | Any |
| **Static** | RPM < 10, WOB < 5 klbs | Any |

---

## Performance Benchmarks

### Real-Time Processing

| Metric | Target | Actual |
|--------|--------|--------|
| Tactical Physics + Routing | < 15ms | ~10ms |
| Template Advisory | < 1ms | ~0.1ms |
| CfC Dual Network (rayon) | < 15ms | ~9ms |
| WITS Packet Rate | 1 Hz | 1 Hz |
| History Buffer | 60 packets | 60 |

### Replay Throughput (Volve data, rayon parallel CfC)

| Well | Packets | Wall Time | us/packet | CPU Util | Tickets | Confirmed |
|------|---------|-----------|-----------|----------|---------|-----------|
| F-12 | 2,423,467 | 7.1s | 2.9 | 168% | 70 | 81% |
| F-5 | 181,617 | 2.1s | 11.5 | 177% | 271 | 97% |
| F-9A | 133,557 | 2.3s | 17.2 | 116% | 8 | 100% |
| **Total** | **2,738,641** | **11.5s** | **4.2 avg** | **154% avg** | | |

**Aggregate: ~238,000 packets/second (~4 days of real-time WITS data per second of processing).**

The full pipeline (ACI conformal intervals, dual CfC neural network online training, physics engine, tactical/strategic two-stage verification, specialist voting) processes 2.7M packets in 11.5 seconds with no batch preprocessing or cloud round-trips. All computation runs locally in a single Rust binary with no GPU dependency. The dual CfC networks run in parallel via `rayon::join()`, with CPU utilization of 168-177% on drilling-heavy wells confirming effective parallelization.

---

## Project Structure

```
src/
  main.rs              # Entry point, CLI handling
  lib.rs               # Library crate (shared modules for testing/reuse)
  types/
    mod.rs             # Re-exports from sub-modules
    advisory.rs        # StrategicAdvisory, DrillingPhysicsReport, TraceEntry
    ticket.rs          # AdvisoryTicket, CfcFeatureSurpriseInfo, ThresholdBreach, CausalLead
    wits.rs            # WitsPacket, DrillingMetrics
    state.rs           # Campaign, RigState, Operation
    formation.rs       # FormationInterval, OffsetPerformance, FormationPrognosis
    knowledge_base.rs  # FieldGeology, PreSpudPrognosis, MidWellSnapshot,
                       # PostWellFormationPerformance, PostWellSummary, KnowledgeBaseConfig
    ml.rs              # MLInsightsReport, OptimalParams, ConfidenceLevel
    optimization.rs    # OptimizationAdvisory, ConfidenceBreakdown, LookAheadAdvisory
    tactical.rs        # TacticalAnalysis types
    thresholds.rs      # AnomalyCategory, FinalSeverity, RiskLevel
    debrief.rs         # Post-well debrief types (WellTimeline, DebriefSection) — early/partial

  bin/
    simulation.rs      # WITS Level 0 data simulator for testing
    volve_replay.rs    # Volve field data replay with ACI + CfC shadow logging
    witsml_to_csv.rs   # WITSML XML to CSV converter (Rust binary)

  config/
    mod.rs             # OnceLock global config access (init/get/is_initialized)
    well_config.rs     # WellConfig TOML loader, all threshold structs (~85 fields)
    defaults.rs        # Default value functions for serde(default)
    validation.rs      # walk_toml_keys(), unknown key detection, physical range checks
    auto_detect.rs     # AutoDetector: median-based mud weight detection from WITS packets
    formation.rs       # FormationPrognosis loader (SAIREN_PROGNOSIS env var)
    watcher.rs         # Polling-based file watcher on well_config.toml (mtime, 2s interval, 500ms debounce)

  causal/
    mod.rs             # Causal inference: Granger-style cross-correlation over 60-packet history;
                       # detect_leads() -> Vec<CausalLead>; pearson_r() pure-std, no deps, < 1 ms

  agents/
    tactical.rs        # Fast anomaly detection + operation classification
    strategic.rs       # Physics verification with configurable thresholds
    orchestrator.rs    # Trait-based specialist voting with regime-adjusted weights (VotingResult)
    specialists/
      mod.rs           # Specialist trait + default_specialists() factory
      mse.rs           # MSE efficiency evaluation
      hydraulic.rs     # ECD margin, SPP deviation evaluation
      well_control.rs  # Flow imbalance, pit rate, CRITICAL override
      formation.rs     # D-exponent trends, formation hardness

  strategic/
    mod.rs             # Strategic analysis module
    advisory.rs        # AdvisoryComposer + VotingResult + CRITICAL cooldown
    templates.rs       # Campaign-aware template advisories per AnomalyCategory; appends causal leads
    aggregation.rs     # Report aggregation helpers
    parsing.rs         # Structured output parsing

  pipeline/
    coordinator.rs     # 11-phase pipeline coordinator (uses KnowledgeStore + AdvisoryComposer)
    state.rs           # AppState, system status
    processing_loop.rs # Main WITS packet processing loop
    source.rs          # Data source abstractions (TCP, stdin, CSV)

  volve.rs             # Volve field dataset replay adapter (Kaggle + Tunkiel CSV formats, auto-detect)
  aci.rs               # Adaptive Conformal Inference tracker (online conformal intervals)
  sensors.rs           # Sensor abstractions and health tracking

  cfc/
    mod.rs             # CfC public API, CfcDrillingResult, feature extraction
    normalizer.rs      # Welford's online mean/variance normalization (16 features)
    wiring.rs          # NCP sparse connectivity generation (deterministic from seed)
    cell.rs            # CfC cell forward pass, gate equations, ForwardCache
    training.rs        # Manual BPTT (depth=4), Adam optimizer, gradient norm clipping
    network.rs         # CfcNetwork: process(), anomaly scoring, calibration
    regime_clusterer.rs # K-means clustering of 8 motor outputs -> regime_id (0-3); stamps packet
    formation_detector.rs # CfC-based formation boundary detection from motor output patterns
    checkpoint.rs      # DualCfcCheckpoint, CfcNetworkCheckpoint, atomic disk save/load
    depth_ahead.rs     # Depth-ahead CfC network (64 neurons, seed=1042), 8-feature extraction

  baseline/
    mod.rs             # Adaptive threshold learning with crash-safe persistence

  ml_engine/
    analyzer.rs        # Core ML analysis
    correlations.rs    # Pearson correlation with p-value testing
    optimal_finder.rs  # Campaign-aware composite scoring
    dysfunction_filter.rs  # Stick-slip, pack-off, founder sample rejection
    formation_segmenter.rs # D-exponent shift detection
    quality_filter.rs  # Sensor glitch / out-of-range rejection
    scheduler.rs       # Configurable interval scheduler
    storage.rs         # ML report storage

  physics_engine/
    mod.rs             # Anomaly detection with configurable thresholds
    drilling_models.rs # MSE, d-exponent, kick/loss/founder detection
    metrics.rs         # Metric calculations
    models.rs          # Physics models

  context/
    mod.rs             # Knowledge base module
    vector_db.rs       # Static drilling knowledge base (keyword search)
    knowledge_store.rs # KnowledgeStore trait + NoOpStore + StaticKnowledgeBase
    ram_recall.rs      # RAMRecall: in-memory fleet episode search (metadata filter + scoring)

  optimization/
    mod.rs             # ParameterOptimizer: real-time drilling parameter optimization
    optimizer.rs       # Core evaluate() logic, formation context, offset well blending
    confidence.rs      # ConfidenceBreakdown: multi-factor confidence scoring
    look_ahead.rs      # LookAheadAdvisory: pre-emptive advice for upcoming formations
    rate_limiter.rs    # Advisory rate limiting (cooldown between optimization advisories)
    templates.rs       # Human-readable recommendation text generation

  knowledge_base/        # Structured per-well knowledge base
    mod.rs             # KnowledgeBase struct, init, hot-reload prognosis
    assembler.rs       # Merge geology + pre-spud + offset wells -> FormationPrognosis
    compressor.rs      # Transparent zstd read/write for .toml and .toml.zst
    layout.rs          # Directory path helpers, file enumeration
    mid_well.rs        # ML snapshot writer + cap enforcement (compress old, delete expired)
    post_well.rs       # Post-well summary generator from mid-well snapshots
    watcher.rs         # Polling directory watcher, triggers reassembly on changes
    migration.rs       # Flat well_prognosis.toml -> KB directory migration

  fleet/
    mod.rs             # Fleet data types module
    types.rs           # FleetEvent, FleetEpisode, EventOutcome, HistorySnapshot

  gossip/                # P2P mesh gossip protocol
    mod.rs             # Module exports
    protocol.rs        # GossipEnvelope, zstd compress/decompress helpers
    store.rs           # SQLite event store (WAL mode, indexed columns, zstd data blob)
    client.rs          # Async broadcast loop: contact all peers concurrently each round
    server.rs          # Axum handlers: POST /api/mesh/gossip, GET /api/mesh/status, GET /api/mesh/fleet
    state.rs           # Sled-backed per-peer sync cursors with exponential backoff

  background/
    mod.rs             # Background services module
    self_healer.rs     # HealthCheck trait + SelfHealer + WITS/Disk health checks

  storage/
    mod.rs             # Storage module
    persistence.rs     # PersistenceLayer trait + InMemoryDAL
    history.rs         # Advisory history storage
    strategic.rs       # Strategic report storage
    lockfile.rs        # Process lock file management
    feedback.rs        # FeedbackRecord, FeedbackOutcome, sled-persisted operator feedback
    suggestions.rs     # CategoryStats, ThresholdSuggestion, compute_stats(), compute_suggestions()
    damping_recipes.rs # Per-formation damping recipe library (sled), recipe blending + pruning

  acquisition/
    mod.rs             # Acquisition module exports
    wits_parser.rs     # WITS Level 0 TCP with reconnection, timeouts,
                       # and data quality validation
    scanner.rs         # WITS subnet scanner for setup wizard (port probing)

  api/
    mod.rs             # App builder, CORS, SPA fallback (rust-embed serving React dashboard)
    routes.rs          # v1 HTTP route definitions (deprecated, adds Sunset headers)
    v2_routes.rs       # v2 HTTP route definitions (primary API)
    v2_handlers.rs     # v2 request handlers (~880 lines, consolidated live endpoint)
    envelope.rs        # ApiResponse<T> / ApiErrorResponse wrappers for v2 JSON envelope
    middleware.rs       # v1 deprecation headers (Deprecation: true, Sunset: 2026-09-01)
    setup.rs           # Setup wizard: WITS scanning, well config, fleet pairing UI
    handlers/
      mod.rs           # DashboardState struct + re-exports from sub-modules
      status.rs        # Health, status, diagnosis, baseline endpoints
      drilling.rs      # Drilling metrics, verification, campaign endpoints
      reports.rs       # Strategic reports, critical reports, shift summary
      ml.rs            # ML insights, history, optimal parameters
      config.rs        # Config get/update/validate, advisory acknowledgment
      metrics.rs       # Prometheus metrics, fleet intelligence cache


dashboard/               # React SPA (Vite + Tailwind + Recharts)
  src/
    api/
      types.ts         # TypeScript interfaces (LiveData, CriticalReport, CategoryStats, ThresholdSuggestion, LookaheadStatus)
      client.ts        # API client (fetchLive, submitFeedback, fetchFeedbackStats, fetchConfigSuggestions, fetchLookaheadStatus)
    components/
      live/
        LiveView.tsx       # Main live dashboard view
        HealthGauge.tsx    # Radial health score gauge
        DrillingParams.tsx # Real-time drilling parameters
        WellControl.tsx    # Well control indicators
        LookAheadPanel.tsx # Formation lookahead advisory panel (polls /api/v2/lookahead/status)
        SpecialistVotes.tsx # Specialist voting display
        BaselineProgress.tsx # Baseline learning progress
      reports/
        CriticalReports.tsx # Critical advisory reports with feedback buttons (Confirmed/FP/Unclear)
      feedback/
        FeedbackView.tsx   # Feedback analytics page (category stats + threshold suggestions)
      charts/            # FlowBalanceChart, MSEChart (Recharts)
      layout/            # Header (with nav items), sidebar
    hooks/
      usePolling.ts      # Generic polling hook for API data
    App.tsx              # React Router (Live, Hourly, Daily, Reports, Feedback)
  index.html           # SPA entry point
  vite.config.ts       # Vite build configuration
  tailwind.config.ts   # Tailwind CSS configuration
  package.json         # Node dependencies
  dist/                # Production build (embedded via rust-embed)

static/
  index.html           # Legacy rig dashboard UI
  reports.html         # Strategic reports viewer
  setup.html           # Setup wizard UI (embedded in setup binary)

deploy/
  sairen-os.service    # systemd service unit for rig (hardened)
  install.sh           # Rig production install script
  wireguard/
    hub_wg0.conf.template   # WireGuard config template for mesh node
    rig_wg0.conf.template   # WireGuard config template for mesh node

tests/
  api_regression.rs              # In-process API regression tests (tower::oneshot)
  auto_detect_tests.rs           # Phase 2 auto-detection integration tests
  config_validation_tests.rs     # Config validation integration tests
  csv_replay_integration.rs      # CSV replay integration tests
  gossip_integration.rs          # P2P gossip exchange and fleet aggregation tests
  knowledge_base_integration.rs  # KB lifecycle tests (migrate, assemble, offset wells)
  pipeline_regression.rs         # Pipeline regression tests (synthetic data, always passes)

scripts/
  witsml_to_csv.py       # WITSML 1.4.1 XML -> Kaggle CSV converter (extracts from Volve zip)

well_config.default.toml  # Reference configuration with all thresholds documented
```

---

## Developer Glossary

| Term | Description |
|------|-------------|
| **CfC** | Closed-form Continuous-time neural network; RNN variant with time-gated updates that naturally handles irregular time steps |
| **NCP** | Neural Circuit Policy; sparse wiring topology inspired by biological neural circuits (sensory -> inter -> command -> motor) |
| **BPTT** | Backpropagation Through Time; training RNNs by unrolling through multiple timesteps (depth=4 in SAIREN-OS) |
| **Adam** | Adaptive Moment Estimation optimizer; maintains per-parameter learning rates via first/second moment tracking |
| **Feature Surprise** | Per-feature anomaly decomposition from CfC; reports which sensors deviated most from prediction |
| **Tiebreaker** | CfC resolves Uncertain strategic verifications: score >= 0.7 -> Confirmed, < 0.2 -> Rejected |
| **ACI** | Adaptive Conformal Inference; online conformal intervals for distribution-free anomaly detection |
| **WITSML** | Wellsite Information Transfer Standard Markup Language; XML-based format for well data exchange (1.4.1 series supported) |
| **Knowledge Base** | Structured per-well directory of geology, engineering, and performance files |
| **Offset Well** | A previously drilled well in the same field whose performance data informs the current well |
| **Pre-Spud Prognosis** | Well-specific engineering plan authored before drilling begins |
| **Mid-Well Snapshot** | Hourly ML performance snapshot written during drilling |
| **Post-Well Summary** | Aggregated performance data generated after well completion, shared across the fleet |
| **CausalLead** | A detected leading indicator: a drilling parameter whose change precedes an MSE shift by `lag_seconds` |
| **Regime ID** | 0-3 integer stamped on each packet by the CfC k-means clusterer based on motor neuron output patterns |
| **RegimeProfile** | Multiplicative weight adjustment table per drilling regime; applied to `ensemble_weights` before orchestrator voting |
| **Granger Causality** | Statistical test for whether one time series improves prediction of another; approximated via Pearson cross-correlation |
| **FleetEvent** | An AMBER/RED advisory with history window and outcome, shared via gossip |
| **FleetEpisode** | Compact precedent extracted from a FleetEvent, used for knowledge lookup |
| **GossipEnvelope** | Wire format for P2P event exchange: sender_id, version, timestamp, recent_events; zstd-compressed JSON |
| **EventStore** | SQLite database on each node storing gossipped events with indexed columns for structured queries |
| **RAMRecall** | In-memory episode search on each rig, populated from the local event store |
| **Founder Point** | The WOB at which ROP peaks; beyond this point, additional weight reduces efficiency |
