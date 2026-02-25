# Changelog

All notable changes to SAIREN-OS are documented here.

---

## v3.1 - Dashboard, v2 API, Setup Wizard & Safety Audit

**Phase 3: v2 API** (`src/api/v2_handlers.rs`, `src/api/v2_routes.rs`, `src/api/envelope.rs`):
- **Consolidated live endpoint** — `GET /api/v2/live` replaces 7 separate v1 polling endpoints
- **JSON envelope** — `ApiResponse<T>` / `ApiErrorResponse` wrappers for consistent error handling
- **v1 deprecation** — `Deprecation: true` + `Sunset: 2026-09-01` headers on all v1 responses via `src/api/middleware.rs`

**Phase 4: React Dashboard** (`dashboard/`):
- **React + Vite + Tailwind + Recharts** SPA compiled into binary via `rust-embed`
- **SPA fallback** routing in `src/api/mod.rs` — serves `index.html` for any non-API path
- **CORS** — configurable via `SAIREN_CORS_ORIGINS` env var for development

**Phase 6: Feature flag de-gating**:
- **`knowledge-base`** and **`fleet-client`** features removed — always compiled
- **`llm`** is now the default feature (`default = ["llm"]`)
- Runtime gates: `FLEET_HUB_URL` env var for fleet sync, `is_cuda_available()` for LLM model selection

**Phase 7: Setup Wizard & Pairing** (`src/api/setup.rs`, `src/acquisition/scanner.rs`, `src/hub/api/pairing.rs`):
- **`sairen-os setup`** — web-based setup wizard with WITS subnet scanner
- **`sairen-os pair`** — headless CLI pairing with Fleet Hub via 6-digit code
- **`sairen-os enroll`** — deprecated (hidden), replaced by setup/pair

**Phase E: Quality of Life**:
- **Handler refactor** — split 1,928-line `handlers.rs` into 7 sub-modules (`handlers/status.rs`, `drilling.rs`, `reports.rs`, `ml.rs`, `config.rs`, `metrics.rs`)
- **API regression tests** — converted from process-spawning `#[ignore]` tests to in-process `tower::oneshot` tests
- **`.gitignore` hardened** — added patterns for `.env`, `*.pem`, `*.key`, `credentials.json`

**Safety audit fixes**:
- **S2**: Flow imbalance defaults lowered from 10/20 to 5/10 gpm for earlier kick detection
- **S3**: Added `check_bidirectional()` for signed metrics (uses `z.abs()`)
- **S4**: Only `well_control_critical` forces Critical severity (removed `any_critical` override)
- **S5**: ECD margin warns when fracture gradient unavailable (was silent)
- **D1**: Gas units critical lowered from 500 to 250 (2.5x gap instead of 5x)
- **L1**: FinalSeverity boundaries changed from `>=` to `>` to prevent half-point inflation
- **L6**: Equal-weight fallback when regime scaling zeros all specialist weights
- **L7**: Causal leads window excludes current packet to prevent self-correlation
- **I3**: All user-supplied query `limit` parameters capped at `.min(1000)` to prevent DoS

---

## v3.0 - Causal Inference & Regime-Aware Intelligence

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

---

## v2.2 - Structured Knowledge Base

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

**Feature**: Knowledge base is always compiled (originally behind `knowledge-base` feature flag, de-gated in v3.1)

**Tests**: 17 new unit tests across all KB modules + 3 integration tests (migrate-and-assemble, offset-well assembly, full lifecycle)

---

## v2.1 - CfC Active Integration & WITSML Support

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

---

## v2.0 - CfC Neural Network Operations Specialist

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

---

## v1.1 - Fleet Hub Implementation

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

---

## v1.0 - Trait Architecture & Fleet Preparation

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

---

## v0.9 - Pattern-Matched Tactical Routing

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

---

## v0.8 - Production Hardening

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

---

## v0.7 - ML Engine V2.2 (Dysfunction-Aware Optimization)

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

---

## v0.6 - Founder Detection & Simulator Enhancements

- **Founder detection**: Two-stage detection (tactical quick check + strategic trend analysis)
- Trend-based WOB/ROP analysis using linear regression over history buffer
- Optimal WOB estimation (identifies where ROP was highest)
- Severity classification: Low (30%), Medium (50%), High (70%+)
- Strategic agent verification with actionable recommendations
- Simulator physics improvements:
  - WOB now correctly zero when bit is off bottom
  - Founder point model in ROP calculation (ROP decreases past optimal WOB)
  - Trip In keyboard control (`I` key)

---

## v0.5 - Operation Classification

- Automatic operation detection based on drilling parameters
- P&A-specific operations: Milling, Cement Drill-Out
- Operation transition logging with parameter context
- Simulator `--operation` flag and keyboard controls (M/O)

---

## v0.4 - ML Engine V2.1

- Optimal drilling conditions analysis (WOB, RPM, flow)
- Campaign-aware optimization weights
- Pearson correlation with p-value significance testing
- Formation boundary detection via d-exponent shifts
- Configurable scheduler (`ML_INTERVAL_SECS`)

---

## v0.3 - Campaign System

- Production and P&A campaign modes
- Campaign-aware thresholds and LLM prompts
- Runtime switching via dashboard/API
- Simulator `--campaign` flag

---

## v0.2 - Stability Improvements

- Periodic 10-minute summaries
- Pit rate noise filtering
- ECD margin stability
- CRITICAL cooldown (30s)
