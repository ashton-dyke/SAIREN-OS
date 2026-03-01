//! Fleet Hub — central server for multi-rig fleet learning and LLM intelligence
//!
//! ## Modules
//!
//! - `config`          — Hub configuration (env vars, CLI args)
//! - `db`              — Database connection pool and migration runner
//! - `api`             — HTTP route handlers
//! - `curator`         — Background curation pipeline (scoring, dedup, pruning)
//! - `auth`            — Passphrase authentication middleware
//! - `knowledge_graph` — PostgreSQL-backed GraphRAG knowledge graph
//! - `intelligence`    — Async LLM analysis workers (requires `llm` feature)

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
pub mod knowledge_graph;
#[cfg(all(feature = "fleet-hub", feature = "llm"))]
pub mod intelligence;
#[cfg(feature = "fleet-hub")]
pub mod federation;

#[cfg(feature = "fleet-hub")]
use std::sync::atomic::AtomicU64;
#[cfg(feature = "fleet-hub")]
use std::sync::Arc;

/// Shared hub application state
#[cfg(feature = "fleet-hub")]
pub struct HubState {
    /// Database connection pool
    pub db: sqlx::PgPool,
    /// Hub configuration
    pub config: config::HubConfig,
    /// Current library version (from DB sequence)
    pub library_version: AtomicU64,
    /// Embedded LLM backend for intelligence workers (None when not configured)
    #[cfg(feature = "llm")]
    pub llm_backend: Option<Arc<crate::llm::MistralRsBackend>>,
    /// In-memory store for pending pairing requests (6-digit code -> request)
    pub pairing_requests: api::pairing::PairingStore,
    /// Per-IP failed pairing lookup tracker (brute-force mitigation)
    pub pairing_attempts: api::pairing::PairingAttemptStore,
    /// Federation state for CfC weight sharing
    pub federation: api::federation::FederationState,
}

#[cfg(feature = "fleet-hub")]
impl HubState {
    /// Create hub state without an LLM backend (curator-only mode).
    pub fn new(db: sqlx::PgPool, config: config::HubConfig) -> Arc<Self> {
        let state = Arc::new(Self {
            db,
            config,
            library_version: AtomicU64::new(0),
            #[cfg(feature = "llm")]
            llm_backend: None,
            pairing_requests: api::pairing::new_pairing_store(),
            pairing_attempts: api::pairing::new_pairing_attempt_store(),
            federation: api::federation::FederationState::new(),
        });
        api::pairing::spawn_pairing_cleanup(
            Arc::clone(&state.pairing_requests),
            Arc::clone(&state.pairing_attempts),
        );
        state
    }

    /// Create hub state with an embedded LLM backend for intelligence workers.
    #[cfg(feature = "llm")]
    pub fn new_with_llm(
        db: sqlx::PgPool,
        config: config::HubConfig,
        backend: Arc<crate::llm::MistralRsBackend>,
    ) -> Arc<Self> {
        let state = Arc::new(Self {
            db,
            config,
            library_version: AtomicU64::new(0),
            llm_backend: Some(backend),
            pairing_requests: api::pairing::new_pairing_store(),
            pairing_attempts: api::pairing::new_pairing_attempt_store(),
            federation: api::federation::FederationState::new(),
        });
        api::pairing::spawn_pairing_cleanup(
            Arc::clone(&state.pairing_requests),
            Arc::clone(&state.pairing_attempts),
        );
        state
    }
}
