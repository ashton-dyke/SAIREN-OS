//! Orchestrator - Phase 8 Ensemble Voting for Drilling Intelligence
//!
//! The Orchestrator collects votes from trait-based specialist agents and combines
//! them into a weighted consensus using configurable weights.
//!
//! ## Specialists and Weights
//!
//! Each specialist implements the `Specialist` trait (see `agents::specialists`):
//! 1. **MSE Specialist** (25%): Drilling efficiency analysis
//! 2. **Hydraulic Specialist** (25%): SPP, flow, ECD margin
//! 3. **WellControl Specialist** (30%): Kick/loss, gas, pit volume (safety-critical)
//! 4. **Formation Specialist** (20%): D-exponent, torque trends
//!
//! ## Voting Rules
//!
//! - **WellControl CRITICAL** overrides all others (safety critical)
//! - Otherwise: weighted average of votes converted to severity
//!
//! ## Regime-Aware Weighting (Phase 6)
//!
//! The `regime_id` (0–3) from the CfC motor-output k-means clusterer adjusts
//! each specialist's weight multiplicatively before the weighted vote is summed.
//! Weights are re-normalised to 1.0 after scaling so the existing `FinalSeverity`
//! thresholds remain calibrated.
//!
//! | Regime | Heuristic label    | Adjustment                                       |
//! |--------|--------------------|--------------------------------------------------|
//! | 0      | Baseline           | No adjustment — all multipliers = 1.0            |
//! | 1      | Hydraulic-stress   | Hydraulic ×1.4, MSE ×0.8, Formation ×0.8         |
//! | 2      | High-WOB/MSE       | MSE ×1.4, Formation ×1.1, Hydraulic ×0.8         |
//! | 3      | Unstable/kick      | WellControl ×1.5, MSE ×0.7, Formation ×0.8       |

use super::specialists::{self, Specialist};
use crate::strategic::advisory::{self, VotingResult};
use crate::types::{
    AdvisoryTicket, DrillingPhysicsReport, FinalSeverity, SpecialistVote, TicketSeverity,
};
use tracing::info;

// ============================================================================
// Regime weight profiles
// ============================================================================

/// Multiplicative weight adjustment for each specialist per CfC regime.
///
/// Applied on top of `EnsembleWeightsConfig` values then re-normalised to 1.0.
struct RegimeProfile {
    mse_mult: f64,
    hydraulic_mult: f64,
    well_control_mult: f64,
    formation_mult: f64,
    label: &'static str,
}

/// Four regime profiles indexed by regime_id (0–3).
const REGIME_PROFILES: [RegimeProfile; 4] = [
    // 0: Baseline — no adjustment (clusterer not yet calibrated, or normal state)
    RegimeProfile { mse_mult: 1.0, hydraulic_mult: 1.0, well_control_mult: 1.0, formation_mult: 1.0, label: "baseline" },
    // 1: Hydraulic-stress — elevated SPP/ECD motor patterns
    RegimeProfile { mse_mult: 0.8, hydraulic_mult: 1.4, well_control_mult: 1.0, formation_mult: 0.8, label: "hydraulic-stress" },
    // 2: High-WOB/MSE — heavy-WOB efficiency-focused drilling
    RegimeProfile { mse_mult: 1.4, hydraulic_mult: 0.8, well_control_mult: 0.9, formation_mult: 1.1, label: "high-wob" },
    // 3: Unstable/kick — erratic motor outputs, potential well-control event
    RegimeProfile { mse_mult: 0.7, hydraulic_mult: 1.0, well_control_mult: 1.5, formation_mult: 0.8, label: "unstable" },
];

/// Apply regime-specific weight multipliers and re-normalise to sum to 1.0.
///
/// Returns the heuristic label for the active regime.
fn apply_regime_weights(votes: &mut [SpecialistVote], regime_id: u8) -> &'static str {
    let profile = &REGIME_PROFILES[(regime_id as usize).min(REGIME_PROFILES.len() - 1)];

    for vote in votes.iter_mut() {
        let mult = match vote.specialist.as_str() {
            "MSE"         => profile.mse_mult,
            "Hydraulic"   => profile.hydraulic_mult,
            "WellControl" => profile.well_control_mult,
            "Formation"   => profile.formation_mult,
            _             => 1.0,
        };
        vote.weight *= mult;
    }

    // Re-normalise so weights still sum to 1.0
    let total: f64 = votes.iter().map(|v| v.weight).sum();
    if total > 1e-10 {
        for vote in votes.iter_mut() {
            vote.weight /= total;
        }
    }

    profile.label
}

