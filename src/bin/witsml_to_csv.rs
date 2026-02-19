//! WITSML XML to CSV converter for Volve field data.
//!
//! Reads WITSML 1.4.1.1 XML log files from the Equinor Volve real-time drilling
//! data zip archive and converts them to CSV format compatible with the SAIREN-OS
//! volve_replay binary (Kaggle/Format A column names).
//!
//! Usage:
//!   cargo run --bin witsml-to-csv -- --zip data/volve/Volve*.zip --well "F-9 A" --output data/volve/F-9A.csv
//!   cargo run --bin witsml-to-csv -- --zip data/volve/Volve*.zip --list

use std::collections::HashMap;
use std::io::{BufWriter, Read, Write};
use std::path::PathBuf;

use clap::Parser;

/// WITSML XML to CSV converter for Volve drilling data.
#[derive(Parser)]
#[command(name = "witsml-to-csv")]
struct Args {
    /// Path to the Volve zip archive.
    #[arg(long)]
    zip: PathBuf,

    /// Well name filter (substring match, e.g. "F-9 A", "F-5", "F-12").
    /// If not specified, lists available wells.
    #[arg(long)]
    well: Option<String>,

    /// Output CSV path. Defaults to data/volve/<well>.csv.
    #[arg(long, short)]
    output: Option<PathBuf>,

    /// List available wells in the archive and exit.
    #[arg(long)]
    list: bool,

    /// Only process time-based logs (skip depth-based). Default: true.
    #[arg(long, default_value = "true")]
    time_logs_only: bool,
}

/// Mapping from WITSML mnemonics to Kaggle-format column names.
/// Multiple mnemonics can map to the same output column (first match wins per row).
struct MnemonicMap {
    /// mnemonic -> (output_column_name, priority)
    /// Lower priority number = preferred.
    map: HashMap<String, (String, u8)>,
}

impl MnemonicMap {
    fn new() -> Self {
        let mut map = HashMap::new();

        // Helper: insert with priority (lower = preferred)
        let mut add = |mnemonics: &[&str], col: &str, priority_start: u8| {
            for (i, &mn) in mnemonics.iter().enumerate() {
                let p = priority_start + i as u8;
                map.entry(mn.to_string())
                    .or_insert_with(|| (col.to_string(), p));
            }
        };

        // TIME — index column (ISO 8601 datetime)
        add(&["TIME"], "Time Time", 0);

        // Depth (metres)
        add(&["DBTM", "GS_DBTM"], "Bit Depth m", 0);
        add(&["DMEA", "GS_DMEA", "DEPT"], "Total Depth m", 0);

        // WOB (kkgf)
        add(&["SWOB", "GS_SWOB", "SWOB30s", "CWOB"], "Weight on Bit kkgf", 0);

        // Torque (kN.m)
        add(&["TQA", "GS_TQA", "TQ30s"], "Average Surface Torque kN.m", 0);

        // RPM
        add(&["RPM", "GS_RPM", "RPM30s"], "Average Rotary Speed rpm", 0);

        // ROP (m/h)
        add(&["ROP", "GS_ROP", "ROP5", "ROP30s", "ROP2M", "ROPH"], "Rate of Penetration m/h", 0);

        // SPP (kPa)
        add(&["SPPA", "GS_SPPA", "SPP5s"], "Stand Pipe Pressure kPa", 0);

        // Hookload (kkgf)
        add(&["HKLD", "GS_HKLD", "HKLD30s", "HKLI", "HKLO"], "Total Hookload kkgf", 0);

        // Flow in (L/min)
        add(&["TFLO", "GS_TFLO", "TFLO30s"], "Mud Flow In L/min", 0);

        // Mud weight in (g/cm3)
        add(&["MWTI", "GS_MWTI"], "Mud Density In g/cm3", 0);

        // Mud weight out (g/cm3)
        add(&["MDOA", "GS_MDOA"], "Mud Density Out g/cm3", 0);

        // ECD (g/cm3)
        add(&["ECD_MW_IN"], "Equivalent Circulating Density g/cm3", 0);

        // Mud temp in (degC)
        add(&["TDH", "GS_TDH"], "Tmp In degC", 0);

        // Mud temp out (degC)
        add(&["MTOA", "GS_MTOA"], "Temperature Out degC", 0);

        // Gas (%)
        add(&["GASA", "GS_GASA"], "Gas (avg) %", 0);

        // Pump SPM (1/min)
        add(&["TSPM", "SPM1", "GS_SPM1"], "Total SPM 1/min", 0);

        // Tank volume (m3)
        add(&["TVA", "GS_TVA", "TVCA", "GS_TVCA"], "Tank Volume (Active) m3", 0);

        // Activity code
        add(&["ACTC", "GS_ACTC"], "Rig Mode", 0);

        // Block position (m)
        add(&["BPOS", "GS_BPOS"], "Block Position m", 0);

        Self { map }
    }

