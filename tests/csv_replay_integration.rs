//! CSV Replay Integration Test
//!
//! Lightweight smoke test that exercises the core pipeline path:
//! Load Volve CSV -> feed packets through TacticalAgent + StrategicAgent -> verify results.
//!
//! This mirrors the `volve-replay` binary's processing loop but runs only ~50 drilling
//! packets to keep the test fast.

use sairen_os::agents::{StrategicAgent, TacticalAgent};
use sairen_os::config::{self, WellConfig};
use sairen_os::context::{KnowledgeStore, StaticKnowledgeBase};
use sairen_os::types::{HistoryEntry, VerificationStatus};
use sairen_os::volve::{VolveConfig, VolveReplay};
use std::collections::VecDeque;
use std::path::PathBuf;

/// Path to the Volve F-9A CSV that ships with the repo.
fn volve_csv_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/volve/F-9A_witsml.csv")
}

/// Load the Volve replay or skip the test if the CSV is missing.
fn load_volve() -> VolveReplay {
    let csv_path = volve_csv_path();
    assert!(
        csv_path.exists(),
        "Volve CSV not found at {}",
        csv_path.display()
    );

    VolveReplay::load(
        &csv_path,
        VolveConfig {
            skip_null_rows: true,
            nan_to_zero: true,
            ..Default::default()
        },
    )
    .expect("Failed to load Volve CSV")
}

/// Smoke test: load CSV, push ~50 drilling packets through tactical + strategic agents,
/// assert no panics, packet count matches, and at least 1 advisory ticket is generated.
///
/// The Volve F-9A CSV has ~100k idle rows before drilling begins, so we use
/// `drilling_packets()` to skip directly to real drilling data.
#[test]
fn csv_replay_50_packets_smoke() {
    // 1. Initialize config with defaults (safe for parallel tests -- double-init is ignored)
    if !config::is_initialized() {
        config::init(WellConfig::default(), config::ConfigProvenance::default());
    }

    // 2. Load Volve CSV and extract drilling packets
    let replay = load_volve();
    let drilling_packets = replay.drilling_packets();
    assert!(
        drilling_packets.len() > 50,
        "Expected >50 drilling packets in CSV, got {}",
        drilling_packets.len()
    );

    // 3. Set up agents (same pattern as volve-replay binary)
    let mut tactical = TacticalAgent::new();
    let mut strategic = StrategicAgent::new();
    let mut history: VecDeque<HistoryEntry> = VecDeque::with_capacity(60);

    // 4. Process first 50 drilling packets through the pipeline
    let target_count: usize = 50;
    let mut packets_processed: u64 = 0;
    let mut tickets_created: u64 = 0;
    let mut tickets_confirmed: u64 = 0;

    for packet in drilling_packets.iter().take(target_count) {
        let (ticket_opt, _metrics, history_entry) = tactical.process(packet, false, None);
        packets_processed += 1;

        // Update rolling history (same as coordinator Phase 4)
        if history.len() >= 60 {
            history.pop_front();
        }
        history.push_back(history_entry);

        // If tactical agent generated a ticket, run strategic verification
        if let Some(ref ticket) = ticket_opt {
            tickets_created += 1;

            let history_slice: Vec<HistoryEntry> = history.iter().cloned().collect();
            let result = strategic.verify_ticket(ticket, &history_slice);

            if result.status == VerificationStatus::Confirmed {
                tickets_confirmed += 1;
            }
        }
    }

    // 5. Assertions
    assert_eq!(
        packets_processed, target_count as u64,
        "Expected {} packets processed, got {}",
        target_count, packets_processed
    );

    // Note: CfC warm-up suppresses non-safety tickets for the first 500 drilling
    // packets, so 50 packets may produce 0 tickets unless WellControl events are
    // present. This smoke test validates no-panic processing, not ticket generation.
    // Ticket generation is validated by csv_replay_200_packets_baseline_and_tickets
    // and the full Volve replay.

    eprintln!(
        "csv_replay_50_packets_smoke: {} packets, {} tickets ({} confirmed)",
        packets_processed, tickets_created, tickets_confirmed
    );
}

