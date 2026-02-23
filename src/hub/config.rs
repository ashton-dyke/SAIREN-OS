//! Hub configuration — environment variables, CLI args, defaults

use tracing::warn;

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
    /// Shared passphrase for all fleet authentication
    pub passphrase: String,

    // ─── Intelligence (LLM) settings ───────────────────────────────────────
    /// Path to the GGUF model file for intelligence workers.
    /// Set via `SAIREN_LLM_MODEL_PATH`. When unset, intelligence workers are
    /// disabled even if the binary was compiled with `--features llm`.
    pub llm_model_path: Option<String>,
    /// How often the intelligence scheduler polls for pending jobs (seconds).
    /// Default: 60. Set via `INTELLIGENCE_INTERVAL_SECS`.
    pub intelligence_interval_secs: u64,
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
            passphrase: String::new(),
            llm_model_path: None,
            intelligence_interval_secs: 60,
        }
    }
}

impl HubConfig {
    /// Load configuration from environment variables with CLI overrides.
    ///
    /// Returns an error in release builds when `FLEET_PASSPHRASE` is not set,
    /// preventing the hub from starting with a publicly known default.
    /// In debug builds a warning is emitted and the dev default is used.
    pub fn from_env(
        database_url: Option<String>,
        bind_address: Option<String>,
        port: Option<u16>,
    ) -> anyhow::Result<Self> {
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

        // Passphrase from env — mandatory in release builds
        config.passphrase = match std::env::var("FLEET_PASSPHRASE") {
            Ok(key) => key,
            Err(_) => {
                if cfg!(debug_assertions) {
                    warn!("FLEET_PASSPHRASE not set, using default dev passphrase — do NOT use in production");
                    "dev-passphrase".to_string()
                } else {
                    anyhow::bail!(
                        "FLEET_PASSPHRASE environment variable is not set. \
                         The hub cannot start in release mode without a passphrase. \
                         Set FLEET_PASSPHRASE to a shared secret."
                    );
                }
            }
        };

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

        // Intelligence / LLM settings
        config.llm_model_path = std::env::var("SAIREN_LLM_MODEL_PATH").ok();
        if let Ok(v) = std::env::var("INTELLIGENCE_INTERVAL_SECS") {
            if let Ok(n) = v.parse() {
                config.intelligence_interval_secs = n;
            }
        }

        Ok(config)
    }
}
