//! Stdin Sensor Source
//!
//! Reads JSON-formatted WitsPacket data from stdin for integration testing.
//! Used with the simulation harness: `python wits_simulator.py | ./sairen-os --stdin`

use super::{AcquisitionError, SensorReading, SensorSource, SensorType};
use crate::types::{RigState, WitsPacket};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, BufReader, Stdin};

/// JSON structure for WITS packet from stdin
/// Matches output from wits_simulator.py
#[derive(Debug, Deserialize)]
struct JsonWitsPacket {
    timestamp: u64,
    // Drilling parameters
    bit_depth: f64,
    hole_depth: f64,
    rop: f64,
    hook_load: f64,
    wob: f64,
    rpm: f64,
    torque: f64,
    // Hydraulics
    spp: f64,
    pump_spm: f64,
    flow_in: f64,
    flow_out: f64,
    pit_volume: f64,
    // Mud properties
    mud_weight_in: f64,
    mud_weight_out: f64,
    mud_temp_in: f64,
    mud_temp_out: f64,
    ecd: f64,
    // Well control / Gas
    #[serde(default)]
    casing_pressure: f64,
    #[serde(default)]
    gas_units: f64,
    #[serde(default)]
    total_gas: f64,
    #[serde(default)]
    h2s: f64,
    #[serde(default)]
    co2: f64,
    // Derived (may be pre-calculated)
    #[serde(default)]
    mse: f64,
    #[serde(default)]
    d_exponent: f64,
    #[serde(default)]
    rop_delta: f64,
    // State
    #[serde(default)]
    rig_state: Option<String>,
}

/// Legacy JSON structure for backward compatibility with TDS simulation
#[derive(Debug, Deserialize)]
struct LegacyJsonSensorPacket {
    timestamp: u64,
    vib_ch1: f64,
    vib_ch2: f64,
    vib_ch3: f64,
    vib_ch4: f64,
    motor_temp1: f64,
    motor_temp2: f64,
    motor_temp3: f64,
    motor_temp4: f64,
    gearbox_temp1: f64,
    gearbox_temp2: f64,
    hookload: f64,
    rpm: f64,
    #[serde(default)]
    flow_rate: f64,
}

/// Sensor source that reads JSON packets from stdin
pub struct StdinSensorSource {
    reader: Option<BufReader<Stdin>>,
    connected: bool,
    line_buffer: String,
    /// Last parsed WITS packet (for higher-level consumers)
    last_wits_packet: Option<WitsPacket>,
}

impl StdinSensorSource {
    /// Create a new stdin sensor source
    pub fn new() -> Self {
        Self {
            reader: None,
            connected: false,
            line_buffer: String::with_capacity(2048),
            last_wits_packet: None,
        }
    }

    /// Get the last parsed WITS packet
    pub fn last_packet(&self) -> Option<&WitsPacket> {
        self.last_wits_packet.as_ref()
    }

    /// Parse a JSON line into sensor readings (tries WITS first, then legacy)
    fn parse_json_line(&mut self, line: &str) -> Result<Vec<SensorReading>, AcquisitionError> {
        // Skip empty lines
        if line.trim().is_empty() {
            return Ok(Vec::new());
        }

        // Try WITS JSON format first
        if let Ok(wits) = serde_json::from_str::<JsonWitsPacket>(line) {
            return self.parse_wits_packet(wits);
        }

        // Fall back to legacy TDS format
        if let Ok(legacy) = serde_json::from_str::<LegacyJsonSensorPacket>(line) {
            return self.parse_legacy_packet(legacy);
        }

        Err(AcquisitionError::ConnectionFailed(format!(
            "JSON parse error: unrecognized format"
        )))
    }

