//! Volve Field Dataset Replay Adapter
//!
//! Parses Equinor's Volve field real-time drilling data (CSV format) into
//! SAIREN-OS WitsPacket structs. Supports two CSV formats:
//!
//! **Format A — Kaggle / Drilling Contractor format:**
//! Descriptive column names with units, e.g. "Weight on Bit kkgf",
//! "Stand Pipe Pressure kPa". This is the format from the Kaggle
//! Volve well F-9A dataset.
//!
//! **Format B — Tunkiel WITSML-mnemonic format:**
//! Short WITSML mnemonics, e.g. "WOB", "PUMP", "SURF_RPM".
//! From the UiS pre-parsed CSV bundle.
//!
//! The adapter auto-detects which format is present from the header row.
//!
//! # Usage
//!
//! ```ignore
//! use sairen_os::volve::{VolveReplay, VolveConfig};
//!
//! let config = VolveConfig::default();
//! let replay = VolveReplay::load("path/to/volve_well.csv", config)?;
//!
//! for packet in replay.packets() {
//!     // Feed into physics engine, tactical agent, etc.
//! }
//! ```

use crate::types::{RigState, WitsPacket};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::Arc;

// ============================================================================
// Unit Conversion Constants
// ============================================================================

/// Metres to feet
const M_TO_FT: f64 = 3.28084;

// --- Kaggle format conversions (Format A) ---

/// Kilo-kilogram-force to kilo-pounds-force (1 kkgf = 2.20462 klbf)
const KKGF_TO_KLBF: f64 = 2.20462;
/// Kilo-Newton-metres to kilo-foot-pounds (1 kN·m = 0.73756 kft·lbs)
const KNM_TO_KFTLB: f64 = 0.737562;
/// Metres per hour to feet per hour
const MH_TO_FTHR: f64 = 3.28084;
/// Kilopascals to PSI
const KPA_TO_PSI: f64 = 0.145038;
/// Litres per minute to gallons per minute
const LMIN_TO_GPM: f64 = 0.264172;
/// Grams per cm³ to pounds per gallon (1 g/cm³ = 8.3454 ppg)
const GCM3_TO_PPG: f64 = 8.34540;

/// Convert Celsius to Fahrenheit
fn celsius_to_fahrenheit(c: f64) -> f64 {
    c * 9.0 / 5.0 + 32.0
}

// --- Tunkiel format conversions (Format B) ---

/// Newtons to kilolbs-force
const N_TO_KLBF: f64 = 1.0 / 4448.222;
/// Newton-metres to kilo-foot-pounds
const NM_TO_KFTLB: f64 = 1.0 / 1355.818;
/// Revolutions per second to RPM
const RPS_TO_RPM: f64 = 60.0;
/// Metres per second to feet per hour
const MS_TO_FTHR: f64 = 11811.024;
/// Pascals to PSI
const PA_TO_PSI: f64 = 1.0 / 6894.757;
/// Cubic metres per second to gallons per minute
const M3S_TO_GPM: f64 = 15850.32;
/// kg/m³ to pounds per gallon
const KGM3_TO_PPG: f64 = 1.0 / 119.826;

/// Convert Kelvin to Fahrenheit
fn kelvin_to_fahrenheit(k: f64) -> f64 {
    if k <= 0.0 {
        return 0.0;
    }
    (k - 273.15) * 9.0 / 5.0 + 32.0
}

// ============================================================================
// CSV Quote-Aware Parsing
// ============================================================================

/// Split a CSV line respecting quoted fields (handles commas inside quotes).
/// Returns owned strings because quoted fields need unquoting.
fn csv_split(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                if in_quotes {
                    // Check for escaped quote ("")
                    if chars.peek() == Some(&'"') {
                        current.push('"');
                        chars.next();
                    } else {
                        in_quotes = false;
                    }
                } else {
                    in_quotes = true;
                }
            }
            ',' if !in_quotes => {
                fields.push(current.clone());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    fields.push(current);
    fields
}

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for Volve replay behaviour
#[derive(Debug, Clone)]
pub struct VolveConfig {
    /// Default bit diameter in inches (Volve data doesn't include this)
    pub default_bit_diameter: f64,
    /// Well identifier (derived from filename if not set)
    pub well_id: Option<String>,
    /// Skip rows where all drilling parameters are zero/NaN
    pub skip_null_rows: bool,
    /// Replace NaN values with 0.0 instead of skipping the row
    pub nan_to_zero: bool,
}

impl Default for VolveConfig {
    fn default() -> Self {
        Self {
            default_bit_diameter: 8.5,
            well_id: None,
            skip_null_rows: true,
            nan_to_zero: true,
        }
    }
}

// ============================================================================
// CSV Format Detection
// ============================================================================

/// Detected CSV format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CsvFormat {
    /// Kaggle / drilling contractor: descriptive names with units
    Kaggle,
    /// Tunkiel WITSML mnemonics: short codes like WOB, PUMP, SURF_RPM
    Tunkiel,
}

// ============================================================================
// Column Mapping
// ============================================================================

/// Maps CSV column names to indices, handling both formats
#[derive(Debug, Clone, Default)]
struct ColumnMap {
    format: Option<CsvFormat>,

    // Timestamp / index
    time: Option<usize>,
    datetime_parsed: Option<usize>,

    // Depth
    depth: Option<usize>,
    hole_depth: Option<usize>,

