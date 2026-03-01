//! SAIREN-OS - Strategic AI Rig ENgine
//!
//! Real-time AI-powered drilling operational intelligence system for
//! WITS Level 0 data processing.
//!
//! # Usage
//!
//! ```bash
//! # Run with synthetic test data (LLM compiled by default, CUDA detected at runtime)
//! cargo run --release
//!
//! # Run with simulation input from stdin
//! python wits_simulator.py | ./sairen-os --stdin
//!
//! # Run with GPU acceleration (requires CUDA toolkit)
//! cargo run --release --features cuda
//! ```
//!
//! # Environment Variables
//!
//! - `STRATEGIC_MODEL_PATH`: Path to strategic LLM (default: Qwen 7B GPU / 4B CPU)
//! - `TACTICAL_MODEL_PATH`: Path to tactical LLM (only with `tactical_llm` feature)
//! - `RUST_LOG`: Logging level (default: info)
//! - `RESET_DB`: Set to "true" to wipe all persistent data on startup (for testing)

use anyhow::{Context, Result};
use clap::Parser;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

mod acquisition;
mod api;
mod llm;
mod ml_engine;
mod pipeline;
mod storage;
mod strategic;

// Multi-agent architecture modules
pub mod config;
pub mod types;
pub mod agents;
pub mod physics_engine;
pub mod context;
pub mod sensors;
pub mod baseline;
pub mod aci;
pub mod cfc;
pub mod fleet;
pub mod knowledge_base;
pub mod debrief;
pub mod background;
pub mod optimization;
pub mod causal;
pub mod volve;

use axum::Router;
use api::{create_app, DashboardState};
use pipeline::{AppState, PipelineCoordinator};
use pipeline::source::{PacketSource, CsvSource, StdinSource, TcpSource};
use pipeline::processing_loop::{PostProcessHooks, ProcessingLoop};

// ============================================================================
// CLI Arguments
// ============================================================================

#[derive(Parser, Debug)]
#[command(name = "sairen-os")]
#[command(about = "SAIREN-OS Drilling Operational Intelligence System")]
#[command(version)]
struct CliArgs {
    /// Read WITS data from stdin (JSON format) instead of synthetic data
    /// Use with simulator: python wits_simulator.py | ./sairen-os --stdin
    #[arg(long)]
    stdin: bool,

    /// Connect to WITS Level 0 TCP server (e.g., wits_simulator.py)
    /// Example: ./sairen-os --wits-tcp localhost:5000
    #[arg(long, value_name = "HOST:PORT")]
    wits_tcp: Option<String>,

    /// Override the server address (default: "0.0.0.0:8080")
    #[arg(short, long)]
    addr: Option<String>,

    /// Path to CSV file with WITS data
    #[arg(long)]
    csv: Option<String>,

    /// Speed multiplier for simulation (1 = realtime, 60 = 60x faster, 0 = no delay)
    #[arg(long, default_value = "1")]
    speed: u64,

    /// Reset all persistent data (databases, ML insights, thresholds) on startup.
    /// WARNING: This is destructive and cannot be undone!
    /// Can also be set via RESET_DB=true environment variable.
    #[arg(long)]
    reset_db: bool,

    #[command(subcommand)]
    command: Option<SubCommand>,
}

#[derive(clap::Subcommand, Debug)]
enum SubCommand {
    /// Generate a minimal operator config template to stdout
    GenerateConfig,

    /// Migrate a flat well_prognosis.toml into the structured knowledge base directory
    MigrateKb {
        /// Path to the source well_prognosis.toml file
        #[arg(long = "from")]
        from: String,
        /// Path to the knowledge base root directory
        #[arg(long = "to")]
        to: String,
    },

    /// Launch the setup wizard (web UI on port 8080)
    Setup {
        /// Override scan port ranges (default: 5000-5010,10001-10010)
        #[arg(long)]
        ports: Option<String>,
        /// Override server address (default: 0.0.0.0:8080)
        #[arg(long)]
        addr: Option<String>,
        /// Config directory (default: /etc/sairen-os)
        #[arg(long, default_value = "/etc/sairen-os")]
        config_dir: String,
    },

    /// Pair this rig with a Fleet Hub using a 6-digit code (no passphrase needed)
    Pair {
        /// Fleet Hub URL (e.g. http://hub:8080)
        #[arg(long)]
        hub: String,
        /// Rig identifier
        #[arg(long)]
        rig_id: String,
        /// Well identifier
        #[arg(long)]
        well_id: String,
        /// Field name
        #[arg(long)]
        field: String,
        /// Config directory (default: /etc/sairen-os)
        #[arg(long, default_value = "/etc/sairen-os")]
        config_dir: String,
    },

    /// [Deprecated] Enroll this rig with a Fleet Hub using the shared passphrase.
    /// Use 'sairen-os setup' or 'sairen-os pair' instead.
    #[command(hide = true)]
    Enroll {
        /// Fleet Hub URL (e.g. http://hub:8080)
        #[arg(long)]
        hub: String,
        /// Shared fleet passphrase
        #[arg(long)]
        passphrase: String,
        /// Rig identifier
        #[arg(long)]
        rig_id: String,
        /// Well identifier
        #[arg(long)]
        well_id: String,
        /// Field name
        #[arg(long)]
        field: String,
        /// Config directory (default: /etc/sairen-os)
        #[arg(long, default_value = "/etc/sairen-os")]
        config_dir: String,
    },
}


// ============================================================================
// Database Reset
// ============================================================================

/// Default data directory path
const DATA_DIR: &str = "./data";

/// Check if database reset is requested via CLI flag or environment variable.
fn should_reset_db(cli_flag: bool) -> bool {
    if cli_flag {
        return true;
    }
    if let Ok(val) = std::env::var("RESET_DB") {
        let val_lower = val.to_lowercase();
        return val_lower == "true" || val_lower == "1" || val_lower == "yes";
    }
    false
}

/// Safely remove the data directory and all its contents.
fn reset_data_directory() -> Result<()> {
    use std::fs;
    use std::path::Path;

    let data_path = Path::new(DATA_DIR);

    if !data_path.exists() {
        info!("Data directory does not exist, nothing to reset");
        return Ok(());
    }

    warn!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    warn!("  RESET_DB DETECTED - WIPING ALL PERSISTENT DATA");
    warn!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    warn!("");
    warn!("  Removing: {}", data_path.display());

    if let Ok(entries) = fs::read_dir(data_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            let file_type = if path.is_dir() { "DIR " } else { "FILE" };
            warn!("    {} {}", file_type, path.display());
        }
    }

    fs::remove_dir_all(data_path).context("Failed to remove data directory")?;

    warn!("");
    warn!("  Data directory removed successfully.");
    warn!("  A fresh database will be created on startup.");
    warn!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    warn!("");

    Ok(())
}