    /// Parse WITS packet into sensor readings
    fn parse_wits_packet(&mut self, packet: JsonWitsPacket) -> Result<Vec<SensorReading>, AcquisitionError> {
        let timestamp = DateTime::<Utc>::from_timestamp(packet.timestamp as i64, 0)
            .unwrap_or_else(Utc::now);
        let quality = 1.0_f32;

        // Parse rig state
        let rig_state = match packet.rig_state.as_deref() {
            Some("Drilling") => RigState::Drilling,
            Some("Reaming") => RigState::Reaming,
            Some("Circulating") => RigState::Circulating,
            Some("Connection") => RigState::Connection,
            Some("TrippingIn") => RigState::TrippingIn,
            Some("TrippingOut") => RigState::TrippingOut,
            Some("Idle") | None => RigState::Idle,
            _ => RigState::Idle,
        };

        // Store full WITS packet for higher-level consumers
        self.last_wits_packet = Some(WitsPacket {
            timestamp: packet.timestamp,
            bit_depth: packet.bit_depth,
            hole_depth: packet.hole_depth,
            rop: packet.rop,
            hook_load: packet.hook_load,
            wob: packet.wob,
            rpm: packet.rpm,
            torque: packet.torque,
            bit_diameter: 8.5, // Default
            spp: packet.spp,
            pump_spm: packet.pump_spm,
            flow_in: packet.flow_in,
            flow_out: packet.flow_out,
            pit_volume: packet.pit_volume,
            pit_volume_change: 0.0,
            mud_weight_in: packet.mud_weight_in,
            mud_weight_out: packet.mud_weight_out,
            ecd: packet.ecd,
            mud_temp_in: packet.mud_temp_in,
            mud_temp_out: packet.mud_temp_out,
            gas_units: packet.gas_units,
            background_gas: 0.0,
            connection_gas: 0.0,
            h2s: packet.h2s,
            co2: packet.co2,
            casing_pressure: packet.casing_pressure,
            annular_pressure: 0.0,
            pore_pressure: 0.0,
            fracture_gradient: 0.0,
            mse: packet.mse,
            d_exponent: packet.d_exponent,
            dxc: 0.0,
            rop_delta: packet.rop_delta,
            torque_delta_percent: 0.0,
            spp_delta: 0.0,
            rig_state,
            waveform_snapshot: std::sync::Arc::new(Vec::new()),
        });

        // Convert key parameters to SensorReadings for compatibility
        Ok(vec![
            SensorReading {
                sensor_id: "WITS-ROP".to_string(),
                timestamp,
                sensor_type: SensorType::Rpm, // Using Rpm as closest match
                value: packet.rop,
                unit: "ft/hr".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "WITS-WOB".to_string(),
                timestamp,
                sensor_type: SensorType::Torque,
                value: packet.wob,
                unit: "klbs".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "WITS-RPM".to_string(),
                timestamp,
                sensor_type: SensorType::Rpm,
                value: packet.rpm,
                unit: "RPM".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "WITS-TORQUE".to_string(),
                timestamp,
                sensor_type: SensorType::Torque,
                value: packet.torque,
                unit: "kft-lbs".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "WITS-SPP".to_string(),
                timestamp,
                sensor_type: SensorType::OilPressure,
                value: packet.spp,
                unit: "psi".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "WITS-FLOW-IN".to_string(),
                timestamp,
                sensor_type: SensorType::OilPressure,
                value: packet.flow_in,
                unit: "gpm".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "WITS-FLOW-OUT".to_string(),
                timestamp,
                sensor_type: SensorType::OilPressure,
                value: packet.flow_out,
                unit: "gpm".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "WITS-MUD-TEMP-IN".to_string(),
                timestamp,
                sensor_type: SensorType::Temperature,
                value: packet.mud_temp_in,
                unit: "°F".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "WITS-MUD-TEMP-OUT".to_string(),
                timestamp,
                sensor_type: SensorType::Temperature,
                value: packet.mud_temp_out,
                unit: "°F".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "WITS-GAS".to_string(),
                timestamp,
                sensor_type: SensorType::Temperature, // No gas type, using temp
                value: packet.gas_units,
                unit: "units".to_string(),
                quality,
            },
        ])
    }

    /// Parse legacy TDS packet into sensor readings
    fn parse_legacy_packet(&mut self, packet: LegacyJsonSensorPacket) -> Result<Vec<SensorReading>, AcquisitionError> {
        let timestamp = DateTime::<Utc>::from_timestamp(packet.timestamp as i64, 0)
            .unwrap_or_else(Utc::now);
        let quality = 1.0_f32;

        // Clear WITS packet for legacy mode
        self.last_wits_packet = None;

        Ok(vec![
            SensorReading {
                sensor_id: "VIB-CH1".to_string(),
                timestamp,
                sensor_type: SensorType::VibrationX,
                value: packet.vib_ch1,
                unit: "g".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "VIB-CH2".to_string(),
                timestamp,
                sensor_type: SensorType::VibrationY,
                value: packet.vib_ch2,
                unit: "g".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "VIB-CH3".to_string(),
                timestamp,
                sensor_type: SensorType::VibrationZ,
                value: packet.vib_ch3,
                unit: "g".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "VIB-CH4".to_string(),
                timestamp,
                sensor_type: SensorType::VibrationX,
                value: packet.vib_ch4,
                unit: "g".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "RPM-MAIN".to_string(),
                timestamp,
                sensor_type: SensorType::Rpm,
                value: packet.rpm,
                unit: "RPM".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "MOTOR-TEMP-1".to_string(),
                timestamp,
                sensor_type: SensorType::Temperature,
                value: packet.motor_temp1,
                unit: "°C".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "MOTOR-TEMP-2".to_string(),
                timestamp,
                sensor_type: SensorType::Temperature,
                value: packet.motor_temp2,
                unit: "°C".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "MOTOR-TEMP-3".to_string(),
                timestamp,
                sensor_type: SensorType::Temperature,
                value: packet.motor_temp3,
                unit: "°C".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "MOTOR-TEMP-4".to_string(),
                timestamp,
                sensor_type: SensorType::Temperature,
                value: packet.motor_temp4,
                unit: "°C".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "GEARBOX-TEMP-1".to_string(),
                timestamp,
                sensor_type: SensorType::Temperature,
                value: packet.gearbox_temp1,
                unit: "°C".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "GEARBOX-TEMP-2".to_string(),
                timestamp,
                sensor_type: SensorType::Temperature,
                value: packet.gearbox_temp2,
                unit: "°C".to_string(),
                quality,
            },
        ])
    }
}