    // Drilling
    wob: Option<usize>,
    torque: Option<usize>,
    rpm: Option<usize>,
    rop: Option<usize>,
    hook_load: Option<usize>,

    // Hydraulics
    spp: Option<usize>,
    flow_in: Option<usize>,
    flow_out: Option<usize>,
    pump_spm: Option<usize>,

    // Mud
    mw_in: Option<usize>,
    mw_out: Option<usize>,
    ecd: Option<usize>,
    temp_in: Option<usize>,
    temp_out: Option<usize>,

    // Well control
    gas: Option<usize>,
    dxc: Option<usize>,

    // Pit volume
    pit_volume: Option<usize>,

    // Rig state
    rig_mode: Option<usize>,
    rig_activity_code: Option<usize>,
}

impl ColumnMap {
    /// Build column map from CSV header, auto-detecting format
    fn from_header(header: &str) -> Self {
        let mut map = Self::default();
        let columns = csv_split(header);
        let col_refs: Vec<&str> = columns.iter().map(|s| s.as_str()).collect();

        // Detect format from header content
        let header_upper = header.to_uppercase();
        let is_kaggle = header_upper.contains("KKGF")
            || header_upper.contains("KPA")
            || header_upper.contains("KN.M")
            || header_upper.contains("WEIGHT ON BIT")
            || header_upper.contains("STAND PIPE PRESSURE");

        if is_kaggle {
            map.format = Some(CsvFormat::Kaggle);
            map.map_kaggle_columns(&col_refs);
        } else {
            map.format = Some(CsvFormat::Tunkiel);
            map.map_tunkiel_columns(&col_refs);
        }

        map
    }

    /// Map Kaggle-format descriptive column names
    fn map_kaggle_columns(&mut self, columns: &[&str]) {
        for (idx, col) in columns.iter().enumerate() {
            let col_trimmed = col.trim();
            let col_lower = col_trimmed.to_lowercase();

            // Time columns
            if col_lower == "datetime parsed" || col_lower == "time time" {
                self.datetime_parsed = Some(idx);
            } else if col_lower.starts_with("time s") || col_lower == "time" {
                self.time = Some(idx);
            }

            // Depth
            if col_lower.starts_with("bit depth (md)") || col_lower.starts_with("bit depth m") {
                // Prefer "Bit Depth (MD)" over plain "Bit Depth m" if both exist
                if self.depth.is_none() || col_lower.contains("(md)") {
                    self.depth = Some(idx);
                }
            }
            if col_lower.starts_with("hole depth") || col_lower.starts_with("total depth m") {
                self.hole_depth = Some(idx);
            }

            // WOB — prefer "Weight on Bit" over "Averaged WOB"
            if col_lower.starts_with("weight on bit kkgf") {
                self.wob = Some(idx);
            } else if col_lower.starts_with("averaged wob") && self.wob.is_none() {
                self.wob = Some(idx);
            }

            // Torque
            if col_lower.starts_with("average surface torque") {
                self.torque = Some(idx);
            } else if col_lower.starts_with("averaged trq") && self.torque.is_none() {
                self.torque = Some(idx);
            }

            // RPM
            if col_lower.starts_with("averaged rpm") || col_lower.starts_with("average rotary speed") {
                if self.rpm.is_none() {
                    self.rpm = Some(idx);
                }
            }

            // ROP — prefer "Rate of penetration m/h" (plain, not averaged)
            if col_lower.starts_with("rate of penetration m/h")
                || col_lower.starts_with("rate of penetration 2 minute")
            {
                if self.rop.is_none() {
                    self.rop = Some(idx);
                }
            }

            // Hookload
            if col_lower.starts_with("total hookload") {
                self.hook_load = Some(idx);
            } else if col_lower.starts_with("average hookload") && self.hook_load.is_none() {
                self.hook_load = Some(idx);
            }

            // SPP
            if col_lower.starts_with("stand pipe pressure")
                || col_lower.starts_with("average standpipe pressure")
            {
                if self.spp.is_none() {
                    self.spp = Some(idx);
                }
            }

            // Flow
            if col_lower.starts_with("mud flow in") || col_lower.starts_with("flow pumps") {
                if self.flow_in.is_none() {
                    self.flow_in = Some(idx);
                }
            }
            // Flow out not typically in Kaggle format

            // Pump SPM
            if col_lower.starts_with("total spm") {
                self.pump_spm = Some(idx);
            }

            // Mud weight
            if col_lower.starts_with("mud density in") {
                if self.mw_in.is_none() {
                    self.mw_in = Some(idx);
                }
            }
            if col_lower.starts_with("mud density out") {
                self.mw_out = Some(idx);
            }

            // ECD
            if col_lower.contains("equivalent circulating density") || col_lower.starts_with("ecd") {
                if self.ecd.is_none() {
                    self.ecd = Some(idx);
                }
            }

            // Temperature
            if col_lower.starts_with("tmp in") {
                self.temp_in = Some(idx);
            }
            if col_lower.starts_with("temperature out") {
                self.temp_out = Some(idx);
            }

            // Gas
            if col_lower.starts_with("gas (avg)") {
                self.gas = Some(idx);
            }

            // DXC (in depth file usually)
            if col_lower.starts_with("corr. drilling exponent") {
                self.dxc = Some(idx);
            }

            // Pit volume
            if col_lower.starts_with("tank volume (active)") {
                self.pit_volume = Some(idx);
            }

            // Rig mode
            if col_lower.starts_with("rig mode") && self.rig_mode.is_none() {
                self.rig_mode = Some(idx);
            }
        }
    }

