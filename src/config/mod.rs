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
//! config::init(WellConfig::load());
//!
//! // Anywhere in the codebase:
//! let threshold = config::get().thresholds.well_control.flow_imbalance_warning_gpm;
//! ```

mod well_config;
mod formation;
pub mod defaults;

pub use well_config::*;

use std::sync::OnceLock;

/// Global well configuration, initialized once at startup.
static WELL_CONFIG: OnceLock<WellConfig> = OnceLock::new();

/// Initialize the global well configuration.
///
/// Must be called exactly once before any calls to `get()`.
/// Panics if called more than once.
pub fn init(config: WellConfig) {
    if WELL_CONFIG.set(config).is_err() {
        tracing::warn!("config::init() called more than once — ignoring");
    }
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

/// Check whether the config has been initialized.
///
/// Useful for tests and optional config paths.
pub fn is_initialized() -> bool {
    WELL_CONFIG.get().is_some()
}
