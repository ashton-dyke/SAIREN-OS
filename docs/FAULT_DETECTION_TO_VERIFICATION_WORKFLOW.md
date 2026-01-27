# Fault Detection to Verification Workflow

This document explains the exact workflow from when a fault is detected by the Tactical Agent to when it is verified (or rejected) by the Strategic Agent in the SAIREN multi-agent system.

---

## Overview

The system uses a **two-stage verification architecture** to reduce false positives. The Tactical Agent performs fast anomaly detection, but instead of directly alerting the dashboard, it creates a **VerificationTicket** that must be validated by the Strategic Agent using physics-based analysis.

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│  Tactical Agent │ --> │ Strategic Agent │ --> │    Dashboard    │
│  (Detection)    │     │ (Verification)  │     │    (Alert)      │
└─────────────────┘     └─────────────────┘     └─────────────────┘
        │                       │
        │                       ├── CONFIRMED → Proceed to dashboard
        │                       ├── REJECTED  → Discard (false positive)
        │                       └── UNCERTAIN → Monitor, no alert
        │
        └── Creates VerificationTicket if anomaly detected
```

---

## Stage 1: Fault Detection (Tactical Agent)

**Source File:** `src/agents/tactical.rs`

### 1.1 Trigger Conditions

The Tactical Agent creates a VerificationTicket when **ALL** of the following are true:

1. **Operational State is Drilling** - Tickets are only created during active drilling operations
2. **At least one anomaly threshold is exceeded:**
   - Kurtosis > 4.0 (indicates impulsive behavior in vibration signal)
   - BPFO amplitude > 0.3g (Ball Pass Frequency Outer race spike)
   - Max vibration amplitude > 0.3g (alternative trigger for limited sensor data)

**Code Reference:** `src/agents/tactical.rs:320-332`
```rust
// RULE 1: Only create tickets during Drilling
if metrics.state != OperationalState::Drilling {
    return None;
}

// RULE 2: Must exceed thresholds to create ticket
if !metrics.is_anomaly {
    return None;
}
```

### 1.2 Anomaly Detection Logic

The `is_anomaly` flag is set in `calculate_basic_metrics()` at line 244-247:

```rust
let kurtosis_anomaly = kurtosis > thresholds::KURTOSIS_WARNING;      // > 4.0
let bpfo_anomaly = bpfo_amplitude > thresholds::BPFO_WARNING;        // > 0.3g
let vib_amplitude_anomaly = max_vib > thresholds::VIB_AMPLITUDE_WARNING; // > 0.3g
let is_anomaly = kurtosis_anomaly || bpfo_anomaly || vib_amplitude_anomaly;
```

### 1.3 VerificationTicket Contents

When a fault is detected, the Tactical Agent creates a `VerificationTicket` containing:

| Field | Description | Source |
|-------|-------------|--------|
| `timestamp` | Unix timestamp of detection | Sensor packet |
| `suspected_fault` | Fault type string (e.g., "BPFO bearing defect") | `determine_fault_and_trigger()` |
| `trigger_value` | The value that exceeded threshold | Metric value |
| `confidence` | Initial confidence (0.5-1.0) | Based on threshold excess |
| `fft_snapshot` | Frequency domain data | `build_fft_snapshot()` |
| `operational_state` | Current rig state (Drilling) | Physics engine |
| `initial_severity` | Low/Medium/High/Critical | `determine_severity()` |
| `metrics` | Full TacticalMetrics struct | Phase 2 calculations |
| `sensor_name` | "vibration_array" | Hardcoded |

**Code Reference:** `src/agents/tactical.rs:347-358`

### 1.4 Fault Classification

The suspected fault type is determined by priority order:

| Priority | Condition | Fault Type |
|----------|-----------|------------|
| 1 | BPFO > 0.5g (critical) | "BPFO bearing defect - Critical" |
| 2 | BPFO > 0.3g (warning) | "BPFO bearing defect" |
| 3 | Kurtosis > 8.0 (critical) | "Impact/impulse damage" |
| 4 | Kurtosis > 4.0 (warning) | "Elevated kurtosis - potential bearing wear" |
| 5 | Default | "Vibration anomaly" |

**Code Reference:** `src/agents/tactical.rs:361-376`

### 1.5 Initial Severity Assignment

| Condition | Severity |
|-----------|----------|
| Kurtosis > 8.0 OR BPFO > 0.5g | Critical |
| Kurtosis > 6.0 OR BPFO > 0.45g | High |
| Kurtosis > 4.0 OR BPFO > 0.3g | Medium |
| Default | Low |

**Code Reference:** `src/agents/tactical.rs:412-435`

---

## Stage 2: Fault Verification (Strategic Agent)

**Source File:** `src/agents/strategic.rs`

### 2.1 Enhanced Physics Analysis

Before applying verification logic, the Strategic Agent runs enhanced physics calculations on the history buffer (up to 60 packets / 1 hour of data):

**Calculated Metrics:**

| Metric | Description | Purpose |
|--------|-------------|---------|
| `cumulative_damage` | Miner's rule damage accumulation | Tracks total bearing wear |
| `l10_life_hours` | ISO 281 bearing life estimate | Predicts remaining life |
| `wear_acceleration` | 2nd derivative of damage | Detects accelerating degradation |
| `trend_consistency` | R² of 72-hour regression | Validates trend reliability |
| `confidence_factor` | Based on history depth | Ensures sufficient data |
| `is_accelerating` | wear_acceleration > 0 | Boolean flag |

**Code Reference:** `src/physics_engine/mod.rs` (enhanced_strategic_analysis function)

### 2.2 Verification Decision Logic

The Strategic Agent applies a series of rules to determine verification status:

**Code Reference:** `src/agents/strategic.rs:277-378`

#### Rule 1: Insufficient Data Check
```
IF confidence_factor < 0.6
THEN status = UNCERTAIN
     reason = "Insufficient data confidence"
     send_to_dashboard = false
