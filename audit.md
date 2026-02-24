# SAIREN-OS Codebase Audit

Comprehensive audit of performance, safety, stability, security, and code quality.
Conducted 2026-02-24 against the `main` branch (post-Phase 7 completion).

---

## Summary

| Category | Critical | High | Medium | Low |
|----------|----------|------|--------|-----|
| Performance | 0 | 4 | 5 | 4 |
| Safety | 0 | 3 | 2 | 3 |
| Stability | 0 | 2 | 8 | 3 |
| Security | 0 | 1 | 4 | 3 |
| Code Quality | 0 | 1 | 3 | 5 |
| **Total** | **0** | **11** | **22** | **18** |

No critical (system-down) issues. Eleven high-severity findings that should be
addressed before a production deployment at scale.

---

## 1. Performance

### P-1 · HIGH — Excessive allocations on the hot path

**Files:** `src/pipeline/coordinator.rs` lines 240, 305, 325, 622–625

Every incoming packet triggers multiple `.iter().cloned().collect::<Vec<_>>()` over
the 60-entry history buffer. At 1 Hz that's 60+ heap allocations per second per
coordinator instance.

```rust
// Line 240 — happens every packet
let physics: Vec<HistoryEntry> = self.history_buffer.iter().cloned().collect();
```

**Fix:** Collect the history snapshot once per packet and pass `&[HistoryEntry]`
into each consumer. Better yet, make trend functions accept `impl Iterator` so
no intermediate Vec is needed.

---

### P-2 · HIGH — Redundant Vec creation in trend calculations

**File:** `src/pipeline/coordinator.rs` lines 632–672, 756–810

Five-element `Vec`s are created just to compute an average:

```rust
let recent: Vec<f64> = mse_values.iter().rev().take(5).copied().collect();
let earlier: Vec<f64> = mse_values.iter().take(5).copied().collect();
```

This pattern repeats for MSE, DXC, flow balance, and pit rates.

**Fix:** Sum directly with iterator adapters — no allocation needed:

```rust
let recent_avg: f64 = mse_values.iter().rev().take(5).sum::<f64>() / 5.0;
```

---

### P-3 · HIGH — String allocation per baseline sample

**File:** `src/baseline/mod.rs` line 293

```rust
pub fn composite_id(&self) -> String {
    format!("{}:{}", self.equipment_id, self.sensor_id)
}
```

Called 12× per packet during baseline learning (once per WITS metric), each time
allocating a new `String` for a HashMap lookup.

**Fix:** Pre-compute and cache the composite ID as a struct field, or use a
`(Cow<str>, Cow<str>)` tuple key to avoid formatting entirely.

---

### P-4 · HIGH — Full table scans in ML storage

**File:** `src/ml_engine/storage.rs` lines 104–122, 138–154

`get_latest()` and `get_well_history()` iterate every key in the Sled database and
do a string-contains check per entry:

```rust
for result in self.db.iter() {
    let (key, value) = result?;
    if key_str.contains(&format!("/{}/", well_id)) { ... }
}
```

**Fix:** Use `self.db.scan_prefix(well_id.as_bytes())` — Sled supports prefix
scans natively and this turns O(n) into O(k) where k = matching entries.

---

### P-5 · MEDIUM — Sparse bin allocation in optimal finder

**File:** `src/ml_engine/optimal_finder.rs` lines 126–134

Allocates 400 `ParameterBin` structs (each with an empty `Vec`) on every analysis
run. Most bins are never populated.

**Fix:** Use `HashMap<usize, ParameterBin>` for sparse binning.

---

### P-6 · MEDIUM — Vec allocation per median call in auto-detection

**File:** `src/config/auto_detect.rs` line 125

```rust
let mut sorted = values.to_vec();
sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
```

Every mud-weight sample triggers a full clone + sort.

**Fix:** Use an incremental median algorithm (e.g., two-heap approach), or at
minimum reuse the allocation across calls.

---

### P-7 · MEDIUM — Prometheus metrics endpoint uses repeated `format!()`

**File:** `src/api/handlers.rs` lines 1796–1832

Builds response line-by-line with `push_str` + `format!()`.