// ============================================================================
// Orchestrator
// ============================================================================

/// Orchestrator for Phase 8 ensemble voting
pub struct Orchestrator {
    /// Trait-based specialist voters
    specialists: Vec<Box<dyn Specialist>>,
    /// Total reports generated
    reports_generated: u64,
}

impl Orchestrator {
    /// Create a new orchestrator with the default 4 drilling specialists
    pub fn new() -> Self {
        Self {
            specialists: specialists::default_specialists(),
            reports_generated: 0,
        }
    }

    /// Phase 8: Collect specialist votes and produce a weighted consensus.
    ///
    /// `regime_id` (0–3) from the CfC k-means clusterer selects a weight profile
    /// that amplifies the specialist most relevant to the current drilling regime.
    /// Weights are re-normalised after scaling so existing severity thresholds hold.
    ///
    /// Returns a `VotingResult` containing all votes, final severity, risk level,
    /// and efficiency score. The caller (typically `AdvisoryComposer`) uses this
    /// to compose the final `StrategicAdvisory`.
    pub fn vote(
        &mut self,
        ticket: &AdvisoryTicket,
        physics: &DrillingPhysicsReport,
        regime_id: u8,
    ) -> VotingResult {
        self.reports_generated += 1;

        // Collect votes from all specialists via the trait
        let mut votes: Vec<SpecialistVote> = self
            .specialists
            .iter()
            .map(|s| s.evaluate(ticket, physics))
            .collect();

        // Apply regime-aware weight scaling (Phase 6)
        let regime_label = apply_regime_weights(&mut votes, regime_id);

        // Check for WellControl CRITICAL override (safety critical)
        let well_control_critical = votes.iter().any(|v| {
            v.specialist == "WellControl" && v.vote == TicketSeverity::Critical
        });

        // Check for any CRITICAL
        let any_critical = votes.iter().any(|v| v.vote == TicketSeverity::Critical);

        // Calculate weighted average
        let weighted_sum: f64 = votes
            .iter()
            .map(|v| (v.vote as u8) as f64 * v.weight)
            .sum();

        // Final severity: WellControl CRITICAL is highest priority override
        let final_severity = if well_control_critical || any_critical {
            FinalSeverity::Critical
        } else {
            FinalSeverity::from(weighted_sum)
        };

        // Calculate efficiency score and risk level
        let efficiency_score =
            advisory::calculate_efficiency_score(&ticket.current_metrics, physics);
        let risk_level =
            advisory::calculate_risk_level(&votes, &ticket.current_metrics);

        // Build voting reasoning string
        let voting_reasoning =
            build_voting_reasoning(&votes, final_severity, regime_id, regime_label);

        info!(
            severity = %final_severity,
            efficiency = efficiency_score,
            risk = %risk_level,
            votes = votes.len(),
            regime_id = regime_id,
            regime = regime_label,
            "Orchestrator voting complete"
        );

        VotingResult {
            votes,
            final_severity,
            risk_level,
            efficiency_score,
            voting_reasoning,
        }
    }

    /// Get orchestrator statistics
    pub fn stats(&self) -> u64 {
        self.reports_generated
    }
}

impl Default for Orchestrator {
    fn default() -> Self {
        Self::new()
    }
}

