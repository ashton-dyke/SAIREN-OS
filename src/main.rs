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
//! # Run with LLM models
//! TACTICAL_MODEL_PATH=models/qwen2.5-1.5b-instruct-q4_k_m.gguf \
//! STRATEGIC_MODEL_PATH=models/deepseek-r1-distill-qwen-7b-q4.gguf \
//! cargo run --release --features llm
//! ```
//!
//! # Environment Variables
//!
//! - `TACTICAL_MODEL_PATH`: Path to tactical LLM (Qwen 2.5 1.5B Instruct)
//! - `STRATEGIC_MODEL_PATH`: Path to strategic LLM (DeepSeek R1 Distill Qwen 7B)
//! - `RUST_LOG`: Logging level (default: info)
//! - `RESET_DB`: Set to "true" to wipe all persistent data on startup (for testing)

use anyhow::{Context, Result};
use clap::Parser;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

mod acquisition;
mod api;
mod director;
mod llm;
mod ml_engine;
mod pipeline;
mod processing;
mod storage;
mod strategic;

// Multi-agent architecture modules
pub mod types;
pub mod agents;
pub mod physics_engine;
pub mod context;
pub mod sensors;
pub mod baseline;

use acquisition::StdinSensorSource;
use api::{create_app, DashboardState};
use pipeline::{AppState, PipelineCoordinator};
use types::{Campaign, WitsPacket};

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
}

// ============================================================================
// Configuration
// ============================================================================

/// Application configuration
#[derive(Debug, Clone)]
struct AppConfig {
    /// HTTP server bind address
    server_addr: String,
    /// Channel buffer size for WITS packets
    channel_buffer_size: usize,
}

impl AppConfig {
    fn from_env() -> Self {
        Self {
            server_addr: std::env::var("SAIREN_SERVER_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:8080".to_string()),
            channel_buffer_size: 10_000,
        }
    }
}

// ============================================================================
// Database Reset
// ============================================================================

/// Default data directory path
const DATA_DIR: &str = "./data";

/// Check if database reset is requested via CLI flag or environment variable.
/// Returns true if RESET_DB=true env var is set OR --reset-db flag is present.
fn should_reset_db(cli_flag: bool) -> bool {
    if cli_flag {
        return true;
    }

    // Check environment variable (case-insensitive, accepts "true", "1", "yes")
    if let Ok(val) = std::env::var("RESET_DB") {
        let val_lower = val.to_lowercase();
        return val_lower == "true" || val_lower == "1" || val_lower == "yes";
    }

    false
}

/// Safely remove the data directory and all its contents.
/// This is called BEFORE any storage initialization.
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

    // List contents before deletion for logging
    if let Ok(entries) = fs::read_dir(data_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            let file_type = if path.is_dir() { "DIR " } else { "FILE" };
            warn!("    {} {}", file_type, path.display());
        }
    }

    // Remove the entire directory tree
    fs::remove_dir_all(data_path)
        .context("Failed to remove data directory")?;

    warn!("");
    warn!("  Data directory removed successfully.");
    warn!("  A fresh database will be created on startup.");
    warn!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    warn!("");

    Ok(())
}

// ============================================================================
// Multi-Agent Pipeline Runner
// ============================================================================