    /// Map Tunkiel WITSML-mnemonic column names
    fn map_tunkiel_columns(&mut self, columns: &[&str]) {
        for (idx, col) in columns.iter().enumerate() {
            let col_upper = col.trim().to_uppercase();
            match col_upper.as_str() {
                "TIME" | "DATETIME" | "TIMESTAMP" => self.time = Some(idx),
                "DEPTH" | "DEPTMEAS" | "DMEA" => self.depth = Some(idx),
                "WOB" | "WOBX" => self.wob = Some(idx),
                "TORQUE" | "TRQ" | "RTOR" => self.torque = Some(idx),
                "SURF_RPM" | "RPM" | "RPMX" => self.rpm = Some(idx),
                "ROP_AVG" | "ROP" | "ROPA" => self.rop = Some(idx),
                "PUMP" | "SPP" | "SPPA" | "STANDPIPE_PRESSURE" => self.spp = Some(idx),
                "FLOWIN" | "FLOW_IN" | "FLIN" => self.flow_in = Some(idx),
                "FLOWOUT" | "FLOW_OUT" | "FLOUT" => self.flow_out = Some(idx),
                "MWIN" | "MW_IN" | "MUD_WEIGHT_IN" => self.mw_in = Some(idx),
                "MWOUT" | "MW_OUT" | "MUD_WEIGHT_OUT" => self.mw_out = Some(idx),
                "ECDBIT" | "ECD" | "ECD_BIT" => self.ecd = Some(idx),
                "MTIN" | "MT_IN" | "MUD_TEMP_IN" => self.temp_in = Some(idx),
                "MTOUT" | "MT_OUT" | "MUD_TEMP_OUT" => self.temp_out = Some(idx),
                "TOTGAS" | "TOT_GAS" | "TOTAL_GAS" => self.gas = Some(idx),
                "DXC" | "DEXP" | "D_EXPONENT" => self.dxc = Some(idx),
                "STRATESUM" | "STRATE_SUM" | "SPM" => self.pump_spm = Some(idx),
                "RIGACTIVITYCODE" | "RIG_ACTIVITY_CODE" | "ACTIVITYCODE" => {
                    self.rig_activity_code = Some(idx)
                }
                "HOOKLOAD" | "HOOK_LOAD" | "HKLD" => self.hook_load = Some(idx),
                _ => {}
            }
        }
    }

    /// Check if minimum required columns are present
    fn validate(&self) -> Result<(), String> {
        if self.depth.is_none() && self.time.is_none() && self.datetime_parsed.is_none() {
            return Err("CSV must have at least a TIME, DEPTH, or DateTime column".to_string());
        }
        Ok(())
    }

    /// Report which columns were found
    fn summary(&self) -> String {
        let mut found: Vec<&str> = Vec::new();
        let mut missing: Vec<&str> = Vec::new();

        macro_rules! check_col {
            ($name:expr, $field:expr) => {
                if $field.is_some() { found.push($name); } else { missing.push($name); }
            };
        }

        check_col!("TIME", self.time.or(self.datetime_parsed));
        check_col!("DEPTH", self.depth);
        check_col!("WOB", self.wob);
        check_col!("TORQUE", self.torque);
        check_col!("RPM", self.rpm);
        check_col!("ROP", self.rop);
        check_col!("SPP", self.spp);
        check_col!("FLOW_IN", self.flow_in);
        check_col!("MW_IN", self.mw_in);
        check_col!("MW_OUT", self.mw_out);
        check_col!("ECD", self.ecd);
        check_col!("TEMP_IN", self.temp_in);
        check_col!("TEMP_OUT", self.temp_out);
        check_col!("GAS", self.gas);
        check_col!("HOOKLOAD", self.hook_load);
        check_col!("RIG_MODE", self.rig_mode.or(self.rig_activity_code));

        let fmt = match self.format {
            Some(CsvFormat::Kaggle) => "Kaggle",
            Some(CsvFormat::Tunkiel) => "Tunkiel",
            None => "Unknown",
        };

        format!(
            "[{}] Found {}/{} columns. Present: [{}]. Missing: [{}]",
            fmt,
            found.len(),
            found.len() + missing.len(),
            found.join(", "),
            missing.join(", "),
        )
    }
}

// ============================================================================
// Volve Replay
// ============================================================================

/// Metadata about a loaded Volve well
#[derive(Debug, Clone)]
pub struct VolveWellInfo {
    /// Well identifier (from filename or config)
    pub well_id: String,
    /// Source file path
    pub source_path: String,
    /// Detected CSV format
    pub format: String,
    /// Number of valid packets loaded
    pub packet_count: usize,
    /// Number of rows skipped (null/NaN)
    pub skipped_rows: usize,
    /// Number of parse errors
    pub error_rows: usize,
    /// Columns found in CSV
    pub columns_found: String,
    /// Depth range in feet (min, max)
    pub depth_range_ft: (f64, f64),
    /// Time range as Unix timestamps (first, last)
    pub time_range: (u64, u64),
}

/// Loaded Volve well ready for replay through SAIREN-OS
pub struct VolveReplay {
    packets: Vec<WitsPacket>,
    pub info: VolveWellInfo,
}