```

#### Rule 2: Clear Rejection (No Wear)
```
IF cumulative_damage < 0.5
   AND l10_life > 500 hours
   AND NOT is_accelerating
THEN status = REJECTED
     reason = "No significant wear trend, likely transient event"
     send_to_dashboard = false
```

#### Rule 3: Confirmed (Accelerating Wear)
```
IF is_accelerating
   AND trend_consistency > 0.7
THEN status = CONFIRMED
     reason = "Accelerating wear pattern detected"
     send_to_dashboard = true
```

#### Rule 4: Confirmed (Critical L10 Life)
```
IF l10_life < 24 hours
THEN status = CONFIRMED
     severity = CRITICAL
     reason = "Critical L10 life remaining"
     send_to_dashboard = true
```

#### Rule 5: Confirmed (High Damage with Trend)
```
IF cumulative_damage > 0.5
   AND trend_consistency > 0.5
THEN status = CONFIRMED
     reason = "Significant cumulative damage with moderate trend"
     send_to_dashboard = true
```

#### Rule 6: Default (Inconclusive)
```
ELSE status = UNCERTAIN
     reason = "Verification inconclusive, recommend monitoring"
     send_to_dashboard = false
```

### 2.3 Verification Thresholds

Defined in `src/types.rs:520-531`:

| Threshold | Value | Purpose |
|-----------|-------|---------|
| `MIN_WEAR_ACCELERATION` | 0.001 | Minimum acceleration to confirm |
| `MIN_TREND_CONSISTENCY` | 0.7 | Minimum R² for confirmation |
| `L10_CRITICAL_HOURS` | 24.0 | Immediate confirmation threshold |
| `MIN_CONFIDENCE_FACTOR` | 0.6 | Minimum data quality |
| `DAMAGE_HIGH_THRESHOLD` | 0.5 | Cumulative damage concern level |

### 2.4 Final Severity Calculation (Confirmed Tickets)

For confirmed tickets, final severity is based on L10 life:

| L10 Life | Final Severity |
|----------|----------------|
| < 24 hours | Critical |
| < 168 hours (1 week) | High |
| < 720 hours (1 month) | Medium |
| >= 720 hours | Uses initial ticket severity |

**Code Reference:** `src/agents/strategic.rs:381-399`

### 2.5 VerificationResult Output

The Strategic Agent returns a `VerificationResult` containing:

| Field | Description |
|-------|-------------|
| `ticket` | Original VerificationTicket |
| `status` | Pending/Confirmed/Rejected/Uncertain |
| `physics_report` | PhysicsReport with L10, damage, acceleration |
| `reasoning` | Human-readable explanation |
| `final_severity` | Healthy/Low/Medium/High/Critical |
| `send_to_dashboard` | Boolean - whether to alert |

**Code Reference:** `src/types.rs:472-486`

---

## Pipeline Integration

**Source File:** `src/pipeline/coordinator.rs`

### Processing Flow

```
process_packet() called with SensorPacket
          │
          ▼
┌─────────────────────────────────────┐
│ PHASE 2-3: Tactical Agent Process   │
│ tactical_agent.process(packet)      │
│ Returns: (VerificationTicket?,      │
│           TacticalMetrics,          │
│           HistoryEntry)             │
└─────────────────────────────────────┘
          │
          ▼
┌─────────────────────────────────────┐
│ PHASE 4: History Buffer Update      │
│ Always runs - stores HistoryEntry   │
│ in 60-packet circular buffer        │
└─────────────────────────────────────┘
          │
          ▼
    VerificationTicket exists?
          │
    ┌─────┴─────┐
    │           │
   NO          YES
    │           │
    ▼           ▼
 [END]   ┌─────────────────────────────────────┐
         │ PHASE 5: Strategic Verification     │
         │ strategic_agent.verify_ticket(      │
         │     ticket, history_packets)        │
         └─────────────────────────────────────┘
                      │
                      ▼
              VerificationStatus?
                      │
         ┌────────────┼────────────┐
         │            │            │
      REJECTED    UNCERTAIN    CONFIRMED
         │            │            │
         ▼            ▼            ▼
      [END]        [END]    ┌─────────────────┐
                            │ PHASES 6-9      │
                            │ Context Lookup  │
                            │ LLM Diagnosis   │
                            │ Orchestrator    │
                            │ Storage         │
                            └─────────────────┘
                                   │
                                   ▼
                            ┌─────────────────┐
                            │ PHASE 10        │
                            │ Dashboard Alert │
                            └─────────────────┘
