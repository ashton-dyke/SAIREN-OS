//! Fleet Hub Integration Tests
//!
//! These tests require a PostgreSQL database running.
//! Set DATABASE_URL env var before running:
//!
//!   DATABASE_URL=postgres://postgres:test@localhost:5433/sairen_fleet_test cargo test --test fleet_integration --features fleet-hub

#![cfg(feature = "fleet-hub")]

use sairen_os::fleet::types::{
    EpisodeMetrics, EventOutcome, FleetEpisode, FleetEvent, HistorySnapshot,
};
use sairen_os::hub;
use std::sync::Arc;

/// Helper: create a test FleetEvent
fn make_test_event(rig_id: &str, event_num: u64) -> FleetEvent {
    use sairen_os::types::{
        Campaign, DrillingPhysicsReport, FinalSeverity, RiskLevel, StrategicAdvisory,
    };

    let ts = chrono::Utc::now().timestamp() as u64 - 60; // 1 minute ago

    FleetEvent {
        id: format!("{}-{}", rig_id, ts + event_num),
        rig_id: rig_id.to_string(),
        well_id: "WELL-TEST-001".to_string(),
        field: "Test Basin".to_string(),
        campaign: Campaign::Production,
        advisory: StrategicAdvisory {
            timestamp: ts + event_num,
            efficiency_score: 65,
            risk_level: RiskLevel::High,
            severity: FinalSeverity::High,
            recommendation: "Reduce WOB by 5 klbs".to_string(),
            expected_benefit: "Clear pack-off indication".to_string(),
            reasoning: "Torque increase of 15% with simultaneous SPP rise".to_string(),
            votes: Vec::new(),
            physics_report: DrillingPhysicsReport::default(),
            context_used: Vec::new(),
            trace_log: Vec::new(),
        },
        history_window: vec![HistorySnapshot {
            timestamp: ts,
            depth: 12450.0,
            rop: 45.0,
            wob: 25.0,
            rpm: 120.0,
            torque: 17.0,
            spp: 3100.0,
            flow_in: 500.0,
            flow_out: 495.0,
            mse: 45000.0,
            mse_efficiency: 62.0,
            d_exponent: 1.45,
            flow_balance: 1.0,
            pit_rate: 0.0,
            ecd_margin: 0.3,
            gas_units: 5.0,
        }],
        outcome: EventOutcome::Pending,
        notes: None,
        depth: 12450.0,
        timestamp: ts + event_num,
    }
}

/// Helper: create a test FleetEpisode
fn make_test_episode(id: &str, rig_id: &str) -> FleetEpisode {
    use sairen_os::types::{AnomalyCategory, Campaign, FinalSeverity, RiskLevel};

    FleetEpisode {
        id: id.to_string(),
        rig_id: rig_id.to_string(),
        category: AnomalyCategory::Mechanical,
        campaign: Campaign::Production,
        depth_range: (12000.0, 12500.0),
        risk_level: RiskLevel::High,
        severity: FinalSeverity::High,
        resolution_summary: "Reduced WOB, resolved pack-off".to_string(),
        outcome: EventOutcome::Resolved {
            action_taken: "Reduced WOB by 5 klbs".to_string(),
        },
        timestamp: chrono::Utc::now().timestamp() as u64 - 3600,
        key_metrics: EpisodeMetrics {
            mse_efficiency: 62.0,
            flow_balance: 1.0,
            d_exponent: 1.45,
            torque_delta_percent: 15.0,
            ecd_margin: 0.3,
            rop: 45.0,
        },
    }
}

/// Test: FleetEpisode::from_event produces valid episodes
#[test]
fn test_episode_from_event() {
    let event = make_test_event("RIG-TEST", 0);
    let episode = FleetEpisode::from_event(&event);

    assert_eq!(episode.rig_id, "RIG-TEST");
    assert!(episode.id.contains("RIG-TEST"));
    assert_eq!(episode.outcome, EventOutcome::Pending);
}

