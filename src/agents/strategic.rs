//! Strategic Agent - Drilling Advisory Verification
//!
//! Performs deep analysis of advisory tickets from the tactical agent.
//! Combines physics engine calculations, contextual knowledge, and LLM
//! to generate comprehensive drilling advisories.
//!
//! ## Verification Rules
//!
//! - CONFIRMED if: sustained MSE inefficiency + physics supports
//! - CONFIRMED if: well control indicators persistent
//! - REJECTED if: transient spike, returned to baseline
//! - UNCERTAIN if: insufficient history or conflicting signals
//!
//! ## LLM Integration
//!
//! When enabled, uses a 7B DeepSeek model for deep reasoning and diagnosis.
//! The LLM receives physics analysis, contextual knowledge from vector DB,
//! and ticket details to generate actionable drilling recommendations.

use crate::baseline::{wits_metrics, ThresholdManager};
use crate::context::vector_db;
use crate::llm::StrategicLLM;
use crate::physics_engine;
use crate::types::{
    verification_thresholds, AdvisoryTicket, AnomalyCategory, CheckStatus, DrillingMetrics,
    DrillingPhysicsReport, EnhancedPhysicsReport, FinalSeverity, HistoryEntry, RiskLevel,
    StrategicAdvisory, TicketEvent, TicketSeverity, TicketStage, TicketType, VerificationResult,
    VerificationStatus, WitsPacket,
};
use std::sync::{Arc, RwLock};

/// Strategic Agent for drilling advisory verification
///
/// Takes tickets from the tactical agent and performs comprehensive analysis:
/// 1. Physics-based drilling calculations (MSE trends, d-exponent, etc.)
/// 2. Baseline context verification (z-score consistency)
/// 3. Contextual knowledge lookup from vector DB
/// 4. LLM-powered advisory generation
pub struct StrategicAgent {
    /// Count of analyses performed
    analyses_performed: u64,
    /// Whether to use verbose output
    verbose: bool,
    /// Optional LLM for advisory generation
    llm: Option<Arc<StrategicLLM>>,
    /// Optional threshold manager for baseline context
    threshold_manager: Option<Arc<RwLock<ThresholdManager>>>,
    /// Equipment ID for baseline lookups
    equipment_id: String,
}

impl std::fmt::Debug for StrategicAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StrategicAgent")
            .field("analyses_performed", &self.analyses_performed)
            .field("verbose", &self.verbose)
            .field("llm_enabled", &self.llm.is_some())
            .field("baseline_enabled", &self.threshold_manager.is_some())
            .field("equipment_id", &self.equipment_id)
            .finish()
    }
}

impl StrategicAgent {
    /// Create a new strategic agent (no LLM, uses mock diagnosis)
    pub fn new() -> Self {
        Self {
            analyses_performed: 0,
            verbose: false,
            llm: None,
            threshold_manager: None,
            equipment_id: "RIG".to_string(),
        }
    }

    /// Create with LLM enabled for real diagnosis
    pub fn with_llm(llm: Arc<StrategicLLM>) -> Self {
        Self {
            analyses_performed: 0,
            verbose: false,
            llm: Some(llm),
            threshold_manager: None,
            equipment_id: "RIG".to_string(),
        }
    }

    /// Create with dynamic thresholds enabled for baseline context
    pub fn with_thresholds(
        equipment_id: &str,
        threshold_manager: Arc<RwLock<ThresholdManager>>,
    ) -> Self {
        Self {
            analyses_performed: 0,
            verbose: false,
            llm: None,
            threshold_manager: Some(threshold_manager),
            equipment_id: equipment_id.to_string(),
        }
    }

    /// Create with both LLM and dynamic thresholds
    pub fn with_llm_and_thresholds(
        equipment_id: &str,
        llm: Arc<StrategicLLM>,
        threshold_manager: Arc<RwLock<ThresholdManager>>,
    ) -> Self {
        Self {
            analyses_performed: 0,
            verbose: false,
            llm: Some(llm),
            threshold_manager: Some(threshold_manager),
            equipment_id: equipment_id.to_string(),
        }
    }

