//! WITS Level 0 Protocol Parser
//!
//! Parses WITS (Wellsite Information Transfer Specification) Level 0 data.
//! WITS Level 0 uses a simple ASCII format: &&\r\n[MMNNVALUE\r\n...]\r\n!!\r\n
//!
//! MM = Record type (01-99)
//! NN = Item number within record (01-99)
//! VALUE = ASCII value (variable format)
//!
//! Common WITS Record 01 (Time-based drilling data):
//! - 0108: Bit Depth (ft)
//! - 0110: Hole Depth (ft)
//! - 0113: ROP (ft/hr)
//! - 0114: Hook Load (klbs)
//! - 0116: WOB (klbs)
//! - 0117: RPM
//! - 0118: Torque (kft-lbs)
//! - 0119: Standpipe Pressure (psi)
//! - 0120: Pump SPM 1
//! - 0121: Flow In (gpm)

use crate::types::{RigState, WitsPacket};
use anyhow::{Context, Result};
use std::collections::HashMap;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// WITS protocol errors
#[derive(Debug, Error)]
pub enum WitsError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Protocol error: {0}")]
    ProtocolError(String),

    #[error("Parse error for item {item}: {message}")]
    ParseError { item: String, message: String },

    #[error("Timeout waiting for data")]
    Timeout,

    #[error("Connection closed")]
    ConnectionClosed,
}

/// WITS item codes for drilling data (Record 01)
pub mod wits_items {
    pub const BIT_DEPTH: &str = "0108";
    pub const HOLE_DEPTH: &str = "0110";
    pub const ROP: &str = "0113";
    pub const HOOK_LOAD: &str = "0114";
    pub const WOB: &str = "0116";
    pub const RPM: &str = "0117";
    pub const TORQUE: &str = "0118";
    pub const SPP: &str = "0119";
    pub const PUMP_SPM_1: &str = "0120";
    pub const FLOW_IN: &str = "0121";
    pub const FLOW_OUT: &str = "0122";
    pub const PIT_VOLUME: &str = "0123";
    pub const MUD_WEIGHT_IN: &str = "0124";
    pub const MUD_WEIGHT_OUT: &str = "0125";
    pub const MUD_TEMP_IN: &str = "0126";
    pub const MUD_TEMP_OUT: &str = "0127";
    pub const CASING_PRESSURE: &str = "0130";
    pub const GAS_UNITS: &str = "0140";
    pub const TOTAL_GAS: &str = "0141";
    pub const H2S: &str = "0142";
    pub const CO2: &str = "0143";
    pub const ECD: &str = "0150";
    pub const BLOCK_POSITION: &str = "0105";
    pub const ROTARY_TORQUE: &str = "0118";
}

/// Default read timeout for WITS data (seconds).
/// Drilling data arrives at 1-60 second intervals. 120s covers
/// even the slowest sample rates with margin for network latency.
const DEFAULT_READ_TIMEOUT_SECS: u64 = 120;

/// Maximum reconnection attempts before giving up.
const MAX_RECONNECT_ATTEMPTS: u32 = 10;

/// Initial reconnection delay (doubles each attempt).
const INITIAL_RECONNECT_DELAY_SECS: u64 = 2;

/// Maximum reconnection delay cap (seconds).
const MAX_RECONNECT_DELAY_SECS: u64 = 60;

/// Stale connection timeout — if no data for this long, force reconnect.
const STALE_CONNECTION_SECS: u64 = 300;

/// WITS Level 0 TCP client with reconnection and timeout resilience
pub struct WitsClient {
    host: String,
    port: u16,
    stream: Option<BufReader<TcpStream>>,
    connected: bool,
    line_buffer: String,
    /// Read timeout per line (seconds)
    read_timeout_secs: u64,
    /// Timestamp of last successful data receipt (Unix secs)
    last_data_time: u64,
    /// Consecutive reconnection attempts (resets on success)
    reconnect_attempts: u32,
    /// Total packets received since creation
    packets_received: u64,
    /// Total reconnections performed
    reconnections: u64,
    /// Total timeouts encountered
    timeouts: u64,
}