// ============================================================================
// Task Names for Supervisor Logging
// ============================================================================

#[derive(Debug, Clone, Copy)]
enum TaskName {
    HttpServer,
    PacketProcessor,
    MLScheduler,
    FleetUploader,
    FleetLibrarySync,
    FleetIntelligenceSync,
    FederationUpload,
    FederationPull,
    ConfigWatcher,
}

impl std::fmt::Display for TaskName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskName::HttpServer => write!(f, "HttpServer"),
            TaskName::PacketProcessor => write!(f, "PacketProcessor"),
            TaskName::MLScheduler => write!(f, "MLScheduler"),
            TaskName::FleetUploader => write!(f, "FleetUploader"),
            TaskName::FleetLibrarySync => write!(f, "FleetLibrarySync"),
            TaskName::FleetIntelligenceSync => write!(f, "FleetIntelligenceSync"),
            TaskName::FederationUpload => write!(f, "FederationUpload"),
            TaskName::FederationPull => write!(f, "FederationPull"),
            TaskName::ConfigWatcher => write!(f, "ConfigWatcher"),
        }
    }
}

// ============================================================================
// Shared Pipeline Initialization
// ============================================================================

/// Common pipeline infrastructure shared between all modes.
#[allow(dead_code)]
struct PipelineCore {
    app_state: Arc<RwLock<AppState>>,
    _process_lock: storage::ProcessLock,
    coordinator: PipelineCoordinator,
    listener: tokio::net::TcpListener,
    app: Router,
    equipment_id: String,
}

/// Initialize the shared pipeline: AppState, storage, thresholds, coordinator,
/// dashboard, and HTTP listener.
async fn init_pipeline(equipment_id: &str, server_addr: &str) -> Result<PipelineCore> {
    use baseline::ThresholdManager;
    use std::path::Path;

    let app_state = Arc::new(RwLock::new(AppState::from_env()));
    info!("✓ Application state initialized");

    info!("🔒 Acquiring process lock...");
    let _process_lock = storage::ProcessLock::acquire("./data")
        .context("Failed to acquire process lock")?;
    info!("✓ Process lock acquired");

    info!("💾 Initializing strategic history storage...");
    if let Err(e) = storage::history::init("./data/strategic_history.db") {
        warn!(
            "Failed to initialize history storage: {}. Reports will not be persisted.",
            e
        );
    } else {
        info!("✓ Strategic history storage initialized");
        match storage::history::prune_old_reports(30) {
            Ok(0) => {}
            Ok(n) => info!("Pruned {} strategic reports older than 30 days", n),
            Err(e) => warn!("Failed to prune old history reports: {}", e),
        }

        // Initialise feedback tree for operator feedback loop.
        match storage::feedback::init() {
            Err(e) => warn!("Failed to init feedback store: {}", e),
            Ok(()) => info!("✓ Feedback storage initialized"),
        }

        // Initialise damping recipes tree for formation-specific recipe persistence.
        match storage::damping_recipes::init() {
            Err(e) => warn!("Failed to init damping recipes store: {}", e),
            Ok(()) => info!("✓ Damping recipes storage initialized"),
        }

        // Initialise acknowledgment tree and restore persisted records.
        match storage::acks::init() {
            Err(e) => warn!("Failed to init acknowledgment store: {}", e),
            Ok(()) => {
                let raw = storage::acks::load_all_raw();
                if !raw.is_empty() {
                    let mut state = app_state.write().await;
                    for bytes in raw {
                        if let Ok(rec) = serde_json::from_slice::<
                            crate::api::handlers::AcknowledgmentRecord,
                        >(&bytes)
                        {
                            state.acknowledgments.push_back(rec);
                        }
                    }
                    info!(
                        count = state.acknowledgments.len(),
                        "Restored acknowledgments from disk"
                    );
                }
            }
        }
    }

    info!(
        "📊 Initializing dynamic threshold system for: {}",
        equipment_id
    );

    let thresholds_path = Path::new(baseline::DEFAULT_STATE_PATH);
    let threshold_manager = Arc::new(std::sync::RwLock::new({
        match ThresholdManager::load_from_file(thresholds_path) {
            Some(mgr) => {
                let locked_count = mgr.locked_count();
                info!(
                    "✓ Loaded {} locked baselines from {:?}",
                    locked_count, thresholds_path
                );
                mgr
            }
            None => {
                info!("📝 No existing thresholds found, starting fresh baseline learning");
                let mut mgr = ThresholdManager::new();
                mgr.start_wits_learning(equipment_id, 0);
                info!("   Started learning for WITS drilling metrics");
                mgr
            }
        }
    }));

    let start_in_learning_mode = {
        let mgr = threshold_manager.read().unwrap_or_else(|e| {
            warn!("RwLock poisoned on ThresholdManager read, recovering");
            e.into_inner()
        });
        if mgr.locked_count() > 0 {
            info!("🎯 Mode: DynamicThresholds (using learned baselines)");
            false
        } else {
            info!("📚 Mode: BaselineLearning (accumulating samples)");
            true
        }
    };

    // LLM inference runs exclusively on the fleet hub (CUDA GPU, embedded mistralrs).
    // The edge pipeline always uses deterministic template advisories.
    let coordinator = PipelineCoordinator::new_with_thresholds(
        threshold_manager.clone(),
        equipment_id.to_string(),
        start_in_learning_mode,
    );

    if let Some(handle) = coordinator.start_kb_watcher() {
        info!("✓ Knowledge base watcher started");
        drop(handle);
    }

    info!("🌐 Starting HTTP server on {}...", server_addr);
    let mut dashboard_state = DashboardState::new_with_storage_and_thresholds(
        Arc::clone(&app_state),
        threshold_manager.clone(),
        equipment_id,
    );

    // Wire strategic and ML storage into dashboard state so v2 endpoints
    // can serve hourly/daily/ML reports (fixes always-None bug).
    match storage::StrategicStorage::open("./data/strategic_reports.db") {
        Ok(s) => {
            info!("✓ Strategic report storage opened for dashboard");
            dashboard_state.strategic_storage = Some(s);
        }
        Err(e) => warn!("Failed to open strategic storage for dashboard: {}", e),
    }
    match ml_engine::MLInsightsStorage::open("./data/ml_insights.db") {
        Ok(s) => {
            info!("✓ ML insights storage opened for dashboard");
            dashboard_state.ml_storage = Some(Arc::new(s));
        }
        Err(e) => warn!("Failed to open ML insights storage for dashboard: {}", e),
    }

    let app = create_app(dashboard_state);

    let listener = tokio::net::TcpListener::bind(server_addr)
        .await
        .with_context(|| format!("Failed to bind to {}", server_addr))?;

    info!("✓ HTTP server listening on {}", server_addr);
    info!("");
    info!("🎯 Dashboard available at: http://{}", server_addr);
    info!("");

    Ok(PipelineCore {
        app_state,
        _process_lock,
        coordinator,
        listener,
        app,
        equipment_id: equipment_id.to_string(),
    })
}

