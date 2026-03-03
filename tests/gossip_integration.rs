//! Gossip integration tests.
//!
//! Spins up two Axum servers on localhost, exchanges gossip between them,
//! and verifies both sides have each other's events.

use sairen_os::config;
use sairen_os::fleet::types::{EventOutcome, FleetEvent};
use sairen_os::gossip::protocol::{self, GossipEnvelope, PROTOCOL_VERSION};
use sairen_os::gossip::server::{FleetStatus, MeshHandlerState, NodeStatus};
use sairen_os::gossip::state::MeshState;
use sairen_os::gossip::store::EventStore;
use sairen_os::types::{
    AnomalyCategory, Campaign, DrillingPhysicsReport, FinalSeverity, RiskLevel, StrategicAdvisory,
};
use std::sync::Arc;
use tokio::sync::Mutex;

fn make_test_event(id: &str, rig_id: &str, depth: f64, timestamp: u64) -> FleetEvent {
    FleetEvent {
        id: id.to_string(),
        rig_id: rig_id.to_string(),
        well_id: "well-alpha".to_string(),
        field: "test-field".to_string(),
        campaign: Campaign::Production,
        advisory: StrategicAdvisory {
            timestamp,
            efficiency_score: 70,
            risk_level: RiskLevel::Elevated,
            severity: FinalSeverity::Medium,
            recommendation: "test".to_string(),
            expected_benefit: "test".to_string(),
            reasoning: "test".to_string(),
            votes: Vec::new(),
            physics_report: DrillingPhysicsReport::default(),
            context_used: Vec::new(),
            trace_log: Vec::new(),
            category: AnomalyCategory::Mechanical,
            trigger_parameter: "torque_cv".to_string(),
            trigger_value: 0.25,
            threshold_value: 0.15,
        },
        history_window: Vec::new(),
        outcome: EventOutcome::Pending,
        notes: None,
        depth,
        timestamp,
    }
}

fn build_mesh_handler(node_id: &str) -> (MeshHandlerState, Arc<Mutex<EventStore>>) {
    let store = EventStore::open_in_memory().expect("open in-memory store");
    let store = Arc::new(Mutex::new(store));
    let mesh_state = Arc::new(MeshState::in_memory());
    let handler = MeshHandlerState {
        node_id: node_id.to_string(),
        store: Arc::clone(&store),
        mesh_state,
    };
    (handler, store)
}

fn init_config_once() {
    if !config::is_initialized() {
        let mut cfg = config::WellConfig::default();
        cfg.mesh.enabled = true;
        cfg.mesh.peers = vec![
            config::PeerInfo {
                id: "node-a".to_string(),
                address: "127.0.0.1:0".to_string(),
            },
            config::PeerInfo {
                id: "node-b".to_string(),
                address: "127.0.0.1:0".to_string(),
            },
        ];
        config::init(cfg, config::ConfigProvenance::default());
    }
}