impl WitsClient {
    /// Create new WITS client with default settings
    pub fn new(host: &str, port: u16) -> Self {
        Self {
            host: host.to_string(),
            port,
            stream: None,
            connected: false,
            line_buffer: String::with_capacity(256),
            read_timeout_secs: DEFAULT_READ_TIMEOUT_SECS,
            last_data_time: 0,
            reconnect_attempts: 0,
            packets_received: 0,
            reconnections: 0,
            timeouts: 0,
        }
    }

    /// Set the read timeout (seconds). Default is 120s.
    pub fn with_read_timeout(mut self, secs: u64) -> Self {
        self.read_timeout_secs = secs;
        self
    }

    /// Connect to WITS server with timeout
    pub async fn connect(&mut self) -> Result<(), WitsError> {
        if self.connected {
            return Ok(());
        }

        let addr = format!("{}:{}", self.host, self.port);
        tracing::info!(address = %addr, "Connecting to WITS server");

        let connect_timeout = tokio::time::Duration::from_secs(30);
        let stream = tokio::time::timeout(connect_timeout, TcpStream::connect(&addr))
            .await
            .map_err(|_| WitsError::Timeout)?
            .map_err(|e| WitsError::ConnectionFailed(e.to_string()))?;

        // Enable TCP keepalive to detect dead connections
        let sock_ref = socket2::SockRef::from(&stream);
        let keepalive = socket2::TcpKeepalive::new()
            .with_time(std::time::Duration::from_secs(30))
            .with_interval(std::time::Duration::from_secs(10));
        let _ = sock_ref.set_tcp_keepalive(&keepalive);

        self.stream = Some(BufReader::new(stream));
        self.connected = true;
        self.last_data_time = current_unix_secs();
        self.reconnect_attempts = 0;

        tracing::info!("WITS connection established");
        Ok(())
    }

    /// Disconnect from WITS server
    pub async fn disconnect(&mut self) -> Result<(), WitsError> {
        if let Some(ref mut reader) = self.stream {
            let _ = reader.get_mut().shutdown().await;
        }
        self.stream = None;
        self.connected = false;
        tracing::info!("WITS connection closed");
        Ok(())
    }

    /// Reconnect with exponential backoff.
    ///
    /// Returns Ok(()) when reconnected, Err if max attempts exhausted.
    pub async fn reconnect(&mut self) -> Result<(), WitsError> {
        // Disconnect first
        let _ = self.disconnect().await;

        for attempt in 1..=MAX_RECONNECT_ATTEMPTS {
            self.reconnect_attempts = attempt;

            let delay_secs = (INITIAL_RECONNECT_DELAY_SECS * 2u64.saturating_pow(attempt - 1))
                .min(MAX_RECONNECT_DELAY_SECS);

            tracing::warn!(
                attempt = attempt,
                max_attempts = MAX_RECONNECT_ATTEMPTS,
                delay_secs = delay_secs,
                "WITS reconnecting after failure"
            );

            tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs)).await;

