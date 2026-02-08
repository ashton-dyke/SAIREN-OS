# SAIREN-OS Build Plan: Prebuild 1.0 to Production

**Date:** February 8, 2026
**Based on:** `README-prebuild.md` (Pre-Build 1.0 Merged) gap analysis against current codebase (v0.9)

---

## Executive Summary

The current codebase implements ~85-90% of the prebuild specification. The 10-phase pipeline is functional, physics engine is complete, agents work, ML engine V2.2 runs, and the API serves 30+ endpoints. The remaining work falls into four phases: **trait formalization**, **resilience hardening**, **fleet preparation**, and **multi-rig deployment**. Each phase is independently shippable.

---

## Current State vs. Prebuild Spec

### Already Done (No Work Needed)

| Component | Location | Status |
|---|---|---|
| WitsPacket, DrillingMetrics, RigState, all core types | `src/types.rs` | Complete |
| WITS TCP ingestion + reconnect + stdin + CSV replay | `src/acquisition/` | Complete |
| CLI flags: `--wits-tcp`, `--stdin`, `--csv` | `src/main.rs:64-96` | Complete |
| Physics engine: MSE, d-exp, ECD, flow balance, detectors | `src/physics_engine/` | Complete |
| Tactical agent: pattern-matched gating (v0.9) | `src/agents/tactical.rs` | Complete |
| History buffer: 60-packet ring | `src/history_storage.rs` | Complete |
| Strategic verification: `verify_ticket()` | `src/agents/strategic.rs` | Complete |
| LLM integration: Qwen 2.5, CPU/GPU auto-select | `src/llm/` | Complete |
| LlmBackend trait | `src/llm/mod.rs:45` | Complete |
| Orchestrator: 4-specialist weighted voting | `src/agents/orchestrator.rs` | Complete |
| StrategicAdvisory composition (inline in orchestrator) | `src/agents/orchestrator.rs:131` | Complete |
| Campaign system: Production + P&A overrides | `src/config/well_config.rs`, `src/types.rs` | Complete |
| Baseline learning: crash-safe persistence | `src/baseline/mod.rs` | Complete |
| ML Engine V2.2: 6-stage pipeline | `src/ml_engine/` | Complete |
| Configuration: TOML, 43+ thresholds, API updates | `src/config/` | Complete |
| REST API: 30+ endpoints | `src/api/` | Complete |
| Dashboard: real-time web UI | `static/index.html` | Complete |
| Regression helpers: slope, R-squared, intercept | `src/physics_engine/drilling_models.rs:642-709` | Complete |
| Per-phase latency logging | `src/pipeline/coordinator.rs` | Complete |
| Edge deployment: systemd + install script | `deploy/` | Complete |
| Advisory acknowledgment + audit trail | `src/api/handlers.rs` | Complete |

### Gaps to Fill

| Gap | Prebuild Section | Priority | Phase |
|---|---|---|---|
| KnowledgeStore trait + NoOpStore | S8, S18, S27-F | High | 1 |
| Specialist trait (formalize inline specialists) | S10, S18, S27-H | Medium | 1 |
| AdvisoryComposer (extract from orchestrator) | S11, S27-I | Medium | 1 |
| CRITICAL advisory cooldown (30s) | S11 | Medium | 1 |
| Template fallback when LLM unavailable | S9, S25 | High | 2 |
| Background self-healer | S13, S27-L | High | 2 |
| PersistenceLayer trait + InMemoryDAL | S19, S18, S27-K | Medium | 2 |
| RAM Recall (HNSW + metadata filtering) | S23, S27-F | Medium | 3 |
| FleetEvent / FleetEpisode types | S20 | Medium | 3 |
| Fleet client + upload queue | S20, S27-M | Low (post-pilot) | 4 |
| Fleet library sync | S20, S27-M | Low (post-pilot) | 4 |
| PostgreSQL persistence | S19 | Low (post-pilot) | 4 |

---

## Phase 1: Trait Formalization & Architecture Cleanup

**Goal:** Codify the implicit trait boundaries that already exist in the code into explicit Rust traits, enabling testability, swappable backends, and clear module contracts.

**Estimated scope:** ~800-1200 lines of refactoring + new trait definitions. No new functionality — purely structural.

### 1.1 KnowledgeStore Trait + NoOpStore

**What:** Extract the knowledge query interface from `src/context/vector_db.rs` into a formal trait, and add a `NoOpStore` that returns empty results.

**Why:** The prebuild requires Phase 6 to be pluggable — NoOp for pilot, RAM Recall later, fleet DB eventually. Currently `vector_db.rs` is a static keyword database with standalone functions.

