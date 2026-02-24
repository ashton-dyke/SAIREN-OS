//! Setup wizard API handlers
//!
//! Served when `sairen-os setup` is run. Provides endpoints for:
//! - WITS subnet scanning and connection testing
//! - Well identity configuration
//! - Fleet Hub pairing via 6-digit codes
//! - Config file generation

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::acquisition::scanner;

/// Setup wizard HTML (embedded at compile time)
const SETUP_HTML: &str = include_str!("../../static/setup.html");

// ============================================================================
// Shared State
// ============================================================================

/// Shared state for the setup wizard session.
#[derive(Clone)]
pub struct SetupState {
    inner: Arc<RwLock<SetupStateInner>>,
    pub config_dir: String,
    pub port_ranges: Vec<(u16, u16)>,
}

struct SetupStateInner {
    /// Most recent scan results
    scan_results: Option<ScanResponse>,
    /// Whether a scan is currently running
    scanning: bool,
    /// Fleet pairing state
    pairing: Option<PairingState>,
}

struct PairingState {
    hub_url: String,
    code: String,
    rig_id: String,
    well_id: String,
    field: String,
}

impl SetupState {
    pub fn new(config_dir: String, port_ranges: Vec<(u16, u16)>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(SetupStateInner {
                scan_results: None,
                scanning: false,
                pairing: None,
            })),
            config_dir,
            port_ranges,
        }
    }
}

// ============================================================================
// Request / Response Types
// ============================================================================

#[derive(Debug, Serialize, Clone)]
pub struct ScanResponse {
    pub status: String,
    pub elapsed_ms: u64,
    pub streams: Vec<scanner::WitsDiscovery>,
}

#[derive(Debug, Deserialize)]
pub struct ConnectRequest {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Serialize)]
pub struct ConnectResponse {
    pub success: bool,
    pub validated: bool,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct SaveRequest {
    pub wits_host: String,
    pub wits_port: u16,
    pub well_name: String,
    pub field: String,
    pub rig_id: String,
    /// Fleet pairing passphrase (set after successful pairing)
    pub fleet_hub_url: Option<String>,
    pub fleet_passphrase: Option<String>,
    pub fleet_rig_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SaveResponse {
    pub success: bool,
    pub message: String,
    pub config_path: String,
}

#[derive(Debug, Deserialize)]
pub struct PairRequest {
    pub hub_url: String,
    pub rig_id: String,
    pub well_id: String,
    pub field: String,
}

#[derive(Debug, Serialize)]
pub struct PairResponse {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct PairStatusResponse {
    pub status: String,
    pub passphrase: Option<String>,
}

// ============================================================================
// Handlers
// ============================================================================

/// GET / — Serve the setup wizard HTML
pub async fn serve_setup() -> Html<&'static str> {
    Html(SETUP_HTML)
}

/// GET /api/setup/scan — Trigger a subnet scan for WITS streams
pub async fn start_scan(State(state): State<SetupState>) -> Response {
    {
        let inner = state.inner.read().await;
        if inner.scanning {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": "Scan already in progress"})),
            )
                .into_response();
        }
    }

    // Mark scanning = true
    {
        let mut inner = state.inner.write().await;
        inner.scanning = true;
        inner.scan_results = None;
    }

    let port_ranges = state.port_ranges.clone();
    let state_clone = state.inner.clone();

    // Run scan in background
    tokio::spawn(async move {
        let start = std::time::Instant::now();
        let streams = scanner::scan_subnet(&port_ranges, 2000).await;
        let elapsed = start.elapsed().as_millis() as u64;

        let result = ScanResponse {
            status: "complete".to_string(),
            elapsed_ms: elapsed,
            streams,
        };

        let mut inner = state_clone.write().await;
        inner.scan_results = Some(result);
        inner.scanning = false;
    });

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({"status": "scanning"})),
    )
        .into_response()
}

/// GET /api/setup/scan/status — Poll scan progress
pub async fn scan_status(State(state): State<SetupState>) -> Json<serde_json::Value> {
    let inner = state.inner.read().await;
    if inner.scanning {
        Json(serde_json::json!({"status": "scanning"}))
    } else if let Some(ref results) = inner.scan_results {
        Json(serde_json::to_value(results).unwrap_or_default())
    } else {
        Json(serde_json::json!({"status": "idle"}))
    }
}

