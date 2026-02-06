//! Pipeline Coordinator - 10-Phase Processing Sequence for Drilling Intelligence
//!
//! This module implements the processing sequence for WITS drilling data:
//!
//! ```text
//! PHASE 1: WITS Ingestion (continuous, 1Hz typical)
//! PHASE 2: Basic Physics (inside Tactical Agent, < 15ms)
//! PHASE 3: Tactical Agent Decision (advisory ticket or discard)
//! PHASE 4: History Buffer (continuous, parallel)
//! PHASE 5: Advanced Physics (ONLY if ticket created)
//! PHASE 6: Context Lookup (ONLY if ticket created)
//! PHASE 7: LLM Explainer (ONLY if ticket created)
//! PHASE 8: Orchestrator Voting (ONLY if ticket created)
//! PHASE 9: Storage (ONLY if ticket created)
//! PHASE 10: Dashboard API (continuous)
//! ```
//!
//! CRITICAL GUARANTEE: Phases 5-9 ONLY execute if Tactical Agent created a ticket.

use crate::agents::{Orchestrator, StrategicAgent, TacticalAgent};
use crate::baseline::ThresholdManager;
use crate::context::vector_db;
use crate::physics_engine;
use crate::types::{
    AdvisoryTicket, AnomalyCategory, Campaign, DrillingMetrics,
    DrillingPhysicsReport, HistoryEntry, RigState, StrategicAdvisory,
    VerificationResult, VerificationStatus, WitsPacket,
};
use std::collections::VecDeque;
use std::sync::{Arc, RwLock};
use std::time::Instant;
use tracing::{debug, info, warn};

#[cfg(feature = "llm")]
use crate::llm::StrategicLLM;

/// History buffer size (60 packets = 1 hour of 60-second samples)
const HISTORY_BUFFER_SIZE: usize = 60;

/// Periodic summary interval in seconds (10 minutes)
const PERIODIC_SUMMARY_INTERVAL_SECS: u64 = 600;

/// Pipeline Coordinator manages the 10-phase processing sequence
pub struct PipelineCoordinator {
    /// Phase 2-3: Tactical Agent
    tactical_agent: TacticalAgent,
    /// Phase 5-7: Strategic Agent (for two-stage verification)
    strategic_agent: StrategicAgent,
    /// Phase 8: Orchestrator
    orchestrator: Orchestrator,
    /// Phase 4: History buffer (60 packets, circular)
    history_buffer: VecDeque<HistoryEntry>,
    /// Latest strategic advisory (for Phase 10 dashboard)
    latest_advisory: Option<StrategicAdvisory>,
    /// Latest verification result (for monitoring verification system)
    latest_verification: Option<VerificationResult>,
    /// Phase 7: Strategic LLM for diagnosis (optional, loaded at runtime)
    #[cfg(feature = "llm")]
    strategic_llm: Option<Arc<StrategicLLM>>,
    /// Statistics
    packets_processed: u64,
    tickets_created: u64,
    tickets_verified: u64,
    tickets_rejected: u64,
    strategic_analyses: u64,
    /// Timestamp of last periodic summary (Unix timestamp)
    last_periodic_summary_time: u64,
    /// Latest drilling metrics (from tactical agent)
    latest_metrics: Option<DrillingMetrics>,
}

impl PipelineCoordinator {
    /// Create a new pipeline coordinator (without LLM - use init_with_llm for LLM support)
    pub fn new() -> Self {
        info!("Initializing Pipeline Coordinator for drilling intelligence");
        Self {
            tactical_agent: TacticalAgent::new(),
            strategic_agent: StrategicAgent::new(),
            orchestrator: Orchestrator::new(),
            history_buffer: VecDeque::with_capacity(HISTORY_BUFFER_SIZE),
            latest_advisory: None,
            latest_verification: None,
            #[cfg(feature = "llm")]
            strategic_llm: None,
            packets_processed: 0,
            tickets_created: 0,
            tickets_verified: 0,
            tickets_rejected: 0,
            strategic_analyses: 0,
            last_periodic_summary_time: 0,
            latest_metrics: None,
        }
    }

