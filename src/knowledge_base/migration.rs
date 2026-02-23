//! Migration tool: converts a flat well_prognosis.toml into the knowledge base directory structure

use crate::knowledge_base::compressor;
use crate::types::{
    FieldGeology, FormationInterval, FormationPrognosis, GeologicalFormation,
    KnowledgeBaseConfig, OffsetPerformanceOverride, PostWellFormationPerformance,
    PreSpudFormation, PreSpudPrognosis,
};
use std::collections::HashMap;
use std::io;
use std::path::Path;
use tracing::info;

/// Migrate a flat `well_prognosis.toml` into the knowledge base directory structure.
///
/// Splits the file into:
/// 1. `{field}/geology.toml` — geological data
/// 2. `{field}/wells/{well}/pre-spud/prognosis.toml` — engineering parameters
/// 3. `{field}/wells/{offset_well}/post-well/performance_{formation}.toml` — offset data
pub fn migrate_flat_to_kb(flat_path: &Path, kb_root: &Path) -> io::Result<()> {
    let content = std::fs::read_to_string(flat_path)?;
    let prognosis: FormationPrognosis = toml::from_str(&content).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("Failed to parse prognosis: {}", e))
    })?;

    let field = &prognosis.well.field;
    let well = &prognosis.well.name;

    info!(field = field, well = well, "Migrating flat prognosis to knowledge base");

    let config = KnowledgeBaseConfig {
        root: kb_root.to_path_buf(),
        field: field.clone(),
        well: well.clone(),
        ..Default::default()
    };
    config.ensure_dirs()?;

    // 1. Extract geology
    let geology = FieldGeology {
        field: field.clone(),
        formations: prognosis.formations.iter().map(|f| GeologicalFormation {
            name: f.name.clone(),
            depth_top_ft: f.depth_top_ft,
            depth_base_ft: f.depth_base_ft,
            lithology: f.lithology.clone(),
            hardness: f.hardness,
            drillability: f.drillability.clone(),
            pore_pressure_ppg: f.pore_pressure_ppg,
            fracture_gradient_ppg: f.fracture_gradient_ppg,
            hazards: f.hazards.clone(),
        }).collect(),
    };
    compressor::write_toml(&config.geology_path(), &geology)?;
    info!(formations = geology.formations.len(), "Wrote geology.toml");

    // 2. Extract pre-spud prognosis
    let pre_spud = PreSpudPrognosis {
        well: prognosis.well.clone(),
        formations: prognosis.formations.iter().map(|f| {
            let has_offset = !f.offset_performance.wells.is_empty();
            PreSpudFormation {
                name: f.name.clone(),
                depth_top_ft: None, // same as geology
                depth_base_ft: None,
                parameters: f.parameters.clone(),
                manual_offset: if has_offset {
                    Some(OffsetPerformanceOverride {
                        wells: f.offset_performance.wells.clone(),
                        avg_rop_ft_hr: f.offset_performance.avg_rop_ft_hr,
                        best_rop_ft_hr: f.offset_performance.best_rop_ft_hr,
                        avg_mse_psi: f.offset_performance.avg_mse_psi,
                        best_params: f.offset_performance.best_params.clone(),
                        notes: f.offset_performance.notes.clone(),
                    })
                } else {
                    None
                },
            }
        }).collect(),
        casings: prognosis.casings.clone(),
    };
    compressor::write_toml(&config.pre_spud_path(well), &pre_spud)?;
    info!("Wrote pre-spud prognosis");

    // 3. Extract offset performance into per-well post-well files
    let mut offset_wells: HashMap<String, Vec<(&FormationInterval, &str)>> = HashMap::new();
    for formation in &prognosis.formations {
        for offset_well in &formation.offset_performance.wells {
            offset_wells
                .entry(offset_well.clone())
                .or_default()
                .push((formation, offset_well));
        }
    }

    for (offset_well_name, formation_refs) in &offset_wells {
        let post_dir = config.post_well_dir(offset_well_name);
        std::fs::create_dir_all(&post_dir)?;

        for (formation, _) in formation_refs {
            // Since offset data in the flat file is aggregated across wells,
            // each offset well gets a copy of the shared performance data
            let perf = PostWellFormationPerformance {
                well_id: offset_well_name.clone(),
                field: field.clone(),
                formation_name: formation.name.clone(),
                depth_top_ft: formation.depth_top_ft,
                depth_base_ft: formation.depth_base_ft,
                avg_rop_ft_hr: formation.offset_performance.avg_rop_ft_hr,
                best_rop_ft_hr: formation.offset_performance.best_rop_ft_hr,
                avg_mse_psi: formation.offset_performance.avg_mse_psi,
                best_params: formation.offset_performance.best_params.clone(),
                avg_wob_range: formation.parameters.wob_klbs.clone(),
                avg_rpm_range: formation.parameters.rpm.clone(),
                avg_flow_range: formation.parameters.flow_gpm.clone(),
                total_snapshots: 100, // synthetic default
                avg_confidence: 0.7,
                avg_stability: 0.8,
                notes: formation.offset_performance.notes.clone(),
                completed_timestamp: 0,
                sustained_only: None,
            };

            let safe_name = formation.name.replace(' ', "_").replace(['/', '\\', '(', ')'], "");
            let filename = format!("performance_{}.toml", safe_name);
            compressor::write_toml(&post_dir.join(&filename), &perf)?;
        }

        info!(well = offset_well_name, formations = formation_refs.len(), "Wrote offset well performance files");
    }

    info!(
        geology = geology.formations.len(),
        offset_wells = offset_wells.len(),
        "Migration complete"
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge_base::assembler;

    #[test]
    fn test_migrate_volve_prognosis() {
        let flat_path = std::path::Path::new("data/volve/well_prognosis.toml");
        if !flat_path.exists() {
            // Skip if test data not available
            return;
        }

        let tmp = tempfile::tempdir().expect("tempdir");
        migrate_flat_to_kb(flat_path, tmp.path()).expect("migration");

        // Verify structure was created
        assert!(tmp.path().join("Volve/geology.toml").exists());
        assert!(tmp.path().join("Volve/wells/F-15B/pre-spud/prognosis.toml").exists());

        // Verify offset wells were created
        assert!(tmp.path().join("Volve/wells/F-9A/post-well").exists());
        assert!(tmp.path().join("Volve/wells/F-12/post-well").exists());

        // Verify assembled output matches expectations
        let config = KnowledgeBaseConfig {
            root: tmp.path().to_path_buf(),
            field: "Volve".to_string(),
            well: "F-15B".to_string(),
            ..Default::default()
        };
        config.ensure_dirs().expect("dirs");

        let assembled = assembler::assemble_prognosis(&config).expect("assemble");
        assert_eq!(assembled.formations.len(), 5);
        assert_eq!(assembled.well.name, "F-15B");
        assert_eq!(assembled.well.field, "Volve");

        // Verify offset data was reconstituted
        let nordland = assembled.formations.iter().find(|f| f.name == "Nordland Group").expect("Nordland");
        assert!(!nordland.offset_performance.wells.is_empty());
        assert!(nordland.offset_performance.avg_rop_ft_hr > 0.0);
    }
}