impl VolveReplay {
    /// Load a Volve CSV file and parse into WitsPackets
    pub fn load(path: impl AsRef<Path>, config: VolveConfig) -> Result<Self, String> {
        let path = path.as_ref();
        let path_str = path.display().to_string();

        let file = File::open(path)
            .map_err(|e| format!("Failed to open {}: {}", path_str, e))?;

        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        // Read header
        let header_line = lines
            .next()
            .ok_or_else(|| format!("Empty file: {}", path_str))?
            .map_err(|e| format!("Failed to read header: {}", e))?;

        let col_map = ColumnMap::from_header(&header_line);
        col_map.validate()?;

        let columns_summary = col_map.summary();
        tracing::info!(file = %path_str, "{}", columns_summary);

        let well_id = config.well_id.clone().unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string()
        });

        let format = col_map.format.unwrap_or(CsvFormat::Kaggle);

        let mut packets = Vec::new();
        let mut skipped = 0usize;
        let mut errors = 0usize;
        let mut line_num = 1usize;

        for line_result in lines {
            line_num += 1;

            let line = match line_result {
                Ok(l) => l,
                Err(e) => {
                    tracing::warn!(line = line_num, error = %e, "Error reading line");
                    errors += 1;
                    continue;
                }
            };

            if line.trim().is_empty() {
                continue;
            }

            match parse_row(&line, &col_map, format, &config, line_num) {
                Ok(Some(packet)) => packets.push(packet),
                Ok(None) => skipped += 1,
                Err(e) => {
                    if errors < 10 {
                        tracing::warn!(line = line_num, error = %e, "Parse error");
                    }
                    errors += 1;
                }
            }
        }

        if packets.is_empty() {
            return Err(format!(
                "No valid packets from {}. {} errors, {} skipped.",
                path_str, errors, skipped
            ));
        }

        let depth_min = packets.iter().map(|p| p.bit_depth).fold(f64::INFINITY, f64::min);
        let depth_max = packets.iter().map(|p| p.bit_depth).fold(f64::NEG_INFINITY, f64::max);
        let time_first = packets.first().map(|p| p.timestamp).unwrap_or(0);
        let time_last = packets.last().map(|p| p.timestamp).unwrap_or(0);

        let fmt_name = match format {
            CsvFormat::Kaggle => "Kaggle",
            CsvFormat::Tunkiel => "Tunkiel",
        };

        let info = VolveWellInfo {
            well_id,
            source_path: path_str,
            format: fmt_name.to_string(),
            packet_count: packets.len(),
            skipped_rows: skipped,
            error_rows: errors,
            columns_found: columns_summary,
            depth_range_ft: (depth_min, depth_max),
            time_range: (time_first, time_last),
        };

        tracing::info!(
            well = %info.well_id,
            format = fmt_name,
            packets = info.packet_count,
            skipped = info.skipped_rows,
            errors = info.error_rows,
            depth_range = format!("{:.0}-{:.0} ft", depth_min, depth_max),
            "Volve well loaded"
        );

        Ok(Self { packets, info })
    }

    /// Get all packets
    pub fn packets(&self) -> &[WitsPacket] {
        &self.packets
    }

    /// Consume and return owned packets
    pub fn into_packets(self) -> Vec<WitsPacket> {
        self.packets
    }

    /// Get packets for a depth range (in feet)
    pub fn packets_in_depth_range(&self, min_ft: f64, max_ft: f64) -> Vec<&WitsPacket> {
        self.packets
            .iter()
            .filter(|p| p.bit_depth >= min_ft && p.bit_depth <= max_ft)
            .collect()
    }

    /// Get only drilling packets (RPM > 0, WOB > 0)
    pub fn drilling_packets(&self) -> Vec<&WitsPacket> {
        self.packets.iter().filter(|p| p.is_drilling()).collect()
    }

    /// Summary statistics for quick validation
    pub fn print_summary(&self) {
        let drilling = self.drilling_packets().len();
        let duration_hrs = if self.info.time_range.1 > self.info.time_range.0 {
            (self.info.time_range.1 - self.info.time_range.0) as f64 / 3600.0
        } else {
            0.0
        };

        println!("=== Volve Well: {} ===", self.info.well_id);
        println!("  Source:     {}", self.info.source_path);
        println!("  Format:     {}", self.info.format);
        println!("  Packets:    {} total, {} drilling", self.info.packet_count, drilling);
        println!("  Skipped:    {} null rows, {} errors", self.info.skipped_rows, self.info.error_rows);
        println!(
            "  Depth:      {:.0} - {:.0} ft ({:.0} - {:.0} m)",
            self.info.depth_range_ft.0,
            self.info.depth_range_ft.1,
            self.info.depth_range_ft.0 / M_TO_FT,
            self.info.depth_range_ft.1 / M_TO_FT,
        );
        println!("  Duration:   {:.1} hours ({:.1} days)", duration_hrs, duration_hrs / 24.0);
        println!("  Columns:    {}", self.info.columns_found);

        if let Some(first) = self.packets.first() {
            println!(
                "  First pkt:  depth={:.0}ft WOB={:.1}klbs RPM={:.0} ROP={:.1}ft/hr SPP={:.0}psi MW={:.2}ppg",
                first.bit_depth, first.wob, first.rpm, first.rop, first.spp, first.mud_weight_in
            );
        }
        if let Some(last) = self.packets.last() {
            println!(
                "  Last pkt:   depth={:.0}ft WOB={:.1}klbs RPM={:.0} ROP={:.1}ft/hr SPP={:.0}psi MW={:.2}ppg",
                last.bit_depth, last.wob, last.rpm, last.rop, last.spp, last.mud_weight_in
            );
        }
    }
}

