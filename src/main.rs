//! SAIREN-OS - Strategic AI Rig ENgine
//!
//! Real-time AI-powered drilling operational intelligence system for
//! WITS Level 0 data processing.
//!
//! # Usage
//!
//! ```bash
//! # Run with synthetic test data
//! cargo run --release
//!
//! # Run with simulation input from stdin
//! python wits_simulator.py | ./sairen-os --stdin
//!
//! # Run with LLM models (CPU - auto-detects hardware)
//! cargo run --release --features llm
//!
//! # Run with LLM models (GPU - requires CUDA toolkit)
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
#[cfg(feature = "knowledge-base")]
pub mod knowledge_base;
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
    /// Migrate a flat well_prognosis.toml into the structured knowledge base directory
    #[cfg(feature = "knowledge-base")]
    MigrateKb {
        /// Path to the source well_prognosis.toml file
        #[arg(long = "from")]
        from: String,
        /// Path to the knowledge base root directory
        #[arg(long = "to")]
        to: String,
    },

    /// Enroll this rig with a Fleet Hub using the shared passphrase
    #[cfg(feature = "fleet-client")]
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
// Configuration
// ============================================================================

/// Application configuration
#[derive(Debug, Clone)]
struct AppConfig {
    /// HTTP server bind address
    server_addr: String,
}

impl AppConfig {
    fn from_env() -> Self {
        Self {
            server_addr: std::env::var("SAIREN_SERVER_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:8080".to_string()),
        }
    }
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
    #[cfg(feature = "fleet-client")]
    FleetUploader,
    #[cfg(feature = "fleet-client")]
    FleetLibrarySync,
    #[cfg(feature = "fleet-client")]
    FleetIntelligenceSync,
}

impl std::fmt::Display for TaskName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskName::HttpServer => write!(f, "HttpServer"),
            TaskName::PacketProcessor => write!(f, "PacketProcessor"),
            TaskName::MLScheduler => write!(f, "MLScheduler"),
            #[cfg(feature = "fleet-client")]
            TaskName::FleetUploader => write!(f, "FleetUploader"),
            #[cfg(feature = "fleet-client")]
            TaskName::FleetLibrarySync => write!(f, "FleetLibrarySync"),
            #[cfg(feature = "fleet-client")]
            TaskName::FleetIntelligenceSync => write!(f, "FleetIntelligenceSync"),
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
                            state.acknowledgments.push(rec);
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

    #[cfg(feature = "knowledge-base")]
    if let Some(handle) = coordinator.start_kb_watcher() {
        info!("✓ Knowledge base watcher started");
        drop(handle);
    }

    info!("🌐 Starting HTTP server on {}...", server_addr);
    let dashboard_state = DashboardState::new_with_storage_and_thresholds(
        Arc::clone(&app_state),
        threshold_manager.clone(),
        equipment_id,
    );
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

/// Spawn fleet client background tasks (uploader + library sync).
///
/// Returns a `FleetContext` with the shared queue so the packet processor
/// can enqueue events directly.
#[cfg(feature = "fleet-client")]
fn spawn_fleet_tasks(
    task_set: &mut JoinSet<Result<TaskName>>,
) -> Option<pipeline::processing_loop::FleetContext> {
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
    let well_id = std::env::var("WELL_ID").unwrap_or_else(|_| "UNKNOWN".to_string());

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

    Some(pipeline::processing_loop::FleetContext { queue, rig_id, well_id })
}

/// Spawn the ML Engine Scheduler task.
fn spawn_ml_scheduler(
    task_set: &mut JoinSet<Result<TaskName>>,
    app_state: Arc<RwLock<AppState>>,
    cancel_token: CancellationToken,
) {
    task_set.spawn(async move {
        use ml_engine::{MLScheduler, get_interval};

        #[cfg(feature = "knowledge-base")]
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

                    #[cfg(feature = "knowledge-base")]
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
                        let mut m = physics_engine::tactical_update(p, None);
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

                    #[cfg(feature = "knowledge-base")]
                    {
                        if let Some(ref kb) = ml_knowledge_base {
                            if let Err(e) = kb.write_snapshot_with_packets(&report, &snapshot_packets) {
                                warn!("Failed to write KB snapshot: {}", e);
                            }
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

    // Fleet client background tasks
    #[cfg(feature = "fleet-client")]
    let fleet_ctx = spawn_fleet_tasks(&mut task_set);

    // Task 2: Packet Processor (unified processing loop)
    let proc_cancel = cancel_token.clone();
    let proc_state = Arc::clone(&app_state);
    #[cfg(feature = "fleet-client")]
    let proc_fleet = fleet_ctx;
    task_set.spawn(async move {
        info!("[PacketProcessor] Task starting");

        let processing_loop = ProcessingLoop::new(
            core.coordinator,
            proc_state,
            hooks,
            proc_cancel,
        );

        #[cfg(feature = "fleet-client")]
        let processing_loop = if let Some(ctx) = proc_fleet {
            processing_loop.with_fleet(ctx)
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
#[cfg(feature = "fleet-client")]
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
#[cfg(feature = "fleet-client")]
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

    let args = CliArgs::parse();

    // Subcommand dispatch
    #[cfg(feature = "knowledge-base")]
    if let Some(SubCommand::MigrateKb { from, to }) = &args.command {
        let from_path = std::path::Path::new(from);
        let to_path = std::path::Path::new(to);
        info!("Migrating knowledge base: {} -> {}", from, to);
        knowledge_base::migration::migrate_flat_to_kb(from_path, to_path)?;
        info!("Knowledge base migration complete");
        return Ok(());
    }

    #[cfg(feature = "fleet-client")]
    if let Some(SubCommand::Enroll {
        hub,
        passphrase,
        rig_id,
        well_id,
        field,
        config_dir,
    }) = &args.command
    {
        return run_enroll(hub, passphrase, rig_id, well_id, field, config_dir).await;
    }

    // Reset DB check — BEFORE any storage initialization
    if should_reset_db(args.reset_db) {
        reset_data_directory()?;
    }

    // Load well configuration
    let well_config = config::WellConfig::load();
    info!(
        "Well: {} | Rig: {} | Bit: {:.1}\"",
        well_config.well.name,
        if well_config.well.rig.is_empty() {
            "unset"
        } else {
            &well_config.well.rig
        },
        well_config.well.bit_diameter_inches
    );
    config::init(well_config);

    let app_config = AppConfig::from_env();
    let server_addr = args.addr.unwrap_or(app_config.server_addr);

    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("  SAIREN-OS - Strategic AI Rig ENgine");
    info!("  Drilling Operational Intelligence System");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("");

    #[cfg(feature = "llm")]
    {
        let cuda_available = llm::is_cuda_available();
        if cuda_available {
            info!("🖥️  Hardware: CUDA detected - LLM inference will use GPU");
            info!("   Strategic model: Qwen 2.5 7B (GPU, ~800ms)");
        } else {
            info!("🖥️  Hardware: CUDA not available - LLM inference will use CPU");
            info!("   Strategic model: Qwen 2.5 4B (CPU, ~10-30s)");
        }
        info!("   Tactical routing: deterministic pattern matching (no LLM)");
        info!("");
    }
    #[cfg(not(feature = "llm"))]
    {
        info!("🖥️  LLM: disabled (template-based advisories)");
        info!("");
    }

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
        let packets = load_packets(args.csv)?;
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