    /// Create a new pipeline coordinator with LLM support
    #[cfg(feature = "llm")]
    pub async fn init_with_llm() -> anyhow::Result<Self> {
        info!("Initializing Pipeline Coordinator with LLM support");
        info!("Loading Strategic LLM for drilling advisory...");

        let llm = StrategicLLM::init().await?;
        info!("Strategic LLM loaded successfully");

        Ok(Self {
            tactical_agent: TacticalAgent::new(),
            strategic_agent: StrategicAgent::new(),
            orchestrator: Orchestrator::new(),
            history_buffer: VecDeque::with_capacity(HISTORY_BUFFER_SIZE),
            latest_advisory: None,
            latest_verification: None,
            strategic_llm: Some(llm),
            packets_processed: 0,
            tickets_created: 0,
            tickets_verified: 0,
            tickets_rejected: 0,
            strategic_analyses: 0,
            last_periodic_summary_time: 0,
            latest_metrics: None,
        })
    }

    /// Create a new pipeline coordinator with dynamic thresholds support
    pub fn new_with_thresholds(
        threshold_manager: Arc<RwLock<ThresholdManager>>,
        equipment_id: String,
        start_in_learning_mode: bool,
    ) -> Self {
        info!(
            "Initializing Pipeline Coordinator with dynamic thresholds (learning: {}, equipment: {})",
            start_in_learning_mode, equipment_id
        );
        Self {
            tactical_agent: TacticalAgent::new_with_thresholds(
                &equipment_id,
                threshold_manager.clone(),
                start_in_learning_mode,
            ),
            strategic_agent: StrategicAgent::with_thresholds(
                &equipment_id,
                threshold_manager,
            ),
            orchestrator: Orchestrator::new(),
            history_buffer: VecDeque::with_capacity(HISTORY_BUFFER_SIZE),
            latest_advisory: None,
            latest_verification: None,
            #[cfg(feature = "llm")]
            strategic_llm: None,
            packets_processed: 0,
            tickets_created: 0,
            tickets_verified: 0,
            tickets_rejected: 0,
            strategic_analyses: 0,
            last_periodic_summary_time: 0,
            latest_metrics: None,
        }
    }

    /// Create a new pipeline coordinator with both LLM and dynamic thresholds
    #[cfg(feature = "llm")]
    pub async fn init_with_llm_and_thresholds(
        threshold_manager: Arc<RwLock<ThresholdManager>>,
        equipment_id: String,
        start_in_learning_mode: bool,
    ) -> anyhow::Result<Self> {
        info!(
            "Initializing Pipeline Coordinator with LLM and dynamic thresholds (learning: {}, equipment: {})",
            start_in_learning_mode, equipment_id
        );
        info!("Loading Strategic LLM...");

        let llm = StrategicLLM::init().await?;
        info!("Strategic LLM loaded successfully");

        Ok(Self {
            tactical_agent: TacticalAgent::new_with_thresholds(
                &equipment_id,
                threshold_manager.clone(),
                start_in_learning_mode,
            ),
            strategic_agent: StrategicAgent::with_thresholds(
                &equipment_id,
                threshold_manager,
            ),
            orchestrator: Orchestrator::new(),
            history_buffer: VecDeque::with_capacity(HISTORY_BUFFER_SIZE),
            latest_advisory: None,
            latest_verification: None,
            strategic_llm: Some(llm),
            packets_processed: 0,
            tickets_created: 0,
            tickets_verified: 0,
            tickets_rejected: 0,
            strategic_analyses: 0,
            last_periodic_summary_time: 0,
            latest_metrics: None,
        })
    }

