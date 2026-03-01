//! Well Configuration Module
//!
//! Provides per-well configuration loaded from TOML files, replacing all
//! hardcoded drilling thresholds with operator-tunable values.
//!
//! ## Loading Order
//!
//! 1. `SAIREN_CONFIG` environment variable (path to TOML file)
//! 2. `well_config.toml` in the current working directory
//! 3. Built-in defaults (matching original hardcoded values)
//!
//! ## Usage
//!
//! Call `config::init()` once at startup, then `config::get()` anywhere:
//!
//! ```ignore
//! // In main():
//! let (cfg, prov) = WellConfig::load_with_provenance();
//! config::init(cfg, prov);
//!
//! // Anywhere in the codebase:
//! let threshold = config::get().thresholds.well_control.flow_imbalance_warning_gpm;
//! let user_set = config::provenance().is_user_set("thresholds.hydraulics.normal_mud_weight_ppg");
//! ```
//!
//! ## Hot Reload
//!
//! Configuration can be reloaded at runtime without restarting:
//!
//! ```ignore
//! // Record the config file path after init:
//! config::set_config_path("/path/to/well_config.toml");
//!
//! // Later, reload from disk:
//! match config::reload() {
//!     Ok(changes) => println!("{} field(s) changed", changes.len()),
//!     Err(e) => eprintln!("Reload failed: {}", e),
//! }
//! ```

mod well_config;
mod formation;
pub mod defaults;
pub mod validation;
pub mod auto_detect;
pub mod watcher;

pub use well_config::*;

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use arc_swap::ArcSwap;
use serde::Serialize;

/// Global well configuration, atomically swappable for hot reload.
static WELL_CONFIG: OnceLock<ArcSwap<WellConfig>> = OnceLock::new();

/// Global config provenance, atomically swappable alongside config.
static CONFIG_PROVENANCE: OnceLock<ArcSwap<ConfigProvenance>> = OnceLock::new();

/// Path to the config file used at startup (for reload/watcher).
static CONFIG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Initialize the global well configuration and provenance.
///
/// Must be called exactly once before any calls to `get()`.
/// Warns if called more than once.
pub fn init(config: WellConfig, provenance: ConfigProvenance) {
    if WELL_CONFIG.set(ArcSwap::from_pointee(config)).is_err() {
        tracing::warn!("config::init() called more than once — ignoring");
    }
    // Provenance is set alongside config; ignore duplicate
    let _ = CONFIG_PROVENANCE.set(ArcSwap::from_pointee(provenance));
}

/// Get a snapshot of the global well configuration.
///
/// Returns a `Guard` that auto-derefs to `&WellConfig`, so all existing
/// call sites like `config::get().thresholds.well_control.xxx` work
/// unchanged via the `Guard → Arc → WellConfig` deref chain.
///
/// Panics if `init()` has not been called. This is by design — a missing
/// config is a fatal startup error, not a recoverable condition.
pub fn get() -> arc_swap::Guard<Arc<WellConfig>> {
    WELL_CONFIG
        .get()
        .expect("config::get() called before config::init() — this is a startup bug")
        .load()
}

/// Get a full `Arc<WellConfig>` for cases that need ownership (e.g. serialization).
///
/// Use this instead of `get()` when you need to clone the entire config
/// or hold it across await points.
pub fn get_arc() -> Arc<WellConfig> {
    WELL_CONFIG
        .get()
        .expect("config::get_arc() called before config::init() — this is a startup bug")
        .load_full()
}

/// Get a snapshot of the global config provenance.
///
/// Returns an empty provenance (no keys user-set) if `init()` has not been called.
pub fn provenance() -> arc_swap::Guard<Arc<ConfigProvenance>> {
    static EMPTY: OnceLock<ArcSwap<ConfigProvenance>> = OnceLock::new();
    match CONFIG_PROVENANCE.get() {
        Some(p) => p.load(),
        None => EMPTY
            .get_or_init(|| ArcSwap::from_pointee(ConfigProvenance::default()))
            .load(),
    }
}