/// Spawn the HTTP server task into the JoinSet.
fn spawn_http_server(
    task_set: &mut JoinSet<Result<TaskName>>,
    listener: tokio::net::TcpListener,
    app: Router,
    cancel_token: CancellationToken,
) {
    task_set.spawn(async move {
        info!("[HttpServer] Task starting");

        let result = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                cancel_token.cancelled().await;
                info!("[HttpServer] Received shutdown signal");
            })
            .await;

        match result {
            Ok(()) => {
                info!("[HttpServer] Graceful shutdown complete");
                Ok(TaskName::HttpServer)
            }
            Err(e) => {
                error!("[HttpServer] Server error: {}", e);
                Err(anyhow::anyhow!("HTTP server error: {}", e))
            }
        }
    });
}

/// Fleet + federation task return type.
struct FleetTasksResult {
    fleet_ctx: pipeline::processing_loop::FleetContext,
    federation_ctx: Option<pipeline::processing_loop::FederationContext>,
}

/// Spawn fleet client background tasks (uploader + library sync + federation).
///
/// Returns a `FleetContext` with the shared queue so the packet processor
/// can enqueue events directly, plus an optional `FederationContext` if
/// federated weight sharing is enabled.
fn spawn_fleet_tasks(
    task_set: &mut JoinSet<Result<TaskName>>,
) -> Option<FleetTasksResult> {
    use fleet::{FleetClient, UploadQueue};
    use fleet::uploader::run_uploader;
    use fleet::sync::run_library_sync;
    use context::RAMRecall;

    let hub_url = match std::env::var("FLEET_HUB_URL") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            info!("Fleet client disabled (FLEET_HUB_URL not set)");
            return None;
        }
    };
    let passphrase = match std::env::var("FLEET_PASSPHRASE") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            warn!("Fleet client disabled (FLEET_PASSPHRASE not set)");
            return None;
        }
    };
    let rig_id = std::env::var("FLEET_RIG_ID").unwrap_or_else(|_| "UNKNOWN".to_string());
    let well_id = std::env::var("WELL_ID")
        .unwrap_or_else(|_| config::get().well.name.clone());

    let client = FleetClient::new(&hub_url, &passphrase, &rig_id);
    info!("Fleet client initialized — hub: {}, rig: {}", hub_url, rig_id);

    let queue = match UploadQueue::open("./data/fleet_queue") {
        Ok(q) => Arc::new(q),
        Err(e) => {
            error!("Failed to open fleet upload queue: {} — fleet disabled", e);
            return None;
        }
    };

    let ram_recall = Arc::new(RAMRecall::new());

    // Task: Fleet Uploader (drain queue -> hub every 60s)
    let uploader_client = client.clone();
    let uploader_queue = Arc::clone(&queue);
    task_set.spawn(async move {
        info!("[FleetUploader] Task starting");
        run_uploader(uploader_queue, uploader_client, config::defaults::FLEET_UPLOADER_INTERVAL_SECS).await;
        Ok(TaskName::FleetUploader)
    });

    // Task: Fleet Library Sync (pull precedents every 6h, jitter +/-30min)
    let library_client = client.clone();
    task_set.spawn(async move {
        info!("[FleetLibrarySync] Task starting");
        run_library_sync(library_client, ram_recall, config::defaults::FLEET_LIBRARY_SYNC_INTERVAL_SECS, config::defaults::FLEET_LIBRARY_SYNC_JITTER_SECS).await;
        Ok(TaskName::FleetLibrarySync)
    });

    // Task: Fleet Intelligence Sync (pull hub LLM outputs every 4h, jitter +/-30min)
    let intel_client = client.clone();
    let intel_cache_path = std::path::PathBuf::from("./data/fleet_intelligence.json");
    task_set.spawn(async move {
        info!("[FleetIntelligenceSync] Task starting");
        fleet::sync::run_intelligence_sync(
            intel_client,
            intel_cache_path,
            config::defaults::FLEET_INTELLIGENCE_SYNC_INTERVAL_SECS,
            config::defaults::FLEET_INTELLIGENCE_SYNC_JITTER_SECS,
        ).await;
        Ok(TaskName::FleetIntelligenceSync)
    });

    // Federation tasks (opt-in via config)
    let fed_config = &config::get().federation;
    let federation_ctx = if fed_config.enable {
        info!(
            "[Federation] Enabled — upload every {}s, pull every {}s, policy: {:?}",
            fed_config.checkpoint_interval_secs,
            fed_config.pull_interval_secs,
            fed_config.init_policy,
        );

        // Watch channels: processing loop <-> federation tasks
        let (checkpoint_tx, checkpoint_rx) =
            tokio::sync::watch::channel::<Option<crate::cfc::checkpoint::DualCfcCheckpoint>>(None);
        let (fed_model_tx, fed_model_rx) =
            tokio::sync::watch::channel::<Option<crate::cfc::checkpoint::DualCfcCheckpoint>>(None);
        let (local_packets_tx, local_packets_rx) =
            tokio::sync::watch::channel::<u64>(0);

        // Task: Federation Upload
        let upload_client = client.clone();
        let checkpoint_path = fed_config.checkpoint_path.clone();
        let upload_interval = fed_config.checkpoint_interval_secs;
        let min_packets = fed_config.min_packets_for_upload;
        task_set.spawn(async move {
            info!("[FederationUpload] Task starting");
            fleet::federation::run_checkpoint_upload(
                upload_client,
                checkpoint_rx,
                checkpoint_path,
                upload_interval,
                min_packets,
            ).await;
            Ok(TaskName::FederationUpload)
        });

        // Task: Federation Pull
        let pull_client = client.clone();
        let pull_interval = fed_config.pull_interval_secs;
        let policy = fed_config.init_policy.clone();
        task_set.spawn(async move {
            info!("[FederationPull] Task starting");
            fleet::federation::run_federation_pull(
                pull_client,
                fed_model_tx,
                policy,
                pull_interval,
                local_packets_rx,
            ).await;
            Ok(TaskName::FederationPull)
        });

        Some(pipeline::processing_loop::FederationContext {
            checkpoint_tx,
            federation_model_rx: fed_model_rx,
            local_packets_tx,
            rig_id: rig_id.clone(),
            well_id: well_id.clone(),
            checkpoint_interval_packets: fed_config.min_packets_for_upload,
        })
    } else {
        info!("[Federation] Disabled (federation.enable = false)");
        None
    };

    Some(FleetTasksResult {
        fleet_ctx: pipeline::processing_loop::FleetContext { queue, rig_id, well_id },
        federation_ctx,
    })
}

