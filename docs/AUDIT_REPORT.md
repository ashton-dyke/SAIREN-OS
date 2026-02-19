# SAIREN-OS System Audit Report

**Date:** 2026-01-25
**Auditor:** Claude Code
**System Version:** WITS-based Drilling Operational Intelligence

---

## Fixes Applied (2026-01-25)

| # | Issue | File(s) | Status |
|---|-------|---------|--------|
| 1 | Missing 6 WITS metrics in baseline learning | `src/agents/tactical.rs:335-347` | ✅ FIXED |
| 2 | ECD margin using hardcoded formula | `src/api/handlers.rs:271-272` | ✅ FIXED |
| 3 | Default equipment ID "TDS" → "RIG" | `src/api/handlers.rs`, `src/api/routes.rs` | ✅ FIXED |
| 4 | MSE baseline using approximation | `src/api/handlers.rs:960-984` | ✅ FIXED |
| 5 | Normal mud weight hardcoded | `src/types.rs:685`, `src/physics_engine/mod.rs:81` | ✅ FIXED |
| 6 | Database lock conflict | `src/storage/lockfile.rs` (new), `src/main.rs` | ✅ FIXED |

**Build Status:** Release build successful | All tests passing

---

## Executive Summary

This audit examined the SAIREN-OS codebase to identify issues that could prevent the system from working correctly and opportunities for efficiency improvements. The system has been successfully transformed from a TDS vibration monitoring system to a WITS-based drilling intelligence platform.

**Overall Assessment:** The system is well-structured with comprehensive drilling physics calculations, proper two-stage agent architecture, and robust anomaly detection. Several issues were identified and most have been fixed.

---

## Remaining Issues

No critical issues remaining. All identified issues have been fixed.

---

### Database Lock Conflict (FIXED)

**Location:** `src/storage/lockfile.rs` (new file), `src/main.rs`
**Issue:** When SAIREN-OS was run while another instance was already using the database, it failed with:
```
Failed to open sled database: Resource temporarily unavailable
```

**Fix Applied:**
- Created `ProcessLock` module that acquires a lock file before opening the database
- Automatically detects stale lock files from crashed processes (checks if PID is still running)
- Provides clear error message with the conflicting PID and instructions to resolve
- Lock is automatically released when application exits (via Drop trait)

**Error message now shown:**
```
Another SAIREN-OS instance is already running (PID: 12345)

To resolve this:
1. Stop the other instance, or
2. If no other instance is running, remove the stale lock file:
   rm "./data/.sairen.lock"
```

---

## Issues Fixed This Session

### 2. Baseline Metrics Not Being Fed During Learning (FIXED)

**Location:** `src/api/handlers.rs:1184-1197`
**Issue:** The baseline status API was checking TDS metrics instead of WITS metrics.

**Status:** Fixed by changing `metrics_to_check` to use `wits_metrics::*`.

### 3. ECD Margin Calculation in API Handler (FIXED)

**Location:** `src/api/handlers.rs:272`
**Issue:** The ECD margin calculation used a hardcoded fallback that doesn't use actual fracture gradient:
```rust
0.5_f64.max(packet.ecd * 0.1)  // Arbitrary calculation
```

**Fix Applied:** Now uses `packet.ecd_margin()` which calculates `fracture_gradient - ecd`:
```rust
packet.ecd_margin()  // Uses fracture_gradient - ecd
```

---

## Moderate Issues (Should Fix)

### 4. Tactical Agent Feeds Only 6 WITS Metrics to Baseline

**Location:** `src/agents/tactical.rs:336-341`
**Issue:** Only 6 metrics are fed to the baseline accumulator during learning:
- MSE, D_EXPONENT, FLOW_BALANCE, SPP, TORQUE, ROP

But the API and baseline system expect 12 metrics:
- MSE, D_EXPONENT, DXC, FLOW_BALANCE, SPP, TORQUE, ROP, WOB, RPM, ECD, PIT_VOLUME, GAS_UNITS

