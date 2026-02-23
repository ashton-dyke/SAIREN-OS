//! Tactical Agent - Phase 2-3 of the Drilling Processing Pipeline
//!
//! The Tactical Agent is responsible for:
//!
//! ## Phase 1.5: Baseline Learning (optional)
//! - Accumulates samples during commissioning window
//! - Computes baseline mean and std for each WITS metric
//! - Locks baseline after sufficient samples collected
//!
//! ## Phase 2: Basic Drilling Physics Calculations (< 15ms)
//! - MSE (Mechanical Specific Energy) calculation
//! - D-exponent and corrected dxc
//! - Flow balance and pit rate
//! - Rig state classification
//! - Anomaly detection
//!
//! ## Phase 3: Advisory Ticket Decision
//! - Create AdvisoryTicket for:
//!   - MSE efficiency < 70% (optimization opportunity)
//!   - Flow imbalance > 10 gpm (well control)
//!   - Torque increase > 15% (mechanical issue)
//!   - D-exponent trend change (formation change)
//! - State filter: Only during Drilling or Reaming
//! - Cooldown: 60 seconds (CRITICAL bypasses)

use crate::baseline::{wits_metrics, ThresholdManager};
use crate::physics_engine;
use crate::types::{
    AdvisoryTicket, AnomalyCategory, Campaign, CfcFeatureSurpriseInfo, DrillingMetrics,
    HistoryEntry, Operation, RigState, ThresholdBreach, TicketContext, TicketSeverity,
    TicketStage, TicketType, WitsPacket,
};

// ============================================================================
// Operation Detection Thresholds
// ============================================================================

/// Thresholds for automatic operation classification
pub mod operation_thresholds {
    /// Minimum torque for Milling detection (kft-lb)
    pub const MILLING_TORQUE_MIN: f64 = 15.0;
    /// Maximum ROP for Milling detection (ft/hr) - milling has very low ROP
    pub const MILLING_ROP_MAX: f64 = 5.0;
    /// Minimum flow for circulation detection (gpm)
    pub const CIRCULATION_FLOW_MIN: f64 = 50.0;

    /// Minimum WOB for Cement Drill-Out detection (klbs)
    pub const CEMENT_DRILLOUT_WOB_MIN: f64 = 15.0;
    /// Minimum torque for Cement Drill-Out detection (kft-lb)
    pub const CEMENT_DRILLOUT_TORQUE_MIN: f64 = 12.0;
    /// Maximum ROP for Cement Drill-Out detection (ft/hr) - cement drilling is slow
    pub const CEMENT_DRILLOUT_ROP_MAX: f64 = 20.0;

    /// Maximum RPM to consider "not rotating" for Circulating/Static
    pub const NO_ROTATION_RPM_MAX: f64 = 10.0;
    /// Maximum WOB to consider "off bottom" (klbs)
    pub const OFF_BOTTOM_WOB_MAX: f64 = 5.0;
}
use std::sync::{Arc, RwLock};
use std::time::Instant;
use tracing::{debug, info, warn};

// ============================================================================
// Baseline Management
// ============================================================================

/// Baseline for drilling parameters (EMA tracking)
#[derive(Debug, Clone)]
pub struct DrillingBaseline {
    pub mse: f64,
    pub torque: f64,
    pub spp: f64,
    pub flow_balance: f64,
    pub pit_volume: f64,
    pub samples_collected: usize,
}

impl Default for DrillingBaseline {
    fn default() -> Self {
        Self {
            mse: 0.0,
            torque: 0.0,
            spp: 0.0,
            flow_balance: 0.0,
            pit_volume: 0.0,
            samples_collected: 0,
        }
    }
}

impl DrillingBaseline {
    /// Update baseline with new readings (exponential moving average)
    pub fn update(&mut self, packet: &WitsPacket, metrics: &DrillingMetrics) {
        let alpha = if self.samples_collected < 10 { 0.5 } else { 0.1 };

        if metrics.mse > 0.0 {
            if self.mse == 0.0 {
                self.mse = metrics.mse;
            } else {
                self.mse = self.mse * (1.0 - alpha) + metrics.mse * alpha;
            }
        }
        if packet.torque > 0.0 {
            if self.torque == 0.0 {
                self.torque = packet.torque;
            } else {
                self.torque = self.torque * (1.0 - alpha) + packet.torque * alpha;
            }
        }
        if packet.spp > 0.0 {
            if self.spp == 0.0 {
                self.spp = packet.spp;
            } else {
                self.spp = self.spp * (1.0 - alpha) + packet.spp * alpha;
            }
        }
        self.flow_balance = self.flow_balance * (1.0 - alpha) + metrics.flow_balance * alpha;
        if packet.pit_volume > 0.0 {
            if self.pit_volume == 0.0 {
                self.pit_volume = packet.pit_volume;
            } else {
                self.pit_volume = self.pit_volume * (1.0 - alpha) + packet.pit_volume * alpha;
            }
        }

        self.samples_collected += 1;
    }

    /// Reset the baseline to a clean state (e.g. after a formation transition).
    ///
    /// Clears all EMA values and resets the sample counter so the next samples
    /// are learned with the fast alpha (0.5) rather than the slow one (0.1).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Calculate MSE delta from baseline (percentage)
    pub fn mse_delta_percent(&self, current_mse: f64) -> f64 {
        if self.mse > 0.0 && self.samples_collected > 5 {
            (current_mse - self.mse) / self.mse
        } else {
            0.0
        }
    }

    /// Calculate torque delta from baseline (percentage)
    pub fn torque_delta_percent(&self, current_torque: f64) -> f64 {
        if self.torque > 0.0 && self.samples_collected > 5 {
            (current_torque - self.torque) / self.torque
        } else {
            0.0
        }
    }

    /// Calculate SPP delta from baseline (absolute psi)
    pub fn spp_delta(&self, current_spp: f64) -> f64 {
        if self.spp > 0.0 && self.samples_collected > 5 {
            current_spp - self.spp
        } else {
            0.0
        }
    }

    /// Calculate pit volume change from baseline (bbl)
    pub fn pit_volume_change(&self, current_pit: f64) -> f64 {
        if self.pit_volume > 0.0 && self.samples_collected > 5 {
            current_pit - self.pit_volume
        } else {
            0.0
        }
    }
}

// ============================================================================
// Operation Detection
// ============================================================================

