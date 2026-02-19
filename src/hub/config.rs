//! Hub configuration — environment variables, CLI args, defaults

use tracing::{error, warn};

/// Fleet Hub configuration
#[derive(Debug, Clone)]
pub struct HubConfig {
    /// PostgreSQL connection URL
    pub database_url: String,
    /// Bind address (e.g., "0.0.0.0:8080")
    pub bind_address: String,
    /// Maximum request payload size in bytes (default: 1 MB)
    pub max_payload_size: usize,
    /// Curation interval in seconds (default: 3600 = hourly)
    pub curation_interval_secs: u64,
    /// Maximum episodes in the library before pruning (default: 50000)
    pub library_max_episodes: i64,
    /// Maximum age for episodes in days before archival (default: 365)
    pub pruning_max_age_days: u64,
    /// Admin API key for rig registration and dashboard
    pub admin_key: String,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            database_url: String::new(),
            bind_address: "0.0.0.0:8080".to_string(),
            max_payload_size: 1_048_576, // 1 MB
            curation_interval_secs: 3600,
            library_max_episodes: 50_000,
            pruning_max_age_days: 365,
            admin_key: String::new(),
        }
    }
}

impl HubConfig {
    /// Load configuration from environment variables with CLI overrides
    pub fn from_env(
        database_url: Option<String>,
        bind_address: Option<String>,
        port: Option<u16>,
    ) -> Self {
        let mut config = Self::default();

        // Database URL: CLI arg > env var
        config.database_url = database_url
            .or_else(|| std::env::var("DATABASE_URL").ok())
            .unwrap_or_default();

        // Bind address: CLI --bind-address or --port
        if let Some(addr) = bind_address {
            config.bind_address = addr;
        } else if let Some(p) = port {
            config.bind_address = format!("0.0.0.0:{}", p);
        }

        // Admin key from env
        config.admin_key = std::env::var("FLEET_ADMIN_KEY")
            .unwrap_or_else(|_| {
                if cfg!(debug_assertions) {
                    warn!("FLEET_ADMIN_KEY not set, using default dev key — do NOT use in production");
                } else {
                    error!("FLEET_ADMIN_KEY not set — falling back to insecure default; set FLEET_ADMIN_KEY env var");
                }
                "admin-dev-key".to_string()
            });

        // Optional overrides from env
        if let Ok(v) = std::env::var("FLEET_MAX_PAYLOAD_SIZE") {
            if let Ok(n) = v.parse() {
                config.max_payload_size = n;
            }
        }
        if let Ok(v) = std::env::var("FLEET_CURATION_INTERVAL") {
            if let Ok(n) = v.parse() {
                config.curation_interval_secs = n;
            }
        }
        if let Ok(v) = std::env::var("FLEET_LIBRARY_MAX_EPISODES") {
            if let Ok(n) = v.parse() {
                config.library_max_episodes = n;
            }
        }

        config
    }
}
