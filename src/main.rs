//! SAIREN-OS - Strategic AI Rig ENgine
//!
//! Real-time drilling operational intelligence system for
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
//! ```
//!
//! # Environment Variables
//!
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
mod gossip;
mod ml_engine;
mod pipeline;
mod storage;
mod strategic;

// Multi-agent architecture modules
pub mod aci;
pub mod agents;
pub mod background;
pub mod baseline;
pub mod causal;
pub mod cfc;
pub mod config;
pub mod context;
pub mod debrief;
pub mod fleet;
pub mod knowledge_base;
pub mod optimization;
pub mod physics_engine;
pub mod sensors;
pub mod types;
pub mod volve;

use api::{create_app, DashboardState};
use axum::Router;
use pipeline::processing_loop::{PostProcessHooks, ProcessingLoop};
use pipeline::source::{CsvSource, PacketSource, StdinSource, TcpSource};
use pipeline::{AppState, PipelineCoordinator};

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
    ConfigWatcher,
    GossipBroadcast,
}

impl std::fmt::Display for TaskName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskName::HttpServer => write!(f, "HttpServer"),
            TaskName::PacketProcessor => write!(f, "PacketProcessor"),
            TaskName::MLScheduler => write!(f, "MLScheduler"),
            TaskName::ConfigWatcher => write!(f, "ConfigWatcher"),
            TaskName::GossipBroadcast => write!(f, "GossipBroadcast"),
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
    /// Gossip event store (shared with server handlers and client loop).
    gossip_store: Option<Arc<tokio::sync::Mutex<gossip::store::EventStore>>>,
    /// Mesh peer sync state (shared with server handlers and client loop).
    mesh_state: Option<Arc<gossip::state::MeshState>>,
}

/// Initialize the shared pipeline: AppState, storage, thresholds, coordinator,
/// dashboard, and HTTP listener.
async fn init_pipeline(equipment_id: &str, server_addr: &str) -> Result<PipelineCore> {
    use baseline::ThresholdManager;
    use std::path::Path;

    let app_state = Arc::new(RwLock::new(AppState::from_env()));
    info!("✓ Application state initialized");

    info!("🔒 Acquiring process lock...");
    let _process_lock =
        storage::ProcessLock::acquire("./data").context("Failed to acquire process lock")?;
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

    let mut app = create_app(dashboard_state);

    // Initialize gossip store and mesh routes if mesh is enabled
    let mesh_cfg = &config::get().mesh;
    let (gossip_store, mesh_state) = if mesh_cfg.enabled {
        info!("🔗 Initializing P2P mesh gossip...");
        std::fs::create_dir_all("./data").ok();
        let store_path = std::path::Path::new("./data/gossip_events.db");
        match gossip::store::EventStore::open(store_path) {
            Ok(store) => {
                let store = Arc::new(tokio::sync::Mutex::new(store));
                let sled_db =
                    sled::open("./data/mesh_state").context("Failed to open mesh state sled DB")?;
                let mesh_st = Arc::new(
                    gossip::state::MeshState::new(&sled_db)
                        .map_err(|e| anyhow::anyhow!("Failed to init mesh state: {}", e))?,
                );

                let handler_state = gossip::server::MeshHandlerState {
                    node_id: equipment_id.to_string(),
                    store: Arc::clone(&store),
                    mesh_state: Arc::clone(&mesh_st),
                };
                app = app.nest(
                    "/api/mesh",
                    api::mesh_routes::mesh_api_routes(handler_state),
                );

                info!(
                    "✓ Mesh gossip initialized ({} peers configured)",
                    mesh_cfg.peers.len()
                );
                (Some(store), Some(mesh_st))
            }
            Err(e) => {
                warn!("Failed to open gossip event store: {} — mesh disabled", e);
                (None, None)
            }
        }
    } else {
        info!("[Mesh] Disabled (mesh.enabled = false)");
        (None, None)
    };

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
        gossip_store,
        mesh_state,
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
fn spawn_config_watcher(task_set: &mut JoinSet<Result<TaskName>>, cancel_token: CancellationToken) {
    let Some(path) = config::config_path().cloned() else {
        info!("[ConfigWatcher] No config file path recorded — watcher disabled");
        return;
    };

    let (tx, mut rx) = tokio::sync::mpsc::channel(4);

    // Spawn the polling loop
    task_set.spawn(async move {
        info!(
            "[ConfigWatcher] Task starting — watching {}",
            path.display()
        );

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
    info!("   Phase 7: Orchestrator Voting (4 Specialists)");
    info!("   Phase 8: Storage");
    info!("   Phase 9: Dashboard API");
    info!("");

    let core = init_pipeline(equipment_id, &server_addr).await?;
    let app_state = core.app_state;

    info!("🔒 Supervisor: Initializing task monitoring");
    let mut task_set: JoinSet<Result<TaskName>> = JoinSet::new();

    // Task 1: HTTP Server
    spawn_http_server(&mut task_set, core.listener, core.app, cancel_token.clone());

    // Task 2: Packet Processor (unified processing loop)
    let proc_cancel = cancel_token.clone();
    let proc_state = Arc::clone(&app_state);
    task_set.spawn(async move {
        info!("[PacketProcessor] Task starting");

        let processing_loop = ProcessingLoop::new(core.coordinator, proc_state, hooks, proc_cancel);

        let _stats = processing_loop.run(&mut source).await;
        Ok(TaskName::PacketProcessor)
    });

    // Task 3: ML Engine Scheduler (only for streaming modes with history)
    if spawn_ml {
        spawn_ml_scheduler(&mut task_set, Arc::clone(&app_state), cancel_token.clone());
    }

    // Task 4: Config File Watcher (hot-reload on file changes)
    spawn_config_watcher(&mut task_set, cancel_token.clone());

    // Task 5: Gossip Broadcast (if mesh is enabled)
    if let (Some(gossip_store), Some(mesh_state)) = (core.gossip_store, core.mesh_state) {
        let gossip_cfg = config::get().gossip.clone();
        let mesh_cfg = config::get().mesh.clone();
        let node_id = core.equipment_id.clone();
        task_set.spawn(async move {
            info!("[GossipBroadcast] Task starting");
            gossip::client::run_gossip_loop(
                node_id,
                mesh_cfg.peers,
                gossip_store,
                mesh_state,
                gossip_cfg,
            )
            .await;
            Ok(TaskName::GossipBroadcast)
        });
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
                    info!(
                        "Restored auto-detected mud weight: {:.1} ppg (from cache)",
                        mw
                    );
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
                if local.exists() {
                    Some(local)
                } else {
                    None
                }
            });
        if let Some(path) = config_file {
            match path.canonicalize() {
                Ok(abs) => {
                    info!(
                        "Config file path recorded for hot-reload: {}",
                        abs.display()
                    );
                    config::set_config_path(abs);
                }
                Err(_) => {
                    info!(
                        "Config file path recorded for hot-reload: {}",
                        path.display()
                    );
                    config::set_config_path(path);
                }
            }
        }
    }

    // Server address: CLI > env > TOML > default
    let server_addr = args
        .addr
        .or_else(|| std::env::var("SAIREN_SERVER_ADDR").ok())
        .unwrap_or_else(|| config::get().server.addr.clone());

    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("  SAIREN-OS - Strategic AI Rig ENgine");
    info!("  Drilling Operational Intelligence System");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("");

    info!("Hardware: CfC neural networks (pure Rust, no GPU required)");
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
        run_pipeline(
            StdinSource::new(),
            (),
            "WITS",
            server_addr,
            false,
            cancel_token,
        )
        .await?;
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