/// Detect the current operation type from WITS packet parameters
///
/// Classification priority (evaluated in order):
/// 1. **Static**: No pumps (flow < 50 gpm), no rotation (RPM < 10)
/// 2. **Circulating**: Pumps on (flow >= 50 gpm), no rotation (RPM < 10), off bottom (WOB < 5)
/// 3. **Milling** (P&A): High torque (>15 kft-lb), low ROP (<5 ft/hr), circulation active
/// 4. **CementDrillOut** (P&A): High WOB (>15 klbs), moderate torque (>12 kft-lb), low ROP (<20 ft/hr)
/// 5. **ProductionDrilling**: Default when actively drilling
///
/// # Arguments
/// * `packet` - Current WITS packet with drilling parameters
/// * `campaign` - Current campaign type (affects P&A operation detection)
///
/// # Returns
/// The detected `Operation` type
pub fn detect_operation(packet: &WitsPacket, campaign: Campaign) -> Operation {
    let cfg = crate::config::get();

    let flow_active = packet.flow_in >= cfg.thresholds.rig_state.circulation_flow_min;
    let rotating = packet.rpm >= cfg.thresholds.operation_detection.no_rotation_rpm_max;
    let on_bottom = packet.wob >= cfg.thresholds.operation_detection.off_bottom_wob_max;

    // Priority 1: Static - no pumps, no rotation
    if !flow_active && !rotating {
        return Operation::Static;
    }

    // Priority 2: Circulating - pumps on, not rotating, off bottom
    if flow_active && !rotating && !on_bottom {
        return Operation::Circulating;
    }

    // P&A-specific operations (only when in P&A campaign)
    if campaign == Campaign::PlugAbandonment {
        // Priority 3: Milling - high torque, low ROP, circulating
        // Milling involves cutting casing/cement with specialized mills
        let is_milling = packet.torque >= cfg.thresholds.operation_detection.milling_torque_min
            && packet.rop < cfg.thresholds.operation_detection.milling_rop_max
            && flow_active
            && rotating;

        if is_milling {
            return Operation::Milling;
        }

        // Priority 4: Cement Drill-Out - high WOB, moderate torque, slow drilling
        // Cement drill-out involves drilling through cement plugs
        let is_cement_drillout = packet.wob >= cfg.thresholds.operation_detection.cement_drillout_wob_min
            && packet.torque >= cfg.thresholds.operation_detection.cement_drillout_torque_min
            && packet.rop > 0.0
            && packet.rop < cfg.thresholds.operation_detection.cement_drillout_rop_max
            && flow_active
            && rotating;

        if is_cement_drillout {
            return Operation::CementDrillOut;
        }
    }

    // Priority 5: Default to Production Drilling if actively drilling
    // (rotating with WOB on bottom and circulation)
    if rotating && on_bottom && flow_active {
        return Operation::ProductionDrilling;
    }

    // Fallback: If we're rotating but conditions don't match above
    // Still classify based on campaign
    if rotating || on_bottom {
        return Operation::ProductionDrilling;
    }

    // Final fallback
    Operation::Static
}

// ============================================================================
// Tactical Agent
// ============================================================================

/// Operating mode for the tactical agent
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TacticalMode {
    /// Using fixed thresholds from config
    FixedThresholds,
    /// Learning baseline - accumulating samples, no advisories generated
    BaselineLearning,
    /// Using dynamic z-score based thresholds
    DynamicThresholds,
}

impl std::fmt::Display for TacticalMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TacticalMode::FixedThresholds => write!(f, "Fixed Thresholds"),
            TacticalMode::BaselineLearning => write!(f, "Baseline Learning"),
            TacticalMode::DynamicThresholds => write!(f, "Dynamic Thresholds"),
        }
    }
}

// Cooldown values are now read from crate::config::get().advisory at runtime.

/// Tactical Agent for Phase 2-3 drilling processing
///
/// Processes WITS packets and generates advisory tickets when anomalies are detected.
pub struct TacticalAgent {
    /// Drilling parameter baseline for delta calculations
    baseline: DrillingBaseline,
    /// Previous packet for rate calculations
    prev_packet: Option<WitsPacket>,
    /// Count of packets processed
    packets_processed: u64,
    /// Count of tickets generated
    tickets_generated: u64,
    /// Current operating mode
    mode: TacticalMode,
    /// Equipment ID for this agent
    equipment_id: String,
    /// Optional shared threshold manager for dynamic thresholds
    threshold_manager: Option<Arc<RwLock<ThresholdManager>>>,
    /// ACI conformal interval tracker — second opinion on anomaly severity
    aci_tracker: crate::aci::AciTracker,
    /// Latest ACI result (only during drilling/reaming)
    aci_result: Option<crate::aci::AciDrillingResult>,
    /// CfC neural network (shadow mode — logs predictions, doesn't influence decisions)
    cfc_network: crate::cfc::CfcNetwork,
    /// Latest CfC result (only during drilling/reaming)
    cfc_result: Option<crate::cfc::CfcDrillingResult>,
    /// CfC-based formation transition detector
    cfc_formation_detector: crate::cfc::formation_detector::FormationTransitionDetector,
    /// Latest CfC formation transition event (if any)
    latest_formation_transition: Option<crate::types::FormationTransitionEvent>,
    /// Online k-means regime clusterer for CfC motor outputs
    regime_clusterer: crate::cfc::RegimeClusterer,
    /// Latest assigned regime ID (0-3)
    latest_regime_id: u8,
    /// Current regime centroids (k=4, dim=8)
    regime_centroids: [[f64; 8]; 4],
    /// Last time a ticket was created (for cooldown)
    last_ticket_time: Option<Instant>,
    /// Current campaign type for operation detection
    campaign: Campaign,
    /// Previous operation for transition logging
    previous_operation: Operation,
    /// Pending operation for debounce (requires 3 consecutive packets to confirm)
    pending_operation: Option<Operation>,
    /// Count of consecutive packets in pending operation state
    pending_operation_count: u32,
}

impl std::fmt::Debug for TacticalAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TacticalAgent")
            .field("packets_processed", &self.packets_processed)
            .field("tickets_generated", &self.tickets_generated)
            .field("mode", &self.mode)
            .field("equipment_id", &self.equipment_id)
            .finish()
    }
}

impl TacticalAgent {
    /// Create a new tactical agent with fixed thresholds
    pub fn new() -> Self {
        Self {
            baseline: DrillingBaseline::default(),
            prev_packet: None,
            packets_processed: 0,
            tickets_generated: 0,
            mode: TacticalMode::FixedThresholds,
            equipment_id: "RIG".to_string(),
            threshold_manager: None,
            aci_tracker: crate::aci::AciTracker::new(crate::aci::AciConfig::default()),
            aci_result: None,
            cfc_network: crate::cfc::CfcNetwork::new(42),
            cfc_result: None,
            cfc_formation_detector: crate::cfc::formation_detector::FormationTransitionDetector::new(),
            latest_formation_transition: None,
            regime_clusterer: crate::cfc::RegimeClusterer::new(),
            latest_regime_id: 0,
            regime_centroids: [[0.0; 8]; 4],
            last_ticket_time: None,
            campaign: Campaign::Production,
            previous_operation: Operation::Static,
            pending_operation: None,
            pending_operation_count: 0,
        }
    }