**Files to modify:**
- `src/context/mod.rs` — re-export trait
- `src/context/vector_db.rs` — implement `KnowledgeStore` for existing `StaticKnowledgeBase`

**Files to create:**
- `src/context/knowledge_store.rs` — trait definition + `NoOpStore` implementation

**Trait shape:**
```rust
#[async_trait]
pub trait KnowledgeStore: Send + Sync {
    async fn query_precedent(
        &self,
        category: &AnomalyCategory,
        campaign: &Campaign,
        depth: f64,
    ) -> Vec<KnowledgePrecedent>;

    fn store_name(&self) -> &'static str;
    fn is_healthy(&self) -> bool;
}
```

**Acceptance criteria:**
- `NoOpStore` returns empty vec, `is_healthy()` returns true
- Existing `vector_db.rs` functionality preserved behind the trait
- Pipeline coordinator uses `Box<dyn KnowledgeStore>` instead of direct calls
- All existing tests pass

### 1.2 Specialist Trait

**What:** Extract the four inline specialist methods from `src/agents/orchestrator.rs:147-339` into structs implementing a `Specialist` trait.

**Why:** Adding a new specialist (e.g., Vibration, Cement Integrity) currently requires editing the orchestrator internals. A trait makes specialists composable and testable in isolation.

**Files to modify:**
- `src/agents/orchestrator.rs` — use `Vec<Box<dyn Specialist>>` instead of calling inline methods
- `src/agents/mod.rs` — re-export specialist module

**Files to create:**
- `src/agents/specialists/mod.rs` — trait definition
- `src/agents/specialists/mse.rs`
- `src/agents/specialists/hydraulic.rs`
- `src/agents/specialists/well_control.rs`
- `src/agents/specialists/formation.rs`

**Trait shape:**
```rust
pub trait Specialist: Send + Sync {
    fn name(&self) -> &str;
    fn evaluate(&self, ticket: &AdvisoryTicket) -> SpecialistVote;
}
```

**Acceptance criteria:**
- Each specialist is a standalone struct implementing the trait
- Orchestrator iterates `Vec<Box<dyn Specialist>>` with campaign-aware weights
- Identical voting behavior to current inline implementation
- Unit tests for each specialist in isolation

### 1.3 AdvisoryComposer (Extract from Orchestrator)

**What:** Extract advisory composition logic from `orchestrator.vote()` into a dedicated `AdvisoryComposer` with a `compose()` method. Add a 30-second CRITICAL cooldown to prevent spam.

**Why:** The orchestrator currently mixes voting logic with advisory construction. Separating them clarifies responsibility and enables the cooldown requirement.

**Files to modify:**
- `src/agents/orchestrator.rs` — `vote()` returns `WeightedConsensus`, not `StrategicAdvisory`
- `src/pipeline/coordinator.rs` — use composer after voting

**Files to create:**
- `src/strategic/advisory.rs` — `AdvisoryComposer` struct with `compose()` and cooldown tracking

**Key detail — CRITICAL cooldown:**
```rust
pub struct AdvisoryComposer {
    last_critical: Option<Instant>,
    cooldown: Duration, // default 30s
}

impl AdvisoryComposer {
    pub fn compose(...) -> Option<StrategicAdvisory> {
        // If CRITICAL and within cooldown window, return None
        // Otherwise compose and update last_critical
    }
}
```

**Acceptance criteria:**
- CRITICAL advisories within 30s of previous CRITICAL are suppressed
- Non-CRITICAL advisories unaffected by cooldown
- Advisory includes all evidence fields (physics verdict, specialist votes, precedent summary)
- Orchestrator unit tests still pass (voting logic unchanged)

### 1.4 Phase 1 Integration Test

**What:** Wire the new traits into the pipeline coordinator and verify end-to-end with the existing simulation binary.

**Files to modify:**
- `src/pipeline/coordinator.rs` — inject `Box<dyn KnowledgeStore>` and `AdvisoryComposer`
- `src/main.rs` — construct `NoOpStore` + specialists + composer at startup

**Acceptance criteria:**
- `cargo build --release` compiles cleanly
- `cargo build --release --features llm` compiles cleanly
- Simulation binary runs without regression
- Clippy clean at `warn` level

---

## Phase 2: Resilience Hardening

**Goal:** Make the system robust against LLM failure, WITS disconnection, disk issues, and other runtime failures. This is the "lights go out, system keeps running" phase.

**Estimated scope:** ~1500-2000 lines of new code.