/// Spawn the ML Engine Scheduler task.
fn spawn_ml_scheduler(
    task_set: &mut JoinSet<Result<TaskName>>,
    app_state: Arc<RwLock<AppState>>,
    cancel_token: CancellationToken,
) {
    task_set.spawn(async move {
        use ml_engine::{MLScheduler, get_interval};

        let ml_knowledge_base = knowledge_base::KnowledgeBase::init();

        info!("[MLScheduler] Task starting with interval {:?}", get_interval());

        let mut interval = tokio::time::interval(get_interval());
        let mut analyses_run = 0u64;

        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    info!("[MLScheduler] Received shutdown signal after {} analyses", analyses_run);
                    return Ok(TaskName::MLScheduler);
                }
                _ = interval.tick() => {
                    let (packets, campaign, well_id, field_name, bit_hours, bit_depth, cfc_transition_timestamps, regime_centroids) = {
                        let state = app_state.read().await;
                        (
                            state.wits_history.iter().cloned().collect::<Vec<_>>(),
                            state.campaign,
                            state.well_id.clone(),
                            state.field_name.clone(),
                            state.bit_hours,
                            state.bit_depth_drilled,
                            state.formation_transition_timestamps.clone(),
                            state.regime_centroids,
                        )
                    };

                    // Apply ROP lag compensation
                    let rop_lag = config::get().ml.rop_lag_seconds as usize;
                    let mut packets = packets;
                    if rop_lag > 0 && packets.len() > rop_lag {
                        for i in 0..packets.len() - rop_lag {
                            packets[i].rop = packets[i + rop_lag].rop;
                        }
                        packets.truncate(packets.len() - rop_lag);
                    } else if rop_lag > 0 && packets.len() <= rop_lag {
                        info!(
                            "[MLScheduler] Skipping analysis: insufficient data for ROP lag ({} packets, need > {})",
                            packets.len(), rop_lag
                        );
                        continue;
                    }

                    if packets.len() < config::defaults::MIN_PACKETS_FOR_ML_ANALYSIS {
                        info!(
                            "[MLScheduler] Skipping analysis: insufficient data ({} packets, need {}+)",
                            packets.len(), config::defaults::MIN_PACKETS_FOR_ML_ANALYSIS
                        );
                        continue;
                    }

                    analyses_run += 1;
                    info!("[MLScheduler] Running ML analysis #{} with {} packets", analyses_run, packets.len());

                    let snapshot_packets = packets.clone();

                    let metrics: Vec<types::DrillingMetrics> = packets.iter().map(|p| {
                        // Use the physics engine — same formulas as the tactical agent.
                        // The previous hand-rolled construction had three bugs:
                        //   1. mse_efficiency used a hardcoded 50 000 psi reference
                        //      instead of a formation-adaptive optimal MSE.
                        //   2. ecd_margin was hardcoded to 14.0 ppg fracture gradient
                        //      instead of using the per-packet fracture_gradient field.
                        //   3. flow_balance sign was inverted (flow_in - flow_out)
                        //      vs the rest of the codebase (flow_out - flow_in).
                        let mut m = physics_engine::tactical_update(p, None, None);
                        // tactical_update leaves operation as default; set it from the
                        // same campaign-aware classifier the tactical agent uses.
                        m.operation = agents::tactical::detect_operation(p, campaign);
                        m
                    }).collect();

                    let dataset = MLScheduler::build_dataset(
                        packets,
                        metrics,
                        &well_id,
                        &field_name,
                        campaign,
                        bit_hours,
                        bit_depth,
                        &cfc_transition_timestamps,
                        regime_centroids,
                    );

                    let report = MLScheduler::run_analysis(&dataset);

                    {
                        let mut state = app_state.write().await;
                        state.latest_ml_report = Some(report.clone());
                    }

                    if let Some(ref kb) = ml_knowledge_base {
                        if let Err(e) = kb.write_snapshot_with_packets(&report, &snapshot_packets) {
                            warn!("Failed to write KB snapshot: {}", e);
                        }
                    }

                    match &report.result {
                        types::AnalysisResult::Success(insights) => {
                            info!(
                                "🧠 ML Analysis #{}: {} | Score: {:.2} | Correlations: {}",
                                analyses_run,
                                insights.confidence,
                                insights.optimal_params.composite_score,
                                insights.correlations.len()
                            );
                        }
                        types::AnalysisResult::Failure(failure) => {
                            warn!("🧠 ML Analysis #{}: Failed - {}", analyses_run, failure);
                        }
                    }
                }
            }
        }
    });
}

/// Spawn the config file watcher task.
///
/// Only starts if a config file path was recorded at startup. The watcher
/// polls the file's mtime and triggers `config::reload()` on changes.
fn spawn_config_watcher(
    task_set: &mut JoinSet<Result<TaskName>>,
    cancel_token: CancellationToken,
) {
    let Some(path) = config::config_path().cloned() else {
        info!("[ConfigWatcher] No config file path recorded — watcher disabled");
        return;
    };

    let (tx, mut rx) = tokio::sync::mpsc::channel(4);

    // Spawn the polling loop
    task_set.spawn(async move {
        info!("[ConfigWatcher] Task starting — watching {}", path.display());

        tokio::select! {
            _ = cancel_token.cancelled() => {
                info!("[ConfigWatcher] Received shutdown signal");
            }
            _ = config::watcher::run_config_watcher(path, tx) => {
                info!("[ConfigWatcher] Watcher loop ended");
            }
        }

        Ok(TaskName::ConfigWatcher)
    });

    // Spawn a lightweight drain task so events are consumed
    // (the watcher sends events; we log them here)
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                config::watcher::ConfigEvent::Reloaded(changes) => {
                    if !changes.is_empty() {
                        info!(
                            count = changes.len(),
                            "[ConfigWatcher] Config hot-reloaded via file change"
                        );
                    }
                }
                config::watcher::ConfigEvent::Error(e) => {
                    warn!("[ConfigWatcher] Reload failed: {}", e);
                }
            }
        }
    });
}