/// Check whether the config has been initialized.
///
/// Useful for tests and optional config paths.
pub fn is_initialized() -> bool {
    WELL_CONFIG.get().is_some()
}

/// Record the config file path used at startup.
///
/// The watcher and `reload()` use this to know which file to re-read.
pub fn set_config_path(path: PathBuf) {
    if CONFIG_PATH.set(path).is_err() {
        tracing::warn!("config::set_config_path() called more than once — ignoring");
    }
}

/// Get the recorded config file path, if any.
pub fn config_path() -> Option<&'static PathBuf> {
    CONFIG_PATH.get()
}

// ============================================================================
// Hot Reload
// ============================================================================

/// A single field that changed between two configs.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigChange {
    /// Dotted key path (e.g. "thresholds.mse.efficiency_warning_percent")
    pub key: String,
    /// Previous value as string
    pub old_value: String,
    /// New value as string
    pub new_value: String,
}

/// Fields that require a restart to take effect.
const NON_RELOADABLE_PREFIXES: &[&str] = &[
    "server.addr",
    "well.name",
    "well.field",
    "well.rig",
];

/// Reload the config from the recorded file path.
///
/// 1. Reads and parses the config file
/// 2. Validates the new config
/// 3. Computes a diff (old vs new)
/// 4. Logs each changed field
/// 5. Warns about non-reloadable fields
/// 6. Atomically swaps the global config
///
/// Returns the list of changes, or an error if parsing/validation fails.
/// On error, the old config remains active.
pub fn reload() -> Result<Vec<ConfigChange>, ConfigError> {
    let path = config_path().ok_or_else(|| {
        ConfigError::Validation(vec![
            "No config file path recorded — cannot reload".to_string(),
        ])
    })?;

    tracing::info!(path = %path.display(), "Reloading config from disk");

    let (new_config, new_provenance) =
        WellConfig::load_from_file_with_provenance(path)?;

    let old_config = get_arc();
    let changes = diff(&old_config, &new_config);

    if changes.is_empty() {
        tracing::info!("Config reloaded — no changes detected");
        return Ok(changes);
    }

    // Log each change
    for change in &changes {
        tracing::info!(
            key = %change.key,
            old = %change.old_value,
            new = %change.new_value,
            "Config field changed"
        );
    }

    // Warn about non-reloadable fields
    let warnings = check_non_reloadable(&changes);
    for warning in &warnings {
        tracing::warn!("{}", warning);
    }

    // Atomic swap
    WELL_CONFIG
        .get()
        .expect("reload called before init")
        .store(Arc::new(new_config));

    CONFIG_PROVENANCE
        .get()
        .expect("reload called before init")
        .store(Arc::new(new_provenance));

    tracing::info!(
        count = changes.len(),
        "Config hot-reloaded successfully"
    );

    Ok(changes)
}

/// Compare two `WellConfig` instances by serializing to `toml::Value`
/// and recursively walking both trees to find leaf differences.
pub fn diff(old: &WellConfig, new: &WellConfig) -> Vec<ConfigChange> {
    let old_value = match toml::Value::try_from(old) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let new_value = match toml::Value::try_from(new) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let mut changes = Vec::new();
    diff_values("", &old_value, &new_value, &mut changes);
    changes
}

