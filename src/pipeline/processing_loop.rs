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
// Processing Loop
// ============================================================================

/// Owns all state needed for the unified packet processing loop.
///
/// Built with [`new()`](ProcessingLoop::new), then consumed by
/// [`run()`](ProcessingLoop::run).
pub struct ProcessingLoop<H: PostProcessHooks> {
    coordinator: PipelineCoordinator,
    app_state: Arc<RwLock<AppState>>,
    hooks: H,
    cancel_token: CancellationToken,
    /// Tracks WOB/RPM changes to stamp `seconds_since_param_change` on every packet.
    param_tracker: crate::ml_engine::param_change_tracker::ParamChangeTracker,
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
        }
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
            let advisory = self.coordinator.process_packet(&mut packet, campaign).await;

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

                // Store damping monitor snapshot for API visibility
                state.damping_monitor_snapshot = Some(self.coordinator.damping_monitor_snapshot());

                // Store CfC formation transition event (if any)
                if let Some(event) = self
                    .coordinator
                    .tactical_agent()
                    .latest_formation_transition()
                {
                    state.formation_transition_timestamps.push(event.timestamp);
                    // Cap at 1000 entries to prevent unbounded growth
                    if state.formation_transition_timestamps.len() > 1000 {
                        let excess = state.formation_transition_timestamps.len() - 1000;
                        state.formation_transition_timestamps.drain(..excess);
                    }
                    state.latest_formation_transition = Some(event.clone());
                }

                // Any remaining mode-specific hooks (no-op for () — see PostProcessHooks)
                self.hooks
                    .on_packet(&mut packet, &self.coordinator, &mut state);
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
        info!("   History Buffer Size:  {}/60", stats.history_buffer_size);
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

/// Truncate a string for display (UTF-8 safe).
fn truncate_str(s: &str, max_len: usize) -> String {
    if max_len < 4 || s.len() <= max_len {
        s.to_string()
    } else {
        let end = max_len.saturating_sub(3);
        // Find the nearest char boundary at or before `end`
        let boundary = s.floor_char_boundary(end);
        format!("{}...", &s[..boundary])
    }
}