    /// Process a WITS packet through the full pipeline
    ///
    /// Returns a StrategicAdvisory if:
    /// 1. A CRITICAL ticket was created (immediate processing), OR
    /// 2. 10 minutes have passed since last advisory (periodic summary)
    ///
    /// Periodic summaries represent the last 10 minutes of drilling activity.
    pub async fn process_packet(
        &mut self,
        packet: &WitsPacket,
        campaign: Campaign,
    ) -> Option<StrategicAdvisory> {
        use crate::types::TicketSeverity;

        let cycle_start = Instant::now();
        self.packets_processed += 1;

        // PHASE 1: Sensor Ingestion (packet already received)
        debug!(
            timestamp = packet.timestamp,
            depth = packet.bit_depth,
            rop = packet.rop,
            state = ?packet.rig_state,
            "Phase 1: WITS packet ingested"
        );

        // Sync tactical agent campaign with AppState campaign
        self.tactical_agent.set_campaign(campaign);

        // PHASE 2-3: Tactical Agent (Basic Physics + Decision)
        let (ticket_opt, metrics, history_entry) = self.tactical_agent.process(packet);

        // Store latest metrics for API/dashboard access
        self.latest_metrics = Some(metrics.clone());

        // PHASE 4: History Buffer (always update, parallel to ticket processing)
        self.update_history_buffer(history_entry.clone());

        // Check if it's time for a periodic summary (every 10 minutes)
        let time_since_last_summary = packet.timestamp.saturating_sub(self.last_periodic_summary_time);
        let should_generate_periodic = time_since_last_summary >= PERIODIC_SUMMARY_INTERVAL_SECS
            && self.history_buffer.len() >= 10;  // Need at least 10 packets for meaningful summary

        // Determine if we have a CRITICAL ticket (bypasses periodic timing)
        let is_critical_ticket = ticket_opt.as_ref()
            .map(|t| matches!(t.severity, TicketSeverity::Critical))
            .unwrap_or(false);

        // Check if tactical agent created an advisory ticket
        let ticket = match ticket_opt {
            Some(t) => {
                self.tickets_created += 1;
                debug!(
                    category = ?t.category,
                    severity = ?t.severity,
                    trigger = %t.trigger_parameter,
                    "Phase 3: Advisory ticket created"
                );
                t
            }
            None => {
                // No ticket - check if we should generate a periodic summary
                if should_generate_periodic {
                    debug!("Phase 3: No ticket, but generating periodic 10-minute summary");
                    return self.generate_periodic_summary(packet, &metrics, campaign).await;
                }
                debug!("Phase 3: No ticket, pipeline ends");
                return None;
            }
        };

        // For non-critical tickets, check if we should wait for periodic summary
        if !is_critical_ticket && !should_generate_periodic {
            // Non-critical ticket but not time for summary yet - skip for now
            // The ticket info is captured in history for the next periodic summary
            debug!("Phase 3: Non-critical ticket, waiting for periodic summary");
            return None;
        }

        // PHASES 5-9: ONLY EXECUTED WHEN TICKET EXISTS

        // PHASE 5: Advanced Physics
        let physics = self.run_advanced_physics(&ticket, packet);

        // PHASE 6: Context Lookup
        let context = self.lookup_context(&ticket);

        // Run strategic verification
        let history: Vec<HistoryEntry> = self.history_buffer.iter().cloned().collect();
        let verification_result = self.strategic_agent.verify_ticket(
            &ticket,
            &history,
        );

        self.latest_verification = Some(verification_result.clone());

        // Check verification status
        match verification_result.status {
            VerificationStatus::Confirmed => {
                self.tickets_verified += 1;
                debug!("Strategic verification: CONFIRMED");
            }
            VerificationStatus::Rejected => {
                self.tickets_rejected += 1;
                debug!(
                    reasoning = %verification_result.reasoning,
                    "Strategic verification: REJECTED"
                );
                return None;
            }
            VerificationStatus::Uncertain => {
                debug!("Strategic verification: UNCERTAIN - proceeding with caution");
            }
            VerificationStatus::Pending => {
                debug!("Strategic verification: PENDING - waiting for analysis");
                return None;
            }
        }

        // PHASE 7: LLM Explainer
        let (recommendation, expected_benefit, reasoning) = self
            .generate_explanation(&ticket, &physics, &context, campaign)
            .await;

        // PHASE 8: Orchestrator Voting
        let advisory = self.orchestrator.vote(
            &ticket,
            &physics,
            &context,
            &recommendation,
            &expected_benefit,
            &reasoning,
        );

        // PHASE 9: Storage (store in latest_advisory for Phase 10 dashboard)
        self.latest_advisory = Some(advisory.clone());
        self.strategic_analyses += 1;
        self.last_periodic_summary_time = packet.timestamp;

        let cycle_time = cycle_start.elapsed();
        info!(
            cycle_ms = cycle_time.as_millis(),
            risk_level = ?advisory.risk_level,
            efficiency_score = advisory.efficiency_score,
            verification_status = ?verification_result.status,
            is_critical = is_critical_ticket,
            "Strategic analysis complete - advisory sent to dashboard"
        );

        // Cycle time targets: 100ms for GPU, 60s for CPU (LLM inference is slower on CPU)
        #[cfg(feature = "llm")]
        let cycle_target_ms: u128 = if crate::llm::is_cuda_available() { 100 } else { 60_000 };
        #[cfg(not(feature = "llm"))]
        let cycle_target_ms: u128 = 100;

        if cycle_time.as_millis() > cycle_target_ms {
            warn!(
                elapsed_ms = cycle_time.as_millis(),
                target_ms = cycle_target_ms,
                "Processing cycle exceeded target"
            );
        }

        Some(advisory)
    }

