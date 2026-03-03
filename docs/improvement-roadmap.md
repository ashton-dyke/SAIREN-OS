# SAIREN-OS Improvement Roadmap

Post-v5.0 assessment based on Volve field data replay results and codebase audit.

---

## Current State (v5.0)

Three Volve wells replayed through the full pipeline (sanitizer → physics → ACI → CfC → tactical → strategic):

| Well | Packets | Rejected | Tickets | Confirmed | Notes |
|------|---------|----------|---------|-----------|-------|
| F-5  | 181,617 | 3,588 (2.0%) | 269 | 97% | Excellent, but 258/269 are pit rate WellControl |
| F-9A | 87,876 | 11,723 (13.3%) | 9 | 56% | Clean well, low ticket count, appropriate restraint |
| F-12 | 2,423,467 | 763,586 (31.5%) | 16 | 81% | Sanitizer caught garbage; remaining tickets are real |

The system detects anomalies reliably. The gap is between "detects anomalies" and "helps a driller decide what to do."

---

## Phase 1: Data Quality Foundation

Problems that corrupt everything downstream — baseline learning, ACI intervals, CfC training. Must fix first.

### 1.1 Depth Continuity Validation

**Problem:** F-12 has depth values jumping from 0 → 10,000 ft between consecutive packets, and periods of negative depth. The sanitizer catches negative depth (Critical rejection) but has no concept of depth *sequence*. A packet at 10,000 ft followed by one at 0 ft followed by one at 10,001 ft all pass individually.

**Root cause:** `validate_packet_quality()` in `src/acquisition/wits_parser.rs` checks each packet in isolation — no memory of previous depth. The pipeline coordinator (`src/pipeline/coordinator.rs`) tracks formation transitions but not depth continuity.

**Impact:** Chaotic depth sequences corrupt the baseline (learns from mixed well sections), widen ACI intervals (noise in the training window), and confuse CfC (depth is a normalizer input).

**Fix:** Add a `DepthContinuityTracker` to the sanitizer or coordinator:
- Track previous packet's depth and timestamp
- Reject packets where `|depth_delta| > max_rop * time_delta` (configurable, default ~500 ft/hr)
- Reject packets where depth reverses by more than a threshold during drilling state (tripping/reaming can legitimately decrease depth)
- Log rejected packets with reason for replay diagnostics

**Files:**
- `src/acquisition/wits_parser.rs` — add stateful depth tracking to `sanitize_packet()` or create a new `DepthContinuityValidator` struct
- `src/pipeline/coordinator.rs` — wire validator into Phase 1.1
- `src/bin/volve_replay.rs` — wire validator into replay loop (same as sanitizer was wired in)

**Complexity:** ~80 LOC. Low risk — purely additive rejection filter.

---

### 1.2 Pit Rate Calculation Refinement

**Problem:** On F-5, 258 of 269 tickets (96%) are WellControl triggered by pit rate. The pit rate calculation in `src/physics_engine/mod.rs` (lines 101-126) converts raw pit volume deltas to an hourly rate, clamped to ±50 bbl/hr. With CSV data where timestamps are irregular:

- A real 8 bbl gain over 60 seconds → `8 / (60/3600) = 480 bbl/hr` → clamped to 50 → Critical ticket
- The ±50 clamp was designed for 1 Hz WITS feeds, not batch CSV replay
- The 5 bbl/hr warning threshold (`well_control.pit_rate_warning_bbl_hr`) means virtually any pit volume change triggers a ticket

**Root cause chain:**
1. Volve CSV "Tank Volume (Active)" is in m³, converted to barrels (`× 6.28981`)
2. Coordinator computes `pit_volume_change = current - previous` (line 307)
3. Physics engine divides by time delta in hours → rate in bbl/hr
4. Clamp to ±50 bbl/hr
5. Tactical agent triggers WellControl if `|pit_rate| > 5.0 bbl/hr`

