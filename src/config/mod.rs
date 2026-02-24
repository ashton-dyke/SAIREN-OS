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

mod well_config;
mod formation;
pub mod defaults;
pub mod validation;
pub mod auto_detect;

pub use well_config::*;

use std::sync::OnceLock;

/// Global well configuration, initialized once at startup.
static WELL_CONFIG: OnceLock<WellConfig> = OnceLock::new();

/// Global config provenance, initialized alongside WELL_CONFIG.
static CONFIG_PROVENANCE: OnceLock<ConfigProvenance> = OnceLock::new();

/// Initialize the global well configuration and provenance.
///
/// Must be called exactly once before any calls to `get()`.
/// Warns if called more than once.
pub fn init(config: WellConfig, provenance: ConfigProvenance) {
    if WELL_CONFIG.set(config).is_err() {
        tracing::warn!("config::init() called more than once — ignoring");
    }
    // Provenance is set alongside config; ignore duplicate
    let _ = CONFIG_PROVENANCE.set(provenance);
}

/// Get a reference to the global well configuration.
///
/// Panics if `init()` has not been called. This is by design — a missing
/// config is a fatal startup error, not a recoverable condition.
pub fn get() -> &'static WellConfig {
    WELL_CONFIG
        .get()
        .expect("config::get() called before config::init() — this is a startup bug")
}

/// Get a reference to the global config provenance.
///
/// Returns an empty provenance (no keys user-set) if `init()` has not been called.
pub fn provenance() -> &'static ConfigProvenance {
    static EMPTY: OnceLock<ConfigProvenance> = OnceLock::new();
    CONFIG_PROVENANCE
        .get()
        .unwrap_or_else(|| EMPTY.get_or_init(ConfigProvenance::default))
}

/// Check whether the config has been initialized.
///
/// Useful for tests and optional config paths.
pub fn is_initialized() -> bool {
    WELL_CONFIG.get().is_some()
}
