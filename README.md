# SAIREN-OS - Drilling Operational Intelligence System

Real-time drilling advisory system using WITS Level 0 data and a multi-agent AI pipeline for drilling optimization and risk prevention.

---

## Table of Contents

1. [Quick Start](#quick-start)
2. [Overview](#overview)
3. [Features](#features)
4. [Architecture](#architecture)
5. [Running the System](#running-the-system)
6. [Configuration](#configuration)
7. [Deployment](#deployment)
8. [WITS Simulator](#wits-simulator)
9. [API Reference](#api-reference)
10. [Understanding Advisories](#understanding-advisories)
11. [Thresholds Reference](#thresholds-reference)
12. [Troubleshooting](#troubleshooting)
13. [Project Structure](#project-structure)
14. [Glossary](#glossary)
15. [Changelog](#changelog)

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

---

## Architecture

SAIREN-OS uses a two-stage multi-agent architecture where a fast **Tactical Agent** handles real-time anomaly detection, and a deeper **Strategic Agent** performs comprehensive drilling physics analysis only when anomalies are detected.

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
                                                                |   |   Orchestrator   |
                                                                |   |  4 Specialists   |
                                                                |   +------------------+
                                                                |            |
                                                                |            v
                                                                |   +------------------+
                                                                +-->|  Dashboard API   |
                                                                    |  :8080           |
                                                                    +------------------+
    ===============================================================================
```

### 10-Phase Processing Pipeline

| Phase | Component | Function |
|-------|-----------|----------|
| 1 | WITS Ingestion | Receive 40+ channel WITS Level 0 packets, classify rig state |
| 2 | Tactical Physics | Calculate MSE, d-exponent, flow balance, pit rate (<15ms) |
| 3 | Decision Gate | Create AdvisoryTicket if thresholds exceeded |
| 4 | History Buffer | Store last 60 packets for trend analysis |
| 5 | Strategic Verification | Physics-based validation of tickets |
| 6 | Context Lookup | Query drilling knowledge base |
| 7 | LLM Advisory | Generate recommendations (Qwen 2.5 7B) |
| 8 | Orchestrator Voting | 4 specialists vote on risk level |
| 9 | Advisory Generation | Combine analysis into StrategicAdvisory |
| 10 | Dashboard API | REST endpoints and web dashboard |

### Specialist Weights

| Specialist | Weight | Evaluates |
|------------|--------|-----------|
| **MSE** | 25% | Drilling efficiency, ROP optimization |
| **Hydraulic** | 25% | SPP, ECD margin, flow rates |
| **WellControl** | 30% | Kick/loss indicators, gas, pit volume |
| **Formation** | 20% | D-exponent trends, formation changes |

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

### Building Options

```bash
# With LLM support (CPU inference - works on any machine)
cargo build --release --features llm

# With LLM support + GPU acceleration (requires CUDA toolkit)
cargo build --release --features cuda

# Without LLM (template-based advisories only)
cargo build --release
```

**Hardware auto-detection**: When built with `llm` or `cuda`, SAIREN-OS checks for CUDA at startup and automatically selects the right models:

| Hardware | Tactical Model | Strategic Model | Build Flag |
|----------|---------------|----------------|------------|
| **GPU** (CUDA) | Qwen 2.5 1.5B (~60ms) | Qwen 2.5 7B (~800ms) | `--features cuda` |
| **CPU** | Qwen 2.5 1.5B (~2-5s) | Qwen 2.5 4B (~10-30s) | `--features llm` |
| **No LLM** | Template-based | Template-based | *(default)* |

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
| `[thresholds.founder]` | Founder point detection sensitivity | `wob_increase_pct = 5.0` |
| `[baseline_learning]` | Sigma thresholds, min samples | `min_samples = 100` |
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
| `TACTICAL_MODEL_PATH` | `models/qwen2.5-1.5b-instruct-q4_k_m.gguf` | Tactical LLM model (same for GPU/CPU) |
| `STRATEGIC_MODEL_PATH` | GPU: `models/qwen2.5-7b-instruct-q4_k_m.gguf`, CPU: `models/qwen2.5-4b-instruct-q4_k_m.gguf` | Strategic LLM model (auto-selected) |
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

Download models and place in `models/` directory, or set environment variables:
```bash
export TACTICAL_MODEL_PATH=/path/to/tactical-model.gguf
export STRATEGIC_MODEL_PATH=/path/to/strategic-model.gguf
```

### LLM inference too slow

1. Check what mode SAIREN-OS detected at startup (look for "Hardware:" in logs)
2. If on CPU, this is expected — CPU inference targets ~2-5s (tactical) and ~10-30s (strategic)
3. For faster inference, build with `--features cuda` and ensure CUDA is available: `nvidia-smi`
4. Use quantized models (Q4_K_M recommended)
5. System works without LLM - falls back to templates

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
  types.rs             # Core data structures (WitsPacket, DrillingMetrics,
                       # Campaign, Operation, AdvisoryTicket, etc.)

  config/
    mod.rs             # OnceLock global config access (init/get/is_initialized)
    well_config.rs     # WellConfig TOML loader, all threshold structs, validation

  agents/
    tactical.rs        # Fast anomaly detection + operation classification
    strategic.rs       # Physics verification with configurable thresholds
    orchestrator.rs    # 4-specialist weighted voting

  pipeline/
    coordinator.rs     # 10-phase pipeline coordinator
    processor.rs       # AppState, system status

  baseline/
    mod.rs             # Adaptive threshold learning with crash-safe persistence

  ml_engine/
    analyzer.rs        # Core ML analysis
    correlations.rs    # Pearson correlation with p-value testing
    optimal_finder.rs  # Campaign-aware composite scoring
    scheduler.rs       # Configurable interval scheduler

  physics_engine/
    mod.rs             # Anomaly detection with configurable thresholds
    drilling_models.rs # MSE, d-exponent, kick/loss/founder detection

  acquisition/
    wits_parser.rs     # WITS Level 0 TCP with reconnection, timeouts,
                       # and data quality validation

  llm/
    tactical_llm.rs    # Qwen 2.5 1.5B classification (GPU & CPU)
    strategic_llm.rs   # Qwen 2.5 7B (GPU) / 4B (CPU) advisory generation
    mistral_rs.rs      # Backend with runtime CUDA detection

  api/
    routes.rs          # HTTP route definitions
    handlers.rs        # Request handlers (config, advisory ack, shift summary)

static/
  index.html           # Dashboard UI

deploy/
  sairen-os.service    # systemd service unit (hardened)
  install.sh           # Production install script

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
| **Rig State** | Operational mode: Drilling, Circulating, Connection, Tripping, Idle |
| **Operation** | Activity classification: Production Drilling, Milling, Cement Drill-Out, Circulating, Static |
| **Milling** | P&A operation: cutting through casing; high torque, very low ROP |
| **Cement Drill-Out** | P&A operation: drilling cement plugs; high WOB, moderate torque, low ROP |

---

## Changelog

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
| Tactical Physics | < 15ms | ~10ms |
| Tactical LLM | < 60ms | ~50ms |
| Strategic LLM | < 800ms | ~750ms |
| WITS Packet Rate | 1 Hz | 1 Hz |
| History Buffer | 60 packets | 60 |

### CPU Mode (no CUDA)

| Metric | Target | Actual |
|--------|--------|--------|
| Tactical Physics | < 15ms | ~10ms |
| Tactical LLM | < 5s | ~2-5s |
| Strategic LLM | < 30s | ~10-30s |
| WITS Packet Rate | 1 Hz | 1 Hz |
| History Buffer | 60 packets | 60 |

> **Note**: Physics-based detection (MSE, d-exponent, flow balance) runs at the same speed
> on both GPU and CPU. Only LLM advisory generation is affected by hardware.

---

## License

Proprietary - SAIREN-OS Team