WellControl tickets bypass both ACI gating (RULE 4, line 1252 in tactical.rs) and CfC gating (RULE 5, line 963) because they're safety-critical. This is correct behaviour — but means there's no backstop against noisy pit rate data.

**Fix options:**
- **Option A:** Add a minimum sustained-duration requirement for pit rate WellControl tickets — require N consecutive packets above threshold before triggering (e.g., 3 packets). Currently only checked for non-WellControl via `is_sustained` in the strategic agent.
- **Option B:** Scale the pit rate by time delta quality — if the time gap between packets is > 5 minutes, apply a confidence discount or require corroboration from flow balance.
- **Option C:** Add a pit rate rate-of-change filter — reject spikes where pit_rate jumps from 0 to ±50 in one packet (likely a CSV artifact, not a real kick/loss).

**Recommendation:** Option A is simplest and most impactful. A 3-packet sustained requirement still catches real kicks (which persist) but filters single-packet CSV noise.

**Files:**
- `src/agents/tactical.rs` — add sustained-check for WellControl pit rate tickets (near line 1069)
- `src/physics_engine/mod.rs` — consider adding time-quality weighting to pit rate (line 101-126)

**Complexity:** ~40 LOC for Option A. Low risk to real safety detection since genuine kicks/losses persist across many packets.

---

## Phase 2: Signal Quality Tuning

With clean data going in, tune the detection/verification pipeline to produce higher-quality signals.

### 2.1 CfC Gate Calibration

**Problem (corrected from initial assessment):** CfC is NOT purely shadow mode. It actively gates non-safety tickets via RULE 5 (`src/agents/tactical.rs` line 959-975, threshold `anomaly_score >= 0.3`) and modulates severity (lines 1197-1243, thresholds 0.3/0.7). It also acts as a tiebreaker in the strategic agent (lines 737-772 in `src/agents/strategic.rs`).

However, the current settings are very conservative:
- 0.3 corroboration threshold lets most tickets through (CfC fast network averages 0.3-0.5 on anomalous packets)
- Severity modulation is ±1 level maximum
- Strategic tiebreak only fires on Uncertain results (narrow window)
- 500-packet calibration warm-up means ~5 hours of no CfC influence on a 1 Hz feed

**Current CfC integration points:**

| Point | Location | Threshold | Effect |
|-------|----------|-----------|--------|
| RULE 5 veto | tactical.rs:959-975 | score < 0.3 → veto | Blocks low-confidence non-safety tickets |
| Severity down | tactical.rs:1197-1215 | score < 0.3 | Downgrades severity by 1 level |
| Severity up | tactical.rs:1222-1243 | score >= 0.7 | Upgrades severity by 1 level |
| Strategic tiebreak | strategic.rs:737-772 | 0.2 / 0.7 | Confirms or rejects Uncertain results |
| WellControl bypass | tactical.rs:963 | — | WellControl always bypasses CfC gate |

**Tuning plan:**
1. **Lower calibration warm-up** from 500 → 300 packets (fast network converges early, avg loss stabilizes by ~200 packets on all three Volve wells)
2. **Raise corroboration threshold** from 0.3 → 0.4 (more selective — CfC must be more confident to let tickets through)
3. **Widen severity modulation** — use 0.4/0.6 thresholds instead of 0.3/0.7, and allow ±2 levels for scores at extremes (< 0.2 or > 0.8)
4. **Expand strategic tiebreak band** — use 0.35/0.65 instead of 0.2/0.7 so CfC resolves more Uncertain cases

**Do NOT change:** WellControl bypass. Safety-critical detection must never depend on a learning system.

**Files:**
- `src/agents/tactical.rs` — adjust thresholds at lines 959, 1197, 1222
- `src/agents/strategic.rs` — adjust tiebreak at lines 737-772
- `src/cfc/network.rs` — reduce FAST_CALIBRATION_PACKETS from 500 to 300 (line 52)

**Complexity:** ~20 LOC of threshold changes. Medium risk — requires re-running Volve replays to validate that confirmed% doesn't drop. The slow network's high final anomaly on F-9A (0.92) suggests it may not converge on shorter wells — monitor this.