/// Run the supervisor loop: monitor tasks, cancel on failure.
async fn run_supervisor(
    task_set: &mut JoinSet<Result<TaskName>>,
    cancel_token: CancellationToken,
) -> Result<()> {
    info!("🔒 Supervisor: All tasks spawned, monitoring...");

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                info!("🛑 Supervisor: Shutdown signal received");
                break;
            }
            result = task_set.join_next() => {
                match result {
                    Some(Ok(Ok(task_name))) => {
                        info!("🔒 Supervisor: Task {} completed normally", task_name);
                    }
                    Some(Ok(Err(e))) => {
                        error!("🔒 Supervisor: Task failed with error: {}", e);
                        cancel_token.cancel();
                        return Err(e);
                    }
                    Some(Err(e)) => {
                        error!("🔒 Supervisor: Task panicked: {}", e);
                        cancel_token.cancel();
                        return Err(anyhow::anyhow!("Task panicked: {}", e));
                    }
                    None => {
                        info!("🔒 Supervisor: All tasks completed");
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

// ============================================================================
// Unified Pipeline Runner
// ============================================================================

/// Run the drilling intelligence pipeline with any packet source.
///
/// All input modes (CSV, stdin, TCP) flow through this function.
/// The `hooks` parameter provides mode-specific per-packet processing
/// (e.g. regime stamping for TCP). The `spawn_ml` flag controls
/// whether the ML scheduler task is started.
async fn run_pipeline<S: PacketSource, H: PostProcessHooks>(
    mut source: S,
    hooks: H,
    equipment_id: &str,
    server_addr: String,
    spawn_ml: bool,
    cancel_token: CancellationToken,
) -> Result<()> {
    info!("🚀 Starting SAIREN-OS Drilling Intelligence Pipeline");
    info!("");
    info!("   Phase 1: WITS Ingestion (continuous)");
    info!("   Phase 2: Basic Physics (MSE, d-exponent, etc.)");
    info!("   Phase 3: Tactical Agent -> AdvisoryTicket");
    info!("   Phase 4: History Buffer (60 packets)");
    info!("   Phase 5: Strategic Agent -> verify_ticket()");
    info!("   Phase 6: Context Lookup");
    info!("   Phase 7: LLM Explainer");
    info!("   Phase 8: Orchestrator Voting (4 Specialists)");
    info!("   Phase 9: Storage");
    info!("   Phase 10: Dashboard API");
    info!("");

    let core = init_pipeline(equipment_id, &server_addr).await?;
    let app_state = core.app_state;

    info!("🔒 Supervisor: Initializing task monitoring");
    let mut task_set: JoinSet<Result<TaskName>> = JoinSet::new();

    // Task 1: HTTP Server
    spawn_http_server(&mut task_set, core.listener, core.app, cancel_token.clone());

    // Fleet client background tasks (+ optional federation)
    let fleet_result = spawn_fleet_tasks(&mut task_set);
    let (proc_fleet, proc_federation) = match fleet_result {
        Some(r) => (Some(r.fleet_ctx), r.federation_ctx),
        None => (None, None),
    };

    // Task 2: Packet Processor (unified processing loop)
    let proc_cancel = cancel_token.clone();
    let proc_state = Arc::clone(&app_state);
    task_set.spawn(async move {
        info!("[PacketProcessor] Task starting");

        let processing_loop = ProcessingLoop::new(
            core.coordinator,
            proc_state,
            hooks,
            proc_cancel,
        );

        let processing_loop = if let Some(ctx) = proc_fleet {
            processing_loop.with_fleet(ctx)
        } else {
            processing_loop
        };

        let processing_loop = if let Some(ctx) = proc_federation {
            processing_loop.with_federation(ctx)
        } else {
            processing_loop
        };

        let _stats = processing_loop.run(&mut source).await;
        Ok(TaskName::PacketProcessor)
    });

    // Task 3: ML Engine Scheduler (only for streaming modes with history)
    if spawn_ml {
        spawn_ml_scheduler(&mut task_set, Arc::clone(&app_state), cancel_token.clone());
    }

    // Task 4: Config File Watcher (hot-reload on file changes)
    spawn_config_watcher(&mut task_set, cancel_token.clone());

    run_supervisor(&mut task_set, cancel_token).await
}

// ============================================================================
// Data Loading (CSV / Synthetic)
// ============================================================================

/// Load WITS packets from CSV file or generate synthetic test data.
fn load_packets(csv_path: Option<String>) -> Result<Vec<types::WitsPacket>> {
    if let Some(path) = csv_path {
        info!("📂 Loading WITS data from CSV: {}", path);
        let data = match volve::VolveReplay::load(&path, volve::VolveConfig::default()) {
            Ok(replay) => {
                info!(
                    "   Detected Volve WITSML format: {} ({} packets, {:.0}-{:.0} ft)",
                    replay.info.well_id,
                    replay.info.packet_count,
                    replay.info.depth_range_ft.0,
                    replay.info.depth_range_ft.1,
                );
                replay.into_packets()
            }
            Err(_) => sensors::read_csv_data(&path),
        };
        if data.is_empty() {
            return Err(anyhow::anyhow!("No WITS data loaded from CSV"));
        }
        info!("   Loaded {} packets", data.len());
        Ok(data)
    } else {
        info!("🧪 Using synthetic test data (drilling fault simulation)");
        let data = sensors::generate_fault_test_data();
        info!("   Generated {} packets", data.len());
        Ok(data)
    }
}

// ============================================================================
// Enrollment
// ============================================================================

/// Update or insert an environment variable in a shell env file.
///
/// If the variable exists (commented or not), update its value.
/// Otherwise, append it at the end.
fn update_env_var(contents: &str, key: &str, value: &str) -> String {
    let mut found = false;
    let mut lines: Vec<String> = contents
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            // Match both "KEY=..." and "# KEY=..."
            if trimmed.starts_with(&format!("{}=", key))
                || trimmed.starts_with(&format!("# {}=", key))
            {
                found = true;
                format!("{}={}", key, value)
            } else {
                line.to_string()
            }
        })
        .collect();

    if !found {
        lines.push(format!("{}={}", key, value));
    }

    let mut result = lines.join("\n");
    if !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// Enroll this rig with a Fleet Hub using the shared passphrase.
///
/// 1. POST /api/fleet/enroll with passphrase auth + rig identity
/// 2. Write env vars to config_dir/env
/// 3. Update well_config.toml
/// 4. Verify hub connectivity
/// 5. Verify passphrase authentication
async fn run_enroll(hub_url: &str, passphrase: &str, rig_id: &str, well_id: &str, field: &str, config_dir: &str) -> Result<()> {
    use std::path::Path;

    let hub_url = hub_url.trim_end_matches('/');
    let config_path = Path::new(config_dir);

    println!("Enrolling with Fleet Hub: {}", hub_url);
    println!();

    // Step 1: Enroll rig with passphrase auth
    println!("  [1/5] Enrolling rig {}...", rig_id);
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("Failed to build HTTP client")?;

    let body = serde_json::json!({
        "rig_id": rig_id,
        "well_id": well_id,
        "field": field,
    });
    let resp = http
        .post(format!("{}/api/fleet/enroll", hub_url))
        .header("Authorization", format!("Bearer {}", passphrase))
        .json(&body)
        .send()
        .await
        .context("Failed to connect to Fleet Hub")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!(
            "Enrollment failed (HTTP {}): {}",
            status,
            text
        ));
    }

    #[derive(serde::Deserialize)]
    struct EnrollResponse {
        rig_id: String,
        well_id: String,
        field: String,
    }

    let enrollment: EnrollResponse = resp.json().await.context("Invalid enrollment response")?;
    println!("        Rig:   {}", enrollment.rig_id);
    println!("        Well:  {}", enrollment.well_id);
    println!("        Field: {}", enrollment.field);

    // Step 2: Write env file
    println!("  [2/5] Writing environment to {}/env...", config_dir);
    let env_path = config_path.join("env");
    let existing = std::fs::read_to_string(&env_path).unwrap_or_default();
    let mut updated = existing;
    updated = update_env_var(&updated, "FLEET_HUB_URL", hub_url);
    updated = update_env_var(&updated, "FLEET_PASSPHRASE", passphrase);
    updated = update_env_var(&updated, "FLEET_RIG_ID", &enrollment.rig_id);
    updated = update_env_var(&updated, "WELL_ID", &enrollment.well_id);
    std::fs::write(&env_path, &updated)
        .with_context(|| format!("Failed to write {}", env_path.display()))?;

    // Step 3: Update well_config.toml
    println!("  [3/5] Updating {}/well_config.toml...", config_dir);
    let well_config_path = config_path.join("well_config.toml");
    let mut well_config = if well_config_path.exists() {
        config::WellConfig::load_from_file(&well_config_path)
            .unwrap_or_else(|_| config::WellConfig::default())
    } else {
        config::WellConfig::default()
    };
    well_config.well.name = enrollment.well_id.clone();
    well_config.well.field = enrollment.field.clone();
    well_config.well.rig = enrollment.rig_id.clone();
    well_config
        .save_to_file(&well_config_path)
        .with_context(|| format!("Failed to write {}", well_config_path.display()))?;

    // Step 4: Verify hub reachability
    println!("  [4/5] Verifying hub connectivity...");
    let health_resp = http
        .get(format!("{}/api/fleet/health", hub_url))
        .send()
        .await;
    match health_resp {
        Ok(r) if r.status().is_success() => {
            println!("        Hub is healthy");
        }
        Ok(r) => {
            warn!("Hub health check returned {}", r.status());
            println!("        WARNING: Hub returned {}", r.status());
        }
        Err(e) => {
            warn!("Hub health check failed: {}", e);
            println!("        WARNING: Hub unreachable ({})", e);
        }
    }

    // Step 5: Verify passphrase auth
    println!("  [5/5] Verifying passphrase authentication...");
    let lib_resp = http
        .get(format!("{}/api/fleet/library", hub_url))
        .header("Authorization", format!("Bearer {}", passphrase))
        .header("X-Rig-ID", rig_id)
        .send()
        .await;
    match lib_resp {
        Ok(r) if r.status().is_success() || r.status() == reqwest::StatusCode::NOT_MODIFIED => {
            println!("        Authentication verified");
        }
        Ok(r) => {
            let status = r.status();
            return Err(anyhow::anyhow!(
                "Passphrase verification failed (HTTP {})",
                status
            ));
        }
        Err(e) => {
            return Err(anyhow::anyhow!("Passphrase verification failed: {}", e));
        }
    }

    println!();
    println!("  Enrollment complete!");
    println!();
    println!("  Next step:");
    println!("    sudo systemctl restart sairen-os");
    println!();

    Ok(())
}