    /// Create a new tactical agent with a specific campaign
    pub fn new_with_campaign(campaign: Campaign) -> Self {
        Self {
            baseline: DrillingBaseline::default(),
            prev_packet: None,
            packets_processed: 0,
            tickets_generated: 0,
            mode: TacticalMode::FixedThresholds,
            equipment_id: "RIG".to_string(),
            threshold_manager: None,
            aci_tracker: crate::aci::AciTracker::new(crate::aci::AciConfig::default()),
            aci_result: None,
            cfc_network: crate::cfc::CfcNetwork::new(42),
            cfc_result: None,
            cfc_formation_detector: crate::cfc::formation_detector::FormationTransitionDetector::new(),
            latest_formation_transition: None,
            regime_clusterer: crate::cfc::RegimeClusterer::new(),
            latest_regime_id: 0,
            regime_centroids: [[0.0; 8]; 4],
            last_ticket_time: None,
            campaign,
            previous_operation: Operation::Static,
            pending_operation: None,
            pending_operation_count: 0,
        }
    }

    /// Create a new tactical agent with dynamic thresholds support
    pub fn new_with_thresholds(
        equipment_id: &str,
        threshold_manager: Arc<RwLock<ThresholdManager>>,
        start_in_learning_mode: bool,
    ) -> Self {
        Self::new_with_thresholds_and_campaign(
            equipment_id,
            threshold_manager,
            start_in_learning_mode,
            Campaign::Production,
        )
    }

    /// Create a new tactical agent with dynamic thresholds and campaign
    pub fn new_with_thresholds_and_campaign(
        equipment_id: &str,
        threshold_manager: Arc<RwLock<ThresholdManager>>,
        start_in_learning_mode: bool,
        campaign: Campaign,
    ) -> Self {
        let mode = if start_in_learning_mode {
            TacticalMode::BaselineLearning
        } else {
            match threshold_manager.read() {
                Ok(manager) => {
                    if manager.all_wits_locked(equipment_id) {
                        TacticalMode::DynamicThresholds
                    } else {
                        TacticalMode::BaselineLearning
                    }
                }
                Err(_) => TacticalMode::BaselineLearning,
            }
        };

        info!(
            equipment_id = %equipment_id,
            mode = %mode,
            campaign = %campaign.short_code(),
            "Created tactical agent"
        );

        Self {
            baseline: DrillingBaseline::default(),
            prev_packet: None,
            packets_processed: 0,
            tickets_generated: 0,
            mode,
            equipment_id: equipment_id.to_string(),
            threshold_manager: Some(threshold_manager),
            aci_tracker: crate::aci::AciTracker::new(crate::aci::AciConfig::default()),
            aci_result: None,
            cfc_network: crate::cfc::CfcNetwork::new(42),
            cfc_result: None,
            cfc_formation_detector: crate::cfc::formation_detector::FormationTransitionDetector::new(),
            latest_formation_transition: None,
            regime_clusterer: crate::cfc::RegimeClusterer::new(),
            latest_regime_id: 0,
            regime_centroids: [[0.0; 8]; 4],
            last_ticket_time: None,
            campaign,
            previous_operation: Operation::Static,
            pending_operation: None,
            pending_operation_count: 0,
        }
    }

    /// Get current operating mode
    pub fn mode(&self) -> TacticalMode {
        self.mode
    }

    /// Get equipment ID
    pub fn equipment_id(&self) -> &str {
        &self.equipment_id
    }

    /// Set operating mode
    pub fn set_mode(&mut self, mode: TacticalMode) {
        info!(old_mode = %self.mode, new_mode = %mode, "Tactical agent mode changed");
        self.mode = mode;
    }

    /// Process a WITS packet through Phase 2-3
    ///
    /// Returns:
    /// - `Option<AdvisoryTicket>` - Present if anomaly detected
    /// - `DrillingMetrics` - Always returned
    /// - `HistoryEntry` - Always returned for Phase 4 buffer
    pub fn process(
        &mut self,
        packet: &WitsPacket,
        has_active_advisory: bool,
    ) -> (Option<AdvisoryTicket>, DrillingMetrics, HistoryEntry) {
        let start = Instant::now();
        self.packets_processed += 1;

        // ====================================================================
        // PHASE 1.5: Baseline Learning (if in learning mode)
        // ====================================================================
        if self.mode == TacticalMode::BaselineLearning {
            self.feed_baseline_samples(packet);
            self.try_auto_lock_baselines(packet.timestamp);
        }

        // ====================================================================
        // PHASE 2: Basic Drilling Physics Calculations (target: < 15ms)
        // ====================================================================
        let mut metrics =
            physics_engine::tactical_update(packet, self.prev_packet.as_ref());

        // Update metrics with baseline deltas
        metrics.mse_delta_percent = self.baseline.mse_delta_percent(metrics.mse);
        metrics.torque_delta_percent = self.baseline.torque_delta_percent(packet.torque);
        metrics.spp_delta = self.baseline.spp_delta(packet.spp);

        // ====================================================================
        // PHASE 2.5: Operation Classification
        // ====================================================================
        let detected_operation = detect_operation(packet, self.campaign);
        metrics.operation = detected_operation;

        // Debounced operation transitions: require 3 consecutive packets in new state
        if detected_operation != self.previous_operation {
            if self.pending_operation == Some(detected_operation) {
                self.pending_operation_count += 1;
                if self.pending_operation_count >= 3 {
                    info!(
                        previous = %self.previous_operation.short_code(),
                        current = %detected_operation.short_code(),
                        campaign = %self.campaign.short_code(),
                        depth = packet.bit_depth,
                        rpm = packet.rpm,
                        wob = packet.wob,
                        torque = packet.torque,
                        rop = packet.rop,
                        flow_in = packet.flow_in,
                        "Operation transition detected"
                    );
                    self.previous_operation = detected_operation;
                    self.pending_operation = None;
                    self.pending_operation_count = 0;
                }
            } else {
                self.pending_operation = Some(detected_operation);
                self.pending_operation_count = 1;
            }
            // Hold previous operation until transition is confirmed
            metrics.operation = self.previous_operation;
        } else {
            self.pending_operation = None;
            self.pending_operation_count = 0;
        }

        let elapsed = start.elapsed();
        if elapsed.as_millis() > 15 {
            warn!(elapsed_ms = elapsed.as_millis(), "Phase 2 exceeded 15ms target");
        }

        // ====================================================================
        // PHASE 2.7: ACI Conformal Interval Update (during drilling/reaming)
        // ====================================================================
        self.aci_result = if metrics.state == RigState::Drilling || metrics.state == RigState::Reaming {
            Some(crate::aci::update_from_drilling(&mut self.aci_tracker, packet, &metrics))
        } else {
            None
        };

        // ====================================================================
        // PHASE 2.8: CfC Neural Network Update (shadow mode, drilling/reaming)
        // ====================================================================
        self.cfc_result = if metrics.state == RigState::Drilling || metrics.state == RigState::Reaming {
            Some(crate::cfc::update_from_drilling(&mut self.cfc_network, packet, &metrics, 1.0))
        } else {
            None
        };

        // ====================================================================
        // PHASE 2.8.1: Regime clustering from CfC motor outputs
        // ====================================================================
        if let Some(ref result) = self.cfc_result {
            if !result.motor_outputs.is_empty() {
                self.latest_regime_id = self.regime_clusterer.assign(&result.motor_outputs);
                self.regime_centroids = self.regime_clusterer.centroids();
            }
        }

        // ====================================================================
        // PHASE 2.9: CfC Formation Transition Detection
        // ====================================================================
        self.latest_formation_transition = if let Some(ref result) = self.cfc_result {
            if result.is_calibrated {
                self.cfc_formation_detector.check(
                    &result.feature_sigmas,
                    has_active_advisory,
                    packet.timestamp,
                    packet.bit_depth,
                )
            } else {
                None
            }
        } else {
            None
        };

        if let Some(ref event) = self.latest_formation_transition {
            info!(
                "CfC formation transition at depth {:.0} ft — surprised features: {:?}",
                event.bit_depth, event.surprised_features
            );
            // Reset the EMA baseline so the new formation builds its own reference
            // values from scratch rather than being compared against the previous one.
            self.baseline.reset();
        }

        // Create history entry (ALWAYS stored in Phase 4 buffer)
        let mse_contribution = self.calculate_mse_contribution(&metrics);
        let history_entry = HistoryEntry {
            packet: packet.clone(),
            metrics: metrics.clone(),
            mse_contribution,
        };

        // Update baseline
        self.baseline.update(packet, &metrics);

        // Store packet for next iteration
        self.prev_packet = Some(packet.clone());

        // ====================================================================
        // PHASE 3: Advisory Ticket Decision
        // ====================================================================

        // During baseline learning, never create tickets
        if self.mode == TacticalMode::BaselineLearning {
            return (None, metrics, history_entry);
        }

        let advisory_ticket = self.decide_advisory_ticket(packet, &metrics);

        if advisory_ticket.is_some() {
            self.tickets_generated += 1;
            info!(
                timestamp = packet.timestamp,
                depth = packet.bit_depth,
                state = %metrics.state,
                category = %metrics.anomaly_category,
                "Advisory ticket created"
            );
        }

        (advisory_ticket, metrics, history_entry)
    }

