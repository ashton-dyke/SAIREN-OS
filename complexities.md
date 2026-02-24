# SAIREN-OS: The Path to Zero Friction

> This system exists to make drilling operations safer and simpler. If the system
> itself isn't safe and simple to deploy, it has failed before it even starts.
>
> The standard: plug it in, it works. No manuals. No config files. No feature flags.
> Like plugging in a Chromecast — not assembling a server rack.

---

## Contents

1. [Design Philosophy](#1-design-philosophy)
2. [85 Config Fields. The User Needs 0.](#2-85-config-fields-the-user-needs-0)
3. [Auto-Detection: What the Data Already Tells Us](#3-auto-detection-what-the-data-already-tells-us)
4. [Seamless Node Connections](#4-seamless-node-connections)
5. [Right Language, Right Job](#5-right-language-right-job)
6. [The Dashboard Problem](#6-the-dashboard-problem)
7. [The API Problem](#7-the-api-problem)
8. [Architecture Leaking Into UX](#8-architecture-leaking-into-ux)
9. [Silent Failures](#9-silent-failures)
10. [The 86KB README](#10-the-86kb-readme)
11. [The Build](#11-the-build)

---

## 1. Design Philosophy

Every decision below follows one rule:

**The user's job is to drill. Our job is to disappear.**

That means:

- If we can figure it out from the data, don't ask.
- If we can learn it from observation, don't configure it.
- If it's an implementation detail, don't expose it.
- If it breaks, say what broke in plain English.
- If two nodes need to talk, they find each other.

Google doesn't ask you to configure your DNS resolver. Chromecast doesn't ask
for your TV's IP address. Tesla doesn't ask you to tune your regenerative braking
coefficient. The complexity exists — it's just not the user's problem.

SAIREN-OS currently makes all of its complexity the user's problem.

---

## 2. 85 Config Fields. The User Needs 0.

### What exists today

`well_config.default.toml` exposes **85+ parameters** across 15 sections.
On top of that, there are **16 environment variables** and **6 compile-time
feature flags**. Configuration is split across three files:

| What                  | Where                      |
|-----------------------|----------------------------|
| Drilling thresholds   | `well_config.toml`         |
| WITS address, fleet   | `/etc/sairen-os/env`       |
| Server address        | CLI arg or env var         |

There is no validation. Typos are silently ignored. A wrong value in one field
(`normal_mud_weight_ppg`) silently corrupts d-exponent calculations by up to 28%.
No warning. No error. Just wrong answers.

### What should exist

**Zero config for first run. One file for production. No env vars.**

```
$ sairen-os
# Auto-detects WITS on the local network.
# Learns baselines from live data.
# Dashboard opens at http://localhost:8080.
# That's it.
```

For production, one file with only the things the system genuinely cannot
figure out on its own:

```toml
# /etc/sairen-os/config.toml — the entire config

well = "Endeavour-7"
field = "North Sea"

# Safety: regulatory limits (can't be learned, set by safety officer)
max_h2s_ppm = 10
```

That's it. Everything else is auto-detected or learned. See Section 3.

### What's wrong with the current 85 fields

Every field in the current config falls into one of five categories:

| Category | Count | What to do |
|----------|-------|------------|
| **AUTO** — detectable from WITS data | ~22 | Read from data stream |
| **LEARN** — learnable from baseline observation | ~34 | Baseline learning already exists; extend it |
| **INFER** — derivable from other values | ~18 | Calculate, don't ask |
| **ASK-ONCE** — genuinely needs human input | ~10 | Keep in config (but just ~10 fields) |
| **UNNECESSARY** — never used or internal-only | ~4 | Delete |

The current system already has a baseline learning engine. It already observes
the first ~100 seconds of data and learns statistical norms. The problem is it
only uses that for anomaly scoring — it doesn't use it to auto-configure the
85 thresholds that are currently manual.

**Extend baseline learning to set its own thresholds.**

---

## 3. Auto-Detection: What the Data Already Tells Us

Every WITS packet contains 40+ channels of live drilling data. Most of the
config file is asking the user to type in values that are already flowing
through the system in real time.

### Fields that should auto-detect from WITS data

| Current config field | WITS source | How |
|---------------------|-------------|-----|
| `bit_diameter_inches` | WITS 0100 record / tool spec | Read from data header |
| `normal_mud_weight_ppg` | WITS 0124 (mud weight in) | Average during first stable circulation period |
| `idle_rpm_max` | RPM channel | Observe minimum RPM when pumps are off |
| `circulation_flow_min` | Flow rate channel | Smallest non-zero flow during baseline |
| `drilling_wob_min` | WOB channel | Minimum WOB during confirmed drilling |
| `trip_out_hook_load_min` | Hook load channel | Observe during first trip operation |
| `trip_in_hook_load_max` | Hook load channel | Observe during first trip operation |
| `gas_units_warning` | Gas channel | 3-sigma above baseline background gas |
| `milling_torque_min` | Torque channel | Signature detection from torque pattern |
| `stick_slip_cv_warning` | Torque variance | Statistical computation from baseline |
| `kick_gas_increase_threshold` | Gas channel | Background gas level + 3-sigma |
| `annular_pressure_loss_coefficient` | Hole geometry + mud + flow | Calculated from measurable inputs |

### Fields that should be learned from baseline observation

| Current config field | Learning method |
|---------------------|-----------------|
| `flow_imbalance_warning_gpm` | 3-sigma above normal flow balance during stable circulation |
| `flow_imbalance_critical_gpm` | 5-sigma above normal flow balance |
| `pit_gain_warning_bbl` | 3-sigma above normal pit volume variance |
| `pit_rate_warning_bbl_hr` | 3-sigma above normal pit rate |
| `spp_deviation_warning_psi` | 3-sigma above normal SPP variance |
| `torque_increase_warning` | 3-sigma above normal torque variance |
| `efficiency_optimal_percent` | Observed MSE efficiency during good drilling |
| `efficiency_warning_percent` | 70% of observed optimal (or 3-sigma below) |
| `packoff_spp_increase_threshold` | Learned from baseline SPP stability |
| `dexp_increase_warning` | 3-sigma above normal d-exponent drift |
| All `[thresholds.founder]` | Learned from WOB/ROP response curves |
| All `[thresholds.formation]` | Learned from baseline d-exponent stability |

### Fields that should be inferred (calculated from other values)

| Current config field | Inferred from |
|---------------------|---------------|
| All `[strategic_verification]` (11 fields) | Derived from `[well_control]` and `[hydraulics]` thresholds. These are literally the same thresholds with different names. |
| `baseline_learning.warning_sigma` | Industry standard: 3.0. Not configurable. |
| `baseline_learning.critical_sigma` | Industry standard: 5.0. Not configurable. |
| `baseline_learning.min_samples_for_lock` | = window_seconds x sample_rate. Calculable. |
| `physics.confidence_full_window` | = desired_window x sample_rate. Calculable. |
| `stick_slip_min_samples` | = baseline_window / sample_rate. Calculable. |
| `founder.min_samples` | Same. |
| `operation_detection.no_rotation_rpm_max` | = idle_rpm_max. Same value, duplicate field. |

### Fields that are unnecessary

| Field | Why |
|-------|-----|
| `well.operator` | Never referenced anywhere in the codebase. Dead config. |
| `ensemble_weights.*` | Internal specialist voting weights. Should never be user-facing. |
| `campaign.*` overrides | Duplicates of main thresholds. DRY violation. |

### Fields that genuinely need human input (~10 total)

| Field | Why it can't be automated |
|-------|--------------------------|
| `well` (name) | Identity. Must be set by operator. |
| `field` | Identity. Must be set by operator. |
| `rig` | Identity. Can default to hostname. |
| `h2s_warning_ppm` | Regulatory safety limit. Set by safety officer. |
| `h2s_critical_ppm` | Regulatory safety limit. |
| `fracture_gradient_ppg` | From well plan. Could auto-detect if WITS 0150 available. |
| `advisory.default_cooldown_seconds` | Operator preference. Sensible default: 60s. |
| WITS server address | Network topology. Could be mDNS-discovered (see Section 4). |

**That's it.** Eight fields that a human might need to touch. Everything else
is knowable from the data or derivable from math. The current system asks for
85+ because it doesn't trust its own data.

---

## 4. Seamless Node Connections

### What exists today

Every connection is manual:

| Connection | How it's configured today |
|------------|--------------------------|
| Rig -> WITS server | `--wits-tcp host:port` CLI arg |
| Rig -> Dashboard | `SAIREN_SERVER_ADDR` env var or `--addr` CLI arg |
| Rig -> Fleet Hub | `FLEET_HUB_URL` env var (must know IP in advance) |
| Rig -> Fleet auth | `FLEET_PASSPHRASE` env var (distributed out-of-band) |
| Hub -> PostgreSQL | `DATABASE_URL` env var (full connection string) |
| Hub -> Network | `--bind-address 10.0.0.1` (hard-coded in service file) |
| Rig -> Rig | Not possible. All traffic through hub. |

Fleet enrollment is a 6-step manual CLI process:

```bash
sairen-os enroll --hub http://hub:8080 --passphrase SECRET \
  --rig-id RIG-001 --well-id WELL-001 --field FIELD
```

The operator must know the hub's IP, have the passphrase, and run a CLI command.
If the hub moves or the network changes, everything breaks.

### What should exist

**Nodes find each other. Automatically.**

#### Step 1: mDNS Service Discovery

Hub advertises itself on the local network:

```
Service: _sairen-hub._tcp.local
TXT: version=3.0, field=north-sea
```

Rig on startup:
1. Scans for `_sairen-hub._tcp.local` (5 second timeout)
2. If found: auto-connects. No config needed.
3. If not found: falls back to `hub_url` in config (if set), or runs standalone.

Same for WITS servers:

```
Service: _wits-server._tcp.local
TXT: well=endeavour-7, rate=1hz
```

Rig scans for WITS on startup. If one source is on the network, connects
automatically. If multiple, picks the one matching `well` from config. If none
found, prompts on dashboard: "No WITS source detected. Connect manually?"

**Zero IP addresses in config files. Ever.**

#### Step 2: Pairing-Code Enrollment (Replace Shared Passphrase)

Current auth: one shared passphrase for all rigs. Anyone with the passphrase
can impersonate any rig. Lost passphrases are unrecoverable.

Replace with pairing codes (like Bluetooth):

```
Rig startup (first time):
  1. Generates Ed25519 keypair
  2. Displays 6-digit pairing code on local dashboard: "PAIR: A3K9M2"
  3. Code expires in 5 minutes

Operator:
  1. Opens hub dashboard
  2. Clicks "Pair New Rig"
  3. Types "A3K9M2"
  4. Confirms: "Rig-001 on Well Endeavour-7? [Confirm]"

Hub:
  1. Issues TLS client certificate to rig (90-day validity)
  2. Auto-rotates 30 days before expiry
  3. Rig uses mTLS for all future connections

Result: No shared secrets. No CLI commands. No env vars. Rig identity is
cryptographic, not a string in a config file.
```

#### Step 3: Health-Aware Reconnection

Current: exponential backoff with 10 retries, then give up.

Replace with circuit breaker:

```
Healthy:         Normal 60s sync cycle
Degraded (1-3):  Backoff with jitter, events queue to disk
Open (>3):       Stop trying for 30 min, dashboard shows "Hub offline"
Recovery:        One success resets to Healthy
```

Dashboard always shows connection state. Operators never have to guess.

#### Step 4: The Whole Network, Self-Assembled

```
                    ┌─────────────────────┐
                    │     Fleet Hub       │
                    │  mDNS: _sairen-hub  │
                    │  Auto-TLS, auto-DB  │
                    └──────────┬──────────┘
                               │
                    mDNS discovery + mTLS
                               │
              ┌────────────────┼────────────────┐
              │                │                │
     ┌────────┴──────┐ ┌──────┴───────┐ ┌──────┴───────┐
     │   Rig Edge    │ │   Rig Edge   │ │   Rig Edge   │
     │  Auto-paired  │ │  Auto-paired │ │  Auto-paired │
     │  mDNS: _wits  │ │  mDNS: _wits │ │  mDNS: _wits │
     └───────┬───────┘ └──────┬───────┘ └──────┬───────┘
             │                │                │
        auto-detect      auto-detect      auto-detect
             │                │                │
     ┌───────┴───────┐ ┌─────┴────────┐ ┌─────┴────────┐
     │  WITS Server  │ │  WITS Server │ │  WITS Server │
     └───────────────┘ └──────────────┘ └──────────────┘
```

No IP addresses. No passphrases. No enrollment CLI. Plug in, pair with a code,
drilling starts being monitored.

---

## 5. Right Language, Right Job

### The honest assessment

Rust is the right choice for some of this system and the wrong choice for others.
The problem isn't Rust itself — it's that everything is in Rust, which means
every change to every layer requires recompiling 41,000 lines of code.

### Where Rust is correct

| Component | Why Rust is right |
|-----------|-------------------|
| **Core pipeline** (Phase 1-3 tactical) | Deterministic <15ms execution. Zero-copy packet processing. Type safety enforces phase ordering. This is exactly what Rust is for. |
| **Physics engine** | Pure math, no I/O, must be fast. Rust's zero-overhead abstractions are ideal. |
| **Fleet hub server** | 10,000+ concurrent connections. No GC pauses. Single binary deployment. Axum + Tokio is excellent here. |
| **WITS acquisition** | TCP stream processing with reconnection logic. Rust's ownership model prevents data races. Correct. |

### Where Rust is wrong

| Component | Problem | Better choice |
|-----------|---------|---------------|
| **Dashboard frontend** | HTML/CSS/JS is embedded via `include_str!()` at compile time. Changing a button colour requires recompiling the entire binary and restarting the service. | **React/Next.js** — separate repo, separate deploy cycle. Designers can iterate in seconds. Hot reload. Component libraries. Tailwind for consistent design system. |
| **LLM integration** | Adding a new prompt template requires recompilation. Model swapping requires rebuilding. 138 lines just to handle async model loading. | **Python sidecar** (FastAPI + Ollama or vLLM). Change prompts in seconds. Swap models via config. Hot-reload. The edge binary talks to it over localhost HTTP. |
| **CfC neural network** (training) | Testing a new loss function = recompile (5-10 min). Inspecting activations requires custom logging. No Jupyter integration. Glacial iteration. | **Python** (PyTorch/JAX) for training + architecture experiments. Export to **ONNX** for inference. Rust loads the .onnx file at runtime. Training iteration drops from hours to minutes. |

### The target architecture

```
EDGE BOX (on the rig)
├── sairen-edge (Rust)
│   ├── WITS acquisition + TCP reconnection
│   ├── Physics engine (MSE, d-exp, ECD, kick detection)
│   ├── Pipeline coordinator (Phase 1-3 tactical)
│   ├── CfC inference (ONNX runtime, not training)
│   ├── REST API server (Axum)
│   └── Baseline learning + auto-config
│
├── sairen-llm (Python sidecar, optional)
│   ├── FastAPI server on localhost:8100
│   ├── Ollama or vLLM backend
│   ├── Prompt templates in YAML (hot-reloadable)
│   └── Model files in /opt/sairen-os/models/
│
└── sairen-dashboard (React, served as static files)
    ├── Built once, served by Rust HTTP server
    ├── Or: separate Next.js process for dev iteration
    └── Talks to sairen-edge API on localhost:8080

FLEET HUB (central office, optional)
├── sairen-hub (Rust) — separate binary, separate repo
│   ├── PostgreSQL backend
│   ├── mDNS advertisement
│   ├── mTLS certificate authority
│   └── Event ingestion + curator + library sync
│
└── sairen-hub-dashboard (React)
    └── Fleet-wide analytics, rig pairing UI
```

### What this unlocks

| Today | After |
|-------|-------|
| Change dashboard colour = recompile Rust = restart service | Change CSS = deploy static files = no restart |
| New LLM prompt = recompile = restart | Edit YAML file = auto-reload = no restart |
| Train CfC variant = edit Rust = recompile = test = repeat | Jupyter notebook = train = export ONNX = drop in |
| Designer needs Rust toolchain | Designer needs Node.js |
| Data scientist needs Rust toolchain | Data scientist needs Python |
| Everything deploys together | Components deploy independently |

---

## 6. The Dashboard Problem

### What exists

The live dashboard shows **40+ metrics simultaneously** on one screen. No
hierarchy. No progressive disclosure. No indication of what matters right now.

Specific problems:

- **CfC internals on screen:** "CfC Anomaly Score", "Regime ID", "Feature Surprises"
  are implementation details. Operators don't know what these mean.
- **Specialist votes exposed:** "MSE: 0.72, Hydraulic: 0.45, WellControl: 0.88"
  means nothing to the person making decisions.
- **No visual priority:** A critical well control alert has the same visual weight
  as an MSE efficiency card.
- **Units are inconsistent:** Depth in "ft" or "m"? Gas in "units" (of what?).
- **No tooltips, no help text, no legends.**
- **Baseline progress always visible**, even after baselines are locked.
- **Shift summary blank for the first hour.** New users: "Is it broken?"
- **Embedded in binary.** Any UI change = full recompile + restart.

### What should exist

The dashboard answers one question: **"Is there a problem right now?"**

Three states. Adaptive detail. Plain language.

```
NORMAL (green):
┌─────────────────────────────────────────────────┐
│                                                   │
│   All Clear                                       │
│                                                   │
│   Endeavour-7  ·  3,250 ft  ·  45 ft/hr         │
│                                                   │
│   ┌─── Key Metrics ───────────────────────────┐  │
│   │ MSE Efficiency   87%  ████████░░           │  │
│   │ Flow Balance    +2 GPM  (normal)           │  │
│   │ Pit Volume      stable                     │  │
│   └────────────────────────────────────────────┘  │
│                                                   │
│   Last alert: 2h ago (resolved)                   │
│   System: ✓ WITS  ✓ Baselines  ✓ Fleet           │
│                                                   │
└─────────────────────────────────────────────────┘

5-7 metrics. Clean. Glanceable. Driller looks up, sees green, goes back to work.
```

```
WARNING (amber):
┌─────────────────────────────────────────────────┐
│                                                   │
│   ⚠  Elevated Torque                             │
│   Torque increased 18% over the last 5 minutes   │
│                                                   │
│   Recommended action:                             │
│   Monitor for pack-off. Reduce WOB if torque      │
│   continues rising. Consider circulating.         │
│                                                   │
│   ┌─── Relevant Parameters ───────────────────┐  │
│   │ Torque      ▲ 22.4 kft-lb  (+18%)        │  │
│   │ SPP         ▲ 2,840 psi    (+6%)         │  │
│   │ ROP         ▼ 38 ft/hr     (-15%)        │  │
│   └────────────────────────────────────────────┘  │
│                                                   │
│   [Acknowledge]  [Show All Parameters]            │
│                                                   │
└─────────────────────────────────────────────────┘

Only the relevant parameters highlighted. Everything else collapsed.
Advisory in plain English. Specific actions. One button to acknowledge.
```

```
CRITICAL (red):
┌─────────────────────────────────────────────────┐
│                                                   │
│   FLOW IMBALANCE — Possible Kick                 │
│   Flow out exceeds flow in by 22 GPM             │
│                                                   │
│   IMMEDIATE ACTION:                               │
│   1. Check returns flow                           │
│   2. Verify pit levels                            │
│   3. Prepare to shut in if confirmed              │
│                                                   │
│   ┌─── Well Control ─────────────────────────┐   │
│   │ Flow Balance   +22 GPM  ████████████     │   │
│   │ Pit Volume     +4.2 bbl  ▲ rising        │   │
│   │ Gas            145 units  ▲ rising        │   │
│   │ Casing Press   stable                     │   │
│   └───────────────────────────────────────────┘   │
│                                                   │
│   [ACKNOWLEDGE — Action Taken]                    │
│                                                   │
└─────────────────────────────────────────────────┘

Everything except well control parameters is HIDDEN.
Full screen. High contrast. No distractions.
```

### Design principles

1. **Default: 5-7 metrics.** Details on demand.
2. **Severity drives layout.** Red = full screen takeover. Green = minimal.
3. **No implementation details.** No CfC scores. No specialist votes. No regime IDs.
4. **Plain English advisories.** Not raw LLM output. Structured: what happened,
   how bad, what to do.
5. **System health always visible.** Small banner: WITS / Baselines / LLM / Fleet.
6. **Tooltips on every metric.** Hover for "What is MSE Efficiency? Why does it
   matter?"
7. **React frontend.** Ships as static files. Iterates independently of the backend.

---

## 7. The API Problem

### What exists

41 REST endpoints (26 edge + 15 hub). No OpenAPI spec. Inconsistent response
shapes. Internal state in responses (trace logs, CfC scores, verification
status). Two auth schemes. Undocumented endpoints.

### What should exist

**4 endpoints for operators. 8 for integrators. Everything else is internal.**

#### Operator API (what 95% of consumers need)

```
GET  /api/status      → { state: "green"|"amber"|"red", well, depth, rop,
                          mse_efficiency, flow_balance, advisory }
GET  /api/advisory     → { severity, title, description, actions[], timestamp }
                          Returns 204 if no active advisory.
POST /api/advisory/ack → { action_taken: "string" }
GET  /api/health       → { status: "ok"|"degraded"|"down", components: {} }
```

That's it. Four endpoints. One GET for "what's happening", one GET for "is
there a problem", one POST for "I handled it", one GET for "is the system
working".

#### Integrator API (for SCADA, third-party dashboards, data export)

```
GET  /api/drilling     → Detailed drilling parameters (all WITS channels)
GET  /api/reports      → Advisory history with filtering
GET  /api/config       → Current configuration
PUT  /api/config       → Update configuration (validated)
GET  /api/metrics      → Prometheus metrics
GET  /api/shifts       → Shift summaries
WS   /api/stream       → WebSocket for real-time parameter streaming
```

#### Internal API (not for external consumption)

Everything else moves behind `/api/internal/`:
- Pipeline stats, CfC debug data, baseline internals, ML engine details,
  spectrum/FFT, time-to-failure predictions.

These are development/debugging tools. They should exist but not pollute the
public API surface.

**Ship an OpenAPI spec.** Generate client libraries automatically. Consistent
response shapes. Versioned.

---

## 8. Architecture Leaking Into UX

### The rule

**If the user didn't ask for it, they shouldn't see it.**

### What currently leaks

| Internal concept | Where it leaks | Fix |
|-----------------|----------------|-----|
| 10-phase pipeline | Pipeline stats in API: `tickets_created`, `tickets_verified` | Replace with: "Advisories: 7 generated, 5 acknowledged" |
| 4 specialist agents | `[ensemble_weights]` in config, votes in API/dashboard | Hard-code weights. Show severity, not votes. |
| CfC neural network | `anomaly_score`, `feature_surprises`, `motor_outputs`, `regime_id` in API responses and dashboard | Remove from all user-facing surfaces. Keep in `/api/internal/`. |
| Verification status | `VerificationResult` on advisory tickets | Don't show. If unverified, don't show the advisory at all. |
| Trace logs | `trace_log: Vec<...>` on advisory tickets | Remove from API response. Log internally. |
| Causal leads | `causal_leads: Vec<...>` on tickets | Translate to English: "Torque started rising 20 seconds before ROP dropped" |
| Knowledge base directory structure | 4+ env vars for KB setup | Auto-initialise from well name and field. One directory. |
| Baseline learning internals | "contaminated", "sigma", "locked_count" on dashboard | Show: "Learning: 45/100 samples" then "Monitoring active" |
| Campaign system | Config duplication, manual switching | Auto-detect operation type from WITS signatures |

### The advisory ticket today vs. what it should be

**Today (what the API returns):**

```json
{
  "ticket_type": "RiskWarning",
  "severity": "AMBER",
  "category": "WellControl",
  "verification_result": {
    "status": "Confirmed",
    "flow_balance_confirmed": true,
    "trend_consistency_r_squared": 0.73
  },
  "cfc_anomaly_score": 0.82,
  "cfc_feature_surprises": [
    { "feature": "flow_out", "sigma": 3.2 },
    { "feature": "pit_volume", "sigma": 2.8 }
  ],
  "causal_leads": [
    { "cause": "flow_out", "effect": "pit_volume", "lag_seconds": 12, "correlation": 0.89 }
  ],
  "trace_log": ["Phase 5: Advanced physics...", "Phase 6: KB lookup...", "..."],
  "ensemble_votes": {
    "mse": { "severity": 0.3, "confidence": 0.6 },
    "hydraulic": { "severity": 0.5, "confidence": 0.7 },
    "well_control": { "severity": 0.8, "confidence": 0.9 },
    "formation": { "severity": 0.2, "confidence": 0.5 }
  },
  "strategic_advisory": "The system has detected a potential flow imbalance...[500 words of LLM output]..."
}
```

**What it should be:**

```json
{
  "severity": "warning",
  "title": "Flow Imbalance Detected",
  "description": "Flow out exceeds flow in by 14 GPM. Pit volume rising.",
  "actions": [
    "Check returns flow at the shakers",
    "Verify pit levels manually",
    "Monitor for continued gain"
  ],
  "context": "Torque started rising 20 seconds before flow imbalance appeared",
  "timestamp": "2026-02-24T14:23:00Z",
  "acknowledged": false
}
```

Seven fields. Plain language. Actionable. No implementation details.

---

## 9. Silent Failures

### What fails silently today

| Failure | What happens | What should happen |
|---------|--------------|-------------------|
| Wrong config file path | Falls back to defaults. No warning. | Error on startup: "Config not found at /path. Using defaults." + dashboard banner. |
| Typo in config key | Silently ignored. | Warn: "Unknown key 'efficency_warning' — did you mean 'efficiency_warning_percent'?" |
| WITS server unreachable | Retries silently. Dashboard shows stale data. | Dashboard banner: "WITS disconnected. Last data: 45s ago. Retrying..." |
| LLM model file missing | Falls back to templates. No indication. | Dashboard banner: "Running in template mode (no LLM model found)" |
| CUDA unavailable | Falls to CPU (10-30s latency). Silent. | Dashboard banner: "LLM on CPU — advisories may be delayed" |
| `--wits-tcp` AND `--stdin` both set | `--stdin` silently ignored. | Error: "Cannot use both --wits-tcp and --stdin. Pick one." |
| `RESET_DB=true` left in env | **Wipes all data on every restart.** | Remove env var support. CLI only: `--reset-db --confirm` |
| `mud_weight_ppg` set wrong | D-exponent silently corrupted by up to 28%. | Auto-detect from WITS. If manual, validate range (7-18 ppg). |
| Hub passphrase lost | No recovery path. Printed to stdout only. | Persist to file. Or: use certificate auth (no passphrase). |
| Feature flag forgotten at build | Subcommands silently missing from --help. | Ship one binary. Runtime detection, not compile flags. |

### The principle

**Every failure should be visible on the dashboard.** If the operator has to SSH
into the box and tail logs to know something is wrong, the system has already
failed.

System status bar, always visible:

```
┌────────────────────────────────────────────────────────────┐
│ ✓ WITS  │  ✓ Baselines  │  ⚠ LLM (template mode)  │  ✓ Fleet │
└────────────────────────────────────────────────────────────┘
```

Every component reports its state. Green/amber/red. Always honest.

---

## 10. The 86KB README

### The problem

The README is 40 printed pages. It explains CfC neural network topology,
Granger-style causal inference, and regime-aware orchestrator weighting before
the user finds out how to start the program.

20+ concepts are introduced before "Quick Start":

1. WITS Level 0 protocol
2. 11-phase processing pipeline
3. Tactical vs Strategic agents
4. 4 specialist voting domains
5. CfC neural networks (12 paragraphs)
6. Causal inference
7. Regime-aware weighting
8. Campaign modes
9. Fleet hub-and-spoke architecture
10. Knowledge base directory structure

The glossary (90+ terms) is at the bottom. Users read 1,200 lines before finding
definitions of terms used on page 1.

### What should exist

**README.md — one screen:**

```markdown
# SAIREN-OS

Real-time drilling intelligence for your rig.
Detects anomalies. Generates advisories. Prevents failures.

## Try It

    sairen-os demo

Dashboard opens at http://localhost:8080.
Press K to simulate a kick. Watch the advisory appear.

## Install on a Rig

    curl -sSL https://get.sairen-os.com | bash

## Documentation

- [Quick Start Guide](docs/quickstart.md) — 5 minutes to running
- [Configuration](docs/configuration.md) — the 10 things you can change
- [Deployment](docs/deployment.md) — production install
- [API Reference](docs/api.md) — OpenAPI spec
- [Troubleshooting](docs/troubleshooting.md) — common issues
- [Architecture](docs/architecture.md) — for developers
```

Everything else lives in `docs/`. The README is a landing page, not a textbook.

---

## 11. The Build

### What exists today

Users must install the Rust toolchain, choose feature flags, and compile 41,000
lines of code:

```bash
cargo build --release --features llm,fleet-client,knowledge-base
# 5-10 minutes on a good machine
# Hope you didn't forget a flag
```

Build with wrong flags = silently missing functionality. Build without `llm` =
template-only advisories with no warning. Build without `fleet-client` = enroll
command doesn't exist.

### What should exist

**Pre-built binaries. One per platform. Everything included.**

```bash
# Linux x86_64 (most rigs)
curl -sSL https://get.sairen-os.com | bash

# Or Docker
docker run -d --network host ghcr.io/sairen-os/edge:latest

# Or manual download
wget https://releases.sairen-os.com/v3.1/sairen-os-linux-amd64
chmod +x sairen-os-linux-amd64
./sairen-os-linux-amd64
```

No Rust toolchain. No feature flags. No compilation. Runtime detection for
everything:

- LLM model present? Use it. Not present? Templates. Dashboard shows which mode.
- GPU available? Use it. Not available? CPU. Dashboard shows which mode.
- Hub on network? Connect. Not found? Run standalone. Dashboard shows status.
- PostgreSQL available? Hub mode. Not available? Edge mode.

One binary. It figures out what it can do based on what's available.

**Delete feature flags entirely.** Compile everything in. Gate on runtime
detection. The binary size increase is worth the elimination of an entire class
of deployment errors.

---

## Summary: The Gap

| What | Today | Target |
|------|-------|--------|
| **Install** | Rust toolchain + feature flags + compile | `curl \| bash` or `docker run` |
| **Configure** | 85+ TOML fields + 16 env vars + 3 files | 0 fields for demo. ~8 for production. 1 file. |
| **Connect to WITS** | `--wits-tcp host:port` | mDNS auto-discovery |
| **Connect to Fleet** | 6-step CLI enrollment + shared passphrase | Pairing code on dashboard |
| **Dashboard** | 40+ metrics, no hierarchy, implementation details | 5 metrics, severity-driven layout, plain English |
| **API** | 41 endpoints, no spec, internal state exposed | 4 core + 8 extended + OpenAPI spec |
| **Advisory format** | CfC scores + votes + trace logs + raw LLM text | Title + description + actions. Seven fields. |
| **Error handling** | Silent fallbacks, expert-level log messages | Dashboard banners, plain English, always honest |
| **Documentation** | 86KB README, 20+ concepts before Quick Start | 1-screen README, structured docs/ |
| **Frontend** | HTML embedded in Rust binary, recompile to change | React app, independent deploy, hot reload |
| **LLM** | Compiled into binary, recompile to change prompts | Python sidecar, YAML prompts, hot reload |
| **Neural network** | Trained in Rust, recompile to iterate | Train in Python, export ONNX, load at runtime |
| **Build time** | 5-10 minutes, wrong flags = silent breakage | Pre-built binary, 0 minutes, nothing to get wrong |

The system is technically impressive. The engineering is solid. But none of that
matters if the person on the rig floor can't plug it in and trust it in under
five minutes.

**Make the complexity invisible. That's the product.**
