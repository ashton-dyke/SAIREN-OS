//! Library sync background task — periodically pulls episodes from the hub

#[cfg(feature = "fleet-client")]
use crate::context::RAMRecall;
#[cfg(feature = "fleet-client")]
use crate::fleet::client::{FleetClient, FleetClientError};
#[cfg(feature = "fleet-client")]
use std::sync::Arc;
#[cfg(feature = "fleet-client")]
use std::time::Duration;
#[cfg(feature = "fleet-client")]
use tracing::{debug, info, warn};

/// Run the library sync background task
#[cfg(feature = "fleet-client")]
pub async fn run_library_sync(
    client: FleetClient,
    ram_recall: Arc<RAMRecall>,
    interval_secs: u64,
    jitter_secs: u64,
) {
    let mut last_sync: Option<u64> = None;

    loop {
        // Sleep with jitter to prevent all rigs syncing simultaneously
        let jitter = if jitter_secs > 0 {
            use rand::Rng;
            rand::thread_rng().gen_range(0..jitter_secs)
        } else {
            0
        };
        tokio::time::sleep(Duration::from_secs(interval_secs + jitter)).await;

        match client.sync_library(last_sync).await {
            Ok(library) => {
                info!(
                    new_episodes = library.episodes.len(),
                    version = library.version,
                    total_fleet = library.total_fleet_episodes,
                    "Library sync complete"
                );

                // Add new episodes to RAMRecall
                for episode in &library.episodes {
                    ram_recall.add_episode(episode.clone());
                }

                // Remove pruned episodes
                if !library.pruned_ids.is_empty() {
                    ram_recall.remove_episodes(&library.pruned_ids);
                }

                last_sync = Some(chrono::Utc::now().timestamp() as u64);
            }
            Err(FleetClientError::NotModified) => {
                debug!("Library sync: no changes");
            }
            Err(e) => {
                warn!(error = %e, "Library sync failed, will retry next cycle");
            }
        }
    }
}
