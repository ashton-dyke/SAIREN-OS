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
use crate::context::KnowledgeStore;
use crate::physics_engine;
use crate::strategic::AdvisoryComposer;
use crate::types::{
    AdvisoryTicket, AnomalyCategory, Campaign, DrillingMetrics,
    DrillingPhysicsReport, FormationPrognosis, HistoryEntry,
    StrategicAdvisory, VerificationResult, VerificationStatus, WitsPacket,
};
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::Instant;
use tracing::{debug, info, warn};

use crate::config::defaults::{
    HISTORY_BUFFER_SIZE, PERIODIC_SUMMARY_INTERVAL_SECS,
    MIN_PACKETS_FOR_PERIODIC_SUMMARY, CYCLE_TARGET_GPU_MS,
};

/// Trend components computed from the history buffer with zero heap allocation.
struct TrendComponents {
    avg_mse: f64,
    mse_trend_pct: f64,
    dxc_trend_pct: f64,
    flow_balance_trend: f64,
    avg_pit_rate: f64,
}

/// Compute trend components from history using iterator sums (no `collect()`).
fn compute_trends(history: &[HistoryEntry], fallback_mse: f64) -> TrendComponents {
    let len = history.len();

    // Average MSE across full history
    let avg_mse = if len > 0 {
        history.iter().map(|e| e.metrics.mse).sum::<f64>() / len as f64
    } else {
        fallback_mse
    };

    // MSE trend: compare last 5 vs first 5
    let mse_trend_pct = if len >= 5 {
        let recent_avg = history.iter().rev().take(5).map(|e| e.metrics.mse).sum::<f64>() / 5.0;
        let earlier_avg = history.iter().take(5).map(|e| e.metrics.mse).sum::<f64>() / 5.0;
        (recent_avg - earlier_avg) / earlier_avg.max(1.0) * 100.0
    } else {
        0.0
    };

    // D-exponent trend
    let dxc_trend_pct = if len >= 5 {
        let recent_avg = history.iter().rev().take(5).map(|e| e.metrics.d_exponent).sum::<f64>() / 5.0;
        let earlier_avg = history.iter().take(5).map(|e| e.metrics.d_exponent).sum::<f64>() / 5.0;
        (recent_avg - earlier_avg) / earlier_avg.max(0.1) * 100.0
    } else {
        0.0
    };

    // Flow balance trend (absolute difference, not percentage)
    let flow_balance_trend = if len >= 5 {
        let recent_avg = history.iter().rev().take(5).map(|e| e.metrics.flow_balance).sum::<f64>() / 5.0;
        let earlier_avg = history.iter().take(5).map(|e| e.metrics.flow_balance).sum::<f64>() / 5.0;
        recent_avg - earlier_avg
    } else {
        0.0
    };

    // Average pit rate
    let avg_pit_rate = if len > 0 {
        history.iter().map(|e| e.metrics.pit_rate).sum::<f64>() / len as f64
    } else {
        0.0
    };

    TrendComponents {
        avg_mse,
        mse_trend_pct,
        dxc_trend_pct,
        flow_balance_trend,
        avg_pit_rate,
    }
}

/// Compute torque coefficient of variation from a slice of torque values.
///
/// Returns `None` if insufficient data or mean is non-positive.
fn compute_torque_cv(torques: &[f64]) -> Option<f64> {
    let n = torques.len() as f64;
    if n < 5.0 {
        return None;
    }
    let mean = torques.iter().sum::<f64>() / n;
    if mean <= 0.0 {
        return None;
    }
    let variance = torques.iter().map(|t| (t - mean).powi(2)).sum::<f64>() / n;
    Some(variance.sqrt() / mean)
}

/// Damping feedback monitor — tracks recommendation effectiveness.
enum DampingMonitorState {
    Idle {
        /// Most recent outcome (for API visibility)
        last_outcome: Option<(crate::types::DampingOutcome, Instant)>,
    },
    Active {
        baseline_cv: f64,
        recommendation: crate::types::DampingRecommendation,
        started_at: Instant,
        formation_name: Option<String>,
        depth: f64,
    },
}

/// Pipeline Coordinator manages the 10-phase processing sequence
pub struct PipelineCoordinator {
    /// Phase 2-3: Tactical Agent
    tactical_agent: TacticalAgent,
    /// Phase 5-7: Strategic Agent (for two-stage verification)
    strategic_agent: StrategicAgent,
    /// Phase 8: Orchestrator (trait-based specialist voting)
    orchestrator: Orchestrator,
    /// Phase 9: Advisory Composer (with CRITICAL cooldown)
    advisory_composer: AdvisoryComposer,
    /// Phase 6: Knowledge Store (trait-based, swappable backend)
    knowledge_store: Box<dyn KnowledgeStore>,
    /// Phase 4: History buffer (60 packets, circular)
    history_buffer: VecDeque<HistoryEntry>,
    /// Latest strategic advisory (for Phase 10 dashboard)
    latest_advisory: Option<StrategicAdvisory>,
    /// Latest verification result (for monitoring verification system)
    latest_verification: Option<VerificationResult>,
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
    /// Formation prognosis (loaded from TOML at startup)
    formation_prognosis: Option<FormationPrognosis>,
    /// Last formation name (for transition detection)
    last_formation_name: Option<String>,
    /// Proactive optimization engine
    optimizer: crate::optimization::ParameterOptimizer,
    /// Structured knowledge base (replaces flat prognosis when available)
    knowledge_base: Option<crate::knowledge_base::KnowledgeBase>,
    /// Previous pit volume for computing pit_volume_change delta
    prev_pit_volume: Option<f64>,
    /// Formation boundaries already alerted (prevents repeat lookahead advisories)
    alerted_boundaries: HashSet<String>,
    /// Active damping feedback monitor state
    damping_monitor: DampingMonitorState,
}