    /// Feed samples to the baseline accumulator during learning phase.
    /// Only feeds during active drilling/reaming states so baselines represent
    /// actual drilling conditions, not idle/connection noise.
    fn feed_baseline_samples(&mut self, packet: &WitsPacket) {
        // Only learn from drilling/reaming states — baselines trained on idle data
        // (zero WOB, zero torque, zero ROP) produce meaningless thresholds.
        let state = crate::physics_engine::classify_rig_state(packet);
        if state != RigState::Drilling && state != RigState::Reaming {
            return;
        }
        if let Some(ref manager) = self.threshold_manager {
            let mut mgr = match manager.write() {
                Ok(m) => m,
                Err(_) => return,
            };
            let timestamp = packet.timestamp;

            // Feed all WITS metrics for baseline learning
            mgr.add_sample(&self.equipment_id, wits_metrics::MSE, packet.mse, timestamp);
            mgr.add_sample(&self.equipment_id, wits_metrics::D_EXPONENT, packet.d_exponent, timestamp);
            mgr.add_sample(&self.equipment_id, wits_metrics::DXC, packet.dxc, timestamp);
            mgr.add_sample(&self.equipment_id, wits_metrics::FLOW_BALANCE, packet.flow_balance(), timestamp);
            mgr.add_sample(&self.equipment_id, wits_metrics::SPP, packet.spp, timestamp);
            mgr.add_sample(&self.equipment_id, wits_metrics::TORQUE, packet.torque, timestamp);
            mgr.add_sample(&self.equipment_id, wits_metrics::ROP, packet.rop, timestamp);
            mgr.add_sample(&self.equipment_id, wits_metrics::WOB, packet.wob, timestamp);
            mgr.add_sample(&self.equipment_id, wits_metrics::RPM, packet.rpm, timestamp);
            mgr.add_sample(&self.equipment_id, wits_metrics::ECD, packet.ecd, timestamp);
            mgr.add_sample(&self.equipment_id, wits_metrics::PIT_VOLUME, packet.pit_volume, timestamp);
            mgr.add_sample(&self.equipment_id, wits_metrics::GAS_UNITS, packet.gas_units, timestamp);
        }
    }

    /// Try to auto-lock baselines when enough samples are collected
    fn try_auto_lock_baselines(&mut self, timestamp: u64) {
        if let Some(ref manager) = self.threshold_manager {
            let mut mgr = match manager.write() {
                Ok(m) => m,
                Err(_) => return,
            };

            let status = mgr.get_status(&self.equipment_id, wits_metrics::MSE);
            let should_lock = match status {
                Some(crate::baseline::LearningStatus::Learning { samples_collected, .. }) => {
                    samples_collected >= 100
                }
                _ => false,
            };

            if should_lock {
                let locked = mgr.try_lock_all_wits(&self.equipment_id, timestamp);
                if !locked.is_empty() {
                    info!(
                        equipment_id = %self.equipment_id,
                        locked_metrics = ?locked,
                        "Auto-locked baselines, switching to DynamicThresholds mode"
                    );
                    drop(mgr);
                    self.mode = TacticalMode::DynamicThresholds;
                }
            }
        }
    }

