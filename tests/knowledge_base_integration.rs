//! Knowledge Base Integration Tests
//!
//! Tests the full KB lifecycle: migrate flat TOML → assemble prognosis → write snapshots → generate post-well

#![cfg(feature = "knowledge-base")]

use sairen_os::knowledge_base::{assembler, migration, post_well};
use sairen_os::types::KnowledgeBaseConfig;
use std::path::PathBuf;
use tempfile::TempDir;

fn volve_prognosis_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/volve/well_prognosis.toml")
}

/// Migrate the real Volve well_prognosis.toml, then assemble and verify the result
/// matches the original flat prognosis.
#[test]
fn test_migrate_and_assemble_volve() {
    let tmp = TempDir::new().unwrap();
    let kb_root = tmp.path();

    // Migrate
    migration::migrate_flat_to_kb(&volve_prognosis_path(), kb_root).unwrap();

    // Verify directory structure exists
    assert!(kb_root.join("Volve/geology.toml").exists());
    assert!(kb_root.join("Volve/wells/F-15B/pre-spud/prognosis.toml").exists());

    // Assemble
    let config = KnowledgeBaseConfig {
        root: kb_root.to_path_buf(),
        field: "Volve".to_string(),
        well: "F-15B".to_string(),
        max_mid_well_snapshots: 168,
        cold_retention_days: 30,
    };

    let prognosis = assembler::assemble_prognosis(&config)
        .expect("Should assemble prognosis from migrated data");

    // The Volve prognosis has 5 formations
    assert_eq!(prognosis.formations.len(), 5, "Expected 5 formations");

    // Check they're sorted by depth
    for w in prognosis.formations.windows(2) {
        assert!(
            w[0].depth_top_ft <= w[1].depth_top_ft,
            "Formations should be sorted by depth_top_ft"
        );
    }

    // Verify first formation
    let nordland = &prognosis.formations[0];
    assert_eq!(nordland.name, "Nordland Group");
    assert_eq!(nordland.depth_top_ft, 0.0);
    assert_eq!(nordland.depth_base_ft, 3200.0);
    assert_eq!(nordland.lithology, "clay, silt");

    // Verify Hugin formation
    let hugin = prognosis.formations.iter().find(|f| f.name == "Hugin Formation").unwrap();
    assert_eq!(hugin.depth_top_ft, 9200.0);
    assert_eq!(hugin.depth_base_ft, 10200.0);

    // Verify last formation (Sleipner)
    let sleipner = prognosis.formations.last().unwrap();
    assert_eq!(sleipner.name, "Sleipner Formation (Reservoir)");
    assert_eq!(sleipner.depth_top_ft, 10200.0);
    assert_eq!(sleipner.depth_base_ft, 11800.0);
}

/// Test that sibling well offset data is picked up during assembly
#[test]
fn test_offset_well_assembly() {
    let tmp = TempDir::new().unwrap();
    let kb_root = tmp.path();

    // Migrate Volve data
    migration::migrate_flat_to_kb(&volve_prognosis_path(), kb_root).unwrap();

    // Simulate an offset well by writing post-well performance data
    let offset_well_dir = kb_root.join("Volve/wells/F-14/post-well");
    std::fs::create_dir_all(&offset_well_dir).unwrap();

    let offset_perf = sairen_os::types::PostWellFormationPerformance {
        well_id: "F-14".to_string(),
        field: "Volve".to_string(),
        formation_name: "Hugin Formation".to_string(),
        depth_top_ft: 11200.0,
        depth_base_ft: 11800.0,
        avg_rop_ft_hr: 55.0,
        best_rop_ft_hr: 80.0,
        avg_mse_psi: 30000.0,
        best_params: sairen_os::types::BestParams {
            wob_klbs: 30.0,
            rpm: 100.0,
        },
        avg_wob_range: sairen_os::types::ParameterRange { min: 20.0, optimal: 30.0, max: 40.0 },
        avg_rpm_range: sairen_os::types::ParameterRange { min: 60.0, optimal: 100.0, max: 120.0 },
        avg_flow_range: sairen_os::types::ParameterRange { min: 450.0, optimal: 550.0, max: 650.0 },
        total_snapshots: 50,
        avg_confidence: 0.85,
        avg_stability: 0.9,
        notes: "Good run through Hugin".to_string(),
        completed_timestamp: 1700000000,
        sustained_only: None,
    };

    let perf_path = offset_well_dir.join("performance_Hugin_Formation.toml");
    let toml_str = toml::to_string_pretty(&offset_perf).unwrap();
    std::fs::write(&perf_path, toml_str).unwrap();

    // Assemble with offset data
    let config = KnowledgeBaseConfig {
        root: kb_root.to_path_buf(),
        field: "Volve".to_string(),
        well: "F-15B".to_string(),
        max_mid_well_snapshots: 168,
        cold_retention_days: 30,
    };

    let prognosis = assembler::assemble_prognosis(&config).unwrap();

    // Check that Hugin formation has offset performance
    let hugin = prognosis.formations.iter().find(|f| f.name == "Hugin Formation").unwrap();
    let offset = &hugin.offset_performance;
    // F-14 should be among the offset wells (alongside any from migration)
    assert!(offset.wells.contains(&"F-14".to_string()),
        "F-14 should appear in offset wells, got: {:?}", offset.wells);
    // The aggregated values should include F-14's data
    assert!(offset.best_rop_ft_hr >= 80.0,
        "best_rop should be at least 80.0 from F-14, got: {}", offset.best_rop_ft_hr);
}