            match self.connect().await {
                Ok(()) => {
                    self.reconnections += 1;
                    tracing::info!(
                        attempt = attempt,
                        total_reconnections = self.reconnections,
                        "WITS reconnection successful"
                    );
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!(attempt = attempt, error = %e, "Reconnection attempt failed");
                }
            }
        }

        tracing::error!(
            max_attempts = MAX_RECONNECT_ATTEMPTS,
            "WITS reconnection exhausted — all attempts failed"
        );
        Err(WitsError::ConnectionFailed(format!(
            "Failed to reconnect after {} attempts",
            MAX_RECONNECT_ATTEMPTS
        )))
    }

    /// Read next WITS packet with timeout and stale connection detection.
    ///
    /// Automatically reconnects on timeout or connection drop.
    pub async fn read_packet(&mut self) -> Result<WitsPacket, WitsError> {
        // Check for stale connection
        let now = current_unix_secs();
        if self.connected && self.last_data_time > 0 && (now - self.last_data_time) > STALE_CONNECTION_SECS {
            tracing::warn!(
                silent_secs = now - self.last_data_time,
                threshold = STALE_CONNECTION_SECS,
                "WITS connection stale — no data received, forcing reconnect"
            );
            self.reconnect().await?;
        }

        // Ensure connected
        if !self.connected {
            self.connect().await?;
        }

        match self.read_packet_inner().await {
            Ok(packet) => {
                self.last_data_time = current_unix_secs();
                self.packets_received += 1;
                self.reconnect_attempts = 0;
                Ok(packet)
            }
            Err(WitsError::Timeout) => {
                self.timeouts += 1;
                tracing::warn!(
                    timeout_secs = self.read_timeout_secs,
                    total_timeouts = self.timeouts,
                    "WITS read timeout — attempting reconnect"
                );
                self.reconnect().await?;
                // Try one more read after reconnect
                self.read_packet_inner().await
            }
            Err(WitsError::ConnectionClosed) => {
                tracing::warn!("WITS connection closed by server — attempting reconnect");
                self.reconnect().await?;
                self.read_packet_inner().await
            }
            Err(e) => Err(e),
        }
    }

    /// Inner packet read with timeout — does NOT auto-reconnect.
    async fn read_packet_inner(&mut self) -> Result<WitsPacket, WitsError> {
        let reader = self.stream.as_mut()
            .ok_or(WitsError::ConnectionFailed("Not connected".to_string()))?;

        let mut items: HashMap<String, f64> = HashMap::new();
        let mut in_record = false;
        let read_timeout = tokio::time::Duration::from_secs(self.read_timeout_secs);

        loop {
            self.line_buffer.clear();

            let read_result = tokio::time::timeout(
                read_timeout,
                reader.read_line(&mut self.line_buffer),
            )
            .await;

            let bytes = match read_result {
                Ok(Ok(b)) => b,
                Ok(Err(e)) => return Err(WitsError::ConnectionFailed(e.to_string())),
                Err(_) => return Err(WitsError::Timeout),
            };

            if bytes == 0 {
                return Err(WitsError::ConnectionClosed);
            }

            let line = self.line_buffer.trim();

            // Start of record
            if line == "&&" {
                in_record = true;
                items.clear();
                continue;
            }

            // End of record
            if line == "!!" {
                if in_record && !items.is_empty() {
                    return Ok(Self::items_to_packet(&items));
                }
                in_record = false;
                continue;
            }

            // Parse data item (MMNNVALUE format)
            if in_record && line.len() >= 5 {
                let item_code = &line[0..4];
                let value_str = &line[4..];

                if let Ok(value) = value_str.trim().parse::<f64>() {
                    items.insert(item_code.to_string(), value);
                }
            }
        }
    }

    /// Get connection health statistics
    pub fn stats(&self) -> WitsClientStats {
        WitsClientStats {
            connected: self.connected,
            packets_received: self.packets_received,
            reconnections: self.reconnections,
            timeouts: self.timeouts,
            last_data_secs_ago: if self.last_data_time > 0 {
                current_unix_secs().saturating_sub(self.last_data_time)
            } else {
                0
            },
        }
    }

    /// Convert parsed items to WitsPacket
    fn items_to_packet(items: &HashMap<String, f64>) -> WitsPacket {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Extract all WITS items with defaults
        let bit_depth = items.get(wits_items::BIT_DEPTH).copied().unwrap_or(0.0);
        let hole_depth = items.get(wits_items::HOLE_DEPTH).copied().unwrap_or(0.0);
        let rop = items.get(wits_items::ROP).copied().unwrap_or(0.0);
        let hook_load = items.get(wits_items::HOOK_LOAD).copied().unwrap_or(0.0);
        let wob = items.get(wits_items::WOB).copied().unwrap_or(0.0);
        let rpm = items.get(wits_items::RPM).copied().unwrap_or(0.0);
        let torque = items.get(wits_items::TORQUE).copied().unwrap_or(0.0);
        let spp = items.get(wits_items::SPP).copied().unwrap_or(0.0);
        let pump_spm = items.get(wits_items::PUMP_SPM_1).copied().unwrap_or(0.0);
        let flow_in = items.get(wits_items::FLOW_IN).copied().unwrap_or(0.0);
        let flow_out = items.get(wits_items::FLOW_OUT).copied().unwrap_or(0.0);
        let pit_volume = items.get(wits_items::PIT_VOLUME).copied().unwrap_or(0.0);
        let mud_weight_in = items.get(wits_items::MUD_WEIGHT_IN).copied().unwrap_or(0.0);
        let mud_weight_out = items.get(wits_items::MUD_WEIGHT_OUT).copied().unwrap_or(0.0);
        let mud_temp_in = items.get(wits_items::MUD_TEMP_IN).copied().unwrap_or(0.0);
        let mud_temp_out = items.get(wits_items::MUD_TEMP_OUT).copied().unwrap_or(0.0);
        let casing_pressure = items.get(wits_items::CASING_PRESSURE).copied().unwrap_or(0.0);
        let gas_units = items.get(wits_items::GAS_UNITS).copied().unwrap_or(0.0);
        let h2s = items.get(wits_items::H2S).copied().unwrap_or(0.0);
        let co2 = items.get(wits_items::CO2).copied().unwrap_or(0.0);
        let ecd = items.get(wits_items::ECD).copied().unwrap_or(0.0);
        let block_position = items.get(wits_items::BLOCK_POSITION).copied().unwrap_or(0.0);

        // Classify rig state based on parameters
        let rig_state = classify_rig_state(rpm, wob, hook_load, rop, block_position);

        WitsPacket {
            timestamp,
            // Drilling parameters
            bit_depth,
            hole_depth,
            rop,
            hook_load,
            wob,
            rpm,
            torque,
            bit_diameter: if crate::config::is_initialized() {
                crate::config::get().well.bit_diameter_inches
            } else {
                8.5
            },
            // Hydraulics
            spp,
            pump_spm,
            flow_in,
            flow_out,
            pit_volume,
            pit_volume_change: 0.0,
            // Mud properties
            mud_weight_in,
            mud_weight_out,
            ecd,
            mud_temp_in,
            mud_temp_out,
            // Well control / Gas
            gas_units,
            background_gas: 0.0,
            connection_gas: 0.0,
            h2s,
            co2,
            casing_pressure,
            annular_pressure: 0.0,
            // Formation
            pore_pressure: 0.0,
            fracture_gradient: 0.0,
            // Derived (calculated elsewhere)
            mse: 0.0,
            d_exponent: 0.0,
            dxc: 0.0,
            rop_delta: 0.0,
            torque_delta_percent: 0.0,
            spp_delta: 0.0,
            // State
            rig_state,
            waveform_snapshot: std::sync::Arc::new(Vec::new()),
        }
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.connected
    }
}