**Fix:** Use `write!(&mut body, ...)` to avoid intermediate `String` allocations.

---

### P-8 · MEDIUM — `tokio` and `hyper` use `features = ["full"]`

**File:** `Cargo.toml` lines 15, 21

Pulls in every sub-feature including unused ones (e.g., `signal`, `process`).

**Fix:** Replace with explicit feature lists to reduce compile time and binary
size.

---

### P-9 · MEDIUM — Blocking zstd compression in async context

**File:** `src/knowledge_base/fleet_bridge.rs` lines 59–61

```rust
let compressed = zstd::encode_all(json.as_slice(), 3)?;
```

CPU-bound compression on a tokio worker thread. For large payloads this blocks
the executor.

**Fix:** Wrap in `tokio::task::spawn_blocking()`.

---

### P-10 · LOW — Duplicated trend calculation logic

**File:** `src/pipeline/coordinator.rs` lines 622–672 vs 751–810

`run_advanced_physics()` and `compute_physics_for_optimizer()` compute identical
MSE/DXC/flow-balance trends independently.

**Fix:** Extract into a shared `compute_trend_components(&[HistoryEntry])`.

---

### P-11 · LOW — Excessive cloning in fleet sync

**File:** `src/fleet/sync.rs` lines 45, 119–120

Clones every episode and every intelligence output ID during dedup.

**Fix:** Use references or `Cow` where possible.

---

### P-12 · LOW — DrillingMetrics cloned every packet

**File:** `src/pipeline/coordinator.rs` lines 224, 227

```rust
self.latest_metrics = Some(metrics.clone());
self.update_history_buffer(history_entry.clone());
```

**Fix:** If the struct is only read after this point, move instead of clone.

---

### P-13 · LOW — LLM scheduler scans full queue to check for tactical

**File:** `src/llm/scheduler.rs` lines 330–333

```rust
let has_tactical = self.queue.iter().any(|p| p.0.priority == Priority::Tactical);
```

O(n) scan on every strategic request selection.

**Fix:** Maintain a `tactical_queued: usize` counter, increment/decrement on
push/pop.

---

## 2. Safety

### S-1 · HIGH — No NaN/Infinity guard on physics outputs

**File:** `src/physics_engine/drilling_models.rs` lines 41–51

MSE calculation can produce `Infinity` if a sensor returns extreme values
(e.g., `wob = 1e10`). The result propagates into baselines, advisories, and the
dashboard without any check.

```rust
let axial_component = (4.0 * wob * 1000.0) / (std::f64::consts::PI * d_squared);
// If wob = 1e10, result = 1.27e13 — technically finite but meaningless
// If bit_diameter is subnormal, d_squared → 0, result → Infinity
```

**Fix:** Add a `is_finite()` guard before returning any computed metric:

```rust
let result = rotary_component + axial_component;
if !result.is_finite() {
    warn!(wob, torque, rpm, rop, "MSE calculation produced non-finite value");
    return 0.0;
}
```

---

### S-2 · HIGH — NaN/Infinity in metrics corrupts LLM prompts

**File:** `src/llm/tactical_llm.rs` lines 89–117

Metrics are formatted directly into the LLM prompt. If any value is NaN or
Infinity, the prompt contains literal `"NaN"` / `"inf"` strings, which the model
cannot interpret correctly.

**Fix:** Validate all metric fields with `is_finite()` before prompt construction.

---

### S-3 · HIGH — Unchecked division in baseline coefficient of variation

**File:** `src/baseline/mod.rs` lines 898–906

```rust
let cv = t.effective_std() / t.baseline_mean;
```

If `baseline_mean` is very small (but positive), `cv` approaches infinity. No
upper bound check.

**Fix:** Clamp: `let cv = (t.effective_std() / t.baseline_mean).min(10.0);`

---

### S-4 · MEDIUM — Periodic summary fires on first packet

**File:** `src/pipeline/coordinator.rs` line 266

```rust
let time_since_last_summary = packet.timestamp.saturating_sub(self.last_periodic_summary_time);
```