```

**Code Reference:** `src/pipeline/coordinator.rs:117-259`

### Key Decision Points

**Decision Point 1 (line 138-158):** If tactical agent returns no ticket, processing stops.

**Decision Point 2 (line 180-212):** Based on verification status:
- `Rejected` → Log and return None (no alert)
- `Uncertain` → Log and return None (no alert)
- `Confirmed` → Continue to phases 6-9
- `Pending` → Warning log, return None (shouldn't happen)

---

## Example Scenarios

### Scenario 1: Transient Vibration Spike (Rejected)

1. Drilling rig hits hard formation, causing momentary vibration spike
2. Tactical Agent detects kurtosis = 5.2 (> 4.0 threshold)
3. Creates VerificationTicket with suspected_fault = "Elevated kurtosis"
4. Strategic Agent analyzes history:
   - cumulative_damage = 0.02 (low)
   - l10_life = 2000 hours (healthy)
   - is_accelerating = false
5. **Result: REJECTED** - "No significant wear trend, likely transient event"
6. No dashboard alert sent

### Scenario 2: Bearing Degradation (Confirmed)

1. Outer race defect developing over several hours
2. Tactical Agent detects BPFO = 0.4g (> 0.3g threshold)
3. Creates VerificationTicket with suspected_fault = "BPFO bearing defect"
4. Strategic Agent analyzes history:
   - cumulative_damage = 0.65 (elevated)
   - l10_life = 120 hours (concerning)
   - trend_consistency = 0.85 (strong trend)
   - is_accelerating = true
5. **Result: CONFIRMED** - "Accelerating wear pattern detected"
6. Final severity: HIGH (l10 < 168 hours)
7. Proceeds to LLM diagnosis and dashboard alert

### Scenario 3: Insufficient History (Uncertain)

1. System just started, only 5 minutes of data
2. Tactical Agent detects vibration anomaly
3. Creates VerificationTicket
4. Strategic Agent analyzes:
   - confidence_factor = 0.4 (< 0.6 minimum)
5. **Result: UNCERTAIN** - "Insufficient data confidence"
6. No dashboard alert, system continues monitoring

---

## Thresholds Summary

### Tactical Agent (Detection)

| Metric | Warning | Critical |
|--------|---------|----------|
| Kurtosis | 4.0 | 8.0 |
| BPFO Amplitude | 0.3g | 0.5g |
| Max Vibration | 0.3g | 0.5g |
| Temperature Delta | 10°C | 20°C |

### Strategic Agent (Verification)

| Metric | Threshold | Effect |
|--------|-----------|--------|
| L10 Life | < 24h | Immediate CRITICAL confirmation |
| L10 Life | < 500h | Cannot reject ticket |
| Cumulative Damage | > 0.5 | Requires only 0.5 trend consistency |
| Trend Consistency | > 0.7 | Required for accelerating wear confirmation |
| Confidence Factor | < 0.6 | Returns UNCERTAIN |

---

## Data Structures Reference

### VerificationTicket (src/types.rs:425-445)

```rust
pub struct VerificationTicket {
    pub timestamp: u64,
    pub suspected_fault: String,
    pub trigger_value: f64,
    pub confidence: f64,
    pub fft_snapshot: FftSnapshot,
    pub operational_state: OperationalState,
    pub initial_severity: TicketSeverity,
    pub metrics: TacticalMetrics,
    pub sensor_name: String,
}
```

### VerificationResult (src/types.rs:472-486)

```rust
pub struct VerificationResult {
    pub ticket: VerificationTicket,
    pub status: VerificationStatus,
    pub physics_report: PhysicsReport,
    pub reasoning: String,
    pub final_severity: FinalSeverity,
    pub send_to_dashboard: bool,
}
```

### VerificationStatus (src/types.rs:448-458)

```rust
pub enum VerificationStatus {
    Pending,    // Awaiting analysis
    Confirmed,  // Physics confirms fault
    Rejected,   // Physics rejects fault (false positive)
    Uncertain,  // Insufficient data or conflicting signals
}
```

---

## Performance Targets

| Phase | Target | Description |
|-------|--------|-------------|
| Phase 2 (Basic Physics) | < 15ms | Tactical calculations |
| Phase 5 (Advanced Physics) | < 50ms | Strategic verification |
| Full Cycle (Phases 1-9) | < 100ms | Complete processing |

**Code References:**
- Phase 2 timing: `src/agents/tactical.rs:137-143`
- Phase 5 timing: `src/pipeline/coordinator.rs:321-327`
- Full cycle timing: `src/pipeline/coordinator.rs:251-256`
