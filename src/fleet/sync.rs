//! Library sync and performance sync background tasks


use crate::context::RAMRecall;

use crate::fleet::client::{FleetClient, FleetClientError};

use std::sync::Arc;

use std::time::Duration;

use tracing::{debug, info, warn};

/// Run the library sync background task

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

/// Run the intelligence sync background task.
///
/// Periodically pulls hub intelligence outputs (formation benchmarks, anomaly
/// fingerprints, rig-specific advisories) and caches them to a local JSON
/// file at `cache_path`.  Uses a cursor (`synced_at` from previous response)
/// so each pull fetches only new outputs.
///
/// The cache file is a JSON array of `IntelligenceOutput` structs, capped at
/// `FLEET_INTELLIGENCE_MAX_CACHED` entries (oldest pruned first).

pub async fn run_intelligence_sync(
    client: FleetClient,
    cache_path: std::path::PathBuf,
    interval_secs: u64,
    jitter_secs: u64,
) {
    use crate::fleet::types::IntelligenceOutput;
    use crate::config::defaults::{FLEET_INTELLIGENCE_MAX_CACHED};

    let mut last_sync: Option<u64> = None;

    loop {
        let jitter = if jitter_secs > 0 {
            use rand::Rng;
            rand::thread_rng().gen_range(0..jitter_secs)
        } else {
            0
        };
        tokio::time::sleep(Duration::from_secs(interval_secs + jitter)).await;

        let formation_hint = std::env::var("SAIREN_KB_FIELD").ok();

        match client.sync_intelligence(last_sync, formation_hint.as_deref()).await {
            Ok(response) => {
                if response.outputs.is_empty() {
                    debug!("Intelligence sync: no new outputs");
                    last_sync = Some(response.synced_at);
                    continue;
                }

                let new_count = response.outputs.len();
                last_sync = Some(response.synced_at);

                // Load existing cache
                let mut cached: Vec<IntelligenceOutput> = if cache_path.exists() {
                    std::fs::read_to_string(&cache_path)
                        .ok()
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };

                // Merge: append new, dedup by id (new wins), keep latest N
                let existing_ids: std::collections::HashSet<String> =
                    cached.iter().map(|o| o.id.clone()).collect();
                for output in response.outputs {
                    if !existing_ids.contains(&output.id) {
                        cached.push(output);
                    }
                }

                // Sort newest-first, cap
                cached.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                cached.truncate(FLEET_INTELLIGENCE_MAX_CACHED);

                // Write back to disk
                match serde_json::to_string_pretty(&cached) {
                    Ok(json) => {
                        if let Some(parent) = cache_path.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        if let Err(e) = std::fs::write(&cache_path, json) {
                            warn!(error = %e, path = %cache_path.display(), "Failed to write intelligence cache");
                        } else {
                            info!(
                                new = new_count,
                                cached = cached.len(),
                                "Intelligence sync: {} new output(s) cached",
                                new_count
                            );
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to serialize intelligence cache");
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "Intelligence sync failed, will retry next cycle");
            }
        }
    }
}

/// Run the performance data sync background task.
///
/// Periodically pulls offset well performance data from the fleet hub
/// and writes it to the knowledge base directory structure. The KB watcher
/// detects the new files and triggers prognosis reassembly.

pub async fn run_performance_sync(
    client: FleetClient,
    config: crate::types::KnowledgeBaseConfig,
    interval_secs: u64,
    jitter_secs: u64,
) {
    let mut last_sync: Option<u64> = None;

    loop {
        let jitter = if jitter_secs > 0 {
            use rand::Rng;
            rand::thread_rng().gen_range(0..jitter_secs)
        } else {
            0
        };
        tokio::time::sleep(Duration::from_secs(interval_secs + jitter)).await;

        match crate::knowledge_base::fleet_bridge::sync_performance(&client, &config, last_sync).await {
            Ok(count) => {
                if count > 0 {
                    info!(files = count, "Performance sync: new offset data received");
                } else {
                    debug!("Performance sync: no new data");
                }
                last_sync = Some(chrono::Utc::now().timestamp() as u64);
            }
            Err(e) => {
                warn!(error = %e, "Performance sync failed, will retry next cycle");
            }
        }
    }
}