/// Run the drilling intelligence pipeline
async fn run_drilling_pipeline(
    csv_path: Option<String>,
    use_stdin: bool,
    wits_tcp: Option<String>,
    speed: u64,
    server_addr: String,
    cancel_token: CancellationToken,
) -> Result<()> {
    info!("🚀 Starting SAIREN-OS Drilling Intelligence Pipeline");
    info!("");
    info!("   Phase 1: WITS Ingestion (continuous)");
    info!("   Phase 2: Basic Physics (MSE, d-exponent, etc.)");
    info!("   Phase 3: Tactical Agent → AdvisoryTicket");
    info!("   Phase 4: History Buffer (60 packets)");
    info!("   Phase 5: Strategic Agent → verify_ticket()");
    info!("   Phase 6: Context Lookup");
    info!("   Phase 7: LLM Explainer");
    info!("   Phase 8: Orchestrator Voting (4 Specialists)");
    info!("   Phase 9: Storage");
    info!("   Phase 10: Dashboard API");
    info!("");

    // Determine input mode
    if let Some(addr) = wits_tcp {
        info!("📥 Input: WITS TCP (Level 0 protocol from {})", addr);
        return run_pipeline_wits_tcp(addr, server_addr, cancel_token).await;
    }

    if use_stdin {
        info!("📥 Input: stdin (JSON WITS packets from simulation)");
        return run_pipeline_stdin(speed, server_addr, cancel_token).await;
    }

    // Load WITS data from CSV or synthetic
    let packets = if let Some(path) = csv_path {
        info!("📂 Loading WITS data from CSV: {}", path);
        let data = sensors::read_csv_data(&path);
        if data.is_empty() {
            return Err(anyhow::anyhow!("No WITS data loaded from CSV"));
        }
        info!("   Loaded {} packets", data.len());
        data
    } else {
        info!("🧪 Using synthetic test data (drilling fault simulation)");
        let data = sensors::generate_fault_test_data();
        info!("   Generated {} packets", data.len());
        data
    };

    // Calculate delay between packets based on speed
    let delay_ms = if speed == 0 {
        0
    } else {
        60_000 / speed
    };

    info!("⏱️  Speed: {}x ({}ms delay between packets)",
        if speed == 0 { "max".to_string() } else { speed.to_string() },
        delay_ms
    );
    info!("");

    // Initialize pipeline coordinator
    #[cfg(feature = "llm")]
    let mut coordinator = {
        info!("🧠 Initializing LLM-enabled pipeline...");
        match PipelineCoordinator::init_with_llm().await {
            Ok(c) => {
                info!("✓ LLM loaded successfully");
                c
            }
            Err(e) => {
                warn!("⚠️  LLM initialization failed: {}. Using template mode.", e);
                PipelineCoordinator::new()
            }
        }
    };

    #[cfg(not(feature = "llm"))]
    let mut coordinator = PipelineCoordinator::new();

    info!("📊 Processing {} WITS packets...", packets.len());
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let mut packets_processed = 0u64;
    let mut advisories_generated = 0u64;

    for packet in &packets {
        if cancel_token.is_cancelled() {
            info!("🛑 Shutdown signal received");
            break;
        }

        packets_processed += 1;

        // Process through pipeline (default to Production campaign for file-based runs)
        let advisory = coordinator.process_packet(packet, Campaign::Production).await;

        if let Some(ref adv) = advisory {
            advisories_generated += 1;

            // Log advisory
            info!(
                "🎯 ADVISORY #{}: {:?} | Risk: {:?} | Efficiency: {}%",
                advisories_generated,
                adv.votes.first().map(|v| &v.specialist).unwrap_or(&"Unknown".to_string()),
                adv.risk_level,
                adv.efficiency_score
            );
            info!("   Recommendation: {}", truncate_str(&adv.recommendation, 70));
            info!("   Expected Benefit: {}", truncate_str(&adv.expected_benefit, 70));

            // Log specialist votes
            for vote in &adv.votes {
                info!(
                    "   {} ({:.0}%): {} - {}",
                    vote.specialist,
                    vote.weight * 100.0,
                    vote.vote,
                    truncate_str(&vote.reasoning, 50)
                );
            }
            info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        }

        // Progress indicator every 10 packets
        if advisory.is_none() && packets_processed % 10 == 0 {
            let stats = coordinator.get_stats();
            info!(
                "📈 Progress: {}/{} packets | Advisories: {} | Buffer: {}/60",
                packets_processed,
                packets.len(),
                stats.strategic_analyses,
                stats.history_buffer_size
            );
        }

        // Delay between packets
        if delay_ms > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
        }
    }

    // Final statistics
    let stats = coordinator.get_stats();

    info!("");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("📊 FINAL STATISTICS");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("   Packets Processed:    {}", stats.packets_processed);
    info!("   Tickets Created:      {}", stats.tickets_created);
    info!("   Tickets Verified:     {}", stats.tickets_verified);
    info!("   Tickets Rejected:     {}", stats.tickets_rejected);
    info!("   Advisories Generated: {}", stats.strategic_analyses);
    info!("   History Buffer Size:  {}/60", stats.history_buffer_size);
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    Ok(())
}

/// Truncate string for display
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

/// Task identification for supervisor logging
#[derive(Debug, Clone, Copy)]
enum TaskName {
    HttpServer,
    WitsIngestion,
    PacketProcessor,
    MLScheduler,
}

