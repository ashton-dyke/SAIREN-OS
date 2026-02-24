# SAIREN-OS Simplification Roadmap

Zero-friction, plug-and-play deployment. Seven phases delivered, one dropped.
Each phase was safe to ship independently. **Roadmap complete.**

## Dependency Graph

```
Phase 1 --> Phase 2 --> Phase 5 --> Phase 6 --> Phase 7
                                            \-> Phase 8
Phase 1 --> Phase 3 --> Phase 4
```

Phases 3/4 can run in parallel with 2/5. Phases 7/8 can run in parallel.

---

## Phase 1: Foundation (Config Validation + Test Harness)

**Status:** Complete

**Goal:** Build the safety net before changing anything that affects the pipeline.

**Delivers:**
- Config typo detection with "did you mean?" suggestions (Levenshtein)
- Physical range validation (impossible values block startup, suspicious values warn)
- Regression test suite: config validation, pipeline with Volve data, HTTP API
- This planning document

**Files:**
- `src/config/validation.rs` — NEW: typo detection, Levenshtein, range checks
- `src/config/well_config.rs` — Add range validation, call typo checker on load
- `src/config/mod.rs` — Add `pub mod validation`
- `src/main.rs` — Enhanced startup logging (config path, well identity, mud weight)
- `tests/config_validation_tests.rs` — NEW: config validation tests
- `tests/pipeline_regression.rs` — NEW: full pipeline regression with Volve
- `tests/api_regression.rs` — NEW: HTTP API endpoint regression
- `tests/csv_replay_integration.rs` — Extended with baseline + advisory assertions
- `SIMPLIFICATION.md` — NEW: this document

**Why first:** The codebase has minimal test coverage. Changing auto-detection,
API shapes, or dashboard architecture without a regression suite is reckless.

---

## Phase 2: Auto-Detection

**Status:** Complete
**Depends on:** Phase 1

**Goal:** Extend baseline learning to automatically set thresholds from WITS data,
eliminating the need for operators to manually configure most drilling parameters.

**Approach:**
- Use the first N minutes of data to auto-detect mud weight, typical flow rates,
  normal SPP, and other key parameters
- Calculate threshold values from statistical distributions of observed data
- Store auto-detected values alongside manual overrides (manual always wins)
- Log what was auto-detected vs manually configured

**Key decisions:**
- How many samples before auto-detection is confident
- Which parameters are safe to auto-detect vs must be manually set
- How to handle parameter drift (e.g., mud weight changes during trip)

---

## Phase 3: API Cleanup

**Status:** Complete
**Depends on:** Phase 1

**Goal:** Clean v2 API with consistent error envelope, consolidated live endpoint,
and v1 deprecation headers.

**Delivers:**
- `src/api/envelope.rs` — `ApiResponse<T>` / `ApiErrorResponse` uniform wrappers
- `src/api/v2_handlers.rs` — All v2 handlers (live, drilling, reports, ML, config, etc.)
- `src/api/v2_routes.rs` — v2 route table at `/api/v2`
- `src/api/middleware.rs` — v1 deprecation headers (Deprecation + Sunset)
- Consolidated `GET /api/v2/live` replaces 7 independent v1 polling intervals
- Debug endpoints under `/api/v2/debug/`
- Fixed SHA256/MD5 labeling bug in critical report signatures
- Gated test endpoint behind `#[cfg(debug_assertions)]`
- Wired `strategic_storage` + `ml_storage` into `DashboardState` (fixes always-None)

---

## Phase 4: Dashboard Overhaul

**Status:** Complete
**Depends on:** Phase 3

**Goal:** React + Vite + Tailwind dashboard compiled into the single binary via
`rust-embed`, with severity-driven layout and consolidated polling.

**Delivers:**
- `dashboard/` — React + TypeScript + Tailwind + Recharts app (~25 files)
- `build.rs` — Triggers `npm run build` during `cargo build`
- `rust-embed` serving with SPA fallback in `src/api/mod.rs`
- Severity-driven layout (alert banner expands on Critical/High)
- Time-series charts (flow balance, MSE vs baseline) with 120-point ring buffer
- Single `GET /api/v2/live` poll every 2s replaces 7 v1 intervals
- Dark industrial theme, mobile responsive (Tailwind breakpoints)
- Reports view: split-pane list+detail for critical reports

---

## Phase 5: Config Consolidation

**Status:** Complete
**Depends on:** Phase 2

**Goal:** Merge environment variables into TOML. Reduce operator-facing config
to ~10 essential fields. Everything else auto-detected or expert-only.

**Approach:**
- Identify which env vars (`SAIREN_SERVER_ADDR`, `SAIREN_CONFIG`, etc.) should
  become TOML fields