`last_periodic_summary_time` initializes to 0. On the first packet,
`time_since_last_summary` equals the Unix timestamp (~1.7 billion), which
immediately exceeds `PERIODIC_SUMMARY_INTERVAL_SECS`.

**Fix:** Initialize `last_periodic_summary_time` to the first packet's timestamp.

---

### S-5 · MEDIUM — `expect("just inserted")` in baseline code

**File:** `src/baseline/mod.rs` (threshold insert path)

```rust
self.thresholds.insert(composite_id.clone(), thresholds);
Ok(self.thresholds.get(&composite_id).expect("just inserted"))
```

Logically safe but a panic path reachable from production code. If a custom
`Hash` impl or allocator ever changes behavior, this crashes.

**Fix:** Return an error or use `Entry` API:

```rust
Ok(self.thresholds.entry(composite_id).or_insert(thresholds))
```

---

### S-6 · LOW — Variance calculation doesn't validate output

**File:** `src/physics_engine/drilling_models.rs` lines 385–389

```rust
let variance = torque_values.iter().map(|t| (t - mean).powi(2)).sum::<f64>()
    / torque_values.len() as f64;
```

If all values are identical, variance = 0.0 (fine). But if values contain NaN,
result is NaN without warning.

---

### S-7 · LOW — Stringly-typed metric IDs

**File:** `src/baseline/mod.rs` lines 1003–1028

```rust
pub const MSE: &str = "mse";
pub const D_EXPONENT: &str = "d_exponent";
```

Typos in string constants compile silently. Used in 12+ locations.

**Fix:** Use an enum with `Display` impl for type safety.

---

### S-8 · LOW — LLM inference stats could overflow (theoretical)

**File:** `src/llm/tactical_llm.rs` lines 128–137

`inference_count` is `u64` — practically safe (58 billion years at 10 Hz), but
`saturating_add` costs nothing and is more defensive.

---

## 3. Stability

### ST-1 · HIGH — No timeout on LLM inference

**File:** `src/llm/scheduler.rs` lines 368–370

```rust
let result = self.generate_with_cleanup(model, &request.prompt, ...).await;
```

If the LLM backend hangs (e.g., GPU driver freeze), the entire scheduler blocks
with no recovery path.

**Fix:** Wrap in `tokio::time::timeout(Duration::from_secs(300), ...)`.

---

### ST-2 · HIGH — Blocking file I/O while holding RwLock write guard

**File:** `src/agents/tactical.rs` lines 691–730

```rust
let mut mgr = manager.write()?;
// ... compute overrides ...
mgr.save_to_file(Path::new(DEFAULT_STATE_PATH))?;
// ← Blocks all readers while writing to disk
```

**Fix:** Clone the data, drop the lock, then save:

```rust
let data_to_save = mgr.serialize_state();
drop(mgr);
ThresholdManager::write_state_file(DEFAULT_STATE_PATH, &data_to_save)?;
```

---

### ST-3 · MEDIUM — No exponential backoff in fleet sync loops

**File:** `src/fleet/sync.rs` lines 34–61

On repeated failures the sync loop retries every interval with no backoff. If the
hub is down, this hammers the network continuously.

**Fix:** Add exponential backoff with jitter (e.g., 1s → 2s → 4s → ... → 5min cap).

---

### ST-4 · MEDIUM — Unbounded growth in `AppState.acknowledgments`

**File:** `src/pipeline/state.rs` line 91

```rust
pub acknowledgments: Vec<AcknowledgmentRecord>,
```

No maximum size. On a long-running rig this grows without bound.

**Fix:** Use a `VecDeque` with a cap (e.g., 1000), or periodically drain old
entries.

---

### ST-5 · MEDIUM — Unbounded accumulation in baseline learning

**File:** `src/baseline/mod.rs`

Welford's algorithm accumulators grow the sample count indefinitely. Over a month
at 1 Hz, that's ~2.6M samples per metric. While Welford's is numerically stable,
the count field could theoretically overflow `u64` after ~584 billion years — the
real concern is that baselines computed over weeks of data become insensitive to
recent changes.

**Fix:** Consider a sliding window or exponential decay for long-running wells.

---

