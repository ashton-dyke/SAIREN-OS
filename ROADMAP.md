# SAIREN-OS v4.0 Roadmap

> Feature roadmap for the next generation of SAIREN-OS.
> Reference this document as we build each feature.

---

## Status Overview

| # | Feature | Backend | API | Dashboard | Overall |
|---|---------|---------|-----|-----------|---------|
| 1 | Hot Config Reload | DONE | DONE | N/A | **DONE** |
| 2 | Operator Feedback Loop | DONE | DONE | DONE | **DONE** |
| 3 | Predictive Lookahead Advisor | DONE | DONE | DONE | **DONE** |
| 4 | Stick-Slip Active Damping | Iter 2 done | DONE | Not started | **Iter 2 done** |
| 5 | Federated CfC Weight Sharing | DONE | DONE | N/A | **DONE** |
| 6 | RigWatch v2 (CfC) | DONE | DONE | N/A | **DONE** |
| 7 | Post-Well AI Debrief | Partial | — | — | Early |
| 8 | Fleet Analytics Dashboard | Not started | — | — | Not started |
| 9 | Wellbore Stability Prediction | Not started | — | — | Not started |

---

## Table of Contents

1. [Hot Config Reload](#1-hot-config-reload)
2. [Operator Feedback Loop](#2-operator-feedback-loop)
3. [Predictive Lookahead Advisor](#3-predictive-lookahead-advisor)
4. [Stick-Slip Active Damping](#4-stick-slip-active-damping)
5. [Federated CfC Weight Sharing](#5-federated-cfc-weight-sharing)
6. [Predictive Equipment Maintenance (RigWatch v2)](#6-predictive-equipment-maintenance-rigwatch-v2)
7. [Post-Well AI Debrief](#7-post-well-ai-debrief)
8. [Fleet Performance Analytics Dashboard](#8-fleet-performance-analytics-dashboard)
9. [Wellbore Stability Prediction](#9-wellbore-stability-prediction)

---

## 1. Hot Config Reload — DONE

**Priority:** Do first — every feature after this benefits from live-tunable thresholds.

**Why:** Currently `well_config.toml` is read once at startup. Changing a single threshold requires a full restart, which interrupts real-time monitoring mid-well. Operators need to tune thresholds while drilling without losing state.

### Tasks

- [x] **Polling-based file watcher on `well_config.toml`**
  - Mtime-based polling every 2s with 500ms debounce (`src/config/watcher.rs`)
  - Emits `ConfigEvent::Reloaded(Vec<ConfigChange>)` or `ConfigEvent::Error`
  - Handles file deletion/reappearance gracefully
  - Spawned in `main.rs` startup sequence

- [x] **Atomic config swap via `ArcSwap`**
  - `ArcSwap<WellConfig>` in `src/config/mod.rs` — lock-free reads via `get()`
  - Full validation before swap; on failure old config stays active
  - `arc-swap = "1.7"` in Cargo.toml

- [x] **Diff logging**
  - `ConfigChange { key, old_value, new_value }` struct in `src/config/mod.rs`
  - Recursive TOML tree diff via `diff_values()` — detects additions, removals, changes
  - Each changed field logged: `key: old_value -> new_value`

- [x] **REST API endpoints**
  - `GET /api/v2/config` — return current config as JSON
  - `POST /api/v2/config` — update config (validates, saves to disk, hot-reloads)
  - `POST /api/v2/config/validate` — dry-run validation without applying
  - `POST /api/v2/config/reload` — trigger manual reload from file

- [x] **Scope boundaries**
  - All threshold values: hot-reloadable
  - Ensemble weights: hot-reloadable
  - Advisory cooldowns: hot-reloadable
  - Non-reloadable fields (`server.addr`, `well.name`, etc.) emit restart-required warnings

- [ ] **Shadow validation mode (stretch)**
  - Run new config in parallel for N packets, compare advisory outputs
  - Auto-rollback if the new config produces >2x the advisory rate

### Key Files

- `src/config/mod.rs` — `ArcSwap` store, `reload()`, `diff()`, `ConfigChange`
- `src/config/watcher.rs` — polling file watcher, `ConfigEvent`
- `src/api/v2_handlers.rs:649-851` — config endpoints
- `src/api/v2_routes.rs:23-28` — route registration
- `src/main.rs:669-719` — watcher task spawn

---

## 2. Operator Feedback Loop — DONE

**Priority:** High — closes the loop on alert quality with minimal engineering effort.

**Why:** SAIREN-OS has 85+ configurable thresholds. Operators don't know which matter. A feedback mechanism lets the system learn from operator judgment: "this was a false positive" gradually tightens thresholds, "this was a real event" reinforces them.

### Tasks

- [x] **Feedback submission API**
  - `POST /api/v2/advisory/feedback/:timestamp` — submit operator feedback
  - Payload: `{ "outcome": "confirmed" | "false_positive" | "unclear", "submitted_by", "notes" }`
  - Persisted in sled (`src/storage/feedback.rs`) with denormalized advisory data
  - `POST /api/v2/advisory/acknowledge` — general acknowledgment endpoint also available

- [x] **Feedback accumulator (per category + threshold)**
  - `compute_stats()` in `src/storage/suggestions.rs` — groups by `AnomalyCategory`
  - Computes per-category: total, confirmed, false_positives, unclear, confirmation_rate
  - `GET /api/v2/advisory/feedback/stats` — returns `Vec<CategoryStats>`

- [x] **Threshold suggestion engine**
  - `compute_suggestions()` in `src/storage/suggestions.rs`
  - Rules: <50% confirmation over 10+ rated → suggest tightening (-10%); >90% over 20+ → suggest loosening (+5%)
  - Per-category sensitivity direction awareness (e.g., lower CV = more sensitive for Mechanical)
  - Value clamping: never suggests beyond ±25% of current
  - Confidence scaling: 0.5 at 10 samples → 1.0 at 50+ samples
  - `GET /api/v2/config/suggestions` — returns `Vec<ThresholdSuggestion>`
  - 6 unit tests covering edge cases

- [x] **Dashboard integration**
  - "Confirmed" / "False Positive" / "Unclear" feedback buttons on critical report detail pane
  - Feedback Analytics page (`/feedback`) with per-category confirmation rate bars and threshold suggestions
  - TypeScript types for `FeedbackRecord`, `CategoryStats`, `ThresholdSuggestion`
  - API client methods: `submitFeedback()`, `fetchFeedbackStats()`, `fetchConfigSuggestions()`
  - Nav item added to header

- [ ] **Fleet-level feedback aggregation (stretch)**
  - Upload feedback alongside FleetEvents to hub
  - Hub computes fleet-wide confirmation rates per category
  - Include in intelligence sync so new rigs start with pre-tuned thresholds

### Key Files

- `src/storage/feedback.rs` — `FeedbackOutcome`, `FeedbackRecord`, sled persistence
- `src/storage/suggestions.rs` — `CategoryStats`, `ThresholdSuggestion`, computation logic
- `src/api/v2_handlers.rs:858-927` — feedback/stats/suggestions endpoints
- `src/api/v2_routes.rs:36-37` — route registration
- `src/main.rs:307-311` — feedback storage init at startup

### Dependencies

- Requires: Hot Config Reload (feature 1) for "Apply" to work — **DONE**

---

## 3. Predictive Lookahead Advisor — DONE

**Priority:** High — the single biggest value-add. Transforms the product from reactive to proactive.

**Why:** Currently SAIREN-OS detects anomalies after they begin. Predictive lookahead uses formation prognosis + offset well data + ML-learned parameters to warn operators *before* they enter problematic zones. "In 500 ft you'll hit the Balder formation — offset wells averaged 15 ft/hr ROP and 2 pack-off events here. Recommend reducing WOB to 25 klbs."

### Tasks

- [x] **Formation lookahead query**
  - `check_look_ahead()` in `src/optimization/look_ahead.rs`
  - Estimates time-to-next-formation-boundary using current ROP and `FormationPrognosis`
  - Returns `LookAheadAdvisory` with depth range, parameter recommendations, hazards
  - Configurable window (default 30 minutes via `[lookahead].window_minutes`)

- [x] **Offset well performance overlay**
  - `OffsetPerformance` type in `src/types/formation.rs` with avg/best ROP, MSE, best params, notes
  - Integrated into `FormationInterval` structure for per-formation offset data
  - Loaded from knowledge base TOML files

- [x] **CfC-powered depth-ahead prediction**
  - `DepthAheadNetwork` in `src/cfc/depth_ahead.rs` — 64-neuron CfC (seed=1042, BPTT=6)
  - 8 real features (wob, rop, rpm, torque, mse, d_exponent, depth_into_formation, formation_hardness) zero-padded into 16-element arrays
  - Integrated into tactical agent as Phase 2.8.2; resets state on formation transitions
  - Confidence annotation on `LookAheadAdvisory` via `cfc_confidence` field
  - 6 unit tests

- [x] **Lookahead advisory generation**
  - `LookAheadAdvisory` type in `src/types/optimization.rs`
  - Formatted via `format_lookahead_advisory()` in `src/optimization/templates.rs`
  - Standalone check in `PipelineCoordinator::check_standalone_lookahead()` (coordinator.rs)
  - One-shot cooldown per formation boundary via `alerted_boundaries` HashSet
  - Risk levels: Low (no hazards) → Elevated (with hazards)

- [ ] **ML engine extension: per-formation parameter prediction**
  - Extend `OptimalFinder` to predict optimal params for *upcoming* formations using offset data
  - Store predictions in knowledge base alongside actual results (for post-well comparison)

- [x] **Configuration**
  - `[lookahead]` section in `well_config.default.toml` and `LookaheadConfig` struct
  - Fields: `enabled` (default true), `window_minutes` (default 30.0)
  - Validation: window_minutes in [5.0, 120.0]

- [x] **Dashboard: formation lookahead panel**
  - `LookAheadPanel` component polls `/api/v2/lookahead/status` every 10s
  - Shows formation name, ETA, depth remaining, parameter changes, hazards, offset notes
  - Yellow accent border (advisory styling), red when < 10 minutes
  - Hidden when no active lookahead; integrated into LiveView between WellControl and charts

### Key Files

- `src/optimization/look_ahead.rs` — core lookahead logic
- `src/optimization/templates.rs:121-161` — advisory formatting
- `src/pipeline/coordinator.rs:914-950` — pipeline integration with cooldown
- `src/types/formation.rs` — `FormationInterval`, `OffsetPerformance`
- `src/types/optimization.rs` — `LookAheadAdvisory` (with `cfc_confidence`)
- `src/cfc/depth_ahead.rs` — depth-ahead CfC network, feature extraction, 6 tests
- `src/agents/tactical.rs` — Phase 2.8.2 depth-ahead processing
- `dashboard/src/components/live/LookAheadPanel.tsx` — formation lookahead panel

### Dependencies

- Knowledge base must have formation prognosis loaded (pre-spud data) — **supported**
- Fleet hub offset well performance data — loaded from KB TOML, not yet from fleet hub

---

## 4. Stick-Slip Active Damping — Iteration 1 Done

**Priority:** High — currently detects stick-slip but doesn't recommend specific corrective actions.

**Why:** Stick-slip damages bits, BHA, and casing. The dysfunction filter already detects torque CV > 0.12 and rejects those samples from ML. The next step is recommending specific parameter changes to dampen the oscillation, monitoring whether the recommendation worked, and learning per-formation "recipes" for what works.

### Iteration 1 (Deterministic) — DONE

- [x] **Torque oscillation characterization**
  - `characterize_oscillation()` in `src/physics_engine/drilling_models.rs:448-528`
  - Zero-crossing frequency estimation on detrended torque series
  - Classifies: `StickSlip` (<1 Hz) vs `TorsionalGeneral` (>=1 Hz)
  - Computes: torque CV, amplitude ratio, severity (0-1), sample count
  - 4 unit tests (sinusoidal, insufficient data, stable torque, amplitude ratio)

- [x] **Parameter recommendation engine**
  - `recommend_damping()` in `src/physics_engine/drilling_models.rs:536-608`
  - Lookup table: stick-slip → WOB -10 to -15%, RPM +5 to +10% (severity-scaled); torsional → WOB -10%, RPM hold
  - Clamped to config-defined safe envelope (`max_wob_reduction_pct`, `max_rpm_change_pct`)
  - Guards: WOB >= 3 klbs (on-bottom), RPM <= 150% of current
  - 4 unit tests (recommendation values, config limits, off-bottom guard, torsional RPM hold)

- [x] **Coordinator enrichment**
  - `enrich_with_damping()` in `src/pipeline/coordinator.rs:956-1015`
  - Runs after ticket creation, before advanced physics (Phase DAMPING)
  - Attaches `DampingRecommendation` to stick-slip tickets only
  - Reads torque time series from 60-packet history buffer

- [x] **Strategic template enhancement**
  - `mechanical_template()` in `src/strategic/templates.rs:193-268`
  - When `damping_recommendation` present: outputs specific "WOB 25.0 → 21.3 klbs (-15%), RPM 120 → 132 (+10%)"
  - Falls back to generic advice when no damping recommendation attached
  - 1 unit test (`test_mechanical_template_with_damping`)

- [x] **API endpoint**
  - `GET /api/v2/damping/status` — returns current damping analysis + recommendation
  - `DampingStatus` and `DampingRecommendationResponse` in `src/api/v2_handlers.rs:1038-1140`

- [x] **Configuration**
  - `[damping]` section: `enabled`, `max_wob_reduction_pct`, `max_rpm_change_pct`, `cv_threshold`, `min_samples`
  - `DampingConfig` in `src/config/well_config.rs:1452-1487`
  - Validation: ranges enforced in `src/config/validation.rs:400-425`

### Iteration 2 (Closed-Loop) — DONE

- [x] **Feedback monitoring loop**
  - After recommendation issued, monitor torque CV over configurable window (default 120s)
  - Three outcomes: Success (CV drops ≥20%), Escalated (window expires), Retracted (CV rises ≥15%)
  - State machine in coordinator: `Idle → Active → Idle` with outcome tracking
  - Monitor outcomes emit standalone advisories when no other advisory is generated
  - Configurable via `[damping]`: `monitor_window_secs`, `success_cv_reduction_pct`, `retract_cv_increase_pct`
  - 4 unit tests (success transition, retraction, escalation, compute_torque_cv)

- [x] **Per-formation recipe library**
  - Successful damping actions stored in sled (`src/storage/damping_recipes.rs`), keyed by formation name
  - On future stick-slip in same formation: recommendation blended with recipe (70% recipe, 30% lookup table)
  - Recipe pruning: max `max_recipes_per_formation` (default 20) per formation
  - Best recipe selection by lowest achieved CV
  - `GET /api/v2/damping/recipes` endpoint lists all formation recipes
  - `GET /api/v2/damping/status` enhanced with monitor snapshot
  - 3 unit tests (persist/retrieve, pruning, best selection) + config validation test

- [ ] **New specialist: Mechanical Damping Specialist (stretch)**
  - Dedicated specialist that votes on stick-slip advisories
  - Considers: current torque CV, oscillation type, historical recipe success rate, proximity to safe envelope limits
  - Weight: 15% (reduce others proportionally when active)

- [ ] **Dashboard: stick-slip damping panel**
  - Real-time torque waveform with oscillation frequency annotation
  - Active recommendation display with countdown timer
  - Outcome history: "Last 5 damping actions: 4 successful, 1 escalated"

### Key Files

- `src/physics_engine/drilling_models.rs:448-608` — characterization + recommendation engine
- `src/pipeline/coordinator.rs:1001-1328` — ticket enrichment, monitor state machine, recipe blending, snapshot API
- `src/strategic/templates.rs:193-268` — template formatting
- `src/types/advisory.rs:134-230` — `OscillationType`, `OscillationAnalysis`, `DampingRecommendation`, `DampingOutcome`, `DampingRecipe`, `DampingMonitorSnapshot`
- `src/storage/damping_recipes.rs` — sled-persisted per-formation recipe library
- `src/api/v2_handlers.rs:1038-1180` — damping status + recipes endpoints
- `src/config/well_config.rs:1452-1520` — `DampingConfig` with monitoring fields

---

## 5. Federated CfC Weight Sharing

**Priority:** Medium — high value but requires fleet hub infrastructure changes.

**Why:** Each rig's CfC networks train from scratch on every new well. Rigs drilling in the same field/formation encounter similar dynamics. Sharing learned weights across rigs means Rig B starts with Rig A's trained priors — reaching optimal sensitivity in hours instead of days.

### Implementation — DONE

Implemented via direct serde derives on CfC types (no wrapper types), watch channels
between the processing loop and async background tasks, and federated averaging with
parallel Welford normalizer merging and fresh Adam reset.

#### Files Created

| File | Purpose |
|------|---------|
| `src/cfc/checkpoint.rs` | `DualCfcCheckpoint`, `CfcNetworkCheckpoint`, `CheckpointMetadata`; `snapshot()`/`restore_from()` on `CfcNetwork`; atomic disk save/load; 5 tests |
| `src/fleet/federation.rs` | Spoke-side background tasks: `run_checkpoint_upload`, `run_federation_pull` with watch channels |
| `src/hub/federation.rs` | Hub-side `federated_average()` with weighted averaging, parallel Welford combination, fresh Adam reset; 4 tests |
| `src/hub/api/federation.rs` | `POST /federation/checkpoint` (UPSERT + re-aggregate), `GET /federation/model`; `FederationState` with DashMap |

#### Files Modified

- `src/cfc/cell.rs`, `normalizer.rs`, `training.rs`, `network.rs`, `wiring.rs` — `Serialize`/`Deserialize` derives
- `src/cfc/mod.rs` — `pub mod checkpoint;`, `snapshot()`/`restore_from()` on `DualCfcNetwork`
- `src/cfc/network.rs` — checkpoint accessors + setters on `CfcNetwork`
- `src/fleet/mod.rs`, `client.rs` — `pub mod federation;`, `upload_checkpoint()`, `pull_federated_model()`
- `src/config/well_config.rs` — `FederationConfig`, `FederationInitPolicy`
- `well_config.default.toml` — commented `[federation]` section
- `src/pipeline/processing_loop.rs` — `FederationContext`, watch channel plumbing, checkpoint publish + inbound restore
- `src/pipeline/coordinator.rs` — `snapshot_cfc()`, `restore_cfc_from_checkpoint()`
- `src/agents/tactical.rs` — `cfc_network_mut()` accessor
- `src/main.rs` — `FederationUpload`/`FederationPull` task variants, federation spawning
- `src/hub/mod.rs`, `src/hub/api/mod.rs` — federation module + route registration

#### Key Design Decisions

1. **Direct serde derives** — no wrapper types or conversion functions
2. **Watch channels** (not `Arc<RwLock>`) — CfcNetwork stays on the processing task without a lock
3. **Optimizer reset after averaging** — standard federated learning practice
4. **One checkpoint per rig (UPSERT)** — only latest state matters
5. **~40KB per checkpoint** (JSON+zstd) — small enough for existing fleet transport
6. **Opt-in, disabled by default** — `federation.enable = false`

#### Configuration

```toml
[federation]
enable                   = false
checkpoint_interval_secs = 3600
pull_interval_secs       = 7200
init_policy              = "fresh_only"   # fresh_only | better_model | upload_only
min_packets_for_upload   = 1000
checkpoint_path          = "./data/cfc_checkpoint.json"
```

#### Verification

- `cargo check` — clean
- `cargo test --lib` — 390 passed
- `cargo test --lib --features fleet-hub` — 405 passed (includes 4 hub federation + 5 checkpoint tests)

#### Remaining (stretch)

- [ ] **Privacy-preserving mode** — gradient-only sharing to prevent weight reverse-engineering
- [ ] **Federation metrics** — track calibration time and false positive rates for federated vs cold start
- [ ] **Formation-scoped models** — group checkpoints by formation for more targeted averaging

---

## 6. Predictive Equipment Maintenance (RigWatch v2) — DONE

**Priority:** Medium — existing RigWatch system (`/home/ashton/rigwatch`) works but is limited by LLM-only intelligence. CfC networks can massively improve prediction accuracy.

**Why:** RigWatch v1 uses deterministic health scoring (40% vibration, 25% bearing, 20% temp, 15% spectral) + LLM diagnosis. It detects current health but cannot predict time-to-failure or learn degradation patterns over time. CfC networks can learn the temporal dynamics of equipment degradation, enabling true predictive maintenance.

### Implementation — DONE

Ported SAIREN-OS dual CfC architecture into RigWatch with 16-feature equipment sensor
mapping, template diagnostic backend for Pi deployment (no GPU/LLM needed), and full
pipeline integration with checkpoint persistence.

#### Files Created (8)

| File | Purpose |
|------|---------|
| `rigwatch/src/cfc/mod.rs` | DualCfcNetwork, extract_features(), spectral entropy, update_dual() |
| `rigwatch/src/cfc/cell.rs` | CfcWeights, forward pass, ForwardCache (port from SAIREN-OS) |
| `rigwatch/src/cfc/wiring.rs` | NcpConfig, NcpWiring, topology generation (port) |
| `rigwatch/src/cfc/normalizer.rs` | OnlineNormalizer with equipment FEATURE_NAMES (adapted) |
| `rigwatch/src/cfc/training.rs` | TrainingConfig, AdamOptimizer, BPTT (port) |
| `rigwatch/src/cfc/network.rs` | CfcNetwork, CfcNetworkConfig, process(), anomaly_score() (port) |
| `rigwatch/src/cfc/checkpoint.rs` | DualCfcCheckpoint, atomic disk save/load (adapted) |
| `rigwatch/src/llm/template.rs` | Template diagnostic backend for Pi (no LLM/GPU needed) |

#### Files Modified (8)

| File | Change |
|------|--------|
| `rigwatch/Cargo.toml` | Added `rand`, `rayon`, `tempfile` |
| `rigwatch/src/main.rs` | `mod cfc;`, rayon 2-thread pool init |
| `rigwatch/src/acquisition/sensors.rs` | Emit torque as SensorReading |
| `rigwatch/src/pipeline/processor.rs` | CfC processing, checkpoint save/load, AppState.cfc_status |
| `rigwatch/src/director/llm_director.rs` | `cfc_context` param enriches LLM prompts |
| `rigwatch/src/strategic/aggregation.rs` | anomaly_score, cfc_calibrated, feature_surprises |
| `rigwatch/src/llm/mod.rs` | Template backend variant, SchedulerHandle stub |
| `rigwatch/src/api/handlers.rs` | `GET /api/v1/cfc` endpoint |

#### Feature Vector (16 features → 24 sensory neurons)

| # | Feature | Source | Weight |
|---|---------|--------|--------|
| 0-7 | rms, peak_freq, bpfo, bpfi, bsf, ftf, 1x, 2x | FFT/bearing analysis | Primary (2x) |
| 8-15 | motor_avg/max, gearbox_avg/max, rpm, torque, entropy, health_score | Sensors + derived | Supplementary (1x) |

#### Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `RIGWATCH_CFC_ENABLED` | `true` | Enable/disable CfC |
| `RIGWATCH_CFC_SEED` | `42` | RNG seed for NCP wiring |
| `RIGWATCH_CFC_CHECKPOINT` | `./data/cfc_checkpoint.json` | Checkpoint path |
| `RIGWATCH_LLM_BACKEND` | (mock) | Set to `template` for Pi deployment |

#### Verification

- `cargo check` — clean (warnings only)
- `cargo test` — 117 passed (48 new CfC + template tests, 4 pre-existing failures)
- 42 CfC tests + 6 template backend tests all passing

#### Remaining (future enhancements)

- [ ] **Time-to-failure (TTF) estimation** — degradation curve fitting, failure threshold extrapolation
- [ ] **Adaptive baseline with CfC** — replace static 5-minute baseline with continuous learning
- [ ] **Learned sensor weighting** — dynamic per-feature importance from CfC surprise analysis
- [ ] **Multi-equipment support** — configurable bearing geometry, per-equipment CfC networks
- [ ] **Fault progression classifier** — motor neuron output stage classification (5 stages)
- [ ] **Integration with SAIREN-OS fleet hub (stretch)** — equipment health in drilling advisories

---

## 7. Post-Well AI Debrief — Early

**Priority:** Medium — high operator value, relatively contained scope.

**Why:** After a well is finished, the company man spends 1-2 days writing a post-well report. SAIREN-OS has all the data to auto-generate this: every advisory, every formation transition, every ML insight, every fleet episode. An LLM-powered debrief can produce a first draft in minutes.

### Tasks

- [ ] **Well timeline assembly**
  - Compile chronological event log from well start to TD (or plug set):
    - All advisories (with specialist votes, severity, operator feedback)
    - Formation transitions (depth, name, d-exponent shift)
    - Rig state changes (drilling → tripping → circulating sequences)
    - ML engine insights (optimal param updates, dysfunction events)
    - CfC regime changes (cluster transitions)
  - Store as structured `WellTimeline` object

- [x] **Performance metrics computation (partial)**
  - `PostWellSummary` and `PostWellFormationPerformance` types in `src/types/knowledge_base.rs`
  - `generate_post_well()` in `src/knowledge_base/post_well.rs` — generates summary from mid-well snapshots
  - `complete_well()` method in `src/knowledge_base/mod.rs`
  - Missing: whole-well NPT, footage/day, cost/ft, advisory quality stats, CfC stats

- [ ] **Planned vs actual comparison**
  - Compare pre-spud prognosis against actual results per formation
  - Highlight: formations that were harder/easier than expected
  - Compute: depth error at each formation boundary (prognosis vs actual)
  - Flag: unplanned events (kicks, losses, stuck pipe) vs prognosis

- [ ] **LLM-powered narrative generation**
  - Feed structured timeline + metrics to LLM (Qwen 7B or strategic model)
  - Prompt: "Generate a post-well drilling report summarizing key events, lessons learned, and recommendations for offset wells"
  - Structured sections: Executive Summary, Formation-by-Formation Review, Anomaly Analysis, Lessons Learned, Recommendations
  - Template fallback if LLM unavailable (fill-in-the-blanks from metrics)

- [ ] **Debrief trigger**
  - Auto-trigger when `RigState::Secured` detected (or manual API call)
  - `POST /api/v2/well/debrief` — generate debrief on demand
  - `GET /api/v2/well/debrief` — retrieve latest debrief

- [x] **Knowledge base storage (partial)**
  - Post-well summary written to KB via `complete_well()`
  - Per-formation performance data stored
  - Missing: offset data consumption by Predictive Lookahead

- [ ] **Fleet hub upload**
  - Upload debrief summary to hub
  - Hub curates "lessons learned" library across all wells
  - Intelligence sync delivers relevant lessons to future rigs in same field

- [ ] **Dashboard: debrief viewer (stretch)**
  - Formatted report view with charts
  - Formation-by-formation performance bars
  - Advisory timeline visualization
  - Export to PDF

### Key Files

- `src/knowledge_base/post_well.rs` — post-well summary generation
- `src/knowledge_base/mod.rs` — `complete_well()` trigger
- `src/types/knowledge_base.rs` — `PostWellSummary`, `PostWellFormationPerformance`

### Integration Points

- New module: `src/debrief/` (timeline assembly, metrics, LLM generation)
- `src/api/handlers.rs` — debrief endpoints
- `src/fleet/types.rs` — `PostWellDebrief` for hub upload
- `src/fleet/sync.rs` — debrief upload task

---

## 8. Fleet Performance Analytics Dashboard

**Priority:** Medium — requires fleet hub with multiple rigs reporting data.

**Why:** With multiple rigs feeding data to the hub, there's a massive opportunity to compare performance across rigs, formations, and operators. "Why is Rig-5 2x slower than Rig-3 in the same formation?" turns fleet data into a competitive moat.

### Tasks

- [ ] **Hub-side aggregation engine**
  - Background worker that computes analytics from stored FleetEvents + performance data:
    - Per-formation benchmarks: median ROP, MSE efficiency, anomaly frequency
    - Per-rig performance: footage/day, NPT hours, advisory confirmation rate
    - Anomaly heatmap: formation x anomaly_category x frequency
    - Trending: rolling 7/30-day performance windows

- [ ] **Hub analytics API**
  - `GET /api/fleet/analytics/formations` — formation benchmarks across fleet
  - `GET /api/fleet/analytics/rigs` — per-rig performance comparison
  - `GET /api/fleet/analytics/anomalies` — anomaly frequency heatmap data
  - `GET /api/fleet/analytics/trends` — time-series performance trends
  - `GET /api/fleet/analytics/wells/{well_id}` — single-well deep dive
  - All endpoints support filtering by: field, formation, campaign, date range

- [ ] **Comparative metrics**
  - ROP percentile: "Rig-3 is at P75 for this formation (faster than 75% of wells)"
  - MSE efficiency ranking across rigs
  - NPT attribution: which anomaly categories cause the most downtime per rig
  - Best practices extraction: what parameters do top-performing rigs use?

- [ ] **Anomaly pattern detection**
  - Cluster similar anomaly sequences across fleet (same formation → same failure mode?)
  - Identify "problem formations" with statistically elevated anomaly rates
  - Detect rig-specific issues (Rig-5 always has stick-slip in Balder — why?)

- [ ] **Dashboard: fleet analytics view**
  - New page/section in the React dashboard (or separate hub dashboard)
  - Formation performance heatmap (depth on Y axis, ROP color-coded)
  - Rig comparison bar charts (footage/day, NPT, confirmation rate)
  - Anomaly frequency treemap (size = frequency, color = severity)
  - Drill-down: click formation → see all rigs' performance in that zone

- [ ] **Spoke-side integration**
  - Spokes can query hub analytics for their own field/formation
  - Pre-load formation benchmarks into knowledge base (feeds Predictive Lookahead)
  - Show "how am I doing vs fleet average" on local dashboard

- [ ] **Configuration**
  ```toml
  [fleet.analytics]
  aggregation_interval_minutes = 60
  benchmark_min_wells = 3        # Minimum wells for meaningful benchmark
  trending_window_days = 30
  ```

### Integration Points

- Fleet hub — new `analytics/` module with aggregation workers
- Fleet hub — new API endpoints
- `src/fleet/sync.rs` — analytics query for spoke-side consumption
- `src/knowledge_base/` — ingest fleet benchmarks
- Dashboard — new analytics page/components

### Dependencies

- Requires: Fleet hub operational with 3+ rigs reporting
- Feeds into: Predictive Lookahead (formation benchmarks), Post-Well Debrief (comparative metrics)

---

## 9. Wellbore Stability Prediction

**Priority:** Lower (requires external data integration) — but highest safety impact.

**Why:** Differential sticking and wellbore collapse are among the costliest drilling problems ($1-5M per event). Current SAIREN-OS detects mechanical issues reactively. Integrating geomechanical models + real-time drilling data can predict instability *before* the bit reaches problem zones.

### Tasks

- [ ] **Geomechanical data ingestion**
  - Accept pre-loaded stress/pore pressure model:
    - Format: TOML profile per formation interval (simplest)
    - Optional: LAS file import for offset well log data
    - Fields: pore pressure gradient (ppg), fracture gradient (ppg), min horizontal stress, max horizontal stress, UCS (unconfined compressive strength)
  - Store in knowledge base: `{field}/geomechanics.toml`

- [ ] **Real-time stability score**
  - Compute at each depth:
    ```
    mud_weight_window = fracture_gradient - pore_pressure
    current_margin = (mud_weight_in - pore_pressure) / mud_weight_window
    stability_score = f(current_margin, torque_trend, drag_trend, cfc_anomaly)
    ```
  - Score 0-100: 80+ stable, 60-80 caution, 40-60 at-risk, <40 critical
  - CfC feature surprises on torque/hookload as leading indicators

- [ ] **Breakout zone prediction**
  - Using UCS + stress model, predict depth intervals where:
    - Wellbore breakout likely (compressive failure)
    - Tensile fracture likely (lost circulation)
    - Differential sticking risk elevated (low ROP + high WOB + overbalance)
  - Flag these zones in formation prognosis for Predictive Lookahead to consume

- [ ] **Mud weight advisory**
  - Given pore pressure + fracture gradient profile, recommend:
    - Minimum safe mud weight (prevent kicks)
    - Maximum safe mud weight (prevent losses)
    - Optimal mud weight (balance)
  - Alert when current mud weight is outside safe window
  - Track ECD (not just static MW) for dynamic stability assessment

- [ ] **New specialist: Stability Specialist**
  - Votes on advisories related to wellbore stability:
    - Pore pressure indicators (d-exponent trend reversal)
    - Mechanical instability (erratic torque, overpull on connections)
    - Lost circulation precursors (SPP drops, flow imbalance)
  - Weight: 20% when geomechanical data available (reduce Formation specialist to 10%)
  - Falls back to Formation specialist behavior when no geomechanical data loaded

- [ ] **CfC stability signals**
  - Use CfC motor output diversity as instability indicator
  - Rapid regime switching = variable rock behavior = instability risk
  - Train on labeled data from fleet: events tagged "stuck pipe", "lost circ", "tight hole"
  - Feature: `stability_risk = f(regime_churn_rate, anomaly_trend, drag_factor)`

- [ ] **Configuration**
  ```toml
  [wellbore_stability]
  enabled = false                 # Opt-in (requires geomechanical data)
  pore_pressure_source = "prognosis"  # prognosis | las | manual
  mud_weight_margin_ppg = 0.3
  breakout_ucs_threshold_psi = 5000
  specialist_weight = 0.20
  ```

### Integration Points

- `src/knowledge_base/` — geomechanical data storage and queries
- `src/agents/specialists/` — new `stability.rs` specialist
- `src/agents/tactical.rs` — stability score computation
- `src/agents/strategic.rs` — mud weight advisory generation
- `src/config/well_config.rs` — `[wellbore_stability]` section
- Feeds into: Predictive Lookahead (breakout zone predictions), Post-Well Debrief (stability analysis)

### Dependencies

- Requires: geomechanical data for the well/field (from operator's geoscience team)
- Enhanced by: Fleet analytics (fleet-wide stability patterns)

---

## Build Order

Recommended implementation sequence based on dependencies and incremental value:

```
Phase 1: Foundation — DONE
  1. Hot Config Reload          ✓ DONE (ArcSwap, file watcher, diff logging, REST API)
  2. Operator Feedback Loop     ✓ DONE (feedback API, suggestions engine, dashboard UI)

Phase 2: Prediction — DONE
  3. Predictive Lookahead       ✓ DONE (lookahead query, CfC depth-ahead, dashboard panel)
                                  Remaining: ML per-formation prediction (stretch)
  4. Stick-Slip Active Damping  ✓ Iteration 2 done (feedback monitoring, recipe library, API)
                                  Remaining: Specialist (stretch), dashboard panel

Phase 3: Fleet Intelligence — DONE
  5. Federated CfC Weights      ✓ DONE (checkpoint types, spoke tasks, hub aggregation, watch channels)
  6. RigWatch v2 (CfC)          ✓ DONE (dual CfC port, template backend, pipeline integration, API)

Phase 4: Deep Integration — Early
  7. Post-Well AI Debrief        ~ Early (post-well summary types + KB storage exist)
                                  Remaining: timeline assembly, planned-vs-actual, LLM narrative, API, dashboard
  8. Fleet Analytics Dashboard   ○ Not started (requires hub with data from multiple rigs)

Phase 5: Geomechanics — Not Started
  9. Wellbore Stability          ○ Not started (requires external geomechanical data)
```

### Next Actions

With Features 1-6 done and Features 2-3 dashboard complete:
- **Finish Feature 4 dashboard** — stick-slip damping panel (torque waveform, recommendation display, outcome history)
- **Post-Well AI Debrief** (Feature 7) — timeline assembly, LLM narrative, API
- **Fleet Analytics Dashboard** (Feature 8) — requires hub with 3+ rigs

---

*Last updated: 2026-03-01*
*SAIREN-OS version: v4.0-dev*