#[tokio::test]
async fn test_gossip_exchange_roundtrip() {
    init_config_once();

    let (handler_a, store_a) = build_mesh_handler("node-a");
    let (handler_b, store_b) = build_mesh_handler("node-b");

    // Insert an event into node A
    {
        let s = store_a.lock().await;
        let event = make_test_event("evt-from-a", "rig-a", 8000.0, 1_700_000_000);
        s.upsert_event(&event, Some("shale")).expect("insert");
    }

    // Insert a different event into node B
    {
        let s = store_b.lock().await;
        let event = make_test_event("evt-from-b", "rig-b", 9000.0, 1_700_001_000);
        s.upsert_event(&event, Some("limestone")).expect("insert");
    }

    // Start node B server
    let app_b = axum::Router::new()
        .route(
            "/api/mesh/gossip",
            axum::routing::post(sairen_os::gossip::server::handle_gossip),
        )
        .route(
            "/api/mesh/status",
            axum::routing::get(sairen_os::gossip::server::handle_status),
        )
        .route(
            "/api/mesh/fleet",
            axum::routing::get(sairen_os::gossip::server::handle_fleet),
        )
        .with_state(handler_b.clone());

    let listener_b = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr_b = listener_b.local_addr().expect("local_addr");
    tokio::spawn(async move {
        axum::serve(listener_b, app_b).await.expect("serve");
    });

    // Node A sends gossip to node B
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("http client");

    let events_to_send = {
        let s = store_a.lock().await;
        s.events_modified_since(0, 50).expect("query")
    };
    assert_eq!(
        events_to_send.len(),
        1,
        "Node A should have 1 event to send"
    );

    let envelope = GossipEnvelope {
        sender_id: "node-a".to_string(),
        version: PROTOCOL_VERSION,
        timestamp: 1_700_002_000,
        recent_events: events_to_send,
        known_peers: Vec::new(),
    };
    let json = serde_json::to_vec(&envelope).expect("serialize");
    let compressed = protocol::compress(&json).expect("compress");

    let resp = http
        .post(format!("http://{}/api/mesh/gossip", addr_b))
        .header("Content-Type", "application/octet-stream")
        .header("X-Node-ID", "node-a")
        .body(compressed)
        .send()
        .await
        .expect("send gossip");

    assert!(resp.status().is_success(), "Gossip exchange should succeed");

    // Parse response — should contain node B's events
    let body = resp.bytes().await.expect("response body");
    let decompressed = protocol::decompress(&body).expect("decompress");
    let response_envelope: GossipEnvelope =
        serde_json::from_slice(&decompressed).expect("deserialize response");

    assert_eq!(response_envelope.sender_id, "node-b");
    assert!(
        !response_envelope.recent_events.is_empty(),
        "Node B should send its events in response"
    );

    // Node B should now have node A's event
    {
        let s = store_b.lock().await;
        assert_eq!(
            s.count().expect("count"),
            2,
            "Node B should have 2 events (its own + node A's)"
        );
    }

    // Upsert node B's response events into node A's store
    {
        let s = store_a.lock().await;
        for event in &response_envelope.recent_events {
            s.upsert_event(event, None).expect("upsert");
        }
        assert_eq!(
            s.count().expect("count"),
            2,
            "Node A should have 2 events after exchange"
        );
    }
}

#[tokio::test]
async fn test_fleet_aggregation() {
    init_config_once();

    let (handler_a, _store_a) = build_mesh_handler("node-a");

    // Start a minimal node B that just responds to /api/mesh/status
    let (handler_b, _store_b) = build_mesh_handler("node-b");
    let app_b = axum::Router::new()
        .route(
            "/api/mesh/status",
            axum::routing::get(sairen_os::gossip::server::handle_status),
        )
        .with_state(handler_b);

    let listener_b = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr_b = listener_b.local_addr().expect("local_addr");
    tokio::spawn(async move {
        axum::serve(listener_b, app_b).await.expect("serve");
    });

    // Start node A with fleet endpoint
    let app_a = axum::Router::new()
        .route(
            "/api/mesh/status",
            axum::routing::get(sairen_os::gossip::server::handle_status),
        )
        .route(
            "/api/mesh/fleet",
            axum::routing::get(sairen_os::gossip::server::handle_fleet),
        )
        .with_state(handler_a);

    let listener_a = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr_a = listener_a.local_addr().expect("local_addr");
    tokio::spawn(async move {
        axum::serve(listener_a, app_a).await.expect("serve");
    });

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("http client");

    // Query node A's status
    let status_resp = http
        .get(format!("http://{}/api/mesh/status", addr_a))
        .send()
        .await
        .expect("status request");
    assert!(status_resp.status().is_success());
    let status: NodeStatus = status_resp.json().await.expect("parse status");
    assert_eq!(status.node_id, "node-a");

    // Query fleet from node A — it will try to reach peers from config
    // (which won't match our test server, but node A's own entry will be there)
    let fleet_resp = http
        .get(format!("http://{}/api/mesh/fleet", addr_a))
        .send()
        .await
        .expect("fleet request");
    assert!(fleet_resp.status().is_success());
    let fleet: FleetStatus = fleet_resp.json().await.expect("parse fleet");

    // Should have at least node A (itself) as online
    assert!(
        fleet.fleet_summary.nodes_online >= 1,
        "At least node A should be online"
    );
    assert!(
        fleet
            .nodes
            .iter()
            .any(|n| n.node_id == "node-a" && n.status == "online"),
        "Node A should appear as online"
    );
}