    /// Decide whether to create an advisory ticket for strategic validation
    fn decide_advisory_ticket(
        &mut self,
        packet: &WitsPacket,
        metrics: &DrillingMetrics,
    ) -> Option<AdvisoryTicket> {
        // RULE 1: Only create tickets during active drilling states
        if metrics.state != RigState::Drilling && metrics.state != RigState::Reaming {
            return None;
        }

        // RULE 2: Must have detected anomaly
        if !metrics.is_anomaly {
            return None;
        }

        // Determine severity and ticket type
        let (severity, ticket_type) = self.determine_severity_and_type(metrics);

        // RULE 3: Cooldown period between tickets
        // CRITICAL tickets bypass cooldown when configured, otherwise use default
        let cfg = crate::config::get();
        if let Some(last_time) = self.last_ticket_time {
            let elapsed = last_time.elapsed().as_secs();
            let cooldown = if severity == TicketSeverity::Critical
                && cfg.advisory.critical_bypass_cooldown
            {
                0
            } else {
                cfg.advisory.default_cooldown_seconds
            };

            if elapsed < cooldown {
                debug!(
                    elapsed_secs = elapsed,
                    cooldown_secs = cooldown,
                    severity = ?severity,
                    "Ticket suppressed - cooldown active"
                );
                return None;
            }

            if severity == TicketSeverity::Critical {
                info!("CRITICAL ticket after {}s cooldown", elapsed);
            }
        }

        // Determine trigger parameter and value
        let (trigger_parameter, trigger_value, threshold_value) =
            self.determine_trigger(metrics);

        // Build description
        let description = metrics
            .anomaly_description
            .clone()
            .unwrap_or_else(|| format!("{} anomaly detected", metrics.anomaly_category));

        // Update cooldown timer
        self.last_ticket_time = Some(Instant::now());

        // Build structured context (replaces tactical LLM description)
        let context = self.build_ticket_context(packet, metrics);

        // Create the ticket with trace log
        let mut ticket = AdvisoryTicket {
            timestamp: packet.timestamp,
            ticket_type,
            category: metrics.anomaly_category,
            severity,
            current_metrics: metrics.clone(),
            trigger_parameter,
            trigger_value,
            threshold_value,
            description,
            context: Some(context),
            depth: packet.bit_depth,
            trace_log: Vec::new(),
            cfc_anomaly_score: self.cfc_result.as_ref()
                .filter(|r| r.is_calibrated)
                .map(|r| r.anomaly_score),
            cfc_feature_surprises: self.cfc_result.as_ref()
                .filter(|r| r.is_calibrated)
                .map(|r| r.feature_surprises.iter().map(|s| CfcFeatureSurpriseInfo {
                    name: s.name.to_string(), error: s.error, magnitude: s.magnitude,
                }).collect())
                .unwrap_or_default(),
            causal_leads: Vec::new(),
        };

        // Log creation event (Flight Recorder entry #1)
        let creation_msg = format!(
            "Created: {} | {} | Trigger: {:.2} vs threshold {:.2}",
            ticket.ticket_type, ticket.category, ticket.trigger_value, ticket.threshold_value
        );
        ticket.log_info(TicketStage::TacticalCreation, creation_msg);

        // Log CfC neural network state
        if let Some(cfc) = &self.cfc_result {
            ticket.log_info(TicketStage::TacticalCreation,
                format!("CfC: anomaly={:.3} calibrated={} surprises={}",
                    cfc.anomaly_score, cfc.is_calibrated, cfc.feature_surprises.len()));
        }

        Some(ticket)
    }

    /// Determine ticket severity and type based on metrics
    fn determine_severity_and_type(&self, metrics: &DrillingMetrics) -> (TicketSeverity, TicketType) {
        let cfg = crate::config::get();
        let (base_severity, ticket_type) = match metrics.anomaly_category {
            AnomalyCategory::WellControl => {
                // Well control issues are always high priority
                if metrics.flow_balance.abs() > cfg.thresholds.well_control.flow_imbalance_critical_gpm
                    || metrics.pit_rate.abs() > cfg.thresholds.well_control.pit_rate_critical_bbl_hr
                {
                    (TicketSeverity::Critical, TicketType::Intervention)
                } else {
                    (TicketSeverity::High, TicketType::RiskWarning)
                }
            }
            AnomalyCategory::Hydraulics => {
                if metrics.ecd_margin < cfg.thresholds.hydraulics.ecd_margin_critical_ppg
                    || metrics.spp_delta.abs() > cfg.thresholds.hydraulics.spp_deviation_critical_psi
                {
                    (TicketSeverity::High, TicketType::RiskWarning)
                } else {
                    (TicketSeverity::Medium, TicketType::RiskWarning)
                }
            }
            AnomalyCategory::Mechanical => {
                if metrics.torque_delta_percent > cfg.thresholds.mechanical.torque_increase_critical {
                    (TicketSeverity::High, TicketType::Intervention)
                } else {
                    (TicketSeverity::Medium, TicketType::RiskWarning)
                }
            }
            AnomalyCategory::DrillingEfficiency => {
                if metrics.mse_efficiency < cfg.thresholds.mse.efficiency_poor_percent {
                    (TicketSeverity::Medium, TicketType::Optimization)
                } else {
                    (TicketSeverity::Low, TicketType::Optimization)
                }
            }
            AnomalyCategory::Formation => (TicketSeverity::Low, TicketType::Optimization),
            AnomalyCategory::None => (TicketSeverity::Low, TicketType::Optimization),
        };

        // ── ACI second opinion ──
        // If ACI is calibrated, use it to modulate severity:
        //   - 3+ outliers: ACI confirms → maintain or escalate severity
        //   - 0-1 outliers: ACI disagrees → downgrade by one level
        // Well control is never downgraded below High (safety critical).
        let severity_after_aci = self.aci_adjust_severity(base_severity, &metrics.anomaly_category);

        if severity_after_aci != base_severity {
            debug!(
                base = ?base_severity,
                adjusted = ?severity_after_aci,
                category = ?metrics.anomaly_category,
                aci_outliers = self.aci_result.as_ref().map(|r| r.outlier_count).unwrap_or(0),
                "ACI adjusted ticket severity"
            );
        }

        // ── CfC neural network second opinion ──
        let severity = self.cfc_adjust_severity(severity_after_aci, &metrics.anomaly_category);

        if severity != severity_after_aci {
            debug!(
                before_cfc = ?severity_after_aci,
                after_cfc = ?severity,
                category = ?metrics.anomaly_category,
                cfc_score = self.cfc_result.as_ref().map(|r| r.anomaly_score).unwrap_or(0.0),
                "CfC adjusted ticket severity"
            );
        }

        (severity, ticket_type)
    }

    /// Use ACI conformal intervals as a second opinion on severity.
    /// Downgrades severity by one level when ACI doesn't corroborate the anomaly.
    /// Escalates by one level when ACI strongly confirms (5+ outliers).
    /// Never downgrades well control below High.
    fn aci_adjust_severity(
        &self,
        base: TicketSeverity,
        category: &AnomalyCategory,
    ) -> TicketSeverity {
        use crate::aci::metrics as aci_m;

        // Only adjust if ACI has enough samples to be reliable
        if !self.aci_tracker.is_calibrated(aci_m::MSE) {
            return base;
        }

        let aci = match &self.aci_result {
            Some(r) => r,
            None => return base,
        };

        let outliers = aci.outlier_count;

        match outliers {
            // 0-1 outliers: ACI doesn't see an anomaly → downgrade
            0..=1 => {
                match category {
                    // Never downgrade well control below High
                    AnomalyCategory::WellControl => {
                        if base == TicketSeverity::Critical {
                            TicketSeverity::High
                        } else {
                            base
                        }
                    }
                    _ => match base {
                        TicketSeverity::Critical => TicketSeverity::High,
                        TicketSeverity::High => TicketSeverity::Medium,
                        TicketSeverity::Medium => TicketSeverity::Low,
                        TicketSeverity::Low => TicketSeverity::Low,
                    },
                }
            }
            // 2-4 outliers: moderate agreement → keep as-is
            2..=4 => base,
            // 5+ outliers: strong ACI confirmation → escalate
            _ => match base {
                TicketSeverity::Low => TicketSeverity::Medium,
                TicketSeverity::Medium => TicketSeverity::High,
                TicketSeverity::High => TicketSeverity::Critical,
                TicketSeverity::Critical => TicketSeverity::Critical,
            },
        }
    }

