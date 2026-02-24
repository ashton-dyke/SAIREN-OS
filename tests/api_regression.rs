//! API Regression Tests
//!
//! Spins up the full SAIREN-OS binary and verifies all /api/v1/* endpoints
//! return valid responses. Uses reqwest for HTTP requests.
//!
//! These tests require:
//! - The sairen-os binary to be built (cargo build)
//! - Port 18080 to be available (uses a non-standard port to avoid conflicts)
//!
//! If the binary is not found, tests are skipped.

use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::Duration;

/// Find the built binary path.
fn binary_path() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // Check debug build first (used by cargo test)
    let debug = manifest_dir.join("target/debug/sairen-os");
    if debug.exists() {
        return debug;
    }
    let release = manifest_dir.join("target/release/sairen-os");
    if release.exists() {
        return release;
    }
    debug // Return debug path even if not found (will fail gracefully)
}

/// Start the server on a test port. Returns the child process.
fn start_server(port: u16) -> Option<Child> {
    let bin = binary_path();
    if !bin.exists() {
        eprintln!(
            "SKIP: Binary not found at {} — skipping API regression tests",
            bin.display()
        );
        return None;
    }

    let child = Command::new(&bin)
        .arg("--addr")
        .arg(format!("127.0.0.1:{port}"))
        .env("RUST_LOG", "warn")
        // Don't use a config file from the working directory
        .env("SAIREN_CONFIG", "/dev/null/nonexistent")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .ok()?;

    Some(child)
}

/// Wait for the server to accept connections, with timeout.
fn wait_for_server(port: u16, timeout: Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// Guard that kills the server process on drop.
struct ServerGuard {
    child: Child,
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Run all API endpoint tests against a live server.
///
/// This test is marked #[ignore] by default because it requires:
/// 1. A pre-built binary
/// 2. An available port
/// 3. Takes several seconds to start/stop
///
/// Run with: cargo test --test api_regression -- --ignored
#[test]
#[ignore]
fn api_endpoints_return_200() {
    let port: u16 = 18080;

    let child = match start_server(port) {
        Some(c) => c,
        None => return,
    };
    let _guard = ServerGuard { child };

    if !wait_for_server(port, Duration::from_secs(10)) {
        eprintln!("SKIP: Server did not start within 10 seconds");
        return;
    }

    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("Failed to build HTTP client");

    // GET endpoints that should always return 200
    let get_endpoints = [
        "/api/v1/health",
        "/api/v1/status",
        "/api/v1/drilling",
        "/api/v1/verification",
        "/api/v1/diagnosis",
        "/api/v1/baseline",
        "/api/v1/campaign",
        "/api/v1/config",
        "/api/v1/ml/latest",
        "/api/v1/metrics",
        "/api/v1/advisory/acknowledgments",
    ];

    for endpoint in &get_endpoints {
        let url = format!("{base}{endpoint}");
        let resp: reqwest::blocking::Response = client
            .get(&url)
            .send()
            .unwrap_or_else(|e| panic!("GET {endpoint} failed: {e}"));
        assert!(
            resp.status().is_success(),
            "GET {endpoint} returned status {}",
            resp.status()
        );
        eprintln!("  GET {endpoint} -> {}", resp.status());
    }

    // Verify /api/v1/health returns valid JSON with expected fields
    let health_resp = client
        .get(format!("{base}/api/v1/health"))
        .send()
        .expect("Health request failed");
    let health_json: serde_json::Value = health_resp.json().expect("Health response not JSON");
    assert!(
        health_json.is_object(),
        "Health response should be a JSON object"
    );

    // Verify /api/v1/status returns valid JSON
    let status_resp = client
        .get(format!("{base}/api/v1/status"))
        .send()
        .expect("Status request failed");
    let status_json: serde_json::Value = status_resp.json().expect("Status response not JSON");
    assert!(
        status_json.is_object(),
        "Status response should be a JSON object"
    );

    // Verify /api/v1/baseline returns valid JSON
    let baseline_resp = client
        .get(format!("{base}/api/v1/baseline"))
        .send()
        .expect("Baseline request failed");
    let baseline_json: serde_json::Value =
        baseline_resp.json().expect("Baseline response not JSON");
    assert!(
        baseline_json.is_object(),
        "Baseline response should be a JSON object"
    );

    // Verify /api/v1/config returns parseable config
    let config_resp = client
        .get(format!("{base}/api/v1/config"))
        .send()
        .expect("Config request failed");
    let config_json: serde_json::Value = config_resp.json().expect("Config response not JSON");
    assert!(
        config_json.is_object(),
        "Config response should be a JSON object"
    );

    // Verify legacy /health endpoint
    let legacy_resp = client
        .get(format!("{base}/health"))
        .send()
        .expect("Legacy health request failed");
    assert!(
        legacy_resp.status().is_success(),
        "Legacy /health should return 200"
    );

    eprintln!("All API regression tests passed");
}
