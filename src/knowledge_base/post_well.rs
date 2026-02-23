//! Post-well summary generator: aggregates mid-well snapshots into performance files

use crate::knowledge_base::{compressor, mid_well};
use crate::types::{
    BestParams, KnowledgeBaseConfig, MidWellSnapshot, ParameterRange,
    PostWellFormationPerformance, PostWellSummary, SustainedFormationStats,
};
use std::collections::HashMap;
use std::io;
use tracing::info;

/// Generate post-well files from all mid-well snapshots.
///
/// Groups snapshots by formation, aggregates per-formation performance,
/// writes individual performance files and an overall summary.
pub fn generate_post_well(config: &KnowledgeBaseConfig) -> io::Result<PostWellSummary> {
    let snapshots = mid_well::load_all_snapshots(config)?;

    if snapshots.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "No mid-well snapshots found to generate post-well summary",
        ));
    }

    // Group snapshots by formation name
    let mut by_formation: HashMap<String, Vec<&MidWellSnapshot>> = HashMap::new();
    for snapshot in &snapshots {
        by_formation
            .entry(snapshot.formation_name.clone())
            .or_default()
            .push(snapshot);
    }

    let post_dir = config.post_well_dir(&config.well);
    std::fs::create_dir_all(&post_dir)?;

    let mut formations = Vec::new();
    let mut total_bit_hours = 0.0_f64;
    let mut max_depth = 0.0_f64;

    for (name, group) in &by_formation {
        let perf = aggregate_formation(config, name, group);
        total_bit_hours += group.iter().map(|s| s.bit_hours).sum::<f64>() / group.len().max(1) as f64;
        max_depth = max_depth.max(perf.depth_base_ft);

        // Write per-formation performance file
        let safe_name = name.replace(' ', "_").replace(['/', '\\', '(', ')'], "");
        let filename = format!("performance_{}.toml", safe_name);
        compressor::write_toml(&post_dir.join(&filename), &perf)?;
        info!(formation = name, file = &filename, "Wrote post-well performance");

        formations.push(perf);
    }

    // Sort formations by depth
    formations.sort_by(|a, b| {
        a.depth_top_ft.partial_cmp(&b.depth_top_ft).unwrap_or(std::cmp::Ordering::Equal)
    });

    let summary = PostWellSummary {
        well_id: config.well.clone(),
        field: config.field.clone(),
        completion_date: chrono::Utc::now().format("%Y-%m-%d").to_string(),
        total_depth_ft: max_depth,
        total_bit_hours,
        formations,
    };

    // Write summary
    compressor::write_toml(&post_dir.join("summary.toml"), &summary)?;
    info!(
        well = &config.well,
        formations = summary.formations.len(),
        "Wrote post-well summary"
    );

    // Compress all mid-well snapshots (well is done, they're cold now)
    let mid_dir = config.mid_well_dir();
    if mid_dir.exists() {
        for entry in std::fs::read_dir(&mid_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                let _ = compressor::compress_file(&path);
            }
        }
    }

    // Compress pre-spud directory (also cold now)
    let pre_spud = config.pre_spud_dir(&config.well);
    if pre_spud.exists() {
        for entry in std::fs::read_dir(&pre_spud)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                let _ = compressor::compress_file(&path);
            }
        }
    }

    Ok(summary)
}

