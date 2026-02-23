//! Sensor data ingestion from CSV files (Legacy TDS + WITS support)

use crate::types::{RigState, WitsPacket};
use chrono::{DateTime, Utc};
use std::fs::File;
use std::io::{BufRead, BufReader};

/// Read WITS sensor data from a CSV file
///
/// Expected CSV format:
/// timestamp,bit_depth,hole_depth,rop,hook_load,wob,rpm,torque,spp,pump_spm,flow_in,flow_out,pit_volume,mud_weight_in,mud_weight_out,ecd,gas_units
pub fn read_csv_data(path: &str) -> Vec<WitsPacket> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            tracing::error!(path = %path, error = %e, "Failed to open CSV file");
            return Vec::new();
        }
    };

    let reader = BufReader::new(file);
    let mut packets = Vec::new();
    let mut line_num = 0;

    for line_result in reader.lines() {
        line_num += 1;

        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(line = line_num, error = %e, "Error reading CSV line");
                continue;
            }
        };

        // Skip header line
        if line_num == 1 && line.starts_with("timestamp") {
            continue;
        }

        // Skip empty lines
        if line.trim().is_empty() {
            continue;
        }

        match parse_csv_line(&line, line_num) {
            Ok(packet) => packets.push(packet),
            Err(e) => {
                tracing::warn!(line = line_num, error = %e, "Error parsing CSV line");
                continue;
            }
        }
    }

    tracing::info!(count = packets.len(), path = %path, "Loaded WITS packets from CSV");
    packets
}

/// Parse a single CSV line into a WitsPacket
fn parse_csv_line(line: &str, line_num: usize) -> Result<WitsPacket, String> {
    let fields: Vec<&str> = line.split(',').collect();

    if fields.len() < 17 {
        return Err(format!(
            "Expected at least 17 fields, got {} on line {}",
            fields.len(),
            line_num
        ));
    }

    // Parse timestamp (ISO 8601 format or Unix epoch)
    let timestamp = parse_timestamp(fields[0])?;

    // Parse drilling parameters
    let bit_depth = parse_f64(fields[1], "bit_depth")?;
    let hole_depth = parse_f64(fields[2], "hole_depth")?;
    let rop = parse_f64(fields[3], "rop")?;
    let hook_load = parse_f64(fields[4], "hook_load")?;
    let wob = parse_f64(fields[5], "wob")?;
    let rpm = parse_f64(fields[6], "rpm")?;
    let torque = parse_f64(fields[7], "torque")?;

    // Parse hydraulics
    let spp = parse_f64(fields[8], "spp")?;
    let pump_spm = parse_f64(fields[9], "pump_spm")?;
    let flow_in = parse_f64(fields[10], "flow_in")?;
    let flow_out = parse_f64(fields[11], "flow_out")?;
    let pit_volume = parse_f64(fields[12], "pit_volume")?;

    // Parse mud properties
    let mud_weight_in = parse_f64(fields[13], "mud_weight_in")?;
    let mud_weight_out = parse_f64(fields[14], "mud_weight_out")?;
    let ecd = parse_f64(fields[15], "ecd")?;
    let gas_units = parse_f64(fields[16], "gas_units")?;

    // Optional fields with defaults
    let mud_temp_in = if fields.len() > 17 { parse_f64(fields[17], "mud_temp_in").unwrap_or(100.0) } else { 100.0 };
    let mud_temp_out = if fields.len() > 18 { parse_f64(fields[18], "mud_temp_out").unwrap_or(120.0) } else { 120.0 };
    let casing_pressure = if fields.len() > 19 { parse_f64(fields[19], "casing_pressure").unwrap_or(0.0) } else { 0.0 };
    let h2s = if fields.len() > 20 { parse_f64(fields[20], "h2s").unwrap_or(0.0) } else { 0.0 };
    let co2 = if fields.len() > 21 { parse_f64(fields[21], "co2").unwrap_or(0.0) } else { 0.0 };

    // Classify rig state based on parameters
    let rig_state = classify_rig_state(rpm, wob, hook_load, rop);

    Ok(WitsPacket {
        timestamp,
        bit_depth,
        hole_depth,
        rop,
        hook_load,
        wob,
        rpm,
        torque,
        bit_diameter: 8.5, // Default
        spp,
        pump_spm,
        flow_in,
        flow_out,
        pit_volume,
        pit_volume_change: 0.0,
        mud_weight_in,
        mud_weight_out,
        ecd,
        mud_temp_in,
        mud_temp_out,
        gas_units,
        background_gas: 0.0,
        connection_gas: 0.0,
        h2s,
        co2,
        casing_pressure,
        annular_pressure: 0.0,
        pore_pressure: 0.0,
        fracture_gradient: 0.0,
        mse: 0.0, // Calculated later
        d_exponent: 0.0, // Calculated later
        dxc: 0.0,
        rop_delta: 0.0,
        torque_delta_percent: 0.0,
        spp_delta: 0.0,
        rig_state,
        regime_id: 0,
        seconds_since_param_change: 0,    })
}