/// Full lifecycle: migrate → write snapshots → generate post-well → re-assemble with offset
#[test]
fn test_full_knowledge_base_lifecycle() {
    use sairen_os::types::{Campaign, ConfidenceLevel, MidWellSnapshot, OptimalParams};

    let tmp = TempDir::new().unwrap();
    let kb_root = tmp.path();

    // Migrate Volve data for well F-15B
    migration::migrate_flat_to_kb(&volve_prognosis_path(), kb_root).unwrap();

    let config = KnowledgeBaseConfig {
        root: kb_root.to_path_buf(),
        field: "Volve".to_string(),
        well: "F-15B".to_string(),
        max_mid_well_snapshots: 168,
        cold_retention_days: 30,
    };

    // Ensure dirs
    config.ensure_dirs().unwrap();

    // Write some mid-well snapshots
    for i in 0..5u64 {
        let snapshot = MidWellSnapshot {
            timestamp: 1700000000 + i * 3600,
            well_id: "F-15B".to_string(),
            formation_name: "Hugin Formation".to_string(),
            depth_range: (11200.0 + i as f64 * 100.0, 11300.0 + i as f64 * 100.0),
            campaign: Campaign::Production,
            bit_hours: 1.0,
            optimal_params: OptimalParams {
                best_wob: 28.0 + i as f64,
                best_rpm: 95.0 + i as f64 * 2.0,
                best_flow: 540.0 + i as f64 * 5.0,
                achieved_rop: 50.0 + i as f64 * 5.0,
                achieved_mse: 32000.0 - i as f64 * 1000.0,
                ..Default::default()
            },
            sample_count: 100 + i as usize * 10,
            confidence: ConfidenceLevel::High,
            sustained_stats: None,
        };
        let snapshot_path = config.mid_well_dir()
            .join(format!("snapshot_{}.toml", snapshot.timestamp));
        let toml_str = toml::to_string_pretty(&snapshot).unwrap();
        std::fs::write(&snapshot_path, toml_str).unwrap();
    }

    // Generate post-well summary
    let summary = post_well::generate_post_well(&config).unwrap();
    assert_eq!(summary.well_id, "F-15B");
    assert_eq!(summary.field, "Volve");
    assert!(!summary.formations.is_empty(), "Should have at least one formation performance");

    // Verify per-formation performance file was written
    let hugin_perf_path = config.post_well_dir("F-15B").join("performance_Hugin_Formation.toml");
    assert!(hugin_perf_path.exists(), "Per-formation performance file should exist");

    // Now create a new well config and verify offset data appears
    let config_new_well = KnowledgeBaseConfig {
        root: kb_root.to_path_buf(),
        field: "Volve".to_string(),
        well: "F-16".to_string(),
        max_mid_well_snapshots: 168,
        cold_retention_days: 30,
    };

    // Create pre-spud dir for F-16 (minimal — just needs to exist for assembly)
    std::fs::create_dir_all(config_new_well.pre_spud_dir("F-16")).unwrap();

    let prognosis = assembler::assemble_prognosis(&config_new_well).unwrap();

    // F-16 should see F-15B's Hugin performance as offset data
    let hugin = prognosis.formations.iter().find(|f| f.name == "Hugin Formation").unwrap();
    let offset = &hugin.offset_performance;
    assert!(!offset.wells.is_empty(), "F-16 should see F-15B's Hugin data as offset");
    assert!(offset.wells.contains(&"F-15B".to_string()));
    assert!(offset.avg_rop_ft_hr > 0.0);
    assert!(offset.best_rop_ft_hr > 0.0);
}