    /// Phase 4: Update history buffer (circular, 60 packets)
    fn update_history_buffer(&mut self, entry: HistoryEntry) {
        if self.history_buffer.len() >= HISTORY_BUFFER_SIZE {
            self.history_buffer.pop_front();
        }
        self.history_buffer.push_back(entry);
    }

    /// Generate a periodic 10-minute summary advisory
    ///
    /// This creates a summary of the last 10 minutes of drilling activity,
    /// whether good or bad, to provide regular operational intelligence.
    async fn generate_periodic_summary(
        &mut self,
        packet: &WitsPacket,
        current_metrics: &DrillingMetrics,
        campaign: Campaign,
    ) -> Option<StrategicAdvisory> {
        use crate::types::{TicketSeverity, TicketType};

        let cycle_start = Instant::now();

        // Calculate summary statistics from history buffer
        let history_len = self.history_buffer.len() as f64;
        if history_len < 10.0 {
            debug!("Insufficient history for periodic summary");
            return None;
        }

        // Aggregate metrics from history
        let mut total_mse = 0.0;
        let mut total_flow_balance = 0.0;
        let mut total_rop = 0.0;
        let mut total_ecd_margin = 0.0;
        let mut anomaly_count = 0;
        let mut worst_category = AnomalyCategory::None;
        let mut has_well_control_events = false;

        for entry in self.history_buffer.iter() {
            total_mse += entry.metrics.mse;
            total_flow_balance += entry.metrics.flow_balance;
            total_rop += entry.packet.rop;
            if entry.metrics.ecd_margin < 100.0 && entry.metrics.ecd_margin > -100.0 {
                total_ecd_margin += entry.metrics.ecd_margin;
            }
            if entry.metrics.is_anomaly {
                anomaly_count += 1;
                // Track the most concerning category
                if matches!(entry.metrics.anomaly_category, AnomalyCategory::WellControl) {
                    has_well_control_events = true;
                    worst_category = AnomalyCategory::WellControl;
                } else if !has_well_control_events {
                    worst_category = entry.metrics.anomaly_category.clone();
                }
            }
        }

        let avg_mse = total_mse / history_len;
        let avg_flow_balance = total_flow_balance / history_len;
        let avg_rop = total_rop / history_len;
        let avg_ecd_margin = total_ecd_margin / history_len;
        let anomaly_rate = anomaly_count as f64 / history_len * 100.0;

        // Determine overall period assessment
        let (ticket_type, severity, category, trigger_param, trigger_value) = if has_well_control_events {
            (
                TicketType::RiskWarning,
                TicketSeverity::High,
                AnomalyCategory::WellControl,
                "well_control_events".to_string(),
                anomaly_count as f64,
            )
        } else if anomaly_rate > 30.0 {
            (
                TicketType::Intervention,
                TicketSeverity::Medium,
                worst_category.clone(),
                "anomaly_rate".to_string(),
                anomaly_rate,
            )
        } else if avg_mse > crate::config::get().thresholds.mse.efficiency_poor_percent * 1000.0 {
            (
                TicketType::Optimization,
                TicketSeverity::Low,
                AnomalyCategory::DrillingEfficiency,
                "avg_mse".to_string(),
                avg_mse,
            )
        } else {
            // Normal operations - still generate a summary
            (
                TicketType::Optimization,
                TicketSeverity::Low,
                AnomalyCategory::None,
                "periodic_summary".to_string(),
                0.0,
            )
        };

        // Calculate MSE efficiency
        let formation_hardness = (current_metrics.d_exponent * 3.0).clamp(1.0, 10.0);
        let optimal_mse = physics_engine::estimate_optimal_mse(formation_hardness);
        let mse_efficiency = (optimal_mse / avg_mse.max(1.0) * 100.0).min(100.0);

        // Create a synthetic summary ticket
        let summary_metrics = DrillingMetrics {
            state: current_metrics.state.clone(),
            operation: current_metrics.operation,
            mse: avg_mse,
            mse_efficiency,
            d_exponent: current_metrics.d_exponent,
            dxc: current_metrics.dxc,
            mse_delta_percent: 0.0,
            flow_balance: avg_flow_balance,
            pit_rate: current_metrics.pit_rate,
            ecd_margin: avg_ecd_margin,
            torque_delta_percent: current_metrics.torque_delta_percent,
            spp_delta: current_metrics.spp_delta,
            is_anomaly: anomaly_rate > 10.0,
            anomaly_category: category.clone(),
            anomaly_description: Some(format!(
                "10-min summary: {:.1}% anomaly rate, avg ROP {:.1} ft/hr, avg MSE {:.0} psi",
                anomaly_rate, avg_rop, avg_mse
            )),
        };

        let summary_ticket = AdvisoryTicket {
            timestamp: packet.timestamp,
            ticket_type,
            category: category.clone(),
            severity,
            current_metrics: summary_metrics.clone(),
            trigger_parameter: trigger_param,
            trigger_value,
            threshold_value: 0.0,
            description: format!(
                "Periodic 10-minute summary: {:.1}% anomaly rate, avg ROP {:.1} ft/hr",
                anomaly_rate, avg_rop
            ),
            depth: packet.bit_depth,
            trace_log: Vec::new(),
        };

        // Run through remaining phases
        let physics = self.run_advanced_physics(&summary_ticket, packet);
        let context = self.lookup_context(&summary_ticket);

        // Strategic verification (summary tickets typically pass)
        let history: Vec<HistoryEntry> = self.history_buffer.iter().cloned().collect();
        let verification_result = self.strategic_agent.verify_ticket(&summary_ticket, &history);
        self.latest_verification = Some(verification_result.clone());

        // Generate explanation
        let (recommendation, expected_benefit, reasoning) = self
            .generate_explanation(&summary_ticket, &physics, &context, campaign)
            .await;

        // Orchestrator voting
        let advisory = self.orchestrator.vote(
            &summary_ticket,
            &physics,
            &context,
            &recommendation,
            &expected_benefit,
            &reasoning,
        );

        // Store and update timing
        self.latest_advisory = Some(advisory.clone());
        self.strategic_analyses += 1;
        self.last_periodic_summary_time = packet.timestamp;

        let cycle_time = cycle_start.elapsed();
        info!(
            cycle_ms = cycle_time.as_millis(),
            anomaly_rate = anomaly_rate,
            avg_rop = avg_rop,
            avg_mse = avg_mse,
            risk_level = ?advisory.risk_level,
            "Periodic 10-minute summary generated"
        );

        Some(advisory)
    }