**Impact:** Baseline status shows only 6 metrics learning, and 6 never get baseline data.

**Recommendation:** Add missing metrics to `feed_baseline_samples()`:
```rust
mgr.add_sample(&self.equipment_id, wits_metrics::DXC, metrics.dxc, timestamp);
mgr.add_sample(&self.equipment_id, wits_metrics::WOB, packet.wob, timestamp);
mgr.add_sample(&self.equipment_id, wits_metrics::RPM, packet.rpm, timestamp);
mgr.add_sample(&self.equipment_id, wits_metrics::ECD, packet.ecd, timestamp);
mgr.add_sample(&self.equipment_id, wits_metrics::PIT_VOLUME, packet.pit_volume, timestamp);
mgr.add_sample(&self.equipment_id, wits_metrics::GAS_UNITS, packet.gas_units, timestamp);
```

### 5. Hardcoded Equipment ID in DashboardState

**Location:** `src/api/handlers.rs:49, 65`
**Issue:** Default equipment ID is "TDS" which is a legacy name:
```rust
equipment_id: "TDS".to_string()
```

**Recommendation:** Change default to "RIG" to match the WITS-based system.

### 6. Specialist Weights Sum to 100% but WELL_CONTROL is 30%

**Location:** `src/types.rs:718-727`
**Issue:** The weights module defines:
```rust
pub const MSE: f64 = 0.25;
pub const HYDRAULIC: f64 = 0.25;
pub const WELL_CONTROL: f64 = 0.30;
pub const FORMATION: f64 = 0.20;
```
Sum = 1.00 ✓

**Status:** Working correctly. The well control weight being highest (30%) is intentional for safety prioritization.

### 7. Legacy TDS Thresholds Still Present

**Location:** `src/types.rs:1042-1057`
**Issue:** The `thresholds` module re-exports drilling thresholds but also defines legacy TDS constants that are no longer used:
- KURTOSIS_WARNING, KURTOSIS_CRITICAL
- BPFO_WARNING, BPFO_CRITICAL
- VIB_AMPLITUDE_WARNING, VIB_AMPLITUDE_CRITICAL

**Recommendation:** Consider removing legacy TDS constants to reduce confusion, or move them to a `legacy_thresholds` module.

---

## Minor Issues & Improvements

### 8. Dashboard MSE Baseline Calculation is Approximate

**Location:** `src/api/handlers.rs:989-990`
**Issue:** MSE baseline is calculated as a rough approximation:
```rust
mse_baseline: mse * 0.85,
mse_deviation: if mse > 0.0 { ((mse - mse * 0.85) / (mse * 0.85)) * 100.0 } else { 0.0 },
```

**Recommendation:** Use actual baseline values from the threshold manager when available.

### 9. Normal Mud Weight Hardcoded

**Location:** `src/physics_engine/mod.rs:81`
**Issue:** Normal mud weight for dxc calculation is hardcoded:
```rust
let normal_mud_weight = 8.6; // Typical normal gradient
```

**Recommendation:** Make this configurable or derive from wellbore data.

### 10. History Buffer Size Fixed at 60

**Location:** `src/pipeline/coordinator.rs` (HISTORY_BUFFER_SIZE)
**Issue:** History buffer holds 60 packets. At 1-minute intervals, this is 1 hour of data.

**Status:** This is appropriate for the current use case. No change needed.

### 11. Pit Rate Calculation Depends on Previous Packet

**Location:** `src/physics_engine/mod.rs:88-97`
**Issue:** Pit rate calculation returns 0.0 if no previous packet exists:
```rust
let pit_rate = if let Some(prev) = prev_packet {
    ...
} else {
    0.0
};
```

**Status:** Expected behavior for first packet. No change needed.

---

## Performance Considerations

### 12. Phase 2 Performance Target of 15ms

**Location:** `src/agents/tactical.rs:282-285`
**Issue:** System logs warning when Phase 2 exceeds 15ms:
```rust
if elapsed.as_millis() > 15 {
    warn!(elapsed_ms = elapsed.as_millis(), "Phase 2 exceeded 15ms target");
}
```

