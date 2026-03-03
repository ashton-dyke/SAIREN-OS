//! Volve Regression Suite
//!
//! Runs the full volve-replay binary on all three wells (F-5, F-9A, F-12)
//! and asserts that key metrics stay within expected bounds.
//!
//! These tests are gated behind `#[ignore]` because they take 30-60 seconds
//! each. Run with: `cargo test --test volve_regression -- --ignored`

use std::path::PathBuf;
use std::process::Command;

/// JSON summary from volve-replay --json-summary
#[derive(Debug)]
struct ReplaySummary {
    total_packets: u64,
    sanitized_rejected: u64,
    sanitized_rejected_pct: f64,
    depth_rejected: u64,
    drilling_packets: u64,
    tickets_generated: u64,
    tickets_confirmed: u64,
    tickets_rejected: u64,
    tickets_uncertain: u64,
    confirmed_pct: f64,
    rejected_pct: f64,
    well_control_events: u64,
    efficiency_events: u64,
    mechanical_events: u64,
    hydraulics_events: u64,
    formation_events: u64,
    avg_mse: f64,
    avg_rop: f64,
    baseline_locked: bool,
}

impl ReplaySummary {
    fn from_json(json: &str) -> Self {
        // Simple manual JSON parsing — avoids adding serde_json as a dev dependency.
        let get_u64 = |key: &str| -> u64 {
            json.lines()
                .find(|l| l.contains(key))
                .and_then(|l| {
                    let after = l.split(':').nth(1)?;
                    let cleaned = after.trim().trim_end_matches(',');
                    cleaned.parse().ok()
                })
                .unwrap_or(0)
        };
        let get_f64 = |key: &str| -> f64 {
            json.lines()
                .find(|l| l.contains(key))
                .and_then(|l| {
                    let after = l.split(':').nth(1)?;
                    let cleaned = after.trim().trim_end_matches(',');
                    cleaned.parse().ok()
                })
                .unwrap_or(0.0)
        };
        let get_bool = |key: &str| -> bool {
            json.lines()
                .find(|l| l.contains(key))
                .map(|l| l.contains("true"))
                .unwrap_or(false)
        };

        Self {
            total_packets: get_u64("\"total_packets\""),
            sanitized_rejected: get_u64("\"sanitized_rejected\""),
            sanitized_rejected_pct: get_f64("\"sanitized_rejected_pct\""),
            depth_rejected: get_u64("\"depth_rejected\""),
            drilling_packets: get_u64("\"drilling_packets\""),
            tickets_generated: get_u64("\"tickets_generated\""),
            tickets_confirmed: get_u64("\"tickets_confirmed\""),
            tickets_rejected: get_u64("\"tickets_rejected\""),
            tickets_uncertain: get_u64("\"tickets_uncertain\""),
            confirmed_pct: get_f64("\"confirmed_pct\""),
            rejected_pct: get_f64("\"rejected_pct\""),
            well_control_events: get_u64("\"well_control_events\""),
            efficiency_events: get_u64("\"efficiency_events\""),
            mechanical_events: get_u64("\"mechanical_events\""),
            hydraulics_events: get_u64("\"hydraulics_events\""),
            formation_events: get_u64("\"formation_events\""),
            avg_mse: get_f64("\"avg_mse\""),
            avg_rop: get_f64("\"avg_rop\""),
            baseline_locked: get_bool("\"baseline_locked\""),
        }
    }
}

fn binary_path() -> PathBuf {
    // Use the release binary if available, otherwise debug
    let release = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/release/volve-replay");
    if release.exists() {
        return release;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/volve-replay")
}

fn csv_path(well: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("data/volve/{}_witsml.csv", well))
}

