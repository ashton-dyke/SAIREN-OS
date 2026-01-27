//! Simplified sensor implementation - Python data generator only

use super::{AcquisitionError, SensorReading, SensorSource, SensorType};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::path::PathBuf;
use std::process::Stdio;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

// ============================================================================
// Error Types
// ============================================================================

#[derive(Error, Debug)]
pub enum PythonSensorError {
    #[error("Failed to spawn Python process: {0}")]
    SpawnError(String),

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    ParseError(String),
}

impl From<PythonSensorError> for AcquisitionError {
    fn from(err: PythonSensorError) -> Self {
        match err {
            PythonSensorError::SpawnError(e) | PythonSensorError::ParseError(e) => {
                AcquisitionError::ConnectionFailed(e)
            }
            PythonSensorError::IoError(e) => {
                AcquisitionError::ConnectionFailed(format!("I/O error: {}", e))
            }
        }
    }
}

// ============================================================================
// Python Sensor Source
// ============================================================================

/// Python-based sensor source using generate_test_data.py
///
/// Spawns a Python subprocess that generates synthetic bearing fault data.
/// Supports multiple scenarios: healthy, demo, bpfo_fault, bpfi_fault,
/// progressive_failure_long, etc.
pub struct PythonSensorSource {
    scenario: String,
    rpm: f64,
    hours: Option<f64>,  // Duration in hours for progressive_failure_long
    python_exe: String,
    script_path: PathBuf,
    child: Option<Child>,
    stdout_reader: Option<BufReader<tokio::process::ChildStdout>>,
    connected: bool,
}

impl PythonSensorSource {
    /// Create a new Python sensor source with the given scenario
    pub fn new(scenario: &str) -> Result<Self, PythonSensorError> {
        // Check for environment variable first
        let python_exe = if let Ok(path) = std::env::var("TDS_PYTHON") {
            path
        } else {
            // Try python3 first (Linux/Mac), then python (Windows)
            "python3".to_string()
        };

        // Get hours from environment (for progressive_failure_long scenario)
        let hours = std::env::var("TDS_HOURS")
            .ok()
            .and_then(|s| s.parse::<f64>().ok());

        // Get RPM from environment or use default for progressive_failure_long
        let rpm = std::env::var("TDS_RPM")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or_else(|| {
                if scenario == "progressive_failure_long" { 225.0 } else { 250.0 }
            });

        tracing::info!(python = %python_exe, "Using Python executable");
        Self::with_config(
            scenario,
            rpm,
            hours,
            &python_exe,
            "scripts/generate_test_data.py",
        )
    }

    /// Create with custom configuration
    pub fn with_config(
        scenario: &str,
        rpm: f64,
        hours: Option<f64>,
        python_exe: &str,
        script_path: &str,
    ) -> Result<Self, PythonSensorError> {
        Ok(Self {
            scenario: scenario.to_string(),
            rpm,
            hours,
            python_exe: python_exe.to_string(),
            script_path: PathBuf::from(script_path),
            child: None,
            stdout_reader: None,
            connected: false,
        })
    }