impl std::fmt::Display for TaskName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskName::HttpServer => write!(f, "HttpServer"),
            TaskName::WitsIngestion => write!(f, "WitsIngestion"),
            TaskName::PacketProcessor => write!(f, "PacketProcessor"),
            TaskName::MLScheduler => write!(f, "MLScheduler"),
        }
    }
}

/// Run the pipeline with stdin input
async fn run_pipeline_stdin(
    _speed: u64,
    server_addr: String,
    cancel_token: CancellationToken,
) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    use baseline::ThresholdManager;
    use std::path::Path;

    info!("⏱️  Stdin mode: processing packets as they arrive");
    info!("");

    // Initialize shared application state for dashboard
    let app_state = Arc::new(RwLock::new(AppState::default()));
    info!("✓ Application state initialized");

    // Acquire process lock to prevent multiple instances
    info!("🔒 Acquiring process lock...");
    let _process_lock = storage::ProcessLock::acquire("./data")
        .context("Failed to acquire process lock")?;
    info!("✓ Process lock acquired");

    // Initialize analysis storage
    info!("💾 Initializing analysis storage...");
    let storage = storage::AnalysisStorage::open("./data/sairen.db")
        .context("Failed to open analysis storage")?;

    // Clean up old data (keep last 7 days)
    match storage.cleanup_old(7) {
        Ok(deleted) if deleted > 0 => {
            info!("🗑️  Cleaned up {} old analysis records", deleted);
        }
        Ok(_) => {}
        Err(e) => {
            warn!("Failed to clean up old data: {}", e);
        }
    }
    info!("✓ Analysis storage initialized");

    // Initialize history storage for strategic reports
    info!("💾 Initializing strategic history storage...");
    if let Err(e) = storage::history::init("./data/strategic_history.db") {
        warn!("Failed to initialize history storage: {}. Reports will not be persisted.", e);
    } else {
        info!("✓ Strategic history storage initialized");
    }

    // Initialize ThresholdManager for dynamic baselines
    let equipment_id = "WITS".to_string();
    info!("📊 Initializing dynamic threshold system for: {}", equipment_id);

    let thresholds_path = Path::new("./data/thresholds.json");
    let threshold_manager = Arc::new(std::sync::RwLock::new({
        match ThresholdManager::load_from_file(thresholds_path) {
            Ok(mgr) => {
                let locked_count = mgr.locked_count();
                info!("✓ Loaded {} locked baselines from {:?}", locked_count, thresholds_path);
                mgr
            }
            Err(_) => {
                info!("📝 No existing thresholds found, starting fresh baseline learning");
                let mut mgr = ThresholdManager::new();
                mgr.start_wits_learning(&equipment_id, 0);
                info!("   Started learning for WITS drilling metrics");
                mgr
            }
        }
    }));

    // Determine if we should start in learning mode
    let start_in_learning_mode = {
        let mgr = threshold_manager.read().unwrap();
        if mgr.locked_count() > 0 {
            info!("🎯 Mode: DynamicThresholds (using learned baselines)");
            false
        } else {
            info!("📚 Mode: BaselineLearning (accumulating samples)");
            true
        }
    };

    // Initialize pipeline coordinator
    #[cfg(feature = "llm")]
    let mut coordinator = {
        info!("🧠 Initializing LLM-enabled pipeline with dynamic thresholds...");
        match PipelineCoordinator::init_with_llm_and_thresholds(
            threshold_manager.clone(),
            equipment_id.clone(),
            start_in_learning_mode,
        ).await {
            Ok(c) => {
                info!("✓ LLM and dynamic thresholds loaded successfully");
                c
            }
            Err(e) => {
                warn!("⚠️  LLM initialization failed: {}. Using template mode.", e);
                PipelineCoordinator::new_with_thresholds(
                    threshold_manager.clone(),
                    equipment_id.clone(),
                    start_in_learning_mode,
                )
            }
        }
    };

    #[cfg(not(feature = "llm"))]
    let mut coordinator = PipelineCoordinator::new_with_thresholds(
        threshold_manager.clone(),
        equipment_id.clone(),
        start_in_learning_mode,
    );

    // Start HTTP server for dashboard
    info!("🌐 Starting HTTP server on {}...", server_addr);
    let dashboard_state = DashboardState::new_with_storage_and_thresholds(
        Arc::clone(&app_state),
        storage,
        threshold_manager.clone(),
        &equipment_id,
    );
    let app = create_app(dashboard_state);

    let listener = tokio::net::TcpListener::bind(&server_addr)
        .await
        .with_context(|| format!("Failed to bind to {}", server_addr))?;

    info!("✓ HTTP server listening on {}", server_addr);
    info!("");
    info!("🎯 Dashboard available at: http://{}", server_addr);
    info!("");

    // JoinSet Supervisor Pattern
    info!("🔒 Supervisor: Initializing task monitoring");
    let mut task_set: JoinSet<Result<TaskName>> = JoinSet::new();

    // Create channel for passing packets from ingestion to processor
    let (packet_tx, packet_rx) = mpsc::channel::<WitsPacket>(1000);

    // Task 1: HTTP Server
    let http_cancel = cancel_token.clone();
    task_set.spawn(async move {
        info!("[HttpServer] Task starting");

        let result = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                http_cancel.cancelled().await;
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

    // Task 2: WITS Ingestion (stdin reader)
    let ingestion_cancel = cancel_token.clone();
    task_set.spawn(async move {
        info!("[WitsIngestion] Task starting");

        let stdin = tokio::io::stdin();
        let mut reader = BufReader::new(stdin);
        let mut line_buffer = String::with_capacity(2048);
        let mut packets_read = 0u64;

        loop {
            tokio::select! {
                _ = ingestion_cancel.cancelled() => {
                    info!("[WitsIngestion] Received shutdown signal after {} packets", packets_read);
                    return Ok(TaskName::WitsIngestion);
                }
                result = reader.read_line(&mut line_buffer) => {
                    match result {
                        Ok(0) => {
                            info!("[WitsIngestion] EOF reached after {} packets", packets_read);
                            return Ok(TaskName::WitsIngestion);
                        }
                        Ok(_) => {
                            let line = line_buffer.trim();
                            if !line.is_empty() {
                                match serde_json::from_str::<WitsPacket>(line) {
                                    Ok(packet) => {
                                        packets_read += 1;
                                        if packet_tx.send(packet).await.is_err() {
                                            error!("[WitsIngestion] Packet channel closed");
                                            return Err(anyhow::anyhow!("Packet channel closed"));
                                        }
                                    }
                                    Err(e) => {
                                        warn!("[WitsIngestion] Failed to parse packet: {}", e);
                                    }
                                }
                            }
                            line_buffer.clear();
                        }
                        Err(e) => {
                            error!("[WitsIngestion] Read error: {}", e);
                            return Err(anyhow::anyhow!("Stdin read error: {}", e));
                        }
                    }
                }
            }
        }
    });

    // Task 3: Packet Processor
    let processor_cancel = cancel_token.clone();
    let processor_app_state = Arc::clone(&app_state);
    task_set.spawn(async move {
        info!("[PacketProcessor] Task starting");

        let mut packet_rx = packet_rx;
        let mut packets_processed = 0u64;
        let mut advisories_generated = 0u64;

        info!("📊 Processing WITS packets from stdin...");
        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        loop {
            tokio::select! {
                _ = processor_cancel.cancelled() => {
                    info!("[PacketProcessor] Received shutdown signal");
                    break;
                }
                maybe_packet = packet_rx.recv() => {
                    match maybe_packet {
                        Some(packet) => {
                            packets_processed += 1;

                            // Update app state with current WITS data and get campaign
                            let campaign = {
                                let mut state = processor_app_state.write().await;
                                state.current_rpm = packet.rpm;
                                state.samples_collected = packets_processed as usize;
                                state.total_analyses = packets_processed;
                                state.last_analysis_time = Some(chrono::Utc::now());
                                state.status = pipeline::SystemStatus::Monitoring;
                                // Store latest WITS packet for dashboard
                                state.latest_wits_packet = Some(packet.clone());
                                state.campaign
                            };

                            // Process through pipeline with campaign context
                            let advisory = coordinator.process_packet(&packet, campaign).await;

                            if let Some(ref adv) = advisory {
                                advisories_generated += 1;

                                // Update app state with strategic advisory
                                {
                                    let mut state = processor_app_state.write().await;
                                    state.latest_advisory = Some(adv.clone());
                                    state.latest_strategic_report = Some(adv.clone());
                                }

                                // Persist to history storage
                                if let Err(e) = storage::history::store_report(adv) {
                                    warn!("Failed to persist advisory to history: {}", e);
                                }

                                // Log advisory
                                info!(
                                    "🎯 ADVISORY #{}: {:?} | Efficiency: {}%",
                                    advisories_generated,
                                    adv.risk_level,
                                    adv.efficiency_score
                                );
                                info!("   Recommendation: {}", truncate_str(&adv.recommendation, 70));

                                for vote in &adv.votes {
                                    info!(
                                        "   {} ({:.0}%): {} - {}",
                                        vote.specialist,
                                        vote.weight * 100.0,
                                        vote.vote,
                                        truncate_str(&vote.reasoning, 50)
                                    );
                                }
                                info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                            }

                            // Progress indicator every 10 packets
                            if advisory.is_none() && packets_processed % 10 == 0 {
                                let stats = coordinator.get_stats();
                                info!(
                                    "📈 Progress: {} packets | Advisories: {} | Buffer: {}/60",
                                    packets_processed,
                                    stats.strategic_analyses,
                                    stats.history_buffer_size
                                );
                            }
                        }
                        None => {
                            info!("[PacketProcessor] Packet channel closed");
                            break;
                        }
                    }
                }
            }
        }

        // Final statistics
        let stats = coordinator.get_stats();

        info!("");
        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        info!("📊 FINAL STATISTICS");
        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        info!("   Packets Processed:    {}", stats.packets_processed);
        info!("   Tickets Created:      {}", stats.tickets_created);
        info!("   Tickets Verified:     {}", stats.tickets_verified);
        info!("   Tickets Rejected:     {}", stats.tickets_rejected);
        info!("   Advisories Generated: {}", stats.strategic_analyses);
        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        Ok(TaskName::PacketProcessor)
    });

    // Supervisor loop
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

/// Run the pipeline with WITS Level 0 TCP input
async fn run_pipeline_wits_tcp(
    wits_addr: String,
    server_addr: String,
    cancel_token: CancellationToken,
) -> Result<()> {
    use baseline::ThresholdManager;
    use std::path::Path;
    use acquisition::WitsClient;

    info!("⏱️  WITS TCP mode: connecting to {} for Level 0 data", wits_addr);
    info!("");

    // Parse host:port from wits_addr
    let (host, port) = {
        let parts: Vec<&str> = wits_addr.split(':').collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!("Invalid WITS address format. Expected HOST:PORT"));
        }
        let port: u16 = parts[1].parse()
            .context("Invalid port number")?;
        (parts[0].to_string(), port)
    };

    // Initialize shared application state for dashboard
    let app_state = Arc::new(RwLock::new(AppState::default()));
    info!("✓ Application state initialized");

    // Acquire process lock to prevent multiple instances
    info!("🔒 Acquiring process lock...");
    let _process_lock = storage::ProcessLock::acquire("./data")
        .context("Failed to acquire process lock")?;
    info!("✓ Process lock acquired");

    // Initialize analysis storage
    info!("💾 Initializing analysis storage...");
    let storage = storage::AnalysisStorage::open("./data/sairen.db")
        .context("Failed to open analysis storage")?;

    // Clean up old data (keep last 7 days)
    match storage.cleanup_old(7) {
        Ok(deleted) if deleted > 0 => {
            info!("🗑️  Cleaned up {} old analysis records", deleted);
        }
        Ok(_) => {}
        Err(e) => {
            warn!("Failed to clean up old data: {}", e);
        }
    }
    info!("✓ Analysis storage initialized");

    // Initialize history storage for strategic reports (ignore if already initialized)
    let _ = storage::history::init("./data/strategic_history.db");

    // Initialize ThresholdManager for dynamic baselines
    let equipment_id = "WITS-TCP".to_string();
    info!("📊 Initializing dynamic threshold system for: {}", equipment_id);

    let thresholds_path = Path::new("./data/thresholds.json");
    let threshold_manager = Arc::new(std::sync::RwLock::new({
        match ThresholdManager::load_from_file(thresholds_path) {
            Ok(mgr) => {
                let locked_count = mgr.locked_count();
                info!("✓ Loaded {} locked baselines from {:?}", locked_count, thresholds_path);
                mgr
            }
            Err(_) => {
                info!("📝 No existing thresholds found, starting fresh baseline learning");
                let mut mgr = ThresholdManager::new();
                mgr.start_wits_learning(&equipment_id, 0);
                info!("   Started learning for WITS drilling metrics");
                mgr
            }
        }
    }));

    // Determine if we should start in learning mode
    let start_in_learning_mode = {
        let mgr = threshold_manager.read().unwrap();
        if mgr.locked_count() > 0 {
            info!("🎯 Mode: DynamicThresholds (using learned baselines)");
            false
        } else {
            info!("📚 Mode: BaselineLearning (accumulating samples)");
            true
        }
    };

    // Initialize pipeline coordinator
    #[cfg(feature = "llm")]
    let coordinator = {
        info!("🧠 Initializing LLM-enabled pipeline with dynamic thresholds...");
        match PipelineCoordinator::init_with_llm_and_thresholds(
            threshold_manager.clone(),
            equipment_id.clone(),
            start_in_learning_mode,
        ).await {
            Ok(c) => {
                info!("✓ LLM and dynamic thresholds loaded successfully");
                c
            }
            Err(e) => {
                warn!("⚠️  LLM initialization failed: {}. Using template mode.", e);
                PipelineCoordinator::new_with_thresholds(
                    threshold_manager.clone(),
                    equipment_id.clone(),
                    start_in_learning_mode,
                )
            }
        }
    };

    #[cfg(not(feature = "llm"))]
    let coordinator = PipelineCoordinator::new_with_thresholds(
        threshold_manager.clone(),
        equipment_id.clone(),
        start_in_learning_mode,
    );

    // Create dashboard state for HTTP server
    let dashboard_state = DashboardState::new_with_storage_and_thresholds(
        app_state.clone(),
        storage,
        threshold_manager.clone(),
        &equipment_id,
    );
    let app = create_app(dashboard_state);

    let listener = tokio::net::TcpListener::bind(&server_addr)
        .await
        .with_context(|| format!("Failed to bind to {}", server_addr))?;

    info!("✓ HTTP server listening on {}", server_addr);
    info!("");
    info!("🎯 Dashboard available at: http://{}", server_addr);
    info!("");

    // JoinSet Supervisor Pattern
    info!("🔒 Supervisor: Initializing task monitoring");
    let mut task_set: JoinSet<Result<TaskName>> = JoinSet::new();

    // Create channel for passing packets from ingestion to processor
    let (packet_tx, packet_rx) = mpsc::channel::<WitsPacket>(1000);

    // Task 1: HTTP Server
    let http_cancel = cancel_token.clone();
    task_set.spawn(async move {
        info!("[HttpServer] Task starting");

        let result = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                http_cancel.cancelled().await;
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

    // Task 2: WITS TCP Ingestion
    let ingestion_cancel = cancel_token.clone();
    let wits_host = host.clone();
    let wits_port = port;
    task_set.spawn(async move {
        info!("[WitsTcpIngestion] Task starting - connecting to {}:{}", wits_host, wits_port);

        let mut client = WitsClient::new(&wits_host, wits_port);
        let mut packets_read = 0u64;
        let mut reconnect_attempts = 0;
        const MAX_RECONNECT_ATTEMPTS: u32 = 10;

        loop {
            // Try to connect if not connected
            if !client.is_connected() {
                info!("[WitsTcpIngestion] Connecting to WITS server...");
                match client.connect().await {
                    Ok(()) => {
                        info!("[WitsTcpIngestion] Connected successfully");
                        reconnect_attempts = 0;
                    }
                    Err(e) => {
                        reconnect_attempts += 1;
                        if reconnect_attempts >= MAX_RECONNECT_ATTEMPTS {
                            error!("[WitsTcpIngestion] Max reconnect attempts reached");
                            return Err(anyhow::anyhow!("Failed to connect after {} attempts: {}", MAX_RECONNECT_ATTEMPTS, e));
                        }
                        warn!("[WitsTcpIngestion] Connection failed (attempt {}): {}. Retrying...", reconnect_attempts, e);
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                        continue;
                    }
                }
            }

            tokio::select! {
                _ = ingestion_cancel.cancelled() => {
                    info!("[WitsTcpIngestion] Received shutdown signal after {} packets", packets_read);
                    let _ = client.disconnect().await;
                    return Ok(TaskName::WitsIngestion);
                }
                result = client.read_packet() => {
                    match result {
                        Ok(packet) => {
                            packets_read += 1;
                            if packet_tx.send(packet).await.is_err() {
                                error!("[WitsTcpIngestion] Packet channel closed");
                                return Err(anyhow::anyhow!("Packet channel closed"));
                            }
                        }
                        Err(acquisition::WitsError::ConnectionClosed) => {
                            warn!("[WitsTcpIngestion] Connection closed by server after {} packets", packets_read);
                            // Mark as disconnected and try to reconnect
                            let _ = client.disconnect().await;
                            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                        }
                        Err(e) => {
                            warn!("[WitsTcpIngestion] Error reading packet: {}", e);
                            // Try to reconnect on error
                            let _ = client.disconnect().await;
                            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                        }
                    }
                }
            }
        }
    });

    // Task 3: Packet Processor
    let processor_cancel = cancel_token.clone();
    let processor_app_state = Arc::clone(&app_state);
    task_set.spawn(async move {
        info!("[PacketProcessor] Task starting");

        let mut coordinator = coordinator;
        let mut packet_rx = packet_rx;
        let mut packets_processed = 0u64;
        let mut advisories_generated = 0u64;

        info!("📊 Processing WITS packets from TCP...");
        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        loop {
            tokio::select! {
                _ = processor_cancel.cancelled() => {
                    info!("[PacketProcessor] Received shutdown signal");
                    break;
                }
                maybe_packet = packet_rx.recv() => {
                    match maybe_packet {
                        Some(packet) => {
                            packets_processed += 1;

                            // Update app state with current WITS data and get campaign
                            let campaign = {
                                let mut state = processor_app_state.write().await;
                                state.current_rpm = packet.rpm;
                                state.samples_collected = packets_processed as usize;
                                state.total_analyses = packets_processed;
                                state.last_analysis_time = Some(chrono::Utc::now());
                                state.status = pipeline::SystemStatus::Monitoring;
                                // Store latest WITS packet for dashboard
                                state.latest_wits_packet = Some(packet.clone());
                                // Add to ML history buffer (keep 2 hours at 1 Hz)
                                if state.wits_history.len() >= 7200 {
                                    state.wits_history.pop_front();
                                }
                                state.wits_history.push_back(packet.clone());
                                state.campaign
                            };

                            // Process through pipeline with campaign context
                            let advisory = coordinator.process_packet(&packet, campaign).await;

                            // Store latest drilling metrics (includes operation classification)
                            if let Some(metrics) = coordinator.get_latest_metrics() {
                                let mut state = processor_app_state.write().await;
                                state.latest_drilling_metrics = Some(metrics.clone());
                            }

                            if let Some(ref adv) = advisory {
                                advisories_generated += 1;

                                // Update app state with strategic advisory
                                {
                                    let mut state = processor_app_state.write().await;
                                    state.latest_advisory = Some(adv.clone());
                                    state.latest_strategic_report = Some(adv.clone());
                                }

                                // Persist to history storage
                                if let Err(e) = storage::history::store_report(adv) {
                                    warn!("Failed to persist advisory to history: {}", e);
                                }

                                // Log advisory
                                info!(
                                    "🎯 ADVISORY #{}: {:?} | Efficiency: {}%",
                                    advisories_generated,
                                    adv.risk_level,
                                    adv.efficiency_score
                                );
                                info!("   Recommendation: {}", truncate_str(&adv.recommendation, 70));

                                for vote in &adv.votes {
                                    info!(
                                        "   {} ({:.0}%): {} - {}",
                                        vote.specialist,
                                        vote.weight * 100.0,
                                        vote.vote,
                                        truncate_str(&vote.reasoning, 50)
                                    );
                                }
                                info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                            }

                            // Progress indicator every 10 packets
                            if advisory.is_none() && packets_processed % 10 == 0 {
                                let stats = coordinator.get_stats();
                                info!(
                                    "📈 Progress: {} packets | Advisories: {} | Buffer: {}/60",
                                    packets_processed,
                                    stats.strategic_analyses,
                                    stats.history_buffer_size
                                );
                            }
                        }
                        None => {
                            info!("[PacketProcessor] Packet channel closed");
                            break;
                        }
                    }
                }
            }
        }

        // Final statistics
        let stats = coordinator.get_stats();

        info!("");
        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        info!("📊 FINAL STATISTICS");
        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        info!("   Packets Processed:    {}", stats.packets_processed);
        info!("   Tickets Created:      {}", stats.tickets_created);
        info!("   Tickets Verified:     {}", stats.tickets_verified);
        info!("   Tickets Rejected:     {}", stats.tickets_rejected);
        info!("   Advisories Generated: {}", stats.strategic_analyses);
        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        Ok(TaskName::PacketProcessor)
    });

    // Task 4: ML Engine Scheduler
    let ml_cancel = cancel_token.clone();
    let ml_app_state = Arc::clone(&app_state);
    task_set.spawn(async move {
        use ml_engine::{MLScheduler, get_interval};

        info!("[MLScheduler] Task starting with interval {:?}", get_interval());

        let mut interval = tokio::time::interval(get_interval());
        let mut analyses_run = 0u64;

        loop {
            tokio::select! {
                _ = ml_cancel.cancelled() => {
                    info!("[MLScheduler] Received shutdown signal after {} analyses", analyses_run);
                    return Ok(TaskName::MLScheduler);
                }
                _ = interval.tick() => {
                    // Get data from app state
                    let (packets, campaign, well_id, field_name, bit_hours, bit_depth) = {
                        let state = ml_app_state.read().await;
                        (
                            state.wits_history.iter().cloned().collect::<Vec<_>>(),
                            state.campaign,
                            state.well_id.clone(),
                            state.field_name.clone(),
                            state.bit_hours,
                            state.bit_depth_drilled,
                        )
                    };

                    // Check if we have enough data for analysis
                    if packets.len() < 100 {
                        info!(
                            "[MLScheduler] Skipping analysis: insufficient data ({} packets, need 100+)",
                            packets.len()
                        );
                        continue;
                    }

                    analyses_run += 1;
                    info!("[MLScheduler] Running ML analysis #{} with {} packets", analyses_run, packets.len());

                    // Build dataset and run analysis
                    let metrics: Vec<types::DrillingMetrics> = packets.iter().map(|p| {
                        // Detect operation from packet parameters
                        let operation = agents::tactical::detect_operation(p, campaign);
                        types::DrillingMetrics {
                            state: p.rig_state,
                            operation,
                            mse: p.mse,
                            mse_efficiency: 100.0 - (p.mse / 50000.0 * 100.0).min(100.0),
                            d_exponent: p.d_exponent,
                            dxc: p.dxc,
                            mse_delta_percent: 0.0,
                            flow_balance: p.flow_in - p.flow_out,
                            pit_rate: p.pit_volume_change,
                            ecd_margin: 14.0 - p.ecd,
                            torque_delta_percent: p.torque_delta_percent,
                            spp_delta: p.spp_delta,
                            is_anomaly: false,
                            anomaly_category: types::AnomalyCategory::None,
                            anomaly_description: None,
                        }
                    }).collect();

                    let dataset = MLScheduler::build_dataset(
                        packets,
                        metrics,
                        &well_id,
                        &field_name,
                        campaign,
                        bit_hours,
                        bit_depth,
                    );

                    let report = MLScheduler::run_analysis(&dataset);

                    // Store the report in app state
                    {
                        let mut state = ml_app_state.write().await;
                        state.latest_ml_report = Some(report.clone());
                    }

                    // Log the result
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

    // Supervisor loop
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

    // Parse CLI arguments
    let args = CliArgs::parse();

    // =========================================================================
    // RESET_DB CHECK - Must happen BEFORE any storage initialization
    // =========================================================================
    if should_reset_db(args.reset_db) {
        reset_data_directory()?;
    }

    // Load configuration
    let config = AppConfig::from_env();

    // Override server address if provided via CLI
    let server_addr = args.addr.unwrap_or(config.server_addr);

    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("  SAIREN-OS - Strategic AI Rig ENgine");
    info!("  Drilling Operational Intelligence System");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("");

    // Create cancellation token for graceful shutdown
    let cancel_token = CancellationToken::new();
    let shutdown_token = cancel_token.clone();

    // Setup signal handlers
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("🛑 Received Ctrl+C, initiating shutdown...");
        shutdown_token.cancel();
    });

    // Run the drilling pipeline
    run_drilling_pipeline(
        args.csv,
        args.stdin,
        args.wits_tcp,
        args.speed,
        server_addr,
        cancel_token,
    ).await?;

    info!("");
    info!("✓ SAIREN-OS shutdown complete");
    Ok(())
}