fn run_replay(well: &str) -> ReplaySummary {
    let bin = binary_path();
    assert!(bin.exists(), "volve-replay binary not found at {:?}. Run `cargo build --release --bin volve-replay` first.", bin);

    let csv = csv_path(well);
    assert!(csv.exists(), "Volve CSV not found at {:?}", csv);

    let output = Command::new(&bin)
        .args(["--file", csv.to_str().unwrap(), "--json-summary"])
        .output()
        .expect("Failed to execute volve-replay");

    assert!(
        output.status.success(),
        "volve-replay exited with error for {}: {}",
        well,
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    ReplaySummary::from_json(&stdout)
}

// ============================================================================
// F-5 Regression
// ============================================================================

/// F-5: Well with historical pit rate flood (v5.0 had 269 tickets, 258 WellControl).
/// After Phase 1-3 tuning: 9 tickets, 56% confirmed, no WellControl noise.
#[test]
#[ignore]
fn regression_f5() {
    let s = run_replay("F-5");
    eprintln!("F-5: {:?}", s);

    assert!(s.baseline_locked, "Baseline should lock on F-5");
    assert!(s.total_packets > 100_000, "F-5 should have >100k packets");

    // Ticket bounds: historically 9, allow 5-20
    assert!(
        s.tickets_generated >= 5 && s.tickets_generated <= 20,
        "F-5 tickets out of range: {} (expected 5-20)",
        s.tickets_generated
    );

    // Confirmed rate >= 40%
    assert!(
        s.confirmed_pct >= 40.0,
        "F-5 confirmed rate too low: {:.1}% (expected >= 40%)",
        s.confirmed_pct
    );

    // Sanitizer rejection should be low (historically 2%)
    assert!(
        s.sanitized_rejected_pct <= 5.0,
        "F-5 sanitizer rejection too high: {:.1}% (expected <= 5%)",
        s.sanitized_rejected_pct
    );

    // No WellControl noise flood (the Phase 1.2 fix)
    assert!(
        s.well_control_events <= 5,
        "F-5 WellControl events: {} (expected <= 5, was 258 pre-fix)",
        s.well_control_events
    );
}

// ============================================================================
// F-9A Regression
// ============================================================================

/// F-9A: Clean well with low ticket count.
/// After Phase 1-3 tuning: 3 tickets, 33% confirmed.
#[test]
#[ignore]
fn regression_f9a() {
    let s = run_replay("F-9A");
    eprintln!("F-9A: {:?}", s);

    assert!(s.baseline_locked, "Baseline should lock on F-9A");
    assert!(s.total_packets > 50_000, "F-9A should have >50k packets");

    // Ticket bounds: historically 3, allow 1-15
    assert!(
        s.tickets_generated >= 1 && s.tickets_generated <= 15,
        "F-9A tickets out of range: {} (expected 1-15)",
        s.tickets_generated
    );

    // Sanitizer rejection should be moderate (historically 13.3%)
    assert!(
        s.sanitized_rejected_pct >= 5.0 && s.sanitized_rejected_pct <= 25.0,
        "F-9A sanitizer rejection out of range: {:.1}% (expected 5-25%)",
        s.sanitized_rejected_pct
    );
}

// ============================================================================
// F-12 Regression
// ============================================================================

/// F-12: Large well with lots of bad data (31.5% sanitizer rejection).
/// After Phase 1-3 tuning: 15 tickets, 73% confirmed.
#[test]
#[ignore]
fn regression_f12() {
    let s = run_replay("F-12");
    eprintln!("F-12: {:?}", s);

    assert!(s.baseline_locked, "Baseline should lock on F-12");
    assert!(s.total_packets > 2_000_000, "F-12 should have >2M packets");

    // Ticket bounds: historically 15, allow 5-30
    assert!(
        s.tickets_generated >= 5 && s.tickets_generated <= 30,
        "F-12 tickets out of range: {} (expected 5-30)",
        s.tickets_generated
    );

    // Confirmed rate >= 60%
    assert!(
        s.confirmed_pct >= 60.0,
        "F-12 confirmed rate too low: {:.1}% (expected >= 60%)",
        s.confirmed_pct
    );

    // Sanitizer rejection should be high (historically 31.5%)
    assert!(
        s.sanitized_rejected_pct >= 25.0,
        "F-12 sanitizer rejection too low: {:.1}% (expected >= 25%)",
        s.sanitized_rejected_pct
    );
}