// ============================================================================
// Row Parsing
// ============================================================================

/// Parse a CSV row into a WitsPacket, handling both formats
fn parse_row(
    line: &str,
    col_map: &ColumnMap,
    format: CsvFormat,
    config: &VolveConfig,
    _line_num: usize,
) -> Result<Option<WitsPacket>, String> {
    let owned_fields = csv_split(line);
    let fields: Vec<&str> = owned_fields.iter().map(|s| s.as_str()).collect();

    // --- Timestamp ---
    let timestamp = parse_timestamp_from_row(&fields, col_map)?;

    // --- Read raw values ---
    let raw_depth = get_f64(&fields, col_map.depth, config.nan_to_zero).unwrap_or(0.0);
    let raw_hole_depth = get_f64(&fields, col_map.hole_depth, config.nan_to_zero).unwrap_or(raw_depth);
    let raw_wob = get_f64(&fields, col_map.wob, config.nan_to_zero).unwrap_or(0.0);
    let raw_torque = get_f64(&fields, col_map.torque, config.nan_to_zero).unwrap_or(0.0);
    let raw_rpm = get_f64(&fields, col_map.rpm, config.nan_to_zero).unwrap_or(0.0);
    let raw_rop = get_f64(&fields, col_map.rop, config.nan_to_zero).unwrap_or(0.0);
    let raw_hookload = get_f64(&fields, col_map.hook_load, config.nan_to_zero).unwrap_or(0.0);
    let raw_spp = get_f64(&fields, col_map.spp, config.nan_to_zero).unwrap_or(0.0);
    let raw_flow_in = get_f64(&fields, col_map.flow_in, config.nan_to_zero).unwrap_or(0.0);
    let raw_flow_out = get_f64(&fields, col_map.flow_out, config.nan_to_zero).unwrap_or(0.0);
    let raw_mw_in = get_f64(&fields, col_map.mw_in, config.nan_to_zero).unwrap_or(0.0);
    let raw_mw_out = get_f64(&fields, col_map.mw_out, config.nan_to_zero).unwrap_or(0.0);
    let raw_ecd = get_f64(&fields, col_map.ecd, config.nan_to_zero).unwrap_or(0.0);
    let raw_temp_in = get_f64(&fields, col_map.temp_in, config.nan_to_zero).unwrap_or(0.0);
    let raw_temp_out = get_f64(&fields, col_map.temp_out, config.nan_to_zero).unwrap_or(0.0);
    let raw_gas = get_f64(&fields, col_map.gas, config.nan_to_zero).unwrap_or(0.0);
    let raw_dxc = get_f64(&fields, col_map.dxc, config.nan_to_zero).unwrap_or(0.0);
    let raw_pump_spm = get_f64(&fields, col_map.pump_spm, config.nan_to_zero).unwrap_or(0.0);
    let raw_pit_vol = get_f64(&fields, col_map.pit_volume, config.nan_to_zero).unwrap_or(0.0);

    // --- Convert to oilfield units based on format ---
    let (depth_ft, hole_depth_ft, wob_klbs, torque_kftlb, rpm, rop_fthr, hookload_klbs,
         spp_psi, flow_in_gpm, flow_out_gpm, mw_in_ppg, mw_out_ppg, ecd_ppg,
         temp_in_f, temp_out_f, pump_spm, pit_vol_bbl) = match format {
        CsvFormat::Kaggle => (
            raw_depth * M_TO_FT,
            raw_hole_depth * M_TO_FT,
            raw_wob * KKGF_TO_KLBF,
            raw_torque * KNM_TO_KFTLB,
            raw_rpm,                        // Already RPM
            raw_rop * MH_TO_FTHR,
            raw_hookload * KKGF_TO_KLBF,
            raw_spp * KPA_TO_PSI,
            raw_flow_in * LMIN_TO_GPM,
            raw_flow_out * LMIN_TO_GPM,
            raw_mw_in * GCM3_TO_PPG,
            raw_mw_out * GCM3_TO_PPG,
            raw_ecd * GCM3_TO_PPG,
            if raw_temp_in != 0.0 { celsius_to_fahrenheit(raw_temp_in) } else { 0.0 },
            if raw_temp_out != 0.0 { celsius_to_fahrenheit(raw_temp_out) } else { 0.0 },
            raw_pump_spm,                   // Already 1/min
            raw_pit_vol * 6.28981,          // m³ to bbl
        ),
        CsvFormat::Tunkiel => (
            raw_depth * M_TO_FT,
            raw_hole_depth * M_TO_FT,
            raw_wob * N_TO_KLBF,
            raw_torque * NM_TO_KFTLB,
            raw_rpm * RPS_TO_RPM,
            raw_rop * MS_TO_FTHR,
            raw_hookload * N_TO_KLBF,
            raw_spp * PA_TO_PSI,
            raw_flow_in * M3S_TO_GPM,
            raw_flow_out * M3S_TO_GPM,
            raw_mw_in * KGM3_TO_PPG,
            raw_mw_out * KGM3_TO_PPG,
            raw_ecd * KGM3_TO_PPG,
            kelvin_to_fahrenheit(raw_temp_in),
            kelvin_to_fahrenheit(raw_temp_out),
            raw_pump_spm * 60.0,            // Hz to 1/min
            raw_pit_vol,                    // Assume already bbl
        ),
    };

    // --- Skip null rows ---
    if config.skip_null_rows {
        let all_zero = wob_klbs.abs() < 1e-10
            && rpm.abs() < 1e-10
            && rop_fthr.abs() < 1e-10
            && spp_psi.abs() < 1e-10
            && depth_ft.abs() < 1e-10;

        if all_zero {
            return Ok(None);
        }
    }

    // --- Rig state ---
    let rig_state = parse_rig_state(&fields, col_map, rpm, wob_klbs, rop_fthr);

    Ok(Some(WitsPacket {
        timestamp,
        bit_depth: depth_ft,
        hole_depth: hole_depth_ft,
        rop: rop_fthr,
        hook_load: hookload_klbs,
        wob: wob_klbs,
        rpm,
        torque: torque_kftlb,
        bit_diameter: config.default_bit_diameter,
        spp: spp_psi,
        pump_spm,
        flow_in: flow_in_gpm,
        flow_out: flow_out_gpm,
        pit_volume: pit_vol_bbl,
        pit_volume_change: 0.0,
        mud_weight_in: mw_in_ppg,
        mud_weight_out: mw_out_ppg,
        ecd: ecd_ppg,
        mud_temp_in: temp_in_f,
        mud_temp_out: temp_out_f,
        gas_units: raw_gas,    // Pass through (units vary)
        background_gas: 0.0,
        connection_gas: 0.0,
        h2s: 0.0,
        co2: 0.0,
        casing_pressure: 0.0,
        annular_pressure: 0.0,
        pore_pressure: 0.0,
        fracture_gradient: 0.0,
        mse: 0.0,              // Physics engine calculates
        d_exponent: 0.0,       // Physics engine calculates
        dxc: raw_dxc,          // Cross-validate against physics engine
        rop_delta: 0.0,
        torque_delta_percent: 0.0,
        spp_delta: 0.0,
        rig_state,
        waveform_snapshot: Arc::new(Vec::new()),
    }))
}