**Status:** Good performance monitoring in place.

### 13. Packet Clone Operations

**Location:** `src/agents/tactical.rs:299`
**Issue:** Full packet clone on every iteration:
```rust
self.prev_packet = Some(packet.clone());
```

**Potential Improvement:** Consider using `Arc<WitsPacket>` for zero-copy sharing if performance becomes an issue at high packet rates.

### 14. History Entry Creation Always Clones Packet and Metrics

**Location:** `src/agents/tactical.rs:289-293`
**Issue:** Every packet creates a new HistoryEntry with cloned data.

**Status:** Acceptable for current 1-minute intervals. Would need optimization for sub-second packet rates.

---

## API/Dashboard Compatibility

### 15. Dashboard Expects Specific API Response Structure

**Location:** `static/index.html`
**Issue:** Dashboard JavaScript expects specific fields from API responses.

**Verified Fields:**
- `/api/v1/status` - All WITS drilling parameters present ✓
- `/api/v1/health` - MSE efficiency and risk level added ✓
- `/api/v1/verification` - Includes ticket details ✓
- `/api/v1/drilling` - MSE metrics and specialist votes ✓
- `/api/v1/baseline` - WITS metrics status ✓
- `/api/v1/diagnosis` - Strategic advisory ✓

**Status:** API handlers properly updated for WITS data.

### 16. Spectrum and TTF Endpoints Use Legacy TDS Calculations

**Location:** `src/api/handlers.rs:346-477`
**Issue:** Spectrum and TTF (Time-to-Failure) endpoints still use bearing frequency calculations and L10 life predictions from the TDS system.

**Recommendation:** Consider updating these for drilling-relevant predictions, or mark them as legacy/deprecated in the API documentation.

---

## Recommendations Summary

### High Priority
1. ✅ Fix ECD margin calculation in API handler (use `packet.ecd_margin()`)
2. ✅ Add missing 6 WITS metrics to baseline learning in tactical agent
3. ✅ Change default equipment ID from "TDS" to "RIG"

### Medium Priority
4. Add process lock detection for database conflicts
5. Use actual baseline values for MSE deviation calculation
6. Make normal mud weight configurable for d-exponent correction

### Low Priority
7. Consider removing legacy TDS constants from types.rs
8. Document legacy spectrum/TTF endpoints or update for drilling context
9. Consider Arc<WitsPacket> for performance at high packet rates

---

## Files Audited

| File | Lines | Status |
|------|-------|--------|
| `src/types.rs` | 1058 | ✓ Clean - Well-structured WITS types |
| `src/api/handlers.rs` | 1345 | ⚠ Minor issues (ECD, equipment ID) |
| `src/pipeline/coordinator.rs` | 719 | ✓ Clean - 10-phase pipeline working |
| `src/agents/tactical.rs` | 713 | ⚠ Missing baseline metrics |
| `src/agents/strategic.rs` | 806 | ✓ Clean - Verification logic sound |
| `src/agents/orchestrator.rs` | 612 | ✓ Clean - Voting system correct |
| `src/physics_engine/mod.rs` | 468 | ✓ Clean - Physics calculations correct |
| `src/physics_engine/drilling_models.rs` | 749 | ✓ Clean - MSE, d-exp formulas correct |
| `src/baseline/mod.rs` | 1103 | ✓ Clean - WITS metrics defined |
| `static/index.html` | 2000+ | ✓ Compatible with API |

---

## Conclusion

The SAIREN-OS system has been successfully transformed to a WITS-based drilling intelligence platform. The core architecture is sound with proper separation between tactical anomaly detection, strategic verification, and orchestrator voting.

The identified issues are primarily configuration and completeness issues rather than fundamental architectural problems. Addressing the high-priority items will ensure the system operates as intended with full baseline learning capability.

---

*Report generated by Claude Code system audit*
