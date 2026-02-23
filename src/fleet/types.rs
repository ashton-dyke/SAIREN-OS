//! Fleet data types for hub-and-spoke multi-rig learning

use crate::types::{
    AnomalyCategory, Campaign, DrillingMetrics, FinalSeverity, RiskLevel,
    StrategicAdvisory, WitsPacket,
};
use serde::{Deserialize, Serialize};

/// A confirmed advisory event to be uploaded to the fleet hub
///
/// Contains the full advisory, a compressed history window for context,
/// and an outcome field that gets updated when the driller acknowledges.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetEvent {
    /// Unique event ID (derived from advisory timestamp + rig ID)
    pub id: String,
    /// Rig identifier
    pub rig_id: String,
    /// Well identifier
    pub well_id: String,
    /// Field/asset name
    pub field: String,
    /// Campaign at time of event
    pub campaign: Campaign,
    /// The strategic advisory that triggered this event
    pub advisory: StrategicAdvisory,
    /// History window: last N packets + metrics around the event
    pub history_window: Vec<HistorySnapshot>,
    /// Event outcome (updated when driller acknowledges)
    pub outcome: EventOutcome,
    /// Free-text notes from driller (optional)
    pub notes: Option<String>,
    /// Depth at time of event (ft)
    pub depth: f64,
    /// Timestamp (unix seconds)
    pub timestamp: u64,
}

/// A single history snapshot (packet + calculated metrics)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistorySnapshot {
    pub timestamp: u64,
    pub depth: f64,
    pub rop: f64,
    pub wob: f64,
    pub rpm: f64,
    pub torque: f64,
    pub spp: f64,
    pub flow_in: f64,
    pub flow_out: f64,
    pub mse: f64,
    pub mse_efficiency: f64,
    pub d_exponent: f64,
    pub flow_balance: f64,
    pub pit_rate: f64,
    pub ecd_margin: f64,
    pub gas_units: f64,
}

impl HistorySnapshot {
    /// Create a snapshot from a WitsPacket and DrillingMetrics
    pub fn from_packet_and_metrics(packet: &WitsPacket, metrics: &DrillingMetrics) -> Self {
        Self {
            timestamp: packet.timestamp,
            depth: packet.bit_depth,
            rop: packet.rop,
            wob: packet.wob,
            rpm: packet.rpm,
            torque: packet.torque,
            spp: packet.spp,
            flow_in: packet.flow_in,
            flow_out: packet.flow_out,
            mse: metrics.mse,
            mse_efficiency: metrics.mse_efficiency,
            d_exponent: metrics.d_exponent,
            flow_balance: metrics.flow_balance,
            pit_rate: metrics.pit_rate,
            ecd_margin: metrics.ecd_margin,
            gas_units: packet.gas_units,
        }
    }
}

/// Outcome of a fleet event (updated post-event)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EventOutcome {
    /// Event is pending driller acknowledgment
    Pending,
    /// Issue was resolved by driller action
    Resolved {
        action_taken: String,
    },
    /// Event was escalated (e.g., well control event escalated to company man)
    Escalated {
        reason: String,
    },
    /// Event was determined to be a false positive
    FalsePositive,
}

impl std::fmt::Display for EventOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventOutcome::Pending => write!(f, "PENDING"),
            EventOutcome::Resolved { action_taken } => write!(f, "RESOLVED: {}", action_taken),
            EventOutcome::Escalated { reason } => write!(f, "ESCALATED: {}", reason),
            EventOutcome::FalsePositive => write!(f, "FALSE_POSITIVE"),
        }
    }
}

/// A compact precedent extracted from a FleetEvent for the library
///
/// This is what gets stored in RAM Recall and synced across the fleet.
/// Much smaller than a full FleetEvent — only the essential metadata
/// needed for similarity search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetEpisode {
    /// Unique episode ID
    pub id: String,
    /// Source rig ID
    pub rig_id: String,
    /// Anomaly category
    pub category: AnomalyCategory,
    /// Campaign at time of event
    pub campaign: Campaign,
    /// Depth range where the event occurred (min, max in ft)
    pub depth_range: (f64, f64),
    /// Risk level of the original advisory
    pub risk_level: RiskLevel,
    /// Final severity
    pub severity: FinalSeverity,
    /// Human-readable summary of what happened and how it was resolved
    pub resolution_summary: String,
    /// Event outcome
    pub outcome: EventOutcome,
    /// Timestamp (unix seconds)
    pub timestamp: u64,
    /// Key metrics at time of event (for similarity matching)
    pub key_metrics: EpisodeMetrics,
}

/// Key metrics stored with a fleet episode for similarity matching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeMetrics {
    pub mse_efficiency: f64,
    pub flow_balance: f64,
    pub d_exponent: f64,
    pub torque_delta_percent: f64,
    pub ecd_margin: f64,
    pub rop: f64,
}