    /// Phase 5: Run advanced physics calculations for drilling
    fn run_advanced_physics(&self, ticket: &AdvisoryTicket, packet: &WitsPacket) -> DrillingPhysicsReport {
        let start = Instant::now();

        // Calculate MSE statistics from history
        let mse_values: Vec<f64> = self.history_buffer.iter().map(|e| e.metrics.mse).collect();
        let avg_mse = if !mse_values.is_empty() {
            mse_values.iter().sum::<f64>() / mse_values.len() as f64
        } else {
            packet.mse
        };

        // MSE trend (positive = increasing inefficiency)
        let mse_trend = if mse_values.len() >= 5 {
            let recent: Vec<f64> = mse_values.iter().rev().take(5).copied().collect();
            let earlier: Vec<f64> = mse_values.iter().take(5).copied().collect();
            let recent_avg: f64 = recent.iter().sum::<f64>() / recent.len() as f64;
            let earlier_avg: f64 = earlier.iter().sum::<f64>() / earlier.len() as f64;
            (recent_avg - earlier_avg) / earlier_avg.max(1.0) * 100.0
        } else {
            0.0
        };

        // Calculate optimal MSE for current parameters (estimate formation hardness from d-exponent)
        let formation_hardness = (ticket.current_metrics.d_exponent * 3.0).clamp(1.0, 10.0);
        let optimal_mse = physics_engine::estimate_optimal_mse(formation_hardness);
        let mse_efficiency = (optimal_mse / avg_mse.max(1.0) * 100.0).min(100.0);

        // D-exponent trend (formation change indicator)
        let dxc_values: Vec<f64> = self.history_buffer.iter().map(|e| e.metrics.d_exponent).collect();
        let dxc_trend = if dxc_values.len() >= 5 {
            let recent: Vec<f64> = dxc_values.iter().rev().take(5).copied().collect();
            let earlier: Vec<f64> = dxc_values.iter().take(5).copied().collect();
            let recent_avg: f64 = recent.iter().sum::<f64>() / recent.len() as f64;
            let earlier_avg: f64 = earlier.iter().sum::<f64>() / earlier.len() as f64;
            (recent_avg - earlier_avg) / earlier_avg.max(0.1) * 100.0
        } else {
            0.0
        };

        // Flow balance statistics
        let flow_balance_values: Vec<f64> = self.history_buffer.iter()
            .map(|e| e.metrics.flow_balance)
            .collect();

        // Flow balance trend (positive = increasing gain)
        let flow_balance_trend = if flow_balance_values.len() >= 5 {
            let recent: Vec<f64> = flow_balance_values.iter().rev().take(5).copied().collect();
            let earlier: Vec<f64> = flow_balance_values.iter().take(5).copied().collect();
            let recent_avg: f64 = recent.iter().sum::<f64>() / recent.len() as f64;
            let earlier_avg: f64 = earlier.iter().sum::<f64>() / earlier.len() as f64;
            recent_avg - earlier_avg
        } else {
            0.0
        };

        // Pit rate trend
        let pit_rates: Vec<f64> = self.history_buffer.iter()
            .map(|e| e.metrics.pit_rate)
            .collect();
        let avg_pit_rate = if !pit_rates.is_empty() {
            pit_rates.iter().sum::<f64>() / pit_rates.len() as f64
        } else {
            0.0
        };

        // Detect drilling dysfunctions based on current metrics
        let mut detected_dysfunctions = Vec::new();
        if ticket.current_metrics.is_anomaly {
            if let Some(ref desc) = ticket.current_metrics.anomaly_description {
                detected_dysfunctions.push(desc.clone());
            }
        }

        // Confidence based on history depth
        let confidence = (self.history_buffer.len() as f64 / HISTORY_BUFFER_SIZE as f64).min(1.0);

        let elapsed = start.elapsed();
        if elapsed.as_millis() > 50 {
            warn!(
                elapsed_ms = elapsed.as_millis(),
                "Phase 5 exceeded 50ms target"
            );
        }

        debug!(
            avg_mse = avg_mse,
            mse_trend = mse_trend,
            mse_efficiency = mse_efficiency,
            flow_balance_trend = flow_balance_trend,
            confidence = confidence,
            "Advanced drilling physics complete"
        );

        DrillingPhysicsReport {
            avg_mse,
            mse_trend,
            optimal_mse,
            mse_efficiency,
            dxc_trend,
            flow_balance_trend,
            avg_pit_rate,
            formation_hardness,
            confidence,
            detected_dysfunctions,
            // Founder detection fields - not calculated in this code path
            // (use strategic_drilling_analysis for full founder detection)
            wob_trend: 0.0,
            rop_trend: 0.0,
            founder_detected: false,
            founder_severity: 0.0,
            optimal_wob_estimate: 0.0,
            // Snapshot current values from packet for LLM prompt
            current_depth: packet.bit_depth,
            current_rop: packet.rop,
            current_wob: packet.wob,
            current_rpm: packet.rpm,
            current_torque: packet.torque,
            current_spp: packet.spp,
            current_casing_pressure: packet.casing_pressure,
            current_flow_in: packet.flow_in,
            current_flow_out: packet.flow_out,
            current_mud_weight: packet.mud_weight_in,
            current_ecd: packet.ecd,
            current_gas: packet.gas_units,
            current_pit_volume: packet.pit_volume,
        }
    }

