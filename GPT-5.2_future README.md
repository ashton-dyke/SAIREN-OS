# SAIREN-OS: Complete System Technical Specification

**Technical Deep-Dive: How Every Stage Functions**

---

## Document Purpose

This document provides a stage-by-stage technical analysis of how SAIREN-OS operates from raw sensor data ingestion through fleet-wide learning. Think of this as the "implementation blueprint" that bridges architectural diagrams to actual code execution.

---

## Table of Contents

1. [System Overview](notion://www.notion.so/30085995ec1f806f86bfcca5060f240b?pvs=15&showMoveTo=true&saveParent=true#1-system-overview)
2. [Stage 0: Data Ingestion](notion://www.notion.so/30085995ec1f806f86bfcca5060f240b?pvs=15&showMoveTo=true&saveParent=true#2-stage-0-data-ingestion)
3. [Stage 1: Fast Path (Tactical Agent)](notion://www.notion.so/30085995ec1f806f86bfcca5060f240b?pvs=15&showMoveTo=true&saveParent=true#3-stage-1-fast-path-tactical-agent)
4. [Stage 2: Deep Path (Strategic Agent)](notion://www.notion.so/30085995ec1f806f86bfcca5060f240b?pvs=15&showMoveTo=true&saveParent=true#4-stage-2-deep-path-strategic-agent)
5. [Stage 3: Knowledge & Reasoning](notion://www.notion.so/30085995ec1f806f86bfcca5060f240b?pvs=15&showMoveTo=true&saveParent=true#5-stage-3-knowledge--reasoning)
6. [Stage 4: Persistence & Output](notion://www.notion.so/30085995ec1f806f86bfcca5060f240b?pvs=15&showMoveTo=true&saveParent=true#6-stage-4-persistence--output)
7. [Stage 5: Background Services](notion://www.notion.so/30085995ec1f806f86bfcca5060f240b?pvs=15&showMoveTo=true&saveParent=true#7-stage-5-background-services)
8. [Stage 6: Fleet Network](notion://www.notion.so/30085995ec1f806f86bfcca5060f240b?pvs=15&showMoveTo=true&saveParent=true#8-stage-6-fleet-network)
9. [Data Flow Examples](notion://www.notion.so/30085995ec1f806f86bfcca5060f240b?pvs=15&showMoveTo=true&saveParent=true#9-data-flow-examples)
10. [Performance Characteristics](notion://www.notion.so/30085995ec1f806f86bfcca5060f240b?pvs=15&showMoveTo=true&saveParent=true#10-performance-characteristics)
11. [Failure Modes & Recovery](notion://www.notion.so/30085995ec1f806f86bfcca5060f240b?pvs=15&showMoveTo=true&saveParent=true#11-failure-modes--recovery)
12. [Deployment Considerations](notion://www.notion.so/30085995ec1f806f86bfcca5060f240b?pvs=15&showMoveTo=true&saveParent=true#12-deployment-considerations)

---

## 1. System Overview

### High-Level Philosophy

SAIREN-OS operates on a **tiered intelligence model**:

- **Layer 0 (Data)**: Protocol-agnostic ingestion
- **Layer 1 (Fast)**: Reflexive safety monitoring (<15ms)
- **Layer 2 (Deep)**: Analytical verification (on anomaly only, ~500-1000ms)
- **Layer 3 (Reasoning)**: Multi-source intelligence synthesis
- **Layer 4 (Output)**: Multi-channel distribution
- **Layer 5 (Learning)**: Continuous optimization
- **Layer 6 (Fleet)**: Collective intelligence

### Core Data Structure

Every packet that enters the system becomes a `WitsPacket`:

```rust
pub struct WitsPacket {
    pub timestamp: DateTime<Utc>,
    pub depth: f64,              // Current hole depth (meters or feet)
    pub wob: f64,                // Weight on Bit (klbs)
    pub rop: f64,                // Rate of Penetration (ft/hr)
    pub rpm: f64,                // Rotary Speed
    pub torque: f64,             // Torque (ft-lbs or kN.m)
    pub spp: f64,                // Standpipe Pressure (psi)
    pub flow_in: f64,            // Pump flow rate (gpm)
    pub flow_out: f64,           // Return flow rate (gpm)
    pub pit_volume: f64,         // Active pit volume (bbl)
    pub mud_weight_in: f64,      // Mud density entering hole (ppg)
    pub mud_weight_out: f64,     // Mud density returning (ppg)
    pub gas_total: f64,          // Total gas units
    pub h2s: f64,                // H2S concentration (ppm)
    pub hook_load: f64,          // Hook load (klbs)
    // ... 40+ channels total

    // Derived state
    pub rig_state: RigState,     // Drilling, Circulating, Tripping, etc.
    pub quality: DataQuality,    // Good, Suspect, Invalid
}

```

---

## 2. Stage 0: Data Ingestion

### Purpose

Convert heterogeneous industrial protocols (WITS Level 0, OPC-UA, Modbus) into unified `WitsPacket` format with quality validation.

### Components

### 2.1 DataSource Trait (Protocol Abstraction)

```rust
#[async_trait]
pub trait DataSource: Send + Sync {
    async fn next_packet(&mut self) -> Result<WitsPacket, IngestionError>;
    async fn reconnect(&mut self) -> Result<(), IngestionError>;
    fn health_check(&self) -> HealthStatus;
    fn source_type(&self) -> &str;  // "WITS-TCP", "OPC-UA", "Modbus-RTU"
}

```

**Why trait-based?** Allows swapping data sources without touching agent logic. V72 uses WITS TCP today, but future rigs might use OPC-UA or Modbus.

### 2.2 WITS TCP Implementation

```rust
pub struct WitsTcpSource {
    tcp_stream: Option<TcpStream>,
    endpoint: String,
    buffer: Vec<u8>,
    reconnect_attempts: u32,
    last_packet_time: Instant,
}

impl DataSource for WitsTcpSource {
    async fn next_packet(&mut self) -> Result<WitsPacket, IngestionError> {
        // 1. Read raw bytes from TCP stream
        let raw_bytes = self.read_until_delimiter(b'\\n').await?;

        // 2. Parse WITS Level 0 format
        let parsed = self.parse_wits_record(&raw_bytes)?;

        // 3. Quality validation
        if self.is_all_zeros(&parsed) {
            return Err(IngestionError::AllZeroPacket);
        }

        if !self.range_check(&parsed) {
            return Err(IngestionError::OutOfRange);
        }

        // 4. Classify rig state
        let rig_state = self.classify_rig_state(&parsed);

        Ok(WitsPacket {
            timestamp: Utc::now(),
            rig_state,
            quality: DataQuality::Good,
            ..parsed
        })
    }

    async fn reconnect(&mut self) -> Result<(), IngestionError> {
        self.tcp_stream = None;
        self.reconnect_attempts += 1;

        // Exponential backoff: 1s, 2s, 4s, 8s, max 30s
        let delay = std::cmp::min(2_u64.pow(self.reconnect_attempts), 30);
        tokio::time::sleep(Duration::from_secs(delay)).await;

        match TcpStream::connect(&self.endpoint).await {
            Ok(stream) => {
                self.tcp_stream = Some(stream);
                self.reconnect_attempts = 0;
                log::info!("✅ WITS reconnected to {}", self.endpoint);
                Ok(())
            }
            Err(e) => Err(IngestionError::ConnectionFailed(e)),
        }
    }
}

```

**Key Features:**

- **Auto-reconnect**: Handles satellite handoffs, weather interruptions
- **Quality gate**: Rejects all-zero packets (sensor failure), out-of-range values
- **Rig state classification**: Determines if drilling, circulating, tripping, etc.

### 2.3 Data Quality Gate

```rust
fn range_check(&self, packet: &WitsPacket) -> bool {
    // Physical impossibility checks
    if packet.wob < 0.0 || packet.wob > 150.0 {  // 0-150 klbs typical
        return false;
    }

    if packet.rop < 0.0 || packet.rop > 500.0 {  // 0-500 ft/hr max
        return false;
    }

    if packet.spp < 0.0 || packet.spp > 10000.0 {  // 0-10,000 psi
        return false;
    }

    // Consistency checks
    if packet.flow_out > packet.flow_in * 2.0 {  // Can't return 2x input
        return false;
    }

    true
}

fn is_all_zeros(&self, packet: &WitsPacket) -> bool {
    packet.wob == 0.0 &&
    packet.rop == 0.0 &&
    packet.rpm == 0.0 &&
    packet.torque == 0.0 &&
    packet.spp == 0.0
}

```

**Output:** Clean, validated `WitsPacket` ready for physics analysis.

---

## 3. Stage 1: Fast Path (Tactical Agent)

### Purpose

Real-time safety monitoring and anomaly detection with <15ms latency. Runs on EVERY packet.

### 3.1 Physics Engine

```rust
pub struct PhysicsEngine {
    baseline: Arc<RwLock<BaselineState>>,
    config: Arc<WellConfig>,
}

pub struct DrillingMetrics {
    // Calculated physics
    pub mse: f64,              // Mechanical Specific Energy (psi)
    pub mse_efficiency: f64,   // Percentage of optimal (0-100%)
    pub d_exponent: f64,       // Formation hardness indicator
    pub ecd: f64,              // Equivalent Circulating Density (ppg)
    pub ecd_margin: f64,       // Distance from fracture gradient (ppg)
    pub flow_balance: f64,     // flow_out - flow_in (gpm)
    pub pit_rate: f64,         // Rate of pit volume change (bbl/hr)

    // Anomaly flags
    pub is_kick_warning: bool,
    pub is_loss_warning: bool,
    pub is_pack_off: bool,
    pub is_stick_slip: bool,
    pub is_founder: bool,
    pub is_washout: bool,

    // Context
    pub severity: Severity,
    pub category: Category,
}

impl PhysicsEngine {
    pub fn analyze(
        &self,
        packet: &WitsPacket,
        history: &VecDeque<WitsPacket>,
    ) -> DrillingMetrics {
        // 1. Calculate MSE (~2ms)
        let mse = self.calculate_mse(packet);
        let mse_optimal = 35000.0;  // From config or ML baseline
        let mse_efficiency = (mse_optimal / mse * 100.0).min(100.0);

        // 2. Calculate d-exponent (~1ms)
        let d_exp = self.calculate_d_exponent(packet);

        // 3. Calculate ECD (~1ms)
        let ecd = self.calculate_ecd(packet);
        let fracture_gradient = 16.5;  // From config
        let ecd_margin = fracture_gradient - ecd;

        // 4. Flow balance (~0.5ms)
        let flow_balance = packet.flow_out - packet.flow_in;

        // 5. Pit rate (smoothed over 5 minutes) (~1ms)
        let pit_rate = self.calculate_pit_rate(history);

        // 6. Dysfunction detection (~3ms)
        let is_pack_off = self.detect_pack_off(packet, history);
        let is_stick_slip = self.detect_stick_slip(history);
        let is_founder = self.detect_founder(history);

        // 7. Well control checks (~2ms)
        let is_kick_warning = flow_balance > 20.0 || pit_rate > 15.0;
        let is_loss_warning = flow_balance < -20.0 || pit_rate < -15.0;

        // 8. Categorize (~1ms)
        let (severity, category) = self.categorize(&metrics);

        DrillingMetrics {
            mse,
            mse_efficiency,
            d_exponent: d_exp,
            ecd,
            ecd_margin,
            flow_balance,
            pit_rate,
            is_kick_warning,
            is_loss_warning,
            is_pack_off,
            is_stick_slip,
            is_founder,
            severity,
            category,
        }
    }
}

```

### 3.1.1 MSE Calculation

**Formula:**

```
MSE = (WOB / bit_area) + (120 * RPM * Torque) / (bit_area * ROP)

Where:
- bit_area = π * (bit_diameter / 2)²
- WOB in pounds
- Torque in ft-lbs
- ROP in ft/hr
- Result in psi

```

**Implementation:**

```rust
fn calculate_mse(&self, packet: &WitsPacket) -> f64 {
    let bit_diameter = self.config.bit_diameter_inches;
    let bit_area = std::f64::consts::PI * (bit_diameter / 2.0).powi(2);

    let wob_component = (packet.wob * 1000.0) / bit_area;

    let rotary_component = if packet.rop > 0.1 {
        (120.0 * packet.rpm * packet.torque) / (bit_area * packet.rop)
    } else {
        0.0  // Avoid division by zero
    };

    wob_component + rotary_component
}

```

**Why MSE matters:** Lower MSE = more efficient drilling. Target: 35,000-50,000 psi. Exceeding 80,000 psi indicates inefficiency (wrong WOB/RPM combination).

### 3.1.2 Pack-Off Detection

```rust
fn detect_pack_off(
    &self,
    current: &WitsPacket,
    history: &VecDeque<WitsPacket>,
) -> bool {
    if history.len() < 10 {
        return false;
    }

    // Get baseline from 60 seconds ago
    let baseline_idx = history.len().saturating_sub(60);
    let baseline = &history[baseline_idx];

    // Check for simultaneous increases
    let torque_increase = (current.torque - baseline.torque) / baseline.torque * 100.0;
    let spp_increase = current.spp - baseline.spp;
    let rop_decrease = (baseline.rop - current.rop) / baseline.rop * 100.0;

    // Pack-off signature:
    // - Torque up >15%
    // - SPP up >100 psi
    // - ROP down >20%
    torque_increase > 15.0 &&
    spp_increase > 100.0 &&
    rop_decrease > 20.0
}

```

### 3.1.3 Founder Detection

```rust
fn detect_founder(&self, history: &VecDeque<WitsPacket>) -> bool {
    if history.len() < 30 {
        return false;
    }

    // Linear regression on last 30 points
    let (wob_slope, wob_r2) = self.linear_trend(
        history.iter().map(|p| p.wob).collect()
    );
    let (rop_slope, rop_r2) = self.linear_trend(
        history.iter().map(|p| p.rop).collect()
    );

    // Founder condition:
    // - WOB increasing (slope > 0, R² > 0.7)
    // - ROP flat or decreasing (slope ≤ 0, R² > 0.5)
    wob_slope > 0.0 && wob_r2 > 0.7 &&
    rop_slope <= 0.0 && rop_r2 > 0.5
}

```

### 3.2 Tactical Agent Decision Gate

```rust
pub struct TacticalAgent {
    physics_engine: Arc<PhysicsEngine>,
    pattern_matcher: Arc<RwLock<PatternMatcher>>,
}

impl TacticalAgent {
    pub async fn analyze(
        &self,
        packet: &WitsPacket,
        history: &VecDeque<WitsPacket>,
    ) -> TacticalResult {
        // Run physics (~10ms)
        let metrics = self.physics_engine.analyze(packet, history);

        // Decision gate
        if metrics.severity == Severity::Green {
            return TacticalResult::Continue;  // No action needed
        }

        // Create ticket for Strategic Agent
        let ticket = AdvisoryTicket {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            metrics: metrics.clone(),
            history_snapshot: history.clone(),
            depth: packet.depth,
            formation: self.get_current_formation(packet.depth),
        };

        TacticalResult::EscalateToStrategic(ticket)
    }
}

```

**Key Decision:** Only 5-10% of packets create tickets. 90-95% return immediately without Strategic Agent involvement.

---

## 4. Stage 2: Deep Path (Strategic Agent)

### Purpose

Physics-based verification and trend analysis. Runs ONLY when Tactical Agent escalates.

### 4.1 Ticket Verification

```rust
pub struct StrategicAgent {
    physics_engine: Arc<PhysicsEngine>,
}

pub enum VerificationResult {
    Confirmed { confidence: f64, reason: String },
    Uncertain { confidence: f64, reason: String },
    Rejected { reason: String },
}

impl StrategicAgent {
    pub fn verify_ticket(
        &self,
        ticket: &AdvisoryTicket,
    ) -> VerificationResult {
        let metrics = &ticket.metrics;

        // 1. Physics double-check
        if metrics.is_kick_warning {
            if self.confirm_kick_signature(ticket) {
                return VerificationResult::Confirmed {
                    confidence: 0.95,
                    reason: format!(
                        "Flow imbalance {:.1} gpm, pit gain {:.1} bbl",
                        metrics.flow_balance,
                        metrics.pit_rate * 0.0167  // Convert bbl/hr to bbl
                    ),
                };
            } else {
                return VerificationResult::Rejected {
                    reason: "Flow imbalance transient, pit volume stable".to_string(),
                };
            }
        }

        // 2. Pack-off verification
        if metrics.is_pack_off {
            let trend_confirms = self.analyze_pack_off_trend(&ticket.history_snapshot);
            if trend_confirms {
                return VerificationResult::Confirmed {
                    confidence: 0.88,
                    reason: "Torque+SPP rising together, ROP declining".to_string(),
                };
            } else {
                return VerificationResult::Uncertain {
                    confidence: 0.60,
                    reason: "Pack-off signature present but not sustained".to_string(),
                };
            }
        }

        // 3. Founder verification
        if metrics.is_founder {
            let (optimal_wob, current_wob) = self.estimate_founder_point(&ticket.history_snapshot);
            return VerificationResult::Confirmed {
                confidence: 0.85,
                reason: format!(
                    "WOB {:.0} klbs exceeds optimal {:.0} klbs, ROP not responding",
                    current_wob, optimal_wob
                ),
            };
        }

        // Default: Escalate to Layer 3 for deeper analysis
        VerificationResult::Uncertain {
            confidence: 0.70,
            reason: "Requires contextual analysis".to_string(),
        }
    }

    fn confirm_kick_signature(&self, ticket: &AdvisoryTicket) -> bool {
        let history = &ticket.history_snapshot;

        // Sustained flow imbalance over 5+ minutes
        let last_30_packets: Vec<_> = history.iter().rev().take(30).collect();
        let avg_flow_balance: f64 = last_30_packets.iter()
            .map(|p| p.flow_out - p.flow_in)
            .sum::<f64>() / last_30_packets.len() as f64;

        // Pit volume increasing
        let pit_first = history[0].pit_volume;
        let pit_last = history[history.len() - 1].pit_volume;
        let pit_gain = pit_last - pit_first;

        avg_flow_balance > 15.0 && pit_gain > 5.0
    }
}

```

**Verification outcomes:**

- **Confirmed (70%)**: Escalate to Layer 3 for full advisory generation
- **Uncertain (20%)**: Log as low-priority, monitor
- **Rejected (10%)**: Discard, false positive

---

## 5. Stage 3: Knowledge & Reasoning

### Purpose

Multi-source intelligence synthesis: fleet precedent + LLM reasoning + specialist voting.

### 5.1 Knowledge Store Query

```rust
pub struct KnowledgeStore {
    ram_recall: Arc<RwLock<RAMRecall>>,
}

impl KnowledgeStore {
    pub async fn query_precedent(
        &self,
        ticket: &AdvisoryTicket,
    ) -> Vec<FleetEpisode> {
        // Generate query embedding
        let query_text = format!(
            "campaign:{} category:{} depth:{:.0}m formation:{} flow_balance:{:.1} torque_delta:{:.1}",
            ticket.campaign,
            ticket.metrics.category,
            ticket.depth,
            ticket.formation,
            ticket.metrics.flow_balance,
            ticket.metrics.torque_delta,
        );

        let query_embedding = self.embed_text(&query_text).await?;

        // RAM-based similarity search (1-2ms)
        let recall = self.ram_recall.read().await;
        let similar = recall.search_similar(
            &query_embedding,
            &ticket.campaign.to_string(),
            &ticket.metrics.category.to_string(),
            5,  // Top 5 matches
        );

        similar
    }
}

```

**Example query result:**

```
Found 3 similar episodes:

Episode #1 (Rig V108, 18 days ago):
- Pack-off in sandstone at 2,920m
- Torque: 12,500 → 17,800 ft-lbs
- SPP: 2,750 → 2,920 psi
- Resolution: Reduced WOB 25%, increased flow 80 gpm
- Outcome: Resolved in 15 minutes, drilling resumed

Episode #2 (Rig V72, 45 days ago):
- Pack-off in siltstone at 3,100m
- Similar signature
- Resolution: Backed off 10m, circulated, reduced WOB 20%
- Outcome: Resolved in 30 minutes

Episode #3 (Rig V109, 3 months ago):
- Pack-off in sandstone at 2,850m
- Resolution: Reduced WOB, increased flow
- Outcome: Resolved

```

### 5.2 LLM Reasoning

```rust
pub struct LLMReasoning {
    tactical_llm: Arc<TacticalLLM>,   // Qwen 2.5 1.5B
    strategic_llm: Arc<StrategicLLM>, // Qwen 2.5 7B
}

impl LLMReasoning {
    pub async fn generate_recommendation(
        &self,
        ticket: &AdvisoryTicket,
        verification: &VerificationResult,
        fleet_precedent: &[FleetEpisode],
    ) -> LLMRecommendation {
        // Build context
        let context = format!(
            r#"Current situation:
- Depth: {:.0}m
- Formation: {}
- Torque: {:.0} ft-lbs (baseline: {:.0})
- SPP: {:.0} psi (baseline: {:.0})
- ROP: {:.1} ft/hr (target: {:.1})
- Flow balance: {:.1} gpm

Physics analysis: {}

Fleet precedent:
{}"#,
            ticket.depth,
            ticket.formation,
            ticket.current_torque,
            ticket.baseline_torque,
            ticket.current_spp,
            ticket.baseline_spp,
            ticket.current_rop,
            ticket.target_rop,
            ticket.metrics.flow_balance,
            verification.reason,
            self.format_precedent(fleet_precedent),
        );

        let prompt = format!(
            r#"You are a drilling engineer analyzing a pack-off condition.

{}

Recommend specific actions with expected outcomes."#,
            context
        );

        // LLM inference (~500-800ms)
        let response = self.strategic_llm.infer(&prompt).await?;

        LLMRecommendation {
            text: response.text,
            confidence: response.confidence,
            reasoning: response.reasoning,
        }
    }
}

```

**Example LLM output:**

```
RECOMMENDATION:
1. Reduce WOB from 42 klbs to 32 klbs (-24%)
2. Increase flow rate from 520 gpm to 600 gpm (+15%)
3. Monitor torque and SPP for 10 minutes
4. If no improvement, back off 10m and circulate

EXPECTED OUTCOME:
- Torque should decrease to 13,000-14,000 ft-lbs within 5 minutes
- ROP may drop 10-15% initially but should stabilize
- Pack-off should clear without trip

REASONING:
Fleet data shows 3/3 similar cases resolved with WOB reduction + flow increase.
Current WOB (42 klbs) likely exceeds optimal for this formation based on founder analysis.

```

### 5.3 Orchestrator Voting

```rust
pub struct Orchestrator {
    specialists: Vec<Box<dyn Specialist>>,
    weights: HashMap<String, f64>,
}

pub trait Specialist {
    fn name(&self) -> &str;
    fn evaluate(&self, ticket: &AdvisoryTicket) -> SpecialistVote;
}

pub struct SpecialistVote {
    pub risk_level: RiskLevel,  // Low, Elevated, High, Critical
    pub confidence: f64,
    pub reason: String,
}

impl Orchestrator {
    pub fn vote(
        &self,
        ticket: &AdvisoryTicket,
    ) -> WeightedConsensus {
        let votes: Vec<_> = self.specialists.iter()
            .map(|specialist| {
                let vote = specialist.evaluate(ticket);
                let weight = self.weights.get(specialist.name()).unwrap_or(&0.25);
                (specialist.name().to_string(), vote, *weight)
            })
            .collect();

        // Weighted risk calculation
        let risk_scores: HashMap<RiskLevel, f64> = HashMap::new();
        for (_, vote, weight) in votes.iter() {
            *risk_scores.entry(vote.risk_level).or_insert(0.0) += weight;
        }

        let consensus_risk = risk_scores.iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(level, _)| *level)
            .unwrap_or(RiskLevel::Low);

        WeightedConsensus {
            risk_level: consensus_risk,
            votes,
            confidence: self.calculate_confidence(&votes),
        }
    }
}

```

**Example voting:**

```
MSE Specialist (25% weight): ELEVATED
- MSE 52,000 psi exceeds optimal 35,000 by 48%
- Confidence: 0.92

Hydraulic Specialist (25% weight): LOW
- Flow balance normal (-2 gpm)
- ECD margin acceptable (1.2 ppg)
- Confidence: 0.88

Well Control Specialist (30% weight): LOW
- No kick indicators
- Pit volume stable
- Confidence: 0.95

Formation Specialist (20% weight): LOW
- D-exponent stable (1.35)
- No formation transition
- Confidence: 0.85

WEIGHTED CONSENSUS: ELEVATED (MSE concern dominates)
Overall confidence: 0.89

```

### 5.4 Advisory Composer

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
            id: format!("ADV-{}", Uuid::new_v4().to_string()[..8]),
            timestamp: Utc::now(),
            risk_level: voting.risk_level,
            category: ticket.metrics.category,

            recommendation: llm_rec.text.clone(),
            expected_benefit: self.estimate_benefit(ticket, llm_rec),

            physics_verdict: verification.reason.clone(),
            fleet_precedent_summary: self.summarize_precedent(fleet_precedent),
            specialist_votes: voting.votes.clone(),

            confidence: voting.confidence,
        }
    }
}

```

---

## 6. Stage 4: Persistence & Output

### 6.1 PostgreSQL Storage

```sql
CREATE TABLE advisories (
    id UUID PRIMARY KEY,
    timestamp TIMESTAMPTZ NOT NULL,
    rig_id VARCHAR(50) NOT NULL,
    well_id VARCHAR(100) NOT NULL,

    risk_level VARCHAR(20) NOT NULL,
    category VARCHAR(50) NOT NULL,

    recommendation TEXT NOT NULL,
    physics_verdict TEXT,
    confidence DECIMAL(3,2),

    -- Physics snapshot
    depth DECIMAL(10,2),
    wob DECIMAL(8,2),
    rop DECIMAL(8,2),
    mse DECIMAL(10,2),
    torque DECIMAL(10,2),
    spp DECIMAL(10,2),

    -- Metadata
    acknowledged BOOLEAN DEFAULT FALSE,
    acknowledged_by VARCHAR(100),
    action_taken TEXT,
    outcome VARCHAR(50),  -- Resolved, Escalated, FalsePositive

    created_at TIMESTAMPTZ DEFAULT NOW(),
    INDEX idx_rig_timestamp (rig_id, timestamp DESC),
    INDEX idx_risk_level (risk_level),
    INDEX idx_outcome (outcome)
);

```

### 6.2 Dashboard API

```rust
#[get("/api/v1/diagnosis")]
async fn get_diagnosis(state: web::Data<AppState>) -> impl Responder {
    let diagnosis = state.latest_advisory.read().await;

    match diagnosis.as_ref() {
        Some(advisory) => HttpResponse::Ok().json(advisory),
        None => HttpResponse::NoContent().finish(),
    }
}

#[get("/api/v1/drilling")]
async fn get_drilling_metrics(state: web::Data<AppState>) -> impl Responder {
    let metrics = state.latest_metrics.read().await;
    HttpResponse::Ok().json(metrics)
}

#[post("/api/v1/advisory/acknowledge")]
async fn acknowledge_advisory(
    req: web::Json<AcknowledgeRequest>,
    state: web::Data<AppState>,
) -> impl Responder {
    let mut advisory = state.latest_advisory.write().await;

    if let Some(adv) = advisory.as_mut() {
        adv.acknowledged = true;
        adv.acknowledged_by = req.user.clone();
        adv.action_taken = req.action.clone();

        // Store in PostgreSQL
        state.dal.store_acknowledgment(adv).await?;
    }

    HttpResponse::Ok().finish()
}

```

### 6.3 Knowledge Deposit (Fleet Learning)

```rust
pub async fn deposit_confirmed_event(
    knowledge_store: &KnowledgeStore,
    advisory: &StrategicAdvisory,
    outcome: EventOutcome,
) -> Result<(), Error> {
    let episode = FleetEpisode {
        id: Uuid::new_v4(),
        timestamp: Utc::now(),
        rig_id: advisory.rig_id.clone(),
        depth: advisory.depth,
        formation: advisory.formation.clone(),
        campaign: advisory.campaign.clone(),

        event_type: advisory.category.to_string(),
        severity: advisory.risk_level.to_string(),

        physics_snapshot: serde_json::to_value(&advisory.physics)?,
        recommendation: advisory.recommendation.clone(),

        outcome,
        resolution_time: outcome.resolution_time,

        embedding: generate_embedding(&advisory).await?,
    };

    // Write to RAM recall
    knowledge_store.add_episode(episode.clone()).await?;

    // Write to ChromaDB (persistent, for fleet sync)
    knowledge_store.persist_to_chroma(episode).await?;

    Ok(())
}

```

---

## 7. Stage 5: Background Services

### 7.1 ML Engine (Hourly Optimization)

```rust
pub struct MLEngine {
    campaign: Campaign,
}

impl MLEngine {
    pub async fn run_hourly_analysis(
        &self,
        packets: &[WitsPacket],
    ) -> MLReport {
        // 1. Quality filtering
        let valid_packets: Vec<_> = packets.iter()
            .filter(|p| p.quality == DataQuality::Good)
            .filter(|p| p.rig_state == RigState::Drilling)
            .collect();

        // 2. Dysfunction filtering (NEW in V2.2)
        let stable_packets: Vec<_> = valid_packets.into_iter()
            .filter(|p| {
                let torque_cv = self.coefficient_variation(&p.torque_history);
                torque_cv < 0.12  // Reject stick-slip
            })
            .filter(|p| !self.is_pack_off(p))
            .filter(|p| !self.is_founder(p))
            .collect();

        // 3. Formation segmentation
        let segments = self.segment_by_formation(&stable_packets);

        // 4. For each formation segment
        let mut reports = Vec::new();
        for segment in segments {
            // Grid-based binning (8x6 WOB×RPM grid)
            let grid = self.create_parameter_grid(&segment);

            // Score each cell
            let optimal_cell = grid.iter()
                .max_by_key(|cell| {
                    let rop_score = cell.avg_rop * self.campaign.rop_weight();
                    let mse_score = (1.0 / cell.avg_mse) * self.campaign.mse_weight();
                    let stability_score = cell.stability * self.campaign.stability_weight();

                    (rop_score + mse_score + stability_score) as i64
                })
                .unwrap();

            reports.push(FormationOptimal {
                formation: segment.formation.clone(),
                optimal_wob: optimal_cell.wob_center,
                optimal_rpm: optimal_cell.rpm_center,
                optimal_flow: optimal_cell.avg_flow,
                expected_rop: optimal_cell.avg_rop,
                expected_mse: optimal_cell.avg_mse,
                safe_ranges: optimal_cell.safe_ranges.clone(),
            });
        }

        MLReport {
            timestamp: Utc::now(),
            formations: reports,
            samples_analyzed: stable_packets.len(),
        }
    }
}

```

### 7.2 Self-Healer (Continuous Monitoring)

```rust
pub struct SelfHealer {
    checks: Vec<Box<dyn HealthCheck>>,
}

#[async_trait]
pub trait HealthCheck: Send + Sync {
    async fn check(&self) -> HealthResult;
    async fn heal(&self) -> Result<(), Error>;
}

pub struct WitsConnectionCheck {
    last_packet_time: Arc<RwLock<Instant>>,
    wits_source: Arc<Mutex<Box<dyn DataSource>>>,
}

#[async_trait]
impl HealthCheck for WitsConnectionCheck {
    async fn check(&self) -> HealthResult {
        let last_packet = *self.last_packet_time.read().await;
        let elapsed = last_packet.elapsed();

        if elapsed > Duration::from_secs(30) {
            HealthResult::Unhealthy {
                component: "WITS Connection".to_string(),
                reason: format!("No data for {} seconds", elapsed.as_secs()),
            }
        } else {
            HealthResult::Healthy
        }
    }

    async fn heal(&self) -> Result<(), Error> {
        log::warn!("🔧 Attempting WITS reconnection...");

        let mut source = self.wits_source.lock().await;
        source.reconnect().await?;

        log::info!("✅ WITS connection restored");
        Ok(())
    }
}

```

---

## 8. Stage 6: Fleet Network

### 8.1 Event Upload (Rig → Hub)

```rust
pub struct FleetClient {
    hub_url: String,
    wireguard_tunnel: WireGuardConfig,
}

impl FleetClient {
    pub async fn upload_event(
        &self,
        advisory: &StrategicAdvisory,
        history: &[WitsPacket],
    ) -> Result<(), Error> {
        // Only upload AMBER or RED advisories
        if advisory.risk_level == RiskLevel::Low {
            return Ok(());
        }

        // Build event package
        let event = FleetEvent {
            rig_id: advisory.rig_id.clone(),
            timestamp: advisory.timestamp,
            advisory: advisory.clone(),
            history_window: history.to_vec(),  // 60 packets ≈ 1 minute
            outcome: EventOutcome::Pending,
        };

        // Compress
        let compressed = zstd::encode_all(
            serde_json::to_vec(&event)?.as_slice(),
            3  // Compression level
        )?;

        log::info!("📤 Uploading event {} ({} KB compressed)",
            event.id, compressed.len() / 1024);

        // Upload via VPN tunnel
        let response = reqwest::Client::new()
            .post(&format!("{}/api/fleet/events", self.hub_url))
            .header("Content-Type", "application/json")
            .header("Content-Encoding", "zstd")
            .body(compressed)
            .send()
            .await?;

        if response.status().is_success() {
            log::info!("✅ Event uploaded successfully");
            Ok(())
        } else {
            Err(Error::UploadFailed(response.status()))
        }
    }
}

```

### 8.2 Fleet Library Sync (Hub → Rigs)

```rust
pub async fn sync_fleet_library(
    hub_url: &str,
    local_recall: &mut RAMRecall,
) -> Result<usize, Error> {
    // Download updated library (every 6 hours)
    let response = reqwest::get(&format!("{}/api/fleet/library", hub_url))
        .await?;

    let library: FleetLibrary = response.json().await?;

    log::info!("📥 Syncing {} new episodes from fleet", library.episodes.len());

    let mut count = 0;
    for episode in library.episodes {
        // Only add if not already present
        if !local_recall.has_episode(&episode.id) {
            local_recall.add_episode(episode)?;
            count += 1;
        }
    }

    log::info!("✅ Synced {} new fleet episodes", count);
    Ok(count)
}

```

---

## 9. Data Flow Examples

### Example 1: Normal Drilling (No Advisory)

```
t=0ms:   WITS packet arrives (WOB: 38 klbs, ROP: 45 ft/hr, all normal)
t=2ms:   Physics engine calculates MSE: 42,000 psi (88% efficiency)
t=5ms:   Tactical Agent: All thresholds green
t=6ms:   Update history buffer
t=7ms:   Cache metrics to Redis
t=8ms:   Push to dashboard via WebSocket
t=10ms:  Ready for next packet

```

**Total latency: 10ms**

**Actions taken: Monitoring only, no alerts**

---

### Example 2: Pack-Off Detected

```
t=0ms:    WITS packet arrives
          - Torque: 12,500 → 17,800 ft-lbs (+42%)
          - SPP: 2,750 → 2,950 psi (+200 psi)
          - ROP: 45 → 28 ft/hr (-38%)

t=10ms:   Physics engine detects pack-off signature
t=12ms:   Tactical Agent creates AdvisoryTicket (AMBER)
t=15ms:   Escalate to Strategic Agent

t=100ms:  Strategic Agent verifies:
          - Trend analysis: torque+SPP rising together (✓)
          - ROP declining (✓)
          - Confidence: 0.88

t=102ms:  Query knowledge store (RAM recall, 1.5ms):
          - Found 3 similar cases (Rig V108, V72, V109)
          - All resolved by WOB reduction + flow increase

t=150ms:  LLM reasoning (Qwen 7B, 50ms):
          "Recommend: Reduce WOB 20-25%, increase flow 80-100 gpm"

t=170ms:  Orchestrator voting:
          - MSE Specialist: ELEVATED
          - Hydraulic: LOW
          - Well Control: LOW
          - Formation: LOW
          → Consensus: ELEVATED

t=180ms:  Advisory Composer generates StrategicAdvisory
t=185ms:  Store to PostgreSQL
t=190ms:  Push to dashboard (RED banner alert)
t=200ms:  Upload event to fleet hub (background)

Total latency: 200ms

```

**Driller sees:**

```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
⚠️  ADVISORY #42: ELEVATED | Confidence: 88%

Category: Mechanical | Type: Pack-Off

RECOMMENDATION:
1. Reduce WOB from 38 klbs to 30 klbs (-21%)
2. Increase flow from 520 gpm to 600 gpm (+15%)
3. Monitor for 10 minutes

EXPECTED BENEFIT:
- Torque should decrease to 13,000-14,000 ft-lbs
- ROP may drop 10-15% initially but stabilize
- Avoid trip (save 4-6 hours)

FLEET PRECEDENT:
- Rig V108: Similar case 18 days ago, resolved
- Rig V72: Similar case 45 days ago, resolved

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

```

---

## 10. Performance Characteristics

### Latency Budget

| Stage | Component | Target | Typical | Max |
| --- | --- | --- | --- | --- |
| 0 | Data ingestion | <5ms | 2ms | 10ms |
| 1 | Physics engine | <15ms | 10ms | 20ms |
| 1 | Tactical decision | <5ms | 3ms | 10ms |
| 2 | Strategic verification | <100ms | 80ms | 150ms |
| 3 | Knowledge query (RAM) | <5ms | 1.5ms | 3ms |
| 3 | LLM reasoning (GPU) | <100ms | 50ms | 200ms |
| 3 | Orchestrator voting | <20ms | 15ms | 30ms |
| 4 | PostgreSQL write | <50ms | 30ms | 100ms |
| **TOTAL (anomaly)** | **<300ms** | **190ms** | **500ms** |  |

### Memory Usage

```
Per-rig edge device (16 GB system):

Base system:                    2.0 GB
SAIREN-OS binary:               150 MB
Physics engine + agents:        100 MB
History buffer (60 packets):    5 MB
RAM Recall (10K episodes):      260 MB
Tactical LLM (Qwen 1.5B):       2.5 GB  (VRAM if GPU)
Strategic LLM (Qwen 7B):        8.0 GB  (VRAM if GPU)
────────────────────────────────────────
TOTAL (GPU mode):               13.0 GB (81% of 16 GB)
TOTAL (CPU mode):               2.5 GB (16% of 16 GB)

```

### Throughput

```
Packet processing rate:
- Normal drilling: 1 Hz (1 packet/second)
- Peak: 10 Hz (10 packets/second)
- Latency per packet: 10ms (normal), 200ms (with advisory)

Advisory generation rate:
- Tactical tickets: 5-10% of packets (0.05-0.1 Hz)
- Strategic advisories: 1-2% of packets (0.01-0.02 Hz)
- ~1-2 advisories per hour typical

```

---

## 11. Failure Modes & Recovery

### Mode 1: WITS Connection Lost

**Symptom:** No packets for 30+ seconds

**Detection:**

```rust
if last_packet.elapsed() > Duration::from_secs(30) {
    self_healer.heal_wits_connection().await?;
}

```

**Recovery:**

1. Log warning
2. Attempt reconnect with exponential backoff
3. If reconnect fails after 5 attempts (60s), alert dashboard
4. System continues running (offline mode)
5. When connection restored, resume immediately

**Impact:** Dashboard shows "OFFLINE" banner, drilling continues blind

---

### Mode 2: LLM Unavailable

**Symptom:** Model file missing or inference timeout

**Detection:**

```rust
match llm.infer(prompt).await {
    Ok(response) => use_response,
    Err(_) => fallback_to_templates,
}

```

**Recovery:**

1. Fall back to template-based advisories
2. Log degraded mode
3. Still generate advisories using physics + rules only
4. Advisory confidence reduced to 0.70

**Impact:** Advisories less nuanced but still functional

---

### Mode 3: RAM Recall Full

**Symptom:** Memory usage > 500 MB

**Detection:**

```rust
if recall.memory_usage_mb() > 500.0 {
    recall.evict_oldest_non_critical();
}

```

**Recovery:**

1. Evict oldest episodes (>30 days)
2. Prioritize keeping Resolved episodes
3. Remove Pending/FalsePositive episodes first
4. Maintain 10,000 most valuable episodes

**Impact:** None, transparent to operators

---

### Mode 4: Hub Unreachable

**Symptom:** Fleet event upload fails

**Detection:**

```rust
match fleet_client.upload_event(advisory).await {
    Ok(_) => (),
    Err(_) => queue_for_retry,
}

```

**Recovery:**

1. Queue event locally (disk-backed queue)
2. Retry every 15 minutes
3. When hub reconnects, upload queued events
4. System continues operating (rig remains autonomous)

**Impact:** Fleet learning delayed, local operation unaffected

---

## 12. Deployment Considerations

### 12.1 Hardware Requirements

**Minimum (CPU-only mode):**

- CPU: 4 cores, 2.5 GHz+
- RAM: 8 GB
- Disk: 50 GB SSD
- Network: 100 Mbps ethernet

**Recommended (GPU mode):**

- CPU: 8 cores, 3.0 GHz+
- RAM: 16 GB
- GPU: NVIDIA RTX A4000 (16 GB VRAM) or better
- Disk: 100 GB NVMe SSD
- Network: 1 Gbps ethernet

### 12.2 Network Configuration

**Rig-side:**

- WITS TCP: Port 5000 (or 9100, configurable)
- Dashboard HTTP: Port 8080
- WireGuard VPN: Port 51820 (UDP)

**Firewall rules:**

```bash
# Allow WITS inbound
iptables -A INPUT -p tcp --dport 5000 -j ACCEPT

# Allow dashboard (rig network only)
iptables -A INPUT -p tcp --dport 8080 -s 10.0.0.0/8 -j ACCEPT

# Allow WireGuard (to hub)
iptables -A OUTPUT -p udp --dport 51820 -j ACCEPT

```

### 12.3 Installation Steps

```bash
# 1. Build binary
cargo build --release --features cuda

# 2. Run installer
sudo ./deploy/install.sh

# 3. Configure well parameters
sudo vi /etc/sairen-os/well_config.toml

# 4. Configure VPN
sudo vi /etc/wireguard/wg0.conf

# 5. Enable and start
sudo systemctl enable sairen-os
sudo systemctl start sairen-os

# 6. Verify
curl <http://localhost:8080/api/v1/health>

```

### 12.4 Monitoring

**Key metrics to watch:**

- Packet ingestion rate (should be ~1 Hz)
- Advisory generation rate (1-2/hour typical)
- LLM latency (GPU: <100ms, CPU: <5s)
- Memory usage (should stay under 13 GB)
- False positive rate (target: <5%)

**Logging:**

```bash
# Follow logs
sudo journalctl -u sairen-os -f

# Check for errors
sudo journalctl -u sairen-os --priority=err --since today

# Performance stats
curl <http://localhost:8080/api/v1/status> | jq .

```

---

## Conclusion

This document provides the complete technical specification for how SAIREN-OS functions at each stage. Key takeaways:

1. **Tiered processing**: Fast tactical (10ms) → Deep strategic (200ms) → Fleet learning (hours)
2. **Physics-first**: All AI reasoning grounded in deterministic calculations
3. **Autonomous operation**: Each rig functions independently, fleet enhances but doesn't depend
4. **Graceful degradation**: System continues operating even if components fail
5. **Explainable**: Every advisory shows physics + precedent + reasoning

The system is **production-ready** with these characteristics optimized for real-world offshore deployment where reliability, latency, and explainability are critical.

Sources
[1] [README.Claude.md](http://readme.claude.md/) [https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/attachments/98842357/329590ec-e626-4fec-939b-4e781ec0d56e/README.Claude.md](https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/attachments/98842357/329590ec-e626-4fec-939b-4e781ec0d56e/README.Claude.md)
[2] SAIREN-OS_The-Complete-Vision.md [https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/attachments/98842357/a6ffeaab-4400-45f0-a254-acc3761b0670/SAIREN-OS_The-Complete-Vision.md](https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/attachments/98842357/a6ffeaab-4400-45f0-a254-acc3761b0670/SAIREN-OS_The-Complete-Vision.md)
[3] Sairen-sentrix-merged-architecture.pdf [https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/attachments/98842357/d30b6abe-3b79-4f65-95c3-1cd8af6b5aee/Sairen-sentrix-merged-architecture.pdf](https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/attachments/98842357/d30b6abe-3b79-4f65-95c3-1cd8af6b5aee/Sairen-sentrix-merged-architecture.pdf)
[4] [SAIREN-Enhancement-Roadmap.md](http://sairen-enhancement-roadmap.md/) [https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/attachments/98842357/2a87771b-bbf7-4ecc-9fb9-823de7d97da4/SAIREN-Enhancement-Roadmap.md](https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/attachments/98842357/2a87771b-bbf7-4ecc-9fb9-823de7d97da4/SAIREN-Enhancement-Roadmap.md)
[5] explain-the-math-behind-linear-hZg_dJWkTjqdXIIQrQavEA.md [https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/collection_68fc49c9-85ac-436b-a83c-ab0a3e1632a6/fb395743-adda-4383-a978-5693c74a69bd/explain-the-math-behind-linear-hZg_dJWkTjqdXIIQrQavEA.md](https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/collection_68fc49c9-85ac-436b-a83c-ab0a3e1632a6/fb395743-adda-4383-a978-5693c74a69bd/explain-the-math-behind-linear-hZg_dJWkTjqdXIIQrQavEA.md)
[6] [ive-got-claude-to-copy-the-who-WSLPSI.hT2KDxkWFRtWA9Q.md](http://ive-got-claude-to-copy-the-who-wslpsi.ht2kdxkwfrtwa9q.md/) [https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/collection_68fc49c9-85ac-436b-a83c-ab0a3e1632a6/4f171f61-8e8f-4334-bc34-e433651bcf58/ive-got-claude-to-copy-the-who-WSLPSI.hT2KDxkWFRtWA9Q.md](https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/collection_68fc49c9-85ac-436b-a83c-ab0a3e1632a6/4f171f61-8e8f-4334-bc34-e433651bcf58/ive-got-claude-to-copy-the-who-WSLPSI.hT2KDxkWFRtWA9Q.md)
[7] [eni-has-a-goal-of-reducing-hum-vV1TjWS7S3OfsmFdbZd68A.md](http://eni-has-a-goal-of-reducing-hum-vv1tjws7s3ofsmfdbzd68a.md/) [https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/collection_68fc49c9-85ac-436b-a83c-ab0a3e1632a6/87acf7d2-b9a1-490d-93dc-094c9b6abd9e/eni-has-a-goal-of-reducing-hum-vV1TjWS7S3OfsmFdbZd68A.md](https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/collection_68fc49c9-85ac-436b-a83c-ab0a3e1632a6/87acf7d2-b9a1-490d-93dc-094c9b6abd9e/eni-has-a-goal-of-reducing-hum-vV1TjWS7S3OfsmFdbZd68A.md)
[8] [what-is-the-formula-called-tha-1cUTiX2IQoOYd3fzkiwL2w.md](http://what-is-the-formula-called-tha-1cutix2iqooyd3fzkiwl2w.md/) [https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/collection_68fc49c9-85ac-436b-a83c-ab0a3e1632a6/78b895ef-8e93-4be6-91f5-150a00ddb7c8/what-is-the-formula-called-tha-1cUTiX2IQoOYd3fzkiwL2w.md](https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/collection_68fc49c9-85ac-436b-a83c-ab0a3e1632a6/78b895ef-8e93-4be6-91f5-150a00ddb7c8/what-is-the-formula-called-tha-1cUTiX2IQoOYd3fzkiwL2w.md)
[9] [how-can-i-use-collected-data-f-uqt2rdSrTX6G6vKQYEfxNQ.md](http://how-can-i-use-collected-data-f-uqt2rdsrtx6g6vkqyefxnq.md/) [https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/collection_68fc49c9-85ac-436b-a83c-ab0a3e1632a6/13dca616-938d-4548-ae80-e1a36e524e3c/how-can-i-use-collected-data-f-uqt2rdSrTX6G6vKQYEfxNQ.md](https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/collection_68fc49c9-85ac-436b-a83c-ab0a3e1632a6/13dca616-938d-4548-ae80-e1a36e524e3c/how-can-i-use-collected-data-f-uqt2rdSrTX6G6vKQYEfxNQ.md)
[10] TEST_ANALYSIS_12hr_simulation_2026-01-22.md [https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/collection_68fc49c9-85ac-436b-a83c-ab0a3e1632a6/a5fe2eff-e348-4738-9eb2-d164ca50eb36/TEST_ANALYSIS_12hr_simulation_2026-01-22.md](https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/collection_68fc49c9-85ac-436b-a83c-ab0a3e1632a6/a5fe2eff-e348-4738-9eb2-d164ca50eb36/TEST_ANALYSIS_12hr_simulation_2026-01-22.md)
[11] [README.md](http://readme.md/) [https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/collection_68fc49c9-85ac-436b-a83c-ab0a3e1632a6/1f88804f-3d63-4e27-a2a8-c488282b24a2/README.md](https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/collection_68fc49c9-85ac-436b-a83c-ab0a3e1632a6/1f88804f-3d63-4e27-a2a8-c488282b24a2/README.md)
[12] [can-you-understand-this-drawin-jdfZGp9SRsePprxScxaWJg.md](http://can-you-understand-this-drawin-jdfzgp9srsepprxscxawjg.md/) [https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/collection_68fc49c9-85ac-436b-a83c-ab0a3e1632a6/d0e0c2c1-f4e0-49e2-89b0-b6f1eaee148a/can-you-understand-this-drawin-jdfZGp9SRsePprxScxaWJg.md](https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/collection_68fc49c9-85ac-436b-a83c-ab0a3e1632a6/d0e0c2c1-f4e0-49e2-89b0-b6f1eaee148a/can-you-understand-this-drawin-jdfZGp9SRsePprxScxaWJg.md)
[13] [will-i-need-patents-and-licens-LEYYMXAKQJi5vHKfZzac0w.md](http://will-i-need-patents-and-licens-leyymxakqji5vhkfzzac0w.md/) [https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/collection_68fc49c9-85ac-436b-a83c-ab0a3e1632a6/44f9c7d4-e6e4-4867-bbbc-8071b817439f/will-i-need-patents-and-licens-LEYYMXAKQJi5vHKfZzac0w.md](https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/collection_68fc49c9-85ac-436b-a83c-ab0a3e1632a6/44f9c7d4-e6e4-4867-bbbc-8071b817439f/will-i-need-patents-and-licens-LEYYMXAKQJi5vHKfZzac0w.md)
[14] SAIREN-LTD_BUSINESS_PLAN.md [https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/collection_68fc49c9-85ac-436b-a83c-ab0a3e1632a6/1f703dec-42cd-40a3-ae0d-e49d4d1fd4ba/SAIREN-LTD_BUSINESS_PLAN.md](https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/collection_68fc49c9-85ac-436b-a83c-ab0a3e1632a6/1f703dec-42cd-40a3-ae0d-e49d4d1fd4ba/SAIREN-LTD_BUSINESS_PLAN.md)
[15] in-memory-storage-strategies-f-3x.73kGkQgKGgXRA_QgnxA.md [https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/collection_68fc49c9-85ac-436b-a83c-ab0a3e1632a6/f93e7316-df34-421b-87f8-7168660e802c/in-memory-storage-strategies-f-3x.73kGkQgKGgXRA_QgnxA.md](https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/collection_68fc49c9-85ac-436b-a83c-ab0a3e1632a6/f93e7316-df34-421b-87f8-7168660e802c/in-memory-storage-strategies-f-3x.73kGkQgKGgXRA_QgnxA.md)
[16] [rigpay-dashboard-W6vh0Pg1RUiqtoQEk9THhA.md](http://rigpay-dashboard-w6vh0pg1ruiqtoqek9thha.md/) [https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/collection_68fc49c9-85ac-436b-a83c-ab0a3e1632a6/3ea0ee53-566b-4fb1-a47b-9fce522c7919/rigpay-dashboard-W6vh0Pg1RUiqtoQEk9THhA.md](https://ppl-ai-file-upload.s3.amazonaws.com/web/direct-files/collection_68fc49c9-85ac-436b-a83c-ab0a3e1632a6/3ea0ee53-566b-4fb1-a47b-9fce522c7919/rigpay-dashboard-W6vh0Pg1RUiqtoQEk9THhA.md)