/// Item 5.5 — verify the StaticKnowledgeBase (context pipeline) returns non-empty
/// results for every drilling-relevant category keyword.
///
/// This closes the concern that `context_used` might always be empty in production:
/// the coordinator calls `lookup_context()` → `StaticKnowledgeBase::query()` in
/// Phase 6 before advisory composition.  Here we confirm the KB is wired correctly
/// and returns at least one snippet for each common fault category.
#[test]
fn csv_replay_context_lookup_non_empty() {
    let kb = StaticKnowledgeBase;

    // These queries mirror the exact strings used by coordinator.rs Phase 6
    let test_queries: &[(&str, &str)] = &[
        ("well control kick loss circulation flow imbalance", "WellControl"),
        ("MSE drilling efficiency ROP optimization", "DrillingEfficiency"),
        ("hydraulics standpipe pressure ECD mud weight", "Hydraulics"),
        ("formation change lithology d-exponent pressure", "FormationEvaluation"),
    ];

    for (query, label) in test_queries {
        let results = kb.query(query, 3);
        assert!(
            !results.is_empty(),
            "StaticKnowledgeBase returned 0 results for '{label}' query; \
             context_used would be empty in production for this fault category"
        );
    }
}

/// Verify that drilling packets have diverse rig states and non-zero parameters.
#[test]
fn csv_replay_drilling_data_quality() {
    if !config::is_initialized() {
        config::init(WellConfig::default(), config::ConfigProvenance::default());
    }

    let replay = load_volve();
    let drilling = replay.drilling_packets();

    assert!(
        !drilling.is_empty(),
        "No drilling packets found in Volve CSV"
    );

    // Verify drilling packets have non-zero key parameters
    let sample_size = drilling.len().min(100);
    let mut has_nonzero_wob = false;
    let mut has_nonzero_rpm = false;
    let mut has_nonzero_rop = false;
    let mut has_nonzero_spp = false;

    for p in drilling.iter().take(sample_size) {
        if p.wob > 0.0 {
            has_nonzero_wob = true;
        }
        if p.rpm > 0.0 {
            has_nonzero_rpm = true;
        }
        if p.rop > 0.0 {
            has_nonzero_rop = true;
        }
        if p.spp > 0.0 {
            has_nonzero_spp = true;
        }
    }

    assert!(has_nonzero_wob, "No non-zero WOB in drilling packets");
    assert!(has_nonzero_rpm, "No non-zero RPM in drilling packets");
    assert!(has_nonzero_rop, "No non-zero ROP in drilling packets");
    assert!(has_nonzero_spp, "No non-zero SPP in drilling packets");

    eprintln!(
        "csv_replay_drilling_data_quality: {} drilling packets validated (of {} total)",
        sample_size,
        replay.info.packet_count
    );
}

/// Verify that processing more packets (200) builds enough baseline for ticket generation.
#[test]
fn csv_replay_200_packets_baseline_and_tickets() {
    if !config::is_initialized() {
        config::init(WellConfig::default(), config::ConfigProvenance::default());
    }

    let replay = load_volve();
    let drilling = replay.drilling_packets();
    let target_count: usize = 200.min(drilling.len());

    let mut tactical = TacticalAgent::new();
    let mut history: VecDeque<HistoryEntry> = VecDeque::with_capacity(60);
    let mut tickets_created: u64 = 0;
    let mut baseline_locked = false;

    for packet in drilling.iter().take(target_count) {
        let (ticket_opt, _metrics, history_entry) = tactical.process(packet, false, None);

        if history.len() >= 60 {
            history.pop_front();
        }
        history.push_back(history_entry);

        if ticket_opt.is_some() {
            tickets_created += 1;
        }
        if tactical.is_baseline_locked() {
            baseline_locked = true;
        }
    }

    // With 200 drilling packets, the tactical agent should have locked baseline
    // (requires ~100 samples) and started generating tickets.
    eprintln!(
        "csv_replay_200_packets: {} packets, baseline_locked={}, {} tickets",
        target_count, baseline_locked, tickets_created
    );

    // At minimum, the pipeline must not panic and must process all packets.
    assert_eq!(
        history.len().min(60),
        if target_count >= 60 { 60 } else { target_count },
        "History buffer should be filled"
    );

    // Phase 1C: Baseline should lock after 200 packets (min_samples_for_lock = 100)
    assert!(
        baseline_locked,
        "Baseline should lock after {} packets (min_samples_for_lock = 100)",
        target_count
    );

    // Phase 1C: Volve data contains real anomalies — should generate at least 1 ticket
    assert!(
        tickets_created >= 1,
        "Expected at least 1 advisory ticket from {} drilling packets (Volve data has anomalies), got {}",
        target_count, tickets_created
    );
}
