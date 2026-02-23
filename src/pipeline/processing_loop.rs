//! Unified packet processing loop shared across all input modes.
//!
//! Extracts the duplicated packet -> process -> advisory -> fleet -> log pattern
//! from the former `run_drilling_pipeline`, `run_pipeline_stdin`, and
//! `run_pipeline_wits_tcp` into a single generic [`ProcessingLoop`].

use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use super::source::{PacketEvent, PacketSource};
use super::{AppState, PipelineCoordinator, PipelineStats, SystemStatus};
use crate::config::defaults::ML_HISTORY_BUFFER_SIZE;
#[cfg(feature = "fleet-client")]
use crate::config::defaults::{ECD_REFERENCE_PPG, MSE_EFFICIENCY_DENOMINATOR};
#[cfg(feature = "fleet-client")]
use crate::types::Campaign;
use crate::types::{StrategicAdvisory, WitsPacket};

// ============================================================================
// Post-Process Hooks
// ============================================================================

/// Extension point for any remaining mode-specific per-packet processing.
///
/// Regime stamping, ML history, metrics, and formation-transition tracking all
/// run unconditionally in the core loop for every input mode. This trait exists
/// as an escape hatch for genuinely mode-specific work that may be needed in
/// the future. Pass `()` when no extra processing is required.
pub trait PostProcessHooks: Send + 'static {
    /// Called after the common per-packet work, before advisory handling.
    fn on_packet(
        &mut self,
        packet: &mut WitsPacket,
        coordinator: &PipelineCoordinator,
        state: &mut AppState,
    );
}

/// No-op implementation — use this when no mode-specific post-processing is needed.
impl PostProcessHooks for () {
    fn on_packet(
        &mut self,
        _packet: &mut WitsPacket,
        _coordinator: &PipelineCoordinator,
        _state: &mut AppState,
    ) {
    }
}

// ============================================================================
// Fleet Context
// ============================================================================

/// Fleet client context for enqueueing advisory events.
#[cfg(feature = "fleet-client")]
pub struct FleetContext {
    pub queue: Arc<crate::fleet::UploadQueue>,
    pub rig_id: String,
    pub well_id: String,
}

// ============================================================================
// Processing Loop
// ============================================================================

/// Owns all state needed for the unified packet processing loop.
///
/// Built with [`new()`](ProcessingLoop::new), optionally enriched with
/// [`with_fleet()`](ProcessingLoop::with_fleet), then consumed by
/// [`run()`](ProcessingLoop::run).
pub struct ProcessingLoop<H: PostProcessHooks> {
    coordinator: PipelineCoordinator,
    app_state: Arc<RwLock<AppState>>,
    hooks: H,
    cancel_token: CancellationToken,
    /// Tracks WOB/RPM changes to stamp `seconds_since_param_change` on every packet.
    param_tracker: crate::ml_engine::param_change_tracker::ParamChangeTracker,
    #[cfg(feature = "fleet-client")]
    fleet_ctx: Option<FleetContext>,
}

impl<H: PostProcessHooks> ProcessingLoop<H> {
    pub fn new(
        coordinator: PipelineCoordinator,
        app_state: Arc<RwLock<AppState>>,
        hooks: H,
        cancel_token: CancellationToken,
    ) -> Self {
        Self {
            coordinator,
            app_state,
            hooks,
            cancel_token,
            param_tracker: crate::ml_engine::param_change_tracker::ParamChangeTracker::new(),
            #[cfg(feature = "fleet-client")]
            fleet_ctx: None,
        }
    }

    /// Attach fleet client context for advisory uploads.
    #[cfg(feature = "fleet-client")]
    pub fn with_fleet(mut self, ctx: FleetContext) -> Self {
        self.fleet_ctx = Some(ctx);
        self
    }

