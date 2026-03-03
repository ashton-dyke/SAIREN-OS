//! Gossip broadcast client.
//!
//! Runs a periodic loop that contacts all peers concurrently, sends recent
//! events, and receives theirs. Reuses the reqwest client pattern from
//! `src/fleet/client.rs`.

use super::protocol::{self, GossipEnvelope, PROTOCOL_VERSION};
use super::state::MeshState;
use super::store::EventStore;
use crate::config::{GossipConfig, PeerInfo};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// Run the gossip broadcast loop.
///
/// Contacts all peers every `config.interval_secs`, sending recent events
/// and receiving theirs. Peers are contacted concurrently.
#[allow(clippy::too_many_lines)]
pub async fn run_gossip_loop(
    node_id: String,
    peers: Vec<PeerInfo>,
    store: Arc<Mutex<EventStore>>,
    mesh_state: Arc<MeshState>,
    config: GossipConfig,
) {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(config.timeout_secs))
        .build()
        .unwrap_or_else(|e| {
            tracing::warn!("HTTP client builder failed: {e} — using default (no timeout)");
            reqwest::Client::new()
        });

    let mut round = 0u64;

    loop {
        round += 1;
        info!(
            round,
            peers = peers.len(),
            "[Gossip] Starting broadcast round"
        );

        let mut handles = Vec::with_capacity(peers.len());

        for peer in &peers {
            let peer_id = peer.id.clone();
            let peer_addr = peer.address.clone();
            let node_id = node_id.clone();
            let http = http.clone();
            let store = Arc::clone(&store);
            let mesh_state = Arc::clone(&mesh_state);
            let max_events = config.max_events_per_exchange;
            let base_interval = config.interval_secs;

            handles.push(tokio::spawn(async move {
                // Check backoff
                let backoff = mesh_state.backoff_secs(&peer_id, base_interval);
                if backoff > base_interval {
                    let failures = mesh_state.failure_count(&peer_id);
                    debug!(
                        peer = %peer_id, failures, backoff_secs = backoff,
                        "[Gossip] Skipping peer (backoff)"
                    );
                    return;
                }

                // Build envelope with events since this peer's last cursor
                let cursor = mesh_state.get_cursor(&peer_id);
                let events = {
                    let s = store.lock().await;
                    match s.events_modified_since(cursor, max_events) {
                        Ok(evts) => evts,
                        Err(e) => {
                            warn!(peer = %peer_id, error = %e, "[Gossip] Failed to query events");
                            return;
                        }
                    }
                };

                let envelope = GossipEnvelope {
                    sender_id: node_id,
                    version: PROTOCOL_VERSION,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                    recent_events: events,
                    known_peers: Vec::new(),
                };

                let json = match serde_json::to_vec(&envelope) {
                    Ok(j) => j,
                    Err(e) => {
                        warn!(peer = %peer_id, error = %e, "[Gossip] Failed to serialize envelope");
                        return;
                    }
                };
                let compressed = match protocol::compress(&json) {
                    Ok(c) => c,
                    Err(e) => {
                        warn!(peer = %peer_id, error = %e, "[Gossip] Failed to compress envelope");
                        return;
                    }
                };

                let url = format!("http://{peer_addr}/api/mesh/gossip");
                let result = http
                    .post(&url)
                    .header("Content-Type", "application/octet-stream")
                    .header("X-Node-ID", &envelope.sender_id)
                    .body(compressed)
                    .send()
                    .await;

                match result {
                    Ok(resp) if resp.status().is_success() => {
                        // Parse response envelope
                        match resp.bytes().await {
                            Ok(body) => {
                                let decompressed = match protocol::decompress(&body) {
                                    Ok(d) => d,
                                    Err(e) => {
                                        warn!(peer = %peer_id, error = %e, "[Gossip] Failed to decompress response");
                                        mesh_state.record_failure(&peer_id);
                                        return;
                                    }
                                };
                                let response: GossipEnvelope = match serde_json::from_slice(&decompressed) {
                                    Ok(e) => e,
                                    Err(e) => {
                                        warn!(peer = %peer_id, error = %e, "[Gossip] Failed to deserialize response");
                                        mesh_state.record_failure(&peer_id);
                                        return;
                                    }
                                };

                                // Upsert received events
                                let received = response.recent_events.len();
                                {
                                    let s = store.lock().await;
                                    for event in &response.recent_events {
                                        if let Err(e) = s.upsert_event(event, None) {
                                            warn!(event_id = %event.id, error = %e, "[Gossip] Failed to upsert event");
                                        }
                                    }
                                }

                                // Update cursor to the max last_modified we sent
                                let new_cursor = {
                                    let s = store.lock().await;
                                    s.max_last_modified().unwrap_or(cursor)
                                };
                                mesh_state.record_success(&peer_id, new_cursor);
                                debug!(
                                    peer = %peer_id, received,
                                    "[Gossip] Exchange successful"
                                );
                            }
                            Err(e) => {
                                warn!(peer = %peer_id, error = %e, "[Gossip] Failed to read response body");
                                mesh_state.record_failure(&peer_id);
                            }
                        }
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        warn!(peer = %peer_id, status = %status, "[Gossip] Peer returned error");
                        mesh_state.record_failure(&peer_id);
                    }
                    Err(e) => {
                        debug!(peer = %peer_id, error = %e, "[Gossip] Peer unreachable");
                        mesh_state.record_failure(&peer_id);
                    }
                }
            }));
        }

        // Wait for all peer exchanges to complete
        for handle in handles {
            let _ = handle.await;
        }

        info!(round, "[Gossip] Broadcast round complete");

        // Periodic pruning (every 100 rounds)
        if round.is_multiple_of(100) {
            let s = store.lock().await;
            if let Err(e) = s.prune() {
                warn!(error = %e, "[Gossip] Pruning failed");
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(config.interval_secs)).await;
    }
}