// ============================================================================
// Helpers
// ============================================================================

/// Extract timestamp from row
fn parse_timestamp_from_row(fields: &[&str], col_map: &ColumnMap) -> Result<u64, String> {
    // Prefer parsed datetime column (Kaggle format)
    if let Some(idx) = col_map.datetime_parsed {
        if let Some(s) = fields.get(idx).map(|s| s.trim()) {
            if !s.is_empty() {
                return parse_datetime_string(s);
            }
        }
    }

    // Try time column
    if let Some(idx) = col_map.time {
        if let Some(s) = fields.get(idx).map(|s| s.trim()) {
            if !s.is_empty() {
                return parse_datetime_string(s);
            }
        }
    }

    Err("No timestamp available".to_string())
}

/// Parse various datetime string formats to Unix timestamp
fn parse_datetime_string(s: &str) -> Result<u64, String> {
    let s = s.trim().trim_matches('"');

    if s.is_empty() || s.eq_ignore_ascii_case("nan") {
        return Err("Empty timestamp".to_string());
    }

    // Unix epoch (numeric)
    if let Ok(epoch) = s.parse::<u64>() {
        return Ok(if epoch > 10_000_000_000 { epoch / 1000 } else { epoch });
    }

    // Try float epoch
    if let Ok(epoch_f) = s.parse::<f64>() {
        if epoch_f > 1_000_000_000.0 && epoch_f.is_finite() {
            return Ok(epoch_f as u64);
        }
    }

    // "2009-06-27 16:50:29+00:00" (Kaggle parsed format)
    if let Ok(dt) = chrono::DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%:z") {
        return Ok(dt.timestamp() as u64);
    }

    // ISO 8601 with timezone
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.timestamp() as u64);
    }
    if let Ok(dt) = chrono::DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f%:z") {
        return Ok(dt.timestamp() as u64);
    }

    // Without timezone (assume UTC)
    for fmt in &[
        "%Y-%m-%dT%H:%M:%S%.fZ",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
    ] {
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Ok(dt.and_utc().timestamp() as u64);
        }
    }

    Err(format!("Cannot parse timestamp: '{}'", s))
}

/// Get an f64 field from CSV row by optional column index
fn get_f64(fields: &[&str], idx: Option<usize>, nan_to_zero: bool) -> Option<f64> {
    idx.and_then(|i| {
        fields.get(i).and_then(|s| {
            let s = s.trim();
            if s.is_empty() || s.eq_ignore_ascii_case("nan") || s.eq_ignore_ascii_case("null") || s == "-" {
                if nan_to_zero { Some(0.0) } else { None }
            } else {
                s.parse::<f64>().ok().map(|v| {
                    if v.is_nan() || v.is_infinite() {
                        if nan_to_zero { 0.0 } else { 0.0 }
                    } else {
                        v
                    }
                })
            }
        })
    })
}

