//! Application State and System Status
//!
//! Shared state for the drilling intelligence pipeline, accessible from
//! API handlers, packet processors, and ML scheduler tasks.

use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Maximum acknowledgment records kept in memory.
pub const MAX_ACKNOWLEDGMENTS: usize = 1000;

// ============================================================================
// Application State
// ============================================================================

/// Shared application state accessible from API handlers and other components.
///
/// This struct is wrapped in `Arc<RwLock<>>` for thread-safe access across
/// the async runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    /// Current operating RPM
    pub current_rpm: f64,

    /// System uptime (serializes as seconds)
    #[serde(skip, default = "Instant::now")]
    pub uptime: Instant,

    /// Total number of analyses performed
    pub total_analyses: u64,

    /// Samples collected during current session
    pub samples_collected: usize,

    /// Last analysis timestamp
    pub last_analysis_time: Option<chrono::DateTime<chrono::Utc>>,

    /// Analysis interval in seconds
    pub analysis_interval_secs: u64,

    /// Current system status
    pub status: SystemStatus,

    /// Latest verification result from strategic agent
    pub latest_verification: Option<crate::types::VerificationResult>,

    /// Count of verified (confirmed) faults
    pub verified_faults: u64,

    /// Count of rejected fault tickets
    pub rejected_faults: u64,

    /// Latest strategic advisory from drilling analysis
    pub latest_advisory: Option<crate::types::StrategicAdvisory>,

    /// Latest WITS packet for dashboard display
    pub latest_wits_packet: Option<crate::types::WitsPacket>,

    /// Latest drilling metrics for dashboard display
    pub latest_drilling_metrics: Option<crate::types::DrillingMetrics>,

    /// Current campaign type (Production or P&A)
    pub campaign: crate::types::Campaign,

    /// Campaign-specific thresholds (derived from campaign)
    #[serde(skip)]
    pub campaign_thresholds: crate::types::CampaignThresholds,

    // === ML Engine Fields (V2.1) ===
    /// Well identifier for ML storage
    pub well_id: String,

    /// Field/asset name for cross-well queries
    pub field_name: String,

    /// Cumulative bit hours (for ML context)
    pub bit_hours: f64,

    /// Depth drilled on current bit in ft (for ML context)
    pub bit_depth_drilled: f64,

    /// Latest ML insights report
    pub latest_ml_report: Option<crate::types::MLInsightsReport>,

    /// WITS packet history for ML analysis
    #[serde(skip)]
    pub wits_history: std::collections::VecDeque<crate::types::WitsPacket>,


    // === Advisory Acknowledgment & Shift Tracking ===

    /// Acknowledged advisory audit trail (bounded ring buffer).
    #[serde(skip)]
    pub acknowledgments: std::collections::VecDeque<crate::api::handlers::AcknowledgmentRecord>,

    /// Total packets processed (for shift summary)
    pub packets_processed: u64,

    /// Total advisory tickets created (for shift summary)
    pub tickets_created: u64,

    /// Total tickets verified/confirmed (for shift summary)
    pub tickets_verified: u64,

    /// Total tickets rejected as transient (for shift summary)
    pub tickets_rejected: u64,

    /// Peak severity observed during current session
    #[serde(skip)]
    pub peak_severity: crate::types::TicketSeverity,

    /// Running average MSE efficiency (None if no drilling data yet)
    pub avg_mse_efficiency: Option<f64>,

    /// Latest CfC formation transition event
    pub latest_formation_transition: Option<crate::types::FormationTransitionEvent>,

    /// CfC formation transition timestamps (for ML dual-source segmentation)
    #[serde(skip)]
    pub formation_transition_timestamps: Vec<u64>,

    /// Current regime centroids from CfC motor output clustering (k=4, dim=8)
    #[serde(skip)]
    pub regime_centroids: [[f64; 8]; 4],

    /// Latest damping monitor snapshot (updated every packet by coordinator)
    #[serde(skip)]
    pub damping_monitor_snapshot: Option<crate::types::DampingMonitorSnapshot>,
}

