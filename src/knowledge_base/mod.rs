//! Structured Knowledge Base for per-well geological and performance data
//!
//! Replaces the flat `well_prognosis.toml` with a directory-based knowledge base
//! that separates geology from engineering parameters and auto-populates
//! performance data from ML analysis during and after drilling.
//!
//! ## Directory Layout
//!
//! ```text
//! {SAIREN_KB}/
//!   {field}/
//!     geology.toml
//!     wells/
//!       {well}/
//!         pre-spud/prognosis.toml
//!         mid-well/snapshot_{timestamp}.toml[.zst]
//!         post-well/summary.toml, performance_{formation}.toml
//! ```

pub mod assembler;
pub mod compressor;
pub mod fleet_bridge;
pub mod layout;
pub mod mid_well;
pub mod migration;
pub mod post_well;
pub mod watcher;

use crate::types::{
    FormationInterval, FormationPrognosis, KnowledgeBaseConfig, MLInsightsReport,
    PostWellSummary, WitsPacket,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{info, warn};

/// Default watcher poll interval
const WATCHER_POLL_SECS: u64 = 30;

/// Knowledge base entry point
pub struct KnowledgeBase {
    config: KnowledgeBaseConfig,
    prognosis: Arc<RwLock<Option<FormationPrognosis>>>,
}

impl KnowledgeBase {
    /// Initialize from environment variables.
    ///
    /// Reads:
    /// - `SAIREN_KB` — root directory (required, returns None if unset)
    /// - `SAIREN_KB_FIELD` — field name (required, returns None if unset)
    /// - Well name from `WellConfig`
    ///
    /// Runs initial assembly and ensures directory structure.
    pub fn init() -> Option<Self> {
        let root = std::env::var("SAIREN_KB").ok()?;
        let field = match std::env::var("SAIREN_KB_FIELD") {
            Ok(f) => f,
            Err(_) => {
                warn!("SAIREN_KB is set but SAIREN_KB_FIELD is not — cannot initialize knowledge base");
                return None;
            }
        };

        let well = if crate::config::is_initialized() {
            crate::config::get().well.name.clone()
        } else {
            std::env::var("SAIREN_KB_WELL").unwrap_or_else(|_| "unknown".to_string())
        };

        let config = KnowledgeBaseConfig {
            root: std::path::PathBuf::from(root),
            field,
            well,
            max_mid_well_snapshots: std::env::var("SAIREN_KB_MAX_SNAPSHOTS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(168),
            cold_retention_days: std::env::var("SAIREN_KB_RETENTION_DAYS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
        };

        // Ensure directories exist
        if let Err(e) = config.ensure_dirs() {
            warn!(error = %e, "Failed to create knowledge base directories");
            return None;
        }

        // Run initial assembly
        let initial = assembler::assemble_prognosis(&config);
        if let Some(ref prog) = initial {
            info!(
                field = &config.field,
                well = &config.well,
                formations = prog.formations.len(),
                "Knowledge base initialized"
            );
        } else {
            info!(
                field = &config.field,
                well = &config.well,
                "Knowledge base initialized (no geology file yet)"
            );
        }

        Some(Self {
            config,
            prognosis: Arc::new(RwLock::new(initial)),
        })
    }

    /// Get the currently assembled prognosis
    pub fn prognosis(&self) -> Option<FormationPrognosis> {
        // Use try_read to avoid blocking in sync context
        match self.prognosis.try_read() {
            Ok(guard) => guard.clone(),
            Err(_) => None,
        }
    }

    /// Get the async prognosis lock (for use in async contexts)
    pub async fn prognosis_async(&self) -> Option<FormationPrognosis> {
        self.prognosis.read().await.clone()
    }

    /// Get formation at a specific depth (convenience method)
    pub fn formation_at_depth(&self, depth_ft: f64) -> Option<FormationInterval> {
        let guard = self.prognosis.try_read().ok()?;
        guard.as_ref()?.formation_at_depth(depth_ft).cloned()
    }

    /// Write a mid-well snapshot from an ML insights report
    pub fn write_snapshot(&self, report: &MLInsightsReport) -> std::io::Result<()> {
        mid_well::write_snapshot(&self.config, report)?;
        mid_well::enforce_snapshot_cap(&self.config)?;
        Ok(())
    }

    /// Write a mid-well snapshot with sustained-stats computed from raw packets
    pub fn write_snapshot_with_packets(
        &self,
        report: &MLInsightsReport,
        packets: &[WitsPacket],
    ) -> std::io::Result<()> {
        mid_well::write_snapshot_with_packets(&self.config, report, packets)?;
        mid_well::enforce_snapshot_cap(&self.config)?;
        Ok(())
    }

    /// Generate post-well summary (called when well is marked complete)
    pub fn complete_well(&self) -> std::io::Result<PostWellSummary> {
        post_well::generate_post_well(&self.config)
    }

    /// Start the background watcher task
    pub fn start_watcher(&self) -> tokio::task::JoinHandle<()> {
        let config = self.config.clone();
        let prognosis = Arc::clone(&self.prognosis);
        let interval = Duration::from_secs(WATCHER_POLL_SECS);

        tokio::spawn(async move {
            watcher::run_watcher(config, prognosis, interval).await;
        })
    }

    /// Get a reference to the configuration
    pub fn config(&self) -> &KnowledgeBaseConfig {
        &self.config
    }
}