    /// Set or replace the LLM instance
    pub fn set_llm(&mut self, llm: Arc<StrategicLLM>) {
        self.llm = Some(llm);
    }

    /// Set or replace the threshold manager
    pub fn set_threshold_manager(&mut self, manager: Arc<RwLock<ThresholdManager>>) {
        self.threshold_manager = Some(manager);
    }

    /// Check if LLM is enabled
    pub fn has_llm(&self) -> bool {
        self.llm.is_some()
    }

    /// Check if baseline context is enabled
    pub fn has_baseline(&self) -> bool {
        self.threshold_manager.is_some()
    }

    /// Verify an advisory ticket using physics engine analysis
    ///
    /// This is the core of the two-stage verification system. It analyzes
    /// the ticket using enhanced physics calculations and applies decision
    /// logic to determine if the advisory should be confirmed or rejected.
    ///
    /// ## Decision Logic
    ///
    /// - Well Control issues: Almost always confirmed (safety critical)
    /// - MSE inefficiency: Confirm if sustained trend
    /// - Mechanical issues: Confirm if trend is consistent
    /// - Formation changes: Usually uncertain, needs monitoring
    pub fn verify_ticket(
        &mut self,
        ticket: &AdvisoryTicket,
        history: &[HistoryEntry],
    ) -> VerificationResult {
        self.analyses_performed += 1;

        // Clone the ticket so we can add trace events
        let mut traced_ticket = ticket.clone();

        // Get enhanced physics analysis
        let enhanced_physics = physics_engine::enhanced_strategic_analysis(history);

        // Log physics analysis results
        traced_ticket.log_info(
            TicketStage::StrategicPhysics,
            format!(
                "Physics: MSE_avg={:.0}, Trend={:.4}, Efficiency={:.0}%, Confidence={:.2}",
                enhanced_physics.base.avg_mse,
                enhanced_physics.base.mse_trend,
                enhanced_physics.base.mse_efficiency,
                enhanced_physics.confidence_factor
            ),
        );

        // Apply verification logic based on anomaly category
        let (status, reasoning, final_severity, send_to_dashboard) =
            self.apply_verification_logic(&mut traced_ticket, &enhanced_physics, history);

        // Log the final decision
        let final_status = match status {
            VerificationStatus::Confirmed => CheckStatus::Passed,
            VerificationStatus::Rejected => CheckStatus::Failed,
            _ => CheckStatus::Inconclusive,
        };
        traced_ticket.log_event(TicketEvent::new(
            TicketStage::FinalDecision,
            final_status,
            format!("{}: {}", status, self.truncate_text(&reasoning, 80)),
        ));

        if self.verbose {
            println!("\n=== Verification Result ===");
            println!("Ticket: {} - {}", ticket.ticket_type, ticket.category);
            println!("Status: {}", status);
            println!("Reasoning: {}", reasoning);
            println!("Trace Log:");
            for event in &traced_ticket.trace_log {
                println!("  {}", event.to_log_line());
            }
            println!("===========================\n");
        }

        VerificationResult {
            ticket: traced_ticket,
            status,
            physics_report: enhanced_physics.base,
            reasoning,
            final_severity,
            send_to_dashboard,
        }
    }

    /// Apply verification decision logic based on anomaly category
    fn apply_verification_logic(
        &self,
        ticket: &mut AdvisoryTicket,
        physics: &EnhancedPhysicsReport,
        history: &[HistoryEntry],
    ) -> (VerificationStatus, String, FinalSeverity, bool) {
        // Log category-specific check
        ticket.log_info(
            TicketStage::StrategicPhysics,
            format!("Verifying {} advisory", ticket.category),
        );

        match ticket.category {
            AnomalyCategory::WellControl => {
                self.verify_well_control(ticket, physics, history)
            }
            AnomalyCategory::Hydraulics => {
                self.verify_hydraulics(ticket, physics, history)
            }
            AnomalyCategory::Mechanical => {
                self.verify_mechanical(ticket, physics, history)
            }
            AnomalyCategory::DrillingEfficiency => {
                self.verify_drilling_efficiency(ticket, physics, history)
            }
            AnomalyCategory::Formation => {
                self.verify_formation(ticket, physics, history)
            }
            AnomalyCategory::None => (
                VerificationStatus::Rejected,
                "No anomaly category specified".to_string(),
                FinalSeverity::Healthy,
                false,
            ),
        }
    }