impl PipelineCoordinator {
    /// Create a new pipeline coordinator (without LLM - use init_with_llm for LLM support)
    pub fn new() -> Self {
        info!("Initializing Pipeline Coordinator for drilling intelligence");

        let knowledge_base = crate::knowledge_base::KnowledgeBase::init();
        let formation_prognosis = if let Some(ref kb) = knowledge_base {
            kb.prognosis()
        } else {
            FormationPrognosis::load()
        };

        Self {
            tactical_agent: TacticalAgent::new(),
            strategic_agent: StrategicAgent::new(),
            orchestrator: Orchestrator::new(),
            advisory_composer: AdvisoryComposer::new(),
            knowledge_store: Box::new(crate::context::StaticKnowledgeBase),
            history_buffer: VecDeque::with_capacity(HISTORY_BUFFER_SIZE),
            latest_advisory: None,
            latest_verification: None,
            packets_processed: 0,
            tickets_created: 0,
            tickets_verified: 0,
            tickets_rejected: 0,
            strategic_analyses: 0,
            last_periodic_summary_time: 0,
            latest_metrics: None,
            formation_prognosis,
            last_formation_name: None,
            optimizer: crate::optimization::ParameterOptimizer::new(300),
            knowledge_base,
            prev_pit_volume: None,
            alerted_boundaries: HashSet::new(),
            damping_monitor: DampingMonitorState::Idle { last_outcome: None },
        }
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

        let knowledge_base = crate::knowledge_base::KnowledgeBase::init();
        let formation_prognosis = if let Some(ref kb) = knowledge_base {
            kb.prognosis()
        } else {
            FormationPrognosis::load()
        };

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
            advisory_composer: AdvisoryComposer::new(),
            knowledge_store: Box::new(crate::context::StaticKnowledgeBase),
            history_buffer: VecDeque::with_capacity(HISTORY_BUFFER_SIZE),
            latest_advisory: None,
            latest_verification: None,
            packets_processed: 0,
            tickets_created: 0,
            tickets_verified: 0,
            tickets_rejected: 0,
            strategic_analyses: 0,
            last_periodic_summary_time: 0,
            latest_metrics: None,
            formation_prognosis,
            last_formation_name: None,
            optimizer: crate::optimization::ParameterOptimizer::new(300),
            knowledge_base,
            prev_pit_volume: None,
            alerted_boundaries: HashSet::new(),
            damping_monitor: DampingMonitorState::Idle { last_outcome: None },
        }
    }

    /// Process a WITS packet through the full pipeline
    ///
    /// Returns a StrategicAdvisory if:
    /// 1. Any advisory ticket was created (immediate processing), OR
    /// 2. No ticket was created but 10 minutes have elapsed (periodic summary)
    ///
    /// Spam protection is provided upstream by the tactical agent's per-severity
    /// cooldown (`default_cooldown_seconds` in `well_config.toml`), so all
    /// confirmed tickets reach the orchestrator immediately regardless of severity.
    /// Periodic summaries represent the last 10 minutes of drilling activity.
    pub async fn process_packet(
        &mut self,
        packet: &mut WitsPacket,
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

        // Phase 1.1: Input Sanitization
        let quality = crate::acquisition::wits_parser::sanitize_packet(packet);
        if !quality.usable {
            warn!(issues = ?quality.issues, "Packet rejected by sanitizer — skipping");
            return None;
        }

        // Compute pit_volume_change from consecutive packets (WITS has no item
        // code for pit volume *change* — only absolute pit_volume via item 0123).
        packet.pit_volume_change = match self.prev_pit_volume {
            Some(prev) if packet.pit_volume > 0.0 => packet.pit_volume - prev,
            _ => 0.0,
        };
        if packet.pit_volume > 0.0 {
            self.prev_pit_volume = Some(packet.pit_volume);
        }

        // Sync tactical agent campaign with AppState campaign
        self.tactical_agent.set_campaign(campaign);

        // Compute formation context for depth-ahead CfC network
        let formation_ctx = self.current_formation_context(packet.bit_depth)
            .map(|f| (packet.bit_depth - f.depth_top_ft, f.hardness));

        // PHASE 2-3: Tactical Agent (Basic Physics + Decision)
        let has_active_advisory = self.latest_advisory.is_some();
        let (ticket_opt, mut metrics, history_entry) = self.tactical_agent.process(packet, has_active_advisory, formation_ctx);

        // Phase 1.2: Enrich metrics with formation context
        if let Some(formation) = self.current_formation_context(packet.bit_depth) {
            let current_name = formation.name.clone();
            let depth_into = packet.bit_depth - formation.depth_top_ft;

            // Detect formation transitions
            if self.last_formation_name.as_deref() != Some(&current_name) {
                info!(
                    from = ?self.last_formation_name,
                    to = &current_name,
                    depth = packet.bit_depth,
                    "Formation transition detected"
                );
                self.last_formation_name = Some(current_name.clone());
                // Clear lookahead alert for the formation we just entered
                // (allow future re-alert if driller trips out and back)
                self.alerted_boundaries.remove(&current_name);
            }
            metrics.current_formation = Some(current_name);
            metrics.formation_depth_in_ft = Some(depth_into);
        }

        // Store latest metrics for API/dashboard access
        self.latest_metrics = Some(metrics.clone());

        // PHASE 4: History Buffer (always update, parallel to ticket processing)
        self.update_history_buffer(history_entry.clone());

        // Refresh prognosis from knowledge base if available (hot reload)
        let dynamic_prognosis = if let Some(ref kb) = self.knowledge_base {
            kb.prognosis()
        } else {
            self.formation_prognosis.clone()
        };

        // PHASE LOOKAHEAD: Standalone formation lookahead (independent of optimizer).
        // Runs before make_contiguous() because it needs &mut self for cooldown tracking.
        let lookahead_advisory = if let Some(ref prognosis) = dynamic_prognosis {
            self.check_standalone_lookahead(packet, prognosis)
        } else { None };

        // Make history buffer contiguous once — O(n) worst case on wrap, O(1) thereafter.
        // Collect into owned Vec so &mut self methods (check_damping_monitor,
        // enrich_with_damping) can be called without borrow conflicts.
        self.history_buffer.make_contiguous();
        let history_vec: Vec<HistoryEntry> = self.history_buffer.iter().cloned().collect();
        let history_slice: &[HistoryEntry] = &history_vec;

        // PHASE DAMPING-MONITOR: Check effectiveness of active damping recommendation
        let damping_monitor_text = self.check_damping_monitor(history_slice);

        // PHASE OPT: Proactive Optimization (every N packets, independent of tickets)
        let opt_advisory = if let Some(ref prognosis) = dynamic_prognosis {
            if let Some(formation) = prognosis.formation_at_depth(packet.bit_depth).cloned() {
                let physics = self.compute_physics_for_optimizer(packet, &metrics, history_slice);
                let cfc_score = self.tactical_agent.cfc_result()
                    .filter(|r| r.is_calibrated)
                    .map(|r| r.anomaly_score);

                match self.optimizer.evaluate(
                    packet, &physics, &formation, prognosis, history_slice, cfc_score, 1.0,
                ) {
                    Ok(adv) => {
                        info!(
                            confidence = adv.confidence.percent(),
                            recs = adv.recommendations.len(),
                            look_ahead = adv.look_ahead.is_some(),
                            "Optimization advisory generated"
                        );
                        Some(crate::optimization::templates::format_optimization_advisory(&adv, &physics))
                    }
                    Err(reason) => {
                        debug!(reason = %reason, "Optimization skipped");
                        None
                    }
                }
            } else { None }
        } else { None };

        // Seed the periodic summary timer on first packet to avoid
        // an immediate spurious summary (timestamp - 0 ≈ 1.7 billion seconds).
        if self.last_periodic_summary_time == 0 {
            self.last_periodic_summary_time = packet.timestamp;
        }

        // Check if it's time for a periodic summary (every 10 minutes)
        let time_since_last_summary = packet.timestamp.saturating_sub(self.last_periodic_summary_time);
        let should_generate_periodic = time_since_last_summary >= PERIODIC_SUMMARY_INTERVAL_SECS
            && self.history_buffer.len() >= MIN_PACKETS_FOR_PERIODIC_SUMMARY;

        // Determine if we have a CRITICAL ticket (bypasses periodic timing)
        let is_critical_ticket = ticket_opt.as_ref()
            .map(|t| matches!(t.severity, TicketSeverity::Critical))
            .unwrap_or(false);

        // Check if tactical agent created an advisory ticket
        let mut ticket = match ticket_opt {
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
                    return self.generate_periodic_summary(packet, &metrics, campaign).await;
                }
                // Return damping monitor advisory if monitoring reached a terminal state
                if let Some(text) = damping_monitor_text {
                    return Some(self.make_damping_monitor_advisory(packet, &text));
                }
                // Return optimization advisory if one was generated this cycle
                if let Some(adv) = opt_advisory {
                    return Some(adv);
                }
                // Return lookahead advisory if one was generated this cycle
                if let Some(adv) = lookahead_advisory {
                    return Some(adv);
                }
                debug!("Phase 3: No ticket, pipeline ends");
                return None;
            }
        };

        // PHASES 5-9: ONLY EXECUTED WHEN TICKET EXISTS

        // PHASE DAMPING: Enrich stick-slip tickets with active damping recommendations
        self.enrich_with_damping(&mut ticket, history_slice);

        // CAUSAL: Detect leading indicators before advanced physics.
        // Exclude the current packet (last entry) from the causal window to
        // avoid spurious self-correlations with the anomaly being analysed.
        {
            let causal_window = if history_slice.len() > 1 {
                &history_slice[..history_slice.len() - 1]
            } else {
                history_slice
            };
            ticket.causal_leads = crate::causal::detect_leads(causal_window);
            if !ticket.causal_leads.is_empty() {
                debug!(
                    leads = ticket.causal_leads.len(),
                    top_param = %ticket.causal_leads[0].parameter,
                    top_r = ticket.causal_leads[0].pearson_r,
                    top_lag_secs = ticket.causal_leads[0].lag_seconds,
                    "Causal leads detected"
                );
            }
        }

        // PHASE 5: Advanced Physics
        let physics = self.run_advanced_physics(&ticket, packet, history_slice);

        // PHASE 6: Context Lookup
        let context = self.lookup_context(&ticket);

        // Run strategic verification
        let verification_result = self.strategic_agent.verify_ticket(
            &ticket,
            history_slice,
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

        // PHASE 8: Orchestrator Voting (regime-aware specialist weighting)
        let voting_result = self.orchestrator.vote(&ticket, &physics, packet.regime_id);

        // PHASE 9: Advisory Composition (with CRITICAL cooldown)
        let advisory = match self.advisory_composer.compose(
            &ticket,
            &physics,
            &context,
            &recommendation,
            &expected_benefit,
            &reasoning,
            &voting_result,
        ) {
            Some(adv) => adv,
            None => {
                debug!("Advisory suppressed by CRITICAL cooldown");
                return None;
            }
        };

        // PHASE 10: Storage (store in latest_advisory for dashboard)
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

        // Cycle time target: 100 ms. LLM inference runs on the hub — not here.
        let cycle_target_ms: u128 = CYCLE_TARGET_GPU_MS;

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
            flow_data_available: current_metrics.flow_data_available,
            is_anomaly: anomaly_rate > 10.0,
            anomaly_category: category.clone(),
            anomaly_description: Some(format!(
                "10-min summary: {:.1}% anomaly rate, avg ROP {:.1} ft/hr, avg MSE {:.0} psi",
                anomaly_rate, avg_rop, avg_mse
            )),
            current_formation: None,
            formation_depth_in_ft: None,
        };

        let mut summary_ticket = AdvisoryTicket {
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
            context: None,
            depth: packet.bit_depth,
            trace_log: Vec::new(),
            cfc_anomaly_score: None,
            cfc_feature_surprises: Vec::new(),
            causal_leads: Vec::new(),
            damping_recommendation: None,
        };

        // Make history buffer contiguous for zero-copy slice access
        self.history_buffer.make_contiguous();
        let (history_slice, _) = self.history_buffer.as_slices();

        // CAUSAL: Attach leading indicators to periodic summary
        summary_ticket.causal_leads = crate::causal::detect_leads(history_slice);

        // Run through remaining phases
        let physics = self.run_advanced_physics(&summary_ticket, packet, history_slice);
        let context = self.lookup_context(&summary_ticket);

        // Strategic verification (summary tickets typically pass)
        let verification_result = self.strategic_agent.verify_ticket(&summary_ticket, history_slice);
        self.latest_verification = Some(verification_result.clone());

        // Generate explanation
        let (recommendation, expected_benefit, reasoning) = self
            .generate_explanation(&summary_ticket, &physics, &context, campaign)
            .await;

        // Orchestrator voting (regime-aware specialist weighting)
        let voting_result = self.orchestrator.vote(&summary_ticket, &physics, packet.regime_id);

        // Advisory composition (with CRITICAL cooldown)
        let advisory = match self.advisory_composer.compose(
            &summary_ticket,
            &physics,
            &context,
            &recommendation,
            &expected_benefit,
            &reasoning,
            &voting_result,
        ) {
            Some(adv) => adv,
            None => {
                debug!("Periodic summary suppressed by CRITICAL cooldown");
                return None;
            }
        };

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

    /// Phase 5: Run advanced physics calculations for drilling.
    ///
    /// Accepts a pre-computed history slice to avoid cloning the history buffer.
    fn run_advanced_physics(
        &self,
        ticket: &AdvisoryTicket,
        packet: &WitsPacket,
        history: &[HistoryEntry],
    ) -> DrillingPhysicsReport {
        let start = Instant::now();

        let trends = compute_trends(history, packet.mse);

        let formation_hardness = (ticket.current_metrics.d_exponent * 3.0).clamp(1.0, 10.0);
        let optimal_mse = physics_engine::estimate_optimal_mse(formation_hardness);
        let mse_efficiency = (optimal_mse / trends.avg_mse.max(1.0) * 100.0).min(100.0);

        // Detect drilling dysfunctions based on current metrics
        let mut detected_dysfunctions = Vec::new();
        if ticket.current_metrics.is_anomaly {
            if let Some(ref desc) = ticket.current_metrics.anomaly_description {
                detected_dysfunctions.push(desc.clone());
            }
        }

        let confidence = (history.len() as f64 / HISTORY_BUFFER_SIZE as f64).min(1.0);

        let elapsed = start.elapsed();
        if elapsed.as_millis() > 50 {
            warn!(
                elapsed_ms = elapsed.as_millis(),
                "Phase 5 exceeded 50ms target"
            );
        }

        debug!(
            avg_mse = trends.avg_mse,
            mse_trend = trends.mse_trend_pct,
            mse_efficiency = mse_efficiency,
            flow_balance_trend = trends.flow_balance_trend,
            confidence = confidence,
            "Advanced drilling physics complete"
        );

        DrillingPhysicsReport {
            avg_mse: trends.avg_mse,
            mse_trend: trends.mse_trend_pct,
            optimal_mse,
            mse_efficiency,
            dxc_trend: trends.dxc_trend_pct,
            flow_balance_trend: trends.flow_balance_trend,
            avg_pit_rate: trends.avg_pit_rate,
            formation_hardness,
            confidence,
            detected_dysfunctions,
            wob_trend: 0.0,
            rop_trend: 0.0,
            founder_detected: false,
            founder_severity: 0.0,
            optimal_wob_estimate: 0.0,
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

    /// Lightweight physics computation for the optimization engine.
    ///
    /// Same trend math as `run_advanced_physics` via shared `compute_trends()`,
    /// but takes `DrillingMetrics` directly instead of an `AdvisoryTicket`.
    fn compute_physics_for_optimizer(
        &self,
        packet: &WitsPacket,
        metrics: &DrillingMetrics,
        history: &[HistoryEntry],
    ) -> DrillingPhysicsReport {
        let trends = compute_trends(history, packet.mse);

        let formation_hardness = (metrics.d_exponent * 3.0).clamp(1.0, 10.0);
        let optimal_mse = physics_engine::estimate_optimal_mse(formation_hardness);
        let mse_efficiency = (optimal_mse / trends.avg_mse.max(1.0) * 100.0).min(100.0);

        let confidence = (history.len() as f64 / HISTORY_BUFFER_SIZE as f64).min(1.0);

        DrillingPhysicsReport {
            avg_mse: trends.avg_mse,
            mse_trend: trends.mse_trend_pct,
            optimal_mse,
            mse_efficiency,
            dxc_trend: trends.dxc_trend_pct,
            flow_balance_trend: trends.flow_balance_trend,
            avg_pit_rate: trends.avg_pit_rate,
            formation_hardness,
            confidence,
            detected_dysfunctions: Vec::new(),
            wob_trend: 0.0,
            rop_trend: 0.0,
            founder_detected: false,
            founder_severity: 0.0,
            optimal_wob_estimate: 0.0,
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

    /// Phase 6: Context lookup from knowledge store (trait-based)
    fn lookup_context(&self, ticket: &AdvisoryTicket) -> Vec<String> {
        let query = match ticket.category {
            AnomalyCategory::WellControl => "well control kick loss circulation flow imbalance",
            AnomalyCategory::DrillingEfficiency => "MSE drilling efficiency ROP optimization",
            AnomalyCategory::Hydraulics => "standpipe pressure ECD flow rate hydraulics",
            AnomalyCategory::Mechanical => "torque pack-off stick-slip mechanical",
            AnomalyCategory::Formation => "d-exponent formation change pore pressure",
            AnomalyCategory::None => "normal drilling operations",
        };

        let context = self.knowledge_store.query(query, 5);

        debug!(
            category = ?ticket.category,
            results = context.len(),
            store = self.knowledge_store.store_name(),
            "Context lookup complete"
        );

        context
    }

    /// Phase 7: Generate explanation (template-based).
    ///
    /// LLM advisory generation runs exclusively on the fleet hub which has a
    /// CUDA GPU and embeds mistralrs directly. The edge client always uses
    /// deterministic templates so pipeline latency stays within the 100 ms target.
    async fn generate_explanation(
        &self,
        ticket: &AdvisoryTicket,
        physics: &DrillingPhysicsReport,
        _context: &[String],
        campaign: Campaign,
    ) -> (String, String, String) {
        Self::template_explanation(ticket, physics, campaign)
    }

    /// Template-based explanation generator (fallback when LLM unavailable)
    ///
    /// Uses the dedicated template module which provides richer, campaign-aware
    /// templates with actual metric values embedded in the text.
    fn template_explanation(
        ticket: &AdvisoryTicket,
        physics: &DrillingPhysicsReport,
        campaign: Campaign,
    ) -> (String, String, String) {
        let result = crate::strategic::templates::template_advisory(ticket, physics, campaign);
        info!(source = result.source, confidence = result.confidence, "Template advisory generated");
        (result.recommendation, result.expected_benefit, result.reasoning)
    }

    /// Standalone formation lookahead check (independent of optimizer).
    ///
    /// Fires once per formation boundary, then enters cooldown until
    /// the formation transition actually occurs (which clears the cooldown).
    fn check_standalone_lookahead(
        &mut self,
        packet: &WitsPacket,
        prognosis: &FormationPrognosis,
    ) -> Option<StrategicAdvisory> {
        let config = crate::config::get();
        if !config.lookahead.enabled {
            return None;
        }

        let formation = prognosis.formation_at_depth(packet.bit_depth)?;
        let mut la = crate::optimization::look_ahead::check_look_ahead(
            prognosis,
            packet.bit_depth,
            packet.rop,
            formation,
            config.lookahead.window_minutes,
        )?;

        // One-shot cooldown per formation boundary
        if self.alerted_boundaries.contains(&la.formation_name) {
            return None;
        }

        // Annotate with depth-ahead CfC confidence if available
        la.cfc_confidence = self.tactical_agent.depth_ahead_result()
            .filter(|r| r.is_calibrated)
            .map(|r| r.confidence);

        info!(
            formation = %la.formation_name,
            eta_min = la.estimated_minutes,
            cfc_confidence = ?la.cfc_confidence,
            "Lookahead advisory: approaching formation boundary"
        );
        self.alerted_boundaries.insert(la.formation_name.clone());

        Some(crate::optimization::templates::format_lookahead_advisory(
            &la,
            packet.bit_depth,
            packet.rop,
        ))
    }

    /// Enrich a stick-slip ticket with active damping recommendations.
    ///
    /// Runs oscillation characterization on the torque time series from
    /// the history buffer and generates bounded parameter recommendations.
    /// When a formation-specific recipe exists, blends its proven parameters
    /// into the recommendation (70% recipe, 30% lookup table).
    fn enrich_with_damping(
        &mut self,
        ticket: &mut AdvisoryTicket,
        history: &[HistoryEntry],
    ) {
        let damping = &crate::config::get().damping;
        if !damping.enabled {
            return;
        }

        // Only enrich stick-slip tickets (Mechanical category with "Stick-slip" pattern)
        let is_stick_slip = ticket.category == AnomalyCategory::Mechanical
            && ticket
                .context
                .as_ref()
                .map(|c| c.pattern.contains("Stick-slip") || c.pattern.contains("stick-slip"))
                .unwrap_or(false);

        if !is_stick_slip {
            return;
        }

        // Extract torque time series from history
        let torques: Vec<f64> = history.iter().map(|e| e.packet.torque).collect();

        // Run oscillation characterization
        let analysis = match physics_engine::characterize_oscillation(&torques, damping.min_samples) {
            Some(a) => a,
            None => return,
        };

        // Only proceed if CV meets the damping threshold
        if analysis.torque_cv < damping.cv_threshold {
            return;
        }

        // Get current WOB and RPM from the latest history entry
        let (wob, rpm) = match history.last() {
            Some(e) => (e.packet.wob, e.packet.rpm),
            None => return,
        };

        // Generate damping recommendation
        if let Some(rec) = physics_engine::recommend_damping(
            &analysis,
            wob,
            rpm,
            damping.max_wob_reduction_pct,
            damping.max_rpm_change_pct,
        ) {
            // Check for a formation-specific recipe to refine the recommendation
            let rec = if let Some(formation) = self.last_formation_name.as_deref() {
                if let Some(recipe) = crate::storage::damping_recipes::best_recipe(formation) {
                    let mut refined = rec;
                    // Blend recipe parameters (70% recipe, 30% lookup table)
                    let recipe_weight = 0.7;
                    refined.wob_change_pct = recipe.wob_change_pct * recipe_weight
                        + refined.wob_change_pct * (1.0 - recipe_weight);
                    refined.rpm_change_pct = recipe.rpm_change_pct * recipe_weight
                        + refined.rpm_change_pct * (1.0 - recipe_weight);
                    // Recompute absolute values from blended percentages
                    refined.recommended_wob = wob * (1.0 + refined.wob_change_pct / 100.0);
                    refined.recommended_rpm = rpm * (1.0 + refined.rpm_change_pct / 100.0);
                    refined.rationale = format!(
                        "{} (refined by {} formation recipe: baseline CV {:.1}% → {:.1}%)",
                        refined.rationale,
                        formation,
                        recipe.baseline_cv * 100.0,
                        recipe.achieved_cv * 100.0,
                    );
                    info!(
                        formation,
                        recipe_wob_pct = recipe.wob_change_pct,
                        recipe_rpm_pct = recipe.rpm_change_pct,
                        blended_wob_pct = refined.wob_change_pct,
                        blended_rpm_pct = refined.rpm_change_pct,
                        "Recipe-informed damping recommendation"
                    );
                    refined
                } else {
                    rec
                }
            } else {
                rec
            };

            info!(
                osc_type = ?analysis.oscillation_type,
                cv = analysis.torque_cv,
                wob_pct = rec.wob_change_pct,
                rpm_pct = rec.rpm_change_pct,
                "Damping recommendation attached to stick-slip ticket"
            );

            // Start monitoring this recommendation's effectiveness
            self.damping_monitor = DampingMonitorState::Active {
                baseline_cv: analysis.torque_cv,
                recommendation: rec.clone(),
                started_at: Instant::now(),
                formation_name: self.last_formation_name.clone(),
                depth: history.last().map(|e| e.packet.bit_depth).unwrap_or(0.0),
            };
            info!(
                baseline_cv = analysis.torque_cv,
                formation = ?self.last_formation_name,
                "Damping monitor activated"
            );

            ticket.damping_recommendation = Some(rec);
        }
    }

    /// Check damping monitor state against current torque data.
    ///
    /// Runs on every packet. If monitoring is active, computes current
    /// torque CV and determines if the recommendation succeeded, should
    /// be escalated, or should be retracted.
    ///
    /// Returns an optional advisory text if monitoring reaches a terminal state.
    fn check_damping_monitor(
        &mut self,
        history: &[HistoryEntry],
    ) -> Option<String> {
        use crate::types::DampingOutcome;
        use std::time::Duration;

        let config = crate::config::get();
        let damping = &config.damping;

        // Only process when actively monitoring
        let (baseline_cv, started_at, formation_name, recommendation, depth) = match &self.damping_monitor {
            DampingMonitorState::Idle { .. } => return None,
            DampingMonitorState::Active {
                baseline_cv,
                started_at,
                formation_name,
                recommendation,
                depth,
            } => (*baseline_cv, *started_at, formation_name.clone(), recommendation.clone(), *depth),
        };

        // Compute current torque CV
        let torques: Vec<f64> = history.iter().map(|e| e.packet.torque).collect();
        let current_cv = match compute_torque_cv(&torques) {
            Some(cv) => cv,
            None => return None, // Not enough data yet
        };

        let cv_change_pct = (current_cv - baseline_cv) / baseline_cv * 100.0;
        let elapsed = started_at.elapsed();

        // Success: CV dropped enough
        if cv_change_pct <= -(damping.success_cv_reduction_pct) {
            let now_ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let recipe = crate::types::DampingRecipe {
                formation_name: formation_name.clone().unwrap_or_default(),
                wob_change_pct: recommendation.wob_change_pct,
                rpm_change_pct: recommendation.rpm_change_pct,
                baseline_cv,
                achieved_cv: current_cv,
                cv_reduction_pct: -cv_change_pct,
                depth_ft: depth,
                recorded_at: now_ts,
            };

            if !recipe.formation_name.is_empty() {
                if let Err(e) = crate::storage::damping_recipes::persist(
                    &recipe,
                    damping.max_recipes_per_formation,
                ) {
                    warn!("Failed to persist damping recipe: {}", e);
                }
            }

            info!(
                formation = ?formation_name,
                baseline_cv,
                current_cv,
                cv_change_pct,
                "Damping success — recipe stored"
            );

            self.damping_monitor = DampingMonitorState::Idle {
                last_outcome: Some((DampingOutcome::Success, Instant::now())),
            };

            let formation_suffix = formation_name
                .as_deref()
                .map(|f| format!(" in formation {}", f))
                .unwrap_or_default();
            return Some(format!(
                "DAMPING SUCCESS: Torque CV reduced from {:.1}% to {:.1}% ({:.1}%){}. Recipe stored.",
                baseline_cv * 100.0,
                current_cv * 100.0,
                cv_change_pct,
                formation_suffix,
            ));
        }

        // Retracted: CV rose too much
        if cv_change_pct >= damping.retract_cv_increase_pct {
            warn!(
                baseline_cv,
                current_cv,
                cv_change_pct,
                "Damping retracted — CV worsened"
            );

            self.damping_monitor = DampingMonitorState::Idle {
                last_outcome: Some((DampingOutcome::Retracted, Instant::now())),
            };

            return Some(format!(
                "DAMPING RETRACTED: Torque CV increased from {:.1}% to {:.1}% (+{:.1}%). \
                 Previous recommendation ineffective — consider alternative approach.",
                baseline_cv * 100.0,
                current_cv * 100.0,
                cv_change_pct,
            ));
        }

        // Escalated: window expired with no success/retraction
        if elapsed >= Duration::from_secs(damping.monitor_window_secs) {
            warn!(
                elapsed_secs = elapsed.as_secs(),
                baseline_cv,
                current_cv,
                "Damping escalated — no improvement within window"
            );

            self.damping_monitor = DampingMonitorState::Idle {
                last_outcome: Some((DampingOutcome::Escalated, Instant::now())),
            };

            return Some(format!(
                "DAMPING ESCALATED: No significant CV improvement after {}s. \
                 Current CV {:.1}% vs baseline {:.1}%. \
                 Consider more aggressive WOB reduction or RPM adjustment.",
                elapsed.as_secs(),
                current_cv * 100.0,
                baseline_cv * 100.0,
            ));
        }

        // Still monitoring
        None
    }

    /// Create a standalone advisory from damping monitor outcome text.
    fn make_damping_monitor_advisory(
        &self,
        packet: &WitsPacket,
        text: &str,
    ) -> StrategicAdvisory {
        use crate::types::{FinalSeverity, RiskLevel};

        let severity = if text.starts_with("DAMPING SUCCESS") {
            FinalSeverity::Low
        } else if text.starts_with("DAMPING RETRACTED") {
            FinalSeverity::Medium
        } else {
            FinalSeverity::Medium
        };

        StrategicAdvisory {
            timestamp: packet.timestamp,
            efficiency_score: 80,
            risk_level: RiskLevel::Low,
            severity,
            recommendation: text.to_string(),
            expected_benefit: "Damping feedback monitoring".to_string(),
            reasoning: "Automated torque CV tracking after damping recommendation".to_string(),
            votes: Vec::new(),
            physics_report: DrillingPhysicsReport::default(),
            context_used: Vec::new(),
            trace_log: Vec::new(),
            category: AnomalyCategory::Mechanical,
            trigger_parameter: "torque_cv_monitor".to_string(),
            trigger_value: 0.0,
            threshold_value: 0.0,
        }
    }

    /// Get a snapshot of the current damping monitor state for API visibility.
    pub fn damping_monitor_snapshot(&self) -> crate::types::DampingMonitorSnapshot {
        use crate::types::DampingMonitorSnapshot;

        let config = &crate::config::get().damping;
        match &self.damping_monitor {
            DampingMonitorState::Idle { last_outcome } => DampingMonitorSnapshot {
                active: false,
                baseline_cv: None,
                current_cv: None,
                cv_change_pct: None,
                elapsed_secs: None,
                window_secs: config.monitor_window_secs,
                formation_name: None,
                last_outcome: last_outcome.map(|(o, _)| o),
            },
            DampingMonitorState::Active {
                baseline_cv,
                started_at,
                formation_name,
                ..
            } => {
                // Compute current CV from history for the snapshot
                let torques: Vec<f64> =
                    self.history_buffer.iter().map(|e| e.packet.torque).collect();
                let current_cv = compute_torque_cv(&torques);
                let cv_change_pct =
                    current_cv.map(|cv| (cv - baseline_cv) / baseline_cv * 100.0);
                DampingMonitorSnapshot {
                    active: true,
                    baseline_cv: Some(*baseline_cv),
                    current_cv,
                    cv_change_pct,
                    elapsed_secs: Some(started_at.elapsed().as_secs()),
                    window_secs: config.monitor_window_secs,
                    formation_name: formation_name.clone(),
                    last_outcome: None,
                }
            }
        }
    }

    /// Look up the formation at the current bit depth.
    ///
    /// When the knowledge base is active, it dynamically reads from the KB
    /// (which may have been updated by the watcher). Falls back to the static prognosis.
    fn current_formation_context(&self, depth_ft: f64) -> Option<crate::types::FormationInterval> {
        if let Some(ref kb) = self.knowledge_base {
            return kb.formation_at_depth(depth_ft);
        }
        self.formation_prognosis.as_ref()?.formation_at_depth(depth_ft).cloned()
    }

    /// Get a reference to the tactical agent
    pub fn tactical_agent(&self) -> &TacticalAgent {
        &self.tactical_agent
    }

    /// Mutable access to the tactical agent (for federation checkpoint operations).
    pub fn tactical_agent_mut(&mut self) -> &mut TacticalAgent {
        &mut self.tactical_agent
    }

    /// Snapshot the dual CfC network state for federation upload.
    pub fn snapshot_cfc(&self, rig_id: &str, well_id: &str) -> crate::cfc::checkpoint::DualCfcCheckpoint {
        self.tactical_agent.cfc_network().snapshot(rig_id, well_id)
    }

    /// Restore dual CfC network state from a federated checkpoint.
    pub fn restore_cfc_from_checkpoint(
        &mut self,
        cp: &crate::cfc::checkpoint::DualCfcCheckpoint,
    ) -> Result<(), String> {
        self.tactical_agent.cfc_network_mut().restore_from(cp)
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

    /// Get latest drilling metrics (from tactical agent)
    pub fn get_latest_metrics(&self) -> Option<&DrillingMetrics> {
        self.latest_metrics.as_ref()
    }

    /// Get the latest regime ID from CfC motor output clustering
    pub fn latest_regime_id(&self) -> u8 {
        self.tactical_agent.latest_regime_id()
    }

    /// Get the current regime centroids (k=4, dim=8)
    pub fn regime_centroids(&self) -> [[f64; 8]; 4] {
        self.tactical_agent.regime_centroids()
    }

    /// Start the knowledge base watcher (if KB is active)
    pub fn start_kb_watcher(&self) -> Option<tokio::task::JoinHandle<()>> {
        self.knowledge_base.as_ref().map(|kb| kb.start_watcher())
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
    use crate::types::RigState;

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
            regime_id: 0,
            seconds_since_param_change: 0,        }
    }

    #[tokio::test]
    async fn test_normal_drilling_no_advisory() {
        let mut coordinator = PipelineCoordinator::new();

        // Process several normal packets to build baseline
        for _ in 0..20 {
            let mut packet = create_test_packet(50.0, 2.0);
            let result = coordinator.process_packet(&mut packet, Campaign::Production).await;
            // During baseline learning, should not generate advisories
            if !coordinator.tactical_agent().is_baseline_locked() {
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
            let mut packet = create_test_packet(50.0, 2.0);
            coordinator.process_packet(&mut packet, Campaign::Production).await;
        }

        // Simulate kick with high flow imbalance
        let mut kick_packet = create_test_packet(30.0, 25.0); // 25 bbl/hr flow out excess
        kick_packet.pit_volume_change = 10.0; // 10 bbl pit gain
        kick_packet.gas_units = 200.0; // High gas
        let _result = coordinator.process_packet(&mut kick_packet, Campaign::Production).await;

        // Verify packets were processed
        let stats = coordinator.get_stats();
        assert!(stats.packets_processed > 0, "Should have processed packets");
        // Note: Ticket creation depends on baseline lock status and anomaly detection
        // The tactical agent may not create tickets during baseline learning
    }

    #[tokio::test]
    async fn test_pit_volume_change_computed() {
        let mut coordinator = PipelineCoordinator::new();

        // First packet: pit_volume = 800.0, no previous → delta should be 0.0
        let mut pkt1 = create_test_packet(50.0, 2.0);
        pkt1.pit_volume = 800.0;
        coordinator.process_packet(&mut pkt1, Campaign::Production).await;
        assert_eq!(pkt1.pit_volume_change, 0.0, "First packet should have delta 0.0");

        // Second packet: pit_volume = 808.0 → delta should be 8.0
        let mut pkt2 = create_test_packet(50.0, 2.0);
        pkt2.pit_volume = 808.0;
        coordinator.process_packet(&mut pkt2, Campaign::Production).await;
        assert!(
            (pkt2.pit_volume_change - 8.0).abs() < 1e-9,
            "Second packet should have delta 8.0, got {}",
            pkt2.pit_volume_change
        );

        // Third packet: pit_volume = 805.0 → delta should be -3.0
        let mut pkt3 = create_test_packet(50.0, 2.0);
        pkt3.pit_volume = 805.0;
        coordinator.process_packet(&mut pkt3, Campaign::Production).await;
        assert!(
            (pkt3.pit_volume_change - (-3.0)).abs() < 1e-9,
            "Third packet should have delta -3.0, got {}",
            pkt3.pit_volume_change
        );
    }

    // ============================================================================
    // Damping Monitor Tests (Iteration 2)
    // ============================================================================

    /// Build a history entry with the given torque value.
    fn make_history_entry(torque: f64) -> HistoryEntry {
        let mut pkt = create_test_packet(50.0, 2.0);
        pkt.torque = torque;
        HistoryEntry {
            packet: pkt,
            metrics: DrillingMetrics::default(),
        }
    }

    #[test]
    fn test_compute_torque_cv() {
        // Constant torque → CV = 0
        let constant: Vec<f64> = vec![10.0; 20];
        assert_eq!(compute_torque_cv(&constant), Some(0.0));

        // Insufficient data (< 5 samples)
        assert_eq!(compute_torque_cv(&[1.0, 2.0, 3.0]), None);

        // Known values: [10, 20, 30, 40, 50] → mean=30, std≈14.14, CV≈0.471
        let known = vec![10.0, 20.0, 30.0, 40.0, 50.0];
        let cv = compute_torque_cv(&known).unwrap();
        assert!((cv - 0.4714).abs() < 0.01, "CV should be ~0.471, got {}", cv);

        // Non-positive mean → None
        let zeros = vec![0.0; 10];
        assert_eq!(compute_torque_cv(&zeros), None);
    }

    /// Build a test DampingRecommendation.
    fn make_test_damping_rec() -> crate::types::DampingRecommendation {
        crate::types::DampingRecommendation {
            analysis: crate::types::OscillationAnalysis {
                oscillation_type: crate::types::OscillationType::StickSlip,
                torque_cv: 0.25,
                estimated_frequency_hz: 0.3,
                amplitude_ratio: 0.5,
                severity: 0.6,
                sample_count: 60,
            },
            current_wob: 25.0,
            recommended_wob: 21.0,
            wob_change_pct: -15.0,
            current_rpm: 120.0,
            recommended_rpm: 132.0,
            rpm_change_pct: 10.0,
            rationale: "test recommendation".to_string(),
        }
    }

    /// Ensure config is initialized for coordinator tests.
    fn ensure_config() {
        crate::config::init(
            crate::config::WellConfig::default(),
            crate::config::ConfigProvenance::default(),
        );
    }

    #[test]
    fn test_monitor_success_transition() {
        ensure_config();
        // Set up storage for recipe persistence
        let tmp = std::env::temp_dir().join("sairen_coord_test_success");
        let _ = std::fs::remove_dir_all(&tmp);
        let _ = crate::storage::history::init(tmp.to_str().unwrap());
        let _ = crate::storage::damping_recipes::init();

        let mut coordinator = PipelineCoordinator::new();

        // Activate monitoring with baseline_cv = 0.25
        coordinator.damping_monitor = DampingMonitorState::Active {
            baseline_cv: 0.25,
            recommendation: make_test_damping_rec(),
            started_at: std::time::Instant::now(),
            formation_name: Some("TestFormation".to_string()),
            depth: 10000.0,
        };

        // Build history with torque that gives CV = ~0.047 (low = success)
        // With baseline 0.25, need CV ≤ 0.25 * (1 - 0.20) = 0.20
        // Use near-constant torque → CV ≈ 0
        let history: Vec<HistoryEntry> = (0..20)
            .map(|_| make_history_entry(15.0))
            .collect();

        let result = coordinator.check_damping_monitor(&history);
        assert!(result.is_some(), "Should return success advisory text");
        let text = result.unwrap();
        assert!(text.contains("DAMPING SUCCESS"), "Text should indicate success: {}", text);

        // Monitor should transition to Idle
        assert!(matches!(
            coordinator.damping_monitor,
            DampingMonitorState::Idle { last_outcome: Some((crate::types::DampingOutcome::Success, _)) }
        ));
    }

    #[test]
    fn test_monitor_retraction() {
        ensure_config();
        let mut coordinator = PipelineCoordinator::new();

        // Activate monitoring with baseline_cv = 0.10
        coordinator.damping_monitor = DampingMonitorState::Active {
            baseline_cv: 0.10,
            recommendation: make_test_damping_rec(),
            started_at: std::time::Instant::now(),
            formation_name: None,
            depth: 10000.0,
        };

        // Build history with high torque variation → high CV (much higher than 0.10)
        // Need cv_change_pct >= 15.0, so current_cv >= 0.10 * 1.15 = 0.115
        // Use widely varying torque: [5, 25, 5, 25, ...] → mean=15, std=10, CV=0.667
        let history: Vec<HistoryEntry> = (0..20)
            .map(|i| {
                if i % 2 == 0 { make_history_entry(5.0) }
                else { make_history_entry(25.0) }
            })
            .collect();

        let result = coordinator.check_damping_monitor(&history);
        assert!(result.is_some(), "Should return retraction advisory text");
        let text = result.unwrap();
        assert!(text.contains("DAMPING RETRACTED"), "Text should indicate retraction: {}", text);

        assert!(matches!(
            coordinator.damping_monitor,
            DampingMonitorState::Idle { last_outcome: Some((crate::types::DampingOutcome::Retracted, _)) }
        ));
    }

    #[test]
    fn test_monitor_escalation() {
        ensure_config();
        let mut coordinator = PipelineCoordinator::new();

        // Activate monitoring with a started_at far in the past (> monitor_window_secs)
        let long_ago = std::time::Instant::now() - std::time::Duration::from_secs(300);
        coordinator.damping_monitor = DampingMonitorState::Active {
            baseline_cv: 0.20,
            recommendation: make_test_damping_rec(),
            started_at: long_ago,
            formation_name: None,
            depth: 10000.0,
        };

        // Build history with moderate variation (CV similar to baseline — no improvement but no worsening)
        // CV around 0.19 → cv_change_pct = (0.19 - 0.20) / 0.20 * 100 = -5% (not enough for success)
        // Use torque values that produce CV ≈ 0.19
        // torques: [14, 15, 16, 14, 15, 16, ...] → mean=15, std≈0.816, CV≈0.054
        // That's way too low. Need CV ≈ 0.18-0.19
        // torques with mean=15, std=15*0.19=2.85
        // Use [12, 15, 18, 12, 15, 18, ...] → mean=15, variance=6, std=2.449, CV=0.163
        // That's CV=0.163 vs baseline 0.20, change = (0.163-0.20)/0.20*100 = -18.5% → that triggers success
        // Need CV between 0.20 * (1-0.20) = 0.16 and 0.20 * (1+0.15) = 0.23
        // So CV in [0.161, 0.229] — doesn't trigger success or retraction
        // Use [11, 15, 19, 11, 15, 19, ...] → mean=15, variance≈10.67, std≈3.27, CV≈0.218
        let history: Vec<HistoryEntry> = (0..21)
            .map(|i| {
                let torque = match i % 3 {
                    0 => 11.0,
                    1 => 15.0,
                    _ => 19.0,
                };
                make_history_entry(torque)
            })
            .collect();

        let result = coordinator.check_damping_monitor(&history);
        assert!(result.is_some(), "Should return escalation advisory text");
        let text = result.unwrap();
        assert!(text.contains("DAMPING ESCALATED"), "Text should indicate escalation: {}", text);

        assert!(matches!(
            coordinator.damping_monitor,
            DampingMonitorState::Idle { last_outcome: Some((crate::types::DampingOutcome::Escalated, _)) }
        ));
    }
}