    /// Parse CSV line from Python stdout into sensor readings
    fn parse_csv_line(&self, line: &str) -> Result<Vec<SensorReading>, PythonSensorError> {
        // Skip header and empty lines
        if line.starts_with("timestamp") || line.trim().is_empty() {
            return Ok(Vec::new());
        }

        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 13 {
            return Err(PythonSensorError::ParseError(format!(
                "Expected 13 columns, got {}",
                parts.len()
            )));
        }

        // Parse timestamp
        let timestamp: DateTime<Utc> = parts[0]
            .trim()
            .parse()
            .map_err(|e| PythonSensorError::ParseError(format!("Invalid timestamp: {}", e)))?;

        // Helper to parse f64 values
        let parse_f64 = |s: &str, name: &str| -> Result<f64, PythonSensorError> {
            s.trim()
                .parse()
                .map_err(|e| PythonSensorError::ParseError(format!("Invalid {}: {}", name, e)))
        };

        // Parse all values
        let vib_ch1 = parse_f64(parts[1], "vib_ch1")?;
        let vib_ch2 = parse_f64(parts[2], "vib_ch2")?;
        let vib_ch3 = parse_f64(parts[3], "vib_ch3")?;
        let vib_ch4 = parse_f64(parts[4], "vib_ch4")?;
        let motor_temp_1 = parse_f64(parts[5], "motor_temp_1")?;
        let motor_temp_2 = parse_f64(parts[6], "motor_temp_2")?;
        let motor_temp_3 = parse_f64(parts[7], "motor_temp_3")?;
        let motor_temp_4 = parse_f64(parts[8], "motor_temp_4")?;
        let gearbox_temp_1 = parse_f64(parts[9], "gearbox_temp_1")?;
        let gearbox_temp_2 = parse_f64(parts[10], "gearbox_temp_2")?;
        let _torque = parse_f64(parts[11], "torque")?;
        let rpm = parse_f64(parts[12], "rpm")?;

        let quality = 1.0_f32;

        Ok(vec![
            SensorReading {
                sensor_id: "VIB-CH1".to_string(),
                timestamp,
                sensor_type: SensorType::VibrationX,
                value: vib_ch1,
                unit: "g".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "VIB-CH2".to_string(),
                timestamp,
                sensor_type: SensorType::VibrationY,
                value: vib_ch2,
                unit: "g".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "VIB-CH3".to_string(),
                timestamp,
                sensor_type: SensorType::VibrationZ,
                value: vib_ch3,
                unit: "g".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "VIB-CH4".to_string(),
                timestamp,
                sensor_type: SensorType::VibrationX,
                value: vib_ch4,
                unit: "g".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "RPM-MAIN".to_string(),
                timestamp,
                sensor_type: SensorType::Rpm,
                value: rpm,
                unit: "RPM".to_string(),
                quality,
            },
            // Motor temperature sensors
            SensorReading {
                sensor_id: "MOTOR-TEMP-1".to_string(),
                timestamp,
                sensor_type: SensorType::Temperature,
                value: motor_temp_1,
                unit: "°C".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "MOTOR-TEMP-2".to_string(),
                timestamp,
                sensor_type: SensorType::Temperature,
                value: motor_temp_2,
                unit: "°C".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "MOTOR-TEMP-3".to_string(),
                timestamp,
                sensor_type: SensorType::Temperature,
                value: motor_temp_3,
                unit: "°C".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "MOTOR-TEMP-4".to_string(),
                timestamp,
                sensor_type: SensorType::Temperature,
                value: motor_temp_4,
                unit: "°C".to_string(),
                quality,
            },
            // Gearbox temperature sensors
            SensorReading {
                sensor_id: "GEARBOX-TEMP-1".to_string(),
                timestamp,
                sensor_type: SensorType::Temperature,
                value: gearbox_temp_1,
                unit: "°C".to_string(),
                quality,
            },
            SensorReading {
                sensor_id: "GEARBOX-TEMP-2".to_string(),
                timestamp,
                sensor_type: SensorType::Temperature,
                value: gearbox_temp_2,
                unit: "°C".to_string(),
                quality,
            },
        ])
    }
}

#[async_trait]
impl SensorSource for PythonSensorSource {
    async fn connect(&mut self) -> Result<(), AcquisitionError> {
        if self.connected {
            return Ok(());
        }

        tracing::info!(
            scenario = %self.scenario,
            rpm = self.rpm,
            hours = ?self.hours,
            python = %self.python_exe,
            "Starting Python sensor source"
        );

        // Build command with base arguments
        let mut cmd = Command::new(&self.python_exe);
        cmd.arg(&self.script_path)
            .arg("--live")
            .arg("--scenario")
            .arg(&self.scenario)
            .arg("--rpm")
            .arg(self.rpm.to_string());

        // Add --hours argument for progressive_failure_long scenario
        if self.scenario == "progressive_failure_long" {
            let hours = self.hours.unwrap_or(10.0);
            cmd.arg("--hours").arg(hours.to_string());
            tracing::info!(hours = hours, "Progressive failure test configured");
        }

        // Spawn Python process
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                PythonSensorError::SpawnError(format!(
                    "Failed to spawn '{}': {}. Is Python installed?",
                    self.python_exe, e
                ))
            })?;

        // Get stdout handle
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| PythonSensorError::SpawnError("Failed to capture stdout".to_string()))?;

        self.stdout_reader = Some(BufReader::new(stdout));
        self.child = Some(child);
        self.connected = true;

        tracing::info!("Python sensor source connected");
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), AcquisitionError> {
        if !self.connected {
            return Ok(());
        }

        tracing::info!("Disconnecting Python sensor source");

        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
        }

        self.stdout_reader = None;
        self.connected = false;
        Ok(())
    }

    async fn read(&mut self) -> Result<Vec<SensorReading>, AcquisitionError> {
        if !self.connected {
            return Err(AcquisitionError::ConnectionFailed(
                "Not connected".to_string(),
            ));
        }

        let reader = self
            .stdout_reader
            .as_mut()
            .ok_or_else(|| AcquisitionError::ConnectionFailed("No stdout reader".to_string()))?;

        // Read a line from Python stdout
        let mut line = String::new();
        let bytes_read = reader
            .read_line(&mut line)
            .await
            .map_err(PythonSensorError::IoError)?;

        if bytes_read == 0 {
            // EOF - process terminated
            self.connected = false;
            return Err(AcquisitionError::ConnectionFailed(
                "Python process terminated".to_string(),
            ));
        }

        // Parse the line into readings
        self.parse_csv_line(&line).map_err(AcquisitionError::from)
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}