/// Classify rig state from drilling parameters
fn classify_rig_state(rpm: f64, wob: f64, hook_load: f64, rop: f64, block_position: f64) -> RigState {
    // Drilling: rotation + weight + ROP
    if rpm > 20.0 && wob > 5.0 && rop > 0.0 {
        return RigState::Drilling;
    }

    // Reaming: rotation + weight, but no ROP (or reaming up)
    if rpm > 20.0 && wob > 2.0 && rop <= 0.0 {
        return RigState::Reaming;
    }

    // Circulating: rotation, minimal weight
    if rpm > 0.0 && wob < 5.0 {
        return RigState::Circulating;
    }

    // Tripping: no rotation, block moving
    if rpm < 5.0 && block_position > 0.0 {
        // Hook load variation indicates tripping direction
        if hook_load > 150.0 {
            return RigState::TrippingOut;
        }
        return RigState::TrippingIn;
    }

    // Connection: stationary, typical hook load
    if rpm < 5.0 && hook_load > 50.0 && hook_load < 200.0 {
        return RigState::Connection;
    }

    // Default to idle
    RigState::Idle
}

/// WITS client connection health statistics
#[derive(Debug, Clone, serde::Serialize)]
pub struct WitsClientStats {
    pub connected: bool,
    pub packets_received: u64,
    pub reconnections: u64,
    pub timeouts: u64,
    pub last_data_secs_ago: u64,
}

