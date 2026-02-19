//! Fleet Hub — central server for multi-rig fleet learning
//!
//! The hub collects advisory events from all rigs, curates them into
//! a scored episode library, and syncs the library back to each rig.
//!
//! ## Modules
//!
//! - `config` — Hub configuration (env vars, CLI args)
//! - `db` — Database connection pool and migration runner
//! - `api` — HTTP route handlers
//! - `curator` — Background curation pipeline (scoring, dedup, pruning)
//! - `auth` — API key authentication middleware

#[cfg(feature = "fleet-hub")]
pub mod config;
#[cfg(feature = "fleet-hub")]
pub mod db;
#[cfg(feature = "fleet-hub")]
pub mod api;
#[cfg(feature = "fleet-hub")]
pub mod curator;
#[cfg(feature = "fleet-hub")]
pub mod auth;

#[cfg(feature = "fleet-hub")]
use std::sync::atomic::AtomicU64;
#[cfg(feature = "fleet-hub")]
use std::sync::Arc;
#[cfg(feature = "fleet-hub")]
use std::collections::HashMap;
#[cfg(feature = "fleet-hub")]
use std::time::Instant;
#[cfg(feature = "fleet-hub")]
use tokio::sync::RwLock;

/// Shared hub application state
#[cfg(feature = "fleet-hub")]
pub struct HubState {
    /// Database connection pool
    pub db: sqlx::PgPool,
    /// Hub configuration
    pub config: config::HubConfig,
    /// Current library version (from DB sequence)
    pub library_version: AtomicU64,
    /// API key verification cache: key -> (rig_id, expires_at)
    pub api_key_cache: RwLock<HashMap<String, (String, Instant)>>,
}

#[cfg(feature = "fleet-hub")]
impl HubState {
    pub fn new(db: sqlx::PgPool, config: config::HubConfig) -> Arc<Self> {
        Arc::new(Self {
            db,
            config,
            library_version: AtomicU64::new(0),
            api_key_cache: RwLock::new(HashMap::new()),
        })
    }
}
