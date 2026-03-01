//! Polling-based config file watcher.
//!
//! Checks the config file's mtime every 2 seconds. When a change is detected,
//! debounces for 500ms (to handle partial writes from editors), then calls
//! `config::reload()` and sends the result via an mpsc channel.
//!
//! Consistent with the existing KB watcher pattern — no external crate needed.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use tokio::sync::mpsc;

use super::ConfigChange;

/// Events emitted by the config watcher.
#[derive(Debug)]
pub enum ConfigEvent {
    /// Config was successfully reloaded with these changes.
    Reloaded(Vec<ConfigChange>),
    /// Reload was attempted but failed (old config remains active).
    Error(String),
}

/// Interval between mtime checks.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Debounce delay after detecting a change (editors often write in stages).
const DEBOUNCE_DELAY: Duration = Duration::from_millis(500);

/// Run the config file watcher loop.
///
/// Polls `path` for mtime changes and reloads the global config when detected.
/// Sends events on `tx`. Returns when the channel is closed or the task is cancelled.
pub async fn run_config_watcher(
    path: PathBuf,
    tx: mpsc::Sender<ConfigEvent>,
) {
    tracing::info!(path = %path.display(), "Config watcher started");

    let mut last_mtime = get_mtime(&path);

    loop {
        tokio::time::sleep(POLL_INTERVAL).await;

        let current_mtime = get_mtime(&path);

        // If we can't read mtime (file deleted, permissions), warn and keep polling
        let current = match current_mtime {
            Some(t) => t,
            None => {
                // Only warn if we previously had a valid mtime (file was deleted)
                if last_mtime.is_some() {
                    tracing::warn!(
                        path = %path.display(),
                        "Config file not accessible — keeping current config, will retry"
                    );
                    last_mtime = None;
                }
                continue;
            }
        };

        // If file reappeared after being gone, update mtime and reload
        let changed = match last_mtime {
            Some(prev) => current != prev,
            None => true, // File reappeared
        };

        if !changed {
            continue;
        }

        // Debounce: wait, then re-check mtime to ensure write is complete
        tokio::time::sleep(DEBOUNCE_DELAY).await;

        let stable_mtime = get_mtime(&path);
        if stable_mtime != Some(current) {
            // Still changing — wait for next poll cycle
            continue;
        }

        last_mtime = Some(current);

        // Reload
        let event = match super::reload() {
            Ok(changes) => ConfigEvent::Reloaded(changes),
            Err(e) => {
                tracing::error!(error = %e, "Config hot-reload failed — keeping previous config");
                ConfigEvent::Error(e.to_string())
            }
        };

        if tx.send(event).await.is_err() {
            tracing::debug!("Config watcher channel closed, stopping");
            return;
        }
    }
}

/// Read the modification time of a file, returning None on any error.
fn get_mtime(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
}