/// Get current Unix timestamp in seconds
fn current_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ============================================================================
// Data Quality Validation
// ============================================================================

/// Data quality issues found in a WITS packet
#[derive(Debug, Clone, serde::Serialize)]
pub struct DataQualityReport {
    /// Whether the packet is usable for analysis
    pub usable: bool,
    /// List of quality issues found
    pub issues: Vec<DataQualityIssue>,
    /// Number of zero-valued critical fields
    pub zero_critical_fields: usize,
    /// Number of fields with physically impossible values
    pub impossible_values: usize,
}

/// A single data quality issue
#[derive(Debug, Clone, serde::Serialize)]
pub struct DataQualityIssue {
    pub field: String,
    pub severity: QualitySeverity,
    pub message: String,
}

/// Severity of a data quality issue
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum QualitySeverity {
    /// Data is corrupted — packet should be discarded
    Critical,
    /// Data is suspicious — proceed with caution
    Warning,
    /// Minor issue — informational only
    Info,
}

/// Validate a WITS packet for data quality issues.
///
/// Checks for:
/// - All-zero packets (sensor feed failure)
/// - Missing critical fields (bit_depth, flow_in)
/// - Physically impossible values (negative depth, ROP > 1000 ft/hr, etc.)
/// - Stale timestamps
/// - Inconsistent values (flow_out without flow_in, etc.)
pub fn validate_packet_quality(packet: &WitsPacket) -> DataQualityReport {
    let mut issues = Vec::new();
    let mut zero_critical = 0usize;
    let mut impossible = 0usize;

    // ---- All-zero packet detection ----
    let critical_sum = packet.bit_depth.abs()
        + packet.rop.abs()
        + packet.wob.abs()
        + packet.rpm.abs()
        + packet.torque.abs()
        + packet.spp.abs()
        + packet.flow_in.abs();

    if critical_sum < f64::EPSILON {
        issues.push(DataQualityIssue {
            field: "ALL".to_string(),
            severity: QualitySeverity::Critical,
            message: "All critical fields are zero — sensor feed failure".to_string(),
        });
        return DataQualityReport {
            usable: false,
            issues,
            zero_critical_fields: 7,
            impossible_values: 0,
        };
    }

    // ---- Zero-value critical field checks ----
    let critical_fields: &[(&str, f64)] = &[
        ("bit_depth", packet.bit_depth),
        ("flow_in", packet.flow_in),
    ];
    for &(name, value) in critical_fields {
        if value.abs() < f64::EPSILON {
            zero_critical += 1;
            issues.push(DataQualityIssue {
                field: name.to_string(),
                severity: QualitySeverity::Warning,
                message: format!("{} is zero — check sensor connection", name),
            });
        }
    }

    // ---- Physically impossible values ----
    if packet.bit_depth < 0.0 {
        impossible += 1;
        issues.push(DataQualityIssue {
            field: "bit_depth".to_string(),
            severity: QualitySeverity::Critical,
            message: format!("Negative bit depth: {:.1} ft", packet.bit_depth),
        });
    }
    if packet.rop < -1.0 {
        impossible += 1;
        issues.push(DataQualityIssue {
            field: "rop".to_string(),
            severity: QualitySeverity::Critical,
            message: format!("Negative ROP: {:.1} ft/hr", packet.rop),
        });
    }
    if packet.rop > 1000.0 {
        impossible += 1;
        issues.push(DataQualityIssue {
            field: "rop".to_string(),
            severity: QualitySeverity::Critical,
            message: format!("ROP > 1000 ft/hr: {:.1} — sensor spike", packet.rop),
        });
    }
    if packet.wob > 200.0 {
        impossible += 1;
        issues.push(DataQualityIssue {
            field: "wob".to_string(),
            severity: QualitySeverity::Critical,
            message: format!("WOB > 200 klbs: {:.1} — exceeds rig capacity", packet.wob),
        });
    }
    if packet.rpm < 0.0 {
        impossible += 1;
        issues.push(DataQualityIssue {
            field: "rpm".to_string(),
            severity: QualitySeverity::Warning,
            message: format!("Negative RPM: {:.1}", packet.rpm),
        });
    }
    if packet.rpm > 500.0 {
        impossible += 1;
        issues.push(DataQualityIssue {
            field: "rpm".to_string(),
            severity: QualitySeverity::Critical,
            message: format!("RPM > 500: {:.1} — sensor spike", packet.rpm),
        });
    }
    if packet.spp < 0.0 {
        impossible += 1;
        issues.push(DataQualityIssue {
            field: "spp".to_string(),
            severity: QualitySeverity::Warning,
            message: format!("Negative SPP: {:.1} psi", packet.spp),
        });
    }
    if packet.spp > 10000.0 {
        impossible += 1;
        issues.push(DataQualityIssue {
            field: "spp".to_string(),
            severity: QualitySeverity::Critical,
            message: format!("SPP > 10000 psi: {:.1}", packet.spp),
        });
    }
    if packet.mud_weight_in != 0.0 && (packet.mud_weight_in < 0.0 || packet.mud_weight_in > 25.0) {
        impossible += 1;
        issues.push(DataQualityIssue {
            field: "mud_weight_in".to_string(),
            severity: QualitySeverity::Critical,
            message: format!("Mud weight out of range: {:.2} ppg (valid: 0-25)", packet.mud_weight_in),
        });
    }
    if packet.h2s < 0.0 {
        impossible += 1;
        issues.push(DataQualityIssue {
            field: "h2s".to_string(),
            severity: QualitySeverity::Warning,
            message: format!("Negative H2S: {:.1} ppm", packet.h2s),
        });
    }

    // ---- Consistency checks ----
    if packet.flow_out > 0.0 && packet.flow_in < f64::EPSILON {
        issues.push(DataQualityIssue {
            field: "flow_in".to_string(),
            severity: QualitySeverity::Warning,
            message: format!("Flow out ({:.0} gpm) without flow in", packet.flow_out),
        });
    }
    if packet.bit_depth > 0.0 && packet.hole_depth > 0.0 && packet.bit_depth > packet.hole_depth + 10.0 {
        issues.push(DataQualityIssue {
            field: "bit_depth".to_string(),
            severity: QualitySeverity::Warning,
            message: format!(
                "Bit depth ({:.0}) exceeds hole depth ({:.0}) by >10 ft",
                packet.bit_depth, packet.hole_depth
            ),
        });
    }
    if packet.timestamp == 0 {
        issues.push(DataQualityIssue {
            field: "timestamp".to_string(),
            severity: QualitySeverity::Warning,
            message: "Timestamp is zero — clock not synchronized".to_string(),
        });
    }

    let has_critical = issues.iter().any(|i| i.severity == QualitySeverity::Critical);

    DataQualityReport {
        usable: !has_critical,
        issues,
        zero_critical_fields: zero_critical,
        impossible_values: impossible,
    }
}