/// POST /api/setup/connect — Test a WITS connection
pub async fn test_connect(Json(req): Json<ConnectRequest>) -> Json<ConnectResponse> {
    info!("Testing WITS connection to {}:{}", req.host, req.port);

    let addr = format!("{}:{}", req.host, req.port);
    let timeout = std::time::Duration::from_secs(5);

    match tokio::time::timeout(timeout, tokio::net::TcpStream::connect(&addr)).await {
        Ok(Ok(mut stream)) => {
            // Try to read WITS header
            use tokio::io::AsyncReadExt;
            let mut buf = [0u8; 256];
            let validated =
                match tokio::time::timeout(std::time::Duration::from_secs(3), stream.read(&mut buf))
                    .await
                {
                    Ok(Ok(n)) if n >= 4 => {
                        let data = &buf[..n];
                        data.windows(4).any(|w| w == b"&&\r\n")
                    }
                    Ok(Ok(n)) if n > 0 => {
                        let data = &buf[..n];
                        data.windows(2).any(|w| w == b"&&")
                    }
                    _ => false,
                };

            Json(ConnectResponse {
                success: true,
                validated,
                message: if validated {
                    "Connected — WITS Level 0 data confirmed".to_string()
                } else {
                    "TCP connected but no WITS header detected (may still be valid)".to_string()
                },
            })
        }
        Ok(Err(e)) => Json(ConnectResponse {
            success: false,
            validated: false,
            message: format!("Connection failed: {}", e),
        }),
        Err(_) => Json(ConnectResponse {
            success: false,
            validated: false,
            message: "Connection timed out (5s)".to_string(),
        }),
    }
}

/// POST /api/setup/save — Write config files
pub async fn save_config(
    State(state): State<SetupState>,
    Json(req): Json<SaveRequest>,
) -> Json<SaveResponse> {
    use std::path::Path;

    let config_dir = Path::new(&state.config_dir);

    // Ensure config directory exists
    if let Err(e) = std::fs::create_dir_all(config_dir) {
        return Json(SaveResponse {
            success: false,
            message: format!("Failed to create config directory: {}", e),
            config_path: state.config_dir.clone(),
        });
    }

    // Write well_config.toml — use typed serialization to prevent TOML injection
    let toml_path = config_dir.join("well_config.toml");

    #[derive(serde::Serialize)]
    struct GeneratedConfig {
        well: GeneratedWell,
        server: GeneratedServer,
        wits: GeneratedWits,
    }
    #[derive(serde::Serialize)]
    struct GeneratedWell { name: String, field: String, rig: String }
    #[derive(serde::Serialize)]
    struct GeneratedServer { addr: String }
    #[derive(serde::Serialize)]
    struct GeneratedWits { tcp_address: String }

    let config = GeneratedConfig {
        well: GeneratedWell {
            name: req.well_name.clone(),
            field: req.field.clone(),
            rig: req.rig_id.clone(),
        },
        server: GeneratedServer {
            addr: "0.0.0.0:8080".to_string(),
        },
        wits: GeneratedWits {
            tcp_address: format!("{}:{}", req.wits_host, req.wits_port),
        },
    };

    let toml_body = match toml::to_string_pretty(&config) {
        Ok(s) => s,
        Err(e) => {
            return Json(SaveResponse {
                success: false,
                message: format!("Failed to serialize config: {}", e),
                config_path: toml_path.display().to_string(),
            });
        }
    };
    let toml_content = format!("# SAIREN-OS Well Configuration\n# Generated by setup wizard\n\n{toml_body}");

    if let Err(e) = std::fs::write(&toml_path, &toml_content) {
        return Json(SaveResponse {
            success: false,
            message: format!("Failed to write config: {}", e),
            config_path: toml_path.display().to_string(),
        });
    }
    info!("Wrote config to {}", toml_path.display());

    // Write env file if fleet pairing was done
    if let (Some(hub_url), Some(passphrase), Some(fleet_rig_id)) =
        (&req.fleet_hub_url, &req.fleet_passphrase, &req.fleet_rig_id)
    {
        let env_path = config_dir.join("env");
        let existing = std::fs::read_to_string(&env_path).unwrap_or_default();
        let mut updated = existing;
        updated = update_env_var(&updated, "FLEET_HUB_URL", hub_url);
        updated = update_env_var(&updated, "FLEET_PASSPHRASE", passphrase);
        updated = update_env_var(&updated, "FLEET_RIG_ID", fleet_rig_id);
        updated = update_env_var(&updated, "WELL_ID", &req.well_name);

        if let Err(e) = std::fs::write(&env_path, &updated) {
            warn!("Failed to write env file: {}", e);
        } else {
            info!("Wrote fleet env to {}", env_path.display());
        }
    }

    Json(SaveResponse {
        success: true,
        message: "Configuration saved. Restart sairen-os to begin monitoring.".to_string(),
        config_path: toml_path.display().to_string(),
    })
}