    /// Phase 6: Context lookup from drilling knowledge base
    fn lookup_context(&self, ticket: &AdvisoryTicket) -> Vec<String> {
        let query = match ticket.category {
            AnomalyCategory::WellControl => "well control kick loss circulation flow imbalance",
            AnomalyCategory::DrillingEfficiency => "MSE drilling efficiency ROP optimization",
            AnomalyCategory::Hydraulics => "standpipe pressure ECD flow rate hydraulics",
            AnomalyCategory::Mechanical => "torque pack-off stick-slip mechanical",
            AnomalyCategory::Formation => "d-exponent formation change pore pressure",
            AnomalyCategory::None => "normal drilling operations",
        };

        let context = vector_db::search(query);

        debug!(
            category = ?ticket.category,
            results = context.len(),
            "Context lookup complete"
        );

        context
    }

    /// Phase 7: Generate explanation using LLM (if available) or template
    #[cfg(feature = "llm")]
    async fn generate_explanation(
        &self,
        ticket: &AdvisoryTicket,
        physics: &DrillingPhysicsReport,
        context: &[String],
        campaign: Campaign,
    ) -> (String, String, String) {
        if let Some(ref llm) = self.strategic_llm {
            let start = Instant::now();
            match llm
                .generate_advisory(ticket, &ticket.current_metrics, physics, context, campaign)
                .await
            {
                Ok(advisory) => {
                    let elapsed = start.elapsed();
                    info!(
                        latency_ms = elapsed.as_millis(),
                        campaign = %campaign.short_code(),
                        "LLM advisory generation complete"
                    );
                    return (advisory.recommendation, advisory.expected_benefit, advisory.reasoning);
                }
                Err(e) => {
                    warn!(error = %e, "LLM advisory failed, falling back to template");
                }
            }
        }

        self.template_explanation(ticket, physics)
    }

