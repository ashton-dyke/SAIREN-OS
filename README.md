# SAIREN-OS - Drilling Operational Intelligence System

Real-time drilling advisory system using WITS Level 0 data and an 11-phase multi-agent AI pipeline for drilling optimization and risk prevention.

---

## Table of Contents

1. [Quick Start](#quick-start)
2. [Overview](#overview)
3. [Features](#features)
4. [Architecture](#architecture)
5. [Running the System](#running-the-system)
6. [Fleet Hub](#fleet-hub)
7. [Configuration](#configuration)
8. [Deployment](#deployment)
9. [WITS Simulator](#wits-simulator)
10. [API Reference](#api-reference)
11. [Understanding Advisories](#understanding-advisories)
12. [Thresholds Reference](#thresholds-reference)
13. [Troubleshooting](#troubleshooting)
14. [Project Structure](#project-structure)
15. [Glossary](#glossary)
16. [Changelog](#changelog)

---

## Quick Start

```bash
# 1. Build the system (use 'cuda' instead of 'llm' if you have a GPU)
cargo build --release --features llm

# 2. Terminal 1: Start the WITS simulator
python3 wits_simulator.py

# 3. Terminal 2: Run SAIREN-OS
./target/release/sairen-os --wits-tcp localhost:5000

# 4. View the dashboard
open http://localhost:8080

# 5. Inject faults in simulator (press in Terminal 1):
#    K = Kick, S = Stick-slip, P = Pack-off, M = Milling
```

---

## Overview

**The Problem**: Drilling operations generate massive amounts of real-time data (40+ WITS channels) that operators must monitor continuously. Suboptimal drilling parameters waste money (reduced ROP), while missed warning signs can lead to:
- **$1M+ per day** in lost drilling time from stuck pipe, kicks, or equipment damage
- Safety risks from well control events
- Non-productive time (NPT) from preventable failures

**The Solution**: SAIREN-OS continuously monitors WITS Level 0 drilling data and uses physics-based analysis plus AI to:

1. **Optimize drilling efficiency** - Detect MSE inefficiency and recommend WOB/RPM adjustments
2. **Prevent well control events** - Early detection of kicks, losses, and gas influx
3. **Predict mechanical issues** - Detect pack-offs, stick-slip, founder conditions, and tool failures before they occur
4. **Track formation changes** - D-exponent trends for pore pressure monitoring
5. **Detect founder conditions** - Identify when WOB exceeds optimal and ROP stops responding

**Think of it like**: An AI drilling engineer that never sleeps, continuously analyzing every parameter and providing actionable recommendations.

---

## Features

### Campaign System

Switch between operational modes that adjust thresholds, LLM prompts, and specialist weights for context-appropriate advisories.

| Campaign | Focus | Flow Warning | Well Control Weight |
|----------|-------|--------------|---------------------|
| **Production** | ROP optimization, MSE efficiency | 10 gpm | 30% |
| **Plug & Abandonment** | Cement integrity, pressure testing | 5 gpm | 40% |

**Switch campaigns via:**
- Dashboard dropdown (top-left)
- API: `POST /api/v1/campaign` with `{"campaign":"PlugAbandonment"}`
- Environment: `CAMPAIGN=pa` when starting SAIREN-OS

### Operation Classification

Automatic detection of current drilling operation based on parameters.

| Operation | Detection Criteria | Campaign |
|-----------|-------------------|----------|
| **Production Drilling** | Default when drilling | Any |
| **Milling** | Torque > 15 kft-lb, ROP < 5 ft/hr | P&A only |
| **Cement Drill-Out** | WOB > 15 klbs, Torque > 12 kft-lb, ROP < 20 ft/hr | P&A only |
| **Circulating** | Flow > 50 gpm, WOB < 5 klbs | Any |
| **Static** | RPM < 10, WOB < 5 klbs | Any |

### CfC Neural Network (Active)

A 128-neuron Closed-form Continuous-time (CfC) neural network with Neural Circuit Policy (NCP) sparse wiring that actively participates in the decision pipeline. The network is **self-supervised** — it predicts next-timestep sensor values and treats prediction error as an anomaly signal. No labeled training data needed.

| Property | Value |
|----------|-------|
| **Neurons** | 128 CfC (24 sensory → 64 inter → 32 command → 8 motor) |
| **Parameters** | ~6,051 trainable |
| **NCP Connections** | ~1,833 (~30% sparse wiring) |
| **Input Features** | 16 (WOB, ROP, RPM, torque, MSE, SPP, d-exp, hookload, ECD, flow balance, pit rate, DXC, pump SPM, mud weight, gas, pit volume) |
| **Outputs** | 16 next-step predictions, anomaly score (0-1), health score (0-1), per-feature surprise decomposition |
| **Training** | Online Adam optimizer with BPTT depth=4, gradient norm clipping |
| **Calibration** | 500 packets before producing anomaly scores |

**How it works:**
1. Each packet: normalize features (Welford's online mean/variance)
2. Train: compare previous predictions against current reality (feature-weighted MSE loss, primary features 2x)
3. Forward: predict next timestep through CfC gates (tau/f/g time-gated update)
4. Score: adaptive z-score of prediction RMSE → sigmoid → anomaly score (0-1)

**Active integration** — CfC participates in three pipeline stages:

| Stage | Role | Mechanism |
|-------|------|-----------|
| **Severity modulation** (Phase 3) | Adjusts ticket severity like ACI | Score < 0.3 → downgrade, ≥ 0.7 → escalate |
| **LLM context** (Phase 7) | CfC section in strategic prompt | Anomaly score + top 5 surprised features |
| **Tiebreaker** (Phase 5) | Resolves Uncertain verifications | Score ≥ 0.7 → Confirmed, < 0.2 → Rejected |

Safety rule: WellControl tickets are never downgraded below High severity.

**Per-feature surprise decomposition**: When the CfC detects anomalies, it reports which specific features deviated most from prediction (e.g., "SPP ↑2.36σ, torque ↑0.48σ"), giving operators and the strategic LLM interpretable context for the anomaly signal.

**Validated on 3 Volve wells:**

| Well | Packets | Avg Loss | Tickets | Confirmation Rate | Notes |
|------|---------|----------|---------|-------------------|-------|
| **F-5** | 181,617 | 0.226 | 144 | 97% | 11 CfC tiebreaker corroborations |
| **F-9A** | 87,876 | 0.702 | 3 | 0% (all rejected) | Quiet well, correct behavior |
| **F-12** (unseen) | 2,423,467 | 0.882 | 222 | 47% | First-time well, CfC calibrated online |

### ML Engine (V2.2)

Hourly analysis finds optimal drilling conditions for each formation type using **dysfunction-aware** optimization.

| Campaign | ROP Weight | MSE Weight | Stability Weight | Focus |
|----------|------------|------------|------------------|-------|
| **Production** | 50% | 30% | 20% | Drill fast, but stably |
| **Plug & Abandonment** | 25% | 45% | 30% | Operational stability first |

**ML Pipeline (V2.2):**
1. Data Collection - Accumulates WITS packets (up to 2 hours at 1 Hz)
2. Quality Filtering - Rejects sensor glitches and out-of-range values
3. **Dysfunction Filtering** - Rejects stick-slip, pack-off, founder samples
4. Formation Segmentation - Detects boundaries via d-exponent shifts (>15%)
5. Correlation Analysis - Pearson correlations (relaxed requirements)
6. **Grid-Based Binning** - 8×6 WOB/RPM grid with stability penalty
7. Report Generation - Stores optimal parameters with safe operating ranges

**V2.2 Key Features:**
- **Dysfunction filtering** - Only analyzes stable, sustainable operating points
- **Stability penalty** - Penalizes parameters near dysfunction thresholds
- **Safe operating ranges** - Returns WOB/RPM/Flow min-max ranges, not just point estimates
- **Relaxed correlations** - Proceeds even if p > 0.05 (flags as low confidence)

### Trait-Based Architecture

Core system boundaries are abstracted behind traits for swappable backends, testability, and graceful degradation.

| Trait | Module | Implementations | Purpose |
|-------|--------|-----------------|---------|
| **KnowledgeStore** | `context/knowledge_store.rs` | `StaticKnowledgeBase`, `NoOpStore`, `RAMRecall` | Precedent lookup for fleet memory |
| **Specialist** | `agents/specialists/mod.rs` | `MseSpecialist`, `HydraulicSpecialist`, `WellControlSpecialist`, `FormationSpecialist` | Domain-specific risk evaluation |
| **HealthCheck** | `background/self_healer.rs` | `WitsHealthCheck`, `DiskHealthCheck` | Background health monitoring |
| **PersistenceLayer** | `storage/persistence.rs` | `InMemoryDAL` | Advisory and ML report storage |

### Advisory Composition

The orchestrator voting and advisory generation are decoupled:

1. **Orchestrator** evaluates all specialists and returns a `VotingResult` (votes, severity, risk level, efficiency score)
2. **AdvisoryComposer** assembles the final `StrategicAdvisory` with a 30-second CRITICAL cooldown to prevent alert spam
3. **Template fallback** provides campaign-aware advisories when LLM is unavailable (confidence: 0.70)

### Fleet Hub (Hub-and-Spoke)

Complete multi-rig fleet learning system with a central hub server and spoke-side clients on each rig.

**Rig-side (Spoke) components:**

| Component | Module | Function |
|-----------|--------|----------|
| **FleetEvent** | `fleet/types.rs` | Full advisory + history window + outcome metadata |
| **FleetEpisode** | `fleet/types.rs` | Compact precedent for library (from_event constructor) |
| **UploadQueue** | `fleet/queue.rs` | Disk-backed durable queue, idempotent by event ID |
| **FleetClient** | `fleet/client.rs` | HTTP client for hub communication (upload, sync, outcome forwarding) |
| **Uploader** | `fleet/uploader.rs` | Background task draining queue to hub with retry |
| **LibrarySync** | `fleet/sync.rs` | Periodic library pull from hub with jitter |
| **RAMRecall** | `context/ram_recall.rs` | In-memory episode search with metadata filtering + scoring |

**Hub-side (Central Server) components:**

| Component | Module | Function |
|-----------|--------|----------|
| **Fleet Hub Binary** | `bin/fleet_hub.rs` | Standalone Axum server with PostgreSQL backend |
| **Event Ingestion** | `hub/api/events.rs` | Validates, decompresses, and stores uploaded events |
| **Library Curator** | `hub/curator/` | Scores, deduplicates, and prunes episodes on a configurable schedule |
| **Library Sync API** | `hub/api/library.rs` | Delta sync with zstd compression and version tracking |
| **Rig Registry** | `hub/api/registry.rs` | API key management with bcrypt hashing and cache |
| **Auth Middleware** | `hub/auth/` | Bearer token extractors (RigAuth, AdminAuth) with 5-min cache |
| **Fleet Dashboard** | `hub/api/dashboard.rs` | Real-time fleet overview with Chart.js visualizations |

**Key design principles:**
- Only AMBER/RED events qualify for upload (`should_upload()` filter)
- Upload queue survives process restarts (scans directory on open)
- Rigs operate independently when hub is unreachable (local autonomy)
- Bandwidth-conscious: zstd compression, delta sync, configurable cadence
- RAMRecall holds up to 10,000 episodes in memory (~50MB)
- Hub episodes scored by outcome (50%), recency (25%), detail (15%), diversity (10%)

### Structured Knowledge Base

Per-well directory-based knowledge base that separates geologist-authored geology from ML-generated performance data and auto-populates offset well performance across the fleet.

**Directory layout:**
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

**Key capabilities:**

| Component | Module | Function |
|-----------|--------|----------|
| **Assembler** | `knowledge_base/assembler.rs` | Merges geology + pre-spud + N offset wells into `FormationPrognosis` at runtime |
| **Mid-Well Writer** | `knowledge_base/mid_well.rs` | Writes hourly ML snapshots during drilling, enforces cap (168 hot, then compress, then delete) |
| **Post-Well Generator** | `knowledge_base/post_well.rs` | Aggregates snapshots into per-formation performance files on well completion |
| **Watcher** | `knowledge_base/watcher.rs` | Polls directories for changes, hot-reloads assembled prognosis |
| **Fleet Bridge** | `knowledge_base/fleet_bridge.rs` | Uploads post-well data to hub, downloads offset data from hub |
| **Migration** | `knowledge_base/migration.rs` | Converts flat `well_prognosis.toml` into KB directory structure |
| **Compressor** | `knowledge_base/compressor.rs` | Transparent zstd read/write for `.toml` and `.toml.zst` files |

**Assembly algorithm:**
1. Load field geology (formations, depths, lithology, hazards)
2. Load well-specific pre-spud engineering parameters
3. Scan sibling wells for post-well performance files
4. Aggregate offset data (weighted average by snapshot count)
5. Merge into `FormationPrognosis` — geologist sets safety envelope, offset wells set target within it

**Legacy fallback:** When `SAIREN_KB` is not set, the system falls back to `FormationPrognosis::load()` from a flat `well_prognosis.toml` file.

### Causal Inference on Edge

Detects which drilling parameters causally precede MSE spikes in the real-time history buffer using lightweight Granger-style cross-correlation. No external crates — pure Rust, < 1 ms per packet.

| Property | Value |
|----------|-------|
| **Method** | Pearson cross-correlation at lags 1–20 seconds |
| **Target series** | MSE (the efficiency metric being predicted) |
| **Candidate inputs** | WOB, RPM, Torque, SPP, ROP |
| **Threshold** | \|r\| ≥ 0.45 to report a causal lead |
| **Output** | Up to 3 `CausalLead` results, sorted by \|r\| descending |
| **Min history** | 20 packets required before analysis runs |

Causal leads are attached to every `AdvisoryTicket` as `causal_leads: Vec<CausalLead>` and surfaced in advisory text: *"increase WOB precedes MSE by 12s (r=+0.73); decrease SPP precedes MSE by 4s (r=−0.61)"*. This gives operators leading-indicator context — not just that MSE is elevated, but which parameter is driving it and how far ahead the signal appears.

### Regime-Aware Orchestrator Weighting

Specialist voting weights are dynamically adjusted based on the current drilling regime (0–3) detected by the CfC k-means clusterer (`src/cfc/regime_clusterer.rs`). This tilts expert attention toward the most relevant specialist for the current operating condition while preserving the operator-configured baseline.

| Regime | Label | MSE mult | Hydraulic mult | WellControl mult | Formation mult |
|--------|-------|----------|----------------|------------------|----------------|
| 0 | baseline | ×1.0 | ×1.0 | ×1.0 | ×1.0 |
| 1 | hydraulic-stress | ×0.8 | ×1.4 | ×1.0 | ×0.8 |
| 2 | high-wob | ×1.4 | ×0.8 | ×0.9 | ×1.1 |
| 3 | unstable | ×0.7 | ×1.0 | ×1.5 | ×0.8 |

Multipliers are applied on top of `[ensemble_weights]` from `well_config.toml`, then re-normalised so the total always sums to 1.0. Advisory reasoning includes the active regime label (e.g., `[regime 1:hydraulic-stress]`). The WellControl CRITICAL severity override is applied after re-normalisation and is unaffected by regime weighting.

### Background Services

Background services run independently of the hot packet pipeline:

| Service | Module | Function |
|---------|--------|----------|
| **SelfHealer** | `background/self_healer.rs` | 30s health check loop with automatic healing |
| **WitsHealthCheck** | `background/self_healer.rs` | Monitors last packet time (30s timeout) |
| **DiskHealthCheck** | `background/self_healer.rs` | Monitors free disk space (warns at 500MB) |

---

## Architecture

SAIREN-OS uses a two-stage multi-agent architecture where a fast **Tactical Agent** handles real-time anomaly detection via deterministic pattern-matched routing, and a deeper **Strategic Agent** performs comprehensive drilling physics analysis only when anomalies are detected. Structured `TicketContext` (all threshold breaches, pattern name, rig state, operation, campaign) travels with each ticket and is templated into the strategic LLM prompt.

The **Orchestrator** uses trait-based `Specialist` implementations for domain-specific evaluation, returning a `VotingResult`. The **AdvisoryComposer** then assembles the final `StrategicAdvisory` with CRITICAL cooldown (30s) to prevent alert spam. Knowledge lookup uses the `KnowledgeStore` trait, allowing swappable backends (static DB, RAMRecall, or NoOp for pilot mode).

**Fleet topology**: A central **Fleet Hub** collects AMBER/RED events from all rigs, curates them into a scored episode library, and syncs the library back so every rig benefits from fleet-wide precedents.

```
                           Fleet Hub-and-Spoke Topology
    ===============================================================================

    +----------+                                               +----------+
    |  RIG-001 |---upload events--->  +------------------+  <--| RIG-003  |
    | (spoke)  |<--sync library----  |   FLEET HUB      |  -->| (spoke)  |
    +----------+                     |  (PostgreSQL)     |     +----------+
                                     |                   |
    +----------+                     |  - Event Store    |     +----------+
    |  RIG-002 |---upload events-->  |  - Curator        |  <--| RIG-004  |
    | (spoke)  |<--sync library----  |  - Episode Library|  -->| (spoke)  |
    +----------+                     |  - Dashboard      |     +----------+
                                     +------------------+
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

### 11-Phase Processing Pipeline

| Phase | Component | Function |
|-------|-----------|----------|
| 1 | WITS Ingestion | Receive 40+ channel WITS Level 0 packets, classify rig state |
| 2 | Tactical Physics | Calculate MSE, d-exponent, flow balance, pit rate (<15ms) |
| 2.8 | CfC Network | Self-supervised neural network: predict, compare, train, score, modulate severity; stamp `regime_id` via k-means clusterer |
| 3 | Decision Gate | Create AdvisoryTicket if thresholds exceeded |
| 4 | History Buffer | Store last 60 packets for trend analysis |
| 4.5 | Causal Inference | Cross-correlate WOB/RPM/Torque/SPP/ROP against MSE at lags 1–20s; attach `CausalLead` results to ticket |
| 5 | Advanced Physics | Strategic verification of tickets (CfC tiebreaker on Uncertain) |
| 6 | Context Lookup | Query KnowledgeStore (StaticKnowledgeBase, RAMRecall, or NoOp) |
| 7 | LLM Advisory | Generate recommendations (Qwen 2.5 7B) or template fallback (causal leads appended) |
| 8 | Orchestrator Voting | 4 trait-based specialists vote with regime-adjusted weights → VotingResult |
| 9 | Advisory Composition | AdvisoryComposer assembles StrategicAdvisory (CRITICAL cooldown) |
| 10 | Dashboard API | REST endpoints and web dashboard |

### Specialist Weights

| Specialist | Baseline Weight | Evaluates |
|------------|-----------------|-----------|
| **MSE** | 25% | Drilling efficiency, ROP optimization |
| **Hydraulic** | 25% | SPP, ECD margin, flow rates |
| **WellControl** | 30% | Kick/loss indicators, gas, pit volume |
| **Formation** | 20% | D-exponent trends, formation changes |

> **Regime adjustment**: Phase 4.5 stamps `regime_id` (0–3) on each packet via the CfC motor-output k-means clusterer. Phase 8 applies regime-specific multipliers to the baseline weights above and re-normalises to 1.0 before voting. See the [Regime-Aware Orchestrator Weighting](#regime-aware-orchestrator-weighting) feature section for the multiplier table.

---

## Running the System

### With WITS Simulator (Recommended)

```bash
# Terminal 1: Start simulator
python3 wits_simulator.py

# Terminal 2: Run SAIREN-OS
./target/release/sairen-os --wits-tcp localhost:5000
```

### With P&A Campaign

```bash
# Terminal 1: Simulator in P&A mode
python3 wits_simulator.py --campaign pa

# Terminal 2: SAIREN-OS in P&A mode
CAMPAIGN=pa ./target/release/sairen-os --wits-tcp localhost:5000
```

### Testing P&A Operations

```bash
# Simulator with milling mode (high torque)
python3 wits_simulator.py --campaign pa --operation milling

# Simulator with cement drill-out mode (high WOB)
python3 wits_simulator.py --campaign pa --operation cement-drillout
```

### Volve Field Data Replay

Replay historical Volve field data through the full pipeline with ACI conformal intervals and CfC neural network.

```bash
# Replay a well CSV (Kaggle or Tunkiel format auto-detected)
cargo run --bin volve-replay -- --file data/volve/F-5_rt_input.csv

# Extract WITSML XML from Volve zip archive to Kaggle CSV
python3 scripts/witsml_to_csv.py F-12 data/volve/F-12_witsml.csv

# Replay the extracted well
cargo run --bin volve-replay -- --file data/volve/F-12_witsml.csv
```

The WITSML converter maps 40+ WITSML mnemonics (SWOB, TQA, RPMA, ROP5, SPPA, etc.) to Kaggle-format columns, merges multiple log segments by timestamp, and handles the Volve zip archive's directory structure automatically.

### Building Options

```bash
# With LLM support (CPU inference - works on any machine)
cargo build --release --features llm

# With LLM support + GPU acceleration (requires CUDA toolkit)
cargo build --release --features cuda

# Without LLM (template-based advisories only)
cargo build --release

# Fleet Hub server (requires PostgreSQL)
cargo build --release --bin fleet-hub --features fleet-hub

# Rig with fleet connectivity (upload events, sync library)
cargo build --release --features fleet-client
```

**Hardware auto-detection**: When built with `llm` or `cuda`, SAIREN-OS checks for CUDA at startup and automatically selects the right model:

| Hardware | Tactical Routing | Strategic Model | Build Flag |
|----------|-----------------|----------------|------------|
| **GPU** (CUDA) | Deterministic pattern matching | Qwen 2.5 7B (~800ms) | `--features cuda` |
| **CPU** | Deterministic pattern matching | Qwen 2.5 4B (~10-30s) | `--features llm` |
| **No LLM** | Deterministic pattern matching | Template-based | *(default)* |

**Feature flags:**

| Feature | Flag | Description |
|---------|------|-------------|
| **LLM (CPU)** | `--features llm` | Qwen 2.5 strategic advisory generation |
| **LLM (GPU)** | `--features cuda` | CUDA-accelerated LLM inference |
| **Fleet Hub** | `--features fleet-hub` | Central hub server binary (PostgreSQL, API, curator) |
| **Fleet Client** | `--features fleet-client` | Spoke-side HTTP client (upload, sync, outcome forwarding) |
| **Knowledge Base** | `--features knowledge-base` | Structured per-well knowledge base with auto-assembly (enabled by default) |
| **Tactical LLM** | `--features tactical_llm` | Legacy LLM-based tactical routing (not recommended) |

> **Note**: The tactical agent uses deterministic physics-based pattern matching (no LLM required).
> Feature flags are additive and can be combined (e.g., `--features "llm,fleet-client"`).

---

## Fleet Hub

The Fleet Hub is an optional central server that enables fleet-wide learning across multiple rigs. Each rig operates autonomously and uploads significant events to the hub. The hub curates events into a scored episode library that is synced back to all rigs.

### Running the Fleet Hub

```bash
# 1. Ensure PostgreSQL is running
# 2. Set DATABASE_URL
export DATABASE_URL=postgres://sairen:password@localhost/sairen_fleet
export FLEET_ADMIN_KEY=your-admin-key

# 3. Start the hub (migrations run automatically)
./target/release/fleet-hub --port 8080

# 4. View the fleet dashboard
open http://localhost:8080
```

### Registering a Rig

```bash
# Register a new rig (returns a one-time API key)
curl -X POST http://hub:8080/api/fleet/rigs/register \
  -H "Authorization: Bearer $FLEET_ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{"rig_id": "RIG-001", "well_id": "WELL-A1", "field": "North Sea"}'
```

### Hub Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | *(required)* | PostgreSQL connection URL |
| `FLEET_ADMIN_KEY` | `admin-dev-key` | Admin API key for rig registration and dashboard |
| `FLEET_MAX_PAYLOAD_SIZE` | `1048576` | Max event upload size in bytes (1 MB) |
| `FLEET_CURATION_INTERVAL` | `3600` | Curation cycle interval in seconds |
| `FLEET_LIBRARY_MAX_EPISODES` | `50000` | Maximum episodes before pruning |

### Hub CLI Arguments

| Argument | Description |
|----------|-------------|
| `--database-url <url>` | PostgreSQL connection URL (overrides env) |
| `--port <N>` | Port to listen on (default: 8080) |
| `--bind-address <addr>` | Full bind address (overrides --port) |

### Curator Rules

The background curator runs hourly (configurable) and applies these rules:

| Rule | Condition | Action |
|------|-----------|--------|
| Age limit | Episode > 12 months | Archive |
| False positive cleanup | FalsePositive + age > 3 months | Archive |
| Stale pending | Pending + age > 30 days | Downgrade score to 0.05 |
| Capacity limit | Total > 50,000 | Prune lowest-scored |

### Episode Scoring

Episodes are scored for library ranking based on four factors:

| Factor | Weight | Description |
|--------|--------|-------------|
| Outcome quality | 50% | Resolved (1.0), Escalated (0.7), Pending (0.2), FalsePositive (0.1) |
| Recency | 25% | Exponential decay (half-life ~180 days) |
| Detail completeness | 15% | Resolution notes, action taken, metrics present |
| Category diversity | 10% | Underrepresented categories score higher |

### Network Architecture

The hub communicates with rigs over a WireGuard VPN tunnel. Configuration templates are provided in `deploy/wireguard/`.

```
Rig (10.0.1.X) ──── WireGuard Tunnel ──── Hub (10.0.0.1:8080)
                     (port 51820)
```

---

## Configuration

### Well Configuration File (`well_config.toml`)

Every drilling threshold in SAIREN-OS is configurable via a single TOML file. The system searches for configuration in this order:

1. `$SAIREN_CONFIG` environment variable (if set)
2. `./well_config.toml` in the working directory
3. Built-in defaults (safe for most wells)

Copy the reference config and edit for your well:

```bash
cp well_config.default.toml well_config.toml
vi well_config.toml
```

**Key sections:**

| Section | Controls | Example |
|---------|----------|---------|
| `[well]` | Well name, rig ID, bit diameter | `bit_diameter_inches = 8.5` |
| `[thresholds.well_control]` | Kick/loss warning & critical triggers | `flow_imbalance_warning_gpm = 10.0` |
| `[thresholds.mse]` | MSE efficiency bands | `efficiency_poor_percent = 50.0` |
| `[thresholds.hydraulics]` | ECD margin, SPP deviation | `ecd_margin_warning_ppg = 0.3` |
| `[thresholds.mechanical]` | Torque, pack-off detection | `torque_increase_warning_pct = 15.0` |
| `[thresholds.founder]` | Founder point detection sensitivity | `quick_wob_delta_percent = 0.05` |
| `[baseline_learning]` | Sigma thresholds, min samples | `min_samples_for_lock = 100` |
| `[ensemble_weights]` | Specialist voting weights (must sum to ~1.0) | `well_control = 0.30` |
| `[physics]` | Mud weight, formation constants | `normal_mud_weight_ppg = 10.0` |
| `[campaign.*]` | Per-campaign threshold overrides | `[campaign.plug_abandonment]` |

Only include sections you want to override — all omitted values use safe defaults. The system validates consistency on load (e.g., critical > warning thresholds, weights sum check).

### Runtime Configuration API

Thresholds can be viewed and updated at runtime without restarting:

```bash
# View current config
curl http://localhost:8080/api/v1/config | jq .

# Update thresholds (validates before applying, saves to well_config.toml)
curl -X POST http://localhost:8080/api/v1/config \
  -H "Content-Type: application/json" \
  -d '{"config": {"thresholds": {"well_control": {"flow_imbalance_warning_gpm": 8.0}}}}'

# Validate without applying
curl -X POST http://localhost:8080/api/v1/config/validate \
  -H "Content-Type: application/json" \
  -d '{"config": {"ensemble_weights": {"mse": 0.5, "hydraulic": 0.5, "well_control": 0.0, "formation": 0.0}}}'
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `SAIREN_CONFIG` | *(none)* | Path to `well_config.toml` (overrides search) |
| `CAMPAIGN` | `production` | Campaign mode: `production` or `pa` |
| `STRATEGIC_MODEL_PATH` | GPU: `models/qwen2.5-7b-instruct-q4_k_m.gguf`, CPU: `models/qwen2.5-4b-instruct-q4_k_m.gguf` | Strategic LLM model (auto-selected) |
| `TACTICAL_MODEL_PATH` | `models/qwen2.5-1.5b-instruct-q4_k_m.gguf` | Only with `tactical_llm` feature |
| `SAIREN_KB` | *(none)* | Root directory of the structured knowledge base |
| `SAIREN_KB_FIELD` | *(none)* | Field name for knowledge base assembly |
| `SAIREN_KB_WELL` | `unknown` | Well name override for knowledge base (defaults to well config) |
| `SAIREN_KB_MAX_SNAPSHOTS` | `168` | Max hot mid-well snapshots before compression |
| `SAIREN_KB_RETENTION_DAYS` | `30` | Days to retain compressed snapshots before deletion |
| `RESET_DB` | *(none)* | Set to `true` to wipe all persistent data on startup |
| `SAIREN_SERVER_ADDR` | `0.0.0.0:8080` | HTTP server bind address |
| `RUST_LOG` | `info` | Log level: `debug`, `info`, `warn`, `error` |
| `ML_INTERVAL_SECS` | `3600` | ML analysis interval (seconds) |
| `WELL_ID` | `WELL-001` | Well identifier for ML storage |
| `FIELD_NAME` | `DEFAULT` | Field/asset name |

### CLI Arguments

| Argument | Description |
|----------|-------------|
| `--wits-tcp <host:port>` | Connect to WITS Level 0 TCP server |
| `--stdin` | Read WITS JSON packets from stdin |
| `--csv <path>` | Replay WITS data from CSV file |
| `--addr <host:port>` | Override HTTP server address |
| `--speed <N>` | Simulation speed multiplier (default: 1) |
| `--reset-db` | Wipe all persistent data on startup |
| `migrate-kb --from <path> --to <path>` | Migrate a flat `well_prognosis.toml` into the KB directory structure |

---

## Deployment

### Production Deployment (systemd)

SAIREN-OS ships with a systemd service unit and install script for rig-edge deployment.

```bash
# 1. Build the release binary
cargo build --release --features llm

# 2. Run the installer (as root)
sudo deploy/install.sh
```

This creates:

| Path | Purpose |
|------|---------|
| `/opt/sairen-os/bin/sairen-os` | Binary |
| `/opt/sairen-os/data/` | Persistent state (baseline, ML insights, databases) |
| `/etc/sairen-os/well_config.toml` | Well configuration (edit before starting) |
| `/etc/sairen-os/env` | Environment overrides |

```bash
# Edit well config for this well
sudo vi /etc/sairen-os/well_config.toml

# Edit env (WITS host, log level, etc.)
sudo vi /etc/sairen-os/env

# Enable and start
sudo systemctl enable sairen-os
sudo systemctl start sairen-os

# Monitor
sudo journalctl -u sairen-os -f
```

**Security hardening** — the service runs as a dedicated `sairen` user with:
- `NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome=yes`, `PrivateTmp=yes`
- Read-write access only to `/opt/sairen-os/data` and `/var/log/sairen-os`
- Automatic restart on failure (5s delay, max 5 retries per 5 minutes)

### Fleet Hub Deployment

The Fleet Hub runs as a separate binary on a central server with PostgreSQL.

```bash
# 1. Build the release binary
cargo build --release --bin fleet-hub --features fleet-hub

# 2. Run the installer (as root, on the hub server)
sudo deploy/install_hub.sh
```

This creates:

| Path | Purpose |
|------|---------|
| `/usr/local/bin/fleet-hub` | Hub binary |
| `/etc/systemd/system/fleet-hub.service` | systemd service unit |
| PostgreSQL `sairen_fleet` database | Event store and episode library |

```bash
# Monitor the hub
sudo journalctl -u fleet-hub -f

# View dashboard
open http://hub-ip:8080/
```

WireGuard configuration templates for hub and rig VPN tunnels are in `deploy/wireguard/`.

### Baseline Persistence

Baseline learning state (locked thresholds) is automatically saved to `data/baseline_state.json` after each metric locks. On restart, the system reloads locked thresholds so it doesn't need to re-learn from scratch. In-progress learning accumulators are intentionally not persisted — learning restarts cleanly.

---

## WITS Simulator

The included `wits_simulator.py` generates realistic WITS Level 0 data with interactive fault injection.

### Basic Usage

```bash
# Default (port 5000, production mode)
python3 wits_simulator.py

# Custom port
python3 wits_simulator.py --port 9100

# P&A campaign mode
python3 wits_simulator.py --campaign pa

# Faster simulation (10x speed)
python3 wits_simulator.py --interval 0.1
```

### P&A Operation Modes

```bash
# Milling simulation (high torque, low ROP)
python3 wits_simulator.py --campaign pa --operation milling

# Cement drill-out simulation (high WOB)
python3 wits_simulator.py --campaign pa --operation cement-drillout
```

| Mode | Simulated Parameters |
|------|---------------------|
| **Normal** | Standard drilling physics |
| **Milling** | Torque: 18-35 kN.m, ROP: 0.3-1.5 m/hr |
| **Cement Drill-Out** | WOB: 70-140 kN, Torque: 15-25 kN.m, ROP: 2-5 m/hr |

### Interactive Keyboard Controls

| Key | Action |
|-----|--------|
| `D` | Start/resume drilling |
| `T` | Trip out |
| `I` | Trip in (run in hole) |
| `K` | Inject kick (well control event) |
| `S` | Inject stick-slip vibration |
| `P` | Inject pack-off |
| `W` | Inject washout |
| `H` | Inject hard stringer |
| `L` | Inject lost circulation |
| `M` | Toggle milling mode (P&A) |
| `O` | Toggle cement drill-out mode (P&A) |
| `C` | Clear all faults |
| `Q` | Quit |

### P&A Simulation States

When running in P&A mode, the simulator cycles through:
1. **Circulating** - Initial circulation
2. **Displacing** - Displacing wellbore fluids
3. **Cementing** - Pumping cement
4. **Setting Plug** - Waiting for cement to set
5. **Pressure Testing** - Testing barrier integrity

---

## API Reference

Base URL: `http://localhost:8080`

### Core Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/v1/health` | GET | System health status |
| `/api/v1/status` | GET | System status, metrics, current operation |
| `/api/v1/spectrum` | GET | FFT spectrum data for visualization |
| `/api/v1/ttf` | GET | Time-to-failure estimates |
| `/api/v1/drilling` | GET | Current drilling metrics |
| `/api/v1/history` | GET | Recent advisory history (last 50) |
| `/api/v1/verification` | GET | Latest ticket verification result |
| `/api/v1/diagnosis` | GET | Current strategic advisory (204 if none) |
| `/api/v1/baseline` | GET | Baseline learning status |
| `/api/v1/campaign` | GET | Current campaign and thresholds |
| `/api/v1/campaign` | POST | Switch campaign |

### Configuration Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/v1/config` | GET | Current well configuration (all thresholds) |
| `/api/v1/config` | POST | Update configuration (validates, saves to file) |
| `/api/v1/config/validate` | POST | Validate config without applying |

### Advisory & Shift Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/v1/advisory/acknowledge` | POST | Acknowledge an advisory (audit trail) |
| `/api/v1/advisory/acknowledgments` | GET | List all advisory acknowledgments |
| `/api/v1/shift/summary` | GET | Shift summary with `?hours=12` or `?from=&to=` |
| `/api/v1/reports/critical` | GET | Critical advisory reports |
| `/api/v1/reports/test` | POST | Create a test critical report (for UI testing) |

### ML Engine Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/v1/ml/latest` | GET | Latest ML insights report |
| `/api/v1/ml/history?hours=N` | GET | ML analysis history |
| `/api/v1/ml/optimal?depth=N` | GET | Optimal parameters for depth |

### Strategic Report Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/v1/strategic/hourly` | GET | Hourly strategic reports |
| `/api/v1/strategic/daily` | GET | Daily strategic reports |
| `/api/v1/report/:timestamp` | GET | Specific report by timestamp |

### Fleet Hub Endpoints

Base URL: `http://hub:8080` (requires `fleet-hub` feature)

**Event Ingestion** (authenticated with rig API key):

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/fleet/events` | POST | Upload a fleet event (supports zstd compression) |
| `/api/fleet/events/{id}` | GET | Retrieve an event by ID |
| `/api/fleet/events/{id}/outcome` | PATCH | Update event outcome (Resolved/Escalated/FalsePositive) |

**Library Sync** (authenticated with rig API key):

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/fleet/library` | GET | Sync library (delta via `If-Modified-Since` header, supports zstd) |
| `/api/fleet/library/stats` | GET | Library statistics (category/outcome breakdown) |

**Rig Registry** (authenticated with admin API key):

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/fleet/rigs/register` | POST | Register a new rig (returns one-time API key) |
| `/api/fleet/rigs` | GET | List all registered rigs |
| `/api/fleet/rigs/{id}` | GET | Get rig details |
| `/api/fleet/rigs/{id}/revoke` | POST | Revoke a rig's API key |

**Performance Data** (authenticated with rig API key):

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/fleet/performance` | POST | Upload post-well performance data (supports zstd compression) |
| `/api/fleet/performance` | GET | Query performance data by field (`?field=&since=&exclude_rig=`) |

**Dashboard** (authenticated with admin API key):

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/fleet/dashboard/summary` | GET | Fleet overview (active rigs, events, episodes) |
| `/api/fleet/dashboard/trends` | GET | Event trends over time (`?days=30`) |
| `/api/fleet/dashboard/outcomes` | GET | Outcome analytics (resolution rates by category) |
| `/api/fleet/health` | GET | Hub health check (DB connectivity, library version) |
| `/` | GET | Fleet dashboard HTML page |

### Example: Switch Campaign

```bash
curl -X POST http://localhost:8080/api/v1/campaign \
  -H "Content-Type: application/json" \
  -d '{"campaign":"PlugAbandonment"}'
```

### Example: Acknowledge Advisory

```bash
curl -X POST http://localhost:8080/api/v1/advisory/acknowledge \
  -H "Content-Type: application/json" \
  -d '{"advisory_id": "ADV-042", "acknowledged_by": "J. Smith", "action_taken": "Reduced WOB to 25 klbs per recommendation"}'
```

### Example: Shift Summary

```bash
# Last 12 hours (default)
curl http://localhost:8080/api/v1/shift/summary

# Custom time range
curl "http://localhost:8080/api/v1/shift/summary?hours=8"
```

---

## Understanding Advisories

### Risk Levels

| Level | Efficiency | Typical Trigger | Response |
|-------|------------|-----------------|----------|
| **LOW** | >85% | Minor deviation | Continue monitoring |
| **ELEVATED** | 70-85% | MSE inefficiency | Consider adjustment |
| **HIGH** | 50-70% | Sustained issue | Act within 30 minutes |
| **CRITICAL** | <50% | Well control event | **Stop drilling immediately** |

### Example Advisory

```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
🎯 ADVISORY #12: ELEVATED | Efficiency: 68%

   Type: OPTIMIZATION | Category: Drilling Efficiency

   Recommendation: Consider adjusting WOB/RPM to improve MSE.
                   Current efficiency: 68%. Target MSE: 35,000 psi.

   Expected Benefit: Potential 10-20% ROP improvement, reduced bit wear

   MSE Specialist (25%): MEDIUM - MSE 52,000 psi exceeds optimal by 48%
   Hydraulic (25%): LOW - Flow balance normal
   WellControl (30%): LOW - No kick/loss indicators
   Formation (20%): LOW - D-exponent stable
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

### Decision Flowchart

```
Advisory Received
      │
      ▼
Is category WELL CONTROL? ──YES──► Execute well control procedures
      │
      NO
      ▼
Is risk level CRITICAL? ──YES──► Stop drilling, investigate immediately
      │
      NO
      ▼
Is category MECHANICAL? ──YES──► Check for pack-off/stick-slip
      │
      NO
      ▼
Is category EFFICIENCY? ──YES──► Optimize WOB/RPM per recommendation
      │
      NO
      ▼
Continue monitoring
```

---

## Thresholds Reference

### MSE Efficiency

| Efficiency | Status | Action |
|------------|--------|--------|
| > 85% | Optimal | Continue current parameters |
| 70-85% | Acceptable | Monitor, minor adjustments |
| 50-70% | Poor | Optimize WOB/RPM |
| < 50% | Very Poor | Immediate parameter review |

### Well Control (Safety-Critical)

| Parameter | Warning | Critical |
|-----------|---------|----------|
| Flow Imbalance | > 10 gpm | > 20 gpm |
| Pit Gain | > 5 bbl | > 10 bbl |
| Pit Rate | > 5 bbl/hr | > 15 bbl/hr |
| Gas Units | > 100 | > 500 |
| H2S | > 10 ppm | > 20 ppm |

### Hydraulics

| Parameter | Warning | Critical |
|-----------|---------|----------|
| ECD Margin | < 0.3 ppg | < 0.1 ppg |
| SPP Deviation | > 100 psi | > 200 psi |

### Mechanical

| Parameter | Warning | Critical |
|-----------|---------|----------|
| Torque Increase | > 15% | > 25% |
| Combined Torque + SPP | Both rising | Sustained trend |
| Founder Condition | WOB +5%, ROP flat | WOB +5%, ROP decreasing |

### Founder Detection

Founder occurs when WOB exceeds the optimal point and ROP stops responding or decreases despite increasing weight. This indicates bit balling, cuttings accumulation, or reaching the formation's founder point.

| Severity | ROP Response | Action |
|----------|--------------|--------|
| Low (30%) | ROP flat despite WOB increase | Monitor, consider reducing WOB |
| Medium (50%) | ROP slightly decreasing | Reduce WOB to optimal point |
| High (70%+) | ROP actively decreasing | Reduce WOB immediately |

The system estimates the optimal WOB (where ROP was highest) and provides specific recommendations.

---

## Troubleshooting

### No advisories being generated

1. **Still in baseline learning** - Wait ~2 minutes for 100 samples. Check: `curl http://localhost:8080/api/v1/baseline`
2. **Drilling conditions are good** - No advisories = optimal operations
3. **Not in drilling state** - Advisories only during Drilling/Reaming
4. **Test with fault injection** - Press `K`, `S`, or `P` in simulator

### Model not found error

Download the strategic model and place in `models/` directory, or set the environment variable:
```bash
export STRATEGIC_MODEL_PATH=/path/to/strategic-model.gguf
```

### LLM inference too slow

1. Check what mode SAIREN-OS detected at startup (look for "Hardware:" in logs)
2. If on CPU, this is expected — CPU inference targets ~10-30s for the strategic model
3. For faster inference, build with `--features cuda` and ensure CUDA is available: `nvidia-smi`
4. Use quantized models (Q4_K_M recommended)
5. System works without LLM - falls back to templates
6. Tactical routing is always fast (~0ms) — it uses deterministic pattern matching, not LLM

### Another instance already running

```bash
# Remove stale lock file
rm ./data/.sairen.lock
```

### Port already in use

```bash
fuser -k 8080/tcp
# Or use different port:
./target/release/sairen-os --wits-tcp localhost:5000 --addr 0.0.0.0:8081
```

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
    optimization.rs    # OptimizationAdvisory, ConfidenceBreakdown
    tactical.rs        # TacticalAnalysis types
    thresholds.rs      # AnomalyCategory, FinalSeverity, RiskLevel

  bin/
    simulation.rs      # WITS Level 0 data simulator for testing
    fleet_hub.rs       # Fleet Hub server binary (fleet-hub feature)
    volve_replay.rs    # Volve field data replay with ACI + CfC shadow logging
    witsml_to_csv.rs   # WITSML XML to CSV converter (Rust binary)

  config/
    mod.rs             # OnceLock global config access (init/get/is_initialized)
    well_config.rs     # WellConfig TOML loader, all threshold structs, validation
    formation.rs       # FormationPrognosis loader (SAIREN_PROGNOSIS env var)

  causal/
    mod.rs             # Causal inference: Granger-style cross-correlation over 60-packet history;
                       # detect_leads() → Vec<CausalLead>; pearson_r() pure-std, no deps, < 1 ms

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
    templates.rs       # Campaign-aware template fallback per AnomalyCategory; appends causal leads
    aggregation.rs     # Report aggregation helpers
    parsing.rs         # Structured output parsing
    actor.rs           # Strategic actor

  pipeline/
    coordinator.rs     # 11-phase pipeline coordinator (uses KnowledgeStore + AdvisoryComposer)
    processor.rs       # AppState, system status

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
    regime_clusterer.rs # K-means clustering of 8 motor outputs → regime_id (0–3); stamps packet
    formation_detector.rs # CfC-based formation boundary detection from motor output patterns

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

  knowledge_base/        # Structured per-well knowledge base (knowledge-base feature)
    mod.rs             # KnowledgeBase struct, init, hot-reload prognosis
    assembler.rs       # Merge geology + pre-spud + offset wells → FormationPrognosis
    compressor.rs      # Transparent zstd read/write for .toml and .toml.zst
    layout.rs          # Directory path helpers, file enumeration
    mid_well.rs        # ML snapshot writer + cap enforcement (compress old, delete expired)
    post_well.rs       # Post-well summary generator from mid-well snapshots
    watcher.rs         # Polling directory watcher, triggers reassembly on changes
    fleet_bridge.rs    # Upload post-well data to hub, download offset data from hub
    migration.rs       # Flat well_prognosis.toml → KB directory migration

  fleet/
    mod.rs             # Fleet hub-and-spoke module
    types.rs           # FleetEvent, FleetEpisode, EventOutcome, HistorySnapshot
    queue.rs           # UploadQueue: disk-backed durable queue for fleet uploads
    client.rs          # FleetClient: HTTP client for hub communication (fleet-client)
    uploader.rs        # Background upload task draining queue to hub (fleet-client)
    sync.rs            # Periodic library sync from hub with jitter (fleet-client)

  hub/                   # Fleet Hub server (fleet-hub feature)
    mod.rs             # HubState, module exports
    config.rs          # HubConfig from env vars and CLI args
    db.rs              # PostgreSQL pool and migration runner
    api/
      mod.rs           # Router builder with all fleet routes
      events.rs        # Event ingestion (POST), retrieval (GET), outcome update (PATCH)
      library.rs       # Library sync with delta support and zstd compression
      performance.rs   # Post-well performance data upload and query endpoints
      registry.rs      # Rig registration, listing, revocation
      dashboard.rs     # Fleet dashboard API endpoints and HTML serving
      health.rs        # Health check endpoint
    auth/
      mod.rs           # Auth module exports
      api_key.rs       # API key generation, bcrypt hashing, RigAuth/AdminAuth extractors
    curator/
      mod.rs           # Curation background task runner
      scoring.rs       # Episode scoring (outcome, recency, detail, diversity)
      dedup.rs         # Episode deduplication (rig + category + depth + time)
      pruning.rs       # Archival rules (age, false positive, capacity)

  background/
    mod.rs             # Background services module
    self_healer.rs     # HealthCheck trait + SelfHealer + WITS/Disk health checks

  storage/
    mod.rs             # Storage module
    persistence.rs     # PersistenceLayer trait + InMemoryDAL
    history.rs         # Advisory history storage
    strategic.rs       # Strategic report storage
    lockfile.rs        # Process lock file management

  acquisition/
    wits_parser.rs     # WITS Level 0 TCP with reconnection, timeouts,
                       # and data quality validation
    sensors.rs         # Sensor abstractions
    stdin_source.rs    # Stdin data source

  llm/
    strategic_llm.rs   # Qwen 2.5 7B (GPU) / 4B (CPU) advisory generation
    tactical_llm.rs    # Legacy 1.5B classification (behind `tactical_llm` feature)
    mistral_rs.rs      # Backend with runtime CUDA detection
    scheduler.rs       # LLM scheduling

  api/
    routes.rs          # HTTP route definitions
    handlers.rs        # Request handlers (config, advisory ack, shift summary)

  processing/
    fft.rs             # FFT spectrum analysis
    health_scoring.rs  # Health score calculations

  director/
    llm_director.rs    # LLM orchestration director

static/
  index.html           # Rig dashboard UI
  reports.html         # Strategic reports viewer
  fleet_dashboard.html # Fleet Hub dashboard UI (Chart.js visualizations)

migrations/
  001_initial_schema.sql  # PostgreSQL schema (rigs, events, episodes, sync_log)
  002_fleet_performance.sql # Fleet performance table for offset well sharing

deploy/
  sairen-os.service    # systemd service unit for rig (hardened)
  install.sh           # Rig production install script
  fleet-hub.service    # systemd service unit for Fleet Hub
  install_hub.sh       # Fleet Hub install script (PostgreSQL + binary + systemd)
  wireguard/
    hub_wg0.conf.template   # WireGuard config template for hub server
    rig_wg0.conf.template   # WireGuard config template for rig

tests/
  fleet_integration.rs           # Fleet Hub integration tests (11 tests)
  knowledge_base_integration.rs  # KB lifecycle tests (migrate, assemble, offset wells)

scripts/
  witsml_to_csv.py       # WITSML 1.4.1 XML → Kaggle CSV converter (extracts from Volve zip)

well_config.default.toml  # Reference configuration with all thresholds documented
```

---

## Glossary

| Term | Description |
|------|-------------|
| **WITS** | Wellsite Information Transfer Specification - industry standard for real-time drilling data (40+ channels) |
| **ROP** | Rate of Penetration (ft/hr) - drilling speed |
| **WOB** | Weight on Bit (klbs) - downward force on drill bit |
| **RPM** | Rotations Per Minute - drill string rotation speed |
| **MSE** | Mechanical Specific Energy (psi) - energy to remove rock; lower = more efficient |
| **D-exponent** | Normalized parameter tracking formation changes; rising may indicate pore pressure increase |
| **ECD** | Equivalent Circulating Density (ppg) - effective mud weight including friction |
| **SPP** | Standpipe Pressure (psi) - pump pressure at surface |
| **Kick** | Uncontrolled influx of formation fluids - CRITICAL safety event |
| **Lost Circulation** | Mud loss into formation |
| **Pack-off** | Restriction from cuttings buildup; signs: rising torque + SPP |
| **Stick-slip** | Torsional oscillation; torque fluctuates cyclically |
| **Founder** | Condition where WOB exceeds optimal and ROP stops responding; caused by bit balling or cuttings accumulation |
| **Founder Point** | The WOB at which ROP peaks; beyond this point, additional weight reduces efficiency |
| **Flow Balance** | flow_out - flow_in (gpm); positive = potential kick |
| **Pit Volume** | Mud volume in surface pits (bbl) |
| **Rig State** | Operational mode: Drilling, Reaming, Circulating, Connection, TrippingIn, TrippingOut, Idle |
| **Operation** | Activity classification: Production Drilling, Milling, Cement Drill-Out, Circulating, Static |
| **Milling** | P&A operation: cutting through casing; high torque, very low ROP |
| **Cement Drill-Out** | P&A operation: drilling cement plugs; high WOB, moderate torque, low ROP |
| **Fleet Hub** | Central server that collects events from all rigs and curates a shared episode library |
| **Spoke** | Individual rig running SAIREN-OS, uploading events to and syncing from the hub |
| **FleetEvent** | An AMBER/RED advisory with history window and outcome, uploaded to the hub |
| **FleetEpisode** | Compact precedent extracted from a FleetEvent, scored and stored in the library |
| **Curator** | Background process on the hub that scores, deduplicates, and prunes episodes |
| **RAMRecall** | In-memory episode search on each rig, populated by library syncs from the hub |
| **CfC** | Closed-form Continuous-time neural network; RNN variant with time-gated updates that naturally handles irregular time steps |
| **NCP** | Neural Circuit Policy; sparse wiring topology inspired by biological neural circuits (sensory → inter → command → motor) |
| **BPTT** | Backpropagation Through Time; training RNNs by unrolling through multiple timesteps (depth=4 in SAIREN-OS) |
| **Adam** | Adaptive Moment Estimation optimizer; maintains per-parameter learning rates via first/second moment tracking |
| **Feature Surprise** | Per-feature anomaly decomposition from CfC; reports which sensors deviated most from prediction (e.g., "SPP ↑2.36σ") |
| **Tiebreaker** | CfC resolves Uncertain strategic verifications: score ≥ 0.7 → Confirmed, < 0.2 → Rejected |
| **WITSML** | Wellsite Information Transfer Standard Markup Language; XML-based format for well data exchange (1.4.1 series supported) |
| **ACI** | Adaptive Conformal Inference; online conformal intervals for distribution-free anomaly detection |
| **Knowledge Base** | Structured per-well directory of geology, engineering, and performance files that replaces the flat `well_prognosis.toml` |
| **Offset Well** | A previously drilled well in the same field whose performance data informs drilling parameters for the current well |
| **Pre-Spud Prognosis** | Well-specific engineering plan authored before drilling begins, setting parameter ranges and casing points |
| **Mid-Well Snapshot** | Hourly ML performance snapshot written during drilling, capturing optimal parameters and confidence |
| **Post-Well Summary** | Aggregated performance data generated after well completion, shared across the fleet as offset data |
| **CausalLead** | A detected leading indicator: a drilling parameter whose change precedes an MSE shift by `lag_seconds`, quantified by Pearson r and labeled with a direction ("increase" or "decrease") |
| **Regime ID** | 0–3 integer stamped on each packet by the CfC k-means clusterer based on motor neuron output patterns; 0=baseline, 1=hydraulic-stress, 2=high-wob, 3=unstable |
| **RegimeProfile** | Multiplicative weight adjustment table per drilling regime; applied to `ensemble_weights` before orchestrator voting, then re-normalised to 1.0 |
| **Granger Causality** | Statistical test for whether one time series improves prediction of another; approximated here via Pearson cross-correlation at multiple lags |

---

## Changelog

### v3.0 - Causal Inference & Regime-Aware Intelligence

**Phase 5: Causal Inference on Edge** (`src/causal/mod.rs`):

- **`detect_leads(history)`** — scans the 60-packet history buffer for drilling parameters that causally precede MSE spikes using Pearson cross-correlation at lags 1–20 s; threshold |r| ≥ 0.45; returns up to 3 `CausalLead` results sorted by |r| descending
- **Pure-std implementation** — no external crates; Pearson r computed in a single O(n) pass over mean-centred values; < 1 ms per packet on 60-sample buffers; minimum 20 packets before analysis runs
- **`CausalLead` type** (`src/types/ticket.rs`) — `{ parameter, lag_seconds, pearson_r, direction }` — attached to every `AdvisoryTicket` as `causal_leads: Vec<CausalLead>`; skipped in JSON serialization when empty
- **Pipeline integration** (`pipeline/coordinator.rs`) — Phase 4.5 block runs causal detection immediately after the history buffer and before advanced physics verification, in both the per-packet and periodic summary paths
- **Advisory surfacing** (`strategic/templates.rs`) — `format_causal_block()` appends leading-indicator context to template advisory reasoning when leads are present: *"Causal leads: increase WOB precedes MSE by 12s (r=+0.73); decrease SPP precedes MSE by 4s (r=−0.61)."*
- **7 unit tests** — perfect/anti correlation, constant-series zero guard, insufficient-history early return, synthetic 60-entry WOB→MSE lead, max-3-leads cap

**Phase 6: Regime-Aware Orchestrator Weighting** (`src/agents/orchestrator.rs`):

- **`RegimeProfile`** — struct with four per-specialist multiplicative weight adjustments; `&'static str label` allows `const` array definition with no heap allocation
- **`REGIME_PROFILES: [RegimeProfile; 4]`** — static table: baseline (0, all ×1.0), hydraulic-stress (1, Hydraulic ×1.4), high-wob (2, MSE ×1.4), unstable (3, WellControl ×1.5)
- **`apply_regime_weights(votes, regime_id)`** — multiplies each `SpecialistVote.weight` by the regime multiplier then re-normalises so the total always sums to 1.0; out-of-range `regime_id` clamps to regime 3
- **`vote()` signature extended** — `regime_id: u8` parameter flows from `packet.regime_id` (stamped by the CfC k-means clusterer in Phase 2.8) through the coordinator to the orchestrator in both process_packet and generate_periodic_summary paths
- **Advisory reasoning** — includes active regime label, e.g., `[regime 2:high-wob]`; WellControl CRITICAL severity override applied after re-normalisation and unaffected by regime weighting
- **7 new regime tests** — all 4 regimes sum to 1.0, each regime elevates the expected specialist, reasoning includes regime label, out-of-range clamp verified; **260 total unit tests passing**

**CfC extensions** (supporting phases 5–6):

- **`src/cfc/regime_clusterer.rs`** — k-means clustering of the 8 CfC motor neuron outputs into 4 regime labels (0–3); runs each packet, writes `regime_id` onto `WitsPacket` before it enters the pipeline
- **`src/cfc/formation_detector.rs`** — CfC motor-output pattern analysis for formation boundary detection; supplements the d-exponent shift detector in the ML engine

### v2.2 - Structured Knowledge Base

**Per-well knowledge base** (`src/knowledge_base/`, `src/types/knowledge_base.rs`):
- **Directory-based KB** replaces flat `well_prognosis.toml` — separates geology, pre-spud engineering, mid-well ML snapshots, and post-well performance into a structured file tree per well per field
- **Assembler** (`assembler.rs`) — merges field geology + well-specific pre-spud + N offset wells into a `FormationPrognosis` at runtime; geologist sets safety envelope, offset wells set target within it; default parameter derivation from hardness (soft/medium/hard)
- **Mid-well snapshots** (`mid_well.rs`) — writes hourly ML performance snapshots during drilling; enforces cap (168 hot TOML files, then compress with zstd, then delete beyond 30-day retention)
- **Post-well generator** (`post_well.rs`) — aggregates all mid-well snapshots into per-formation `PostWellFormationPerformance` files on well completion; compresses mid-well and pre-spud to cold storage
- **Directory watcher** (`watcher.rs`) — polling-based (30s interval) background task detects new/modified files and hot-reloads the assembled prognosis via `Arc<RwLock>`
- **Transparent compression** (`compressor.rs`) — reads both `.toml` and `.toml.zst` transparently; zstd level 3 matching fleet convention
- **Layout helpers** (`layout.rs`) — path construction, directory creation, sibling well enumeration
- **Legacy fallback** — when `SAIREN_KB` env var is not set, falls back to `FormationPrognosis::load()` from flat TOML

**Fleet performance sharing** (`knowledge_base/fleet_bridge.rs`, `hub/api/performance.rs`):
- **Upload** — `POST /api/fleet/performance` receives zstd-compressed post-well performance data, upserts into `fleet_performance` PostgreSQL table with `UNIQUE(well_id, formation_name)` constraint
- **Download** — `GET /api/fleet/performance?field=&since=&exclude_rig=` returns all performance records for a field; spoke writes received files into KB directory, watcher detects and triggers reassembly
- **Fleet bridge** — `upload_post_well()` sends all per-formation files after well completion; `sync_performance()` pulls offset data during fleet sync loop
- **Migration** (`migrations/002_fleet_performance.sql`) — new `fleet_performance` table with indexes on field, rig_id, and updated_at

**Migration tool** (`knowledge_base/migration.rs`):
- `sairen-os migrate-kb --from well_prognosis.toml --to ./knowledge-base/` — splits flat prognosis into geology, pre-spud engineering, and per-offset-well performance files
- Verified round-trip: migrate Volve data → reassemble → all 5 formations match original

**Pipeline integration** (`pipeline/coordinator.rs`, `main.rs`):
- `PipelineCoordinator` gains optional `KnowledgeBase` field; all 4 constructors attempt KB init before falling back to flat prognosis
- `process_packet()` reads dynamic prognosis from KB when available
- ML scheduler writes mid-well snapshots after each successful analysis
- KB watcher starts automatically on pipeline init

**New feature flag**: `knowledge-base` (enabled by default, implies `zstd`)

**Tests**: 17 new unit tests across all KB modules + 3 integration tests (migrate-and-assemble, offset-well assembly, full lifecycle)

### v2.1 - CfC Active Integration & WITSML Support

**CfC moves from shadow mode to active pipeline participation:**

- **Severity modulation** (`src/agents/tactical.rs`) — `cfc_adjust_severity()` mirrors ACI pattern: score < 0.3 → downgrade one level, 0.3-0.7 → no change, ≥ 0.7 → escalate one level; WellControl never downgraded below High
- **CfC fields on AdvisoryTicket** (`src/types/ticket.rs`) — `CfcFeatureSurpriseInfo` struct (name, error, magnitude), `cfc_anomaly_score: Option<f64>` and `cfc_feature_surprises: Vec<CfcFeatureSurpriseInfo>` on every ticket
- **Strategic LLM context** (`src/llm/strategic_llm.rs`) — CfC section injected into advisory prompt with anomaly score, health score, and top 5 surprised features with direction and magnitude
- **Strategic tiebreaker** (`src/agents/strategic.rs`) — `cfc_tiebreak()` resolves Uncertain verifications across all 5 category verifiers: score ≥ 0.7 → Confirmed (CfC corroborates), score < 0.2 → Rejected (CfC sees nothing)
- **Trace logging** — CfC data logged at both tactical creation and strategic verification stages

**WITSML 1.4.1 extraction tooling:**

- **`scripts/witsml_to_csv.py`** — extracts time-indexed WITSML XML logs from Volve zip archive into Kaggle-format CSV; maps 40+ WITSML mnemonics to standard column names; merges multiple log segments by timestamp
- Successfully extracted F-12 well: 2,542,561 rows with good parameter coverage (WOB=1.68M, RPM=1.5M, ROP=1.49M, SPP=1.79M)

**Validation across 3 wells:**

- F-5: 181K packets, 144 tickets, 97% confirmation rate, 11 CfC tiebreaker corroborations
- F-9A: 88K packets, 3 tickets all correctly rejected, avg loss 0.702
- F-12 (unseen): 2.4M packets, 222 tickets, CfC calibrated online at packet 65K, active corroboration on hydraulic anomalies at depth

### v2.0 - CfC Neural Network Operations Specialist

**CfC/NCP Neural Network** (`src/cfc/`):
- **128-neuron CfC network** with NCP sparse wiring (~30% connectivity, ~1,833 connections) — pure Rust, no ML framework dependencies
- **Self-supervised online learning** — predicts next-timestep sensor values, treats prediction error as anomaly signal; no labeled data needed
- **16 input features** — primary (WOB, ROP, RPM, torque, MSE, SPP, d-exponent, hookload) weighted 2x in loss; supplementary (ECD, flow balance, pit rate, DXC, pump SPM, mud weight, gas, pit volume)
- **NCP architecture** — 24 sensory neurons (variable mapping: 2 per primary feature, 1 per supplementary), 64 inter, 32 command, 8 motor; ~6,051 trainable parameters
- **Adam optimizer** with decaying base LR (0.001 → floor 0.0001), beta1=0.9, beta2=0.999 — 64% lower loss vs SGD baseline
- **Truncated BPTT (depth=4)** — backprop through 4 cached timesteps with 0.7^k gradient decay per step
- **Gradient norm clipping** (max norm=5.0) — preserves gradient direction while preventing explosion; replaces per-element hard clipping
- **Feature-weighted MSE loss** — primary drilling features weighted 2x to focus learning on the signals that matter for anomaly detection
- **Adaptive anomaly scoring** — EMA of RMSE → z-score → sigmoid(z-2) → 0-1 score; calibrates after 500 packets
- **Welford's online normalization** — numerically stable incremental mean/variance per feature, no historical data storage
- **Initial shadow mode integration** — CfC results logged alongside tickets (Phase 2.8 in tactical agent); promoted to active in v2.1
- **Volve F-9A validation** — avg loss 0.70, correctly flags confirmed SPP deviation (anomaly=0.93), low false positives on rejected tickets

### v1.1 - Fleet Hub Implementation

**Fleet Hub Server** (`src/hub/`, `src/bin/fleet_hub.rs`):
- **Axum HTTP server** with PostgreSQL backend (sqlx) — standalone `fleet-hub` binary behind `fleet-hub` feature flag
- **Event ingestion** (`hub/api/events.rs`) — POST endpoint with zstd decompression, validation (risk level, timestamp range, history window), dedup by event ID, rig_id/auth cross-check
- **Library curator** (`hub/curator/`) — hourly background task: episode scoring (outcome 50%, recency 25%, detail 15%, diversity 10%), deduplication (rig + category + depth + time window), pruning (age limit, false positive cleanup, capacity cap)
- **Library sync** (`hub/api/library.rs`) — delta sync via `If-Modified-Since`, zstd-compressed responses, version tracking via PostgreSQL sequence, excludes requesting rig's own episodes
- **Rig registry** (`hub/api/registry.rs`) — admin-only registration returning one-time API key, bcrypt-hashed storage, revocation support
- **Auth middleware** (`hub/auth/api_key.rs`) — `RigAuth` and `AdminAuth` extractors with 5-minute verification cache, Bearer token authentication
- **Dashboard API** (`hub/api/dashboard.rs`) — summary, trends, outcome analytics endpoints + embedded HTML dashboard with Chart.js visualizations
- **Health endpoint** (`hub/api/health.rs`) — DB connectivity, library version

**Spoke-Side Clients** (`src/fleet/client.rs`, `uploader.rs`, `sync.rs`):
- **FleetClient** — HTTP client with zstd-compressed uploads, outcome forwarding (PATCH), delta library sync
- **Uploader** — background task draining UploadQueue to hub with per-event retry
- **LibrarySync** — periodic library pull with configurable jitter to prevent thundering herd
- **RAMRecall.remove_episodes()** — pruned episode cleanup on sync

**Database** (`migrations/001_initial_schema.sql`):
- PostgreSQL schema: `rigs`, `events`, `episodes`, `sync_log` tables
- Indexes on rig_id, timestamp, needs_curation, category, score, updated_at
- Auto-updated `updated_at` triggers, `library_version_seq` sequence

**Infrastructure** (`deploy/`):
- WireGuard configuration templates for hub and rig VPN tunnels
- systemd service unit for Fleet Hub
- Install script with PostgreSQL setup, admin key generation, systemd integration

**Testing** (`tests/fleet_integration.rs`):
- 11 integration tests covering episode creation, event validation, serialization round-trips, API key hash/verify, config loading, RAMRecall operations

### v1.0 - Trait Architecture & Fleet Preparation

**Phase 1: Trait Formalization**
- **KnowledgeStore trait** (`context/knowledge_store.rs`) — swappable knowledge backends with `query()`, `store_name()`, `is_healthy()` methods; includes `StaticKnowledgeBase` (wraps existing vector_db) and `NoOpStore` for pilot mode
- **Specialist trait** (`agents/specialists/`) — extracted 4 domain specialists (MSE, Hydraulic, WellControl, Formation) from inline orchestrator methods into trait-based implementations with `default_specialists()` factory
- **VotingResult decoupling** — orchestrator now returns `VotingResult` (votes, severity, risk level, efficiency score) instead of directly composing advisories
- **AdvisoryComposer** (`strategic/advisory.rs`) — separate component that assembles `StrategicAdvisory` from `VotingResult` with 30-second CRITICAL cooldown to prevent alert spam

**Phase 2: Resilience Hardening**
- **Template fallback system** (`strategic/templates.rs`) — campaign-aware template advisories per `AnomalyCategory` with actual metric values; confidence 0.70, source "template"; P&A-specific notes for well control
- **Background self-healer** (`background/self_healer.rs`) — `HealthCheck` trait with `SelfHealer` running 30s check loop; `WitsHealthCheck` (30s packet timeout) and `DiskHealthCheck` (500MB free space warning via `libc::statvfs`)
- **PersistenceLayer trait** (`storage/persistence.rs`) — `InMemoryDAL` with configurable limits for advisories and ML reports; `PersistenceError` enum with Serialization, Storage, NotFound variants

**Phase 3: Fleet Preparation**
- **Fleet types** (`fleet/types.rs`) — `FleetEvent` (full advisory + history window + outcome), `FleetEpisode` (compact precedent with `from_event` constructor), `EventOutcome` (Pending/Resolved/Escalated/FalsePositive), `HistorySnapshot`, `should_upload()` filter (AMBER/RED only)
- **Upload queue** (`fleet/queue.rs`) — disk-backed durable queue using JSON files named by event ID for idempotent retry; survives process restarts; auto-evicts oldest when full (default 1000 events)
- **RAMRecall** (`context/ram_recall.rs`) — in-memory fleet episode search implementing `KnowledgeStore`; metadata-filtered linear scan with recency + outcome scoring; max 10,000 episodes (~50MB); keyword-based category parsing from query strings

### v0.9 - Pattern-Matched Tactical Routing

**Tactical LLM replaced with deterministic pattern matching** — the tactical agent now uses physics-based routing instead of an LLM for anomaly classification:
- `TicketContext` struct carries all threshold breaches, pattern name, rig state, operation, and campaign with every ticket
- `ThresholdBreach` struct records exact actual vs threshold values for every exceeded limit
- Pattern routing table maps anomaly categories to named patterns (Kick, Pack-off, MSE Inefficiency, etc.)
- Structured context templated directly into strategic LLM prompt (`### TACTICAL CONTEXT` section)
- Tactical LLM (Qwen 2.5 1.5B) gated behind `tactical_llm` feature flag — not loaded by default
- **Result**: Eliminates ~60ms (GPU) / ~2-5s (CPU) tactical LLM latency, reduces VRAM by ~1.5 GB

**Hardened float math:**
- NaN/Inf guards on all averaging operations, divisors, and critical calculations
- Division-by-zero protection for configurable divisors (MSE, formation hardness, severity)
- WITS parser rejects NaN/Inf from sensor data at ingestion
- Config validation sweeps for NaN/Inf via TOML serialization
- Poisoned RwLock recovery (`.unwrap_or_else(|e| e.into_inner())`) prevents cascading panics

### v0.8 - Production Hardening

**Well Configuration System** — every hardcoded threshold (43 total) replaced with a configurable TOML file:
- `well_config.toml` with 3-tier search (`$SAIREN_CONFIG` → `./well_config.toml` → defaults)
- Runtime config API (`GET/POST /api/v1/config`, `POST /api/v1/config/validate`)
- Validation on load: critical > warning consistency, weights sum check, sigma ordering
- `well_config.default.toml` reference with comprehensive operational documentation

**WITS Feed Resilience:**
- Read timeouts (120s default) prevent silent hangs
- TCP keepalive via `socket2` for stale connection detection
- Exponential backoff reconnection (2s → 60s cap, 10 attempts max)
- Per-packet data quality validation (all-zero detection, physically impossible values, consistency checks)

**Operational Features:**
- Advisory acknowledgment API with audit trail (`POST /api/v1/advisory/acknowledge`)
- Shift summary endpoint with time-range filtering (`GET /api/v1/shift/summary`)
- Baseline learning state persists across crashes (`data/baseline_state.json`)
- Critical reports endpoint (`GET /api/v1/reports/critical`)

**Deployment:**
- systemd service unit with security hardening (`deploy/sairen-os.service`)
- Production install script (`deploy/install.sh`)
- Dedicated `sairen` service user with minimal privileges

### v0.7 - ML Engine V2.2 (Dysfunction-Aware Optimization)
- **Dysfunction Filter**: New pipeline stage that rejects samples with:
  - Torque instability (stick-slip precursor, CV > 12%)
  - Pack-off signatures (torque + SPP both elevated)
  - Founder conditions (WOB up, ROP not responding)
  - Low MSE efficiency (< 50%)
- **Grid-Based Binning**: Replaced "top 10% averaging" with 8×6 WOB×RPM grid
  - Ensures recommended parameters were actually used together
  - Avoids mixing disjoint operating modes
- **Stability Penalty**: Campaign-aware composite scoring now includes stability
  - Production: 50% ROP + 30% MSE + 20% stability
  - P&A: 25% ROP + 45% MSE + 30% stability
- **Safe Operating Ranges**: Reports now include min/max for WOB, RPM, Flow
- **Relaxed Correlation Requirements**: Pipeline no longer fails if p > 0.05
  - Proceeds with optimization, flags as low confidence instead
- **Stability Metrics**: New fields in OptimalParams for stability tracking
  - `stability_score`, `bin_sample_count`, `bins_evaluated`, `dysfunction_filtered`

### v0.6 - Founder Detection & Simulator Enhancements
- **Founder detection**: Two-stage detection (tactical quick check + strategic trend analysis)
- Trend-based WOB/ROP analysis using linear regression over history buffer
- Optimal WOB estimation (identifies where ROP was highest)
- Severity classification: Low (30%), Medium (50%), High (70%+)
- Strategic agent verification with actionable recommendations
- Simulator physics improvements:
  - WOB now correctly zero when bit is off bottom
  - Founder point model in ROP calculation (ROP decreases past optimal WOB)
  - Trip In keyboard control (`I` key)

### v0.5 - Operation Classification
- Automatic operation detection based on drilling parameters
- P&A-specific operations: Milling, Cement Drill-Out
- Operation transition logging with parameter context
- Simulator `--operation` flag and keyboard controls (M/O)

### v0.4 - ML Engine V2.1
- Optimal drilling conditions analysis (WOB, RPM, flow)
- Campaign-aware optimization weights
- Pearson correlation with p-value significance testing
- Formation boundary detection via d-exponent shifts
- Configurable scheduler (`ML_INTERVAL_SECS`)

### v0.3 - Campaign System
- Production and P&A campaign modes
- Campaign-aware thresholds and LLM prompts
- Runtime switching via dashboard/API
- Simulator `--campaign` flag

### v0.2 - Stability Improvements
- Periodic 10-minute summaries
- Pit rate noise filtering
- ECD margin stability
- CRITICAL cooldown (30s)

---

## Performance

### GPU Mode (with CUDA)

| Metric | Target | Actual |
|--------|--------|--------|
| Tactical Physics + Routing | < 15ms | ~10ms |
| Strategic LLM | < 800ms | ~750ms |
| WITS Packet Rate | 1 Hz | 1 Hz |
| History Buffer | 60 packets | 60 |

### CPU Mode (no CUDA)

| Metric | Target | Actual |
|--------|--------|--------|
| Tactical Physics + Routing | < 15ms | ~10ms |
| Strategic LLM | < 30s | ~10-30s |
| WITS Packet Rate | 1 Hz | 1 Hz |
| History Buffer | 60 packets | 60 |

### Replay Throughput (Volve data, no LLM)

| Well | Packets | Drilling | Tickets | Pipeline |
|------|---------|----------|---------|----------|
| F-5 | 181,617 | 9,574 | 144 | Full (ACI + CfC + physics + voting) |
| F-9A | 87,876 | 5,284 | 3 | Full (ACI + CfC + physics + voting) |
| F-12 | 2,423,467 | 80,888 | 222 | Full (ACI + CfC + physics + voting) |

The full pipeline (ACI conformal intervals, CfC neural network online training, physics engine, tactical/strategic two-stage verification, specialist voting) processes 2.4M packets in a single pass with no batch preprocessing or cloud round-trips. All computation runs locally in a single Rust binary with no GPU dependency.

> **Note**: Tactical routing (pattern matching, ticket creation) is purely deterministic and
> runs at physics speed (~10ms) on all hardware. Only the strategic LLM advisory generation
> is affected by GPU/CPU selection.

---

## License

Proprietary - SAIREN-OS Team