    /// Verify well control advisory (SAFETY CRITICAL)
    fn verify_well_control(
        &self,
        ticket: &mut AdvisoryTicket,
        physics: &EnhancedPhysicsReport,
        history: &[HistoryEntry],
    ) -> (VerificationStatus, String, FinalSeverity, bool) {
        // Log well control check
        ticket.log_info(TicketStage::WellControlCheck, "Analyzing well control indicators");

        // Check if flow imbalance is sustained
        let recent_flow_balances: Vec<f64> = history
            .iter()
            .rev()
            .take(5)
            .map(|h| h.metrics.flow_balance)
            .collect();

        let avg_flow_balance = if !recent_flow_balances.is_empty() {
            recent_flow_balances.iter().sum::<f64>() / recent_flow_balances.len() as f64
        } else {
            ticket.current_metrics.flow_balance
        };

        // Check pit rate trend
        let recent_pit_rates: Vec<f64> = history
            .iter()
            .rev()
            .take(5)
            .map(|h| h.metrics.pit_rate)
            .collect();

        let avg_pit_rate = if !recent_pit_rates.is_empty() {
            recent_pit_rates.iter().sum::<f64>() / recent_pit_rates.len() as f64
        } else {
            ticket.current_metrics.pit_rate
        };

        // Well control is CRITICAL - almost always confirm
        let is_sustained = physics.is_sustained || recent_flow_balances.len() >= 3;

        if avg_flow_balance.abs() > 15.0 || avg_pit_rate.abs() > 10.0 {
            ticket.log_passed(
                TicketStage::WellControlCheck,
                format!("Sustained well control issue: flow={:.1}, pit_rate={:.1}", avg_flow_balance, avg_pit_rate),
            );

            let severity = if avg_flow_balance.abs() > 20.0 || avg_pit_rate.abs() > 15.0 {
                FinalSeverity::Critical
            } else {
                FinalSeverity::High
            };

            let kick_or_loss = if avg_flow_balance > 0.0 { "kick" } else { "loss" };

            return (
                VerificationStatus::Confirmed,
                format!(
                    "CONFIRMED: Sustained {} indicators. Flow balance: {:.1} gpm, Pit rate: {:.1} bbl/hr. \
                     Immediate well control response recommended.",
                    kick_or_loss, avg_flow_balance, avg_pit_rate
                ),
                severity,
                true,
            );
        }

        if is_sustained && (avg_flow_balance.abs() > 5.0 || avg_pit_rate.abs() > 3.0) {
            return (
                VerificationStatus::Confirmed,
                format!(
                    "CONFIRMED: Persistent flow imbalance ({:.1} gpm) and pit rate ({:.1} bbl/hr). \
                     Monitor closely and prepare well control response.",
                    avg_flow_balance, avg_pit_rate
                ),
                FinalSeverity::High,
                true,
            );
        }

        // Check if it was transient
        if !is_sustained && avg_flow_balance.abs() < 5.0 {
            ticket.log_failed(
                TicketStage::WellControlCheck,
                "Flow balance returned to normal - transient event",
            );
            return (
                VerificationStatus::Rejected,
                "Well control indicators returned to normal. Likely transient event.".to_string(),
                FinalSeverity::Healthy,
                false,
            );
        }

        // Uncertain - need more data
        (
            VerificationStatus::Uncertain,
            format!(
                "Well control status uncertain. Flow balance: {:.1} gpm. Continue monitoring.",
                avg_flow_balance
            ),
            FinalSeverity::Medium,
            false,
        )
    }

