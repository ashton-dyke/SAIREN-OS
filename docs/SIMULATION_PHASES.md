# Simulation Phase Progression

This document explains how the simulation evolves sensor variables over time to simulate bearing degradation in the TDS-11SA top drive system.

## Phase Timeline (1-hour simulation)

| Phase | Time | % Progress |
|-------|------|------------|
| Healthy Baseline | 0-15 min | 0-25% |
| Normal Drilling | 15-30 min | 25-50% |
| Progressive Failure | 30-50 min | 50-83% |
| Critical Failure | 50-60 min | 83-100% |

---

## Variable Changes by Phase

### Phase 1: Healthy Baseline (0-15 min)

| Variable | Value | Notes |
|----------|-------|-------|
| Vibration | **0.05g** + noise (σ=0.02) | Well below 0.12g threshold |
| Motor Temp | 65°C ± 0.5° | Stable |
| Gearbox Temp | 60°C ± 0.5° | Stable |
| RPM | 110 ± 5 | Steady |
| BPFO Signal | **None** | No fault |

---

### Phase 2: Normal Drilling (15-30 min)

| Variable | Value | Notes |
|----------|-------|-------|
| Vibration | **0.06g** + noise (σ=0.03) | Still below threshold |
| Motor Temp | 65°C ± 1° | Stable |
| Gearbox Temp | 60°C ± 1° | Stable |
| RPM | 110 ± 10 | Normal variation |
| BPFO Signal | **None** | No fault |

---

### Phase 3: Progressive Failure (30-50 min)

**Fault progress calculation:**
```
fault_progress = (current_progress - 0.50) / 0.333
```
Ramps from 0.0 → 1.0 over this phase.

| Variable | Start | End | Formula |
|----------|-------|-----|---------|
| Vibration Base | 0.15g | 0.25g | `0.15 + 0.1 × fault_progress` |
| BPFO Amplitude | 0g | **1.5g** | `1.5 × fault_progress` |
| Motor Temp | 65°C | **80°C** | `65 + 15 × fault_progress` |
| Gearbox Temp | 60°C | **72°C** | `60 + 12 × fault_progress` |

**BPFO Signal:** Sine wave at bearing fault frequency (~15 Hz at 110 RPM)
```
bpfo_signal = fault_amplitude × sin(2π × bpfo_freq × t)
```

**BPFO Frequency Calculation (SKF 29434 bearing):**
- N (rolling elements) = 18
- d (ball diameter) = 38mm
- D (pitch diameter) = 280mm
- θ (contact angle) = 15°
- Formula: `BPFO = (N/2) × shaft_freq × (1 - d/D × cos(θ))`

---

### Phase 4: Critical Failure (50-60 min)

| Variable | Value | Notes |
|----------|-------|-------|
| Vibration Base | **0.3g** | High baseline |
| BPFO Amplitude | **1.4-1.6g** (spikes to 2.2g) | Sustained critical |
| Motor Temp | **80°C** ± 2° | Near thermal limit |
| Gearbox Temp | **75°C** ± 2° | Elevated |
| Harmonics | 2x BPFO (40%), 3x BPFO (20%) | Added damage signature |

**Spike behavior:** 10% chance of amplitude spike to 1.8-2.2g (simulates intermittent severe impacts)

---

## Visual Summary

```
Vibration (g)
    2.0 |                              ████ Critical
    1.5 |                         ████
    1.0 |                    ████
    0.5 |               ████
    0.1 |████████████████
        +----+----+----+----+----+----+
        0   10   20   30   40   50   60 min
             Healthy    |  Progressive | Critical
                        ↑ Fault starts (30 min)
```

```
Temperature (°C)
   80 |                              ████ Motor
   75 |                         ████      Gearbox
   70 |                    ████
   65 |████████████████████
   60 |████████████████████
        +----+----+----+----+----+----+
        0   10   20   30   40   50   60 min
```

---

## Detection Thresholds

The tactical agent uses these thresholds to detect anomalies:

| Metric | Warning | Critical |
|--------|---------|----------|
| Vibration Amplitude | 0.12g | 0.5g |
| Kurtosis | 3.0 | 6.0 |
| BPFO Amplitude | 0.15g | 0.5g |

With the corrected simulation:
- **Phases 1-2:** All values below warning thresholds → No tickets
- **Phase 3:** Values progressively exceed thresholds → Tickets created
- **Phase 4:** All values well above critical → Continuous alerts

---

## Running the Simulation

```bash
# 1-hour simulation at 100x speed (~36 seconds real time)
./target/release/simulation --hours 1 --speed 100 | \
  ./target/release/tds-guardian --stdin --multiagent

# 12-hour full scenario at 10x speed (~72 minutes real time)
./target/release/simulation --hours 12 --speed 10 | \
  ./target/release/tds-guardian --stdin --multiagent
```