    /// Get the output column name for a mnemonic.
    fn get(&self, mnemonic: &str) -> Option<&str> {
        self.map.get(mnemonic).map(|(col, _)| col.as_str())
    }
}

/// Activity code to rig mode string mapping.
fn activity_code_to_rig_mode(code: &str) -> &'static str {
    // Try parsing as float then truncating to int
    let val: f64 = match code.trim().parse() {
        Ok(v) => v,
        Err(_) => return "",
    };
    let code_int = val as i32;

    match code_int {
        0 => "",
        1 | 2 | 3 => "Drilling",
        4 | 5 => "Reaming",
        6 | 7 | 8 => "Circulating",
        9 | 10 => "Connection",
        11 | 12 | 13 => "Trip In",
        14 | 15 | 16 => "Trip Out",
        _ => "",
    }
}

/// Parse a single WITSML XML file content and extract rows.
fn parse_witsml_xml(
    xml_content: &str,
    mnemonic_map: &MnemonicMap,
    output_columns: &[String],
    col_index: &HashMap<String, usize>,
) -> Vec<(String, Vec<String>)> {
    // Extract mnemonicList
    let mnemonic_list: Vec<&str> = match extract_between(xml_content, "<mnemonicList>", "</mnemonicList>") {
        Some(s) => s.split(',').collect(),
        None => return Vec::new(),
    };

    // Build column mapping: xml_col_index -> output_col_index
    let mut xml_to_output: Vec<Option<usize>> = vec![None; mnemonic_list.len()];
    // Track which output columns are already mapped (prefer earlier mnemonics)
    let mut output_mapped: Vec<bool> = vec![false; output_columns.len()];

    for (xml_idx, &mnemonic) in mnemonic_list.iter().enumerate() {
        if let Some(col_name) = mnemonic_map.get(mnemonic) {
            if let Some(&out_idx) = col_index.get(col_name) {
                if !output_mapped[out_idx] {
                    xml_to_output[xml_idx] = Some(out_idx);
                    output_mapped[out_idx] = true;
                }
            }
        }
    }

    // Extract all <data>...</data> rows
    let mut rows = Vec::new();
    let mut search_start = 0;

    loop {
        let data_start = match xml_content[search_start..].find("<data>") {
            Some(pos) => search_start + pos + 6,
            None => break,
        };
        let data_end = match xml_content[data_start..].find("</data>") {
            Some(pos) => data_start + pos,
            None => break,
        };

        let data_str = &xml_content[data_start..data_end];
        let values: Vec<&str> = data_str.split(',').collect();

        if values.len() != mnemonic_list.len() {
            search_start = data_end + 7;
            continue;
        }

        // Build output row
        let mut row = vec![String::new(); output_columns.len()];
        let mut timestamp = String::new();

        for (xml_idx, &val) in values.iter().enumerate() {
            if xml_idx == 0 {
                // TIME column — always first
                timestamp = val.to_string();
            }
            if let Some(out_idx) = xml_to_output[xml_idx] {
                // Handle Rig Mode: convert activity code to text
                if output_columns[out_idx] == "Rig Mode" {
                    let mode = activity_code_to_rig_mode(val);
                    if !mode.is_empty() {
                        row[out_idx] = mode.to_string();
                    }
                } else {
                    row[out_idx] = val.to_string();
                }
            }
        }

        if !timestamp.is_empty() {
            row[0] = timestamp.clone();
            rows.push((timestamp, row));
        }

        search_start = data_end + 7;
    }

    rows
}