---

### 2.2 ACI Interval Tightening After Baseline Lock

**Problem:** ACI intervals are wide early in the well because they're built during baseline learning when drilling parameters vary most. Once baseline locks, the intervals don't retroactively tighten. On F-9A, the MSE interval is [418.7 — 676.0] psi which is quite wide.

**Current behaviour:** ACI uses a sliding window of 200 samples with γ=0.005 adaptive conformal parameter. The window is FIFO — old samples drop off naturally. But the early noisy samples take 200+ drilling packets to fully flush.

**Fix:** After baseline locks, reset the ACI window (or shrink it) to force faster convergence on the now-stable drilling parameters. This is a one-time operation — the `ThresholdManager` already announces when baseline locks.

**Files:**
- `src/aci.rs` — add a `reset_window()` or `shrink_window()` method
- `src/agents/tactical.rs` — call it when baseline transitions from learning to locked

**Complexity:** ~30 LOC. Low risk — worst case is a brief period of over-sensitivity right after baseline lock, which self-corrects within 200 packets.

---

## Phase 3: Actionable Output

The system detects problems. Now make it tell the driller what to do about them.

### 3.1 Wire Template Advisories Into Production Pipeline

**Problem (corrected from initial assessment):** The template advisory system in `src/strategic/templates.rs` is already comprehensive — it covers all 6 anomaly categories with:
- Specific WOB/RPM parameter adjustments
- Expected ROP/efficiency improvement estimates
- Formation hardness and depth context
- Stick-slip damping with oscillation analysis
- Causal lead integration (parameter X rising precedes MSE spike by Ns)
- Campaign-aware notes (P&A mode tighter tolerances)

However, this system was designed as an LLM fallback (tagged `source: "template"`, confidence 0.70). With the LLM removed in v5.0, templates ARE the advisory system — but they may not be consistently wired into all code paths.

**Fix:** Audit all paths from strategic verification → advisory output and ensure template advisories are generated for every confirmed ticket. Currently the `AdvisoryComposer` (`src/strategic/advisory.rs`) expects recommendation/reasoning text to be passed in — verify that `template_advisory()` is called on every confirmed ticket path.

**Files:**
- `src/strategic/mod.rs` — verify template_advisory is called after verify_ticket confirms
- `src/strategic/advisory.rs` — no changes expected, just audit
- `src/bin/volve_replay.rs` — add template advisory output to replay log for confirmed tickets (currently only shows verification reasoning, not the actionable recommendation)

**Complexity:** ~50 LOC for replay wiring, audit-only for production path. Low risk.

---

### 3.2 Formation-Aware Context Enrichment

**Problem:** The system has formation infrastructure but doesn't use it for context enrichment:
- `FormationTop` config struct exists (`src/config/well_config.rs` lines 1598-1610)
- `formation_tops: Vec<FormationTop>` field on WellConfig (line 96)
- Formation transition detection exists in coordinator (lines 318-346)
- `current_formation()` lookup exists in gossip store (lines 275-286)
- Formation hardness is included in template advisories

But none of this enriches ticket context. A ticket at 3,000 ft in shale should have different severity and recommendations than the same ticket at 3,000 ft in limestone. The strategic agent doesn't read formation data at all.

**Fix:**
1. Pass current formation context into the strategic agent's `verify_ticket()` — add formation name, hardness, depth-into-formation as parameters
2. Use formation context in severity modulation — e.g., mechanical tickets near formation boundaries are more expected (Warning not High)
3. Include formation in template advisory text — "Entering limestone at 3,200 ft — expect higher torque"
4. Tag gossip events with formation for cross-well learning

**Files:**
- `src/agents/strategic.rs` — extend `verify_ticket()` to accept formation context
- `src/strategic/templates.rs` — already includes formation_hardness, add formation name
- `src/pipeline/coordinator.rs` — pass formation context through to agents