// ============================================================================
// Generate Config Template
// ============================================================================

/// Output a minimal operator TOML template to stdout.
///
/// Operator-relevant fields are uncommented; expert fields are commented out
/// with descriptions. This is the starting point for a new deployment.
fn generate_config_template() {
    print!(
        r#"# SAIREN-OS Well Configuration
# Generated by: sairen-os generate-config
#
# Only the fields below need to be set for a standard deployment.
# All other thresholds are auto-detected from WITS data or use safe defaults.

[well]
name = "WELL-001"
field = ""
rig = ""
bit_diameter_inches = 8.5
campaign = "production"       # "production" or "plug_abandonment"

[server]
addr = "0.0.0.0:8080"

[thresholds.hydraulics]
normal_mud_weight_ppg = 8.6      # auto-detected from WITS if not set
fracture_gradient_ppg = 14.0

[thresholds.well_control]
flow_imbalance_warning_gpm = 10.0   # auto-detected from baseline if not set
flow_imbalance_critical_gpm = 20.0

# === Expert Configuration (change only with engineering review) ===

# [thresholds.mse]
# efficiency_warning_percent = 70.0
# efficiency_poor_percent = 50.0

# [thresholds.mechanical]
# torque_increase_warning = 0.15
# torque_increase_critical = 0.25
# stick_slip_cv_warning = 0.15
# stick_slip_cv_critical = 0.25
# packoff_spp_increase_threshold = 0.10
# packoff_rop_decrease_threshold = 0.20
# stick_slip_min_samples = 5

# [thresholds.founder]
# wob_increase_min = 0.02
# rop_response_min = 0.01
# severity_warning = 0.3
# severity_high = 0.7
# min_samples = 5
# quick_wob_delta_percent = 0.05

# [thresholds.formation]
# dexp_decrease_warning = -0.15
# mse_change_significant = 0.20
# dxc_trend_threshold = 0.05
# dxc_pressure_threshold = -0.05
# mse_pressure_tolerance = 0.10

# [thresholds.rig_state]
# idle_rpm_max = 5.0
# circulation_flow_min = 50.0
# drilling_wob_min = 1.0
# reaming_depth_offset = 5.0
# trip_out_hook_load_min = 200.0
# trip_in_hook_load_max = 50.0
# tripping_flow_max = 100.0

# [baseline_learning]
# warning_sigma = 3.0
# critical_sigma = 5.0
# min_samples_for_lock = 100
# min_std_floor = 0.001
# max_outlier_percentage = 0.05
# outlier_sigma_threshold = 3.0

# [ensemble_weights]
# mse = 0.25
# hydraulic = 0.25
# well_control = 0.30
# formation = 0.20

# [advisory]
# default_cooldown_seconds = 60
# critical_bypass_cooldown = true

# [physics]
# formation_hardness_base_psi = 5000.0
# formation_hardness_multiplier = 8000.0
# kick_min_indicators = 2
# loss_min_indicators = 2
# kick_gas_increase_threshold = 50.0
# confidence_full_window = 60
# min_rop_for_mse = 0.1

# [ml]
# rop_lag_seconds = 60
# interval_secs = 3600
"#
    );
}

// ============================================================================
// Setup Wizard
// ============================================================================

