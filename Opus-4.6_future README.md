# SAIREN-OS: Complete Technical Blueprint

## Every Stage, Every Decision, Every Byte — Before You Write a Line

**Version:** Pre-Build 1.0  
**Date:** February 7, 2026  
**Purpose:** Preemptive technical specification. Analyse, debate, and perfect this before touching code.  
**Audience:** You (Ashton), future contributors, and anyone who needs to understand exactly how SAIREN-OS works from packet to advisory to fleet.

-----

## Table of Contents

1. [System Identity](#1-system-identity)
2. [The 10-Phase Pipeline: Packet to Advisory](#2-the-10-phase-pipeline)
3. [Stage 1: Data Ingestion](#3-stage-1-data-ingestion)
4. [Stage 2: Physics Engine](#4-stage-2-physics-engine)
5. [Stage 3: Tactical Agent](#5-stage-3-tactical-agent)
6. [Stage 4: History Buffer](#6-stage-4-history-buffer)
7. [Stage 5: Strategic Agent](#7-stage-5-strategic-agent)
8. [Stage 6: Knowledge Store (Fleet Memory)](#8-stage-6-knowledge-store)
9. [Stage 7: LLM Reasoning](#9-stage-7-llm-reasoning)
10. [Stage 8: Orchestrator (Specialist Voting)](#10-stage-8-orchestrator)
11. [Stage 9: Advisory Composition](#11-stage-9-advisory-composition)
12. [Stage 10: Output Layer](#12-stage-10-output-layer)
13. [Background Services](#13-background-services)
14. [Campaign System](#14-campaign-system)
15. [Baseline Learning](#15-baseline-learning)
16. [ML Engine V2.2](#16-ml-engine)
17. [Configuration System](#17-configuration-system)
18. [Trait Architecture (Sentrix Integration)](#18-trait-architecture)
19. [Persistence Layer](#19-persistence-layer)
20. [Fleet Network (Multi-Rig)](#20-fleet-network)
21. [Edge Hardware & Deployment](#21-edge-hardware)
22. [Dashboard & API](#22-dashboard-and-api)
23. [Enhancement Roadmap (RAM Recall + Pattern Routing)](#23-enhancement-roadmap)
24. [Data Flow: One Packet, Full Journey](#24-data-flow)
25. [Failure Modes & Fallbacks](#25-failure-modes)
26. [Build Order & Implementation Phases](#26-build-order)

-----

## 1. System Identity

SAIREN-OS is a **deterministic-first, AI-enhanced drilling advisory system**. That sentence matters — the order is deliberate.

**What it is:** A multi-agent pipeline that ingests WITS Level 0 drilling data at 1 Hz, runs physics calculations on every packet, and escalates anomalies through progressively deeper analysis layers until it produces an actionable advisory for the driller.

**What it is not:** A chatbot, a cloud analytics dashboard, or a control system. It cannot send commands to rig equipment. It is read-only, passive, and air-gapped.

**The core philosophy:** Physics cannot hallucinate. Every advisory starts with deterministic calculations (MSE, d-exponent, flow balance, ECD margin). AI layers (LLM reasoning, fleet knowledge) add context and nuance, but the physics engine is the ground truth. If the physics says “normal,” no amount of AI pattern-matching can override that to generate a false alarm.

**The cognitive model:** Kahneman’s System 1 / System 2.

- **System 1 (Tactical Agent):** Fast, reflexive, runs on every packet. Catches hard limit breaches and obvious anomalies in under 15ms. This is the watchdog — it never sleeps, never gets fatigued, and triggers on thresholds.
- **System 2 (Strategic Agent):** Slow, deliberate, only triggered when System 1 raises a ticket. Performs deep causal analysis — trend regression, fleet knowledge queries, LLM reasoning. Takes 500-800ms but only runs when something is actually wrong.

**Why this architecture matters:** A driller on a rig floor needs two things — instant alerts for safety-critical events (kicks, losses) and thoughtful recommendations for efficiency optimization. System 1 handles the first. System 2 handles the second. Neither interferes with the other.

-----

## 2. The 10-Phase Pipeline

Every WITS packet that enters SAIREN-OS passes through a defined sequence. Most packets exit at Phase 3 (no anomaly detected). Only anomalous packets continue to Phase 5+.

```
Phase 1:  WITS Ingestion        → Receive, validate, timestamp
Phase 2:  Physics Engine         → MSE, d-exponent, flow balance, ECD, pit rate
Phase 3:  Tactical Agent         → Threshold check, anomaly detection, ticket gate
Phase 4:  History Buffer         → Store packet in 60-packet ring buffer
Phase 5:  Strategic Verification → Physics-based ticket validation
Phase 6:  Knowledge Query        → "Has any rig seen this before?"
Phase 7:  LLM Reasoning          → Generate natural-language diagnosis
Phase 8:  Orchestrator Voting    → 4 specialists vote on risk level
Phase 9:  Advisory Composition   → Merge all signals into StrategicAdvisory
Phase 10: Output                 → Dashboard, API, logs, persistence
```

**Latency budget per phase (GPU mode):**

|Phase|Target|Actual                                |Notes                         |
|-----|------|--------------------------------------|------------------------------|
|1    |<5ms  |~2ms                                  |TCP read + JSON parse         |
|2    |<15ms |~10ms                                 |All physics calculations      |
|3    |<5ms  |~3ms                                  |Threshold comparison          |
|4    |<1ms  |<1ms                                  |Ring buffer push              |
|5    |<100ms|~50ms                                 |Trend regression, history scan|
|6    |<15ms |8-15ms (target: 1-2ms with RAM recall)|Vector similarity search      |
|7    |<800ms|~750ms                                |Qwen 2.5 7B inference         |
|8    |<5ms  |~2ms                                  |Weighted average calculation  |
|9    |<5ms  |~2ms                                  |Struct composition            |
|10   |<10ms |~5ms                                  |HTTP push, file write         |

**Total for normal packet (Phases 1-4):** ~16ms  
**Total for anomalous packet (all phases):** ~840ms on GPU, ~35s on CPU

-----

## 3. Stage 1: Data Ingestion

### What Happens

The system receives raw WITS Level 0 data over a TCP socket. WITS is the industry standard — every rig in the world speaks it. The data arrives as ASCII strings, one record per line, containing record type, item number, and value.

### Input Format

```
WITS Record 01 (Drill String Data)
├── Item 08: Weight on Bit (WOB) — thousands of pounds
├── Item 10: Rate of Penetration (ROP) — feet/hour
├── Item 11: Rotary Speed (RPM)
├── Item 14: Torque at Motor — ft-lbs
└── Item 15: Hook Load — total string weight

WITS Record 02 (Wellhead Parameters)
├── Item 01: Surface Pressure (SPP) — psi
├── Item 04: Pump Stroke Count
└── Item 05: Pump Pressure

WITS Record 06 (Equipment Parameters)
├── Item 01: Flow In — gpm
├── Item 02: Flow Out — gpm
└── Item 03: Pit Volume — bbl

WITS Record 08 (Drilling Fluids)
├── Item 01: Mud Weight In — ppg
├── Item 02: Mud Weight Out — ppg
├── Item 03: Mud Temperature In
└── Item 04: Mud Temperature Out

WITS Record 13 (Gas Monitoring)
├── Item 01: Total Gas — units
└── Item 03: H2S — ppm
```

**Update frequency:** 1-5 seconds (typically 1 Hz)  
**Protocol:** TCP/IP on port 9100 (Rockwell), 5000 (Schneider), or custom  
**Alternative protocols:** Serial RS-232, OPC-UA (future), Modbus (future)

### Data Quality Gate

Before any packet enters the pipeline, it passes through validation:

1. **All-zero rejection:** If every value in a packet is 0.0, it’s a dead sensor or disconnected feed. Reject silently.
2. **Range validation:** Each parameter has physically possible bounds. WOB cannot be -5,000 lbs. ROP cannot be 10,000 ft/hr. Reject packets with impossible values.
3. **Consistency checks:** If flow_in is 500 gpm but flow_out is 0 gpm, something is wrong with the sensor, not the well. Flag but don’t reject (might be a real loss event — let physics sort it out).
4. **Timestamp injection:** The system adds its own UTC timestamp to every validated packet. WITS timestamps from the rig can drift.

### Connection Resilience

The TCP connection to the WITS source will drop. Satellite handoffs, rig power events, cable bumps — it happens. The ingestion layer handles this:

- **Read timeout:** 120 seconds. If no data arrives for 2 minutes, assume the connection is dead and attempt reconnection.
- **TCP keepalive:** Enabled via `socket2` to detect stale connections at the OS level.
- **Reconnection strategy:** Exponential backoff — 2s, 4s, 8s, 16s, 32s, 60s cap. Maximum 10 attempts before alerting the operator via dashboard.
- **During disconnection:** The system continues operating with cached data. Physics calculations pause but the dashboard remains accessible. The baseline learning clock pauses.

### Input Modes

|Mode |Flag                       |Use Case                                |
|-----|---------------------------|----------------------------------------|
|TCP  |`--wits-tcp localhost:5000`|Production — live rig data              |
|STDIN|`--stdin`                  |Development — pipe JSON packets directly|
|CSV  |`--csv path/to/file.csv`   |Replay — historical data analysis       |

### Trait Abstraction (Future)

Currently, the WITS TCP parser is tightly coupled. The Sentrix integration introduces a `trait DataSource`:

```rust
trait DataSource {
    async fn next_packet(&mut self) -> Result<WitsPacket>;
    async fn reconnect(&mut self) -> Result<()>;
    fn health_check(&self) -> HealthStatus;
}
```

Implementations: `WitsTcpSource` (current, refactored), `OpcUaSource` (future), `ModbusSource` (future), `CsvReplaySource` (current, refactored), `StdinSource` (current, refactored).

This abstraction means you can swap data sources without touching any downstream code. The physics engine doesn’t care whether data came from WITS TCP, OPC-UA, or a CSV file — it receives a `WitsPacket` struct either way.

-----

## 4. Stage 2: Physics Engine

### What Happens

Every validated packet triggers deterministic physics calculations. This is the ground truth layer — no ML, no heuristics, no AI. Pure mathematics derived from drilling engineering textbooks.

### Calculations Performed

#### MSE (Mechanical Specific Energy)

**What it measures:** The energy required to remove one unit volume of rock. Lower MSE = more efficient drilling.

**Formula:**

```
MSE = (4 × WOB) / (π × D²) + (480 × T × RPM) / (D² × ROP)

Where:
  WOB  = Weight on Bit (lbs)
  D    = Bit diameter (inches)
  T    = Torque (ft-lbs)
  RPM  = Rotary speed
  ROP  = Rate of Penetration (ft/hr)
```

**Target range:** 35,000-50,000 psi depending on formation.

**What it tells the driller:** If MSE is 52,000 psi and the target is 35,000 psi, the bit is working 48% harder than it should. Adjusting WOB or RPM could improve ROP by 10-20% and reduce bit wear.

**Edge case:** When ROP approaches zero (connections, tripping), MSE goes to infinity. The system gates MSE calculation on `rig_state == Drilling` to avoid garbage values.

#### D-Exponent

**What it measures:** Normalised drilling parameter that tracks formation hardness and pore pressure changes.

**Formula:**

```
d_exp = log10(ROP / (60 × RPM)) / log10(12 × WOB / (1000 × D))
d_corrected = d_exp × (ρ_normal / ρ_actual)

Where:
  ρ_normal = Normal hydrostatic gradient mud weight (ppg)
  ρ_actual = Current mud weight (ppg)
```

**What it tells the driller:** A rising d-exponent means the formation is getting harder (compaction trend). A sudden drop could indicate overpressure ahead — the formation is weaker than expected because it’s holding pressurised fluid. This is a critical well control indicator.

**Formation boundary detection:** If d-exponent shifts by more than 15% within a short window, SAIREN flags a formation change. This triggers the ML engine to re-segment its analysis.

#### ECD (Equivalent Circulating Density)

**What it measures:** The effective mud weight at the bit, accounting for friction losses while circulating.

**Formula:**

```
ECD = ρ_mud + (ΔP_annular / (0.052 × TVD))

Where:
  ρ_mud       = Surface mud weight (ppg)
  ΔP_annular  = Annular pressure loss (psi)
  TVD         = True Vertical Depth (feet)
```

**Critical thresholds:**

- ECD margin > 0.3 ppg from fracture gradient → GREEN (safe)
- ECD margin 0.1-0.3 ppg → YELLOW (monitor closely)
- ECD margin < 0.1 ppg → RED (risk of fracturing formation, potential lost circulation)

#### Flow Balance

**What it measures:** The difference between what goes down the hole (flow_in) and what comes back (flow_out).

```
flow_balance = flow_out - flow_in
```

**Interpretation:**

- Positive (flow_out > flow_in): Formation fluid is entering the wellbore. This is a **kick** — the most dangerous event in drilling. Gas, oil, or water is flowing in because formation pressure exceeds mud weight.
- Negative (flow_in > flow_out): Mud is being lost into the formation. This is **lost circulation** — less dangerous than a kick but expensive and operationally problematic.
- Zero (±5 gpm): Normal. Perfect balance.

**Thresholds (Production campaign):**

- Warning: ±10 gpm
- Critical: ±20 gpm

**Thresholds (P&A campaign):**

- Warning: ±5 gpm
- Critical: ±15 gpm

#### Pit Rate

**What it measures:** The rate of change of surface pit volume (mud tanks).

```
pit_rate = Δ(pit_volume) / Δt  (bbl/hr, 5-minute rolling average)
```

The 5-minute rolling average is critical — pit volume is noisy (mixing, transfers, sampling). Without smoothing, every mud transfer triggers a false kick alarm.

**Critical thresholds:**

- Pit gain > 5 bbl → Warning
- Pit gain > 10 bbl → CRITICAL
- Pit rate > 15 bbl/hr → CRITICAL

### Dysfunction Detection

Beyond the core calculations, the physics engine runs pattern detectors:

#### Stick-Slip

**Detection:** Torque coefficient of variation (CV) > 12% over a rolling window.

```
CV = (σ_torque / μ_torque) × 100

If CV > 12%: stick-slip detected
```

**What’s happening physically:** The drill string is alternately sticking (held by friction, building torsional energy) and slipping (releasing that energy in violent rotation). This destroys bits, damages drill string, and reduces ROP.

**What the advisory says:** Reduce WOB, increase RPM, consider anti-stick-slip tools.

#### Pack-Off

**Detection:** Torque AND SPP both rising simultaneously over a sustained period.

**What’s happening physically:** Cuttings are accumulating around the BHA (bottom hole assembly), physically squeezing the drill string. As the pack-off develops, it restricts flow (SPP rises) and grabs the string (torque rises). If unchecked, this leads to stuck pipe — a £500K+ problem.

**What the advisory says:** Reduce WOB 20-25%, increase flow 80-100 gpm, consider back-reaming.

#### Founder Condition

**Detection:** Two-stage analysis.

**Stage 1 (Tactical — quick check):** WOB increasing by >5% while ROP is flat or decreasing.

**Stage 2 (Strategic — trend analysis):** Linear regression on the WOB vs ROP relationship over the last 60 packets.

```
If WOB trend = positive AND ROP trend = negative:
  founder_severity = HIGH (70%+)
  optimal_wob = WOB value where ROP was highest in window
  recommendation = "Reduce WOB to {optimal_wob}"
```

**What’s happening physically:** The bit has passed its “founder point” — the WOB at which ROP peaks. Beyond this, adding weight actually makes things worse. The bit balls up with cuttings, the formation compresses rather than fractures, and energy is wasted. The solution is counterintuitive: reduce weight to drill faster.

#### Washout

**Detection:** Flow loss (flow_out drops) while WOB and ROP remain stable.

**What’s happening physically:** The drill string or bit has developed a hole (washout). Drilling fluid is escaping through the breach instead of reaching the bit. This reduces hydraulic cleaning and cools/lubricates the bit less.

### Output

The physics engine produces a `DrillingMetrics` struct that accompanies the packet through every subsequent phase:

```rust
struct DrillingMetrics {
    mse: f64,
    mse_efficiency: f64,      // percentage of optimal
    d_exponent: f64,
    d_exponent_corrected: f64,
    ecd: f64,
    ecd_margin: f64,
    flow_balance: f64,
    pit_rate: f64,
    pit_gain: f64,
    torque_cv: f64,            // stick-slip indicator
    torque_delta_pct: f64,     // pack-off indicator
    spp_delta: f64,            // pack-off indicator
    rig_state: RigState,       // Drilling, Connection, Tripping, Idle
    operation: Operation,      // Production, Milling, CementDrillOut, etc.
    founder_detected: bool,
    founder_severity: f64,
    timestamp: DateTime<Utc>,
}
```

-----

## 5. Stage 3: Tactical Agent

### What Happens

The Tactical Agent is the gatekeeper. It receives `DrillingMetrics` and decides: is this normal, or does something need deeper investigation?

### Decision Logic

The agent compares each metric against configurable thresholds (from `well_config.toml` or adaptive baselines):

```
For each metric in DrillingMetrics:
  If metric > CRITICAL_THRESHOLD:
    Create ticket → severity: RED
    Skip to Strategic Agent immediately
  
  If metric > WARNING_THRESHOLD:
    Increment anomaly counter
    If anomaly sustained for N seconds:
      Create ticket → severity: AMBER
      Escalate to Strategic Agent
  
  Else:
    metric is GREEN → no action
```

### Ticket Structure

When the Tactical Agent creates a ticket, it packages everything the Strategic Agent needs:

```rust
struct AdvisoryTicket {
    id: Uuid,
    timestamp: DateTime<Utc>,
    severity: Severity,           // GREEN, YELLOW, AMBER, RED
    category: Category,           // WellControl, Mechanical, Optimization, Formation
    trigger_metric: String,       // "flow_balance", "torque_cv", etc.
    trigger_value: f64,
    threshold_breached: f64,
    physics_snapshot: DrillingMetrics,
    routing_decision: RoutingDecision,  // from pattern matcher (future)
    certainty: f64,
}
```

### Operation Classification

The Tactical Agent also classifies the current drilling operation based on parameter signatures:

|Operation          |Detection Criteria                                     |Campaign|
|-------------------|-------------------------------------------------------|--------|
|Production Drilling|Default when drilling                                  |Any     |
|Milling            |Torque > 15 kft-lb AND ROP < 5 ft/hr                   |P&A only|
|Cement Drill-Out   |WOB > 15 klbs AND Torque > 12 kft-lb AND ROP < 20 ft/hr|P&A only|
|Circulating        |Flow > 50 gpm AND WOB < 5 klbs                         |Any     |
|Static             |RPM < 10 AND WOB < 5 klbs                              |Any     |

This classification matters because thresholds and specialist weights change per operation. You wouldn’t apply ROP optimization advisories during a connection.

### Traffic Ratio

In normal drilling operations, approximately:

- **95%** of packets exit at Phase 3 (GREEN — no anomaly)
- **4%** generate YELLOW logs (minor deviations, noted but not escalated)
- **<1%** create AMBER/RED tickets that proceed to Phase 5+

This means the Strategic Agent, LLM, and Orchestrator are idle most of the time — they only activate when something genuinely needs attention.

-----

## 6. Stage 4: History Buffer

### What Happens

Every packet (regardless of Tactical Agent outcome) gets pushed into a 60-packet ring buffer. This provides the Strategic Agent with temporal context — it can look back 60 seconds to see trends.

### Why 60 Packets

- At 1 Hz, 60 packets = 1 minute of history
- Sufficient for trend detection (linear regression needs ~20+ data points for statistical significance)
- Small enough to fit entirely in L1/L2 CPU cache (~100KB)
- Provides the “60-packet history window” referenced by the Strategic Agent’s verification logic

### Implementation

```rust
struct HistoryBuffer {
    buffer: VecDeque<(WitsPacket, DrillingMetrics)>,
    capacity: usize,  // 60
}

impl HistoryBuffer {
    fn push(&mut self, packet: WitsPacket, metrics: DrillingMetrics) {
        if self.buffer.len() >= self.capacity {
            self.buffer.pop_front();  // drop oldest
        }
        self.buffer.push_back((packet, metrics));
    }
    
    fn trend(&self, metric: &str) -> TrendResult {
        // Linear regression over all values of `metric` in buffer
        // Returns slope, intercept, r², and trend direction
    }
}
```

-----

## 7. Stage 5: Strategic Agent

### What Happens

The Strategic Agent receives a ticket from the Tactical Agent and performs deep verification. Its job is to answer: “Is this real, or is it noise?”

### Verification Process

**Step 1: Physics-based validation.** Re-run the physics calculations using the 60-packet history buffer, not just the single triggering packet. A single spike might be sensor noise; a sustained trend is real.

```
If trigger_metric shows sustained trend over 30+ seconds:
  → CONFIRM (proceed to Phase 6)
If trigger_metric was a single spike that returned to normal:
  → REJECT (discard ticket, log as false positive)
If trigger_metric is ambiguous:
  → UNCERTAIN (log at lower priority, don't generate advisory)
```

**Step 2: Trend analysis (for specific dysfunctions).**

For **founder detection**, the Strategic Agent runs linear regression on the WOB/ROP relationship:

```
wob_trend = linear_regression(history.wob_values)
rop_trend = linear_regression(history.rop_values)

If wob_trend.slope > 0 AND rop_trend.slope <= 0:
  founder_confirmed = true
  optimal_wob = wob_value_at_max_rop(history)
  founder_severity = calculate_severity(wob_trend, rop_trend)
```

For **pack-off confirmation**, it checks whether torque AND SPP are both trending upward simultaneously:

```
torque_trend = linear_regression(history.torque_values)
spp_trend = linear_regression(history.spp_values)

If torque_trend.slope > 0 AND spp_trend.slope > 0:
  pack_off_confirmed = true
```

**Step 3: Context enrichment.** Attach formation type (from d-exponent), current depth, campaign mode, and operation type to the ticket. This context feeds into Phase 6 (knowledge query) and Phase 7 (LLM reasoning).

### Verification Outcomes

|Outcome  |Action                                      |Rate           |
|---------|--------------------------------------------|---------------|
|CONFIRM  |Proceed to Phase 6-9, generate advisory     |~70% of tickets|
|REJECT   |Discard, log to PostgreSQL as false positive|~20% of tickets|
|UNCERTAIN|Log at lower priority, no advisory          |~10% of tickets|

The 20% reject rate is important — it means the Tactical Agent is intentionally sensitive (catches everything) while the Strategic Agent filters out the noise. Better to raise 10 tickets and reject 2 than to miss 1 real event.

-----

## 8. Stage 6: Knowledge Store (Fleet Memory)

### What Happens (Single Rig — V72 Pilot)

For the V72 pilot, this stage is a **NoOp**. There is no fleet knowledge — it’s the first rig. The system skips to Phase 7.

### What Happens (Fleet Mode — Post-Pilot)

The Strategic Agent queries a vector database to find similar historical events:

```rust
let similar_events = knowledge_store.query(
    embedding: ticket.to_embedding(),  // 384-dim vector from BGE-small-en-v1.5
    filters: {
        campaign: current_campaign,
        event_type: ticket.category,
        formation: current_formation,  // optional
        depth_range: (current_depth - 300, current_depth + 300),  // optional
        outcome: [Resolved, Escalated],  // skip false positives
    },
    top_k: 5,
);
```

### What the Query Returns

If the fleet library has seen this pattern before:

```
Match 1: Rig #12, North Sea, 3200m, Shale
  Event: Pack-off (torque +30%, SPP +150 psi)
  Resolution: Reduced WOB 25%, increased flow 100 gpm
  Outcome: Resolved in 15 minutes, no stuck pipe
  
Match 2: Rig #19, Angola, 3100m, Shale  
  Event: Pack-off (torque +28%, SPP +140 psi)
  Resolution: Reduced WOB 20%, increased flow 80 gpm
  Outcome: Resolved in 20 minutes
```

This precedent data feeds directly into the LLM prompt (Phase 7) and influences the orchestrator’s confidence (Phase 8).

### Trait Abstraction

```rust
trait KnowledgeStore {
    async fn query(&self, embedding: &[f32], filters: QueryFilters) -> Vec<FleetEvent>;
    async fn deposit(&self, event: FleetEvent) -> Result<()>;
    fn health_check(&self) -> HealthStatus;
}
```

|Implementation |Use Case                                   |
|---------------|-------------------------------------------|
|`NoOpStore`    |V72 pilot (no fleet yet)                   |
|`InMemoryStore`|Testing, single-rig development            |
|`ChromaStore`  |Production fleet mode (lifted from Sentrix)|

### RAM Recall Enhancement (Roadmap)

The Enhancement Roadmap proposes replacing the external vector DB with in-memory HNSW indexing:

- **Current latency:** 8-15ms (Qdrant/ChromaDB over HTTP)
- **Target latency:** 1-2ms (in-memory HNSW)
- **Memory cost:** ~260MB for 10,000 episodes
- **Trade-off:** Uses 1.6% of 16GB system RAM for 7-12x faster recall

The RAM recall module maintains metadata indices (campaign, event type, formation, outcome) for O(1) pre-filtering before HNSW search. This means only relevant episodes are searched, not the entire library.

-----

## 9. Stage 7: LLM Reasoning

### What Happens

A local LLM (Qwen 2.5) synthesises all available signals into a natural-language diagnosis and recommendation.

### Model Selection

The system auto-detects hardware at startup and selects appropriate models:

|Hardware  |Tactical Model       |Strategic Model      |Build Flag       |
|----------|---------------------|---------------------|-----------------|
|GPU (CUDA)|Qwen 2.5-1.5B (~60ms)|Qwen 2.5-7B (~800ms) |`--features cuda`|
|CPU only  |Qwen 2.5-1.5B (~2-5s)|Qwen 2.5-4B (~10-30s)|`--features llm` |
|No LLM    |Template-based       |Template-based       |default          |

All models are quantised (Q4_K_M format — 4-bit quantisation with K-quant mixed precision). This reduces VRAM requirements by 4-8x with minimal quality loss for classification and advisory tasks.

### Tactical LLM Input

The Tactical LLM receives a structured prompt with the current packet, physics results, and campaign context. Its job is classification: confirm or refine the Tactical Agent’s initial category assignment.

### Strategic LLM Input

The Strategic LLM receives a richer prompt:

```
CONTEXT:
  Campaign: Production
  Operation: Production Drilling
  Formation: Sandstone (d-exponent: 1.45)
  Depth: 2,850m
  
CURRENT STATE:
  WOB: 35 klbs | ROP: 18 ft/hr | RPM: 120
  Torque: 18,000 ft-lbs (↑30% from baseline)
  SPP: 3,050 psi (↑150 psi from baseline)
  Flow balance: -3 gpm | Pit volume: stable
  MSE: 52,000 psi (efficiency: 68%)
  
PHYSICS VERDICT:
  Pack-off signature detected (torque + SPP rising)
  Founder: Not detected
  Stick-slip: Not detected
  
FLEET PRECEDENT: (if available)
  3 similar events found across fleet
  All resolved by WOB reduction 20-25% + flow increase
  
TASK:
  Diagnose the situation. Recommend specific parameter adjustments.
  State expected benefit and risk level.
```

### Strategic LLM Output

The LLM produces structured text that gets parsed into advisory fields:

```
DIAGNOSIS: Developing pack-off in sandstone at 2,850m. 
Torque increase of 30% combined with SPP rise of 150 psi indicates 
cuttings accumulation around the BHA.

RECOMMENDATION: 
1. Reduce WOB from 35 to 28 klbs (-20%)
2. Increase flow rate by 80 gpm
3. Consider short back-ream (50m) to clear cuttings

EXPECTED BENEFIT:
- Prevent pack-off progression → avoid stuck pipe risk
- Restore ROP to baseline levels
- Reduce MSE from 52,000 to target range (35-45,000 psi)

RISK IF IGNORED:
- Progressive pack-off → stuck pipe (£500K+ recovery cost)
- Extended NPT (8-12 hours typical)
```

### Template Fallback

If the LLM is unavailable (GPU failure, model loading error, CPU too slow), the system falls back to template-based advisories. These are pre-written responses keyed to physics verdicts:

```rust
fn template_advisory(physics: &DrillingMetrics) -> String {
    if physics.pack_off_detected {
        format!(
            "Pack-off signature detected. Torque increase: {:.0}%. SPP increase: {:.0} psi. \
             Recommend: Reduce WOB 20%, increase flow rate. Monitor for stuck pipe indicators.",
            physics.torque_delta_pct, physics.spp_delta
        )
    }
    // ... other templates
}
```

Templates are less nuanced than LLM output but functionally correct. The physics verdict is the same either way — the LLM just wraps it in better language and adds fleet context.

### Trait Abstraction

```rust
trait InferenceBackend {
    fn classify(&self, packet: &WitsPacket, context: &Context) -> TacticalResult;
    fn reason(&self, ticket: &AdvisoryTicket, history: &[WitsPacket]) -> StrategicReasoning;
    fn is_available(&self) -> bool;
}
```

|Implementation   |Use Case                                        |
|-----------------|------------------------------------------------|
|`GgufBackend`    |Current — local Qwen models via mistral.rs      |
|`TemplateBackend`|Fallback — no LLM, rule-based templates         |
|`CloudApiBackend`|Future — Anthropic/OpenAI for fleet hub analysis|

-----

## 10. Stage 8: Orchestrator (Specialist Voting)

### What Happens

Four domain-specific specialists independently evaluate the anomaly and vote on risk level. Their votes are weighted to produce a single composite risk score.

### The Four Specialists

|Specialist                 |Weight|Domain             |What It Evaluates                                                 |
|---------------------------|------|-------------------|------------------------------------------------------------------|
|**MSE Specialist**         |25%   |Drilling efficiency|MSE value vs target, ROP trends, energy waste, bit wear indicators|
|**Hydraulic Specialist**   |25%   |Fluid mechanics    |SPP deviations, ECD margin, flow rates, pump pressure trends      |
|**Well Control Specialist**|30%   |Safety-critical    |Kick/loss indicators, gas readings, pit volume, H2S, flow balance |
|**Formation Specialist**   |20%   |Geology            |D-exponent trends, formation changes, pore pressure estimates     |

**Why Well Control has the highest weight (30%):** A kick will kill people. An MSE inefficiency will cost money. The weighting reflects consequence severity.

### Voting Mechanism

Each specialist returns a risk level: LOW, ELEVATED, HIGH, or CRITICAL.

```
Numerical mapping:
  LOW      = 1.0  (>85% efficiency)
  ELEVATED = 0.75 (70-85% efficiency)
  HIGH     = 0.50 (50-70% efficiency)
  CRITICAL = 0.25 (<50% efficiency)

Composite score = Σ(specialist_vote × specialist_weight)

Example:
  MSE:         ELEVATED (0.75) × 0.25 = 0.1875
  Hydraulic:   LOW (1.0)       × 0.25 = 0.2500
  WellControl: LOW (1.0)       × 0.30 = 0.3000
  Formation:   LOW (1.0)       × 0.20 = 0.2000
  ─────────────────────────────────────────────
  Composite:                            0.9375

  Score > 0.85 → LOW
  Score 0.70-0.85 → ELEVATED
  Score 0.50-0.70 → HIGH
  Score < 0.50 → CRITICAL
  
  Result: 0.9375 → LOW (but with ELEVATED MSE flag)
```

### Why Not Just Use the Physics Verdict?

The physics engine detects individual symptoms. The orchestrator provides **holistic diagnosis**. Consider:

- Torque rising + SPP rising + d-exponent changing → Pack-off in new formation (MSE + Hydraulic + Formation all flag)
- Flow balance positive + pit volume rising + gas units increasing → Kick developing (Well Control flags CRITICAL, overrides everything)
- MSE high + torque stable + flow normal → Simple inefficiency (only MSE flags, low overall risk)

The voting system captures these multi-dimensional relationships that a single threshold check cannot.

### Configurable Weights

Weights are configurable in `well_config.toml`:

```toml
[ensemble_weights]
mse = 0.25
hydraulic = 0.25
well_control = 0.30
formation = 0.20
```

The system validates that weights sum to approximately 1.0 on startup.

-----

## 11. Stage 9: Advisory Composition

### What Happens

All signals from Phases 5-8 are merged into a single `StrategicAdvisory` struct — the final output of the analytical pipeline.

### Advisory Structure

```rust
struct StrategicAdvisory {
    // Identity
    id: String,                    // "ADV-042"
    timestamp: DateTime<Utc>,
    advisory_number: u32,
    
    // Classification
    risk_level: RiskLevel,         // LOW, ELEVATED, HIGH, CRITICAL
    efficiency_score: f64,         // 0-100%
    category: Category,            // WellControl, Mechanical, Optimization, Formation
    advisory_type: AdvisoryType,   // Safety, Optimization, Informational
    
    // Content
    recommendation: String,        // "Reduce WOB to 28 klbs, increase flow 80 gpm"
    expected_benefit: String,      // "Prevent pack-off, restore ROP to baseline"
    diagnosis: String,             // LLM reasoning output
    
    // Evidence
    physics_verdict: PhysicsVerdict,
    specialist_votes: SpecialistVotes,
    fleet_precedent: Option<Vec<FleetEvent>>,
    
    // Context
    campaign: Campaign,
    operation: Operation,
    formation: String,
    depth: f64,
    
    // Tracking
    acknowledged: bool,
    acknowledged_by: Option<String>,
    action_taken: Option<String>,
    outcome: Option<AdvisoryOutcome>,
}
```

### Advisory Display Format

```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
🎯 ADVISORY #42: ELEVATED | Efficiency: 68%

   Type: OPTIMIZATION | Category: Drilling Efficiency

   Recommendation: Consider adjusting WOB/RPM to improve MSE.
                   Current MSE: 52,000 psi (Target: 35,000 psi)

   Expected Benefit: Potential 10-20% ROP improvement, reduced bit wear

   MSE Specialist (25%): MEDIUM — MSE 52,000 psi exceeds optimal by 48%
   Hydraulic (25%): LOW — Flow balance normal
   WellControl (30%): LOW — No kick/loss indicators
   Formation (20%): LOW — D-exponent stable
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

### CRITICAL Cooldown

To prevent alert fatigue, CRITICAL advisories have a 30-second cooldown. If the same CRITICAL condition persists, the system logs it but doesn’t re-alert until 30 seconds have passed. The driller has already been warned — spamming them degrades trust.

-----

## 12. Stage 10: Output Layer

### What Happens

The advisory is delivered through multiple channels simultaneously.

### Dashboard (Primary)

The web dashboard at `http://localhost:8080` receives advisories via server-sent events or websocket push. Three role-based views:

**Screen 1 — Rig Floor (Driller):** Operational score gauge, rig state indicator, live parameters with colour coding, CRITICAL alerts only. No clutter.

**Screen 2 — Company Man (Strategic):** Current advisory with full reasoning, ML insights (optimal parameters), advisory history, trend charts.

**Screen 3 — Engineering (Deep Dive):** 24-hour parameter trends, all physics calculations over time, complete anomaly log, ML reports, dysfunction event timeline, PDF export.

### API Endpoints

|Endpoint                      |Method  |Returns                                 |
|------------------------------|--------|----------------------------------------|
|`/api/v1/health`              |GET     |System health status                    |
|`/api/v1/status`              |GET     |Current metrics, operation, campaign    |
|`/api/v1/drilling`            |GET     |Live drilling metrics                   |
|`/api/v1/diagnosis`           |GET     |Current strategic advisory (204 if none)|
|`/api/v1/history`             |GET     |Last 50 advisories                      |
|`/api/v1/verification`        |GET     |Latest ticket verification result       |
|`/api/v1/baseline`            |GET     |Baseline learning progress              |
|`/api/v1/campaign`            |GET/POST|Current campaign / switch campaign      |
|`/api/v1/config`              |GET/POST|Well configuration / update thresholds  |
|`/api/v1/config/validate`     |POST    |Validate config without applying        |
|`/api/v1/advisory/acknowledge`|POST    |Acknowledge advisory (audit trail)      |
|`/api/v1/shift/summary`       |GET     |Shift summary with time-range filter    |
|`/api/v1/ml/latest`           |GET     |Latest ML insights                      |
|`/api/v1/ml/optimal?depth=N`  |GET     |Optimal parameters for depth            |
|`/api/v1/reports/critical`    |GET     |Critical advisory reports               |

### Advisory Log

Every advisory is written to local storage (CSV/JSON) for post-well analysis and the daily check-in with the OIM.

### PostgreSQL Persistence (Future)

Advisories, ML reports, and system metrics will be persisted to PostgreSQL (lifted from Sentrix DAL). This enables:

- Historical queries across wells
- Shift summaries spanning multiple days
- Fleet event upload payloads
- Audit trails for regulatory compliance

-----

## 13. Background Services

These run continuously, independent of the main pipeline.

### ML Engine V2.2

**Cadence:** Every hour (configurable via `ML_INTERVAL_SECS`)

**Purpose:** Find optimal drilling parameters for the current formation.

See [Section 16: ML Engine](#16-ml-engine) for full details.

### Baseline Learning

**Duration:** First 100 samples per metric (approximately 2 minutes at 1 Hz)

**Purpose:** Learn what “normal” looks like for this specific rig.

See [Section 15: Baseline Learning](#15-baseline-learning) for full details.

### Self-Healer (Sentrix Pattern — Future)

**Cadence:** Continuous (every 30 seconds)

**Monitors:**

- WITS connection alive?
- LLM loaded and responding?
- Disk space sufficient?
- Dashboard serving?
- ChromaDB connected? (fleet mode)
- PostgreSQL healthy? (if enabled)

**On failure:**

- Auto-restart failed service
- Reconnect with exponential backoff
- Fall back to templates if LLM fails
- Log failure to knowledge store for pattern learning
- Alert via dashboard banner

-----

## 14. Campaign System

### What It Is

Campaigns change the system’s personality. Different drilling operations have different priorities, different danger thresholds, and different optimisation goals.

### Available Campaigns

|Campaign              |Focus                             |Flow Warning|Flow Critical|Well Control Weight|
|----------------------|----------------------------------|------------|-------------|-------------------|
|**Production**        |ROP optimisation, MSE efficiency  |10 gpm      |20 gpm       |30%                |
|**Plug & Abandonment**|Cement integrity, pressure testing|5 gpm       |15 gpm       |40%                |

### What Changes Per Campaign

1. **Thresholds:** P&A has tighter flow balance thresholds (5 gpm warning vs 10 gpm) because cement operations are more sensitive.
2. **Specialist weights:** P&A increases Well Control weight to 40% (from 30%) because barrier integrity is paramount.
3. **LLM prompts:** Campaign context is injected into LLM prompts so recommendations are contextually appropriate.
4. **ML optimisation weights:** P&A prioritises stability (30% weight) over ROP (25%), whereas Production prioritises ROP (50%).
5. **Operation detection:** Milling and Cement Drill-Out operations are only detected in P&A mode.

### Switching Campaigns

Three methods:

- **Dashboard dropdown** (top-left)
- **API:** `POST /api/v1/campaign` with `{"campaign":"PlugAbandonment"}`
- **Environment variable:** `CAMPAIGN=pa` at startup

Campaign switches are immediate — thresholds update on the next packet.

-----

## 15. Baseline Learning

### What It Is

Every rig is different. Motor #3 runs hotter. Pump #2 has higher SPP. The drill crew on this rig applies WOB differently than the crew on the next rig. Generic thresholds fail because they don’t account for these individual characteristics.

Baseline learning solves this by watching the rig for its first 100 samples per metric and building rig-specific “normal” ranges.

### How It Works

For each tracked metric (WOB, RPM, torque, SPP, flow_in, flow_out, pit_volume):

1. **Accumulate:** Collect 100 samples during normal drilling operations.
2. **Calculate:** Compute mean (μ) and standard deviation (σ) for each metric.
3. **Lock:** Set adaptive thresholds:
- Warning = μ + 2σ
- Critical = μ + 3σ
1. **Persist:** Save locked thresholds to `data/baseline_state.json`.

### Shadow Mode

During the baseline learning period (first ~2 minutes), the system operates in **shadow mode** — it calculates everything but generates no alerts. This ensures zero operational impact during the learning period.

### Crash Recovery

Locked thresholds are persisted to `data/baseline_state.json` after each metric locks. On restart, the system reloads locked thresholds and doesn’t need to re-learn. In-progress accumulators are intentionally not persisted — learning restarts cleanly for any metric that hadn’t locked yet.

-----

## 16. ML Engine V2.2

### What It Is

An hourly analysis pipeline that finds optimal drilling parameters for the current formation. Unlike the real-time pipeline (physics + agents), the ML engine works on batches of historical data.

### Pipeline Stages

**Stage 1: Data Collection**  
Accumulate up to 2 hours of WITS packets at 1 Hz (up to 7,200 samples).

**Stage 2: Quality Filtering**  
Reject sensor glitches, out-of-range values, and packets from non-drilling states (connections, tripping, idle).

**Stage 3: Dysfunction Filtering (V2.2 feature)**  
Reject samples where the rig was in a dysfunctional state:

- Torque instability > 12% CV (stick-slip precursor)
- Pack-off signatures (torque + SPP both elevated)
- Founder conditions (WOB up, ROP not responding)
- Low MSE efficiency < 50%

**Why this matters:** Without dysfunction filtering, the ML engine would learn from bad data. If the rig spent 30 minutes in stick-slip, those 1,800 samples would pollute the optimal parameter calculation. V2.2 ensures the ML only learns from stable, sustainable operations.

**Stage 4: Formation Segmentation**  
Detect formation boundaries via d-exponent shifts > 15%. If the d-exponent changes significantly, the rig has entered a new formation and the optimal parameters are likely different.

**Stage 5: Correlation Analysis**  
Compute Pearson correlations between WOB/RPM/Flow and ROP with p-value significance testing. The pipeline proceeds even if p > 0.05 but flags the results as low confidence.

**Stage 6: Grid-Based Binning**  
Create an 8×6 grid of WOB × RPM bins. For each bin, calculate average ROP, MSE, and stability score. Apply a stability penalty to bins near dysfunction thresholds.

**Why grid binning (not top-10% averaging):** The old approach (V2.1) would average the top 10% of ROP values, but those values might come from completely different WOB/RPM combinations. The grid ensures that recommended parameters were actually used together in practice.

**Stage 7: Composite Scoring**  
Score each bin using campaign-aware weights:

|Campaign  |ROP Weight|MSE Weight|Stability Weight|
|----------|----------|----------|----------------|
|Production|50%       |30%       |20%             |
|P&A       |25%       |45%       |30%             |

**Stage 8: Report Generation**  
Store optimal parameters with safe operating ranges:

```
Optimal Parameters (Sandstone, 2800-3000m):
  WOB:  35-42 klbs (optimal: 38 klbs)
  RPM:  110-130 (optimal: 120)
  Flow: 520-580 gpm (optimal: 550 gpm)
  
  Expected ROP: 32-38 ft/hr
  Expected MSE: 38,000-44,000 psi
  
  Stability Score: 0.87 (HIGH)
  Confidence: HIGH (p < 0.01, 450 samples)
  Dysfunction Filtered: 127 samples rejected
```

-----

## 17. Configuration System

### What It Is

Every hardcoded threshold in SAIREN-OS has been replaced with a configurable TOML file. 43 thresholds total, all in one place.

### Configuration Hierarchy

The system searches for configuration in order:

1. `$SAIREN_CONFIG` environment variable (if set)
2. `./well_config.toml` in the working directory
3. Built-in defaults (safe for most wells)

### Key Sections

```toml
[well]
well_name = "WELL-001"
rig_id = "V72"
bit_diameter_inches = 8.5

[thresholds.well_control]
flow_imbalance_warning_gpm = 10.0
flow_imbalance_critical_gpm = 20.0
pit_gain_warning_bbl = 5.0
pit_gain_critical_bbl = 10.0
pit_rate_critical_bbl_hr = 15.0
gas_warning_units = 100.0
gas_critical_units = 500.0
h2s_warning_ppm = 10.0
h2s_critical_ppm = 20.0

[thresholds.mse]
efficiency_poor_percent = 50.0
target_min_psi = 35000.0
target_max_psi = 50000.0

[thresholds.hydraulics]
ecd_margin_warning_ppg = 0.3
ecd_margin_critical_ppg = 0.1
spp_deviation_warning_psi = 100.0
spp_deviation_critical_psi = 200.0

[thresholds.mechanical]
torque_increase_warning_pct = 15.0
torque_increase_critical_pct = 25.0
torque_cv_stickslip_pct = 12.0

[thresholds.founder]
wob_increase_pct = 5.0
rop_decrease_threshold = 0.0

[baseline_learning]
min_samples = 100
sigma_warning = 2.0
sigma_critical = 3.0

[ensemble_weights]
mse = 0.25
hydraulic = 0.25
well_control = 0.30
formation = 0.20

[physics]
normal_mud_weight_ppg = 10.0
bit_diameter_inches = 8.5

[campaign.plug_abandonment]
flow_imbalance_warning_gpm = 5.0
flow_imbalance_critical_gpm = 15.0
well_control_weight = 0.40
```

### Validation

On load, the system validates:

- Critical thresholds are strictly greater than warning thresholds
- Ensemble weights sum to approximately 1.0
- Sigma ordering (sigma_critical > sigma_warning)
- All values are within physically reasonable ranges

### Runtime Updates

Thresholds can be updated at runtime without restarting:

```bash
# Update a threshold
curl -X POST http://localhost:8080/api/v1/config \
  -H "Content-Type: application/json" \
  -d '{"config": {"thresholds": {"well_control": {"flow_imbalance_warning_gpm": 8.0}}}}'

# Validate without applying
curl -X POST http://localhost:8080/api/v1/config/validate \
  -H "Content-Type: application/json" \
  -d '{"config": {"ensemble_weights": {"mse": 0.5, "hydraulic": 0.5, "well_control": 0.0, "formation": 0.0}}}'
```

-----

## 18. Trait Architecture (Sentrix Integration)

### The Principle

Every major component boundary becomes a Rust trait. Traits enable:

- **Swappability:** Change the vector database without touching agents
- **Testability:** Mock implementations for unit tests
- **Graceful degradation:** Fall back to simpler implementations when resources are limited
- **Future-proofing:** Add new backends without modifying existing code

### Five Core Traits

|Trait             |Abstracts          |Current Impl   |Pilot Impl     |Fleet Impl         |
|------------------|-------------------|---------------|---------------|-------------------|
|`DataSource`      |Layer 0 (ingestion)|`WitsTcpSource`|Same           |+ `OpcUaSource`    |
|`InferenceBackend`|LLM layer          |`GgufBackend`  |Same           |+ `CloudApiBackend`|
|`KnowledgeStore`  |Fleet memory       |N/A            |`NoOpStore`    |`ChromaStore`      |
|`PersistenceLayer`|Storage            |`InMemoryDAL`  |Same           |`PostgresDAL`      |
|`CacheLayer`      |Hot data           |N/A            |`InMemoryCache`|`RedisCache`       |

### Implementation Priority (from merged architecture)

**Phase 1 (before V72 pilot):** Extract `DataSource` from `wits_parser.rs` and `InferenceBackend` from `mistral_rs.rs`. Both with InMemory/NoOp defaults so nothing breaks.

**Phase 2 (during/after pilot):** Lift `sentrix_dal` PostgreSQL crate → adapt schema for advisories. Lift Redis crate → cache physics results. Implement SelfHealer.

**Phase 3 (post-pilot):** Lift Sentrix ChromaDB crate → `KnowledgeStore` trait + impl. Advisory Composer queries KnowledgeStore. Deposit confirmed events back.

**Phase 4 (multi-rig):** WireGuard VPN. Event upload. Fleet library download.

-----

## 19. Persistence Layer

### Current State (v0.8)

Advisory history, ML reports, and system metrics live in memory. They’re lost on restart. Baseline state persists to `data/baseline_state.json`.

### Future State (Sentrix DAL Integration)

**PostgreSQL** stores:

- Advisory history (every advisory ever generated)
- ML analysis reports (hourly)
- System metrics (uptime, packet counts, latency)
- Audit logs (advisory acknowledgments, config changes)
- Event uploads (for fleet sync)

**Redis** caches:

- Last N physics results (for dashboard reads without recomputation)
- Current advisory (fast access)
- System health status
- Rate limiting (if API exposed externally)

**Schema (adapted from Sentrix DAL):**

```sql
CREATE TABLE advisories (
    id UUID PRIMARY KEY,
    advisory_number SERIAL,
    timestamp TIMESTAMPTZ NOT NULL,
    risk_level VARCHAR(20),
    efficiency_score FLOAT,
    category VARCHAR(50),
    recommendation TEXT,
    diagnosis TEXT,
    specialist_votes JSONB,
    campaign VARCHAR(30),
    operation VARCHAR(30),
    formation VARCHAR(50),
    depth FLOAT,
    acknowledged BOOLEAN DEFAULT FALSE,
    acknowledged_by VARCHAR(100),
    action_taken TEXT,
    outcome VARCHAR(30)
);

CREATE TABLE ml_reports (
    id UUID PRIMARY KEY,
    timestamp TIMESTAMPTZ NOT NULL,
    formation VARCHAR(50),
    depth_min FLOAT,
    depth_max FLOAT,
    optimal_wob FLOAT,
    optimal_rpm FLOAT,
    optimal_flow FLOAT,
    wob_range JSONB,
    rpm_range JSONB,
    flow_range JSONB,
    confidence VARCHAR(20),
    samples_used INT,
    samples_filtered INT,
    campaign VARCHAR(30)
);

CREATE TABLE audit_logs (
    id UUID PRIMARY KEY,
    timestamp TIMESTAMPTZ NOT NULL,
    event_type VARCHAR(50),
    actor VARCHAR(100),
    details JSONB
);
```

-----

## 20. Fleet Network (Multi-Rig)

### Architecture: Hub-and-Spoke

Each rig connects to its nearest regional hub via encrypted WireGuard VPN. Hubs sync with each other over terrestrial fibre.

```
                    Operator HQ
                    ┌───────────┐
                    │           │
              ┌─────┘           └─────┐
              │                       │
    ┌─────────▼──────┐     ┌─────────▼──────┐
    │ ABERDEEN HUB   │     │ HOUSTON HUB    │
    │ EMEA/Africa    │◄───►│ Americas       │
    │ ~18 rigs       │MPLS │ ~12 rigs       │
    └───────┬────────┘     └───────┬────────┘
            │                      │
    ┌───────┼───────┐      ┌──────┼───────┐
    │       │       │      │      │       │
   V72    V108   V109    V34    V56    V200
```

### What Gets Uploaded

**Trigger:** Tactical Agent creates AMBER or RED ticket AND Strategic Agent confirms.

**Payload (~10-50 MB compressed):**

- 10-minute sensor window around the event
- Event metadata (type, severity, outcome)
- Context (formation, depth, campaign, d-exponent, MSE)
- Physics features (flow balance, pit rate, ECD margin)
- Operator notes (if entered)
- Outcome (what happened next)
- ML insights (optimal vs actual parameters)

**What does NOT get uploaded:**

- Continuous WITS streams (14 GB/day)
- Normal baseline data
- Raw proprietary parameters (well locations, mud formulations)

### Bandwidth Math

30 rigs × 2 events/month × 25 MB = **1.5 GB/month total**. This fits within even the most constrained satellite links (Inmarsat FleetBroadband: 432 kbps = ~140 GB/month theoretical maximum).

### Fleet Library Sync

Every 6 hours, each hub pushes updated fleet knowledge to its connected rigs. This is a small package — the vector embeddings and metadata for new events, not the raw sensor data.

### Hub Hardware

Dell PowerEdge R250: Xeon E-2378, 64GB ECC RAM, 2× 2TB NVMe (RAID 1). Cost: £3,500 per hub. Handles 60 events/month ingestion with sub-100ms query latency.

-----

## 21. Edge Hardware & Deployment

### V72 Pilot Hardware

For the pilot, you’re using your personal RTX 5080 workstation:

- **Location:** Safe Zone (Maintenance Office or Radio Room)
- **Power:** 1× 240V socket, <450W draw, PAT tested
- **Network:** 1× Ethernet (RJ45) to rig data aggregator, one-way (ingress only)
- **Isolation:** Air-gapped — WiFi/Bluetooth removed/disabled, no internet

### Fleet Hardware (Per Rig)

```
GPU Compute Box (Industrial Ruggedized)
├── GPU: NVIDIA RTX 4060 Ti (16 GB VRAM)
├── CPU: Intel Core i7-13700K
├── RAM: 32 GB DDR5
├── Storage: 1 TB NVMe SSD
├── Cooling: Industrial fans (0-50°C operating range)
├── Ruggedness: IP54 sealed, vibration-dampened, ATEX Zone 2
├── Network: GbE (WITS) + satellite modem (VPN)
└── Cost: £4,500 installed
```

### Deployment Process

**Day 1 (2 hours):** Physical install — mount box, connect power and ethernet, power on.

**Day 1 (30 minutes):** Auto-configuration — system boots, discovers WITS endpoint, technician validates channel mappings via web UI.

**Days 1-7:** Baseline learning — shadow mode, no alerts, building rig-specific thresholds.

**Day 8+:** Production mode — advisories enabled, contributing to fleet intelligence.

### systemd Service

```ini
[Unit]
Description=SAIREN-OS Drilling Intelligence
After=network.target

[Service]
Type=simple
User=sairen
ExecStart=/opt/sairen-os/bin/sairen-os --wits-tcp localhost:5000
Restart=on-failure
RestartSec=5
StartLimitBurst=5
StartLimitIntervalSec=300

# Security hardening
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
ReadWritePaths=/opt/sairen-os/data /var/log/sairen-os

[Install]
WantedBy=multi-user.target
```

-----

## 22. Dashboard & API

### Three-Screen Model

**Screen 1 — Rig Floor Tactical Display**

Purpose: Quick situational awareness. The driller glances at this between pipe connections.

Content:

- 100-point operational score gauge (GREEN/YELLOW/RED)
- Rig state indicator (Drilling / Connection / Tripping / Idle)
- Operation type (Production / Milling / Cement Drill-Out)
- Live parameters with colour coding: WOB, ROP, RPM, SPP, Flow, Pit Volume
- Safety envelope bars (ECD margin, flow balance, pit rate)
- CRITICAL alerts only — red banner for immediate threats
- Nothing else. Every additional element is noise to a busy driller.

**Screen 2 — Company Man Strategic View**

Purpose: Strategic decision support for the Company Man (operator representative).

Content:

- Current strategic advisory with full recommendation text
- Expected benefit and risk level
- Advisory reasoning (specialist voting breakdown)
- ML insights: optimal parameters for current formation
- Safe operating ranges (WOB/RPM/Flow min-max)
- Last 10 advisories with efficiency trends

**Screen 3 — Engineering Deep Dive**

Purpose: Post-well analysis and troubleshooting.

Content:

- 24-hour parameter trends (all WITS channels)
- Physics calculations over time (MSE, d-exponent, ECD, flow balance)
- Formation analysis (d-exponent segmentation)
- Complete anomaly log (CRITICAL through YELLOW)
- ML reports (historical optimal parameters)
- Dysfunction event timeline
- PDF export (drilling report, efficiency summary)

### Advisory Acknowledgment

The OIM or driller can acknowledge advisories through the API:

```bash
curl -X POST http://localhost:8080/api/v1/advisory/acknowledge \
  -H "Content-Type: application/json" \
  -d '{
    "advisory_id": "ADV-042",
    "acknowledged_by": "J. Smith",
    "action_taken": "Reduced WOB to 28 klbs per recommendation"
  }'
```

This creates an audit trail — critical for the daily check-in with the OIM and for post-pilot review against the Success Criteria Scorecard.

### Shift Summary

```bash
curl "http://localhost:8080/api/v1/shift/summary?hours=12"
```

Returns a summary of the last 12 hours: total advisories, acceptance rate, CRITICAL events, uptime percentage, and any notable findings.

-----

## 23. Enhancement Roadmap (RAM Recall + Pattern Routing)

### RAM Recall (Phase 1 — 2 weeks)

**Problem:** External vector DB (Qdrant/ChromaDB) adds 8-15ms latency per query.

**Solution:** In-memory HNSW index with metadata filtering.

**Result:** 1-2ms recall latency, 260MB RAM cost, 7-12x speed improvement.

**How it works:**

1. On startup, load all episodes from external storage into RAM
2. Build HNSW spatial index over 384-dimensional embeddings
3. Maintain HashMap indices for campaign, event type, formation, outcome
4. On query: O(1) metadata filter → O(log n) HNSW search → return top-k
5. Async hourly backup to external storage for durability

### Pattern-Matched Routing (Phase 2 — 2 weeks)

**Problem:** Static rule-based routing achieves ~80% accuracy. Formation-blind, depth-blind, no self-correction.

**Solution:** Learned pattern matching with configurable triggers.

**Result:** 90%+ routing accuracy, 50% fewer false positives.

**How it works:**

1. Define patterns as combinations of triggers (metric thresholds, correlations, trends)
2. Each pattern tracks its own success/failure rate
3. Patterns are checked in accuracy-descending order
4. First matching pattern with confidence > 70% wins
5. If no pattern matches, fall back to static rules

**Example pattern: Pack-off signature**

```
Triggers:
  - torque_delta > 15%
  - spp_delta > 100 psi
  - torque AND spp rising simultaneously
  - ROP falling over 60 seconds

Context: Sandstone or siltstone formation
Confidence: 94% (47 successes, 3 failures)
```

### Learning Loop (Phase 3 — 1 week)

**Problem:** Patterns are static — they don’t improve from experience.

**Solution:** Outcome-based feedback loop.

**How it works:**

1. Every hour, batch-process all routing outcomes from the last hour
2. For each outcome, update the matching pattern’s success/failure count
3. Recompute accuracy = successes / (successes + failures)
4. Patterns that drop below 70% accuracy are demoted (fall back to rules)
5. Log pattern health to dashboard

-----

## 24. Data Flow: One Packet, Full Journey

Here’s exactly what happens to a single WITS packet that triggers an advisory:

```
1. TCP socket receives bytes: "01 08 35.50 10 18 01 14 18000 01 15 1800..."
   └─ wits_parser.rs parses ASCII → WitsPacket struct [2ms]

2. Quality gate validates all fields, injects UTC timestamp
   └─ All values within physical range? ✓ [<1ms]

3. Physics engine calculates derived values
   ├─ MSE = 52,000 psi (efficiency: 68%)
   ├─ d_exponent = 1.45 (stable)
   ├─ flow_balance = -3 gpm (normal)
   ├─ pit_rate = 0.5 bbl/hr (normal)
   ├─ ecd_margin = 0.45 ppg (safe)
   ├─ torque_cv = 8% (no stick-slip)
   ├─ torque_delta = +30% ← ABOVE WARNING (15%)
   └─ spp_delta = +150 psi ← ABOVE WARNING (100 psi) [10ms]

4. Tactical agent evaluates thresholds
   ├─ torque_delta 30% > warning 15% → anomaly counter++
   ├─ spp_delta 150 > warning 100 → anomaly counter++
   ├─ Both rising simultaneously → pack-off signature
   └─ Creates AMBER ticket: "Mechanical/PackOff" [3ms]

5. Packet pushed to history buffer (ring buffer, 60 capacity) [<1ms]

6. Strategic agent receives AMBER ticket
   ├─ Pulls 60-packet history
   ├─ Linear regression: torque trending upward (slope > 0, r² = 0.85)
   ├─ Linear regression: SPP trending upward (slope > 0, r² = 0.78)
   ├─ Founder check: WOB stable, not founder
   └─ Verdict: CONFIRM — sustained pack-off trend [50ms]

7. Knowledge store query (fleet mode)
   ├─ Search: "pack-off torque+SPP sandstone"
   ├─ Returns: 3 similar events from fleet
   └─ All resolved by WOB reduction + flow increase [1-15ms]

8. LLM reasoning (Qwen 2.5 7B)
   ├─ Input: physics verdict + fleet precedent + context
   ├─ Output: diagnosis + recommendation + expected benefit
   └─ "Developing pack-off. Reduce WOB 20%, increase flow 80 gpm" [750ms]

9. Orchestrator voting
   ├─ MSE specialist: ELEVATED (MSE 52K > target 35K)
   ├─ Hydraulic specialist: ELEVATED (SPP +150 psi)
   ├─ Well Control specialist: LOW (no kick/loss indicators)
   ├─ Formation specialist: LOW (d-exponent stable)
   └─ Composite: 0.81 → ELEVATED [2ms]

10. Advisory composed
    ├─ ADV-042: ELEVATED | Efficiency: 68%
    ├─ Category: Mechanical | Type: Pack-Off
    └─ Recommendation: Reduce WOB to 28 klbs, increase flow 80 gpm [2ms]

11. Output
    ├─ Dashboard push (websocket)
    ├─ Advisory log (CSV)
    ├─ PostgreSQL (if enabled)
    └─ Console log [5ms]

TOTAL: ~840ms (GPU) | ~35s (CPU) | ~70ms (no LLM, template fallback)
```

-----

## 25. Failure Modes & Fallbacks

|Failure                |Detection                     |Fallback                                              |Impact                                             |
|-----------------------|------------------------------|------------------------------------------------------|---------------------------------------------------|
|WITS TCP disconnected  |Read timeout (120s)           |Reconnect with backoff. Dashboard shows “DISCONNECTED”|Pipeline pauses, no new data                       |
|LLM model fails to load|`is_available()` returns false|Template-based advisories                             |Less nuanced recommendations, same physics accuracy|
|GPU failure            |CUDA init fails               |CPU inference (slower) or templates                   |10-30s strategic latency instead of 800ms          |
|ChromaDB down          |Health check fails            |`InMemoryStore` or `NoOpStore`                        |No fleet knowledge, local physics still works      |
|PostgreSQL down        |Connection refused            |`InMemoryDAL`                                         |No persistence, advisories still generated         |
|Redis down             |Connection refused            |`InMemoryCache`                                       |Slightly slower dashboard reads                    |
|Disk full              |Health check                  |Alert via dashboard, stop writing logs                |Core pipeline unaffected                           |
|Baseline not locked    |Sample count < 100            |Use config-file thresholds                            |Less personalised thresholds                       |

**The key principle:** Every failure mode has a fallback that preserves the core function (physics calculations + threshold detection). The system degrades gracefully — losing fleet knowledge or LLM reasoning is unfortunate but not dangerous. Losing physics is impossible (it’s deterministic code, not a service).

-----

## 26. Build Order & Implementation Phases

### Phase 1: V72 Pilot (Current Priority)

**What exists:** Everything in the “SAIREN-OS v0.8 KEEP AS-IS” column from the merged architecture. Physics engine, tactical/strategic agents, orchestrator, ML engine, baseline learning, campaign system, config system, dashboard, WITS parser, systemd deployment.

**What to build before pilot:**

1. Extract `trait DataSource` from `wits_parser.rs` (2-3 hours)
2. Extract `trait InferenceBackend` from `mistral_rs.rs` (2-3 hours)
3. Both with InMemory/NoOp defaults so nothing breaks

**What NOT to build before pilot:** PostgreSQL, Redis, ChromaDB, fleet networking, RAM recall, pattern routing. These add complexity with zero benefit for a single-rig, 7-day pilot.

### Phase 2: Post-Pilot Hardening

**If pilot succeeds:**

1. Lift `sentrix_dal` PostgreSQL crate → adapt schema for advisories
2. Lift `sentrix_dal` Redis crate → cache physics results for dashboard
3. Implement SelfHealer service (watch WITS, LLM, disk, dashboard)
4. Implement RAM Recall (in-memory HNSW for fast episode retrieval)
5. Implement Pattern-Matched Routing (learned patterns with outcome feedback)

### Phase 3: Fleet Preparation

1. Lift Sentrix ChromaDB crate → `KnowledgeStore` trait + `ChromaStore` impl
2. Advisory Composer queries KnowledgeStore before generating recommendations
3. Deposit confirmed events back to KnowledgeStore
4. Build event upload format (10-minute sensor window + metadata)

### Phase 4: Multi-Rig Deployment

1. WireGuard VPN to regional hub
2. Event upload (AMBER/RED tickets → hub)
3. Fleet library download (hub → all rigs, every 6 hours)
4. Hub-to-hub replication (Aberdeen ↔ Houston)
5. Monitoring dashboard for fleet-wide health

-----

## Appendix: Key Numbers

|Metric                      |Value                      |
|----------------------------|---------------------------|
|WITS channels monitored     |40+                        |
|Data rate                   |1 Hz (1 packet/second)     |
|Physics calculation time    |~10ms                      |
|Tactical Agent decision time|~3ms                       |
|Strategic Agent verification|~50ms                      |
|LLM advisory (GPU)          |~750ms                     |
|LLM advisory (CPU)          |~10-30s                    |
|Template advisory (no LLM)  |~1ms                       |
|History buffer depth        |60 packets                 |
|Baseline learning period    |~100 samples (~2 minutes)  |
|ML analysis interval        |1 hour                     |
|ML data window              |Up to 2 hours              |
|CRITICAL cooldown           |30 seconds                 |
|Advisory success target     |>90% safe and correct      |
|Detection speed target      |<1 second for state changes|
|Uptime target               |>90%                       |
|RAM Recall latency (target) |1-2ms                      |
|Fleet event upload size     |10-50 MB                   |
|Fleet bandwidth (30 rigs)   |~1.5 GB/month              |
|Edge box VRAM usage         |~8 GB of 16 GB             |
|Hub query latency           |<100ms                     |

-----

*This document is the blueprint. Read it, challenge it, improve it. Then build exactly what it describes.*