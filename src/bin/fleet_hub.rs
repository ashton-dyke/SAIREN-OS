//! Fleet Hub — central server binary for SAIREN-OS fleet learning
//!
//! ## Build variants
//!
//! ```bash
//! # Curator-only (no LLM — works on any hardware)
//! cargo build --release --features fleet-hub --bin fleet-hub
//!
//! # Full intelligence mode (embedded mistralrs, requires CUDA GPU)
//! cargo build --release --features hub-intelligence,cuda --bin fleet-hub
//! ```
//!
//! ## Environment variables
//!
//! | Variable                   | Required | Description                                |
//! |----------------------------|----------|--------------------------------------------|
//! | `DATABASE_URL`             | Yes      | PostgreSQL connection string               |
//! | `FLEET_PASSPHRASE`         | Yes      | Shared fleet passphrase                    |
//! | `SAIREN_LLM_MODEL_PATH`    | For LLM  | Path to GGUF model file                    |
//! | `INTELLIGENCE_INTERVAL_SECS` | No     | Job poll interval (default: 60)            |

use clap::Parser;
use sairen_os::hub;
use std::net::SocketAddr;
use std::sync::atomic::Ordering;
#[cfg(feature = "llm")]
use std::sync::Arc;
use tracing::info;
#[cfg(feature = "llm")]
use tracing::warn;

#[derive(Parser, Debug)]
#[command(name = "fleet-hub", about = "SAIREN Fleet Hub — fleet-wide learning server")]
struct CliArgs {
    /// PostgreSQL connection URL
    #[arg(long, env = "DATABASE_URL")]
    database_url: Option<String>,

    /// Port to listen on (default: 8080)
    #[arg(long, short)]
    port: Option<u16>,

    /// Bind address (overrides --port)
    #[arg(long)]
    bind_address: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file if present
    let _ = dotenvy::dotenv();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,fleet_hub=debug")),
        )
        .init();

    let args = CliArgs::parse();

    // Build config — fails in release when FLEET_PASSPHRASE is not set
    let config = hub::config::HubConfig::from_env(
        args.database_url,
        args.bind_address,
        args.port,
    )?;

    if config.database_url.is_empty() {
        anyhow::bail!("DATABASE_URL must be set via --database-url or DATABASE_URL env var");
    }

    info!(bind = %config.bind_address, "Starting SAIREN Fleet Hub");

    // ── Database ──────────────────────────────────────────────────────────────
    let pool = hub::db::create_pool(&config.database_url).await?;
    hub::db::run_migrations(&pool).await?;

    let version: i64 = sqlx::query_scalar("SELECT last_value FROM library_version_seq")
        .fetch_one(&pool)
        .await
        .unwrap_or(1);

    // ── LLM Backend (intelligence workers) ────────────────────────────────────
    #[cfg(feature = "llm")]
    let llm_backend: Option<Arc<sairen_os::llm::MistralRsBackend>> = {
        match &config.llm_model_path {
            Some(path) => {
                info!(model_path = %path, "Loading embedded LLM for intelligence workers");
                match sairen_os::llm::MistralRsBackend::load(path).await {
                    Ok(backend) => {
                        info!("LLM backend loaded — intelligence workers active");
                        Some(Arc::new(backend))
                    }
                    Err(e) => {
                        warn!(
                            error = %e,
                            model_path = %path,
                            "Failed to load LLM — intelligence workers disabled"
                        );
                        None
                    }
                }
            }
            None => {
                info!("SAIREN_LLM_MODEL_PATH not set — intelligence workers disabled");
                None
            }
        }
    };

    // ── Hub State ─────────────────────────────────────────────────────────────
    #[cfg(feature = "llm")]
    let state = match &llm_backend {
        Some(backend) => hub::HubState::new_with_llm(
            pool.clone(),
            config.clone(),
            Arc::clone(backend),
        ),
        None => hub::HubState::new(pool.clone(), config.clone()),
    };

    #[cfg(not(feature = "llm"))]
    let state = hub::HubState::new(pool.clone(), config.clone());

    state.library_version.store(version as u64, Ordering::Relaxed);

    // ── Background Tasks ──────────────────────────────────────────────────────

    // Curation pipeline (always on)
    tokio::spawn(hub::curator::run_curator(pool.clone(), config.clone()));
    info!("Curator task started");

    // Intelligence scheduler (only when LLM backend is loaded)
    #[cfg(feature = "llm")]
    if let Some(backend) = llm_backend {
        let interval = config.intelligence_interval_secs;
        tokio::spawn(hub::intelligence::run_intelligence_scheduler(
            pool.clone(),
            backend,
            interval,
        ));
        info!(interval_secs = interval, "Intelligence scheduler started");
    }

    // ── HTTP Server ───────────────────────────────────────────────────────────
    let app = hub::api::build_router(state);
    let listener = tokio::net::TcpListener::bind(&config.bind_address).await?;
    info!(address = %config.bind_address, "Fleet Hub listening");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    info!("Fleet Hub shut down gracefully");
    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install Ctrl+C handler");
    info!("Shutdown signal received");
}
