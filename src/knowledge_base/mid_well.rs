//! Mid-well snapshot writer and cap enforcement

use crate::knowledge_base::compressor;
use crate::types::{
    AnalysisResult, KnowledgeBaseConfig, MidWellSnapshot, MLInsightsReport,
    SustainedStats, WitsPacket,
};
use std::io;
use tracing::{debug, info, warn};

/// Write a mid-well snapshot from a successful `MLInsightsReport`.
///
/// Only writes if the report contains successful analysis results.
pub fn write_snapshot(config: &KnowledgeBaseConfig, report: &MLInsightsReport) -> io::Result<()> {
    let insights = match &report.result {
        AnalysisResult::Success(insights) => insights,
        AnalysisResult::Failure(_) => return Ok(()), // skip failed analyses
    };

    let snapshot = MidWellSnapshot {
        timestamp: report.timestamp,
        well_id: report.well_id.clone(),
        formation_name: report.formation_type.clone(),
        depth_range: report.depth_range,
        campaign: report.campaign,
        bit_hours: report.bit_hours,
        optimal_params: insights.optimal_params.clone(),
        sample_count: insights.sample_count,
        confidence: insights.confidence,
        sustained_stats: None,
    };

    let dir = config.mid_well_dir();
    std::fs::create_dir_all(&dir)?;

    let filename = format!("snapshot_{}.toml", snapshot.timestamp);
    let path = dir.join(&filename);

    compressor::write_toml(&path, &snapshot)?;
    debug!(path = %path.display(), "Wrote mid-well snapshot");

    Ok(())
}

/// Write a mid-well snapshot with sustained-stats computed from raw packets.
pub fn write_snapshot_with_packets(
    config: &KnowledgeBaseConfig,
    report: &MLInsightsReport,
    packets: &[WitsPacket],
) -> io::Result<()> {
    let insights = match &report.result {
        AnalysisResult::Success(insights) => insights,
        AnalysisResult::Failure(_) => return Ok(()),
    };

    let sustained_stats = compute_sustained_stats(packets);

    let snapshot = MidWellSnapshot {
        timestamp: report.timestamp,
        well_id: report.well_id.clone(),
        formation_name: report.formation_type.clone(),
        depth_range: report.depth_range,
        campaign: report.campaign,
        bit_hours: report.bit_hours,
        optimal_params: insights.optimal_params.clone(),
        sample_count: insights.sample_count,
        confidence: insights.confidence,
        sustained_stats,
    };

    let dir = config.mid_well_dir();
    std::fs::create_dir_all(&dir)?;

    let filename = format!("snapshot_{}.toml", snapshot.timestamp);
    let path = dir.join(&filename);

    compressor::write_toml(&path, &snapshot)?;
    debug!(path = %path.display(), "Wrote mid-well snapshot (with sustained stats)");

    Ok(())
}

fn compute_sustained_stats(packets: &[WitsPacket]) -> Option<SustainedStats> {
    let sustained: Vec<_> = packets
        .iter()
        .filter(|p| p.seconds_since_param_change > 120)
        .collect();
    if sustained.is_empty() {
        return None;
    }
    let n = sustained.len() as f64;
    Some(SustainedStats {
        avg_rop_ft_hr: sustained.iter().map(|p| p.rop).sum::<f64>() / n,
        best_rop_ft_hr: sustained.iter().map(|p| p.rop).fold(0.0_f64, f64::max),
        avg_mse_psi: sustained.iter().map(|p| p.mse).sum::<f64>() / n,
        avg_wob_klbs: sustained.iter().map(|p| p.wob).sum::<f64>() / n,
        avg_rpm: sustained.iter().map(|p| p.rpm).sum::<f64>() / n,
        sample_count: sustained.len(),
    })
}

/// Load all mid-well snapshots (both plain .toml and compressed .toml.zst)
pub fn load_all_snapshots(config: &KnowledgeBaseConfig) -> io::Result<Vec<MidWellSnapshot>> {
    let dir = config.mid_well_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut snapshots = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if !fname.starts_with("snapshot_") {
            continue;
        }
        if !fname.ends_with(".toml") && !fname.ends_with(".toml.zst") {
            continue;
        }

        match compressor::read_toml::<MidWellSnapshot>(&path) {
            Ok(s) => snapshots.push(s),
            Err(e) => {
                warn!(path = %path.display(), error = %e, "Failed to read snapshot");
            }
        }
    }

    snapshots.sort_by_key(|s| s.timestamp);
    Ok(snapshots)
}

