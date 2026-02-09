//! Fleet Hub — central server binary for SAIREN-OS fleet learning
//!
//! Usage:
//!   fleet-hub --database-url postgres://... [--port 8080] [--bind-address 0.0.0.0:8080]

use clap::Parser;
use sairen_os::hub;
use std::sync::atomic::Ordering;
use tracing::info;

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

    // Build config
    let config = hub::config::HubConfig::from_env(
        args.database_url,
        args.bind_address,
        args.port,
    );

    if config.database_url.is_empty() {
        anyhow::bail!("DATABASE_URL must be set via --database-url or DATABASE_URL env var");
    }

    info!(bind = %config.bind_address, "Starting SAIREN Fleet Hub");

    // Connect to PostgreSQL
    let pool = hub::db::create_pool(&config.database_url).await?;

    // Run migrations
    hub::db::run_migrations(&pool).await?;

    // Load current library version from DB
    let version: i64 = sqlx::query_scalar("SELECT last_value FROM library_version_seq")
        .fetch_one(&pool)
        .await
        .unwrap_or(1);

    // Build shared state
    let state = hub::HubState::new(pool.clone(), config.clone());
    state
        .library_version
        .store(version as u64, Ordering::Relaxed);

    // Spawn curator background task
    tokio::spawn(hub::curator::run_curator(pool.clone(), config.clone()));

    // Build router
    let app = hub::api::build_router(state);

    // Bind and serve
    let listener = tokio::net::TcpListener::bind(&config.bind_address).await?;
    info!(address = %config.bind_address, "Fleet Hub listening");

    axum::serve(listener, app)
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