impl Default for StdinSensorSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SensorSource for StdinSensorSource {
    async fn connect(&mut self) -> Result<(), AcquisitionError> {
        if self.connected {
            return Ok(());
        }

        tracing::info!("Connecting to stdin sensor source...");

        let stdin = tokio::io::stdin();
        self.reader = Some(BufReader::new(stdin));
        self.connected = true;

        tracing::info!("Stdin sensor source connected - waiting for WITS JSON packets");
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), AcquisitionError> {
        if !self.connected {
            return Ok(());
        }

        tracing::info!("Disconnecting stdin sensor source");
        self.reader = None;
        self.connected = false;
        self.last_wits_packet = None;
        Ok(())
    }

    async fn read(&mut self) -> Result<Vec<SensorReading>, AcquisitionError> {
        if !self.connected {
            return Err(AcquisitionError::ConnectionFailed(
                "Not connected".to_string(),
            ));
        }

        let reader = self
            .reader
            .as_mut()
            .ok_or_else(|| AcquisitionError::ConnectionFailed("No stdin reader".to_string()))?;

        // Clear and read a line
        self.line_buffer.clear();
        let bytes_read = reader
            .read_line(&mut self.line_buffer)
            .await
            .map_err(|e| AcquisitionError::ConnectionFailed(format!("Stdin read error: {}", e)))?;

        if bytes_read == 0 {
            // EOF - stdin closed
            self.connected = false;
            return Err(AcquisitionError::ConnectionFailed(
                "Stdin closed (EOF)".to_string(),
            ));
        }

        // Parse the JSON line (clone to avoid borrow conflict)
        let line = self.line_buffer.clone();
        self.parse_json_line(&line)
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wits_json_parsing() {
        let mut source = StdinSensorSource::new();

        let json = r#"{"timestamp":1705564800,"bit_depth":10000.0,"hole_depth":10050.0,"rop":50.0,"hook_load":200.0,"wob":25.0,"rpm":120.0,"torque":15.0,"spp":2800.0,"pump_spm":120.0,"flow_in":500.0,"flow_out":505.0,"pit_volume":500.0,"mud_weight_in":12.0,"mud_weight_out":12.1,"mud_temp_in":100.0,"mud_temp_out":120.0,"ecd":12.4,"gas_units":50.0,"rig_state":"Drilling"}"#;

        let readings = source.parse_json_line(json).unwrap();
        assert!(!readings.is_empty());

        let packet = source.last_packet().unwrap();
        assert_eq!(packet.rop, 50.0);
        assert_eq!(packet.wob, 25.0);
        assert_eq!(packet.rig_state, RigState::Drilling);
    }

    #[test]
    fn test_legacy_json_parsing() {
        let mut source = StdinSensorSource::new();

        let json = r#"{"timestamp":1705564800,"vib_ch1":0.1,"vib_ch2":0.2,"vib_ch3":0.15,"vib_ch4":0.12,"motor_temp1":65.0,"motor_temp2":66.0,"motor_temp3":64.0,"motor_temp4":65.5,"gearbox_temp1":55.0,"gearbox_temp2":56.0,"hookload":200.0,"rpm":120.0}"#;

        let readings = source.parse_json_line(json).unwrap();
        assert_eq!(readings.len(), 11);
        assert!(source.last_packet().is_none()); // Legacy mode clears WITS packet
    }
}