/// Run the setup wizard — a standalone HTTP server with the setup UI.
async fn run_setup(ports: Option<String>, addr: &str, config_dir: &str) -> Result<()> {
    let port_ranges = match ports {
        Some(ref s) => acquisition::scanner::parse_port_ranges(s)
            .map_err(|e| anyhow::anyhow!("Invalid port ranges: {}", e))?,
        None => acquisition::scanner::DEFAULT_PORT_RANGES.to_vec(),
    };

    let state = api::setup::SetupState::new(config_dir.to_string(), port_ranges);
    let app = api::setup::setup_router(state);

    info!("Starting SAIREN-OS Setup Wizard on {}", addr);
    info!("");
    info!("  Open http://{} in a browser to configure this rig.", addr);
    info!("");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind to {}", addr))?;

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
            info!("Setup wizard shutting down");
        })
        .await
        .context("Setup wizard server error")?;

    Ok(())
}

// ============================================================================
// CLI Pairing (headless)
// ============================================================================

/// Pair with a Fleet Hub using a 6-digit code (headless CLI flow).
async fn run_pair(hub_url: &str, rig_id: &str, well_id: &str, field: &str, config_dir: &str) -> Result<()> {
    use rand::Rng;
    use std::path::Path;

    let hub_url = hub_url.trim_end_matches('/');
    let code: String = {
        let mut rng = rand::thread_rng();
        format!("{:06}", rng.gen_range(0..1_000_000u32))
    };

    println!("Pairing with Fleet Hub: {}", hub_url);
    println!();

    // Step 1: Send pairing request
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .context("Failed to build HTTP client")?;

    let body = serde_json::json!({
        "rig_id": rig_id,
        "well_id": well_id,
        "field": field,
        "code": code,
    });

    let resp = http
        .post(format!("{}/api/fleet/pair/request", hub_url))
        .json(&body)
        .send()
        .await
        .context("Failed to send pairing request to hub")?;

    if !resp.status().is_success() && resp.status().as_u16() != 202 {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Hub rejected pairing request (HTTP {}): {}", status, text));
    }

    println!("  Pairing code: {}", code);
    println!();
    println!("  Approve this code on the Fleet Hub dashboard.");
    println!("  Waiting for approval...");

    // Step 2: Poll for approval
    let poll_interval = std::time::Duration::from_secs(3);
    let max_wait = std::time::Duration::from_secs(600); // 10 minutes
    let start = std::time::Instant::now();

    let passphrase = loop {
        if start.elapsed() > max_wait {
            return Err(anyhow::anyhow!("Pairing code expired (10 minute timeout)"));
        }

        tokio::time::sleep(poll_interval).await;

        let status_resp = http
            .get(format!("{}/api/fleet/pair/status?code={}", hub_url, code))
            .send()
            .await;

        match status_resp {
            Ok(r) if r.status().is_success() => {
                #[derive(serde::Deserialize)]
                struct PairStatus {
                    status: String,
                    passphrase: Option<String>,
                }

                if let Ok(s) = r.json::<PairStatus>().await {
                    match s.status.as_str() {
                        "approved" => {
                            if let Some(pass) = s.passphrase {
                                break pass;
                            }
                            return Err(anyhow::anyhow!("Approved but no passphrase returned"));
                        }
                        "expired" => {
                            return Err(anyhow::anyhow!("Pairing code expired"));
                        }
                        _ => {
                            // Still pending, keep polling
                            print!(".");
                            use std::io::Write;
                            std::io::stdout().flush().ok();
                        }
                    }
                }
            }
            _ => {
                // Network error, retry
                print!("!");
                use std::io::Write;
                std::io::stdout().flush().ok();
            }
        }
    };

    println!();
    println!();
    println!("  Paired successfully!");

    // Step 3: Write env file
    let config_path = Path::new(config_dir);
    if let Err(e) = std::fs::create_dir_all(config_path) {
        warn!("Failed to create config directory: {}", e);
    }

    let env_path = config_path.join("env");
    let existing = std::fs::read_to_string(&env_path).unwrap_or_default();
    let mut updated = existing;
    updated = update_env_var(&updated, "FLEET_HUB_URL", hub_url);
    updated = update_env_var(&updated, "FLEET_PASSPHRASE", &passphrase);
    updated = update_env_var(&updated, "FLEET_RIG_ID", rig_id);
    updated = update_env_var(&updated, "WELL_ID", well_id);
    std::fs::write(&env_path, &updated)
        .with_context(|| format!("Failed to write {}", env_path.display()))?;

    println!("  Fleet env written to {}", env_path.display());
    println!();
    println!("  Next step:");
    println!("    sudo systemctl restart sairen-os");
    println!();

    Ok(())
}