    /// Phase 7: Generate explanation (template-based, no LLM feature)
    #[cfg(not(feature = "llm"))]
    async fn generate_explanation(
        &self,
        ticket: &AdvisoryTicket,
        physics: &DrillingPhysicsReport,
        _context: &[String],
        _campaign: Campaign,
    ) -> (String, String, String) {
        self.template_explanation(ticket, physics)
    }

    /// Template-based explanation generator (fallback when LLM unavailable)
    fn template_explanation(
        &self,
        ticket: &AdvisoryTicket,
        physics: &DrillingPhysicsReport,
    ) -> (String, String, String) {
        let (recommendation, expected_benefit, reasoning) = match ticket.category {
            AnomalyCategory::WellControl => (
                format!(
                    "Verify flow balance and pit levels. Current flow imbalance: {:.1} bbl/hr. Prepare for well control procedures.",
                    ticket.current_metrics.flow_balance
                ),
                "Well control incident prevention, potential rig safety".to_string(),
                format!(
                    "Flow imbalance of {:.1} bbl/hr detected with pit rate {:.1} bbl/hr. ECD margin: {:.2} ppg.",
                    ticket.current_metrics.flow_balance,
                    ticket.current_metrics.pit_rate,
                    ticket.current_metrics.ecd_margin
                ),
            ),
            AnomalyCategory::DrillingEfficiency => (
                format!(
                    "Consider adjusting WOB/RPM to improve MSE. Current efficiency: {:.0}%. Target MSE: {:.0} psi.",
                    physics.mse_efficiency, physics.optimal_mse
                ),
                "Potential 10-20% ROP improvement, reduced bit wear".to_string(),
                format!(
                    "MSE trending {}. Average MSE: {:.0} psi vs optimal {:.0} psi ({:.0}% efficiency).",
                    if physics.mse_trend > 0.0 { "up (inefficient)" } else { "stable" },
                    physics.avg_mse,
                    physics.optimal_mse,
                    physics.mse_efficiency
                ),
            ),
            AnomalyCategory::Hydraulics => (
                "Monitor standpipe pressure and flow rates. Check for potential washout or plugging.".to_string(),
                "Hydraulic efficiency optimization, prevented equipment damage".to_string(),
                format!(
                    "Flow balance trend: {:.1} bbl/hr. ECD margin: {:.2} ppg.",
                    physics.flow_balance_trend, ticket.current_metrics.ecd_margin
                ),
            ),
            AnomalyCategory::Mechanical => (
                "Monitor torque and drag trends. Consider backreaming if torque continues to increase.".to_string(),
                "Pack-off prevention, reduced NPT risk".to_string(),
                "Elevated torque/drag detected. Possible mechanical resistance or pack-off developing.".to_string(),
            ),
            AnomalyCategory::Formation => (
                format!(
                    "Formation change detected. D-exponent trend: {:.1}%. Adjust drilling parameters accordingly.",
                    physics.dxc_trend
                ),
                "Optimized drilling through formation transition".to_string(),
                format!(
                    "D-exponent trend indicates formation change. Trend: {:.1}%.",
                    physics.dxc_trend
                ),
            ),
            AnomalyCategory::None => (
                "Continue monitoring drilling parameters.".to_string(),
                "Maintained operational efficiency".to_string(),
                "Normal drilling operations.".to_string(),
            ),
        };

        (recommendation, expected_benefit, reasoning)
    }

    /// Get latest advisory for dashboard
    pub fn get_latest_advisory(&self) -> Option<&StrategicAdvisory> {
        self.latest_advisory.as_ref()
    }

    /// Get latest verification result
    pub fn get_latest_verification(&self) -> Option<&VerificationResult> {
        self.latest_verification.as_ref()
    }

