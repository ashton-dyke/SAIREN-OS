//! Volve Field Data Replay
//!
//! Feeds real Equinor Volve drilling data through the SAIREN-OS pipeline:
//! Physics Engine → Tactical Agent → ACI Conformal Intervals → Strategic Agent
//! (no LLM, CPU only).
//!
//! Usage:
//!   cargo run --bin volve-replay
//!   cargo run --bin volve-replay -- --file data/volve/some_other_well.csv

use sairen_os::aci::{self, ConformalInterval};
use sairen_os::agents::{StrategicAgent, TacticalAgent};
use sairen_os::baseline::ThresholdManager;
use sairen_os::config::{self, WellConfig};
use sairen_os::types::{
    AnomalyCategory, DrillingMetrics, HistoryEntry, RigState, VerificationStatus,
};
use sairen_os::volve::{VolveConfig, VolveReplay};
use std::collections::VecDeque;
use std::sync::{Arc, RwLock};

// ============================================================================
// Statistics tracking
// ============================================================================

#[derive(Default)]
struct ReplayStats {
    total_packets: u64,
    drilling_packets: u64,
    tickets_generated: u64,
    tickets_confirmed: u64,
    tickets_rejected: u64,
    tickets_uncertain: u64,

    // Anomaly category counts
    well_control_events: u64,
    efficiency_events: u64,
    mechanical_events: u64,
    hydraulics_events: u64,
    formation_events: u64,

    // Physics aggregates (drilling packets only)
    mse_sum: f64,
    mse_count: u64,
    mse_max: f64,
    rop_sum: f64,
    rop_count: u64,

    // Depth tracking
    depth_at_first_ticket: Option<f64>,
    depth_at_last_ticket: Option<f64>,

    // Baseline
    baseline_locked_at_packet: Option<u64>,

    // ACI stats
    aci_outlier_packets: u64,
    aci_multi_outlier_packets: u64,
}

impl ReplayStats {
    fn record_metrics(&mut self, metrics: &DrillingMetrics, packet: &sairen_os::types::WitsPacket) {
        self.total_packets += 1;

        if matches!(metrics.state, RigState::Drilling) {
            self.drilling_packets += 1;

            if metrics.mse > 0.0 {
                self.mse_sum += metrics.mse;
                self.mse_count += 1;
                if metrics.mse > self.mse_max {
                    self.mse_max = metrics.mse;
                }
            }

            if packet.rop > 0.0 {
                self.rop_sum += packet.rop;
                self.rop_count += 1;
            }
        }
    }

    fn avg_mse(&self) -> f64 {
        if self.mse_count > 0 { self.mse_sum / self.mse_count as f64 } else { 0.0 }
    }

    fn avg_rop(&self) -> f64 {
        if self.rop_count > 0 { self.rop_sum / self.rop_count as f64 } else { 0.0 }
    }
}