**Complexity:** ~100 LOC. Low-medium risk — requires formation_tops to be populated in config, which means well-specific TOML files.

---

## Phase 4: Operational Maturity

Features that make the system useful across wells and over time, not just on a single replay.

### 4.1 Outcome Feedback Loop

**Problem:** The gossip event store has `outcome` and `outcome_notes` fields, and `last_modified` sync cursor for propagating outcome updates to peers. But nothing writes outcomes. An operator can't mark a ticket as true positive / false positive, and that feedback doesn't improve future detection.

**Fix:**
1. Add `PATCH /api/mesh/events/{id}/outcome` endpoint — accepts `{ outcome: "true_positive" | "false_positive" | "inconclusive", notes: "..." }`
2. Update `last_modified` timestamp so the outcome propagates via gossip
3. Track per-category false positive rates in a rolling window
4. Display FP rate on dashboard per category — if Hydraulics FP rate is > 30%, flag for threshold review

**Future extension:** Use accumulated outcome data to auto-tune thresholds (requires significant data — hundreds of labelled outcomes per category).

**Files:**
- `src/gossip/server.rs` — add PATCH endpoint
- `src/gossip/store.rs` — add `update_outcome()` method
- `src/api/v2_handlers.rs` or new mesh handler — wire endpoint

**Complexity:** ~80 LOC. Low risk — purely additive, no changes to detection logic.

---

### 4.2 Volve Replay as Regression Suite

**Problem:** The Volve replays are currently ad-hoc (`cargo run --bin volve-replay`). As we tune thresholds and add features, we need to know if F-5's 97% confirmed rate degrades or F-12's rejection count changes.

**Fix:**
1. Add a `--json-summary` flag to volve-replay that outputs machine-readable results
2. Create a `tests/volve_regression.rs` integration test that runs all three wells and asserts:
   - F-5: confirmed% >= 95%, rejected% <= 3%
   - F-9A: tickets <= 15, confirmed% >= 50%
   - F-12: sanitizer_rejected% >= 25%, confirmed% >= 75%
3. Run as part of CI (slow test, gated behind `#[ignore]` or a feature flag)

**Files:**
- `src/bin/volve_replay.rs` — add `--json-summary` output mode
- `tests/volve_regression.rs` — new regression test file

**Complexity:** ~150 LOC. Zero risk to production — test-only code.

---

## Phase Summary

| Phase | Focus | Key Metric | Risk |
|-------|-------|-----------|------|
| **1.1** Depth continuity | Reject F-12 depth chaos | F-12 drilling packets should be contiguous | Low |
| **1.2** Pit rate refinement | Reduce F-5 WellControl flood | F-5 WellControl tickets < 50 (from 258) | Low |
| **2.1** CfC gate tuning | Stronger neural network influence | Confirmed% stable or improved across all wells | Medium |
| **2.2** ACI post-baseline reset | Tighter intervals after lock | Narrower final intervals, faster anomaly detection | Low |
| **3.1** Template advisory wiring | Actionable output for every ticket | Every confirmed ticket has recommendation text | Low |
| **3.2** Formation context | Geology-aware detection | Formation name in ticket context and advisory text | Low-Medium |
| **4.1** Outcome feedback | Operator closes the loop | PATCH endpoint functional, outcomes propagate via gossip | Low |
| **4.2** Volve regression suite | Prevent regressions during tuning | Automated pass/fail on all three wells | Zero |

---

## Recommended Execution Order

```
Phase 1.2 → 1.1 → 2.2 → 2.1 → 3.1 → 3.2 → 4.2 → 4.1
```

**Rationale:** Start with 1.2 (pit rate) because it's the highest-impact lowest-risk fix — F-5 goes from 258 WellControl noise tickets to a realistic count. Then 1.1 (depth) solidifies the data foundation. Phase 2 tuning should happen on clean data. Phase 3 delivers user value. Phase 4 builds long-term infrastructure.

Each step should be followed by a full Volve replay across all three wells to validate no regressions.