### ST-6 · MEDIUM — Intelligence cache unbounded before truncation

**File:** `src/fleet/sync.rs` lines 108–129

New outputs are appended to the full cached list before `truncate()` is called.
A large batch from the hub could cause a memory spike.

**Fix:** Truncate before merging, or stream-parse the cache file.

---

### ST-7 · MEDIUM — Silent failure on poisoned RwLock

**File:** `src/agents/tactical.rs` lines 668–671

```rust
let mut mgr = match manager.write() {
    Ok(m) => m,
    Err(_) => return,  // Silent — baseline learning just stops
};
```

If a panic ever poisons the lock, baseline learning silently halts with no
indication to the operator.

**Fix:** Log an error-level message and set a health flag.

---

### ST-8 · MEDIUM — KB watcher task has no cancellation mechanism

**File:** `src/pipeline/coordinator.rs` lines 940–942

`start_kb_watcher()` spawns a background task with no `CancellationToken` or
`JoinHandle` management. If the KB directory becomes unresponsive (e.g., NFS
stall), the task hangs forever.

**Fix:** Accept a `CancellationToken` and select on it in the watch loop.

---

### ST-9 · MEDIUM — Setup wizard scan can stack up

**File:** `src/api/setup.rs` lines 163–177

Each scan request spawns up to 2540 tokio tasks (254 IPs × 10 ports). Rapid
clicking spawns duplicate scans with no dedup.

**Fix:** Cancel the previous scan before starting a new one, or reject concurrent
scans.

---

### ST-10 · MEDIUM — `build.rs` doesn't fail on frontend build errors

**File:** `build.rs` line 41

If `npm run build` fails, a warning is printed but `cargo build` succeeds. The
binary ships without a dashboard and falls back to the SPA 404 handler silently.

**Fix:** Return a non-zero exit from the build script, or at minimum emit
`cargo:warning=DASHBOARD BUILD FAILED`.

---

### ST-11 · LOW — `FleetClient::new()` panics on HTTP client build failure

**File:** `src/fleet/client.rs` lines 47–51

```rust
.build().expect("Failed to build HTTP client")
```

**Fix:** Return `Result<Self, FleetClientError>`.

---

### ST-12 · LOW — Missing validation for positive-only config fields

**File:** `src/config/validation.rs`

Only `min_rop_for_mse` and H2S fields are checked for `< 0`. Other fields that
must be positive (torque increase thresholds, flow imbalance GPM, pit gain BBL)
are not validated.

---

### ST-13 · LOW — `std_floor` default is very small

**File:** `src/config/well_config.rs` line 1100

```rust
fn default_bl_std_floor() -> f64 { 0.001 }
```

0.001 may cause numerical instability when used as a divisor in threshold
calculations. Consider 0.01 or document the rationale.

---

## 4. Security

### SEC-1 · HIGH — TOML injection in setup wizard config generation

**File:** `src/api/setup.rs` lines 283–288

User-supplied well name, field, and rig ID are interpolated into TOML without
escaping:

```rust
let toml_content = format!(
    r#"[well]
name = "{well_name}"
field = "{field}"
rig = "{rig_id}""#
);
```

A crafted input like `test"\n[malicious]\nkey = "value` would inject arbitrary
TOML keys.

**Fix:** Use `toml::to_string()` to serialize a struct, or escape quotes and
newlines in user input.

---

### SEC-2 · MEDIUM — Pairing code brute-forceable

**File:** `src/hub/api/pairing.rs` lines 112–121, 220–257

The 6-digit code space is 1M possibilities. The `/pair/status` endpoint is
unauthenticated. Even with the hub's 20 req/s rate limit, the entire space can be
exhausted in ~14 hours. With parallel connections, much faster.

**Fix:** Add per-code attempt tracking with exponential backoff, or increase code
entropy (e.g., 8 alphanumeric characters = 2.8 trillion possibilities).

---

### SEC-3 · MEDIUM — Fleet passphrase returned over HTTP

**File:** `src/hub/api/pairing.rs` lines 239–244

The shared fleet passphrase is returned in a plain HTTP response to whoever polls
with a valid 6-digit code. If the hub is not behind TLS, the passphrase is
exposed to network observers.

