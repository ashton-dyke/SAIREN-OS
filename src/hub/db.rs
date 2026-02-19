//! Database connection pool and migration runner

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;
use tracing::info;

/// Create a PostgreSQL connection pool
pub async fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(Duration::from_secs(10))
        .connect(database_url)
        .await?;

    info!("Connected to PostgreSQL");
    Ok(pool)
}

/// Run database migrations from the migrations/ directory
pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::Error> {
    info!("Running database migrations...");
    sqlx::migrate!("./migrations").run(pool).await?;
    info!("Migrations complete");
    Ok(())
}
