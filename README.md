# SAIREN-OS - Drilling Operational Intelligence System

Real-time drilling advisory system using WITS Level 0 data and an 11-phase multi-agent AI pipeline for drilling optimization and risk prevention.

---

## Table of Contents

1. [Quick Start](#quick-start)
2. [Overview](#overview)
3. [Building](#building)
4. [First-Time Setup](#first-time-setup)
5. [Configuration Reference](#configuration-reference)
6. [Running the System](#running-the-system)
7. [WITS Simulator](#wits-simulator)
8. [Dashboard & Monitoring](#dashboard--monitoring)
9. [Understanding Advisories](#understanding-advisories)
10. [API Reference](#api-reference)
11. [Fleet Hub](#fleet-hub)
12. [Deployment](#deployment)
13. [Troubleshooting](#troubleshooting)
14. [Architecture Overview](#architecture-overview)
15. [Glossary](#glossary)
16. [Changelog](#changelog)

---

## Quick Start

```bash
# 1. Build the system (LLM enabled by default; use 'cuda' for GPU acceleration)
cargo build --release

# 2. Run with the built-in simulator (pipes simulated WITS data via stdin)
cargo run --bin simulation -- --hours 1 --speed 100 | ./target/release/sairen-os --stdin

# 3. View the dashboard
open http://localhost:8080
```

Or connect to a real WITS source:

```bash
./target/release/sairen-os --wits-tcp <wits-host>:5000
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

## Building

```bash
# Default build (LLM enabled, CPU inference — works on any machine)
cargo build --release

# With GPU acceleration (requires CUDA toolkit)
cargo build --release --features cuda

# Without LLM (template-based advisories only)
cargo build --release --no-default-features

# Fleet Hub server (requires PostgreSQL)
cargo build --release --bin fleet-hub --features fleet-hub
```

**Hardware auto-detection**: When built with `llm` or `cuda`, SAIREN-OS checks for CUDA at startup and automatically selects the right model:

| Hardware | Tactical Routing | Strategic Model | Build Flag |
|----------|-----------------|----------------|------------|
| **GPU** (CUDA) | Deterministic pattern matching | Qwen 2.5 7B (~800ms) | `--features cuda` |
| **CPU** | Deterministic pattern matching | Qwen 2.5 4B (~10-30s) | *(default)* |
| **No LLM** | Deterministic pattern matching | Template-based | `--no-default-features` |

**Feature flags:**

| Feature | Flag | Description |
|---------|------|-------------|
| **LLM (CPU)** | `--features llm` | Qwen 2.5 strategic advisory generation (default, enabled by default) |
| **LLM (GPU)** | `--features cuda` | CUDA-accelerated LLM inference |
| **Fleet Hub** | `--features fleet-hub` | Central hub server binary (PostgreSQL, API, curator) |
| **Tactical LLM** | `--features tactical_llm` | Legacy LLM-based tactical routing (not recommended) |

Fleet client, knowledge base, and fleet intelligence are always compiled — no feature flag needed.

> **Note**: The tactical agent uses deterministic physics-based pattern matching (no LLM required).
> Feature flags are additive and can be combined (e.g., `--features "cuda,fleet-hub"`).

---

## First-Time Setup

### Option A: Setup Wizard (recommended)

The interactive web-based setup wizard scans for WITS sources, configures well identity, and pairs with a Fleet Hub — all from a browser.

```bash
# Launch the setup wizard (opens web UI on :8080)
sairen-os setup
```

The wizard walks through:
1. **WITS Scanner** — probes your subnet for WITS TCP servers
2. **Well Identity** — configure well name, rig ID, field name
3. **Fleet Pairing** — pair with a Fleet Hub via 6-digit code (no passphrase needed)
4. **Config Generation** — writes `well_config.toml` with discovered settings

### Option B: Manual Configuration

```bash
# Generate a config file from defaults
sairen-os generate-config

# Edit for your well
vi well_config.toml
```

### Option C: Zero Configuration

Run with safe built-in defaults — no config file needed:

```bash
./target/release/sairen-os --wits-tcp <your-wits-host>:5000
```

### Key Configuration Sections

| Section | Controls | Example |
|---------|----------|---------|
| `[well]` | Well name, rig ID, bit diameter | `bit_diameter_inches = 8.5` |
| `[thresholds.well_control]` | Kick/loss warning & critical triggers | `flow_imbalance_warning_gpm = 5.0` |
| `[thresholds.mse]` | MSE efficiency bands | `efficiency_poor_percent = 50.0` |
| `[thresholds.hydraulics]` | ECD margin, SPP deviation | `ecd_margin_warning_ppg = 0.3` |
| `[thresholds.mechanical]` | Torque, pack-off detection | `torque_increase_warning_pct = 15.0` |
| `[thresholds.founder]` | Founder point detection sensitivity | `quick_wob_delta_percent = 0.05` |
| `[baseline_learning]` | Sigma thresholds, min samples | `min_samples_for_lock = 100` |
| `[ensemble_weights]` | Specialist voting weights (must sum to ~1.0) | `well_control = 0.30` |
| `[physics]` | Mud weight, formation constants | `normal_mud_weight_ppg = 10.0` |
| `[campaign.*]` | Per-campaign threshold overrides | `[campaign.plug_abandonment]` |

Only include sections you want to override — all omitted values use safe defaults. The system validates consistency on load (e.g., critical > warning thresholds, weights sum check).

---

## Configuration Reference

### Config File Search Order

The system searches for `well_config.toml` in this order:

1. `$SAIREN_CONFIG` environment variable (if set)
2. `./well_config.toml` in the working directory
3. Built-in defaults (safe for most wells)

### Runtime Configuration API

Thresholds can be viewed and updated at runtime without restarting:

```bash
# View current config
curl http://localhost:8080/api/v2/config | jq .

# Update thresholds (validates before applying, saves to well_config.toml)
curl -X POST http://localhost:8080/api/v2/config \
  -H "Content-Type: application/json" \
  -d '{"config": {"thresholds": {"well_control": {"flow_imbalance_warning_gpm": 8.0}}}}'

# Validate without applying
curl -X POST http://localhost:8080/api/v2/config/validate \
  -H "Content-Type: application/json" \
  -d '{"config": {"ensemble_weights": {"mse": 0.5, "hydraulic": 0.5, "well_control": 0.0, "formation": 0.0}}}'
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `SAIREN_CONFIG` | *(none)* | Path to `well_config.toml` (overrides search) |
| `CAMPAIGN` | `production` | Campaign mode: `production` or `pa` |
| `STRATEGIC_MODEL_PATH` | *(auto-selected)* | Strategic LLM model path (GPU: 7B, CPU: 4B) |
| `TACTICAL_MODEL_PATH` | `models/qwen2.5-1.5b-instruct-q4_k_m.gguf` | Only with `tactical_llm` feature |
| `SAIREN_KB` | *(none)* | Root directory of the structured knowledge base |
| `SAIREN_KB_FIELD` | *(none)* | Field name for knowledge base assembly |
| `SAIREN_KB_WELL` | `unknown` | Well name override for knowledge base |
| `SAIREN_KB_MAX_SNAPSHOTS` | `168` | Max hot mid-well snapshots before compression |
| `SAIREN_KB_RETENTION_DAYS` | `30` | Days to retain compressed snapshots |
| `RESET_DB` | *(none)* | Set to `true` to wipe all persistent data on startup |
| `SAIREN_SERVER_ADDR` | `0.0.0.0:8080` | HTTP server bind address |
| `SAIREN_CORS_ORIGINS` | *(none)* | Comma-separated CORS origins (e.g. `http://localhost:5173`) |
| `RUST_LOG` | `info` | Log level: `debug`, `info`, `warn`, `error` |
| `ML_INTERVAL_SECS` | `3600` | ML analysis interval (seconds) |
| `WELL_ID` | `WELL-001` | Well identifier for ML storage |
| `FIELD_NAME` | `DEFAULT` | Field/asset name |
| `FLEET_HUB_URL` | *(none)* | Fleet Hub URL — enables fleet sync when set |
| `FLEET_RIG_ID` | *(none)* | Rig identifier for fleet communication |
| `FLEET_PASSPHRASE` | *(none)* | Shared passphrase for fleet hub enrollment |

### CLI Arguments

| Argument | Description |
|----------|-------------|
| `--wits-tcp <host:port>` | Connect to WITS Level 0 TCP server |
| `--stdin` | Read WITS JSON packets from stdin |
| `--csv <path>` | Replay WITS data from CSV file |
| `--addr <host:port>` | Override HTTP server address |
| `--speed <N>` | Simulation speed multiplier (default: 1) |
| `--reset-db` | Wipe all persistent data on startup |

### CLI Subcommands

| Subcommand | Description |
|------------|-------------|
| `setup` | Launch the setup wizard (web UI on :8080) |
| `pair --hub <url> --rig <id> --well <id> --field <name>` | Headless CLI pairing with a Fleet Hub via 6-digit code |
| `generate-config` | Generate a `well_config.toml` from current defaults |
| `migrate-kb --from <path> --to <path>` | Migrate a flat `well_prognosis.toml` into the KB directory structure |

---

## Running the System

### With Built-In Simulator (Recommended)

The Rust simulator (`src/bin/simulation.rs`) generates realistic WITS data and pipes it via stdin:

```bash
# 1-hour simulation at 100x speed (runs through baseline, normal, kick, pack-off, recovery phases)
cargo run --bin simulation -- --hours 1 --speed 100 | ./target/release/sairen-os --stdin
```

Simulator options:

| Argument | Default | Description |
|----------|---------|-------------|
| `-H, --hours <N>` | `1` | Simulation duration (1-24 hours) |
| `-s, --speed <N>` | `100` | Time compression (1 = real-time, 100 = 100x faster) |
| `-f, --format <fmt>` | `json` | Output format: `json` or `csv` |
| `--scenario <name>` | `full` | Drilling scenario to simulate |
| `--seed <N>` | *(random)* | Random seed for reproducibility |
| `--sample-rate <N>` | `1` | Output sample rate in Hz |
| `-q, --quiet` | `false` | Suppress mission log (only output sensor data) |

### With P&A Campaign

```bash
CAMPAIGN=pa cargo run --bin simulation -- --hours 1 --speed 100 | ./target/release/sairen-os --stdin
```

### Volve Field Data Replay

```bash
# Replay a well CSV (Kaggle or Tunkiel format auto-detected)
cargo run --bin volve-replay -- --file data/volve/F-5_rt_input.csv

# Extract WITSML XML from Volve zip archive to Kaggle CSV
python3 scripts/witsml_to_csv.py F-12 data/volve/F-12_witsml.csv

# Replay the extracted well
cargo run --bin volve-replay -- --file data/volve/F-12_witsml.csv
```

### Connecting to a Real WITS Source

```bash
./target/release/sairen-os --wits-tcp <wits-host>:<port>
```

---

## WITS Simulator

The built-in Rust simulator (`src/bin/simulation.rs`) generates realistic WITS Level 0 data that pipes into SAIREN-OS via stdin.

### Basic Usage

```bash
# Full scenario: baseline learning → normal drilling → kick → pack-off → recovery
cargo run --bin simulation -- --hours 1 --speed 100 | ./target/release/sairen-os --stdin

# Real-time speed (1x), 2-hour simulation
cargo run --bin simulation -- --hours 2 --speed 1 | ./target/release/sairen-os --stdin

# Reproducible run with fixed seed
cargo run --bin simulation -- --hours 1 --speed 100 --seed 42 | ./target/release/sairen-os --stdin
```

### Simulation Phases

The simulator automatically progresses through realistic drilling scenarios:

| Phase | Progress | Scenario |
|-------|----------|----------|
| **Baseline Learning** | 0-40% | System warmup, stable drilling |
| **Normal Drilling** | 40-55% | Optimal parameters, efficient ROP |
| **MSE Inefficiency** | 55-70% | Bit wear / formation change |
| **Kick Event** | 70-80% | Well control event simulation |
| **Pack-Off** | 80-90% | Mechanical issue (reaming state) |
| **Recovery** | 90-100% | Return to normal drilling |

---

## Dashboard & Monitoring

SAIREN-OS includes a React SPA dashboard served at `http://localhost:8080`. It uses the v2 API exclusively (`/api/v2/live` consolidated endpoint) and is compiled into the binary via `rust-embed`.

**Development mode** (for dashboard contributors):
```bash
cd dashboard && npm run dev   # Vite dev server on :5173
SAIREN_CORS_ORIGINS=http://localhost:5173 ./target/release/sairen-os --stdin
```

If the dashboard was not built (e.g., CI without Node.js), a fallback message is shown at the root URL.

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
ADVISORY #12: ELEVATED | Efficiency: 68%

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
      |
      v
Is category WELL CONTROL? --YES--> Execute well control procedures
      |
      NO
      v
Is risk level CRITICAL? --YES--> Stop drilling, investigate immediately
      |
      NO
      v
Is category MECHANICAL? --YES--> Check for pack-off/stick-slip
      |
      NO
      v
Is category EFFICIENCY? --YES--> Optimize WOB/RPM per recommendation
      |
      NO
      v
Continue monitoring
```

### Thresholds Reference

#### MSE Efficiency

| Efficiency | Status | Action |
|------------|--------|--------|
| > 85% | Optimal | Continue current parameters |
| 70-85% | Acceptable | Monitor, minor adjustments |
| 50-70% | Poor | Optimize WOB/RPM |
| < 50% | Very Poor | Immediate parameter review |

#### Well Control (Safety-Critical)

| Parameter | Warning | Critical |
|-----------|---------|----------|
| Flow Imbalance | > 5 gpm | > 10 gpm |
| Pit Gain | > 5 bbl | > 10 bbl |
| Pit Rate | > 5 bbl/hr | > 15 bbl/hr |
| Gas Units | > 100 | > 250 |
| H2S | > 10 ppm | > 20 ppm |

#### Hydraulics

| Parameter | Warning | Critical |
|-----------|---------|----------|
| ECD Margin | < 0.3 ppg | < 0.1 ppg |
| SPP Deviation | > 100 psi | > 200 psi |

#### Mechanical

| Parameter | Warning | Critical |
|-----------|---------|----------|
| Torque Increase | > 15% | > 25% |
| Combined Torque + SPP | Both rising | Sustained trend |
| Founder Condition | WOB +5%, ROP flat | WOB +5%, ROP decreasing |

#### Founder Detection

Founder occurs when WOB exceeds the optimal point and ROP stops responding or decreases despite increasing weight. This indicates bit balling, cuttings accumulation, or reaching the formation's founder point.

| Severity | ROP Response | Action |
|----------|--------------|--------|
| Low (30%) | ROP flat despite WOB increase | Monitor, consider reducing WOB |
| Medium (50%) | ROP slightly decreasing | Reduce WOB to optimal point |
| High (70%+) | ROP actively decreasing | Reduce WOB immediately |

The system estimates the optimal WOB (where ROP was highest) and provides specific recommendations.

---

## API Reference

Base URL: `http://localhost:8080`

### v2 API (Primary)

The v2 API uses a consistent JSON envelope (`ApiResponse<T>`) for all responses.

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/v2/system/health` | GET | System health status |
| `/api/v2/live` | GET | Consolidated live data (replaces 7 v1 polls) |
| `/api/v2/drilling` | GET | Current drilling metrics |
| `/api/v2/reports/hourly` | GET | Hourly strategic reports |
| `/api/v2/reports/daily` | GET | Daily strategic reports |
| `/api/v2/reports/critical` | GET | Critical advisory reports |
| `/api/v2/ml/latest` | GET | Latest ML insights report |
| `/api/v2/ml/optimal?depth=N` | GET | Optimal parameters for depth |
| `/api/v2/config` | GET | Current well configuration |
| `/api/v2/config` | POST | Update configuration |
| `/api/v2/config/validate` | POST | Validate config without applying |
| `/api/v2/config/reload` | POST | Trigger manual config reload from file |
| `/api/v2/config/suggestions` | GET | Threshold adjustment suggestions from feedback |
| `/api/v2/campaign` | GET | Current campaign and thresholds |
| `/api/v2/campaign` | POST | Switch campaign |
| `/api/v2/advisory/acknowledge` | POST | Acknowledge an advisory |
| `/api/v2/advisory/acknowledgments` | GET | List advisory acknowledgments |
| `/api/v2/advisory/feedback/:timestamp` | POST | Submit operator feedback on advisory |
| `/api/v2/advisory/feedback/stats` | GET | Per-category feedback statistics |
| `/api/v2/shift/summary` | GET | Shift summary with `?hours=12` |
| `/api/v2/lookahead/status` | GET | Formation lookahead advisory status |
| `/api/v2/damping/status` | GET | Stick-slip damping analysis + recommendation |
| `/api/v2/damping/recipes` | GET | Per-formation damping recipe library |
| `/api/v2/debug/baseline` | GET | Baseline learning status |
| `/api/v2/debug/ml/history` | GET | ML analysis history |
| `/api/v2/debug/fleet/intelligence` | GET | Fleet intelligence cache |
| `/api/v2/metrics` | GET | Prometheus metrics |

### v1 API (Deprecated)

> **Deprecated**: v1 endpoints include `Deprecation: true` and `Sunset: 2026-09-01` headers.
> Migrate to v2 before the sunset date. The React dashboard already uses v2 exclusively.

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/v1/health` | GET | System health status |
| `/api/v1/status` | GET | System status, metrics, current operation |
| `/api/v1/drilling` | GET | Current drilling metrics |
| `/api/v1/verification` | GET | Latest ticket verification result |
| `/api/v1/diagnosis` | GET | Current strategic advisory (204 if none) |
| `/api/v1/baseline` | GET | Baseline learning status |
| `/api/v1/campaign` | GET/POST | Campaign status and switching |
| `/api/v1/config` | GET/POST | Configuration view and update |
| `/api/v1/config/validate` | POST | Validate config without applying |
| `/api/v1/advisory/acknowledge` | POST | Acknowledge an advisory |
| `/api/v1/advisory/acknowledgments` | GET | List all advisory acknowledgments |
| `/api/v1/shift/summary` | GET | Shift summary with `?hours=12` or `?from=&to=` |
| `/api/v1/reports/critical` | GET | Critical advisory reports |
| `/api/v1/reports/test` | POST | Create a test critical report (debug builds only) |
| `/api/v1/ml/latest` | GET | Latest ML insights report |
| `/api/v1/ml/history?limit=N` | GET | ML analysis history |
| `/api/v1/ml/optimal?depth=N` | GET | Optimal parameters for depth |
| `/api/v1/strategic/hourly` | GET | Hourly strategic reports |
| `/api/v1/strategic/daily` | GET | Daily strategic reports |
| `/api/v1/metrics` | GET | Prometheus metrics |
| `/api/v1/fleet/intelligence` | GET | Fleet intelligence cache |

---

## Fleet Hub

The Fleet Hub is an optional central server that enables fleet-wide learning across multiple rigs. Each rig operates autonomously and uploads significant events to the hub. The hub curates events into a scored episode library that is synced back to all rigs.

### Running the Fleet Hub

```bash
# 1. Ensure PostgreSQL is running
# 2. Set environment variables
export DATABASE_URL=postgres://sairen:password@localhost/sairen_fleet
export FLEET_PASSPHRASE=your-shared-passphrase

# 3. Start the hub (migrations run automatically)
./target/release/fleet-hub --port 8080

# 4. View the fleet dashboard
open http://localhost:8080
```

### Registering a Rig

```bash
# Enroll a new rig (passphrase auth, X-Rig-ID header required)
curl -X POST http://hub:8080/api/fleet/enroll \
  -H "Authorization: Bearer $FLEET_PASSPHRASE" \
  -H "X-Rig-ID: RIG-001" \
  -H "Content-Type: application/json" \
  -d '{"rig_id": "RIG-001", "well_id": "WELL-A1", "field": "North Sea"}'
```

Headless pairing alternative (6-digit code flow):
```bash
sairen-os pair --hub http://hub:8080 --rig RIG-001 --well WELL-A1 --field "North Sea"
```

### Hub Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | *(required)* | PostgreSQL connection URL |
| `FLEET_PASSPHRASE` | *(required)* | Shared passphrase for all hub auth (admin + rig) |
| `FLEET_MAX_PAYLOAD_SIZE` | `1048576` | Max event upload size in bytes (1 MB) |
| `FLEET_CURATION_INTERVAL` | `3600` | Curation cycle interval in seconds |
| `FLEET_LIBRARY_MAX_EPISODES` | `50000` | Maximum episodes before pruning |
| `SAIREN_LLM_MODEL_PATH` | *(none)* | Path to GGUF model for intelligence workers |
| `INTELLIGENCE_INTERVAL_SECS` | `60` | Intelligence job poll interval in seconds |

### Hub CLI Arguments

| Argument | Description |
|----------|-------------|
| `--database-url <url>` | PostgreSQL connection URL (overrides env) |
| `--port <N>` | Port to listen on (default: 8080) |
| `--bind-address <addr>` | Full bind address (overrides --port) |

### Hub API Endpoints

**Event Ingestion** (passphrase + `X-Rig-ID` auth):

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/fleet/events` | POST | Upload a fleet event (supports zstd compression) |
| `/api/fleet/events/{id}` | GET | Retrieve an event by ID |
| `/api/fleet/events/{id}/outcome` | PATCH | Update event outcome |

**Library Sync** (passphrase + `X-Rig-ID` auth):

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/fleet/library` | GET | Sync library (delta via `If-Modified-Since`, supports zstd) |
| `/api/fleet/library/stats` | GET | Library statistics |

**Rig Registry** (passphrase auth):

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/fleet/enroll` | POST | Enroll a new rig (passphrase + `X-Rig-ID` header) |
| `/api/fleet/rigs` | GET | List all registered rigs |
| `/api/fleet/rigs/{id}` | GET | Get rig details |
| `/api/fleet/rigs/{id}/revoke` | POST | Revoke a rig |

**Performance Data** (passphrase + `X-Rig-ID` auth):

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/fleet/performance` | POST | Upload post-well performance data (zstd) |
| `/api/fleet/performance` | GET | Query by field (`?field=&since=&exclude_rig=`) |

**Pairing** (code-based rig onboarding):

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/fleet/pair/request` | POST | Rig submits 6-digit pairing code (unauthenticated) |
| `/api/fleet/pair/approve` | POST | Admin approves a pairing code (passphrase auth) |
| `/api/fleet/pair/status` | GET | Rig polls pairing status (`?code=123456`, unauthenticated) |
| `/api/fleet/pair/pending` | GET | List pending pairing requests (passphrase auth) |

**Intelligence & Graph** (passphrase auth):

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/fleet/intelligence` | GET | Pull intelligence outputs (`?since=&formation=`) |
| `/api/fleet/graph/stats` | GET | Knowledge graph statistics |
| `/api/fleet/graph/formation` | GET | Formation context lookup |
| `/api/fleet/graph/rebuild` | POST | Rebuild knowledge graph |
| `/api/fleet/metrics` | GET | Prometheus metrics (no auth) |

**Dashboard** (passphrase auth):

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/fleet/dashboard/summary` | GET | Fleet overview (active rigs, events, episodes) |
| `/api/fleet/dashboard/trends` | GET | Event trends over time (`?days=30`) |
| `/api/fleet/dashboard/outcomes` | GET | Outcome analytics (resolution rates by category) |
| `/api/fleet/health` | GET | Hub health check |
| `/` | GET | Fleet dashboard HTML page |

### Network Architecture

The hub communicates with rigs over a WireGuard VPN tunnel. Configuration templates are provided in `deploy/wireguard/`.

```
Rig (10.0.1.X) ---- WireGuard Tunnel ---- Hub (10.0.0.1:8080)
                     (port 51820)
```

**Key design principles:**
- Only AMBER/RED events qualify for upload
- Upload queue survives process restarts
- Rigs operate independently when hub is unreachable (local autonomy)
- Bandwidth-conscious: zstd compression, delta sync, configurable cadence

For curator rules, episode scoring, and component details, see [ARCHITECTURE.md](ARCHITECTURE.md#fleet-hub-internals).

---

## Deployment

### Production Deployment (systemd)

SAIREN-OS ships with a systemd service unit and install script for rig-edge deployment.

```bash
# 1. Build the release binary
cargo build --release

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

## Troubleshooting

### No advisories being generated

1. **Still in baseline learning** - Wait ~2 minutes for 100 samples. Check: `curl http://localhost:8080/api/v2/debug/baseline`
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

## Architecture Overview

SAIREN-OS uses a two-stage multi-agent architecture where a fast **Tactical Agent** handles real-time anomaly detection via deterministic pattern-matched routing, and a deeper **Strategic Agent** performs comprehensive drilling physics analysis only when anomalies are detected.

The **Orchestrator** uses 4 trait-based specialists for domain-specific evaluation, returning a `VotingResult`. The **AdvisoryComposer** assembles the final advisory with a CRITICAL cooldown (30s) to prevent alert spam.

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
| 2.8 | CfC Network | Self-supervised neural network: predict, compare, train, score |
| 3 | Decision Gate | 6-rule ticket gate (rig state, anomaly, cooldown, ACI, CfC, founder debounce) |
| 4 | History Buffer | Store last 60 packets for trend analysis |
| 4.5 | Causal Inference | Cross-correlate parameters against MSE at lags 1-20s |
| 5 | Advanced Physics | Strategic verification of tickets |
| 6 | Context Lookup | Query knowledge store for precedents |
| 7 | LLM Advisory | Generate recommendations or template fallback |
| 8 | Orchestrator Voting | 4 specialists vote with regime-adjusted weights |
| 9 | Advisory Composition | Assemble final advisory (CRITICAL cooldown) |

### Specialist Weights

| Specialist | Baseline Weight | Evaluates |
|------------|-----------------|-----------|
| **MSE** | 25% | Drilling efficiency, ROP optimization |
| **Hydraulic** | 25% | SPP, ECD margin, flow rates |
| **WellControl** | 30% | Kick/loss indicators, gas, pit volume |
| **Formation** | 20% | D-exponent trends, formation changes |

> **Regime adjustment**: The CfC k-means clusterer stamps a regime (0-3) on each packet. The orchestrator applies regime-specific multipliers to these weights and re-normalizes before voting. See the regime multiplier table in [ARCHITECTURE.md](ARCHITECTURE.md#regime-aware-orchestrator-weighting).

### Campaign System

| Campaign | Focus | Flow Warning | Well Control Weight |
|----------|-------|--------------|---------------------|
| **Production** | ROP optimization, MSE efficiency | 5 gpm | 30% |
| **Plug & Abandonment** | Cement integrity, pressure testing | 5 gpm | 40% |

Switch campaigns via dashboard dropdown, API (`POST /api/v2/campaign`), or `CAMPAIGN=pa` env var.

### CfC Neural Network

Dual 64-neuron Closed-form Continuous-time (CfC) network with NCP sparse wiring. Two networks run in parallel via `rayon::join()`: a **fast network** (LR 0.001, BPTT=4) catches acute events and a **slow network** (LR 0.0001, BPTT=8) catches gradual trends. Combined scoring via `max(fast, slow)`. Self-supervised — predicts next-timestep sensor values and uses prediction error as anomaly signal. No labeled training data needed. Calibrates after 500 packets. Participates in severity modulation, LLM context enrichment, and strategic tiebreaking.

For full CfC architecture, training details, and validation results, see [ARCHITECTURE.md](ARCHITECTURE.md#cfc-neural-network).

### Additional Systems

- **ML Engine (V2.2)** — hourly analysis finds optimal drilling conditions using dysfunction-aware optimization
- **Structured Knowledge Base** — per-well directory-based KB with geology, engineering, and offset well performance data
- **Causal Inference** — detects which parameters causally precede MSE spikes using cross-correlation at lags 1-20s
- **Fleet Intelligence** — fleet-wide learning via hub-and-spoke episode library

For implementation details on all systems, see [ARCHITECTURE.md](ARCHITECTURE.md).

---

## Glossary

| Term | Description |
|------|-------------|
| **WITS** | Wellsite Information Transfer Specification — industry standard for real-time drilling data (40+ channels) |
| **ROP** | Rate of Penetration (ft/hr) — drilling speed |
| **WOB** | Weight on Bit (klbs) — downward force on drill bit |
| **RPM** | Rotations Per Minute — drill string rotation speed |
| **MSE** | Mechanical Specific Energy (psi) — energy to remove rock; lower = more efficient |
| **D-exponent** | Normalized parameter tracking formation changes; rising may indicate pore pressure increase |
| **ECD** | Equivalent Circulating Density (ppg) — effective mud weight including friction |
| **SPP** | Standpipe Pressure (psi) — pump pressure at surface |
| **Kick** | Uncontrolled influx of formation fluids — CRITICAL safety event |
| **Lost Circulation** | Mud loss into formation |
| **Pack-off** | Restriction from cuttings buildup; signs: rising torque + SPP |
| **Stick-slip** | Torsional oscillation; torque fluctuates cyclically |
| **Founder** | Condition where WOB exceeds optimal and ROP stops responding |
| **Flow Balance** | flow_out - flow_in (gpm); positive = potential kick |
| **Pit Volume** | Mud volume in surface pits (bbl) |
| **Rig State** | Operational mode: Drilling, Reaming, Circulating, Connection, Tripping |
| **Operation** | Activity classification: Production Drilling, Milling, Cement Drill-Out, Circulating, Static |
| **Campaign** | Operating mode that adjusts thresholds and specialist weights (Production or P&A) |
| **Fleet Hub** | Central server that collects events from all rigs and curates a shared episode library |
| **Spoke** | Individual rig running SAIREN-OS, uploading events to and syncing from the hub |

For developer/ML terms (CfC, NCP, BPTT, ACI, RegimeProfile, etc.), see [ARCHITECTURE.md](ARCHITECTURE.md#developer-glossary).

---

## Changelog

See [CHANGELOG.md](CHANGELOG.md) for detailed version history.

Current version: v4.0-dev

---

## License

Proprietary - SAIREN-OS Team