/// POST /api/setup/fleet/pair — Initiate fleet pairing (generate 6-digit code)
pub async fn initiate_pair(
    State(state): State<SetupState>,
    Json(req): Json<PairRequest>,
) -> Response {
    use rand::Rng;

    let code: String = {
        let mut rng = rand::thread_rng();
        format!("{:06}", rng.gen_range(0..1_000_000u32))
    };

    let hub_url = req.hub_url.trim_end_matches('/').to_string();

    // Send pairing request to hub
    let http = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("HTTP client error: {}", e)})),
            )
                .into_response();
        }
    };

    let body = serde_json::json!({
        "rig_id": req.rig_id,
        "well_id": req.well_id,
        "field": req.field,
        "code": code,
    });

    match http
        .post(format!("{}/api/fleet/pair/request", hub_url))
        .json(&body)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() || resp.status() == StatusCode::ACCEPTED => {
            // Store pairing state
            let mut inner = state.inner.write().await;
            inner.pairing = Some(PairingState {
                hub_url: hub_url.clone(),
                code: code.clone(),
                rig_id: req.rig_id,
                well_id: req.well_id,
                field: req.field,
            });

            (
                StatusCode::OK,
                Json(PairResponse {
                    code,
                    message: "Pairing code sent to hub. Approve it on the Fleet Hub dashboard."
                        .to_string(),
                }),
            )
                .into_response()
        }
        Ok(resp) => {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "error": format!("Hub returned {} — {}", status, text)
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({
                "error": format!("Cannot reach hub: {}", e)
            })),
        )
            .into_response(),
    }
}

/// GET /api/setup/fleet/status — Poll fleet pairing status
pub async fn pair_status(State(state): State<SetupState>) -> Json<PairStatusResponse> {
    let inner = state.inner.read().await;

    let pairing = match &inner.pairing {
        Some(p) => p,
        None => {
            return Json(PairStatusResponse {
                status: "no_pairing".to_string(),
                passphrase: None,
            });
        }
    };

    // Poll hub for status
    let http = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => {
            return Json(PairStatusResponse {
                status: "error".to_string(),
                passphrase: None,
            });
        }
    };

    let url = format!(
        "{}/api/fleet/pair/status?code={}",
        pairing.hub_url, pairing.code
    );
    match http.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            #[derive(Deserialize)]
            struct HubPairStatus {
                status: String,
                passphrase: Option<String>,
            }

            match resp.json::<HubPairStatus>().await {
                Ok(s) => Json(PairStatusResponse {
                    status: s.status,
                    passphrase: s.passphrase,
                }),
                Err(_) => Json(PairStatusResponse {
                    status: "error".to_string(),
                    passphrase: None,
                }),
            }
        }
        _ => Json(PairStatusResponse {
            status: "error".to_string(),
            passphrase: None,
        }),
    }
}

// ============================================================================
// Router
// ============================================================================

/// Build the setup wizard router.
pub fn setup_router(state: SetupState) -> axum::Router {
    use axum::routing::{get, post};

    axum::Router::new()
        .route("/", get(serve_setup))
        .route("/api/setup/scan", get(start_scan))
        .route("/api/setup/scan/status", get(scan_status))
        .route("/api/setup/connect", post(test_connect))
        .route("/api/setup/save", post(save_config))
        .route("/api/setup/fleet/pair", post(initiate_pair))
        .route("/api/setup/fleet/status", get(pair_status))
        .with_state(state)
}

// ============================================================================
// Helpers
// ============================================================================

/// Update or insert an environment variable in a shell env file.
fn update_env_var(contents: &str, key: &str, value: &str) -> String {
    let mut found = false;
    let mut lines: Vec<String> = contents
        .lines()
        .map(|line| {
            let trimmed = line.trim();
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