    /// Use CfC neural network anomaly score as a second opinion on severity.
    /// Downgrades severity by one level when CfC sees nothing abnormal (score < 0.3).
    /// Escalates by one level when CfC strongly corroborates (score >= 0.7).
    /// Never downgrades well control below High (safety critical).
    fn cfc_adjust_severity(
        &self,
        base: TicketSeverity,
        category: &AnomalyCategory,
    ) -> TicketSeverity {
        let cfc = match &self.cfc_result {
            Some(r) if r.is_calibrated => r,
            _ => return base, // Not calibrated or no result → no change
        };

        let score = cfc.anomaly_score;

        if score < 0.3 {
            // CfC sees nothing abnormal → downgrade one level
            match category {
                // Never downgrade well control below High
                AnomalyCategory::WellControl => {
                    if base == TicketSeverity::Critical {
                        TicketSeverity::High
                    } else {
                        base
                    }
                }
                _ => match base {
                    TicketSeverity::Critical => TicketSeverity::High,
                    TicketSeverity::High => TicketSeverity::Medium,
                    TicketSeverity::Medium => TicketSeverity::Low,
                    TicketSeverity::Low => TicketSeverity::Low,
                },
            }
        } else if score >= 0.7 {
            // CfC strongly corroborates → escalate one level
            match base {
                TicketSeverity::Low => TicketSeverity::Medium,
                TicketSeverity::Medium => TicketSeverity::High,
                TicketSeverity::High => TicketSeverity::Critical,
                TicketSeverity::Critical => TicketSeverity::Critical,
            }
        } else {
            // 0.3 <= score < 0.7: ambiguous → no change
            base
        }
    }

    /// Determine the primary trigger parameter and its value
    fn determine_trigger(&self, metrics: &DrillingMetrics) -> (String, f64, f64) {
        let cfg = crate::config::get();
        match metrics.anomaly_category {
            AnomalyCategory::WellControl => {
                if metrics.flow_balance.abs() > cfg.thresholds.well_control.flow_imbalance_warning_gpm {
                    (
                        "flow_balance".to_string(),
                        metrics.flow_balance,
                        cfg.thresholds.well_control.flow_imbalance_warning_gpm,
                    )
                } else {
                    (
                        "pit_rate".to_string(),
                        metrics.pit_rate,
                        cfg.thresholds.well_control.pit_rate_warning_bbl_hr,
                    )
                }
            }
            AnomalyCategory::Hydraulics => {
                if metrics.ecd_margin < cfg.thresholds.hydraulics.ecd_margin_warning_ppg {
                    (
                        "ecd_margin".to_string(),
                        metrics.ecd_margin,
                        cfg.thresholds.hydraulics.ecd_margin_warning_ppg,
                    )
                } else {
                    (
                        "spp_delta".to_string(),
                        metrics.spp_delta,
                        cfg.thresholds.hydraulics.spp_deviation_warning_psi,
                    )
                }
            }
            AnomalyCategory::Mechanical => (
                "torque_delta_percent".to_string(),
                metrics.torque_delta_percent,
                cfg.thresholds.mechanical.torque_increase_warning,
            ),
            AnomalyCategory::DrillingEfficiency => (
                "mse_efficiency".to_string(),
                metrics.mse_efficiency,
                cfg.thresholds.mse.efficiency_warning_percent,
            ),
            AnomalyCategory::Formation => (
                "d_exponent".to_string(),
                metrics.d_exponent,
                0.0, // No fixed threshold for formation changes
            ),
            AnomalyCategory::None => ("unknown".to_string(), 0.0, 0.0),
        }
    }

