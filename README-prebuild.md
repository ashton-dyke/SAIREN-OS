# README.prebuild.merged.md — SAIREN-OS

**Version:** Pre-Build 1.0 (Merged)
**Date:** February 7, 2026
**Canonical backbone:** `Opus-4.6_future-README.md` (structure + requirements)
**Implementation anchors:** `GPT-5.2_future-README.md` (Rust-ish shapes, schemas, API outlines)
**Repo alignment:** This doc references the *current* repo module layout; where a needed file/module is missing it is marked `[NEW]` with a proposed path.

---

## Table of Contents

1. [System Identity](#1-system-identity)
2. [The 10-Phase Pipeline: Packet to Advisory](#2-the-10-phase-pipeline)
3. [Stage 1: Data Ingestion](#3-stage-1-data-ingestion)
4. [Stage 2: Physics Engine](#4-stage-2-physics-engine)
5. [Stage 3: Tactical Agent](#5-stage-3-tactical-agent)
6. [Stage 4: History Buffer](#6-stage-4-history-buffer)
7. [Stage 5: Strategic Agent](#7-stage-5-strategic-agent)
8. [Stage 6: Knowledge Store (Fleet Memory)](#8-stage-6-knowledge-store-fleet-memory)
9. [Stage 7: LLM Reasoning](#9-stage-7-llm-reasoning)
10. [Stage 8: Orchestrator (Specialist Voting)](#10-stage-8-orchestrator-specialist-voting)
11. [Stage 9: Advisory Composition](#11-stage-9-advisory-composition)
12. [Stage 10: Output Layer](#12-stage-10-output-layer)
13. [Background Services](#13-background-services)
14. [Campaign System](#14-campaign-system)
15. [Baseline Learning](#15-baseline-learning)
16. [ML Engine V2.2](#16-ml-engine-v22)
17. [Configuration System](#17-configuration-system)
18. [Trait Architecture (Sentrix Integration)](#18-trait-architecture-sentrix-integration)
19. [Persistence Layer](#19-persistence-layer)
20. [Fleet Network (Multi-Rig)](#20-fleet-network-multi-rig)
21. [Edge Hardware & Deployment](#21-edge-hardware--deployment)
22. [Dashboard & API](#22-dashboard--api)
23. [Enhancement Roadmap (RAM Recall + Pattern Routing)](#23-enhancement-roadmap-ram-recall--pattern-routing)
24. [Data Flow: One Packet, Full Journey](#24-data-flow-one-packet-full-journey)
25. [Failure Modes & Fallbacks](#25-failure-modes--fallbacks)
26. [Build Order & Implementation Phases](#26-build-order--implementation-phases)
27. [Concrete Build-Order Checklist (Repo-Aligned)](#27-concrete-build-order-checklist-repo-aligned)

---

## 1. System Identity

SAIREN-OS is a **deterministic-first, AI-enhanced drilling advisory system**, meaning physics is the ground truth and AI adds context only when physics gates allow escalation. It ingests WITS Level 0 drilling data (typically ~1 Hz), computes physics metrics per packet, and escalates anomalies through a multi-agent pipeline to produce actionable advisories without controlling equipment. It is read-only and designed for air-gapped edge operation, with graceful degradation when higher layers (LLM, fleet memory) are unavailable.

**Repo alignment note:** The current repo is structured as a Rust workspace-style application with modules like `src/types.rs`, `src/agents/*`, `src/pipeline/*`, `src/physics_engine/*`, `src/llm/*`, and `src/api/*`; SAIREN-OS drilling components must be added by extending these modules or introducing `[NEW]` modules.

---

## 2. The 10-Phase Pipeline

Every packet follows the same fixed pipeline, with most packets exiting early (GREEN) and only anomalous packets paying the full latency cost.

```
Phase 1:  WITS Ingestion        → Receive, validate, timestamp
Phase 2:  Physics Engine         → MSE, d-exponent, flow balance, ECD, pit rate
Phase 3:  Tactical Agent         → Threshold check, anomaly detection, ticket gate
Phase 4:  History Buffer         → Store packet in 60-packet ring buffer
Phase 5:  Strategic Verification → Physics-based ticket validation
Phase 6:  Knowledge Query        → "Has any rig seen this before?"
Phase 7:  LLM Reasoning          → Generate natural-language diagnosis
Phase 8:  Orchestrator Voting    → Specialists vote on risk level
Phase 9:  Advisory Composition   → Merge all signals into StrategicAdvisory
Phase 10: Output                 → Dashboard, API, logs, persistence
```

**Latency budget per phase (GPU mode):**

| Phase | Target | Actual | Notes |
|---|---:|---:|---|
| 1 | <5ms | ~2ms | TCP read + parse |
| 2 | <15ms | ~10ms | All physics calculations |
| 3 | <5ms | ~3ms | Threshold comparison |
| 4 | <1ms | <1ms | Ring buffer push |
| 5 | <100ms | ~50ms | Trend regression, history scan |
| 6 | <15ms | 8–15ms (target: 1–2ms with RAM recall) | Vector similarity search |
| 7 | <800ms | ~750ms | Qwen 2.5 7B inference |
| 8 | <5ms | ~2ms | Weighted vote calculation |
| 9 | <5ms | ~2ms | Struct composition |
| 10 | <10ms | ~5ms | HTTP push, file write |

**Total for normal packet (Phases 1–4):** ~16ms
**Total for anomalous packet (all phases):** ~840ms on GPU, ~35s on CPU

**Repo location (pipeline coordinator):** `src/pipeline/coordinator.rs` should remain the orchestration home; drilling SAIREN-OS phases should be expressed as coordinator steps similar to the existing 10-phase coordinator concept.

---

## 3. Stage 1: Data Ingestion

### What/why
The system receives raw WITS Level 0 ASCII records over TCP and converts them into a unified packet struct that downstream components treat as protocol-agnostic. Ingestion must apply a data quality gate (all-zero rejection, range validation, consistency checks) and inject a UTC timestamp to avoid rig clock drift. The connection must be resilient, using timeouts, keepalive, and exponential backoff reconnect with a dashboard-visible degraded/offline state.

### Inputs / outputs
- **Inputs:** TCP stream of WITS Level 0 records (ASCII line-delimited), plus ingestion mode flags (TCP / STDIN / CSV replay).
- **Outputs:** `WitsPacket` with `timestamp`, parsed channels, derived `rig_state`, and `quality` classification suitable for gating downstream logic.

### Invariants / quality gates
- Reject "all-zero" packets silently as dead feed.
- Reject impossible ranges (e.g., negative WOB, absurd ROP, negative SPP), but prefer flagging over rejecting for ambiguous "could be real" inconsistencies (e.g., flow_out=0).
- Inject system UTC timestamp and track `last_packet_time` for health checks/self-heal.

### Key algos
- Rig state classification must be derived deterministically from channel signatures and used to gate physics and ticketing (e.g., only "Drilling" runs MSE to avoid divide-by-zero garbage).
- Exponential backoff reconnect strategy (2s, 4s, 8s, … capped) and max attempt count before alerting.

### Failure / fallback
- If TCP disconnects or times out, attempt reconnect with backoff; keep dashboard/API alive and mark pipeline paused/offline rather than crashing.
- If a packet is invalid, drop or mark `quality=Invalid` so downstream filters can ignore it.

### Repo location
- **Existing:** `src/acquisition/` holds source abstractions and stdin support; extend it to add WITS ingestion.
- **[NEW] Proposed:** `src/acquisition/wits_tcp_source.rs` (TCP read/parse + reconnect).
- **Existing:** `src/types.rs` should host core structs; extend it with `WitsPacket`, `RigState`, `DataQuality`.

### Implementation anchors

#### `WitsPacket` (core canonical ingest struct)
> Add to `src/types.rs` (or split to `[NEW] src/types/wits.rs` if `types.rs` becomes too large).

```rust
use chrono::{DateTime, Utc};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WitsPacket {
    pub timestamp: DateTime<Utc>,
    pub depth: f64,
    pub wob: f64,
    pub rop: f64,
    pub rpm: f64,
    pub torque: f64,
    pub spp: f64,
    pub flow_in: f64,
    pub flow_out: f64,
    pub pit_volume: f64,
    pub mud_weight_in: f64,
    pub mud_weight_out: f64,
    pub gas_total: f64,
    pub h2s: f64,
    pub hook_load: f64,

    // Derived
    pub rig_state: RigState,
    pub quality: DataQuality,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DataQuality { Good, Suspect, Invalid }

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RigState { Drilling, Circulating, Tripping, Connection, Idle }
```

#### `DataSource` abstraction + WITS TCP reconnect
> Prefer merging into the existing acquisition abstraction (`src/acquisition/sensors.rs`) to keep "sources" consistent across modes.

```rust
use async_trait::async_trait;

#[async_trait]
pub trait DataSource: Send + Sync {
    async fn next_packet(&mut self) -> Result<WitsPacket, IngestionError>;
    async fn reconnect(&mut self) -> Result<(), IngestionError>;
    fn health_check(&self) -> HealthStatus;
    fn source_type(&self) -> &'static str;
}
```

```rust
pub struct WitsTcpSource {
    tcp_stream: Option<tokio::net::TcpStream>,
    endpoint: String,
    buffer: Vec<u8>,
    reconnect_attempts: u32,
    last_packet_time: std::time::Instant,
}

#[async_trait]
impl DataSource for WitsTcpSource {
    async fn next_packet(&mut self) -> Result<WitsPacket, IngestionError> {
        // read line, parse WITS L0 record(s) into fields, apply gates
        // classify rig state, set timestamp, set quality, return WitsPacket
        todo!()
    }

    async fn reconnect(&mut self) -> Result<(), IngestionError> {
        self.tcp_stream = None;
        self.reconnect_attempts += 1;

        let delay = std::cmp::min(2_u64.pow(self.reconnect_attempts), 60);
        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;

        let stream = tokio::net::TcpStream::connect(&self.endpoint)
            .await
            .map_err(IngestionError::ConnectionFailed)?;

        self.tcp_stream = Some(stream);
        self.reconnect_attempts = 0;
        Ok(())
    }

    fn source_type(&self) -> &'static str { "WITS-TCP" }
    fn health_check(&self) -> HealthStatus { todo!() }
}
```

---

## 4. Stage 2: Physics Engine

### What/why
Every validated packet triggers deterministic drilling physics calculations, which are treated as ground truth for gating and explainability. Core metrics include MSE, d-exponent (and corrected variant), ECD + margin to fracture gradient, flow balance, pit rate smoothing, and dysfunction detectors (stick-slip, pack-off, founder, washout). Physics must be gated by `rig_state` to avoid nonsensical values during connections/tripping (e.g., ROP≈0).

### Inputs / outputs
- **Inputs:** `WitsPacket` + `WellConfig` (bit diameter, mud weight normals, fracture gradient) + short history window for smoothing.
- **Outputs:** `DrillingMetrics` struct carried forward into ticketing, reasoning, and persistence.

### Invariants / quality gates
- MSE calculations must avoid divide-by-zero by gating on drilling state and minimum ROP threshold.
- Pit rate must be smoothed (rolling average) to reduce false alarms from transfers/noise.
- Detectors must be explainable (thresholded signatures) and must not "invent" anomalies when inputs are invalid.

### Key algos
- **MSE:** deterministic energy model using bit area and rotary component; interpret vs target range.
- **d-exponent:** formation/pore-pressure indicator with corrected form using mud weights.
- **Stick-slip:** torque coefficient of variation (CV) over rolling window > 12% as signature.
- **Pack-off:** torque + SPP rising together with ROP decline signature.
- **Founder:** trend-based confirmation using regression on WOB and ROP (tactical quick check + strategic regression).
- **Washout:** flow_out drop while WOB/ROP stable.

### Failure / fallback
- If config is missing critical fields (bit diameter, gradients), physics must fail closed (no advisory escalation) or degrade to metrics that don't require those fields, with explicit degraded banners.
- If history is insufficient for a detector (e.g., <N samples), detectors must return "not enough data" rather than noisy guesses.

### Repo location
- **Existing:** `src/physics_engine/` exists and is the natural home for deterministic calculations.
- **[NEW] Proposed:** `src/physics_engine/drilling/mod.rs`, `src/physics_engine/drilling/metrics.rs`, `src/physics_engine/drilling/detectors.rs`.
- **Existing (types):** add `DrillingMetrics` into `src/types.rs` (or `[NEW] src/types/drilling.rs`).

### Implementation anchors

#### `PhysicsEngine` + `DrillingMetrics`
```rust
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct PhysicsEngine {
    pub baseline: Arc<RwLock<BaselineState>>,
    pub config: Arc<WellConfig>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DrillingMetrics {
    pub mse: f64,
    pub mse_efficiency: f64,
    pub d_exponent: f64,
    pub ecd: f64,
    pub ecd_margin: f64,
    pub flow_balance: f64,
    pub pit_rate: f64,

    pub is_kick_warning: bool,
    pub is_loss_warning: bool,
    pub is_pack_off: bool,
    pub is_stick_slip: bool,
    pub is_founder: bool,
    pub is_washout: bool,

    pub severity: Severity,
    pub category: Category,
}
```

#### MSE calculation (division gating)
```rust
fn calculate_mse(config: &WellConfig, p: &WitsPacket) -> f64 {
    let d = config.bit_diameter_inches;
    let bit_area = std::f64::consts::PI * (d / 2.0).powi(2);

    let wob_component = (p.wob * 1000.0) / bit_area;

    let rotary_component = if p.rop > 0.1 {
        (120.0 * p.rpm * p.torque) / (bit_area * p.rop)
    } else {
        0.0
    };

    wob_component + rotary_component
}
```

#### Example detector: pack-off signature (trend-based)
```rust
fn detect_pack_off(current: &WitsPacket, history: &VecDeque<WitsPacket>) -> bool {
    if history.len() < 10 { return false; }

    let baseline_idx = history.len().saturating_sub(60);
    let baseline = &history[baseline_idx];

    let torque_increase = (current.torque - baseline.torque) / baseline.torque * 100.0;
    let spp_increase = current.spp - baseline.spp;
    let rop_decrease = (baseline.rop - current.rop) / baseline.rop * 100.0;

    torque_increase > 15.0 && spp_increase > 100.0 && rop_decrease > 20.0
}
```

---

## 5. Stage 3: Tactical Agent

### What/why
The Tactical Agent is the gatekeeper that runs on every packet, compares physics outputs against thresholds/baselines, and decides whether to escalate by creating an advisory ticket. It also classifies operation mode (drilling vs circulating vs special P&A modes) because thresholds and weights change per operation/campaign. The Tactical Agent is intentionally sensitive, relying on Strategic verification to filter false positives.

### Inputs / outputs
- **Inputs:** `(WitsPacket, DrillingMetrics)` + current configuration thresholds and baseline state.
- **Outputs:** either "Continue" (no ticket) or `AdvisoryTicket` for Strategic verification.

### Invariants / quality gates
- Ticket rate should be low (target: <1% AMBER/RED tickets) to protect downstream latency budget and avoid alert fatigue.
- Suppress ticketing outside valid operational states for the metric (e.g., MSE optimization suppressed outside drilling).

### Key algos
- Sustained anomaly counters for WARNING conditions to avoid one-sample spikes.
- Immediate escalation for CRITICAL safety breaches (flow balance/pit gain thresholds).

### Failure / fallback
- If baseline learning is not locked, fall back to config thresholds rather than blocking operation.
- If config validation fails, run in a "safe defaults" mode with explicit banner and no write-back.

### Repo location
- **Existing:** `src/agents/tactical.rs` for fast gatekeeping logic.
- **Existing:** `src/types.rs` for `AdvisoryTicket`, enums, severity/category.

### Implementation anchors

```rust
pub struct TacticalAgent {
    pub physics_engine: Arc<PhysicsEngine>,
    // optional future: pattern matcher routing
}

pub enum TacticalResult {
    Continue,
    EscalateToStrategic(AdvisoryTicket),
}

impl TacticalAgent {
    pub async fn analyze(
        &self,
        packet: &WitsPacket,
        history: &std::collections::VecDeque<WitsPacket>,
    ) -> TacticalResult {
        let metrics = self.physics_engine.analyze(packet, history);

        if metrics.severity == Severity::Green {
            return TacticalResult::Continue;
        }

        let ticket = AdvisoryTicket::from_packet_and_metrics(packet, metrics, history);
        TacticalResult::EscalateToStrategic(ticket)
    }
}
```

> **Repo fit:** implement `AdvisoryTicket::from_packet_and_metrics(...)` in `src/types.rs` or `[NEW] src/strategic/ticket.rs` to keep constructors centralized.

---

## 6. Stage 4: History Buffer

### What/why
All packets are stored in a fixed-size ring buffer (60 packets ≈ 60 seconds at 1 Hz) to provide temporal context for verification, regression, and smoothing. The buffer is small enough to remain cache-friendly and large enough for simple regression significance. History storage is part of the deterministic core and must not block the hot path.

### Inputs / outputs
- **Inputs:** `(WitsPacket, DrillingMetrics)` pairs per packet.
- **Outputs:** bounded context windows for Phase 5–7 computations (verification, recall query context, LLM prompts).

### Invariants / quality gates
- Buffer operations must be O(1) and non-allocating in steady-state.
- Capacity must be configurable but default to 60 packets for Phase 5 regression assumptions.

### Key algos
- Linear regression helper on a sliding window for trend detection (slope, intercept, R²).

### Failure / fallback
- If buffer is empty/too small, strategic verification returns `UNCERTAIN` rather than fabricating trends.

### Repo location
- **Existing:** `src/history_storage.rs` and `src/storage/history.rs` exist; use one as canonical for the in-memory ring buffer and keep persistence concerns separate.
- **Suggested:** keep hot ring buffer in `src/history_storage.rs` and optional persistence in `src/storage/history.rs`.

### Implementation anchors
```rust
use std::collections::VecDeque;

pub struct HistoryBuffer {
    pub buffer: VecDeque<(WitsPacket, DrillingMetrics)>,
    pub capacity: usize,  // default 60
}

impl HistoryBuffer {
    pub fn push(&mut self, packet: WitsPacket, metrics: DrillingMetrics) {
        if self.buffer.len() >= self.capacity {
            self.buffer.pop_front();
        }
        self.buffer.push_back((packet, metrics));
    }

    pub fn trend(&self, _metric: &str) -> TrendResult {
        // regression helper: slope, intercept, r²
        todo!()
    }
}
```

---

## 7. Stage 5: Strategic Agent

### What/why
Strategic Agent performs deep verification to answer "is this real or noise," filtering Tactical sensitivity and preventing dashboard spam. Verification reruns/aggregates physics over the 60-packet history window, confirming sustained trends and rejecting transient spikes. Outcomes are `CONFIRM`, `REJECT`, or `UNCERTAIN`, and only `CONFIRM` proceeds to knowledge/LLM/voting.

### Inputs / outputs
- **Inputs:** `AdvisoryTicket` + history window + config/baseline context.
- **Outputs:** `VerificationResult` (confirmed/uncertain/rejected) plus justification string used downstream for explainability.

### Invariants / quality gates
- Strategic verification must be deterministic and explainable, producing explicit reasons for each decision.
- Rejection rate is a feature (meaningful rejects keep Tactical gates sensitive without increasing false alarm rate).

### Key algos
- Sustained signature checks (e.g., kick signature requires sustained flow imbalance and pit gain).
- Regression confirmation for pack-off (torque+SPP slopes positive) and founder (WOB slope positive while ROP slope non-positive).

### Failure / fallback
- If inputs lack sufficient history, return `UNCERTAIN` with "insufficient context" and do not generate advisory.
- If a detector's prerequisites fail (e.g., unreliable flow sensors), degrade to available signals and lower confidence.

### Repo location
- **Existing:** `src/agents/strategic.rs` should host `verify_ticket()` style logic.
- **Existing:** `src/strategic/aggregation.rs` may host helper computations; prefer keeping verification core in `agents/strategic.rs` and reuse helpers.

### Implementation anchors

```rust
pub enum VerificationResult {
    Confirmed { confidence: f64, reason: String },
    Uncertain { confidence: f64, reason: String },
    Rejected { reason: String },
}

pub struct StrategicAgent {
    pub physics_engine: Arc<PhysicsEngine>,
}

impl StrategicAgent {
    pub fn verify_ticket(&self, ticket: &AdvisoryTicket) -> VerificationResult {
        // double-check: sustained signature using ticket.history_snapshot
        // return Confirmed/Rejected/Uncertain with reason
        todo!()
    }
}
```

---

## 8. Stage 6: Knowledge Store (Fleet Memory)

### What/why
For a single-rig pilot, the knowledge store may be a NoOp that skips to LLM reasoning, but fleet mode queries historical episodes for precedent. Knowledge recall is used to ground recommendations in prior outcomes and to boost confidence when similar cases resolved with specific actions. Roadmap includes RAM Recall (in-memory HNSW + metadata filtering) to reduce recall latency from external DB calls.

### Inputs / outputs
- **Inputs:** confirmed ticket + embedding query and filters (campaign, category, formation, depth range, outcome).
- **Outputs:** top-k similar `FleetEpisode` / `FleetEvent` objects (with resolution + outcome metadata).

### Invariants / quality gates
- Do not use false positives as precedent unless explicitly requested; filter on outcomes like Resolved/Escalated.
- Query must be fast enough not to break anomaly latency budget (targets 1–2ms with RAM Recall).

### Key algos
- Build a query text/features from ticket context, embed it, then run similarity search with metadata pre-filtering.
- Keep a NoOp implementation for pilot mode.

### Failure / fallback
- If store is unhealthy, fall back to `NoOpStore` and proceed without precedent, reducing confidence and/or switching to template-only reasoning if needed.

### Repo location
- **Existing:** `src/context/vector_db.rs` is the current "embedded knowledge base" location and should be extended into a generalized `KnowledgeStore` interface.
- **[NEW] Proposed:** `src/context/knowledge_store.rs` (trait + impls), `src/context/ram_recall.rs` (HNSW + metadata indices).

### Implementation anchors

```rust
pub struct KnowledgeStore {
    pub ram_recall: std::sync::Arc<tokio::sync::RwLock<RAMRecall>>,
}

impl KnowledgeStore {
    pub async fn query_precedent(&self, ticket: &AdvisoryTicket) -> anyhow::Result<Vec<FleetEpisode>> {
        let query_text = format!(
            "campaign:{} category:{:?} depth:{:.0} formation:{} flow_balance:{:.1}",
            ticket.campaign, ticket.metrics.category, ticket.depth, ticket.formation, ticket.metrics.flow_balance
        );

        let query_embedding = self.embed_text(&query_text).await?;

        let recall = self.ram_recall.read().await;
        let similar = recall.search_similar(
            &query_embedding,
            &ticket.campaign.to_string(),
            &ticket.metrics.category.to_string(),
            5,
        );

        Ok(similar)
    }
}
```

---

## 9. Stage 7: LLM Reasoning

### What/why
The LLM synthesizes deterministic physics verdict + optional fleet precedent + campaign/operation context into human-readable diagnosis and specific parameter recommendations. The system must auto-select models based on hardware (GPU vs CPU vs none) and support template fallback when LLM is unavailable. LLM output must be parsed into structured advisory fields, not treated as freeform truth, and must not override physics gating.

### Inputs / outputs
- **Inputs:** confirmed ticket + verification reason + precedent list + context (formation/depth/campaign/operation) + prompt templates.
- **Outputs:** `LLMRecommendation` with text + confidence + reasoning fields for downstream composition.

### Invariants / quality gates
- LLM must only run on confirmed anomalies (or explicit "analysis requested" mode) to protect latency and avoid noise.
- LLM unavailability must not block advisories; templates must exist for core dysfunctions.

### Key algos
- Context builder that merges physics snapshot and precedent summaries into a prompt.
- Hardware-based backend selection and a uniform `InferenceBackend` trait boundary.

### Failure / fallback
- If inference times out or model fails to load, fall back to templates and reduce advisory confidence.

### Repo location
- **Existing:** `src/llm/` already contains tactical/strategic LLM modules and backend (`mistral_rs.rs`) plus scheduler.
- **Action:** adapt `src/llm/strategic_llm.rs` interface to accept `AdvisoryTicket` and return a structured result for drilling advisories.

### Implementation anchors

```rust
pub struct LLMReasoning {
    pub tactical_llm: std::sync::Arc<TacticalLLM>,
    pub strategic_llm: std::sync::Arc<StrategicLLM>,
}

pub struct LLMRecommendation {
    pub text: String,
    pub confidence: f64,
    pub reasoning: String,
}

impl LLMReasoning {
    pub async fn generate_recommendation(
        &self,
        ticket: &AdvisoryTicket,
        verification: &VerificationResult,
        fleet_precedent: &[FleetEpisode],
    ) -> anyhow::Result<LLMRecommendation> {
        // build prompt, infer, parse
        todo!()
    }
}
```

---

## 10. Stage 8: Orchestrator (Specialist Voting)

### What/why
Specialist voting provides holistic risk assessment across multiple domains and maintains explainability by showing each specialist's reasoning and weight. Weights are campaign-configurable and must prioritize safety (Well Control highest weight). Voting combines deterministic evaluations into a consensus risk level and composite confidence.

### Inputs / outputs
- **Inputs:** confirmed ticket + metrics + verification context + optional precedent summary.
- **Outputs:** `WeightedConsensus` with final `risk_level`, per-specialist votes, and overall confidence.

### Invariants / quality gates
- Weights must sum ~1.0 and be validated at startup/config updates.
- Specialist votes must be reproducible and must not depend on LLM output unless explicitly modeled as a specialist with bounded influence.

### Key algos
- Weighted aggregation over specialist votes to compute consensus risk level.
- Campaign-aware weight adjustment (e.g., P&A increases Well Control weight).

### Failure / fallback
- If a specialist can't evaluate due to missing signals, it must return an "Unknown/Low-confidence" vote and the system must adjust confidence accordingly rather than panicking.

### Repo location
- **Existing:** `src/agents/orchestrator.rs` is the home for ensemble voting patterns.
- **[NEW] Proposed:** `src/agents/specialists/` to keep drilling specialists separate from existing vibration specialists (if present).

### Implementation anchors

```rust
pub trait Specialist {
    fn name(&self) -> &str;
    fn evaluate(&self, ticket: &AdvisoryTicket) -> SpecialistVote;
}

pub struct SpecialistVote {
    pub risk_level: RiskLevel,
    pub confidence: f64,
    pub reason: String,
}

pub struct Orchestrator {
    pub specialists: Vec<Box<dyn Specialist>>,
    pub weights: std::collections::HashMap<String, f64>,
}

impl Orchestrator {
    pub fn vote(&self, ticket: &AdvisoryTicket) -> WeightedConsensus {
        // weighted risk aggregation + confidence
        todo!()
    }
}
```

---

## 11. Stage 9: Advisory Composition

### What/why
Advisory composition merges verification verdict, specialist votes, LLM recommendation, and fleet precedent into a single `StrategicAdvisory` that is durable, explainable, and ready for output/persistence. A cooldown prevents CRITICAL spam (30-second cooldown for repeat criticals). The advisory struct must support acknowledgment/action/outcome tracking for audit and learning loops.

### Inputs / outputs
- **Inputs:** ticket + verification + precedent + llm recommendation + consensus vote + campaign/operation context.
- **Outputs:** `StrategicAdvisory` persisted and broadcast to dashboard/API.

### Invariants / quality gates
- Advisory must include evidence fields (physics verdict + votes + precedent summary) to maintain operator trust.
- Advisory ID must be unique and stable across persistence and upload payloads.

### Key algos
- Compose function that maps consensus risk → advisory type and includes expected benefit estimation.

### Failure / fallback
- If LLM is unavailable, composition must still produce advisory using templates and deterministic evidence.

### Repo location
- **Existing:** `src/strategic/parsing.rs` suggests a structured-output parsing layer; composition should live nearby.
- **[NEW] Proposed:** `src/strategic/advisory.rs` (struct + composer), `src/strategic/templates.rs` (template fallback text).

### Implementation anchors

```rust
pub struct AdvisoryComposer;

impl AdvisoryComposer {
    pub fn compose(
        &self,
        ticket: &AdvisoryTicket,
        verification: &VerificationResult,
        fleet_precedent: &[FleetEpisode],
        llm_rec: &LLMRecommendation,
        voting: &WeightedConsensus,
    ) -> StrategicAdvisory {
        StrategicAdvisory {
            id: format!("ADV-{}", uuid::Uuid::new_v4().to_string()[..8].to_string()),
            timestamp: chrono::Utc::now(),
            risk_level: voting.risk_level,
            category: ticket.metrics.category,
            recommendation: llm_rec.text.clone(),
            expected_benefit: self.estimate_benefit(ticket, llm_rec),
            physics_verdict: verification_reason(verification),
            fleet_precedent_summary: self.summarize_precedent(fleet_precedent),
            specialist_votes: voting.votes.clone(),
            confidence: voting.confidence,
            // + ack/outcome fields
        }
    }
}
```

---

## 12. Stage 10: Output Layer

### What/why
Outputs must deliver advisories quickly to the rig-local dashboard and provide an API for status, history, verification, baseline progress, configuration, and acknowledgments. Output should include local logs (CSV/JSON) for post-well analysis and optional database persistence (PostgreSQL future). The dashboard is role-based (Driller tactical / Company Man strategic / Engineering deep dive).

### Inputs / outputs
- **Inputs:** `StrategicAdvisory` + latest metrics/status caches + persistence handles.
- **Outputs:** HTTP API responses, websocket/SSE pushes, local logs, optional DB records.

### Invariants / quality gates
- API must return 204 for "no current advisory" to keep polling cheap and explicit.
- Acknowledge endpoint must create an audit trail linking advisory ID to actor and action_taken.

### Failure / fallback
- If persistence backends are down (Postgres/Redis), outputs still function using in-memory state and file logs.
- If disk is full, stop writing logs but keep core pipeline running with explicit health banner.

### Repo location
- **Existing:** `src/api/routes.rs` + `src/api/handlers.rs` are the canonical API surfaces.
- **Existing:** `src/storage/` for storing history/strategic reports (currently Sled-based in repo).
- **[NEW] Proposed:** `src/api/drilling_routes.rs` if you want to keep drilling endpoints separate from existing machinery endpoints.

### Implementation anchors

```rust
// NOTE: Repo uses an API module with routes/handlers; map these endpoints accordingly.
#[get("/api/v1/diagnosis")]
async fn get_diagnosis(state: actix_web::web::Data<AppState>) -> impl actix_web::Responder {
    let diagnosis = state.latest_advisory.read().await;
    match diagnosis.as_ref() {
        Some(advisory) => actix_web::HttpResponse::Ok().json(advisory),
        None => actix_web::HttpResponse::NoContent().finish(),
    }
}

#[post("/api/v1/advisory/acknowledge")]
async fn acknowledge_advisory(
    req: actix_web::web::Json<AcknowledgeRequest>,
    state: actix_web::web::Data<AppState>,
) -> impl actix_web::Responder {
    // update in-memory + persist via storage layer
    actix_web::HttpResponse::Ok().finish()
}
```

---

## 13. Background Services

### What/why
Background services run independently of the hot packet pipeline and handle hourly ML optimization, baseline learning progression, and health/self-heal checks. Self-healer monitors WITS connection, LLM availability, disk space, dashboard serving, and optional fleet/persistence backends, performing automatic reconnection/restarts and template fallback.

### Inputs / outputs
- **Inputs:** system health state, last packet time, backend handles, ML windows of packets.
- **Outputs:** health banners, reconnection actions, periodic reports (ML), and updated caches.

### Invariants / quality gates
- Background tasks must never block the hot path; they run asynchronously on their own cadence.
- Healing actions must be bounded (backoff, max retries) to avoid thrashing.

### Failure / fallback
- If LLM fails, switch to templates; if hub is unreachable, queue uploads for retry; if recall store is unhealthy, fall back to NoOp.

### Repo location
- **Existing:** `src/director/llm_director.rs` suggests orchestration; background service orchestration can live alongside.
- **[NEW] Proposed:** `src/background/mod.rs`, `src/background/self_healer.rs`, `src/background/ml_scheduler.rs`.

### Implementation anchors (self-healer)

```rust
#[async_trait::async_trait]
pub trait HealthCheck: Send + Sync {
    async fn check(&self) -> HealthResult;
    async fn heal(&self) -> anyhow::Result<()>;
}

pub struct WitsConnectionCheck {
    pub last_packet_time: std::sync::Arc<tokio::sync::RwLock<std::time::Instant>>,
    pub wits_source: std::sync::Arc<tokio::sync::Mutex<Box<dyn DataSource>>>,
}

#[async_trait::async_trait]
impl HealthCheck for WitsConnectionCheck {
    async fn check(&self) -> HealthResult {
        let last_packet = *self.last_packet_time.read().await;
        if last_packet.elapsed() > std::time::Duration::from_secs(30) {
            HealthResult::Unhealthy { component: "WITS Connection".into(), reason: "No data >30s".into() }
        } else {
            HealthResult::Healthy
        }
    }

    async fn heal(&self) -> anyhow::Result<()> {
        let mut source = self.wits_source.lock().await;
        source.reconnect().await?;
        Ok(())
    }
}
```

---

## 14. Campaign System

### What/why
Campaigns change the system's personality (thresholds, weights, prompts, and ML objective weights) for different operational priorities like Production vs Plug & Abandonment. Campaign switches must be immediate and auditable via dashboard/API/environment variable.

### Available Campaigns

| Campaign | Focus | Flow Warning | Flow Critical | Well Control Weight |
|---|---|---|---|---|
| **Production** | ROP optimisation, MSE efficiency | 10 gpm | 20 gpm | 30% |
| **Plug & Abandonment** | Cement integrity, pressure testing | 5 gpm | 15 gpm | 40% |

### What changes per campaign
1. **Thresholds:** P&A has tighter flow balance thresholds because cement operations are more sensitive.
2. **Specialist weights:** P&A increases Well Control weight to 40% (from 30%) because barrier integrity is paramount.
3. **LLM prompts:** Campaign context is injected into LLM prompts so recommendations are contextually appropriate.
4. **ML optimisation weights:** P&A prioritises stability over ROP, whereas Production prioritises ROP.
5. **Operation detection:** Milling and Cement Drill-Out operations are only detected in P&A mode.

### Repo location
- **[NEW] Proposed:** `src/campaign/mod.rs` (campaign enum + override application), or embed into existing config module if one exists.

---

## 15. Baseline Learning

### What/why
Every rig is different. Baseline learning adapts thresholds to the specific rig by collecting initial samples during normal operation and locking mean/stddev-based thresholds. During baseline learning the system runs in shadow mode (computes everything but generates no alerts) to prevent operational impact. Baseline state persists to `data/baseline_state.json` for crash recovery.

### Inputs / outputs
- **Inputs:** stream of `WitsPacket`/metrics during normal drilling states.
- **Outputs:** locked thresholds (warning/critical) and persisted baseline state file.

### Invariants / quality gates
- Do not alert during baseline learning; only after lock.
- Persist locked metrics incrementally to survive restarts cleanly.

### Repo location
- **[NEW] Proposed:** `src/baseline/mod.rs` and persistence in `data/baseline_state.json`.

---

## 16. ML Engine V2.2

### What/why
The ML engine runs hourly on batches of recent packets to find optimal parameters per formation segment, using dysfunction filtering so it learns only from stable operations. It segments formations using d-exponent shifts and scores WOB×RPM bins with campaign-aware weights over ROP, MSE, and stability.

### Inputs / outputs
- **Inputs:** up to ~2 hours of packets (1 Hz) filtered by quality and drilling state.
- **Outputs:** `MLReport` / formation-optimal parameter ranges and confidence metadata.

### Invariants / quality gates
- Dysfunction filtering must remove stick-slip, pack-off, founder, and poor efficiency samples before computing optima.
- Grid-based binning must ensure recommended parameters are "co-observed" combinations rather than mixing top-percentile points from different regimes.

### Repo location
- **[NEW] Proposed:** `src/ml_engine/mod.rs`, `src/ml_engine/segmentation.rs`, `src/ml_engine/grid.rs`.
- **Existing:** `src/strategic/aggregation.rs` may host report aggregation; keep ML engine separate to avoid mixing real-time and batch semantics.

### Implementation anchors

```rust
pub struct MLEngine {
    pub campaign: Campaign,
}

impl MLEngine {
    pub async fn run_hourly_analysis(&self, packets: &[WitsPacket]) -> MLReport {
        // quality filtering → dysfunction filtering → formation segmentation → grid binning → scoring
        todo!()
    }
}
```

---

## 17. Configuration System

### What/why
All thresholds and weights must be configurable via TOML with a defined hierarchy: env var path → local `./well_config.toml` → built-in defaults. Config must validate ordering (critical > warning), weight sums (~1.0), and physically reasonable ranges, and must support runtime updates via API with validate-only option.

### Inputs / outputs
- **Inputs:** TOML + runtime PATCH updates via API.
- **Outputs:** validated config applied to next packet and logged to audit.

### Repo location
- **[NEW] Proposed:** `src/config/mod.rs`, `src/config/validate.rs`, with `well_config.toml` at repo root.

---

## 18. Trait Architecture (Sentrix Integration)

### What/why
Every major boundary becomes a trait to enable swappable backends, testability, and graceful degradation. The five core traits are `DataSource`, `InferenceBackend`, `KnowledgeStore`, `PersistenceLayer`, and `CacheLayer`, each with pilot and fleet implementations.

### Repo location
- **Existing:** the repo already has modular boundaries in `acquisition/`, `llm/`, `context/`, and `storage/`; codify them as explicit traits in those modules.
- **[NEW] Proposed:** `src/traits/` is optional; prefer colocating traits with their domain modules to avoid "trait dumping ground."

---

## 19. Persistence Layer

### What/why
Pilot can run with in-memory + file logs and baseline JSON persistence, but long-term needs PostgreSQL for advisories, ML reports, audit logs, and fleet payloads, plus Redis for hot caches. Persistence must be optional and have fallback implementations so core pipeline never depends on database uptime.

### Inputs / outputs
- **Inputs:** advisories, ML reports, audit events, metrics.
- **Outputs:** durable storage and query support for dashboard/history and fleet packaging.

### Repo location
- **Existing:** `src/storage/strategic.rs` and `src/storage/history.rs` exist for storage concerns and can host a `PersistenceLayer` trait + in-memory/Sled impl.
- **[NEW] Proposed:** `src/storage/postgres.rs` + migrations folder if Postgres is enabled.

---

## 20. Fleet Network (Multi-Rig)

### What/why
Fleet is hub-and-spoke over encrypted WireGuard VPN, uploading only confirmed AMBER/RED events (compressed windows + metadata) and syncing a compact precedent library every 6 hours. Fleet must not require continuous streaming; only rare events are uploaded to keep bandwidth low.

### Inputs / outputs
- **Inputs:** confirmed advisories + sensor windows + outcomes/notes.
- **Outputs:** compressed uploads to hub and periodic library sync downloads.

### Failure / fallback
- If hub is unreachable, queue uploads on disk and retry periodically; local rig remains autonomous.

### Repo location
- **[NEW] Proposed:** `src/fleet/mod.rs`, `src/fleet/client.rs`, `src/fleet/queue.rs`, `src/fleet/sync.rs`.

### Implementation anchors (upload + sync)

```rust
pub struct FleetClient {
    pub hub_url: String,
}

impl FleetClient {
    pub async fn upload_event(
        &self,
        advisory: &StrategicAdvisory,
        history: &[WitsPacket],
    ) -> anyhow::Result<()> {
        if advisory.risk_level == RiskLevel::Low {
            return Ok(());
        }

        let event = FleetEvent {
            rig_id: advisory.rig_id.clone(),
            timestamp: advisory.timestamp,
            advisory: advisory.clone(),
            history_window: history.to_vec(),
            outcome: EventOutcome::Pending,
        };

        let compressed = zstd::encode_all(serde_json::to_vec(&event)?.as_slice(), 3)?;
        reqwest::Client::new()
            .post(format!("{}/api/fleet/events", self.hub_url))
            .header("Content-Encoding", "zstd")
            .body(compressed)
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }
}
```

```rust
pub async fn sync_fleet_library(
    hub_url: &str,
    local_recall: &mut RAMRecall,
) -> anyhow::Result<usize> {
    let library: FleetLibrary = reqwest::get(format!("{}/api/fleet/library", hub_url))
        .await?
        .json()
        .await?;

    let mut count = 0;
    for episode in library.episodes {
        if !local_recall.has_episode(&episode.id) {
            local_recall.add_episode(episode)?;
            count += 1;
        }
    }

    Ok(count)
}
```

---

## 21. Edge Hardware & Deployment

Keep edge deployment posture: edge box, air-gapped, systemd service, no rig control, ingress-only data feed. Where the repo already provides a `--addr` and LLM model path env vars, extend CLI flags for `--wits-tcp`, `--stdin`, and `--csv` replay modes consistent with ingestion modes.

---

## 22. Dashboard & API

### What/why
Dashboard is the primary UI and must present role-based views with increasing detail (Driller tactical minimalism, Company Man full reasoning, Engineering deep dive). API must expose health/status/drilling/diagnosis/history/verification/baseline/campaign/config/acknowledge and report endpoints.

### API Endpoint Summary

| Endpoint | Method | Returns |
|---|---|---|
| `/api/v1/health` | GET | System health status |
| `/api/v1/status` | GET | Current metrics, operation, campaign |
| `/api/v1/drilling` | GET | Live drilling metrics |
| `/api/v1/diagnosis` | GET | Current strategic advisory (204 if none) |
| `/api/v1/history` | GET | Last 50 advisories |
| `/api/v1/verification` | GET | Latest ticket verification result |
| `/api/v1/baseline` | GET | Baseline learning progress |
| `/api/v1/campaign` | GET/POST | Current campaign / switch campaign |
| `/api/v1/config` | GET/POST | Well configuration / update thresholds |
| `/api/v1/config/validate` | POST | Validate config without applying |
| `/api/v1/advisory/acknowledge` | POST | Acknowledge advisory (audit trail) |
| `/api/v1/shift/summary` | GET | Shift summary with time-range filter |
| `/api/v1/ml/latest` | GET | Latest ML insights |
| `/api/v1/ml/optimal?depth=N` | GET | Optimal parameters for depth |
| `/api/v1/reports/critical` | GET | Critical advisory reports |

### Repo location
- **Existing:** `src/api/routes.rs` and `src/api/handlers.rs` should be the only place adding new endpoints.

---

## 23. Enhancement Roadmap (RAM Recall + Pattern Routing)

Keep roadmap: RAM Recall (HNSW + metadata indices), then pattern-matched routing with outcome feedback, then fleet hardening. Implement RAM Recall under the KnowledgeStore boundary so external vector DB vs in-memory swap is transparent to the rest of the pipeline.

---

## 24. Data Flow: One Packet, Full Journey

Preserve the "one packet" journey as the canonical narrative and ensure each step maps to coordinator phases and module calls. When implementing, ensure that the coordinator logs per-phase timings so the latency table remains measurable and enforceable.

---

## 25. Failure Modes & Fallbacks

| Failure Mode | Detection | Recovery | Impact |
|---|---|---|---|
| **WITS disconnect** | No packets >30s | Reconnect with exponential backoff (2s–60s cap); max retries then alert dashboard | Pipeline paused; dashboard shows OFFLINE banner |
| **LLM unavailable** | Inference timeout or model load failure | Fall back to template-based advisories; reduce confidence to 0.70 | Advisories less nuanced but functional |
| **GPU failure** | CUDA init error or OOM | Fall back to CPU inference or template mode | Higher latency (~35s) or template-only mode |
| **Knowledge store down** | Health check fail | Fall back to `NoOpStore`/`InMemoryStore` | No fleet precedent; confidence reduced |
| **PostgreSQL/Redis down** | Connection timeout | Use in-memory state + file logs | No durable persistence; dashboard still works |
| **Hub unreachable** | Upload HTTP error | Queue events on disk; retry every 15 minutes | Fleet learning delayed; local operation unaffected |
| **Disk full** | OS check | Stop writing logs; keep core pipeline running | Advisory logs lost; health banner shown |
| **Baseline not locked** | Learning state check | Use config-defined thresholds as fallback | May be less rig-specific; explicit banner |
| **RAM Recall full** | Memory usage >500MB | Evict oldest non-critical episodes | Transparent to operators |

---

## 26. Build Order & Implementation Phases

Phased plan:
1. **Pilot-first trait extraction** — working hot path (ingest → physics → tactical → buffer → strategic verify → compose → output).
2. **Post-pilot hardening** — DAL/Redis/self-heal/RAM recall/pattern routing.
3. **Fleet preparation** — knowledge store + deposits + upload format.
4. **Multi-rig deployment** — WireGuard + sync + hub API.

---

## 27. Concrete Build-Order Checklist (Repo-Aligned)

> Goal: produce a working, testable Phase 1–4 hot path first, then add verification, then knowledge/LLM/voting/composition, then API/persistence, then fleet/self-heal.

### A. Types + enums (compile-first)
- [ ] Extend `src/types.rs` with `WitsPacket`, `RigState`, `DataQuality`, and `DrillingMetrics` (or add `[NEW] src/types/wits.rs` + `src/types/drilling.rs` and re-export).
- [ ] Add `AdvisoryTicket`, `VerificationResult`, `StrategicAdvisory`, `Severity`, `RiskLevel`, `Category`, `Campaign`, `Operation` into `src/types.rs`, keeping naming consistent with existing repo patterns.

### B. Acquisition (ingestion + replay)
- [ ] Implement `[NEW] src/acquisition/wits_tcp_source.rs` and wire into `src/acquisition/mod.rs`.
- [ ] Extend `src/acquisition/sensors.rs` to host the `DataSource` trait (or add `[NEW] src/acquisition/data_source.rs` and re-export).
- [ ] Add CLI flags in `src/main.rs` for `--wits-tcp`, `--stdin`, `--csv` consistent with ingestion modes.

### C. Physics engine (deterministic metrics)
- [ ] Add `[NEW] src/physics_engine/drilling/metrics.rs` — MSE, d-exponent, ECD, flow balance, pit rate.
- [ ] Add `[NEW] src/physics_engine/drilling/detectors.rs` — stick-slip CV, pack-off, founder, washout.
- [ ] Expose `PhysicsEngine::analyze(&WitsPacket, &history) -> DrillingMetrics` from `src/physics_engine/mod.rs`.

### D. Hot path agent + buffer (Phases 3–4)
- [ ] Implement drilling Tactical gate in `src/agents/tactical.rs`.
- [ ] Ensure `src/history_storage.rs` provides a 60-packet ring buffer for `(WitsPacket, DrillingMetrics)`.
- [ ] Wire Phases 1–4 into `src/pipeline/coordinator.rs` with per-phase latency logging.

### E. Strategic verification (Phase 5)
- [ ] Implement `verify_ticket()` in `src/agents/strategic.rs` returning `VerificationResult` with reasons.
- [ ] Add regression helpers (slope + R²) in `src/strategic/aggregation.rs` or `[NEW] src/strategic/regression.rs`.

### F. Knowledge store (Phase 6)
- [ ] Extend `src/context/vector_db.rs` into a `KnowledgeStore` trait with `NoOpStore` for pilot.
- [ ] Add `[NEW] src/context/ram_recall.rs` (feature-flagged for roadmap).
- [ ] Implement `query_precedent(ticket) -> Vec<FleetEpisode>`.

### G. LLM reasoning (Phase 7)
- [ ] Adapt `src/llm/strategic_llm.rs` to accept drilling context and return structured `LLMRecommendation`.
- [ ] Implement template fallback in `[NEW] src/llm/templates.rs` or `src/strategic/templates.rs`.

### H. Orchestrator voting (Phase 8)
- [ ] Add drilling specialists in `[NEW] src/agents/specialists/` and wire into `src/agents/orchestrator.rs`.
- [ ] Validate weights sum ~1.0 in config validation.

### I. Advisory composition (Phase 9)
- [ ] Add `[NEW] src/strategic/advisory.rs` with `AdvisoryComposer::compose()` and CRITICAL cooldown.
- [ ] Ensure advisory carries evidence + tracking fields.

### J. Output API + dashboard (Phase 10)
- [ ] Add/extend endpoints in `src/api/routes.rs` + `src/api/handlers.rs`: drilling, diagnosis, acknowledge, config, campaign, baseline.
- [ ] Maintain in-memory caches (`latest_metrics`, `latest_advisory`) for O(1) handler responses.

### K. Persistence (optional; pilot-safe)
- [ ] Keep pilot persistence minimal: `data/baseline_state.json` + advisory CSV/JSON logs.
- [ ] Add `PersistenceLayer` trait with `InMemoryDAL` first, then `[NEW] src/storage/postgres.rs` later.

### L. Background services + self-heal
- [ ] Add `[NEW] src/background/self_healer.rs` with 30s check cadence.
- [ ] Implement WITS reconnect healing, LLM fallback switching, disk-space checks, health banners.

### M. Fleet (post-pilot)
- [ ] Add `[NEW] src/fleet/client.rs`, `[NEW] src/fleet/sync.rs`, `[NEW] src/fleet/queue.rs`.
- [ ] Ensure uploads only on confirmed AMBER/RED advisories.

---

*End of merged pre-build specification.*