**Fix:** Enforce HTTPS at the pairing endpoints, or switch to per-rig tokens
instead of the shared passphrase.

---

### SEC-4 · MEDIUM — CORS is fully permissive

**Files:** `src/api/mod.rs` line 61, `src/hub/api/mod.rs` line 83

```rust
let cors = CorsLayer::permissive();
```

Allows any origin to make authenticated requests. If the API is reachable from
the internet (even briefly), any website can issue requests on behalf of the
operator.

**Fix:** Restrict to known origins, or at minimum restrict to the same host.

---

### SEC-5 · MEDIUM — Passphrase stored in plaintext `.env` file

**File:** `src/api/setup.rs` lines 300–316

The setup wizard writes `FLEET_PASSPHRASE=...` to an `.env` file in plaintext.

**Fix:** Ensure `.env` is created with `0600` permissions. Document that it must
not be committed or shared.

---

### SEC-6 · LOW — Empty `api_key_hash` in rig DB registration

**File:** `src/hub/api/pairing.rs` lines 192–201

```rust
.bind("")  // api_key_hash is empty
```

If any future code checks this hash for auth, it will match against an empty
string.

**Fix:** Generate a random token hash at pairing time.

---

### SEC-7 · LOW — No explicit request body size limit

**File:** `src/api/mod.rs`

Axum defaults to 2 MB, which is reasonable, but it's undocumented and not
explicitly configured. A config-update endpoint accepting arbitrary JSON could be
abused.

**Fix:** Add `DefaultBodyLimit::max(2 * 1024 * 1024)` explicitly.

---

### SEC-8 · LOW — Naive URL encoding in fleet bridge

**File:** `src/knowledge_base/fleet_bridge.rs` lines 159–161

```rust
fn urlencoding_field(s: &str) -> String {
    s.replace(' ', "%20")
}
```

Only encodes spaces. Characters like `&`, `?`, `=`, `#` pass through and could
break or manipulate query strings.

**Fix:** Use the `urlencoding` crate or `percent_encoding::utf8_percent_encode()`.

---

## 5. Code Quality

### CQ-1 · HIGH — `api/handlers.rs` is 1,930 lines

**File:** `src/api/handlers.rs`

This file mixes v1 handlers, Prometheus metrics, acknowledgment logic, fleet
intelligence endpoints, and critical report formatting. Hard to navigate and test
in isolation.

**Fix:** Split into sub-modules: `handlers/v1.rs`, `handlers/metrics.rs`,
`handlers/fleet.rs`, etc.

---

### CQ-2 · MEDIUM — `.gitignore` is missing several patterns

**File:** `.gitignore`

Missing entries for:
- `.env.local`, `.env.*.local` (development overrides)
- `.obsidian/` (already untracked, visible in `git status`)
- `*.pem`, `*.key` (TLS credentials)
- `credentials.json`, `secrets.json`

---

### CQ-3 · MEDIUM — `api_regression.rs` tests are `#[ignore]`d

**File:** `tests/api_regression.rs`

These tests require a running binary on port 18080 and are permanently skipped in
CI. Effectively dead test coverage.

**Fix:** Use `axum::test::TestClient` or `tower::ServiceExt` for in-process API
testing.

---

### CQ-4 · MEDIUM — Wrong error variant mapping in fleet bridge

**File:** `src/knowledge_base/fleet_bridge.rs` line 39

```rust
.map_err(|e| FleetClientError::Compression(format!("IO error listing files: {}", e)))?;
```

Maps an IO error to the `Compression` variant. Semantically incorrect.

**Fix:** Add an `Io` variant to `FleetClientError`.

---

### CQ-5 · LOW — Acknowledgment endpoint is not idempotent

**File:** `src/api/handlers.rs` lines 1623–1668

Retrying a POST creates duplicate acknowledgment records.

**Fix:** Dedup on `(ticket_timestamp, acknowledged_by)`.

---

### CQ-6 · LOW — Inconsistent error response format between v1 and v2

**File:** `src/api/handlers.rs`