### 2.1 Template Fallback for LLM

**What:** Create structured advisory templates for each anomaly category that produce useful advisories when the LLM is unavailable, timed out, or returned garbage.

**Why:** The prebuild requires that LLM unavailability never blocks advisory generation. Currently if the LLM fails, the pipeline has no structured fallback — advisory quality degrades silently.

**Files to create:**
- `src/strategic/templates.rs` — template functions per `AnomalyCategory`

**Template structure:**
```rust
pub fn template_advisory(
    category: &AnomalyCategory,
    metrics: &DrillingMetrics,
    campaign: &Campaign,
) -> TemplateAdvisory {
    match category {
        AnomalyCategory::Kick => kick_template(metrics, campaign),
        AnomalyCategory::PackOff => pack_off_template(metrics, campaign),
        // ... one template per category
    }
}
```

Each template should produce:
- A human-readable recommendation string with specific parameter values from metrics
- A confidence of 0.70 (reduced from LLM's typical 0.85+)
- A `source: "template"` field so the dashboard can show a banner

**Acceptance criteria:**
- Every `AnomalyCategory` variant has a template
- Templates produce actionable text (not just "anomaly detected")
- Templates include actual metric values ("Torque at 18.5 kft-lb, 23% above baseline")
- Strategic LLM falls back to templates on timeout/error
- Integration test: disable LLM feature flag, verify advisories still generate

### 2.2 Background Self-Healer

**What:** A background tokio task that runs every 30 seconds, checks system component health, and performs automatic recovery where possible.

**Files to create:**
- `src/background/mod.rs` — module root
- `src/background/self_healer.rs` — `SelfHealer` struct + `HealthCheck` trait

**Health checks to implement:**

| Check | Detection | Healing Action |
|---|---|---|
| WITS Connection | No packet received > 30s | Trigger reconnect via `DataSource::reconnect()` |
| LLM Availability | Last inference failed or > 60s timeout | Switch to template mode, log warning |
| Disk Space | `statvfs` shows < 500MB free | Stop writing non-critical logs, raise banner |
| Dashboard Server | Axum bind check | Log error (can't self-heal listener) |
| Baseline State | JSON file corrupted/unreadable | Reset to unlocked state, re-learn |

**Trait shape:**
```rust
#[async_trait]
pub trait HealthCheck: Send + Sync {
    fn component_name(&self) -> &str;
    async fn check(&self) -> HealthStatus;
    async fn heal(&self) -> Result<HealAction, HealError>;
}

pub enum HealthStatus {
    Healthy,
    Degraded { reason: String },
    Unhealthy { reason: String },
}

pub enum HealAction {
    Reconnected,
    FallbackActivated,
    NoActionNeeded,
    ManualInterventionRequired { reason: String },
}
```

**Acceptance criteria:**
- Self-healer runs as a background `tokio::spawn` task
- Never panics — all errors caught and logged
- Health status exposed via `GET /api/v1/health` (extend existing endpoint)
- Dashboard shows degraded/offline banners based on health status
- WITS reconnect tested by killing simulator and restarting

### 2.3 PersistenceLayer Trait

**What:** Abstract current sled-based storage behind a `PersistenceLayer` trait so PostgreSQL can be swapped in later without touching pipeline code.

**Files to modify:**
- `src/storage/mod.rs` — define trait, implement for existing sled backend

**Files to create:**
- `src/storage/in_memory.rs` — `InMemoryDAL` for testing and minimal deployments

**Trait shape:**
```rust
#[async_trait]
pub trait PersistenceLayer: Send + Sync {
    async fn store_advisory(&self, advisory: &StrategicAdvisory) -> Result<()>;
    async fn get_advisory(&self, id: &str) -> Result<Option<StrategicAdvisory>>;
    async fn list_advisories(&self, limit: usize) -> Result<Vec<StrategicAdvisory>>;
    async fn store_ml_report(&self, report: &MLInsightsReport) -> Result<()>;
    async fn get_latest_ml_report(&self) -> Result<Option<MLInsightsReport>>;
}
```

**Acceptance criteria:**
- Existing sled-based storage implements the trait
- InMemoryDAL passes the same test suite
- Pipeline coordinator accepts `Arc<dyn PersistenceLayer>`
- No behavioral regression

---

## Phase 3: Fleet Preparation

**Goal:** Build the data structures, knowledge infrastructure, and local queuing needed for multi-rig learning — without requiring a hub to be running.

**Estimated scope:** ~2000-2500 lines of new code.

### 3.1 Fleet Types

**What:** Define the data structures for fleet events, episodes, and the precedent library.

**Files to create:**
- `src/fleet/mod.rs` — module root + types

**Key types:**
```rust
pub struct FleetEvent {
    pub id: String,
    pub rig_id: String,
    pub timestamp: DateTime<Utc>,
    pub advisory: StrategicAdvisory,
    pub history_window: Vec<(WitsPacket, DrillingMetrics)>,
    pub outcome: EventOutcome,
    pub notes: Option<String>,
}

pub enum EventOutcome {
    Pending,
    Resolved { action_taken: String },
    Escalated { reason: String },
    FalsePositive,
}

pub struct FleetEpisode {
    pub id: String,
    pub category: AnomalyCategory,
    pub campaign: Campaign,
    pub depth_range: (f64, f64),
    pub resolution_summary: String,
    pub outcome: EventOutcome,
    pub embedding: Vec<f32>,
}
```

**Acceptance criteria:**
- Types serialize/deserialize cleanly with serde
- Types are reusable by both fleet client (Phase 4) and knowledge store (3.2)

### 3.2 RAM Recall (HNSW + Metadata Filtering)

**What:** In-memory vector similarity search for fleet episodes, enabling sub-2ms precedent lookup.

**Why:** The prebuild targets 1-2ms for Phase 6 knowledge query. The current static keyword DB is ~8-15ms. HNSW (Hierarchical Navigable Small World) graphs give O(log n) approximate nearest neighbor search.

**Files to create:**
- `src/context/ram_recall.rs` — `RAMRecall` struct implementing `KnowledgeStore`

**Dependencies to add:**
- `instant-distance` or `hnsw_rs` crate for HNSW implementation (evaluate at build time)

**Key design decisions:**
- Maximum 10,000 episodes in memory (~50MB at 512-dim embeddings)
- Metadata pre-filtering: category + campaign before vector search
- Eviction policy: oldest non-critical episodes when memory > 500MB
- Episodes loaded from fleet library sync (Phase 4) or local advisory history

**Acceptance criteria:**
- Implements `KnowledgeStore` trait from Phase 1.1
- Sub-2ms query latency for 1000 episodes (benchmark test)
- Feature-gated behind `ram_recall` cargo feature
- Graceful fallback to `NoOpStore` if feature disabled

### 3.3 Upload Queue (Local Disk)

**What:** A durable queue that stores confirmed AMBER/RED advisories on disk for eventual upload to the fleet hub.

**Why:** The rig must never lose advisory data even if the hub is unreachable for days. This is the local half of the hub-and-spoke architecture.

**Files to create:**
- `src/fleet/queue.rs` — `UploadQueue` struct with disk-backed durability

**Design:**
- Store each `FleetEvent` as a zstd-compressed JSON file in `data/fleet_queue/`
- Files named by advisory ID for idempotent retry
- Queue scans directory on startup to resume pending uploads
- Maximum queue size: configurable, default 1000 events (~50MB)

**Acceptance criteria:**
- Events survive process restart (disk-backed)
- Duplicate detection by advisory ID
- Queue can be drained by fleet client (Phase 4)
- Works without fleet client (just accumulates locally)

---

## Phase 4: Multi-Rig Deployment (Post-Pilot)

**Goal:** Connect rigs to a hub for fleet-wide learning. This phase requires a hub service (separate deployment) and WireGuard VPN between rigs and hub.

**Estimated scope:** ~2500-3000 lines of new code (rig side) + separate hub service.

### 4.1 Fleet Client

**What:** HTTP client that uploads queued events to the hub and downloads the precedent library.

**Files to create:**
- `src/fleet/client.rs` — `FleetClient` struct

**Dependencies to add:**
- `reqwest` (already may be present, or add)
- `zstd` for compression

**Key behaviors:**
- Upload: drain `UploadQueue`, POST compressed events, mark as uploaded on 200
- Sync: GET `/api/fleet/library`, diff against local RAM Recall, ingest new episodes
- Retry: exponential backoff (2s, 4s, 8s, 16s) on HTTP failure, max 4 retries then sleep 15 minutes
- Idempotent: event ID in upload ensures no duplicates at hub
- Auth: WireGuard provides transport security; add simple API key header for tenant isolation

**Acceptance criteria:**
- Upload only AMBER/RED events (skip GREEN/LOW)
- Handles hub unreachability gracefully (queues, retries, doesn't crash)
- Library sync populates RAM Recall with new episodes
- Sync cadence: every 6 hours (configurable)

### 4.2 Fleet Library Sync

**What:** Periodic background task that pulls the precedent library from the hub and updates local RAM Recall.

**Files to create:**
- `src/fleet/sync.rs` — `LibrarySync` struct

**Design:**
- Runs every 6 hours as a background tokio task
- Downloads delta (episodes newer than local watermark)
- Ingests into RAM Recall via `KnowledgeStore` trait
- Logs sync statistics (new episodes, total library size)
- Handles partial sync (if connection drops mid-download)

**Acceptance criteria:**
- Sync is idempotent (re-running adds no duplicates)
- New episodes available for Phase 6 queries within seconds of sync
- Rig operates normally during sync (non-blocking)

### 4.3 PostgreSQL Persistence (Optional)

**What:** Add PostgreSQL as an optional persistence backend for production deployments that need SQL-queryable audit trails, compliance reporting, and fleet hub storage.

**Dependencies to add (feature-gated):**
- `sqlx` with `postgres` and `runtime-tokio` features
- Migration files in `migrations/`

**Files to create:**
- `src/storage/postgres.rs` — implements `PersistenceLayer` trait from Phase 2.3

**Tables:**
- `advisories` — all strategic advisories with full evidence
- `ml_reports` — hourly ML analysis results
- `fleet_events` — uploaded events (hub side)
- `fleet_library` — curated precedent episodes (hub side)
- `audit_log` — acknowledgments, config changes, campaign switches

**Acceptance criteria:**
- Feature-gated: `cargo build --features postgres`
- Migrations run automatically on startup
- Implements same `PersistenceLayer` trait as sled backend
- Query performance acceptable for dashboard history (< 50ms for last 50 advisories)

### 4.4 Hub Service (Separate Binary/Deployment)

**What:** The fleet hub is a separate service (could be a second binary in this workspace or a separate repo) that receives uploads, curates the precedent library, and serves it to rigs.

**This is out of scope for the current codebase** but should be designed with these endpoints:

| Endpoint | Method | Purpose |
|---|---|---|
| `/api/fleet/events` | POST | Receive compressed FleetEvent uploads |
| `/api/fleet/library` | GET | Serve precedent library (delta-capable) |
| `/api/fleet/health` | GET | Hub health status |
| `/api/fleet/rigs` | GET | Connected rig status |

**Decision needed:** Build as a second binary in this workspace (`src/bin/hub.rs`) or as a separate repo? Recommend separate repo for deployment independence.

---

## Dependency Summary

| Phase | New Crate Dependencies | Cargo Features |
|---|---|---|
| 1 | None | None |
| 2 | None | None |
| 3 | `hnsw_rs` or `instant-distance`, `zstd` | `ram_recall` |
| 4 | `reqwest`, `sqlx` (optional) | `fleet`, `postgres` |

---

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Specialist trait refactor breaks voting behavior | Medium | High | Snapshot current specialist outputs as golden test fixtures before refactoring |
| RAM Recall memory pressure on edge hardware | Low | Medium | Hard cap at 10K episodes, eviction policy, feature-gated |
| Fleet sync over satellite causes bandwidth spikes | Medium | Medium | Delta sync, zstd compression, configurable sync interval |
| PostgreSQL adds operational complexity | Medium | Low | Feature-gated, sled remains default for single-rig |
| HNSW crate compatibility with Rust 2021 edition | Low | Medium | Evaluate 2-3 candidate crates before committing |

---

## Build Order Decision Tree

```
Start here:
  |
  v
Is the system running on a single rig?
  |-- YES --> Phase 1 (traits) + Phase 2 (resilience) = production-ready single rig
  |-- NO, planning multi-rig -->
      |
      v
      Is the hub service built?
        |-- NO --> Phase 3 (fleet prep, local queuing)
        |           then build hub service separately
        |-- YES --> Phase 4 (connect rigs to hub)
```

**Recommended sequence for immediate work:** Phase 1 then Phase 2. These improve the single-rig product without requiring any infrastructure changes. Phases 3-4 can be tackled when multi-rig deployment is on the roadmap.

---

## Verification Strategy

Each phase should be verified before moving to the next:

- **Phase 1:** `cargo test`, `cargo clippy`, simulation binary runs identically to current behavior
- **Phase 2:** Kill WITS simulator mid-run → system recovers. Disable LLM feature → templates produce advisories. Fill disk → system degrades gracefully
- **Phase 3:** Load 1000 synthetic episodes → RAM Recall returns relevant precedents in < 2ms. Queue 100 events → restart process → queue intact
- **Phase 4:** Start hub + 2 rigs → events upload → library syncs → Rig B benefits from Rig A's precedent within 6 hours