- Split config into "operator" section (~10 fields) and "expert" section
- Auto-populate expert fields from auto-detection (Phase 2)
- Generate well_config.toml template with only operator fields uncommented

---

## Phase 6: Feature Flags to Runtime Detection

**Status:** Complete
**Depends on:** Phase 5

**Goal:** Single binary that detects capabilities at runtime. Pre-built releases
with no compile-time feature selection needed.

**What was done:**
- `knowledge-base` feature fully de-gated — always compiled, self-initializes at
  runtime (returns `None` if no KB directory exists)
- `fleet-client` feature fully de-gated — always compiled, `FLEET_HUB_URL` env
  var gates activation at runtime (existing pattern in `spawn_fleet_tasks()`)
- `llm` made default feature — always compiled, runtime `is_cuda_available()`
  check gates actual GPU inference usage
- `zstd` and `reqwest` moved from optional to required dependencies
- ~80 `#[cfg]` annotations removed across 13 files

**Features staying compile-time:**
- `cuda` — links native CUDA libraries (cuDNN, mistralrs/cuda), must be compile-time
- `fleet-hub` — separate binary with heavy PostgreSQL deps (sqlx), hub operators
  explicitly build this
- `hub-intelligence` — composition of `fleet-hub` + `llm`, stays for hub builds

---

## Phase 7: Fleet Simplification — Setup Wizard + WITS Auto-Discovery + Pairing Codes

**Status:** Complete
**Depends on:** Phase 6

**Goal:** Zero-friction first-run setup via web wizard, automatic WITS stream
discovery, and passphrase-free pairing codes for fleet enrollment.

**What was done:**
- **WITS Subnet Scanner** (`src/acquisition/scanner.rs`) — Active /24 subnet scan
  for WITS Level 0 TCP streams on configurable port ranges (default: 5000-5010,
  10001-10010). Semaphore-limited to 64 concurrent probes, validates WITS frames
  by detecting `&&\r\n` header.
- **Setup Wizard** (`src/api/setup.rs` + `static/setup.html`) — Web-based setup
  wizard served on port 8080 via `sairen-os setup`. Three-section UI: WITS
  connection discovery, well identity, optional fleet pairing. Dark industrial
  theme, inline CSS/JS, no external dependencies, compiled into binary.
- **`sairen-os setup`** subcommand — Launches standalone setup wizard HTTP server.
  Supports `--ports` for custom scan ranges, `--addr` for bind address override.
- **`sairen-os pair`** subcommand — Headless CLI pairing with 6-digit code for
  rigs without browser access. Generates code, prints to terminal, polls hub
  until approved, writes env file.
- **Fleet Pairing Code Flow** (hub side: `src/hub/api/pairing.rs`) — Three new
  unauthenticated/admin endpoints: `POST /pair/request`, `POST /pair/approve`,
  `GET /pair/status`. In-memory `DashMap` store with 10-minute TTL and automatic
  cleanup. Approved pairings register the rig in the DB and return the fleet
  passphrase.
- **Fleet Dashboard** — "Pending Pairings" section added to hub dashboard with
  approve buttons, auto-refreshes every 5 seconds.
- **Deprecated `sairen-os enroll`** — Hidden from help, prints deprecation warning,
  remains functional for backward compatibility.

---

## Phase 8: LLM Sidecar

**Status:** Dropped
**Depends on:** Phase 6

**Original goal:** Decouple LLM inference from the main Rust binary via a Python
FastAPI sidecar with hot-reloadable prompts.

**Why dropped:** The template-based advisory system with physics verification and
CfC tiebreaking already handles edge node advisory generation well. Adding LLM
inference to edge nodes would introduce complexity (model files, memory pressure,
latency risk) for marginal benefit over deterministic templates. No user-facing
problem remains that LLM inference on edge nodes would solve.

**Note:** The existing LLM infrastructure code (`src/llm/`) is retained. It compiles
cleanly, has tests, and doesn't affect the runtime. If LLM integration is
reconsidered in the future, the prompt building and response parsing foundations
are ready.

---

## Guiding Principles

1. **Never break existing deployments.** Every phase must be backwards-compatible
   with existing configs and workflows.
2. **Warn, don't error.** Unknown config keys warn. Suspicious values warn.
   Only physically impossible values block startup.
3. **Test before you touch.** No pipeline changes without regression coverage.
4. **Ship phases independently.** Each phase is a self-contained PR that can be
   reviewed, merged, and deployed on its own.
5. **Auto-detect everything possible.** The ideal config file is empty. The system
   should work correctly with zero configuration for standard operations.