    /// Verify hydraulics advisory
    fn verify_hydraulics(
        &self,
        ticket: &mut AdvisoryTicket,
        physics: &EnhancedPhysicsReport,
        history: &[HistoryEntry],
    ) -> (VerificationStatus, String, FinalSeverity, bool) {
        ticket.log_info(TicketStage::HydraulicsCheck, "Analyzing hydraulics indicators");

        // Check ECD margin trend
        let ecd_margins: Vec<f64> = history
            .iter()
            .rev()
            .take(10)
            .filter_map(|h| {
                if h.metrics.ecd_margin < f64::MAX {
                    Some(h.metrics.ecd_margin)
                } else {
                    None
                }
            })
            .collect();

        let avg_ecd_margin = if !ecd_margins.is_empty() {
            ecd_margins.iter().sum::<f64>() / ecd_margins.len() as f64
        } else {
            ticket.current_metrics.ecd_margin
        };

        // Critical ECD margin
        if avg_ecd_margin < 0.1 {
            ticket.log_passed(
                TicketStage::HydraulicsCheck,
                format!("Critical ECD margin: {:.2} ppg", avg_ecd_margin),
            );
            return (
                VerificationStatus::Confirmed,
                format!(
                    "CONFIRMED: Critical ECD margin ({:.2} ppg). Risk of induced fractures. \
                     Reduce flow rate and/or reduce ROP.",
                    avg_ecd_margin
                ),
                FinalSeverity::Critical,
                true,
            );
        }

        // Check SPP deviation
        let spp_deltas: Vec<f64> = history
            .iter()
            .rev()
            .take(10)
            .map(|h| h.metrics.spp_delta)
            .collect();

        let avg_spp_delta = if !spp_deltas.is_empty() {
            spp_deltas.iter().sum::<f64>() / spp_deltas.len() as f64
        } else {
            ticket.current_metrics.spp_delta
        };

        if avg_spp_delta.abs() > 150.0 && physics.is_sustained {
            ticket.log_passed(
                TicketStage::HydraulicsCheck,
                format!("Sustained SPP deviation: {:.0} psi", avg_spp_delta),
            );
            return (
                VerificationStatus::Confirmed,
                format!(
                    "CONFIRMED: Sustained SPP deviation ({:.0} psi). Possible washout or pack-off. \
                     Investigate before continuing.",
                    avg_spp_delta
                ),
                FinalSeverity::High,
                true,
            );
        }

        // Low ECD margin warning
        if avg_ecd_margin < 0.3 {
            return (
                VerificationStatus::Confirmed,
                format!(
                    "CONFIRMED: Low ECD margin ({:.2} ppg to fracture). Consider adjusting flow rate.",
                    avg_ecd_margin
                ),
                FinalSeverity::Medium,
                true,
            );
        }

        // Transient or normal
        if avg_spp_delta.abs() < 50.0 && avg_ecd_margin > 0.3 {
            ticket.log_failed(TicketStage::HydraulicsCheck, "Hydraulics returned to normal");
            return (
                VerificationStatus::Rejected,
                "Hydraulics parameters returned to normal. Transient event.".to_string(),
                FinalSeverity::Healthy,
                false,
            );
        }

        (
            VerificationStatus::Uncertain,
            "Hydraulics status uncertain. Continue monitoring.".to_string(),
            FinalSeverity::Low,
            false,
        )
    }