/// Parse WITS JSON format (for testing with wits_simulator.py)
pub fn parse_wits_json(json_str: &str) -> Result<WitsPacket> {
    let packet: WitsPacket = serde_json::from_str(json_str)
        .context("Failed to parse WITS JSON")?;
    Ok(packet)
}

/// WITS Level 0 frame builder (for testing/simulation)
pub struct WitsFrameBuilder {
    items: Vec<(String, String)>,
}

impl WitsFrameBuilder {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn add_item(&mut self, code: &str, value: f64) -> &mut Self {
        self.items.push((code.to_string(), format!("{:.2}", value)));
        self
    }

    pub fn build(&self) -> String {
        let mut frame = String::from("&&\r\n");
        for (code, value) in &self.items {
            frame.push_str(&format!("{}{}\r\n", code, value));
        }
        frame.push_str("!!\r\n");
        frame
    }
}

impl Default for WitsFrameBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wits_frame_parsing() {
        let mut items = HashMap::new();
        items.insert(wits_items::BIT_DEPTH.to_string(), 10500.0);
        items.insert(wits_items::ROP.to_string(), 45.5);
        items.insert(wits_items::WOB.to_string(), 25.0);
        items.insert(wits_items::RPM.to_string(), 120.0);
        items.insert(wits_items::TORQUE.to_string(), 15.5);
        items.insert(wits_items::SPP.to_string(), 2800.0);
        items.insert(wits_items::FLOW_IN.to_string(), 500.0);
        items.insert(wits_items::FLOW_OUT.to_string(), 505.0);