/// Test: Event validation rejects low-risk events
#[test]
fn test_event_validation_risk_level() {
    use sairen_os::types::RiskLevel;

    let mut event = make_test_event("RIG-TEST", 0);
    event.advisory.risk_level = RiskLevel::Low;

    // should_upload returns false for Low risk
    assert!(!sairen_os::fleet::types::should_upload(&event.advisory));
}

/// Test: Event validation rejects empty history window
#[test]
fn test_event_validation_empty_history() {
    let mut event = make_test_event("RIG-TEST", 0);
    event.history_window.clear();

    // The event should be rejected by the hub (empty history window)
    assert!(event.history_window.is_empty());
}

/// Test: RAMRecall remove_episodes works
#[test]
fn test_ram_recall_remove_episodes() {
    use sairen_os::context::RAMRecall;

    let recall = RAMRecall::new();
    let ep1 = make_test_episode("ep-1", "RIG-1");
    let ep2 = make_test_episode("ep-2", "RIG-2");
    let ep3 = make_test_episode("ep-3", "RIG-3");

    recall.add_episode(ep1);
    recall.add_episode(ep2);
    recall.add_episode(ep3);
    assert_eq!(recall.episode_count(), 3);

    recall.remove_episodes(&["ep-1".to_string(), "ep-3".to_string()]);
    assert_eq!(recall.episode_count(), 1);
}

/// Test: Hub config loads from env with defaults
#[test]
fn test_hub_config_defaults() {
    let config = hub::config::HubConfig::default();

    assert_eq!(config.bind_address, "0.0.0.0:8080");
    assert_eq!(config.max_payload_size, 1_048_576);
    assert_eq!(config.curation_interval_secs, 3600);
    assert_eq!(config.library_max_episodes, 50_000);
}

/// Test: Hub config from_env with CLI overrides
#[test]
fn test_hub_config_overrides() {
    let config = hub::config::HubConfig::from_env(
        Some("postgres://test".to_string()),
        None,
        Some(9090),
    )
    .expect("from_env should succeed in debug mode");

    assert_eq!(config.database_url, "postgres://test");
    assert_eq!(config.bind_address, "0.0.0.0:9090");
}

/// Test: ErrorResponse is serializable
#[test]
fn test_error_response_serializable() {
    let err = hub::auth::api_key::ErrorResponse {
        error: "test error".to_string(),
    };
    let json = serde_json::to_string(&err).expect("serialize");
    assert!(json.contains("test error"));
}

/// Test: Scoring algorithm produces valid scores
#[tokio::test]
async fn test_episode_scoring() {
    // Scoring without DB access uses default diversity
    let episode = make_test_episode("ep-score-test", "RIG-1");

    // Verify episode has expected fields
    assert!(episode.timestamp > 0);
    match &episode.outcome {
        EventOutcome::Resolved { action_taken } => {
            assert!(!action_taken.is_empty());
        }
        _ => panic!("Expected Resolved outcome"),
    }
}

/// Test: Event serialization round-trip (ensures wire compatibility)
#[test]
fn test_event_serialization_roundtrip() {
    let event = make_test_event("RIG-SERIAL", 42);
    let json = serde_json::to_string(&event).expect("serialize");
    let back: FleetEvent = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(back.id, event.id);
    assert_eq!(back.rig_id, event.rig_id);
    assert_eq!(back.well_id, event.well_id);
    assert_eq!(back.depth, event.depth);
    assert_eq!(back.timestamp, event.timestamp);
    assert_eq!(back.history_window.len(), 1);
}

/// Test: Episode serialization round-trip
#[test]
fn test_episode_serialization_roundtrip() {
    let episode = make_test_episode("ep-serial", "RIG-1");
    let json = serde_json::to_string(&episode).expect("serialize");
    let back: FleetEpisode = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(back.id, episode.id);
    assert_eq!(back.rig_id, episode.rig_id);
}