    /// Verify mechanical issue advisory
    fn verify_mechanical(
        &self,
        ticket: &mut AdvisoryTicket,
        physics: &EnhancedPhysicsReport,
        history: &[HistoryEntry],
    ) -> (VerificationStatus, String, FinalSeverity, bool) {
        ticket.log_info(TicketStage::MseAnalysis, "Analyzing mechanical indicators");

        // Check torque trend
        let torque_deltas: Vec<f64> = history
            .iter()
            .rev()
            .take(10)
            .map(|h| h.metrics.torque_delta_percent)
            .collect();

        let avg_torque_delta = if !torque_deltas.is_empty() {
            torque_deltas.iter().sum::<f64>() / torque_deltas.len() as f64
        } else {
            ticket.current_metrics.torque_delta_percent
        };

        // Check for stick-slip from physics report
        let has_stick_slip = physics.base.detected_dysfunctions.iter()
            .any(|d| d.contains("Stick-slip"));

        let has_packoff = physics.base.detected_dysfunctions.iter()
            .any(|d| d.contains("Pack-off"));

        // Check for founder condition from physics report (uses full history for reliable detection)
        let has_founder = physics.base.founder_detected;
        let founder_severity = physics.base.founder_severity;
        let optimal_wob = physics.base.optimal_wob_estimate;

        // === FOUNDER DETECTION (priority over other mechanical issues) ===
        if has_founder {
            let severity_level = if founder_severity >= 0.7 {
                FinalSeverity::High
            } else if founder_severity >= 0.3 {
                FinalSeverity::Medium
            } else {
                FinalSeverity::Low
            };

            ticket.log_passed(
                TicketStage::MseAnalysis,
                format!(
                    "Founder condition confirmed: severity={:.0}%, WOB trend={:.3}, ROP trend={:.3}",
                    founder_severity * 100.0,
                    physics.base.wob_trend,
                    physics.base.rop_trend
                ),
            );

            let recommendation = if optimal_wob > 0.0 {
                format!(
                    "CONFIRMED: Founder condition - WOB increasing but ROP not responding (severity: {:.0}%). \
                     Reduce WOB to ~{:.1} klbs where ROP was optimal. Current WOB: {:.1} klbs.",
                    founder_severity * 100.0,
                    optimal_wob,
                    physics.base.current_wob
                )
            } else {
                format!(
                    "CONFIRMED: Founder condition - WOB increasing but ROP not responding (severity: {:.0}%). \
                     Reduce WOB to improve drilling efficiency. Current WOB: {:.1} klbs.",
                    founder_severity * 100.0,
                    physics.base.current_wob
                )
            };

            return (
                VerificationStatus::Confirmed,
                recommendation,
                severity_level,
                true,
            );
        }

        // === STICK-SLIP DETECTION ===
        if has_stick_slip && physics.trend_consistency > 0.5 {
            ticket.log_passed(TicketStage::MseAnalysis, "Stick-slip detected with consistent pattern");
            return (
                VerificationStatus::Confirmed,
                "CONFIRMED: Stick-slip vibration detected. Reduce WOB or increase RPM. \
                 Consider adjusting drilling parameters.".to_string(),
                FinalSeverity::Medium,
                true,
            );
        }

        // === PACK-OFF DETECTION ===
        if has_packoff || (avg_torque_delta > 0.20 && physics.is_sustained) {
            ticket.log_passed(
                TicketStage::MseAnalysis,
                format!("Pack-off condition: torque_delta={:.1}%", avg_torque_delta * 100.0),
            );
            return (
                VerificationStatus::Confirmed,
                format!(
                    "CONFIRMED: Pack-off condition indicated. Torque increase: {:.1}%. \
                     Pick up off bottom, increase flow, and work pipe.",
                    avg_torque_delta * 100.0
                ),
                FinalSeverity::High,
                true,
            );
        }

        if avg_torque_delta > 0.15 {
            return (
                VerificationStatus::Uncertain,
                format!(
                    "Elevated torque ({:.1}% increase). Monitor for pack-off development.",
                    avg_torque_delta * 100.0
                ),
                FinalSeverity::Medium,
                false,
            );
        }

        ticket.log_failed(TicketStage::MseAnalysis, "Mechanical parameters within normal range");
        (
            VerificationStatus::Rejected,
            "Mechanical parameters returned to normal. Transient event.".to_string(),
            FinalSeverity::Healthy,
            false,
        )
    }