/// Classify rig state from drilling parameters
fn classify_rig_state(rpm: f64, wob: f64, hook_load: f64, rop: f64) -> RigState {
    if rpm > 20.0 && wob > 5.0 && rop > 0.0 {
        RigState::Drilling
    } else if rpm > 20.0 && wob > 2.0 {
        RigState::Reaming
    } else if rpm > 0.0 && wob < 5.0 {
        RigState::Circulating
    } else if rpm < 5.0 && hook_load > 50.0 && hook_load < 200.0 {
        RigState::Connection
    } else {
        RigState::Idle
    }
}

/// Parse ISO 8601 timestamp to Unix epoch (seconds)
fn parse_timestamp(s: &str) -> Result<u64, String> {
    let s = s.trim();

    // Try direct numeric parsing first (already epoch)
    if let Ok(epoch) = s.parse::<u64>() {
        return Ok(epoch);
    }

    // Parse ISO 8601 using chrono
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc).timestamp().max(0) as u64)
        .or_else(|_| {
            format!("{}Z", s.trim_end_matches('Z'))
                .parse::<DateTime<Utc>>()
                .map(|dt| dt.timestamp().max(0) as u64)
                .map_err(|e| format!("Cannot parse timestamp '{}': {}", s, e))
        })
}

/// Parse a string to f64 with field name for error messages
fn parse_f64(s: &str, field: &str) -> Result<f64, String> {
    s.trim()
        .parse::<f64>()
        .map_err(|_| format!("Cannot parse {} as f64: '{}'", field, s))
}