/// Aggregate snapshots for a single formation into `PostWellFormationPerformance`
fn aggregate_formation(
    config: &KnowledgeBaseConfig,
    name: &str,
    snapshots: &[&MidWellSnapshot],
) -> PostWellFormationPerformance {
    let n = snapshots.len().max(1) as f64;

    let avg_rop = snapshots.iter().map(|s| s.optimal_params.achieved_rop).sum::<f64>() / n;
    let best_rop = snapshots.iter()
        .map(|s| s.optimal_params.achieved_rop)
        .fold(0.0_f64, f64::max);
    let avg_mse = snapshots.iter().map(|s| s.optimal_params.achieved_mse).sum::<f64>() / n;

    // Best params from snapshot with highest achieved ROP
    let best_snapshot = snapshots.iter()
        .max_by(|a, b| {
            a.optimal_params.achieved_rop
                .partial_cmp(&b.optimal_params.achieved_rop)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

    let best_params = best_snapshot.map(|s| BestParams {
        wob_klbs: s.optimal_params.best_wob,
        rpm: s.optimal_params.best_rpm,
    }).unwrap_or(BestParams { wob_klbs: 0.0, rpm: 0.0 });

    // Average ranges across snapshots
    let avg_wob_range = ParameterRange {
        min: snapshots.iter().map(|s| s.optimal_params.wob_min).sum::<f64>() / n,
        optimal: snapshots.iter().map(|s| s.optimal_params.best_wob).sum::<f64>() / n,
        max: snapshots.iter().map(|s| s.optimal_params.wob_max).sum::<f64>() / n,
    };
    let avg_rpm_range = ParameterRange {
        min: snapshots.iter().map(|s| s.optimal_params.rpm_min).sum::<f64>() / n,
        optimal: snapshots.iter().map(|s| s.optimal_params.best_rpm).sum::<f64>() / n,
        max: snapshots.iter().map(|s| s.optimal_params.rpm_max).sum::<f64>() / n,
    };
    let avg_flow_range = ParameterRange {
        min: snapshots.iter().map(|s| s.optimal_params.flow_min).sum::<f64>() / n,
        optimal: snapshots.iter().map(|s| s.optimal_params.best_flow).sum::<f64>() / n,
        max: snapshots.iter().map(|s| s.optimal_params.flow_max).sum::<f64>() / n,
    };

    let avg_confidence = snapshots.iter()
        .map(|s| match s.confidence {
            crate::types::ConfidenceLevel::High => 1.0,
            crate::types::ConfidenceLevel::Medium => 0.7,
            crate::types::ConfidenceLevel::Low => 0.4,
            crate::types::ConfidenceLevel::Insufficient => 0.1,
        })
        .sum::<f64>() / n;

    let avg_stability = snapshots.iter()
        .map(|s| s.optimal_params.stability_score)
        .sum::<f64>() / n;

    // Depth range from snapshots
    let depth_top = snapshots.iter().map(|s| s.depth_range.0).fold(f64::MAX, f64::min);
    let depth_base = snapshots.iter().map(|s| s.depth_range.1).fold(0.0_f64, f64::max);

    let total_snapshots: usize = snapshots.iter().map(|s| s.sample_count).sum();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Aggregate sustained-only stats from snapshots that have them
    let sustained_snapshots: Vec<_> = snapshots
        .iter()
        .filter_map(|s| s.sustained_stats.as_ref())
        .collect();

    let sustained_only = if !sustained_snapshots.is_empty() {
        let sn = sustained_snapshots.len() as f64;
        let total_sustained_samples: usize =
            sustained_snapshots.iter().map(|s| s.sample_count).sum();
        Some(SustainedFormationStats {
            avg_rop_ft_hr: sustained_snapshots.iter().map(|s| s.avg_rop_ft_hr).sum::<f64>() / sn,
            best_rop_ft_hr: sustained_snapshots
                .iter()
                .map(|s| s.best_rop_ft_hr)
                .fold(0.0_f64, f64::max),
            avg_mse_psi: sustained_snapshots.iter().map(|s| s.avg_mse_psi).sum::<f64>() / sn,
            avg_wob_klbs: sustained_snapshots.iter().map(|s| s.avg_wob_klbs).sum::<f64>() / sn,
            avg_rpm: sustained_snapshots.iter().map(|s| s.avg_rpm).sum::<f64>() / sn,
            total_sustained_samples,
            low_sample_count: total_sustained_samples < 30,
        })
    } else {
        None
    };

    PostWellFormationPerformance {
        well_id: config.well.clone(),
        field: config.field.clone(),
        formation_name: name.to_string(),
        depth_top_ft: depth_top,
        depth_base_ft: depth_base,
        avg_rop_ft_hr: avg_rop,
        best_rop_ft_hr: best_rop,
        avg_mse_psi: avg_mse,
        best_params,
        avg_wob_range,
        avg_rpm_range,
        avg_flow_range,
        total_snapshots,
        avg_confidence,
        avg_stability,
        notes: String::new(),
        completed_timestamp: now,
        sustained_only,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge_base::mid_well::write_snapshot;
    use crate::types::{
        AnalysisInsights, AnalysisResult, Campaign, ConfidenceLevel, MLInsightsReport,
        OptimalParams,
    };

    fn make_report(ts: u64, formation: &str, rop: f64) -> MLInsightsReport {
        MLInsightsReport {
            timestamp: ts,
            campaign: Campaign::Production,
            depth_range: (1000.0, 2000.0),
            well_id: "Well-A".to_string(),
            field_name: "TestField".to_string(),
            bit_hours: 10.0,
            bit_depth: 500.0,
            formation_type: formation.to_string(),
            result: AnalysisResult::Success(AnalysisInsights {
                optimal_params: OptimalParams {
                    achieved_rop: rop,
                    achieved_mse: 15000.0,
                    best_wob: 25.0,
                    best_rpm: 110.0,
                    best_flow: 500.0,
                    ..Default::default()
                },
                correlations: Vec::new(),
                summary_text: "test".to_string(),
                confidence: ConfidenceLevel::Medium,
                sample_count: 1000,
            }),
        }
    }

    #[test]
    fn test_generate_post_well() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = KnowledgeBaseConfig {
            root: tmp.path().to_path_buf(),
            field: "TestField".to_string(),
            well: "Well-A".to_string(),
            ..Default::default()
        };
        config.ensure_dirs().expect("dirs");

        // Write some snapshots
        write_snapshot(&config, &make_report(1700000000, "Shallow", 100.0)).expect("write");
        write_snapshot(&config, &make_report(1700003600, "Shallow", 120.0)).expect("write");
        write_snapshot(&config, &make_report(1700007200, "Deep", 50.0)).expect("write");

        let summary = generate_post_well(&config).expect("generate");
        assert_eq!(summary.formations.len(), 2);
        assert_eq!(summary.well_id, "Well-A");
        assert_eq!(summary.field, "TestField");

        // Check the Shallow formation (avg of 100 and 120)
        let shallow = summary.formations.iter().find(|f| f.formation_name == "Shallow").expect("shallow");
        assert!((shallow.avg_rop_ft_hr - 110.0).abs() < 0.01);
        assert!((shallow.best_rop_ft_hr - 120.0).abs() < 0.01);

        // Verify files were written
        let post_dir = config.post_well_dir("Well-A");
        assert!(post_dir.join("performance_Shallow.toml").exists());
        assert!(post_dir.join("performance_Deep.toml").exists());
        assert!(post_dir.join("summary.toml").exists());
    }
}