/// Parse rig state from row
fn parse_rig_state(fields: &[&str], col_map: &ColumnMap, rpm: f64, wob_klbs: f64, rop_fthr: f64) -> RigState {
    // Try Kaggle "Rig Mode" text column
    if let Some(idx) = col_map.rig_mode {
        if let Some(s) = fields.get(idx).map(|s| s.trim().to_lowercase()) {
            if !s.is_empty() && s != "nan" {
                return match s.as_str() {
                    s if s.contains("drill") => RigState::Drilling,
                    s if s.contains("ream") => RigState::Reaming,
                    s if s.contains("circ") => RigState::Circulating,
                    s if s.contains("conn") => RigState::Connection,
                    s if s.contains("trip") && s.contains("in") => RigState::TrippingIn,
                    s if s.contains("trip") && s.contains("out") => RigState::TrippingOut,
                    s if s.contains("trip") => RigState::TrippingOut,
                    _ => classify_from_params(rpm, wob_klbs, rop_fthr),
                };
            }
        }
    }

    // Try Tunkiel numeric activity code
    if let Some(idx) = col_map.rig_activity_code {
        if let Some(code) = get_f64(fields, Some(idx), false) {
            let code_int = code as i32;
            return match code_int {
                1..=3 => RigState::Drilling,
                4 | 5 => RigState::Reaming,
                6..=8 => RigState::Circulating,
                9 | 10 => RigState::Connection,
                11..=13 => RigState::TrippingIn,
                14..=16 => RigState::TrippingOut,
                _ => RigState::Idle,
            };
        }
    }

    classify_from_params(rpm, wob_klbs, rop_fthr)
}

fn classify_from_params(rpm: f64, wob_klbs: f64, rop_fthr: f64) -> RigState {
    if rpm > 20.0 && wob_klbs > 2.0 && rop_fthr > 0.5 {
        RigState::Drilling
    } else if rpm > 20.0 && wob_klbs > 1.0 {
        RigState::Reaming
    } else if rpm > 0.0 && wob_klbs < 2.0 {
        RigState::Circulating
    } else {
        RigState::Idle
    }
}