/// Generate synthetic drilling test data
///
/// Creates drilling scenarios with normal operation, MSE inefficiency,
/// well control events, and formation changes to test the tactical agent.
pub fn generate_fault_test_data() -> Vec<WitsPacket> {
    let mut packets = Vec::new();
    let base_timestamp = 1705564800u64;

    // Phase 1: Normal drilling (40 samples)
    for i in 0..40 {
        packets.push(WitsPacket {
            timestamp: base_timestamp + i * 60,
            bit_depth: 10000.0 + i as f64 * 0.8,
            hole_depth: 10050.0,
            rop: 50.0 + (i as f64 * 0.1).sin() * 5.0,
            hook_load: 200.0,
            wob: 25.0 + (i as f64 * 0.05).sin() * 2.0,
            rpm: 120.0,
            torque: 15.0,
            bit_diameter: 8.5,
            spp: 2800.0,
            pump_spm: 120.0,
            flow_in: 500.0,
            flow_out: 502.0,
            pit_volume: 500.0,
            pit_volume_change: 0.0,
            mud_weight_in: 12.0,
            mud_weight_out: 12.1,
            ecd: 12.4,
            mud_temp_in: 100.0,
            mud_temp_out: 120.0,
            gas_units: 50.0,
            background_gas: 45.0,
            connection_gas: 5.0,
            h2s: 0.0,
            co2: 0.1,
            casing_pressure: 0.0,
            annular_pressure: 0.0,
            pore_pressure: 10.5,
            fracture_gradient: 14.0,
            mse: 35000.0,
            d_exponent: 1.5,
            dxc: 1.45,
            rop_delta: 0.0,
            torque_delta_percent: 0.0,
            spp_delta: 0.0,
            rig_state: RigState::Drilling,
            regime_id: 0,
            seconds_since_param_change: 0,        });
    }

    // Phase 2: MSE inefficiency - poor drilling (20 samples)
    for i in 0..20 {
        packets.push(WitsPacket {
            timestamp: base_timestamp + (40 + i) * 60,
            bit_depth: 10032.0 + i as f64 * 0.3,
            hole_depth: 10080.0,
            rop: 25.0 + (i as f64 * 0.1).sin() * 3.0,
            hook_load: 210.0,
            wob: 30.0,
            rpm: 100.0,
            torque: 18.0,
            bit_diameter: 8.5,
            spp: 2900.0,
            pump_spm: 120.0,
            flow_in: 500.0,
            flow_out: 502.0,
            pit_volume: 500.0,
            pit_volume_change: 0.0,
            mud_weight_in: 12.0,
            mud_weight_out: 12.1,
            ecd: 12.5,
            mud_temp_in: 100.0,
            mud_temp_out: 125.0,
            gas_units: 60.0,
            background_gas: 55.0,
            connection_gas: 5.0,
            h2s: 0.0,
            co2: 0.1,
            casing_pressure: 0.0,
            annular_pressure: 0.0,
            pore_pressure: 10.5,
            fracture_gradient: 14.0,
            mse: 55000.0,
            d_exponent: 1.6,
            dxc: 1.55,
            rop_delta: -25.0,
            torque_delta_percent: 20.0,
            spp_delta: 100.0,
            rig_state: RigState::Drilling,
            regime_id: 0,
            seconds_since_param_change: 0,        });
    }

    // Phase 3: Well control event - kick (15 samples)
    for i in 0..15 {
        let flow_gain = 5.0 + i as f64 * 2.0;
        packets.push(WitsPacket {
            timestamp: base_timestamp + (60 + i) * 60,
            bit_depth: 10038.0 + i as f64 * 0.5,
            hole_depth: 10100.0,
            rop: 30.0,
            hook_load: 190.0,
            wob: 20.0,
            rpm: 120.0,
            torque: 14.0,
            bit_diameter: 8.5,
            spp: 2700.0,
            pump_spm: 120.0,
            flow_in: 500.0,
            flow_out: 500.0 + flow_gain * 5.0,
            pit_volume: 500.0 + flow_gain,
            pit_volume_change: flow_gain,
            mud_weight_in: 12.0,
            mud_weight_out: 11.8,
            ecd: 12.2,
            mud_temp_in: 100.0,
            mud_temp_out: 130.0,
            gas_units: 100.0 + i as f64 * 50.0,
            background_gas: 80.0 + i as f64 * 40.0,
            connection_gas: 20.0 + i as f64 * 10.0,
            h2s: 0.0,
            co2: 0.2,
            casing_pressure: 50.0 + i as f64 * 10.0,
            annular_pressure: 30.0 + i as f64 * 5.0,
            pore_pressure: 10.8,
            fracture_gradient: 14.0,
            mse: 38000.0,
            d_exponent: 1.4,
            dxc: 1.35,
            rop_delta: 5.0,
            torque_delta_percent: -7.0,
            spp_delta: -100.0,
            rig_state: RigState::Drilling,
            regime_id: 0,
            seconds_since_param_change: 0,        });
    }

    // Phase 4: Return to normal (10 samples)
    for i in 0..10 {
        packets.push(WitsPacket {
            timestamp: base_timestamp + (75 + i) * 60,
            bit_depth: 10045.0 + i as f64 * 0.8,
            hole_depth: 10120.0,
            rop: 48.0,
            hook_load: 200.0,
            wob: 25.0,
            rpm: 120.0,
            torque: 15.0,
            bit_diameter: 8.5,
            spp: 2800.0,
            pump_spm: 120.0,
            flow_in: 500.0,
            flow_out: 501.0,
            pit_volume: 500.0,
            pit_volume_change: 0.0,
            mud_weight_in: 12.5,
            mud_weight_out: 12.6,
            ecd: 12.8,
            mud_temp_in: 100.0,
            mud_temp_out: 120.0,
            gas_units: 40.0,
            background_gas: 35.0,
            connection_gas: 5.0,
            h2s: 0.0,
            co2: 0.1,
            casing_pressure: 0.0,
            annular_pressure: 0.0,
            pore_pressure: 10.5,
            fracture_gradient: 14.5,
            mse: 36000.0,
            d_exponent: 1.5,
            dxc: 1.45,
            rop_delta: 0.0,
            torque_delta_percent: 0.0,
            spp_delta: 0.0,
            rig_state: RigState::Drilling,
            regime_id: 0,
            seconds_since_param_change: 0,        });
    }

    tracing::debug!(count = packets.len(), "Generated synthetic drilling test packets");
    packets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_timestamp_iso8601() {
        let ts = parse_timestamp("2025-01-18T08:00:00Z").unwrap();
        assert_eq!(ts, 1737187200);
    }

    #[test]
    fn test_parse_timestamp_epoch() {
        let ts = parse_timestamp("1705564800").unwrap();
        assert_eq!(ts, 1705564800);
    }

    #[test]
    fn test_parse_f64() {
        assert_eq!(parse_f64("1.234", "test").unwrap(), 1.234);
        assert!(parse_f64("invalid", "test").is_err());
    }

    #[test]
    fn test_generate_fault_data() {
        let data = generate_fault_test_data();
        assert_eq!(data.len(), 85);

        // Check normal drilling phase
        assert!(data[0].mse < 40000.0);
        assert!(data[0].rop > 40.0);

        // Check MSE inefficiency phase
        assert!(data[50].mse > 50000.0);

        // Check well control phase
        assert!(data[65].gas_units > 200.0);
        assert!(data[65].flow_out > 520.0);
    }

    #[test]
    fn test_rig_state_classification() {
        assert_eq!(classify_rig_state(120.0, 25.0, 200.0, 50.0), RigState::Drilling);
        assert_eq!(classify_rig_state(80.0, 10.0, 180.0, 0.0), RigState::Reaming);
        assert_eq!(classify_rig_state(0.0, 0.0, 100.0, 0.0), RigState::Connection);
    }
}