impl Default for AppState {
    /// Returns a deterministic zero-value suitable for tests.
    /// For production startup use [`AppState::from_env()`].
    fn default() -> Self {
        Self {
            current_rpm: 0.0,
            uptime: Instant::now(),
            total_analyses: 0,
            samples_collected: 0,
            last_analysis_time: None,
            analysis_interval_secs: 60,
            status: SystemStatus::Initializing,
            latest_verification: None,
            verified_faults: 0,
            rejected_faults: 0,
            latest_advisory: None,
            latest_wits_packet: None,
            latest_drilling_metrics: None,
            campaign: crate::types::Campaign::Production,
            campaign_thresholds: crate::types::CampaignThresholds::production(),
            well_id: "WELL-001".to_string(),
            field_name: "DEFAULT".to_string(),
            bit_hours: 0.0,
            bit_depth_drilled: 0.0,
            latest_ml_report: None,
            wits_history: std::collections::VecDeque::with_capacity(7200),
            acknowledgments: std::collections::VecDeque::with_capacity(MAX_ACKNOWLEDGMENTS),
            packets_processed: 0,
            tickets_created: 0,
            tickets_verified: 0,
            tickets_rejected: 0,
            peak_severity: crate::types::TicketSeverity::Low,
            avg_mse_efficiency: None,
            latest_formation_transition: None,
            formation_transition_timestamps: Vec::new(),
            regime_centroids: [[0.0; 8]; 4],
            damping_monitor_snapshot: None,
        }
    }
}

impl AppState {
    /// Build `AppState` from config and environment for production startup.
    ///
    /// Precedence for each field: env var override > TOML config > default.
    /// Env vars (`CAMPAIGN`, `WELL_ID`, `FIELD_NAME`) are supported for
    /// backward compatibility but operators should prefer TOML fields.
    pub fn from_env() -> Self {
        let cfg = crate::config::get();

        // Campaign: CAMPAIGN env > well.campaign TOML > "production"
        let campaign = match std::env::var("CAMPAIGN").as_deref() {
            Ok("pa") | Ok("PA") | Ok("p&a") | Ok("P&A") | Ok("plug_abandonment") => {
                crate::types::Campaign::PlugAbandonment
            }
            Ok(_) => crate::types::Campaign::Production,
            Err(_) => {
                // Fall back to TOML config
                match cfg.well.campaign.as_str() {
                    "plug_abandonment" | "pa" | "P&A" => crate::types::Campaign::PlugAbandonment,
                    _ => crate::types::Campaign::Production,
                }
            }
        };

        // WELL_ID env > well.name TOML
        let well_id = std::env::var("WELL_ID")
            .unwrap_or_else(|_| cfg.well.name.clone());

        // FIELD_NAME env > well.field TOML
        let field_name = std::env::var("FIELD_NAME")
            .unwrap_or_else(|_| {
                if cfg.well.field.is_empty() {
                    "DEFAULT".to_string()
                } else {
                    cfg.well.field.clone()
                }
            });

        Self {
            campaign,
            campaign_thresholds: crate::types::CampaignThresholds::for_campaign(campaign),
            well_id,
            field_name,
            ..Self::default()
        }
    }

    /// Set the campaign type and update thresholds accordingly
    pub fn set_campaign(&mut self, campaign: crate::types::Campaign) {
        self.campaign = campaign;
        self.campaign_thresholds = crate::types::CampaignThresholds::for_campaign(campaign);
        tracing::info!(
            campaign = %campaign.display_name(),
            "Campaign switched - thresholds updated"
        );
    }

    /// Get uptime in seconds
    pub fn uptime_secs(&self) -> u64 {
        self.uptime.elapsed().as_secs()
    }
}

/// System operational status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SystemStatus {
    /// System is starting up
    Initializing,
    /// Learning baseline drilling patterns
    Learning,
    /// Normal operation, monitoring active
    Monitoring,
    /// Analysis detected issues requiring attention
    Alert,
    /// System error or degraded operation
    Error,
}

impl std::fmt::Display for SystemStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SystemStatus::Initializing => write!(f, "Initializing"),
            SystemStatus::Learning => write!(f, "Learning"),
            SystemStatus::Monitoring => write!(f, "Monitoring"),
            SystemStatus::Alert => write!(f, "Alert"),
            SystemStatus::Error => write!(f, "Error"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_state_default() {
        let state = AppState::default();

        assert_eq!(state.current_rpm, 0.0);
        assert_eq!(state.total_analyses, 0);
        assert_eq!(state.status, SystemStatus::Initializing);
    }

    #[test]
    fn test_system_status_display() {
        assert_eq!(format!("{}", SystemStatus::Initializing), "Initializing");
        assert_eq!(format!("{}", SystemStatus::Learning), "Learning");
        assert_eq!(format!("{}", SystemStatus::Monitoring), "Monitoring");
        assert_eq!(format!("{}", SystemStatus::Alert), "Alert");
        assert_eq!(format!("{}", SystemStatus::Error), "Error");
    }
}