/// Format a conformal interval compactly
fn fmt_interval(name: &str, ci: &ConformalInterval, unit: &str) -> String {
    let flag = if ci.is_outlier { " ◀ OUTLIER" } else { "" };
    format!(
        "    {:<8} {:>8.1} {} [{:.1} — {:.1}] cov={:.0}% dev={:.2}{}",
        name, ci.value, unit, ci.lower, ci.upper,
        ci.coverage * 100.0, ci.deviation_score, flag
    )
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    // Parse args
    let args: Vec<String> = std::env::args().collect();
    let csv_path = if args.len() > 2 && args[1] == "--file" {
        args[2].clone()
    } else {
        "data/volve/Norway-NA-15_47_9-F-9 A time.csv".to_string()
    };

    // Initialize config (required before agents)
    if !config::is_initialized() {
        config::init(WellConfig::default());
    }

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  SAIREN-OS  ·  Volve Field Data Replay                     ║");
    println!("║  CPU-only  ·  No LLM  ·  Physics + ACI + Rule-Based Agents ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    // Load Volve data
    println!("[1/5] Loading Volve CSV...");
    let volve_config = VolveConfig {
        skip_null_rows: true,
        nan_to_zero: true,
        ..Default::default()
    };

    let replay = match VolveReplay::load(&csv_path, volve_config) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("ERROR: Failed to load Volve data: {}", e);
            eprintln!("  Ensure the CSV exists at: {}", csv_path);
            std::process::exit(1);
        }
    };

    replay.print_summary();
    println!();

    // Set up threshold manager for baseline learning
    println!("[2/5] Initializing agents...");
    let threshold_manager = Arc::new(RwLock::new(ThresholdManager::new()));

    let mut tactical = TacticalAgent::new_with_thresholds(
        "VOLVE-F9A",
        threshold_manager.clone(),
        true, // start in baseline learning mode
    );

    let mut strategic = StrategicAgent::with_thresholds(
        "VOLVE-F9A",
        threshold_manager.clone(),
    );

    println!("  Tactical Agent:  baseline learning + ACI conformal intervals");
    println!("  Strategic Agent: rule-based verification (no LLM)");
    println!("  ACI:             90% coverage, window=200, γ=0.005 (integrated into tactical)");
    println!("  CfC Network:     128 neurons, NCP wiring, shadow mode (self-supervised)");
    println!();

    // Process all packets
    println!("[3/5] Replaying {} packets through pipeline...", replay.info.packet_count);
    println!("─────────────────────────────────────────────────────────────────");
    println!();

    let mut history: VecDeque<HistoryEntry> = VecDeque::with_capacity(60);
    let mut stats = ReplayStats::default();
    let mut last_depth_print = 0.0_f64;
    let mut baseline_announced = false;
    let mut aci_calibrated_announced = false;
    let mut cfc_calibrated_announced = false;

    // ACI alert debouncing: track active alert zones instead of printing every packet
    let mut aci_alert_zone_start: Option<f64> = None; // depth where current zone started
    let mut aci_alert_zone_packets: u64 = 0;
    let mut aci_alert_zone_peak_outliers: usize = 0;
    let mut aci_alert_zones_total: u64 = 0;

    let packets = replay.packets();
    let total = packets.len();

    for (i, packet) in packets.iter().enumerate() {
        // Phase 2-3: Tactical agent processes packet
        let (ticket_opt, metrics, history_entry) = tactical.process(packet);

        // Track stats
        stats.record_metrics(&metrics, packet);

        // Phase 4: Update rolling history
        if history.len() >= 60 {
            history.pop_front();
        }
        history.push_back(history_entry);

        // ACI results come from the tactical agent (integrated)
        let aci_result = tactical.aci_result().cloned();
        if let Some(ref aci_r) = aci_result {
            if aci_r.outlier_count > 0 {
                stats.aci_outlier_packets += 1;
            }
            if aci_r.outlier_count >= 3 {
                stats.aci_multi_outlier_packets += 1;
            }
        }

        // Announce baseline lock
        if !baseline_announced && tactical.is_baseline_locked() {
            baseline_announced = true;
            stats.baseline_locked_at_packet = Some(i as u64);
            println!(
                "  ✓ BASELINE LOCKED at packet {} (depth {:.0} ft) — switching to dynamic thresholds",
                i, packet.bit_depth
            );
        }

        // Announce ACI calibration
        if !aci_calibrated_announced && tactical.aci_tracker().is_calibrated(aci::metrics::MSE) {
            aci_calibrated_announced = true;
            println!(
                "  ✓ ACI CALIBRATED at packet {} (depth {:.0} ft) — conformal intervals active",
                i, packet.bit_depth
            );
            println!();
        }

        // Announce CfC calibration
        if !cfc_calibrated_announced && tactical.cfc_network().is_calibrated() {
            cfc_calibrated_announced = true;
            println!(
                "  ✓ CfC CALIBRATED at packet {} (depth {:.0} ft) — {} params, {} connections, avg loss: {:.6}",
                i, packet.bit_depth,
                tactical.cfc_network().num_params(),
                tactical.cfc_network().num_connections(),
                tactical.cfc_network().avg_loss()
            );
            println!();
        }

        // Progress: print every 500 ft of new depth
        if packet.bit_depth > last_depth_print + 500.0 && packet.bit_depth > 0.0 {
            last_depth_print = packet.bit_depth;
            let pct = (i as f64 / total as f64) * 100.0;

            // Show drilling metrics with ACI intervals when available
            if let Some(ref aci_r) = aci_result {
                // CfC shadow info
                let cfc_info = if let Some(ref cfc_r) = tactical.cfc_result() {
                    if cfc_r.is_calibrated {
                        format!(" | CfC anom={:.2} lr={:.5}", cfc_r.anomaly_score, cfc_r.learning_rate)
                    } else {
                        format!(" | CfC calibrating ({}/500)", cfc_r.packets_processed)
                    }
                } else {
                    String::new()
                };

                println!(
                    "  [{:5.1}%] Depth: {:6.0} ft | State: {:?} | ACI outliers: {} | Tickets: {}{}",
                    pct, packet.bit_depth, metrics.state, aci_r.outlier_count, stats.tickets_generated, cfc_info
                );
                println!("{}", fmt_interval("MSE", &aci_r.mse, "psi"));
                println!("{}", fmt_interval("ROP", &aci_r.rop, "ft/hr"));
                println!("{}", fmt_interval("WOB", &aci_r.wob, "klbs"));
                println!("{}", fmt_interval("SPP", &aci_r.spp, "psi"));
                println!("{}", fmt_interval("Torque", &aci_r.torque, "kft-lb"));
            } else {
                println!(
                    "  [{:5.1}%] Depth: {:6.0} ft | MSE: {:8.0} psi | ROP: {:5.1} ft/hr | State: {:?} | Tickets: {}",
                    pct, packet.bit_depth, metrics.mse, packet.rop, metrics.state, stats.tickets_generated
                );
            }
        }

        // Report ACI multi-outlier events — debounced into zones
        if let Some(ref aci_r) = aci_result {
            let is_alert = aci_r.outlier_count >= 3 && tactical.aci_tracker().is_calibrated(aci::metrics::MSE);
            if is_alert {
                // In an alert zone
                if aci_alert_zone_start.is_none() {
                    aci_alert_zone_start = Some(packet.bit_depth);
                    aci_alert_zone_packets = 0;
                    aci_alert_zone_peak_outliers = 0;
                }
                aci_alert_zone_packets += 1;
                if aci_r.outlier_count > aci_alert_zone_peak_outliers {
                    aci_alert_zone_peak_outliers = aci_r.outlier_count;
                }
            } else if let Some(start_depth) = aci_alert_zone_start.take() {
                // Zone just ended — only print significant zones (3+ packets or 5+ ft span)
                aci_alert_zones_total += 1;
                let span = (packet.bit_depth - start_depth).abs();
                if aci_alert_zone_packets >= 3 || span >= 5.0 || aci_alert_zone_peak_outliers >= 5 {
                    println!(
                        "  ⚡ ACI ZONE #{} | {:.0}–{:.0} ft | {} packets | peak {} metrics outside interval",
                        aci_alert_zones_total, start_depth, packet.bit_depth,
                        aci_alert_zone_packets, aci_alert_zone_peak_outliers
                    );
                }
            }
        }

        // Phase 5-7: Process ticket if generated
        if let Some(ref ticket) = ticket_opt {
            stats.tickets_generated += 1;

            match ticket.category {
                AnomalyCategory::WellControl => stats.well_control_events += 1,
                AnomalyCategory::DrillingEfficiency => stats.efficiency_events += 1,
                AnomalyCategory::Mechanical => stats.mechanical_events += 1,
                AnomalyCategory::Hydraulics => stats.hydraulics_events += 1,
                AnomalyCategory::Formation => stats.formation_events += 1,
                AnomalyCategory::None => {}
            }

            if stats.depth_at_first_ticket.is_none() {
                stats.depth_at_first_ticket = Some(packet.bit_depth);
            }
            stats.depth_at_last_ticket = Some(packet.bit_depth);

            // Strategic verification
            let history_slice: Vec<HistoryEntry> = history.iter().cloned().collect();
            let result = strategic.verify_ticket(ticket, &history_slice);

            let status_str = match result.status {
                VerificationStatus::Confirmed => {
                    stats.tickets_confirmed += 1;
                    "CONFIRMED"
                }
                VerificationStatus::Rejected => {
                    stats.tickets_rejected += 1;
                    "REJECTED "
                }
                VerificationStatus::Uncertain | VerificationStatus::Pending => {
                    stats.tickets_uncertain += 1;
                    "UNCERTAIN"
                }
            };

            println!(
                "  ▶ TICKET #{:<3} @ {:6.0} ft | {:?} {:?} | {} | {}",
                stats.tickets_generated,
                packet.bit_depth,
                ticket.severity,
                ticket.category,
                status_str,
                ticket.description
            );

            // Print strategic reasoning for confirmed tickets
            if result.status == VerificationStatus::Confirmed {
                println!(
                    "    └─ Severity: {:?} | {}",
                    result.final_severity, result.reasoning
                );
            }

            // Print ACI context alongside ticket
            if let Some(ref aci_r) = aci_result {
                let outliers = aci_r.outliers();
                if !outliers.is_empty() {
                    println!("    └─ ACI context: {} metrics outside interval:", outliers.len());
                    for (name, ci) in &outliers {
                        println!("       {} = {:.1} (interval [{:.1}, {:.1}], dev={:.2})",
                            name, ci.value, ci.lower, ci.upper, ci.deviation_score);
                    }
                }
            }

            // Print CfC shadow context alongside ticket
            if let Some(ref cfc_r) = tactical.cfc_result() {
                if cfc_r.is_calibrated {
                    println!("    └─ CfC shadow: anomaly={:.3} health={:.3} loss={:.6}",
                        cfc_r.anomaly_score, cfc_r.health_score,
                        cfc_r.training_loss.unwrap_or(0.0));
                    if !cfc_r.feature_surprises.is_empty() {
                        let top: Vec<String> = cfc_r.feature_surprises.iter()
                            .take(5)
                            .map(|s| {
                                let direction = if s.error > 0.0 { "↑" } else { "↓" };
                                format!("{} {}{:.2}σ", s.name, direction, s.magnitude)
                            })
                            .collect();
                        println!("       CfC surprised by: {}", top.join(", "));
                    }
                }
            }
        }
    }

    // Flush any open ACI alert zone
    if let Some(start_depth) = aci_alert_zone_start {
        aci_alert_zones_total += 1;
        println!(
            "  ⚡ ACI ZONE #{} | {:.0}–end ft | {} packets | peak {} metrics outside interval",
            aci_alert_zones_total, start_depth,
            aci_alert_zone_packets, aci_alert_zone_peak_outliers
        );
    }

    // ========================================================================
    // Final summary
    // ========================================================================
    println!();
    println!("─────────────────────────────────────────────────────────────────");
    println!("[4/5] REPLAY COMPLETE");
    println!("═════════════════════════════════════════════════════════════════");
    println!();

    let tactical_stats = tactical.stats();
    println!("Pipeline Statistics:");
    println!("  Packets processed:    {}", stats.total_packets);
    println!("  Drilling packets:     {} ({:.1}%)",
        stats.drilling_packets,
        stats.drilling_packets as f64 / stats.total_packets as f64 * 100.0
    );
    println!("  Tactical agent:       {}", tactical_stats);
    println!();

    println!("Physics Summary (drilling only):");
    println!("  Average MSE:          {:.0} psi", stats.avg_mse());
    println!("  Peak MSE:             {:.0} psi", stats.mse_max);
    println!("  Average ROP:          {:.1} ft/hr", stats.avg_rop());
    println!();

    println!("Tickets Generated:      {}", stats.tickets_generated);
    if stats.tickets_generated > 0 {
        println!("  Confirmed:            {} ({:.0}%)",
            stats.tickets_confirmed,
            stats.tickets_confirmed as f64 / stats.tickets_generated as f64 * 100.0
        );
        println!("  Rejected:             {} ({:.0}%)",
            stats.tickets_rejected,
            stats.tickets_rejected as f64 / stats.tickets_generated as f64 * 100.0
        );
        println!("  Uncertain:            {} ({:.0}%)",
            stats.tickets_uncertain,
            stats.tickets_uncertain as f64 / stats.tickets_generated as f64 * 100.0
        );
        println!();

        println!("By Category:");
        if stats.well_control_events > 0 {
            println!("  Well Control:         {}", stats.well_control_events);
        }
        if stats.efficiency_events > 0 {
            println!("  Drilling Efficiency:  {}", stats.efficiency_events);
        }
        if stats.mechanical_events > 0 {
            println!("  Mechanical:           {}", stats.mechanical_events);
        }
        if stats.hydraulics_events > 0 {
            println!("  Hydraulics:           {}", stats.hydraulics_events);
        }
        if stats.formation_events > 0 {
            println!("  Formation:            {}", stats.formation_events);
        }
        println!();

        if let (Some(first), Some(last)) = (stats.depth_at_first_ticket, stats.depth_at_last_ticket) {
            println!("  First ticket depth:   {:.0} ft", first);
            println!("  Last ticket depth:    {:.0} ft", last);
        }
    }

    // ACI Summary
    println!();
    println!("[5/5] ACI Conformal Interval Summary:");
    println!("  Drilling packets with outliers:      {} ({:.1}%)",
        stats.aci_outlier_packets,
        if stats.drilling_packets > 0 { stats.aci_outlier_packets as f64 / stats.drilling_packets as f64 * 100.0 } else { 0.0 }
    );
    println!("  Multi-outlier packets (3+ metrics):  {}", stats.aci_multi_outlier_packets);
    println!("  Alert zones (consecutive clusters):  {}", aci_alert_zones_total);

    // Print final interval state for key metrics
    let key_metrics = [
        (aci::metrics::MSE, "MSE", "psi"),
        (aci::metrics::ROP, "ROP", "ft/hr"),
        (aci::metrics::WOB, "WOB", "klbs"),
        (aci::metrics::SPP, "SPP", "psi"),
        (aci::metrics::TORQUE, "Torque", "kft-lb"),
        (aci::metrics::RPM, "RPM", "RPM"),
        (aci::metrics::ECD, "ECD", "ppg"),
    ];

    println!();
    println!("  Final calibrated intervals (90% target coverage):");
    let aci_tracker = tactical.aci_tracker();
    for (id, name, unit) in &key_metrics {
        if let Some(ci) = aci_tracker.interval(id) {
            println!(
                "    {:<8} [{:>8.1} — {:>8.1}] {} | coverage: {:.1}% | samples: {}",
                name, ci.lower, ci.upper, unit,
                ci.coverage * 100.0,
                aci_tracker.sample_count(id)
            );
        }
    }

    // CfC Neural Network Summary
    println!();
    println!("CfC Neural Network (Shadow Mode):");
    let cfc_net = tactical.cfc_network();
    println!("  Parameters:           {}", cfc_net.num_params());
    println!("  NCP connections:      {}", cfc_net.num_connections());
    println!("  Packets processed:    {}", cfc_net.packets_processed());
    println!("  Training steps:       {}", cfc_net.train_steps());
    println!("  Calibrated:           {}", cfc_net.is_calibrated());
    println!("  Average loss:         {:.6}", cfc_net.avg_loss());
    println!("  Final learning rate:  {:.6}", cfc_net.learning_rate());
    println!("  Final anomaly score:  {:.4}", cfc_net.anomaly_score());
    println!("  Final health score:   {:.4}", cfc_net.health_score());

    if let Some(locked_at) = stats.baseline_locked_at_packet {
        println!();
        println!("Baseline Learning:");
        println!("  Locked after packet:  {}", locked_at);
        println!("  Samples needed:       100 per metric (12 WITS metrics)");
    }

    println!();
    println!("═════════════════════════════════════════════════════════════════");
}