    /// Verify drilling efficiency advisory
    fn verify_drilling_efficiency(
        &self,
        ticket: &mut AdvisoryTicket,
        physics: &EnhancedPhysicsReport,
        history: &[HistoryEntry],
    ) -> (VerificationStatus, String, FinalSeverity, bool) {
        ticket.log_info(TicketStage::MseAnalysis, "Analyzing drilling efficiency");

        // Use physics report efficiency
        let efficiency = physics.base.mse_efficiency;
        let optimal_mse = physics.base.optimal_mse;
        let avg_mse = physics.base.avg_mse;

        // Sustained low efficiency
        if efficiency < 50.0 && physics.trend_consistency > 0.5 {
            ticket.log_passed(
                TicketStage::MseAnalysis,
                format!("Low efficiency: {:.0}%", efficiency),
            );

            // Calculate recommended adjustment
            let mse_excess = avg_mse / optimal_mse;
            let recommendation = if mse_excess > 2.0 {
                "Significantly reduce WOB or increase ROP target"
            } else {
                "Optimize WOB/RPM combination"
            };

            return (
                VerificationStatus::Confirmed,
                format!(
                    "CONFIRMED: Drilling efficiency at {:.0}%. MSE: {:.0} psi (optimal: {:.0}). \
                     Recommendation: {}. Expected ROP improvement: 20-40%.",
                    efficiency, avg_mse, optimal_mse, recommendation
                ),
                FinalSeverity::Medium,
                true,
            );
        }

        if efficiency < 70.0 && physics.is_sustained {
            return (
                VerificationStatus::Confirmed,
                format!(
                    "CONFIRMED: Sub-optimal drilling efficiency ({:.0}%). \
                     Consider adjusting drilling parameters for better ROP.",
                    efficiency
                ),
                FinalSeverity::Low,
                true,
            );
        }

        // Efficiency improved
        if efficiency >= 70.0 {
            ticket.log_failed(TicketStage::MseAnalysis, "Efficiency improved");
            return (
                VerificationStatus::Rejected,
                format!("Drilling efficiency improved to {:.0}%. No action needed.", efficiency),
                FinalSeverity::Healthy,
                false,
            );
        }

        (
            VerificationStatus::Uncertain,
            format!("Efficiency at {:.0}%. Continue monitoring.", efficiency),
            FinalSeverity::Low,
            false,
        )
    }

    /// Verify formation change advisory
    fn verify_formation(
        &self,
        ticket: &mut AdvisoryTicket,
        physics: &EnhancedPhysicsReport,
        history: &[HistoryEntry],
    ) -> (VerificationStatus, String, FinalSeverity, bool) {
        ticket.log_info(TicketStage::FormationAnalysis, "Analyzing formation indicators");

        // D-exponent trend analysis
        let dxc_trend = physics.base.dxc_trend;
        let formation_hardness = physics.base.formation_hardness;

        // Significant d-exponent change indicates formation change
        if dxc_trend.abs() > 0.1 && physics.trend_consistency > 0.6 {
            let formation_type = if dxc_trend > 0.0 { "harder" } else { "softer" };
            let recommendation = if dxc_trend > 0.0 {
                "Adjust WOB/RPM for harder formation"
            } else if dxc_trend < -0.1 {
                "Possible abnormal pressure - monitor closely"
            } else {
                "Optimize parameters for new formation"
            };

            ticket.log_passed(
                TicketStage::FormationAnalysis,
                format!("Formation change: dxc_trend={:.3}", dxc_trend),
            );

            return (
                VerificationStatus::Confirmed,
                format!(
                    "CONFIRMED: Formation change detected. Drilling into {} formation \
                     (hardness: {:.1}/10). {}.",
                    formation_type, formation_hardness, recommendation
                ),
                FinalSeverity::Low,
                true,
            );
        }

        // Check for potential abnormal pressure
        if dxc_trend < -0.15 {
            return (
                VerificationStatus::Confirmed,
                format!(
                    "CONFIRMED: D-exponent decreasing trend ({:.3}). \
                     Possible abnormal pore pressure. Verify mud weight adequacy.",
                    dxc_trend
                ),
                FinalSeverity::Medium,
                true,
            );
        }

        ticket.log_failed(TicketStage::FormationAnalysis, "No significant formation change");
        (
            VerificationStatus::Uncertain,
            "Formation change not confirmed. Continue monitoring d-exponent.".to_string(),
            FinalSeverity::Low,
            false,
        )
    }

