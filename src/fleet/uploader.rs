//! Upload background task — drains the queue and sends events to the hub

#[cfg(feature = "fleet-client")]
use crate::fleet::client::FleetClient;
#[cfg(feature = "fleet-client")]
use crate::fleet::queue::UploadQueue;
#[cfg(feature = "fleet-client")]
use std::sync::Arc;
#[cfg(feature = "fleet-client")]
use std::time::Duration;
#[cfg(feature = "fleet-client")]
use tracing::{info, warn};

/// Run the upload background task
#[cfg(feature = "fleet-client")]
pub async fn run_uploader(queue: Arc<UploadQueue>, client: FleetClient, interval_secs: u64) {
    let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));

    loop {
        interval.tick().await;

        let events = match queue.drain() {
            Ok(events) if events.is_empty() => continue,
            Ok(events) => events,
            Err(e) => {
                warn!(error = %e, "Failed to drain upload queue");
                continue;
            }
        };

        for event in &events {
            match client.upload_event(event).await {
                Ok(true) => {
                    let _ = queue.mark_uploaded(&event.id);
                    info!(event_id = %event.id, "Uploaded fleet event");
                }
                Ok(false) => {
                    // Duplicate — safe to remove from queue
                    let _ = queue.mark_uploaded(&event.id);
                    info!(event_id = %event.id, "Event already on hub (duplicate)");
                }
                Err(e) => {
                    warn!(event_id = %event.id, error = %e, "Upload failed, will retry next cycle");
                    break; // Stop on first failure
                }
            }
        }
    }
}