/// Extract text between two markers in a string.
fn extract_between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let start_idx = s.find(start)? + start.len();
    let end_idx = s[start_idx..].find(end)? + start_idx;
    Some(&s[start_idx..end_idx])
}

fn main() {
    let args = Args::parse();

    // Open zip
    let file = std::fs::File::open(&args.zip).unwrap_or_else(|e| {
        eprintln!("ERROR: Failed to open zip: {}: {}", args.zip.display(), e);
        std::process::exit(1);
    });
    let mut archive = zip::ZipArchive::new(file).unwrap_or_else(|e| {
        eprintln!("ERROR: Failed to read zip archive: {}", e);
        std::process::exit(1);
    });

    // Scan for wells
    let mut wells: HashMap<String, Vec<String>> = HashMap::new();
    for i in 0..archive.len() {
        let entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let name = entry.name().to_string();
        if name.contains("/log/") && name.ends_with(".xml") {
            // Extract well name from path
            let parts: Vec<&str> = name.split('/').collect();
            if parts.len() >= 2 {
                let well_dir = parts[1]; // e.g., "Norway-NA-15_$47$_9-F-9 A"
                let well_name = well_dir
                    .replace("_$47$_", "/")
                    .replace("NA-NA-", "")
                    .replace("Norway-NA-", "")
                    .replace("Norway-Statoil-", "")
                    .replace("Norway-StatoilHydro-", "");
                wells.entry(well_name).or_default().push(name);
            }
        }
    }

    if args.list || args.well.is_none() {
        println!("Available wells in archive:");
        println!("{:<35} {:>6}", "Well", "Files");
        println!("{}", "-".repeat(45));
        let mut well_list: Vec<_> = wells.iter().collect();
        well_list.sort_by_key(|(name, _)| (*name).clone());
        for (name, files) in &well_list {
            // Separate time vs depth logs
            let time_files = files.iter().filter(|f| f.contains("log/1/1/")).count();
            let depth_files = files.iter().filter(|f| f.contains("log/1/2/")).count();
            println!("{:<35} {:>3} time, {:>3} depth", name, time_files, depth_files);
        }
        if args.well.is_none() {
            println!("\nUse --well <name> to convert a well. Example:");
            println!("  cargo run --bin witsml-to-csv -- --zip '{}' --well 'F-9 A'", args.zip.display());
        }
        return;
    }

    let well_filter = args.well.as_deref().unwrap_or("");

    // Find matching well
    let matching: Vec<_> = wells.iter()
        .filter(|(name, _)| name.contains(well_filter))
        .collect();

    if matching.is_empty() {
        eprintln!("ERROR: No well matching '{}' found.", well_filter);
        eprintln!("Available: {:?}", wells.keys().collect::<Vec<_>>());
        std::process::exit(1);
    }
    if matching.len() > 1 {
        eprintln!("WARNING: Multiple wells match '{}', using first:", well_filter);
        for (name, files) in &matching {
            eprintln!("  {} ({} files)", name, files.len());
        }
    }

    let (well_name, xml_files) = matching[0];
    let mut xml_files: Vec<_> = xml_files.clone();

    // Filter to time logs only (log/1/1/ = time, log/1/2/ = depth)
    if args.time_logs_only {
        xml_files.retain(|f| f.contains("/log/1/1/"));
    }

    // Sort by filename for consistent ordering
    xml_files.sort();

    println!("Converting well: {}", well_name);
    println!("  XML files: {} (time logs)", xml_files.len());

    // Setup mnemonic mapping and output columns
    let mnemonic_map = MnemonicMap::new();

    let output_columns: Vec<String> = vec![
        "Time Time",
        "Bit Depth m",
        "Total Depth m",
        "Weight on Bit kkgf",
        "Average Surface Torque kN.m",
        "Average Rotary Speed rpm",
        "Rate of Penetration m/h",
        "Stand Pipe Pressure kPa",
        "Total Hookload kkgf",
        "Mud Flow In L/min",
        "Mud Density In g/cm3",
        "Mud Density Out g/cm3",
        "Equivalent Circulating Density g/cm3",
        "Tmp In degC",
        "Temperature Out degC",
        "Gas (avg) %",
        "Total SPM 1/min",
        "Tank Volume (Active) m3",
        "Rig Mode",
        "Block Position m",
    ].into_iter().map(String::from).collect();

    let col_index: HashMap<String, usize> = output_columns.iter()
        .enumerate()
        .map(|(i, name)| (name.clone(), i))
        .collect();

    // Parse all XML files
    let mut all_rows: Vec<(String, Vec<String>)> = Vec::new();
    let mut files_processed = 0;
    let mut files_with_data = 0;

    for xml_path in &xml_files {
        // Re-open archive for each file (zip crate limitation with sequential access)
        let file = std::fs::File::open(&args.zip).unwrap_or_else(|e| {
            eprintln!("ERROR: Failed to reopen zip: {}", e);
            std::process::exit(1);
        });
        let mut archive = zip::ZipArchive::new(file).unwrap_or_else(|e| {
            eprintln!("ERROR: Failed to read zip: {}", e);
            std::process::exit(1);
        });

        let mut entry = match archive.by_name(xml_path) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("  WARN: Failed to read {}: {}", xml_path, e);
                continue;
            }
        };

        let mut content = String::new();
        if let Err(e) = entry.read_to_string(&mut content) {
            eprintln!("  WARN: Failed to read content of {}: {}", xml_path, e);
            continue;
        }

        files_processed += 1;
        let rows = parse_witsml_xml(&content, &mnemonic_map, &output_columns, &col_index);

        if !rows.is_empty() {
            files_with_data += 1;
            all_rows.extend(rows);
            eprint!("\r  Parsed {}/{} files, {} rows so far...", files_processed, xml_files.len(), all_rows.len());
        }
    }
    eprintln!();

    if all_rows.is_empty() {
        eprintln!("ERROR: No data rows extracted. Check well name and XML structure.");
        std::process::exit(1);
    }

    // Sort by timestamp
    all_rows.sort_by(|a, b| a.0.cmp(&b.0));

    // Deduplicate by timestamp (some XML files overlap)
    all_rows.dedup_by(|a, b| a.0 == b.0);

    println!("  Files with data: {}/{}", files_with_data, files_processed);
    println!("  Total rows: {} (after dedup)", all_rows.len());
    if let (Some(first), Some(last)) = (all_rows.first(), all_rows.last()) {
        println!("  Time range: {} to {}", first.0, last.0);
    }

    // Determine output path
    let output_path = args.output.unwrap_or_else(|| {
        let safe_name = well_name.replace('/', "_").replace(' ', "_");
        PathBuf::from(format!("data/volve/{} time.csv", safe_name))
    });

    // Write CSV
    let outfile = std::fs::File::create(&output_path).unwrap_or_else(|e| {
        eprintln!("ERROR: Failed to create output file {}: {}", output_path.display(), e);
        std::process::exit(1);
    });
    let mut writer = BufWriter::new(outfile);

    // Header
    writeln!(writer, "{}", output_columns.join(",")).unwrap_or_else(|e| {
        eprintln!("ERROR: Failed to write header: {}", e);
        std::process::exit(1);
    });

    // Data rows
    let mut non_empty_rows = 0;
    for (_ts, row) in &all_rows {
        // Skip rows where all values except timestamp are empty
        let has_data = row.iter().skip(1).any(|v| !v.is_empty());
        if !has_data {
            continue;
        }
        non_empty_rows += 1;
        writeln!(writer, "{}", row.join(",")).unwrap_or_else(|e| {
            eprintln!("ERROR: Failed to write row: {}", e);
            std::process::exit(1);
        });
    }

    writer.flush().unwrap_or_else(|e| {
        eprintln!("ERROR: Failed to flush output: {}", e);
        std::process::exit(1);
    });

    println!("  Output: {} ({} rows with data)", output_path.display(), non_empty_rows);
    println!("Done.");
}
