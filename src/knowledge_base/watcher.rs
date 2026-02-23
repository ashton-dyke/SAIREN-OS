//! Polling-based directory watcher that triggers prognosis reassembly on changes

use crate::knowledge_base::assembler;
use crate::types::{FormationPrognosis, KnowledgeBaseConfig};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Run the knowledge base directory watcher.
///
/// Polls at the given interval, detects file changes, and re-assembles
/// the prognosis when changes are detected. Writes the result into the
/// shared `RwLock<Option<FormationPrognosis>>`.
pub async fn run_watcher(
    config: KnowledgeBaseConfig,
    prognosis: Arc<RwLock<Option<FormationPrognosis>>>,
    poll_interval: Duration,
) {
    let mut last_seen: HashMap<PathBuf, SystemTime> = HashMap::new();

    // Initial scan to establish baseline
    scan_directories(&config, &mut last_seen);

    loop {
        tokio::time::sleep(poll_interval).await;

        let mut changed = false;
        let mut old_state = last_seen.clone();

        // Re-scan directories
        let mut current: HashMap<PathBuf, SystemTime> = HashMap::new();
        scan_directories(&config, &mut current);

        // Check for new or modified files
        for (path, mtime) in &current {
            match old_state.remove(path) {
                Some(old_mtime) if *mtime != old_mtime => {
                    info!(path = %path.display(), "Knowledge base file modified");
                    changed = true;
                }
                None => {
                    info!(path = %path.display(), "New knowledge base file detected");
                    changed = true;
                }
                _ => {}
            }
        }

        // Check for deleted files
        for (path, _) in &old_state {
            info!(path = %path.display(), "Knowledge base file removed");
            changed = true;
        }

        last_seen = current;

        if changed {
            debug!("Reassembling prognosis due to knowledge base changes");
            let new_prognosis = assembler::assemble_prognosis(&config);

            if let Some(ref prog) = new_prognosis {
                info!(
                    formations = prog.formations.len(),
                    "Prognosis reassembled from knowledge base"
                );
            }

            let mut guard = prognosis.write().await;
            *guard = new_prognosis;
        }
    }
}

/// Scan all relevant directories and record file modification times
fn scan_directories(config: &KnowledgeBaseConfig, state: &mut HashMap<PathBuf, SystemTime>) {
    // Geology file
    record_file(&config.geology_path(), state);

    // Pre-spud for current well
    scan_dir_files(&config.pre_spud_dir(&config.well), state);

    // Post-well directories for all wells (including siblings)
    let wells_dir = config.field_dir().join("wells");
    if wells_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&wells_dir) {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    if let Some(name) = entry.file_name().to_str() {
                        scan_dir_files(&config.post_well_dir(name), state);
                    }
                }
            }
        }
    }
}

/// Record a single file's modification time
fn record_file(path: &PathBuf, state: &mut HashMap<PathBuf, SystemTime>) {
    if let Ok(meta) = std::fs::metadata(path) {
        if let Ok(mtime) = meta.modified() {
            state.insert(path.clone(), mtime);
        }
    }
}

/// Scan all files in a directory and record modification times
fn scan_dir_files(dir: &PathBuf, state: &mut HashMap<PathBuf, SystemTime>) {
    if !dir.exists() {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Ok(meta) = path.metadata() {
                    if let Ok(mtime) = meta.modified() {
                        state.insert(path, mtime);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge_base::compressor::write_toml;
    use crate::types::{FieldGeology, GeologicalFormation};

    #[test]
    fn test_scan_detects_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = KnowledgeBaseConfig {
            root: tmp.path().to_path_buf(),
            field: "TestField".to_string(),
            well: "Well-A".to_string(),
            ..Default::default()
        };
        config.ensure_dirs().expect("dirs");

        // Write geology file
        let geology = FieldGeology {
            field: "TestField".to_string(),
            formations: vec![GeologicalFormation {
                name: "Test".to_string(),
                depth_top_ft: 0.0,
                depth_base_ft: 1000.0,
                lithology: "clay".to_string(),
                hardness: 2.0,
                drillability: "soft".to_string(),
                pore_pressure_ppg: 8.6,
                fracture_gradient_ppg: 12.5,
                hazards: Vec::new(),
            }],
        };
        write_toml(&config.geology_path(), &geology).expect("write");

        let mut state = HashMap::new();
        scan_directories(&config, &mut state);

        assert!(!state.is_empty(), "should detect geology file");
        assert!(state.contains_key(&config.geology_path()));
    }
}