    /// Truncate text for trace log
    fn truncate_text(&self, s: &str, max_len: usize) -> String {
        if s.len() <= max_len {
            s.to_string()
        } else {
            format!("{}...", &s[..max_len - 3])
        }
    }

    /// Get the number of analyses performed
    pub fn analyses_count(&self) -> u64 {
        self.analyses_performed
    }

    /// Set verbose mode
    pub fn set_verbose(&mut self, verbose: bool) {
        self.verbose = verbose;
    }
}

impl Default for StrategicAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DrillingMetrics, RigState};
    use std::sync::Arc;

    fn create_test_ticket(category: AnomalyCategory, severity: TicketSeverity) -> AdvisoryTicket {
        AdvisoryTicket {
            timestamp: 1705564800,
            ticket_type: TicketType::RiskWarning,
            category,
            severity,
            current_metrics: DrillingMetrics {
                state: RigState::Drilling,
                operation: crate::types::Operation::ProductionDrilling,
                mse: 30000.0,
                mse_efficiency: 60.0,
                d_exponent: 1.5,
                dxc: 1.4,
                mse_delta_percent: 0.1,
                flow_balance: 15.0,
                pit_rate: 5.0,
                ecd_margin: 0.5,
                torque_delta_percent: 0.1,
                spp_delta: 50.0,
                is_anomaly: true,
                anomaly_category: category,
                anomaly_description: Some("Test anomaly".to_string()),
            },
            trigger_parameter: "flow_balance".to_string(),
            trigger_value: 15.0,
            threshold_value: 10.0,
            description: "Test advisory".to_string(),
            depth: 10000.0,
            trace_log: Vec::new(),
        }
    }

    fn create_test_history() -> Vec<HistoryEntry> {
        (0..60)
            .map(|i| {
                let packet = WitsPacket {
                    timestamp: 1705564800 + i * 60,
                    bit_depth: 10000.0 + i as f64,
                    hole_depth: 10000.0 + i as f64,
                    rop: 60.0,
                    wob: 25.0,
                    rpm: 120.0,
                    torque: 15.0,
                    bit_diameter: 8.5,
                    spp: 3000.0,
                    flow_in: 500.0,
                    flow_out: 515.0,
                    pit_volume: 800.0 + (i as f64 * 0.1),
                    ..WitsPacket::default()
                };
                let metrics = DrillingMetrics {
                    state: RigState::Drilling,
                    mse: 30000.0,
                    mse_efficiency: 60.0,
                    flow_balance: 15.0,
                    pit_rate: 5.0,
                    ..DrillingMetrics::default()
                };
                HistoryEntry {
                    packet,
                    metrics,
                    mse_contribution: 500.0,
                }
            })
            .collect()
    }

    #[test]
    fn test_verify_well_control_confirmed() {
        let mut agent = StrategicAgent::new();
        let ticket = create_test_ticket(AnomalyCategory::WellControl, TicketSeverity::High);
        let history = create_test_history();

        let result = agent.verify_ticket(&ticket, &history);

        assert_eq!(result.status, VerificationStatus::Confirmed);
        assert!(result.send_to_dashboard);
    }

    #[test]
    fn test_verification_count() {
        let mut agent = StrategicAgent::new();
        let ticket = create_test_ticket(AnomalyCategory::DrillingEfficiency, TicketSeverity::Low);
        let history = create_test_history();

        assert_eq!(agent.analyses_count(), 0);

        agent.verify_ticket(&ticket, &history);
        assert_eq!(agent.analyses_count(), 1);

        agent.verify_ticket(&ticket, &history);
        assert_eq!(agent.analyses_count(), 2);
    }
}