// ============================================================================
// Main Entry Point
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    // Initialize rayon thread pool for CfC dual-network parallelism.
    // 2 threads: leaves 2 cores for tokio (packet loop, API, fleet sync).
    rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .thread_name(|idx| format!("rayon-cfc-{}", idx))
        .build_global()
        .expect("Failed to initialize rayon thread pool");

    let args = CliArgs::parse();

    // Subcommand dispatch
    if let Some(SubCommand::GenerateConfig) = &args.command {
        generate_config_template();
        return Ok(());
    }

    if let Some(SubCommand::MigrateKb { from, to }) = &args.command {
        let from_path = std::path::Path::new(from);
        let to_path = std::path::Path::new(to);
        info!("Migrating knowledge base: {} -> {}", from, to);
        knowledge_base::migration::migrate_flat_to_kb(from_path, to_path)?;
        info!("Knowledge base migration complete");
        return Ok(());
    }

    if let Some(SubCommand::Setup {
        ports,
        addr,
        config_dir,
    }) = &args.command
    {
        let bind_addr = addr.as_deref().unwrap_or("0.0.0.0:8080");
        return run_setup(ports.clone(), bind_addr, config_dir).await;
    }

    if let Some(SubCommand::Pair {
        hub,
        rig_id,
        well_id,
        field,
        config_dir,
    }) = &args.command
    {
        return run_pair(hub, rig_id, well_id, field, config_dir).await;
    }

    if let Some(SubCommand::Enroll {
        hub,
        passphrase,
        rig_id,
        well_id,
        field,
        config_dir,
    }) = &args.command
    {
        warn!("'sairen-os enroll' is deprecated — use 'sairen-os setup' or 'sairen-os pair' instead");
        eprintln!("WARNING: 'sairen-os enroll' is deprecated.");
        eprintln!("  Use 'sairen-os setup' for the web-based wizard, or");
        eprintln!("  Use 'sairen-os pair' for headless pairing with a 6-digit code.");
        eprintln!();
        return run_enroll(hub, passphrase, rig_id, well_id, field, config_dir).await;
    }

    // Reset DB check — BEFORE any storage initialization
    if should_reset_db(args.reset_db) {
        reset_data_directory()?;
    }

    // Load well configuration with provenance tracking
    let (mut well_config, provenance) = config::WellConfig::load_with_provenance();

    // Pre-init auto-detection: infer config values from WITS data before freezing config.
    // For CSV mode: peek at first 30 packets from the CSV file.
    // For other modes: restore from cached auto-detected values from a previous run.
    let preloaded_packets = if let Some(ref csv_path) = args.csv {
        match load_packets(Some(csv_path.clone())) {
            Ok(packets) => {
                // Auto-detect from first N packets
                let mut detector = config::auto_detect::AutoDetector::new();
                let peek_count = packets.len().min(30);
                for packet in &packets[..peek_count] {
                    detector.observe(packet);
                }
                if detector.ready() {
                    let detected = detector.detect();
                    if let Some(mw) = detected.normal_mud_weight_ppg {
                        if !provenance.is_user_set("thresholds.hydraulics.normal_mud_weight_ppg") {
                            info!("Auto-detected mud weight: {:.1} ppg (from WITS stream)", mw);
                            well_config.thresholds.hydraulics.normal_mud_weight_ppg = mw;
                        } else {
                            info!(
                                "Mud weight: {:.1} ppg (user-configured, ignoring auto-detected {:.1})",
                                well_config.thresholds.hydraulics.normal_mud_weight_ppg, mw
                            );
                        }
                    }
                    // Cache auto-detected values for next restart
                    if let Err(e) = detected.save() {
                        warn!("Failed to cache auto-detected values: {}", e);
                    }
                }
                Some(packets)
            }
            Err(e) => {
                error!("Failed to load CSV for auto-detection: {}", e);
                None
            }
        }
    } else {
        // For non-CSV modes, try loading cached auto-detected values
        if let Some(cached) = config::auto_detect::AutoDetectedValues::load_cached() {
            if let Some(mw) = cached.normal_mud_weight_ppg {
                if !provenance.is_user_set("thresholds.hydraulics.normal_mud_weight_ppg") {
                    info!("Restored auto-detected mud weight: {:.1} ppg (from cache)", mw);
                    well_config.thresholds.hydraulics.normal_mud_weight_ppg = mw;
                }
            }
        }
        None
    };

    info!(
        "Well: {} | Field: {} | Rig: {} | Mud weight: {:.1} ppg | Bit: {:.1}\"",
        well_config.well.name,
        if well_config.well.field.is_empty() {
            "unset"
        } else {
            &well_config.well.field
        },
        if well_config.well.rig.is_empty() {
            "unset"
        } else {
            &well_config.well.rig
        },
        well_config.thresholds.hydraulics.normal_mud_weight_ppg,
        well_config.well.bit_diameter_inches
    );
    // Log deprecation warnings for env vars consolidated into TOML
    for (env_var, toml_key) in &[
        ("CAMPAIGN", "well.campaign"),
        ("WELL_ID", "well.name"),
        ("FIELD_NAME", "well.field"),
        ("SAIREN_SERVER_ADDR", "server.addr"),
        ("ML_INTERVAL_SECS", "ml.interval_secs"),
    ] {
        if std::env::var(env_var).is_ok() {
            warn!(
                "Env var {} is deprecated — use TOML key '{}' instead (env var still works as override)",
                env_var, toml_key
            );
        }
    }

    config::init(well_config, provenance);

    // Record config file path for hot-reload watcher
    {
        let config_file = std::env::var("SAIREN_CONFIG")
            .ok()
            .map(std::path::PathBuf::from)
            .filter(|p| p.exists())
            .or_else(|| {
                let local = std::path::PathBuf::from("well_config.toml");
                if local.exists() { Some(local) } else { None }
            });
        if let Some(path) = config_file {
            match path.canonicalize() {
                Ok(abs) => {
                    info!("Config file path recorded for hot-reload: {}", abs.display());
                    config::set_config_path(abs);
                }
                Err(_) => {
                    info!("Config file path recorded for hot-reload: {}", path.display());
                    config::set_config_path(path);
                }
            }
        }
    }

    // Server address: CLI > env > TOML > default
    let server_addr = args.addr
        .or_else(|| std::env::var("SAIREN_SERVER_ADDR").ok())
        .unwrap_or_else(|| config::get().server.addr.clone());

    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("  SAIREN-OS - Strategic AI Rig ENgine");
    info!("  Drilling Operational Intelligence System");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("");

    #[cfg(feature = "llm")]
    let cuda_available = llm::is_cuda_available();
    #[cfg(not(feature = "llm"))]
    let cuda_available = false;
    if cuda_available {
        info!("Hardware: CUDA detected - LLM inference will use GPU");
    } else {
        info!("Hardware: No CUDA GPU - LLM disabled (template-based advisories)");
    }
    info!("");

    // Graceful shutdown via Ctrl+C
    let cancel_token = CancellationToken::new();
    let shutdown_token = cancel_token.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("🛑 Received Ctrl+C, initiating shutdown...");
        shutdown_token.cancel();
    });

    // Dispatch to unified pipeline with the appropriate source and hooks
    if let Some(addr) = args.wits_tcp {
        // --- TCP mode ---
        let parts: Vec<&str> = addr.split(':').collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!(
                "Invalid WITS address format. Expected HOST:PORT"
            ));
        }
        let port: u16 = parts[1].parse().context("Invalid port number")?;
        let host = parts[0];

        info!("📥 Input: WITS TCP (Level 0 protocol from {})", addr);
        let source = TcpSource::connect(host, port).await?;
        run_pipeline(source, (), "WITS-TCP", server_addr, true, cancel_token).await?;
    } else if args.stdin {
        // --- Stdin mode ---
        info!("📥 Input: stdin (JSON WITS packets from simulation)");
        run_pipeline(StdinSource::new(), (), "WITS", server_addr, false, cancel_token).await?;
    } else {
        // --- CSV / synthetic mode ---
        // Reuse preloaded packets from auto-detection if available, otherwise load fresh
        let packets = match preloaded_packets {
            Some(p) => p,
            None => load_packets(args.csv)?,
        };
        let delay_ms = if args.speed == 0 {
            0
        } else {
            config::defaults::SIMULATION_BASE_DELAY_MS / args.speed
        };
        info!(
            "⏱️  Speed: {}x ({}ms delay between packets)",
            if args.speed == 0 {
                "max".to_string()
            } else {
                args.speed.to_string()
            },
            delay_ms
        );
        let total = packets.len();
        info!("📊 {} WITS packets queued for processing", total);
        let source = CsvSource::new(packets, delay_ms);
        run_pipeline(source, (), "Volve", server_addr, false, cancel_token).await?;
    }

    info!("");
    info!("✓ SAIREN-OS shutdown complete");
    Ok(())
}