        let packet = WitsClient::items_to_packet(&items);

        assert_eq!(packet.bit_depth, 10500.0);
        assert_eq!(packet.rop, 45.5);
        assert_eq!(packet.wob, 25.0);
        assert_eq!(packet.rpm, 120.0);
        assert_eq!(packet.torque, 15.5);
        assert_eq!(packet.spp, 2800.0);
        assert_eq!(packet.rig_state, RigState::Drilling);
    }

    #[test]
    fn test_rig_state_classification() {
        // Drilling
        assert_eq!(classify_rig_state(120.0, 25.0, 200.0, 50.0, 50.0), RigState::Drilling);

        // Reaming
        assert_eq!(classify_rig_state(80.0, 10.0, 180.0, 0.0, 60.0), RigState::Reaming);

        // Circulating
        assert_eq!(classify_rig_state(50.0, 2.0, 150.0, 0.0, 50.0), RigState::Circulating);

        // Idle
        assert_eq!(classify_rig_state(0.0, 0.0, 30.0, 0.0, 0.0), RigState::Idle);
    }

    #[test]
    fn test_wits_frame_builder() {
        let frame = WitsFrameBuilder::new()
            .add_item(wits_items::BIT_DEPTH, 10000.0)
            .add_item(wits_items::ROP, 50.0)
            .add_item(wits_items::RPM, 120.0)
            .build();

        assert!(frame.starts_with("&&\r\n"));
        assert!(frame.ends_with("!!\r\n"));
        assert!(frame.contains("010810000.00"));
        assert!(frame.contains("011350.00"));
        assert!(frame.contains("0117120.00"));
    }

    #[test]
    fn test_wits_json_parsing() {
        let json = r#"{
            "timestamp": 1705564800,
            "bit_depth": 10000.0,
            "hole_depth": 10050.0,
            "rop": 50.0,
            "hook_load": 200.0,
            "wob": 25.0,
            "rpm": 120.0,
            "torque": 15.0,
            "bit_diameter": 8.5,
            "spp": 2800.0,
            "pump_spm": 120.0,
            "flow_in": 500.0,
            "flow_out": 505.0,
            "pit_volume": 500.0,
            "mud_weight_in": 12.0,
            "mud_weight_out": 12.1,
            "mud_temp_in": 100.0,
            "mud_temp_out": 120.0,
            "ecd": 12.4,
            "casing_pressure": 0.0,
            "gas_units": 50.0,
            "total_gas": 0.5,
            "h2s": 0.0,
            "co2": 0.1,
            "mse": 35000.0,
            "d_exponent": 1.5,
            "rop_delta": 0.0,
            "rig_state": "Drilling"
        }"#;

        let packet = parse_wits_json(json).unwrap();
        assert_eq!(packet.bit_depth, 10000.0);
        assert_eq!(packet.rop, 50.0);
        assert_eq!(packet.rig_state, RigState::Drilling);
    }
}