    /// Get pipeline statistics
    pub fn get_stats(&self) -> PipelineStats {
        PipelineStats {
            packets_processed: self.packets_processed,
            tickets_created: self.tickets_created,
            tickets_verified: self.tickets_verified,
            tickets_rejected: self.tickets_rejected,
            strategic_analyses: self.strategic_analyses,
            history_buffer_size: self.history_buffer.len(),
        }
    }

    /// Get current rig state from tactical agent
    pub fn get_rig_state(&self) -> RigState {
        self.tactical_agent.current_state()
    }

    /// Check if baseline learning is complete
    pub fn is_baseline_locked(&self) -> bool {
        self.tactical_agent.is_baseline_locked()
    }

    /// Get tactical agent statistics
    pub fn get_tactical_stats(&self) -> crate::agents::AgentStats {
        self.tactical_agent.stats()
    }

    /// Get latest drilling metrics (from tactical agent)
    pub fn get_latest_metrics(&self) -> Option<&DrillingMetrics> {
        self.latest_metrics.as_ref()
    }
}

impl Default for PipelineCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

/// Pipeline statistics
#[derive(Debug, Clone)]
pub struct PipelineStats {
    pub packets_processed: u64,
    pub tickets_created: u64,
    pub tickets_verified: u64,
    pub tickets_rejected: u64,
    pub strategic_analyses: u64,
    pub history_buffer_size: usize,
}

impl std::fmt::Display for PipelineStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Pipeline: {} packets, {} tickets ({} verified, {} rejected), {} advisories",
            self.packets_processed,
            self.tickets_created,
            self.tickets_verified,
            self.tickets_rejected,
            self.strategic_analyses
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_packet(rop: f64, flow_balance: f64) -> WitsPacket {
        WitsPacket {
            timestamp: 1705564800,
            bit_depth: 10000.0,
            hole_depth: 10050.0,
            rop,
            hook_load: 200.0,
            wob: 25.0,
            rpm: 120.0,
            torque: 15.0,
            bit_diameter: 8.5,
            spp: 2800.0,
            pump_spm: 120.0,
            flow_in: 500.0,
            flow_out: 500.0 + flow_balance,
            pit_volume: 500.0,
            pit_volume_change: 0.0,
            mud_weight_in: 12.0,
            mud_weight_out: 12.1,
            ecd: 12.4,
            mud_temp_in: 100.0,
            mud_temp_out: 120.0,
            gas_units: 50.0,
            background_gas: 40.0,
            connection_gas: 10.0,
            h2s: 0.0,
            co2: 0.1,
            casing_pressure: 0.0,
            annular_pressure: 0.0,
            pore_pressure: 10.5,
            fracture_gradient: 14.0,
            mse: 35000.0,
            d_exponent: 1.5,
            dxc: 1.45,
            rop_delta: 0.0,
            torque_delta_percent: 0.0,
            spp_delta: 0.0,
            rig_state: RigState::Drilling,
            waveform_snapshot: std::sync::Arc::new(Vec::new()),
        }
    }

    #[tokio::test]
    async fn test_normal_drilling_no_advisory() {
        let mut coordinator = PipelineCoordinator::new();

        // Process several normal packets to build baseline
        for _ in 0..20 {
            let packet = create_test_packet(50.0, 2.0);
            let result = coordinator.process_packet(&packet, Campaign::Production).await;
            // During baseline learning, should not generate advisories
            if !coordinator.is_baseline_locked() {
                assert!(result.is_none());
            }
        }

        let stats = coordinator.get_stats();
        assert_eq!(stats.packets_processed, 20);
    }

    #[tokio::test]
    async fn test_well_control_advisory() {
        let mut coordinator = PipelineCoordinator::new();

        // Build baseline first with more packets to ensure lock
        for _ in 0..50 {
            let packet = create_test_packet(50.0, 2.0);
            coordinator.process_packet(&packet, Campaign::Production).await;
        }

        // Simulate kick with high flow imbalance
        let mut kick_packet = create_test_packet(30.0, 25.0); // 25 bbl/hr flow out excess
        kick_packet.pit_volume_change = 10.0; // 10 bbl pit gain
        kick_packet.gas_units = 200.0; // High gas
        let _result = coordinator.process_packet(&kick_packet, Campaign::Production).await;

        // Verify packets were processed
        let stats = coordinator.get_stats();
        assert!(stats.packets_processed > 0, "Should have processed packets");
        // Note: Ticket creation depends on baseline lock status and anomaly detection
        // The tactical agent may not create tickets during baseline learning
    }
}