/// Load all Volve CSV files from a directory
pub fn load_volve_directory(dir: impl AsRef<Path>, config: VolveConfig) -> Vec<VolveReplay> {
    let dir = dir.as_ref();
    let mut wells = Vec::new();

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::error!(dir = %dir.display(), error = %e, "Failed to read Volve directory");
            return wells;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("csv") {
            match VolveReplay::load(&path, config.clone()) {
                Ok(replay) => {
                    tracing::info!(well = %replay.info.well_id, packets = replay.info.packet_count, "Loaded");
                    wells.push(replay);
                }
                Err(e) => {
                    tracing::warn!(file = %path.display(), error = %e, "Failed to load Volve well");
                }
            }
        }
    }

    wells
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kaggle_unit_conversions() {
        // 1 kkgf = 2.20462 klbs
        let wob = 10.0 * KKGF_TO_KLBF;
        assert!((wob - 22.0462).abs() < 0.01, "wob: {}", wob);

        // 1 kN·m = 0.73756 kft·lbs
        let tq = 10.0 * KNM_TO_KFTLB;
        assert!((tq - 7.3756).abs() < 0.01, "torque: {}", tq);

        // 100 m/h = 328.084 ft/hr
        let rop = 100.0 * MH_TO_FTHR;
        assert!((rop - 328.084).abs() < 0.1, "rop: {}", rop);

        // 20000 kPa = 2900.75 psi
        let spp = 20000.0 * KPA_TO_PSI;
        assert!((spp - 2900.75).abs() < 1.0, "spp: {}", spp);

        // 1000 L/min = 264.172 gpm
        let flow = 1000.0 * LMIN_TO_GPM;
        assert!((flow - 264.172).abs() < 0.1, "flow: {}", flow);

        // 1.2 g/cm³ = 10.014 ppg
        let mw = 1.2 * GCM3_TO_PPG;
        assert!((mw - 10.0145).abs() < 0.1, "mw: {}", mw);

        // 50°C = 122°F
        let f = celsius_to_fahrenheit(50.0);
        assert!((f - 122.0).abs() < 0.1, "temp: {}", f);
    }

    #[test]
    fn test_tunkiel_unit_conversions() {
        // 100 m = 328.084 ft
        let depth = 100.0 * M_TO_FT;
        assert!((depth - 328.084).abs() < 0.01);

        // 1 rev/s = 60 RPM
        let rpm = 1.0 * RPS_TO_RPM;
        assert!((rpm - 60.0).abs() < 0.01);

        // 373.15 K = 212 °F
        let f = kelvin_to_fahrenheit(373.15);
        assert!((f - 212.0).abs() < 0.1);
    }

    #[test]
    fn test_format_detection_kaggle() {
        let header = ",Unnamed: 0,Time s,Weight on Bit kkgf,Stand Pipe Pressure kPa,Average Surface Torque kN.m";
        let map = ColumnMap::from_header(header);
        assert_eq!(map.format, Some(CsvFormat::Kaggle));
        assert!(map.wob.is_some());
        assert!(map.spp.is_some());
        assert!(map.torque.is_some());
    }

    #[test]
    fn test_format_detection_tunkiel() {
        let header = "Time,Depth,WOB,TORQUE,SURF_RPM,ROP_AVG,PUMP,FLOWIN";
        let map = ColumnMap::from_header(header);
        assert_eq!(map.format, Some(CsvFormat::Tunkiel));
        assert!(map.wob.is_some());
        assert!(map.spp.is_some());
        assert!(map.rpm.is_some());
    }

    #[test]
    fn test_parse_kaggle_timestamp() {
        let ts = parse_datetime_string("2009-06-27 16:50:29+00:00").unwrap();
        assert!(ts > 1_000_000_000);
        assert!(ts < 2_000_000_000);
    }

    /// Integration test: load actual Volve CSV and validate parsing
    #[test]
    fn test_load_real_volve_csv() {
        let path = "data/volve/Norway-NA-15_47_9-F-9 A time.csv";
        if !std::path::Path::new(path).exists() {
            eprintln!("Skipping real data test: {} not found", path);
            return;
        }

        let config = VolveConfig {
            skip_null_rows: true,
            nan_to_zero: true,
            ..Default::default()
        };

        let replay = VolveReplay::load(path, config).expect("Failed to load Volve CSV");
        let info = &replay.info;

        eprintln!("=== Volve F-9A Integration Test ===");
        eprintln!("Well:       {}", info.well_id);
        eprintln!("Packets:    {}", info.packet_count);
        eprintln!("Skipped:    {}", info.skipped_rows);
        eprintln!("Errors:     {}", info.error_rows);
        eprintln!("Format:     {}", info.format);
        eprintln!("Time range: {} - {}", info.time_range.0, info.time_range.1);
        eprintln!("Depth:      {:.0} - {:.0} ft", info.depth_range_ft.0, info.depth_range_ft.1);
        eprintln!("Columns:    {}", info.columns_found);

        // We should have parsed a meaningful number of packets
        assert!(info.packet_count > 1000,
            "Expected >1000 packets, got {}", info.packet_count);

        // Should detect Kaggle format
        assert_eq!(info.format, "Kaggle");

        // Timestamps should be in 2008-2016 range (Volve field operations)
        assert!(info.time_range.0 >= 1_199_145_600, "time_start too early: {}", info.time_range.0); // 2008-01-01
        assert!(info.time_range.1 <= 1_483_228_800, "time_end too late: {}", info.time_range.1);     // 2017-01-01

        // Depth should be reasonable for Volve (up to ~3000m = ~10000ft)
        assert!(info.depth_range_ft.1 > 0.0, "No depth data parsed");
        assert!(info.depth_range_ft.1 < 15_000.0, "Depth too deep: {}", info.depth_range_ft.1);

        // Spot-check a few packets for sane oilfield-unit values
        let packets = replay.packets();
        let drilling: Vec<&WitsPacket> = packets.iter()
            .filter(|p| matches!(p.rig_state, RigState::Drilling))
            .take(100)
            .collect();

        eprintln!("Drilling packets in first batch: {}", drilling.len());

        if !drilling.is_empty() {
            let p = drilling[0];
            eprintln!("Sample drilling packet:");
            eprintln!("  timestamp:  {}", p.timestamp);
            eprintln!("  bit_depth:  {:.1} ft", p.bit_depth);
            eprintln!("  wob:        {:.2} klbs", p.wob);
            eprintln!("  rpm:        {:.1} RPM", p.rpm);
            eprintln!("  torque:     {:.3} kft-lbs", p.torque);
            eprintln!("  rop:        {:.2} ft/hr", p.rop);
            eprintln!("  spp:        {:.1} psi", p.spp);
            eprintln!("  flow_in:    {:.1} gpm", p.flow_in);
            eprintln!("  mw_in:      {:.2} ppg", p.mud_weight_in);
            eprintln!("  temp_in:    {:.1} °F", p.mud_temp_in);

            // Sanity: WOB should be < 100 klbs for this well
            assert!(p.wob < 100.0, "WOB too high: {} klbs", p.wob);
            // RPM should be < 300
            assert!(p.rpm < 300.0, "RPM too high: {} RPM", p.rpm);
            // SPP should be < 10000 psi for North Sea
            assert!(p.spp < 10_000.0, "SPP too high: {} psi", p.spp);
            // Mud weight should be in 7-18 ppg range
            if p.mud_weight_in > 0.0 {
                assert!(p.mud_weight_in > 5.0 && p.mud_weight_in < 20.0,
                    "MW out of range: {} ppg", p.mud_weight_in);
            }
        }

        // Count rig states
        let mut state_counts = std::collections::HashMap::new();
        for p in packets {
            *state_counts.entry(format!("{:?}", p.rig_state)).or_insert(0u64) += 1;
        }
        eprintln!("Rig state distribution:");
        for (state, count) in &state_counts {
            eprintln!("  {}: {} ({:.1}%)", state, count,
                *count as f64 / info.packet_count as f64 * 100.0);
        }
    }

    #[test]
    fn test_get_f64_nan_handling() {
        let fields = vec!["1.5", "NaN", "", "null", "-", "3.14"];
        assert_eq!(get_f64(&fields, Some(0), true), Some(1.5));
        assert_eq!(get_f64(&fields, Some(1), true), Some(0.0));
        assert_eq!(get_f64(&fields, Some(1), false), None);
        assert_eq!(get_f64(&fields, Some(2), true), Some(0.0));
        assert_eq!(get_f64(&fields, Some(3), true), Some(0.0));
        assert_eq!(get_f64(&fields, Some(4), true), Some(0.0));
        assert_eq!(get_f64(&fields, Some(5), true), Some(3.14));
        assert_eq!(get_f64(&fields, None, true), None);
    }
}