/// Recursively walk two TOML value trees and collect leaf differences.
fn diff_values(
    prefix: &str,
    old: &toml::Value,
    new: &toml::Value,
    changes: &mut Vec<ConfigChange>,
) {
    match (old, new) {
        (toml::Value::Table(old_map), toml::Value::Table(new_map)) => {
            // Check all keys in old
            for (key, old_val) in old_map {
                let full_key = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", prefix, key)
                };
                match new_map.get(key) {
                    Some(new_val) => diff_values(&full_key, old_val, new_val, changes),
                    None => {
                        changes.push(ConfigChange {
                            key: full_key,
                            old_value: format_toml_value(old_val),
                            new_value: "<removed>".to_string(),
                        });
                    }
                }
            }
            // Check for keys added in new
            for (key, new_val) in new_map {
                if !old_map.contains_key(key) {
                    let full_key = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{}.{}", prefix, key)
                    };
                    changes.push(ConfigChange {
                        key: full_key,
                        old_value: "<absent>".to_string(),
                        new_value: format_toml_value(new_val),
                    });
                }
            }
        }
        _ => {
            let old_str = format_toml_value(old);
            let new_str = format_toml_value(new);
            if old_str != new_str {
                changes.push(ConfigChange {
                    key: prefix.to_string(),
                    old_value: old_str,
                    new_value: new_str,
                });
            }
        }
    }
}

/// Format a TOML value as a human-readable string for diff output.
fn format_toml_value(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        other => other.to_string(),
    }
}

/// Check which changes affect non-reloadable fields that need a restart.
pub fn check_non_reloadable(changes: &[ConfigChange]) -> Vec<String> {
    let mut warnings = Vec::new();
    for change in changes {
        for prefix in NON_RELOADABLE_PREFIXES {
            if change.key == *prefix || change.key.starts_with(&format!("{}.", prefix)) {
                warnings.push(format!(
                    "Field '{}' changed ({} → {}) but requires a restart to take effect",
                    change.key, change.old_value, change.new_value
                ));
            }
        }
    }
    warnings
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_identical_configs() {
        let a = WellConfig::default();
        let b = WellConfig::default();
        let changes = diff(&a, &b);
        assert!(changes.is_empty(), "Identical configs should produce no diff");
    }

    #[test]
    fn test_diff_changed_threshold() {
        let a = WellConfig::default();
        let mut b = WellConfig::default();
        b.thresholds.mse.efficiency_warning_percent = 65.0;
        let changes = diff(&a, &b);
        assert!(!changes.is_empty(), "Changed threshold should show in diff");
        let mse_change = changes
            .iter()
            .find(|c| c.key.contains("efficiency_warning_percent"));
        assert!(mse_change.is_some(), "Should find MSE warning change");
        let c = mse_change.unwrap();
        // TOML may serialize 70.0 as "70" (integer form) — check the parsed value
        assert!(
            c.old_value == "70" || c.old_value == "70.0",
            "Expected old_value 70 or 70.0, got {}",
            c.old_value
        );
        assert!(
            c.new_value == "65" || c.new_value == "65.0",
            "Expected new_value 65 or 65.0, got {}",
            c.new_value
        );
    }

    #[test]
    fn test_diff_multiple_changes() {
        let a = WellConfig::default();
        let mut b = WellConfig::default();
        b.thresholds.mse.efficiency_warning_percent = 65.0;
        b.advisory.default_cooldown_seconds = 120;
        let changes = diff(&a, &b);
        assert!(changes.len() >= 2, "Should detect at least 2 changes");
    }

    #[test]
    fn test_check_non_reloadable_server_addr() {
        let changes = vec![ConfigChange {
            key: "server.addr".to_string(),
            old_value: "0.0.0.0:8080".to_string(),
            new_value: "0.0.0.0:9090".to_string(),
        }];
        let warnings = check_non_reloadable(&changes);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("restart"));
    }

    #[test]
    fn test_check_non_reloadable_well_name() {
        let changes = vec![ConfigChange {
            key: "well.name".to_string(),
            old_value: "WELL-001".to_string(),
            new_value: "WELL-002".to_string(),
        }];
        let warnings = check_non_reloadable(&changes);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn test_check_non_reloadable_threshold_ok() {
        let changes = vec![ConfigChange {
            key: "thresholds.mse.efficiency_warning_percent".to_string(),
            old_value: "70.0".to_string(),
            new_value: "65.0".to_string(),
        }];
        let warnings = check_non_reloadable(&changes);
        assert!(warnings.is_empty(), "Threshold changes should be reloadable");
    }
}
