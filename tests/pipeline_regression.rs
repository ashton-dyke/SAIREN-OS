//! Pipeline Regression Tests
//!
//! Exercises the full pipeline through TacticalAgent + StrategicAgent with
//! real Volve drilling data. Asserts on baseline locking, advisory generation,
//! and data integrity (no NaN values, valid severity enums).
//!
//! These tests require the Volve F-9A CSV at data/volve/F-9A_witsml.csv.
//! If the CSV is missing, tests are skipped (not failed).

use sairen_os::agents::{StrategicAgent, TacticalAgent};
use sairen_os::config::{self, WellConfig};
use sairen_os::types::{HistoryEntry, VerificationStatus};
use sairen_os::volve::{VolveConfig, VolveReplay};
use std::collections::VecDeque;
use std::path::PathBuf;

/// Path to the Volve F-9A CSV that ships with the repo.
fn volve_csv_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/volve/F-9A_witsml.csv")
}

/// Load Volve replay or return None if CSV is missing (skip test).
fn try_load_volve() -> Option<VolveReplay> {
    let path = volve_csv_path();
    if !path.exists() {
        eprintln!(
            "SKIP: Volve CSV not found at {} — skipping pipeline regression test",
            path.display()
        );
        return None;
    }
    Some(
        VolveReplay::load(
            &path,
            VolveConfig {
                skip_null_rows: true,
                nan_to_zero: true,
                ..Default::default()
            },
        )
        .expect("Failed to load Volve CSV"),
    )
}

fn ensure_config() {
    if !config::is_initialized() {
        config::init(WellConfig::default(), config::ConfigProvenance::default());
    }
}

/// Process N drilling packets through tactical + strategic agents.
/// Returns (packets_processed, tickets_created, tickets_confirmed, baseline_locked, has_nan).
fn run_pipeline(
    drilling_packets: &[&sairen_os::types::WitsPacket],
    count: usize,
) -> (u64, u64, u64, bool, bool) {
    let mut tactical = TacticalAgent::new();
    let mut strategic = StrategicAgent::new();
    let mut history: VecDeque<HistoryEntry> = VecDeque::with_capacity(60);

    let mut packets_processed: u64 = 0;
    let mut tickets_created: u64 = 0;
    let mut tickets_confirmed: u64 = 0;
    let mut baseline_locked = false;
    let mut has_nan = false;

    for packet in drilling_packets.iter().take(count) {
        let (ticket_opt, metrics, history_entry) = tactical.process(packet, false, None);
        packets_processed += 1;

        // Check for NaN in drilling metrics
        if metrics.mse.is_nan() || metrics.d_exponent.is_nan() || metrics.mse_efficiency.is_nan() {
            has_nan = true;
        }

        // Update rolling history
        if history.len() >= 60 {
            history.pop_front();
        }
        history.push_back(history_entry);

        // Run strategic verification on tickets
        if let Some(ref ticket) = ticket_opt {
            tickets_created += 1;

            // Verify severity is a valid enum value (not a corrupted number)
            let severity_val = ticket.severity as u8;
            assert!(
                (1..=4).contains(&severity_val),
                "Invalid ticket severity: {} (must be 1-4)",
                severity_val
            );

            let history_slice: Vec<HistoryEntry> = history.iter().cloned().collect();
            let result = strategic.verify_ticket(ticket, &history_slice);
            if result.status == VerificationStatus::Confirmed {
                tickets_confirmed += 1;
            }
        }

        if tactical.is_baseline_locked() {
            baseline_locked = true;
        }
    }

    (
        packets_processed,
        tickets_created,
        tickets_confirmed,
        baseline_locked,
        has_nan,
    )
}

/// 200-packet regression: baseline should lock and advisories should appear.
#[test]
fn pipeline_200_packets_baseline_locks() {
    ensure_config();
    let Some(replay) = try_load_volve() else {
        return;
    };
    let drilling = replay.drilling_packets();
    let count = 200.min(drilling.len());

    let (processed, tickets, _confirmed, locked, _) = run_pipeline(&drilling, count);

    assert_eq!(processed, count as u64);

    // With 200 drilling packets (min_samples_for_lock = 100), baseline should lock
    eprintln!(
        "pipeline_200_packets: {} packets, baseline_locked={}, {} tickets",
        processed, locked, tickets
    );
    assert!(
        locked,
        "Baseline should lock after {} packets (min_samples_for_lock = 100)",
        count
    );
}

/// With enough data, the Volve anomalies should generate at least one advisory.
#[test]
fn pipeline_200_packets_generates_advisories() {
    ensure_config();
    let Some(replay) = try_load_volve() else {
        return;
    };
    let drilling = replay.drilling_packets();
    let count = 200.min(drilling.len());

    let (_, tickets, _, _, _) = run_pipeline(&drilling, count);

    assert!(
        tickets >= 1,
        "Volve data should generate at least 1 advisory ticket in {} packets",
        count
    );
}

/// No NaN values should appear in drilling metrics.
#[test]
fn pipeline_no_nan_in_metrics() {
    ensure_config();
    let Some(replay) = try_load_volve() else {
        return;
    };
    let drilling = replay.drilling_packets();
    let count = 200.min(drilling.len());

    let (_, _, _, _, has_nan) = run_pipeline(&drilling, count);

    assert!(
        !has_nan,
        "No NaN values should appear in drilling metrics (MSE, d-exponent, efficiency)"
    );
}

/// Process all available drilling packets — no panics.
#[test]
fn pipeline_full_volve_no_panic() {
    ensure_config();
    let Some(replay) = try_load_volve() else {
        return;
    };
    let drilling = replay.drilling_packets();
    // Process up to 1000 packets (enough for a thorough test without being slow)
    let count = 1000.min(drilling.len());

    let (processed, tickets, confirmed, locked, has_nan) = run_pipeline(&drilling, count);

    eprintln!(
        "pipeline_full_volve: {} packets, baseline_locked={}, {} tickets ({} confirmed), nan={}",
        processed, locked, tickets, confirmed, has_nan
    );

    // Just assert no panic (the test completing is the assertion)
    assert!(processed > 0);
}