    /// Build structured TicketContext from current metrics and packet.
    ///
    /// Collects ALL threshold breaches (not just the primary trigger) so the
    /// strategic agent has a complete picture of what went wrong.
    fn build_ticket_context(
        &self,
        packet: &WitsPacket,
        metrics: &DrillingMetrics,
    ) -> TicketContext {
        let cfg = crate::config::get();
        let mut triggers = Vec::new();

        // --- Well Control breaches ---
        if metrics.flow_balance.abs() > cfg.thresholds.well_control.flow_imbalance_warning_gpm {
            let is_critical = metrics.flow_balance.abs()
                > cfg.thresholds.well_control.flow_imbalance_critical_gpm;
            triggers.push(ThresholdBreach {
                parameter: "flow_balance".into(),
                actual_value: metrics.flow_balance,
                threshold_value: if is_critical {
                    cfg.thresholds.well_control.flow_imbalance_critical_gpm
                } else {
                    cfg.thresholds.well_control.flow_imbalance_warning_gpm
                },
                threshold_type: if is_critical { "CRITICAL" } else { "WARNING" }.into(),
                unit: "gpm".into(),
            });
        }
        if metrics.pit_rate.abs() > cfg.thresholds.well_control.pit_rate_warning_bbl_hr {
            let is_critical =
                metrics.pit_rate.abs() > cfg.thresholds.well_control.pit_rate_critical_bbl_hr;
            triggers.push(ThresholdBreach {
                parameter: "pit_rate".into(),
                actual_value: metrics.pit_rate,
                threshold_value: if is_critical {
                    cfg.thresholds.well_control.pit_rate_critical_bbl_hr
                } else {
                    cfg.thresholds.well_control.pit_rate_warning_bbl_hr
                },
                threshold_type: if is_critical { "CRITICAL" } else { "WARNING" }.into(),
                unit: "bbl/hr".into(),
            });
        }
        if packet.gas_units > cfg.thresholds.well_control.gas_units_warning {
            let is_critical = packet.gas_units > cfg.thresholds.well_control.gas_units_critical;
            triggers.push(ThresholdBreach {
                parameter: "gas_units".into(),
                actual_value: packet.gas_units,
                threshold_value: if is_critical {
                    cfg.thresholds.well_control.gas_units_critical
                } else {
                    cfg.thresholds.well_control.gas_units_warning
                },
                threshold_type: if is_critical { "CRITICAL" } else { "WARNING" }.into(),
                unit: "units".into(),
            });
        }
        if packet.h2s > cfg.thresholds.well_control.h2s_warning_ppm {
            let is_critical = packet.h2s > cfg.thresholds.well_control.h2s_critical_ppm;
            triggers.push(ThresholdBreach {
                parameter: "h2s".into(),
                actual_value: packet.h2s,
                threshold_value: if is_critical {
                    cfg.thresholds.well_control.h2s_critical_ppm
                } else {
                    cfg.thresholds.well_control.h2s_warning_ppm
                },
                threshold_type: if is_critical { "CRITICAL" } else { "WARNING" }.into(),
                unit: "ppm".into(),
            });
        }

        // --- Hydraulics breaches ---
        if metrics.ecd_margin < cfg.thresholds.hydraulics.ecd_margin_warning_ppg {
            let is_critical =
                metrics.ecd_margin < cfg.thresholds.hydraulics.ecd_margin_critical_ppg;
            triggers.push(ThresholdBreach {
                parameter: "ecd_margin".into(),
                actual_value: metrics.ecd_margin,
                threshold_value: if is_critical {
                    cfg.thresholds.hydraulics.ecd_margin_critical_ppg
                } else {
                    cfg.thresholds.hydraulics.ecd_margin_warning_ppg
                },
                threshold_type: if is_critical { "CRITICAL" } else { "WARNING" }.into(),
                unit: "ppg".into(),
            });
        }
        if metrics.spp_delta.abs() > cfg.thresholds.hydraulics.spp_deviation_warning_psi {
            let is_critical = metrics.spp_delta.abs()
                > cfg.thresholds.hydraulics.spp_deviation_critical_psi;
            triggers.push(ThresholdBreach {
                parameter: "spp_delta".into(),
                actual_value: metrics.spp_delta,
                threshold_value: if is_critical {
                    cfg.thresholds.hydraulics.spp_deviation_critical_psi
                } else {
                    cfg.thresholds.hydraulics.spp_deviation_warning_psi
                },
                threshold_type: if is_critical { "CRITICAL" } else { "WARNING" }.into(),
                unit: "psi".into(),
            });
        }

        // --- Mechanical breaches ---
        if metrics.torque_delta_percent > cfg.thresholds.mechanical.torque_increase_warning {
            let is_critical = metrics.torque_delta_percent
                > cfg.thresholds.mechanical.torque_increase_critical;
            triggers.push(ThresholdBreach {
                parameter: "torque_increase".into(),
                actual_value: metrics.torque_delta_percent * 100.0,
                threshold_value: if is_critical {
                    cfg.thresholds.mechanical.torque_increase_critical * 100.0
                } else {
                    cfg.thresholds.mechanical.torque_increase_warning * 100.0
                },
                threshold_type: if is_critical { "CRITICAL" } else { "WARNING" }.into(),
                unit: "%".into(),
            });
        }

        // --- Efficiency breaches ---
        if metrics.mse_efficiency < cfg.thresholds.mse.efficiency_warning_percent {
            let is_poor =
                metrics.mse_efficiency < cfg.thresholds.mse.efficiency_poor_percent;
            triggers.push(ThresholdBreach {
                parameter: "mse_efficiency".into(),
                actual_value: metrics.mse_efficiency,
                threshold_value: if is_poor {
                    cfg.thresholds.mse.efficiency_poor_percent
                } else {
                    cfg.thresholds.mse.efficiency_warning_percent
                },
                threshold_type: if is_poor { "CRITICAL" } else { "WARNING" }.into(),
                unit: "%".into(),
            });
        }

        // Determine pattern name from anomaly category + description
        let pattern = self.detect_pattern_name(metrics, packet);

        TicketContext {
            triggers,
            pattern,
            rig_state: metrics.state,
            operation: metrics.operation,
            campaign: self.campaign,
        }
    }

    /// Map anomaly to a human-readable pattern name using the routing table.
    fn detect_pattern_name(&self, metrics: &DrillingMetrics, packet: &WitsPacket) -> String {
        let cfg = crate::config::get();
        match metrics.anomaly_category {
            AnomalyCategory::WellControl => {
                if metrics.flow_balance > cfg.thresholds.well_control.flow_imbalance_critical_gpm {
                    "Kick".into()
                } else if metrics.flow_balance
                    > cfg.thresholds.well_control.flow_imbalance_warning_gpm
                {
                    "Kick Warning".into()
                } else if metrics.flow_balance
                    < -cfg.thresholds.well_control.flow_imbalance_critical_gpm
                {
                    "Lost Circulation".into()
                } else if metrics.flow_balance
                    < -cfg.thresholds.well_control.flow_imbalance_warning_gpm
                {
                    "Loss Warning".into()
                } else if packet.gas_units > cfg.thresholds.well_control.gas_units_critical
                    || packet.h2s > cfg.thresholds.well_control.h2s_critical_ppm
                {
                    "Gas Critical".into()
                } else if packet.gas_units > cfg.thresholds.well_control.gas_units_warning {
                    "Gas Warning".into()
                } else if metrics.pit_rate.abs()
                    > cfg.thresholds.well_control.pit_rate_critical_bbl_hr
                {
                    "Pit Rate Critical".into()
                } else {
                    "Well Control Warning".into()
                }
            }
            AnomalyCategory::Hydraulics => {
                if metrics.ecd_margin < cfg.thresholds.hydraulics.ecd_margin_critical_ppg {
                    "ECD Critical".into()
                } else if metrics.ecd_margin < cfg.thresholds.hydraulics.ecd_margin_warning_ppg {
                    "ECD Warning".into()
                } else if metrics.spp_delta.abs()
                    > cfg.thresholds.hydraulics.spp_deviation_critical_psi
                {
                    "SPP Critical".into()
                } else {
                    "SPP Warning".into()
                }
            }
            AnomalyCategory::Mechanical => {
                if metrics.torque_delta_percent
                    > cfg.thresholds.mechanical.torque_increase_critical
                    && metrics.spp_delta > cfg.thresholds.hydraulics.spp_deviation_warning_psi
                {
                    "Pack-off".into()
                } else if metrics.torque_delta_percent
                    > cfg.thresholds.mechanical.torque_increase_warning
                    && metrics.spp_delta > 0.0
                {
                    "Pack-off Warning".into()
                } else if metrics.torque_delta_percent
                    > cfg.thresholds.mechanical.stick_slip_cv_warning
                {
                    "Stick-slip".into()
                } else {
                    // Default to founder if mechanical but not torque-driven
                    "Founder".into()
                }
            }
            AnomalyCategory::DrillingEfficiency => {
                if metrics.mse_efficiency < cfg.thresholds.mse.efficiency_poor_percent {
                    "MSE Inefficiency".into()
                } else {
                    "MSE Warning".into()
                }
            }
            AnomalyCategory::Formation => "Formation Change".into(),
            AnomalyCategory::None => "Unknown".into(),
        }
    }

    /// Calculate MSE contribution for history entry
    fn calculate_mse_contribution(&self, metrics: &DrillingMetrics) -> f64 {
        // MSE-hours contribution = MSE * time_interval (assume 1 minute intervals)
        metrics.mse * (1.0 / 60.0)
    }

    /// Get agent statistics
    /// Get the latest ACI result (only populated during drilling/reaming)
    pub fn aci_result(&self) -> Option<&crate::aci::AciDrillingResult> {
        self.aci_result.as_ref()
    }