    /// Run the processing loop until the source is exhausted or cancellation.
    ///
    /// Returns final pipeline statistics.
    pub async fn run<S: PacketSource>(mut self, source: &mut S) -> PipelineStats {
        let mut packets_processed = 0u64;
        let mut advisories_generated = 0u64;

        info!(
            "📊 Processing WITS packets from {}...",
            source.source_name()
        );
        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        loop {
            let event = tokio::select! {
                _ = self.cancel_token.cancelled() => {
                    info!("[PacketProcessor] Shutdown signal received");
                    break;
                }
                result = source.next_packet() => {
                    match result {
                        Ok(ev) => ev,
                        Err(e) => {
                            warn!("[PacketProcessor] Source error: {}", e);
                            break;
                        }
                    }
                }
            };

            let packet = match event {
                PacketEvent::Packet(p) => p,
                PacketEvent::Eof => {
                    info!(
                        "[PacketProcessor] Source reached end ({} packets processed)",
                        packets_processed
                    );
                    break;
                }
            };

            packets_processed += 1;

            // Update app state with incoming data and read current campaign
            let campaign = {
                let mut state = self.app_state.write().await;
                state.current_rpm = packet.rpm;
                state.samples_collected = packets_processed as usize;
                state.total_analyses = packets_processed;
                state.last_analysis_time = Some(chrono::Utc::now());
                state.status = SystemStatus::Monitoring;
                state.latest_wits_packet = Some(packet.clone());
                state.campaign
            };

            // Process through the 10-phase pipeline
            let mut packet = packet;
            let advisory = self
                .coordinator
                .process_packet(&mut packet, campaign)
                .await;

            // Per-packet post-processing — runs for ALL input modes.
            {
                let mut state = self.app_state.write().await;

                // Stamp regime_id from CfC motor output clustering (result of
                // the *previous* packet's CfC pass, which is the latest stable value).
                packet.regime_id = self.coordinator.latest_regime_id();
                packet.seconds_since_param_change =
                    self.param_tracker
                        .update(packet.timestamp, packet.wob, packet.rpm);

                // Add to ML history buffer (keep 2 hours at 1 Hz)
                if state.wits_history.len() >= ML_HISTORY_BUFFER_SIZE {
                    state.wits_history.pop_front();
                }
                state.wits_history.push_back(packet.clone());
                state.regime_centroids = self.coordinator.regime_centroids();

                // Store latest drilling metrics (includes operation classification)
                if let Some(metrics) = self.coordinator.get_latest_metrics() {
                    state.latest_drilling_metrics = Some(metrics.clone());
                }

                // Store CfC formation transition event (if any)
                if let Some(event) = self.coordinator.tactical_agent().latest_formation_transition() {
                    state.formation_transition_timestamps.push(event.timestamp);
                    state.latest_formation_transition = Some(event.clone());
                }

                // Any remaining mode-specific hooks (no-op for () — see PostProcessHooks)
                self.hooks.on_packet(&mut packet, &self.coordinator, &mut state);
            }

            if let Some(ref adv) = advisory {
                advisories_generated += 1;

                // Update dashboard state
                {
                    let mut state = self.app_state.write().await;
                    state.latest_advisory = Some(adv.clone());
                }

                // Persist to history storage
                if let Err(e) = crate::storage::history::store_report(adv) {
                    warn!("Failed to persist advisory to history: {}", e);
                }

                // Enqueue for fleet upload
                #[cfg(feature = "fleet-client")]
                if let Some(ref ctx) = self.fleet_ctx {
                    fleet_enqueue_advisory(
                        &ctx.queue, adv, &packet, &ctx.rig_id, &ctx.well_id, "Volve", campaign,
                    );
                }

                // Log advisory summary
                log_advisory(advisories_generated, adv);
            }

            // Progress indicator every 10 packets
            if advisory.is_none() && packets_processed % 10 == 0 {
                let stats = self.coordinator.get_stats();
                info!(
                    "📈 Progress: {} packets | Advisories: {} | Buffer: {}/60",
                    packets_processed, stats.strategic_analyses, stats.history_buffer_size
                );
            }
        }

        // Final statistics
        let stats = self.coordinator.get_stats();
        info!("");
        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        info!("📊 FINAL STATISTICS");
        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        info!("   Packets Processed:    {}", stats.packets_processed);
        info!("   Tickets Created:      {}", stats.tickets_created);
        info!("   Tickets Verified:     {}", stats.tickets_verified);
        info!("   Tickets Rejected:     {}", stats.tickets_rejected);
        info!("   Advisories Generated: {}", stats.strategic_analyses);
        info!(
            "   History Buffer Size:  {}/60",
            stats.history_buffer_size
        );
        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        stats
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Log an advisory summary to tracing output.
fn log_advisory(count: u64, adv: &StrategicAdvisory) {
    info!(
        "🎯 ADVISORY #{}: {:?} | Efficiency: {}%",
        count, adv.risk_level, adv.efficiency_score
    );
    info!(
        "   Recommendation: {}",
        truncate_str(&adv.recommendation, 70)
    );
    for vote in &adv.votes {
        info!(
            "   {} ({:.0}%): {} - {}",
            vote.specialist,
            vote.weight * 100.0,
            vote.vote,
            truncate_str(&vote.reasoning, 50)
        );
    }
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
}

/// Truncate a string for display.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

/// Enqueue a qualifying advisory as a FleetEvent for upload to the hub.
///
/// Only AMBER/RED advisories are uploaded (see `fleet::types::should_upload`).
/// The event is written to the disk-backed queue; the uploader task drains it.
#[cfg(feature = "fleet-client")]
fn fleet_enqueue_advisory(
    queue: &crate::fleet::UploadQueue,
    advisory: &StrategicAdvisory,
    packet: &WitsPacket,
    rig_id: &str,
    well_id: &str,
    field: &str,
    campaign: Campaign,
) {
    use crate::fleet::types::{should_upload, EventOutcome, FleetEvent, HistorySnapshot};

    if !should_upload(advisory) {
        return;
    }

    // Use current wall-clock time so the hub's 7-day window check passes
    // (historical replay data has timestamps from the past).
    let now = chrono::Utc::now().timestamp() as u64;

    let snapshot = HistorySnapshot {
        timestamp: now,
        depth: packet.bit_depth,
        rop: packet.rop,
        wob: packet.wob,
        rpm: packet.rpm,
        torque: packet.torque,
        spp: packet.spp,
        flow_in: packet.flow_in,
        flow_out: packet.flow_out,
        mse: packet.mse,
        mse_efficiency: 100.0 - (packet.mse / MSE_EFFICIENCY_DENOMINATOR * 100.0).min(100.0),
        d_exponent: packet.d_exponent,
        flow_balance: packet.flow_in - packet.flow_out,
        pit_rate: packet.pit_volume_change,
        ecd_margin: ECD_REFERENCE_PPG - packet.ecd,
        gas_units: packet.gas_units,
    };

    let event = FleetEvent {
        id: format!("{}-{}", rig_id, advisory.timestamp),
        rig_id: rig_id.to_string(),
        well_id: well_id.to_string(),
        field: field.to_string(),
        campaign,
        advisory: advisory.clone(),
        history_window: vec![snapshot],
        outcome: EventOutcome::Pending,
        notes: None,
        depth: packet.bit_depth,
        timestamp: now,
    };

    if let Err(e) = queue.enqueue(&event) {
        warn!("Failed to enqueue fleet event: {}", e);
    }
}