v1 endpoints return `200 + []` on error; v2 uses proper `ApiErrorResponse`
envelope. Clients must handle both patterns.

---

### CQ-7 · LOW — Hardcoded latency thresholds in tactical LLM

**File:** `src/llm/tactical_llm.rs` lines 149–157

```rust
let target_ms: u128 = if self.backend.uses_gpu() { 60 } else { 5000 };
```

Should be configurable, not hardcoded.

---

### CQ-8 · LOW — `#[allow(dead_code)]` on entire ML storage impl

**File:** `src/ml_engine/storage.rs` line 53

Blanket suppression hides actually-unused methods. Should be narrowed to specific
items.

---

### CQ-9 · LOW — No test coverage for concurrent access patterns

**Files:** `tests/`

No tests for OnceLock double-init, parallel TacticalAgent instances, DashMap
contention in pairing store, or RwLock poisoning recovery.

---

## 6. Testing Gaps

The test suite (~297 unit tests + 7 integration test files) covers the happy path
well but has notable gaps:

| Gap | Risk |
|-----|------|
| No tests for malformed WITS packets | Bad sensor data crashes pipeline |
| No tests for out-of-order timestamps | Baseline learning could produce nonsense |
| No tests for network failure in fleet sync | Silent data loss |
| No tests for API auth failures | Security regression undetected |
| No tests for config with extreme boundary values | Validation gaps slip through |
| No concurrency/race-condition tests | Intermittent production failures |
| `api_regression.rs` always skipped | 11 API endpoints untested in CI |

---

## Recommended Fix Order

### Batch 1 — Safety & correctness (highest value per line changed)

1. **S-1** Add `is_finite()` guards to physics engine outputs
2. **S-3** Clamp baseline coefficient of variation
3. **S-4** Initialize `last_periodic_summary_time` to first packet timestamp
4. **SEC-1** Fix TOML injection in setup wizard
5. **ST-2** Release RwLock before blocking file I/O

### Batch 2 — Hot-path performance

6. **P-1** Single history snapshot per packet
7. **P-2** Iterator-based trend calculations (no intermediate Vecs)
8. **P-3** Cached composite ID for baseline lookups
9. **P-4** Prefix scan in ML storage

### Batch 3 — Stability hardening

10. **ST-1** Timeout on LLM inference
11. **ST-3** Exponential backoff in fleet sync
12. **ST-4** Cap acknowledgment list size
13. **ST-7** Log on poisoned RwLock instead of silent return

### Batch 4 — Security tightening

14. **SEC-2** Increase pairing code entropy or add attempt tracking
15. **SEC-3** Enforce TLS for pairing flow
16. **SEC-4** Restrict CORS origins

### Batch 5 — Quality of life

17. **CQ-1** Split `api/handlers.rs` into sub-modules
18. **CQ-2** Update `.gitignore`
19. **CQ-3** Convert API regression tests to in-process

---

## Appendix: Files Audited

```
src/pipeline/coordinator.rs     src/api/mod.rs
src/pipeline/processing_loop.rs src/api/handlers.rs
src/pipeline/state.rs           src/api/v2_handlers.rs
src/physics_engine/mod.rs       src/api/v2_routes.rs
src/physics_engine/drilling_models.rs  src/api/envelope.rs
src/baseline/mod.rs             src/api/middleware.rs
src/agents/tactical.rs          src/api/setup.rs
src/agents/orchestrator.rs      src/fleet/client.rs
src/agents/strategic.rs         src/fleet/sync.rs
src/config/mod.rs               src/fleet/uploader.rs
src/config/well_config.rs       src/hub/api/mod.rs
src/config/validation.rs        src/hub/api/pairing.rs
src/config/auto_detect.rs       src/acquisition/scanner.rs
src/llm/mod.rs                  src/knowledge_base/fleet_bridge.rs
src/llm/scheduler.rs            src/ml_engine/storage.rs
src/llm/tactical_llm.rs         src/ml_engine/optimal_finder.rs
src/lib.rs                      src/ml_engine/analyzer.rs
src/main.rs                     build.rs
Cargo.toml                      .gitignore
tests/ (all 7 integration files)
```