    /// Get a reference to the ACI tracker for interval queries
    pub fn aci_tracker(&self) -> &crate::aci::AciTracker {
        &self.aci_tracker
    }

    /// Get the latest CfC result (shadow mode, only during drilling/reaming)
    pub fn cfc_result(&self) -> Option<&crate::cfc::CfcDrillingResult> {
        self.cfc_result.as_ref()
    }

    /// Get the latest CfC formation transition event (if any)
    pub fn latest_formation_transition(&self) -> Option<&crate::types::FormationTransitionEvent> {
        self.latest_formation_transition.as_ref()
    }

    /// Get a reference to the CfC network for diagnostics
    pub fn cfc_network(&self) -> &crate::cfc::CfcNetwork {
        &self.cfc_network
    }

    /// Get the latest regime ID (0-3) from CfC motor output clustering
    pub fn latest_regime_id(&self) -> u8 {
        self.latest_regime_id
    }

    /// Get the current regime centroids (k=4, dim=8)
    pub fn regime_centroids(&self) -> [[f64; 8]; 4] {
        self.regime_centroids
    }

    pub fn stats(&self) -> AgentStats {
        AgentStats {
            packets_processed: self.packets_processed,
            tickets_generated: self.tickets_generated,
            ticket_rate: if self.packets_processed > 0 {
                (self.tickets_generated as f64 / self.packets_processed as f64) * 100.0
            } else {
                0.0
            },
        }
    }

    /// Reset agent counters
    pub fn reset(&mut self) {
        self.packets_processed = 0;
        self.tickets_generated = 0;
        self.baseline = DrillingBaseline::default();
        self.prev_packet = None;
        self.aci_tracker = crate::aci::AciTracker::new(crate::aci::AciConfig::default());
        self.aci_result = None;
        self.cfc_network.reset();
        self.cfc_result = None;
        self.cfc_formation_detector = crate::cfc::formation_detector::FormationTransitionDetector::new();
        self.latest_formation_transition = None;
        self.regime_clusterer.reset();
        self.latest_regime_id = 0;
        self.regime_centroids = [[0.0; 8]; 4];
        self.previous_operation = Operation::Static;
        self.pending_operation = None;
        self.pending_operation_count = 0;
    }

    /// Get current rig state from last processed packet
    pub fn current_state(&self) -> RigState {
        self.prev_packet
            .as_ref()
            .map(|p| p.rig_state)
            .unwrap_or(RigState::Idle)
    }

    /// Get current detected operation
    pub fn current_operation(&self) -> Operation {
        self.previous_operation
    }

    /// Get current campaign
    pub fn campaign(&self) -> Campaign {
        self.campaign
    }

    /// Set campaign type (updates operation detection behavior)
    pub fn set_campaign(&mut self, campaign: Campaign) {
        if self.campaign != campaign {
            info!(
                old_campaign = %self.campaign.short_code(),
                new_campaign = %campaign.short_code(),
                "Campaign changed - operation detection updated"
            );
            self.campaign = campaign;
        }
    }

    /// Check if baseline learning is complete
    pub fn is_baseline_locked(&self) -> bool {
        match self.mode {
            TacticalMode::BaselineLearning => false,
            TacticalMode::FixedThresholds | TacticalMode::DynamicThresholds => true,
        }
    }
}

impl Default for TacticalAgent {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Statistics
// ============================================================================

/// Statistics about tactical agent performance
#[derive(Debug, Clone)]
pub struct AgentStats {
    pub packets_processed: u64,
    pub tickets_generated: u64,
    pub ticket_rate: f64,
}

impl std::fmt::Display for AgentStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Processed: {}, Tickets: {} ({:.1}%)",
            self.packets_processed, self.tickets_generated, self.ticket_rate,
        )
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn ensure_config() {
        if !crate::config::is_initialized() {
            crate::config::init(crate::config::WellConfig::default());
        }
    }

    fn create_normal_drilling_packet() -> WitsPacket {
        let mut packet = WitsPacket::default();
        packet.timestamp = 1000;
        packet.bit_depth = 10000.0;
        packet.hole_depth = 10000.0;
        packet.rop = 60.0;
        packet.wob = 25.0;
        packet.rpm = 120.0;
        packet.torque = 15.0;
        packet.bit_diameter = 8.5;
        packet.spp = 3000.0;
        packet.flow_in = 500.0;
        packet.flow_out = 500.0;
        packet.pit_volume = 800.0;
        packet.mud_weight_in = 10.5;
        packet.ecd = 10.8;
        packet.fracture_gradient = 14.0;
        packet.gas_units = 20.0;
        packet.background_gas = 15.0;
        packet.rig_state = RigState::Drilling;
        packet
    }

    fn create_kick_packet() -> WitsPacket {
        let mut packet = create_normal_drilling_packet();
        packet.flow_out = 530.0; // 30 gpm gain
        packet.pit_volume_change = 8.0;
        packet.gas_units = 200.0;
        packet
    }

    #[test]
    fn test_normal_drilling_no_ticket() {
        ensure_config();
        let mut agent = TacticalAgent::new();
        let packet = create_normal_drilling_packet();
        let (ticket, metrics, _entry) = agent.process(&packet, false);

        // Normal drilling should not generate ticket
        assert!(ticket.is_none() || metrics.anomaly_category == AnomalyCategory::DrillingEfficiency);
    }

    #[test]
    fn test_kick_generates_ticket() {
        ensure_config();
        let mut agent = TacticalAgent::new();
        let packet = create_kick_packet();
        let (ticket, metrics, _entry) = agent.process(&packet, false);

        assert!(metrics.is_anomaly, "Kick conditions should be detected as anomaly");
        assert_eq!(
            metrics.anomaly_category,
            AnomalyCategory::WellControl,
            "Should be classified as well control"
        );
    }

    #[test]
    fn test_history_entry_always_created() {
        ensure_config();
        let mut agent = TacticalAgent::new();
        let (_, _, entry) = agent.process(&create_normal_drilling_packet(), false);
        assert!(entry.packet.timestamp > 0);
    }

    #[test]
    fn test_stats_tracking() {
        ensure_config();
        let mut agent = TacticalAgent::new();

        agent.process(&create_normal_drilling_packet(), false);
        agent.process(&create_normal_drilling_packet(), false);
        agent.process(&create_normal_drilling_packet(), false);

        let stats = agent.stats();
        assert_eq!(stats.packets_processed, 3);
    }

    #[test]
    fn test_baseline_update() {
        ensure_config();
        let mut agent = TacticalAgent::new();

        // Process several packets to build baseline
        for i in 0..20 {
            let mut packet = create_normal_drilling_packet();
            packet.timestamp = 1000 + i * 60;
            agent.process(&packet, false);
        }

        assert!(agent.baseline.samples_collected >= 20);
        assert!(agent.baseline.mse > 0.0);
    }
}