/// Build reasoning string from all votes, including the active regime.
fn build_voting_reasoning(
    votes: &[SpecialistVote],
    final_severity: FinalSeverity,
    regime_id: u8,
    regime_label: &str,
) -> String {
    let vote_summaries: Vec<String> = votes
        .iter()
        .map(|v| format!("{}={} ({:.0}%)", v.specialist, v.vote, v.weight * 100.0))
        .collect();

    format!(
        "Final severity {} from ensemble [regime {}:{}]: {}",
        final_severity,
        regime_id,
        regime_label,
        vote_summaries.join(", ")
    )
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AnomalyCategory, DrillingMetrics, DrillingPhysicsReport, RigState, RiskLevel, TicketType, TicketSeverity};

    fn ensure_config() {
        if !crate::config::is_initialized() {
            crate::config::init(crate::config::WellConfig::default(), crate::config::ConfigProvenance::default());
        }
    }

    fn create_test_ticket() -> AdvisoryTicket {
        AdvisoryTicket {
            timestamp: 1000,
            ticket_type: TicketType::RiskWarning,
            category: AnomalyCategory::DrillingEfficiency,
            severity: TicketSeverity::Medium,
            current_metrics: DrillingMetrics {
                state: RigState::Drilling,
                operation: crate::types::Operation::ProductionDrilling,
                mse: 30000.0,
                mse_efficiency: 65.0,
                d_exponent: 1.5,
                dxc: 1.4,
                mse_delta_percent: 0.1,
                flow_balance: 2.0,
                pit_rate: 1.0,
                ecd_margin: 0.5,
                torque_delta_percent: 0.05,
                spp_delta: 30.0,
                flow_data_available: true,
                is_anomaly: true,
                anomaly_category: AnomalyCategory::DrillingEfficiency,
                anomaly_description: Some("MSE efficiency below optimal".to_string()),
                current_formation: None,
                formation_depth_in_ft: None,
            },
            trigger_parameter: "mse_efficiency".to_string(),
            trigger_value: 65.0,
            threshold_value: 70.0,
            description: "MSE efficiency below optimal".to_string(),
            context: None,
            depth: 10000.0,
            trace_log: Vec::new(),
            cfc_anomaly_score: None,
            cfc_feature_surprises: Vec::new(),
            causal_leads: Vec::new(),
        }
    }

    fn create_test_physics() -> DrillingPhysicsReport {
        DrillingPhysicsReport {
            avg_mse: 30000.0,
            mse_trend: 0.01,
            optimal_mse: 20000.0,
            mse_efficiency: 66.7,
            dxc_trend: 0.02,
            flow_balance_trend: 0.0,
            avg_pit_rate: 0.5,
            formation_hardness: 5.0,
            confidence: 0.9,
            detected_dysfunctions: Vec::new(),
            current_depth: 10000.0,
            current_rop: 50.0,
            current_wob: 25.0,
            current_rpm: 120.0,
            current_torque: 15.0,
            current_spp: 2500.0,
            current_casing_pressure: 0.0,
            current_flow_in: 500.0,
            current_flow_out: 505.0,
            current_mud_weight: 12.0,
            current_ecd: 12.4,
            current_gas: 50.0,
            current_pit_volume: 500.0,
            wob_trend: 0.0,
            rop_trend: 0.0,
            founder_detected: false,
            founder_severity: 0.0,
            optimal_wob_estimate: 0.0,
        }
    }

    #[test]
    fn test_orchestrator_voting() {
        ensure_config();
        let mut orchestrator = Orchestrator::new();
        let ticket = create_test_ticket();
        let physics = create_test_physics();

        let result = orchestrator.vote(&ticket, &physics, 0);

        assert_eq!(result.votes.len(), 4);
        assert!(result.efficiency_score > 0 && result.efficiency_score <= 100);
    }

    #[test]
    fn test_well_control_critical_override() {
        ensure_config();
        let mut orchestrator = Orchestrator::new();
        let mut ticket = create_test_ticket();

        // Set well control critical conditions
        ticket.category = AnomalyCategory::WellControl;
        ticket.current_metrics.flow_balance = 25.0;
        ticket.current_metrics.pit_rate = 20.0;
        ticket.current_metrics.anomaly_category = AnomalyCategory::WellControl;

        let physics = create_test_physics();
        let result = orchestrator.vote(&ticket, &physics, 0);

        // WellControl CRITICAL should override
        assert_eq!(result.final_severity, FinalSeverity::Critical);
        assert_eq!(result.risk_level, RiskLevel::Critical);
    }

    #[test]
    fn test_all_specialists_vote() {
        ensure_config();
        let mut orchestrator = Orchestrator::new();
        let ticket = create_test_ticket();
        let physics = create_test_physics();

        let result = orchestrator.vote(&ticket, &physics, 0);

        // Verify all 4 specialists voted
        let specialists: Vec<&str> = result.votes.iter().map(|v| v.specialist.as_str()).collect();
        assert!(specialists.contains(&"MSE"));
        assert!(specialists.contains(&"Hydraulic"));
        assert!(specialists.contains(&"WellControl"));
        assert!(specialists.contains(&"Formation"));

        // Verify weights sum to 1.0
        let total_weight: f64 = result.votes.iter().map(|v| v.weight).sum();
        assert!(
            (total_weight - 1.0).abs() < 0.001,
            "Weights should sum to 1.0, got {}",
            total_weight
        );
    }

    #[test]
    fn test_efficiency_score() {
        ensure_config();
        let mut orchestrator = Orchestrator::new();

        let mut ticket = create_test_ticket();
        ticket.current_metrics.mse_efficiency = 85.0;
        ticket.current_metrics.is_anomaly = false;

        let mut physics = create_test_physics();
        physics.mse_efficiency = 85.0;

        let result = orchestrator.vote(&ticket, &physics, 0);

        assert!(
            result.efficiency_score >= 80,
            "Good efficiency should result in high score, got {}",
            result.efficiency_score
        );
    }

    // ── Regime-aware weighting tests (Phase 6) ────────────────────────────

    #[test]
    fn test_regime_0_weights_sum_to_one() {
        ensure_config();
        let mut orchestrator = Orchestrator::new();
        let ticket = create_test_ticket();
        let physics = create_test_physics();

        // Regime 0 is the baseline — weights should still sum to 1.0
        let result = orchestrator.vote(&ticket, &physics, 0);
        let total: f64 = result.votes.iter().map(|v| v.weight).sum();
        assert!(
            (total - 1.0).abs() < 1e-9,
            "Regime 0 weights should sum to 1.0, got {total}"
        );
    }

    #[test]
    fn test_all_regimes_weights_sum_to_one() {
        ensure_config();
        let ticket = create_test_ticket();
        let physics = create_test_physics();

        for regime_id in 0u8..4 {
            let mut orchestrator = Orchestrator::new();
            let result = orchestrator.vote(&ticket, &physics, regime_id);
            let total: f64 = result.votes.iter().map(|v| v.weight).sum();
            assert!(
                (total - 1.0).abs() < 1e-9,
                "Regime {regime_id} weights should sum to 1.0, got {total}"
            );
        }
    }

    #[test]
    fn test_regime_3_elevates_well_control_weight() {
        ensure_config();
        let ticket = create_test_ticket();
        let physics = create_test_physics();

        let mut orch0 = Orchestrator::new();
        let mut orch3 = Orchestrator::new();

        let result0 = orch0.vote(&ticket, &physics, 0);
        let result3 = orch3.vote(&ticket, &physics, 3);

        let wc_weight_0 = result0.votes.iter()
            .find(|v| v.specialist == "WellControl")
            .map(|v| v.weight)
            .unwrap_or(0.0);
        let wc_weight_3 = result3.votes.iter()
            .find(|v| v.specialist == "WellControl")
            .map(|v| v.weight)
            .unwrap_or(0.0);

        assert!(
            wc_weight_3 > wc_weight_0,
            "Regime 3 (unstable/kick) should elevate WellControl weight: {wc_weight_3:.3} vs baseline {wc_weight_0:.3}"
        );
    }

    #[test]
    fn test_regime_2_elevates_mse_weight() {
        ensure_config();
        let ticket = create_test_ticket();
        let physics = create_test_physics();

        let mut orch0 = Orchestrator::new();
        let mut orch2 = Orchestrator::new();

        let result0 = orch0.vote(&ticket, &physics, 0);
        let result2 = orch2.vote(&ticket, &physics, 2);

        let mse_weight_0 = result0.votes.iter()
            .find(|v| v.specialist == "MSE")
            .map(|v| v.weight)
            .unwrap_or(0.0);
        let mse_weight_2 = result2.votes.iter()
            .find(|v| v.specialist == "MSE")
            .map(|v| v.weight)
            .unwrap_or(0.0);

        assert!(
            mse_weight_2 > mse_weight_0,
            "Regime 2 (high-WOB) should elevate MSE weight: {mse_weight_2:.3} vs baseline {mse_weight_0:.3}"
        );
    }

    #[test]
    fn test_regime_1_elevates_hydraulic_weight() {
        ensure_config();
        let ticket = create_test_ticket();
        let physics = create_test_physics();

        let mut orch0 = Orchestrator::new();
        let mut orch1 = Orchestrator::new();

        let result0 = orch0.vote(&ticket, &physics, 0);
        let result1 = orch1.vote(&ticket, &physics, 1);

        let hyd_weight_0 = result0.votes.iter()
            .find(|v| v.specialist == "Hydraulic")
            .map(|v| v.weight)
            .unwrap_or(0.0);
        let hyd_weight_1 = result1.votes.iter()
            .find(|v| v.specialist == "Hydraulic")
            .map(|v| v.weight)
            .unwrap_or(0.0);

        assert!(
            hyd_weight_1 > hyd_weight_0,
            "Regime 1 (hydraulic-stress) should elevate Hydraulic weight: {hyd_weight_1:.3} vs baseline {hyd_weight_0:.3}"
        );
    }

    #[test]
    fn test_regime_reasoning_includes_label() {
        ensure_config();
        let mut orchestrator = Orchestrator::new();
        let ticket = create_test_ticket();
        let physics = create_test_physics();

        let result = orchestrator.vote(&ticket, &physics, 3);
        assert!(
            result.voting_reasoning.contains("unstable"),
            "Voting reasoning should include regime label: {}",
            result.voting_reasoning
        );
    }

    /// Item 2.4 — pipeline-level regime weighting test.
    ///
    /// Regime 3 applies a 1.5× multiplier to WellControl before renormalisation.
    /// After renormalisation the ratio is slightly less than 1.5 (divided by the
    /// total scaled weight), but must still be ≥ 1.4× the baseline weight.
    /// Weights must sum to exactly 1.0 in both regimes.
    #[test]
    fn test_regime_3_well_control_pipeline_weighting() {
        ensure_config();
        let ticket = create_test_ticket();
        let physics = create_test_physics();

        let mut orch0 = Orchestrator::new();
        let mut orch3 = Orchestrator::new();

        // Regime 0: baseline — no multiplier applied
        let result0 = orch0.vote(&ticket, &physics, 0);
        // Regime 3: unstable/kick — WellControl × 1.5
        let result3 = orch3.vote(&ticket, &physics, 3);

        let wc_weight_0 = result0.votes.iter()
            .find(|v| v.specialist == "WellControl")
            .map(|v| v.weight)
            .unwrap_or(0.0);
        let wc_weight_3 = result3.votes.iter()
            .find(|v| v.specialist == "WellControl")
            .map(|v| v.weight)
            .unwrap_or(0.0);

        // After renormalisation the effective boost is 1.5 / sum-of-scaled-weights
        // (≈ 1.45 with defaults). Assert at ≥ 1.4× to verify the boost is real.
        assert!(
            wc_weight_3 >= wc_weight_0 * 1.4,
            "Regime 3 WellControl weight ({wc_weight_3:.4}) should be ≥ 1.4× baseline ({wc_weight_0:.4})"
        );

        // Weights must still sum to 1.0 in both regimes
        let total0: f64 = result0.votes.iter().map(|v| v.weight).sum();
        let total3: f64 = result3.votes.iter().map(|v| v.weight).sum();
        assert!(
            (total0 - 1.0).abs() < 1e-9,
            "Regime 0 weights must sum to 1.0, got {total0}"
        );
        assert!(
            (total3 - 1.0).abs() < 1e-9,
            "Regime 3 weights must sum to 1.0, got {total3}"
        );
    }

    #[test]
    fn test_out_of_range_regime_clamps_to_profile_3() {
        ensure_config();
        let mut orchestrator = Orchestrator::new();
        let ticket = create_test_ticket();
        let physics = create_test_physics();

        // regime_id = 99 should clamp to profile 3 (last valid)
        let result = orchestrator.vote(&ticket, &physics, 99);
        let total: f64 = result.votes.iter().map(|v| v.weight).sum();
        assert!(
            (total - 1.0).abs() < 1e-9,
            "Out-of-range regime should still produce valid weights, got {total}"
        );
    }
}