impl FleetEpisode {
    /// Create an episode from a fleet event
    pub fn from_event(event: &FleetEvent) -> Self {
        let metrics = &event.advisory.physics_report;
        Self {
            id: format!("{}-episode", event.id),
            rig_id: event.rig_id.clone(),
            category: event.advisory.votes.first()
                .map(|v| {
                    // Infer category from highest-weighted critical vote
                    if v.specialist == "WellControl" {
                        AnomalyCategory::WellControl
                    } else if v.specialist == "MSE" {
                        AnomalyCategory::DrillingEfficiency
                    } else if v.specialist == "Hydraulic" {
                        AnomalyCategory::Hydraulics
                    } else if v.specialist == "Formation" {
                        AnomalyCategory::Formation
                    } else {
                        AnomalyCategory::None
                    }
                })
                .unwrap_or(AnomalyCategory::None),
            campaign: event.campaign,
            depth_range: (event.depth, event.depth),
            risk_level: event.advisory.risk_level,
            severity: event.advisory.severity,
            resolution_summary: match &event.outcome {
                EventOutcome::Resolved { action_taken } => action_taken.clone(),
                EventOutcome::Escalated { reason } => format!("Escalated: {}", reason),
                EventOutcome::FalsePositive => "False positive — no action needed".to_string(),
                EventOutcome::Pending => "Pending resolution".to_string(),
            },
            outcome: event.outcome.clone(),
            timestamp: event.timestamp,
            key_metrics: EpisodeMetrics {
                mse_efficiency: metrics.mse_efficiency,
                flow_balance: metrics.flow_balance_trend,
                d_exponent: metrics.dxc_trend,
                torque_delta_percent: 0.0,
                ecd_margin: 0.0,
                rop: metrics.current_rop,
            },
        }
    }
}

// ─── Intelligence distribution types ─────────────────────────────────────────

/// A hub intelligence output received by the rig during an intelligence sync.
///
/// Fleet-wide outputs (`rig_id == None`) are formation benchmarks and anomaly
/// fingerprints relevant to all rigs in the field.  Rig-specific outputs
/// (`rig_id == Some(...)`) are post-well reports or benchmark gap advisories
/// addressed to a particular rig.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntelligenceOutput {
    /// Hub-assigned UUID
    pub id: String,
    /// Worker that produced this output (e.g. `formation_benchmark`)
    pub job_type: String,
    /// Output classification: `benchmark` | `fingerprint` | `report` | `advisory`
    pub output_type: String,
    /// Raw LLM text or formatted summary
    pub content: String,
    /// Formation this output relates to (if applicable)
    pub formation_name: Option<String>,
    /// Rig this output is addressed to (`None` = fleet-wide)
    pub rig_id: Option<String>,
    /// Well this output relates to (if applicable)
    pub well_id: Option<String>,
    /// Confidence 0.0–1.0 (if provided by the worker)
    pub confidence: Option<f64>,
    /// Creation timestamp on the hub (unix seconds)
    pub created_at: u64,
}

/// Response from `GET /api/fleet/intelligence`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntelligenceSyncResponse {
    pub outputs: Vec<IntelligenceOutput>,
    /// Unix timestamp to use as `?since=` on the next pull
    pub synced_at: u64,
    pub total: usize,
}

// ─────────────────────────────────────────────────────────────────────────────

/// Check if an advisory's risk level qualifies for fleet upload
///
/// Only AMBER (Elevated/High) and RED (Critical) events are uploaded.
/// GREEN (Low) events stay local to avoid bandwidth waste.
pub fn should_upload(advisory: &StrategicAdvisory) -> bool {
    matches!(
        advisory.risk_level,
        RiskLevel::Elevated | RiskLevel::High | RiskLevel::Critical
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DrillingPhysicsReport, FinalSeverity};

    fn make_advisory(risk: RiskLevel) -> StrategicAdvisory {
        StrategicAdvisory {
            timestamp: 1000,
            efficiency_score: 70,
            risk_level: risk,
            severity: FinalSeverity::Medium,
            recommendation: "test".to_string(),
            expected_benefit: "test".to_string(),
            reasoning: "test".to_string(),
            votes: Vec::new(),
            physics_report: DrillingPhysicsReport::default(),
            context_used: Vec::new(),
            trace_log: Vec::new(),
        }
    }

    #[test]
    fn test_should_upload() {
        assert!(!should_upload(&make_advisory(RiskLevel::Low)));
        assert!(should_upload(&make_advisory(RiskLevel::Elevated)));
        assert!(should_upload(&make_advisory(RiskLevel::High)));
        assert!(should_upload(&make_advisory(RiskLevel::Critical)));
    }

    #[test]
    fn test_event_outcome_display() {
        assert_eq!(
            format!("{}", EventOutcome::Resolved { action_taken: "increased MW".to_string() }),
            "RESOLVED: increased MW"
        );
        assert_eq!(format!("{}", EventOutcome::Pending), "PENDING");
    }

    #[test]
    fn test_fleet_episode_from_event() {
        let event = FleetEvent {
            id: "RIG1-1000".to_string(),
            rig_id: "RIG1".to_string(),
            well_id: "WELL-001".to_string(),
            field: "TestField".to_string(),
            campaign: Campaign::Production,
            advisory: make_advisory(RiskLevel::High),
            history_window: Vec::new(),
            outcome: EventOutcome::Resolved { action_taken: "reduced WOB".to_string() },
            notes: None,
            depth: 10000.0,
            timestamp: 1000,
        };

        let episode = FleetEpisode::from_event(&event);
        assert_eq!(episode.rig_id, "RIG1");
        assert_eq!(episode.resolution_summary, "reduced WOB");
        assert_eq!(episode.outcome, EventOutcome::Resolved { action_taken: "reduced WOB".to_string() });
    }
}