/// Enforce the snapshot cap:
/// - Keep newest `max_mid_well_snapshots` as plain TOML
/// - Beyond cap but within `cold_retention_days` → compress
/// - Beyond retention → delete
pub fn enforce_snapshot_cap(config: &KnowledgeBaseConfig) -> io::Result<()> {
    let dir = config.mid_well_dir();
    if !dir.exists() {
        return Ok(());
    }

    // Collect all snapshot files with their timestamps
    let mut files: Vec<(u64, std::path::PathBuf, bool)> = Vec::new(); // (timestamp, path, is_compressed)

    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();

        if !fname.starts_with("snapshot_") {
            continue;
        }

        let is_compressed = fname.ends_with(".toml.zst");
        if !fname.ends_with(".toml") && !is_compressed {
            continue;
        }

        // Extract timestamp from filename: snapshot_{timestamp}.toml[.zst]
        let ts_str = fname
            .strip_prefix("snapshot_")
            .and_then(|s| s.strip_suffix(".toml.zst").or_else(|| s.strip_suffix(".toml")));

        if let Some(ts) = ts_str.and_then(|s| s.parse::<u64>().ok()) {
            files.push((ts, path, is_compressed));
        }
    }

    if files.is_empty() {
        return Ok(());
    }

    // Sort by timestamp descending (newest first)
    files.sort_by(|a, b| b.0.cmp(&a.0));

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let retention_cutoff = now.saturating_sub(config.cold_retention_days as u64 * 86400);

    let mut compressed_count = 0u32;
    let mut deleted_count = 0u32;

    for (i, (ts, path, is_compressed)) in files.iter().enumerate() {
        if i < config.max_mid_well_snapshots {
            // Within hot cap — keep as plain TOML (already is, nothing to do)
            continue;
        }

        if *ts < retention_cutoff {
            // Beyond retention — delete
            if let Err(e) = std::fs::remove_file(path) {
                warn!(path = %path.display(), error = %e, "Failed to delete old snapshot");
            } else {
                deleted_count += 1;
            }
        } else if !is_compressed {
            // Beyond cap but within retention — compress
            if let Err(e) = compressor::compress_file(path) {
                warn!(path = %path.display(), error = %e, "Failed to compress snapshot");
            } else {
                compressed_count += 1;
            }
        }
    }

    if compressed_count > 0 || deleted_count > 0 {
        info!(compressed = compressed_count, deleted = deleted_count, "Snapshot cap enforced");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AnalysisInsights, Campaign, ConfidenceLevel, OptimalParams};

    fn make_report(ts: u64) -> MLInsightsReport {
        MLInsightsReport {
            timestamp: ts,
            campaign: Campaign::Production,
            depth_range: (1000.0, 2000.0),
            well_id: "Well-A".to_string(),
            field_name: "TestField".to_string(),
            bit_hours: 10.0,
            bit_depth: 500.0,
            formation_type: "Shallow".to_string(),
            result: AnalysisResult::Success(AnalysisInsights {
                optimal_params: OptimalParams::default(),
                correlations: Vec::new(),
                summary_text: "test".to_string(),
                confidence: ConfidenceLevel::Medium,
                sample_count: 1000,
            }),
        }
    }

    #[test]
    fn test_write_and_load_snapshot() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = KnowledgeBaseConfig {
            root: tmp.path().to_path_buf(),
            field: "TestField".to_string(),
            well: "Well-A".to_string(),
            ..Default::default()
        };
        config.ensure_dirs().expect("dirs");

        let report = make_report(1700000000);
        write_snapshot(&config, &report).expect("write");

        let snapshots = load_all_snapshots(&config).expect("load");
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].timestamp, 1700000000);
        assert_eq!(snapshots[0].formation_name, "Shallow");
    }

    #[test]
    fn test_enforce_cap_compresses_old() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = KnowledgeBaseConfig {
            root: tmp.path().to_path_buf(),
            field: "TestField".to_string(),
            well: "Well-A".to_string(),
            max_mid_well_snapshots: 2,
            cold_retention_days: 365, // long retention so nothing gets deleted
        };
        config.ensure_dirs().expect("dirs");

        // Use recent timestamps so they don't exceed retention
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Write 4 snapshots
        for i in 0..4 {
            let report = make_report(now - (3 - i) * 3600); // oldest first
            write_snapshot(&config, &report).expect("write");
        }

        enforce_snapshot_cap(&config).expect("enforce");

        // Count plain vs compressed files
        let dir = config.mid_well_dir();
        let mut plain = 0;
        let mut compressed = 0;
        for entry in std::fs::read_dir(&dir).expect("readdir") {
            let name = entry.expect("entry").file_name().into_string().unwrap_or_default();
            if name.ends_with(".toml.zst") {
                compressed += 1;
            } else if name.ends_with(".toml") {
                plain += 1;
            }
        }

        assert_eq!(plain, 2, "should keep 2 newest as plain");
        assert_eq!(compressed, 2, "should compress 2 oldest");
    }
}
