//! Spoke-side federation background tasks.
//!
//! Two async tasks bridge the processing loop (owns the CfC networks) with
//! the fleet hub via `tokio::sync::watch` channels:
//!
//! - **Checkpoint upload**: reads snapshots from the processing loop and
//!   persists them to disk + uploads to the hub.
//! - **Federation pull**: periodically fetches the hub's aggregated model
//!   and sends it to the processing loop for restoration.

use std::path::Path;
use tokio::sync::watch;
use tracing::{debug, info, warn};

use crate::cfc::checkpoint::{self, DualCfcCheckpoint};
use crate::config::FederationInitPolicy;
use crate::fleet::client::FleetClient;

/// Background task: periodically upload CfC checkpoints to the hub.
///
/// Reads the latest snapshot from a `watch` channel (published by the
/// processing loop every N packets), persists it to disk, and uploads
/// it to the hub.
pub async fn run_checkpoint_upload(
    client: FleetClient,
    mut rx: watch::Receiver<Option<DualCfcCheckpoint>>,
    checkpoint_path: String,
    interval_secs: u64,
    min_packets: u64,
) {
    // Add jitter to avoid thundering herd across fleet
    let jitter_ms = (rand::random::<u64>() % 5000) + 500;
    tokio::time::sleep(std::time::Duration::from_millis(jitter_ms)).await;

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        interval.tick().await;

        let snapshot = rx.borrow_and_update().clone();
        let Some(cp) = snapshot else {
            debug!("[FederationUpload] No checkpoint available yet");
            continue;
        };

        if cp.metadata.packets_processed < min_packets {
            debug!(
                "[FederationUpload] Skipping — only {} packets (need {})",
                cp.metadata.packets_processed, min_packets,
            );
            continue;
        }

        // Persist to disk first (survives restarts)
        let disk_path = Path::new(&checkpoint_path);
        if let Err(e) = checkpoint::save_to_disk(&cp, disk_path) {
            warn!("[FederationUpload] Failed to save checkpoint to disk: {}", e);
        } else {
            debug!("[FederationUpload] Checkpoint saved to {}", checkpoint_path);
        }

        // Upload to hub
        match client.upload_checkpoint(&cp).await {
            Ok(true) => {
                info!(
                    "[FederationUpload] Checkpoint uploaded ({} packets, loss={:.6})",
                    cp.metadata.packets_processed, cp.metadata.avg_loss,
                );
            }
            Ok(false) => {
                debug!("[FederationUpload] Checkpoint rejected (duplicate)");
            }
            Err(e) => {
                warn!("[FederationUpload] Upload failed: {} — will retry next cycle", e);
            }
        }
    }
}

/// Background task: periodically pull the federated-averaged model from the hub.
///
/// When a new federated model is available and passes the init policy check,
/// it is sent to the processing loop via a `watch` channel for restoration.
pub async fn run_federation_pull(
    client: FleetClient,
    tx: watch::Sender<Option<DualCfcCheckpoint>>,
    policy: FederationInitPolicy,
    interval_secs: u64,
    local_packets_rx: watch::Receiver<u64>,
) {
    if policy == FederationInitPolicy::UploadOnly {
        info!("[FederationPull] Policy is UploadOnly — pull task exiting");
        return;
    }

    // Immediately check on start, then periodic
    let mut last_round: Option<u64> = None;

    // Add jitter
    let jitter_ms = (rand::random::<u64>() % 5000) + 500;
    tokio::time::sleep(std::time::Duration::from_millis(jitter_ms)).await;

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        interval.tick().await;

        let local_packets = *local_packets_rx.borrow();

        match client.pull_federated_model(last_round).await {
            Ok(Some(resp)) => {
                last_round = Some(resp.round);

                // Apply init policy
                let accept = match policy {
                    FederationInitPolicy::FreshOnly => local_packets == 0,
                    FederationInitPolicy::BetterModel => {
                        resp.checkpoint.metadata.packets_processed > local_packets
                    }
                    FederationInitPolicy::UploadOnly => false, // already handled above
                };

                if accept {
                    info!(
                        "[FederationPull] Accepted federated model (round={}, rigs={}, packets={})",
                        resp.round, resp.contributing_rigs, resp.total_packets,
                    );
                    // Send to processing loop for restoration
                    let _ = tx.send(Some(resp.checkpoint));
                } else {
                    debug!(
                        "[FederationPull] Skipped federated model (policy={:?}, local_packets={}, fed_packets={})",
                        policy, local_packets, resp.checkpoint.metadata.packets_processed,
                    );
                }
            }
            Ok(None) => {
                debug!("[FederationPull] No new federated model available");
            }
            Err(e) => {
                warn!("[FederationPull] Pull failed: {} — will retry next cycle", e);
            }
        }
    }
}
