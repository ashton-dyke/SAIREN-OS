//! Assembler: merges geology + pre-spud + offset wells → FormationPrognosis

use crate::knowledge_base::compressor;
use crate::types::{
    BestParams, FieldGeology, FormationInterval, FormationParameters,
    FormationPrognosis, KnowledgeBaseConfig, OffsetPerformance, ParameterRange,
    PostWellFormationPerformance, PreSpudPrognosis, PrognosisWellInfo,
};
use tracing::{debug, info, warn};

/// Assemble a `FormationPrognosis` from the knowledge base directory.
///
/// Returns `None` if no geology file exists (minimum requirement).
pub fn assemble_prognosis(config: &KnowledgeBaseConfig) -> Option<FormationPrognosis> {
    // 1. Load field geology
    let geology_path = config.geology_path();
    let geology: FieldGeology = match compressor::read_toml(&geology_path) {
        Ok(g) => g,
        Err(e) => {
            debug!(path = %geology_path.display(), error = %e, "No geology file found");
            return None;
        }
    };
    info!(field = %geology.field, formations = geology.formations.len(), "Loaded field geology");

    // 2. Load pre-spud (optional)
    let pre_spud_path = config.pre_spud_path(&config.well);
    let pre_spud: Option<PreSpudPrognosis> = compressor::read_toml(&pre_spud_path).ok();
    if let Some(ref ps) = pre_spud {
        info!(well = %ps.well.name, formations = ps.formations.len(), "Loaded pre-spud prognosis");
    }

    // 3. Collect sibling wells' post-well performance
    let siblings = config.list_sibling_wells().unwrap_or_default();

    // 4. Build formation intervals
    let mut formations: Vec<FormationInterval> = Vec::with_capacity(geology.formations.len());

    for geo in &geology.formations {
        // Find matching pre-spud formation
        let pre_spud_fm = pre_spud.as_ref().and_then(|ps| {
            ps.formations.iter().find(|f| f.name == geo.name)
        });

        // Apply depth overrides from pre-spud
        let depth_top = pre_spud_fm.and_then(|f| f.depth_top_ft).unwrap_or(geo.depth_top_ft);
        let depth_base = pre_spud_fm.and_then(|f| f.depth_base_ft).unwrap_or(geo.depth_base_ft);

        // Parameters: from pre-spud or derived from hardness
        let parameters = match pre_spud_fm {
            Some(f) => f.parameters.clone(),
            None => derive_default_parameters(geo.hardness),
        };

        // Scan offset wells for this formation
        let offset_data = collect_offset_data(config, &siblings, &geo.name);

        // Build offset performance
        let offset_performance = if !offset_data.is_empty() {
            aggregate_offset_performance(&offset_data)
        } else if let Some(manual) = pre_spud_fm.and_then(|f| f.manual_offset.as_ref()) {
            // Fall back to manual offset from pre-spud
            OffsetPerformance {
                wells: manual.wells.clone(),
                avg_rop_ft_hr: manual.avg_rop_ft_hr,
                best_rop_ft_hr: manual.best_rop_ft_hr,
                avg_mse_psi: manual.avg_mse_psi,
                best_params: manual.best_params.clone(),
                notes: manual.notes.clone(),
            }
        } else {
            // No offset data at all
            OffsetPerformance {
                wells: Vec::new(),
                avg_rop_ft_hr: 0.0,
                best_rop_ft_hr: 0.0,
                avg_mse_psi: 0.0,
                best_params: BestParams { wob_klbs: parameters.wob_klbs.optimal, rpm: parameters.rpm.optimal },
                notes: String::new(),
            }
        };

        formations.push(FormationInterval {
            name: geo.name.clone(),
            depth_top_ft: depth_top,
            depth_base_ft: depth_base,
            lithology: geo.lithology.clone(),
            hardness: geo.hardness,
            drillability: geo.drillability.clone(),
            pore_pressure_ppg: geo.pore_pressure_ppg,
            fracture_gradient_ppg: geo.fracture_gradient_ppg,
            hazards: geo.hazards.clone(),
            parameters,
            offset_performance,
        });
    }

    // 5. Sort by depth
    formations.sort_by(|a, b| {
        a.depth_top_ft.partial_cmp(&b.depth_top_ft).unwrap_or(std::cmp::Ordering::Equal)
    });

    // Well info from pre-spud or defaults from geology
    let well_info = pre_spud.as_ref().map(|ps| ps.well.clone()).unwrap_or_else(|| {
        PrognosisWellInfo {
            name: config.well.clone(),
            field: config.field.clone(),
            spud_date: String::new(),
            target_depth_ft: formations.last().map(|f| f.depth_base_ft).unwrap_or(0.0),
            coordinate_system: String::new(),
        }
    });

    let casings = pre_spud.map(|ps| ps.casings).unwrap_or_default();

    info!(
        formations = formations.len(),
        siblings = siblings.len(),
        "Assembled formation prognosis from knowledge base"
    );

    Some(FormationPrognosis {
        well: well_info,
        formations,
        casings,
    })
}

/// Derive default drilling parameters from formation hardness (0-10 scale)
fn derive_default_parameters(hardness: f64) -> FormationParameters {
    if hardness < 3.5 {
        // Soft
        FormationParameters {
            wob_klbs: ParameterRange { min: 5.0, optimal: 10.0, max: 15.0 },
            rpm: ParameterRange { min: 80.0, optimal: 120.0, max: 160.0 },
            flow_gpm: ParameterRange { min: 400.0, optimal: 500.0, max: 600.0 },
            mud_weight_ppg: 9.0,
            bit_type: "PDC".to_string(),
        }
    } else if hardness < 6.0 {
        // Medium
        FormationParameters {
            wob_klbs: ParameterRange { min: 15.0, optimal: 25.0, max: 35.0 },
            rpm: ParameterRange { min: 80.0, optimal: 110.0, max: 140.0 },
            flow_gpm: ParameterRange { min: 450.0, optimal: 520.0, max: 600.0 },
            mud_weight_ppg: 10.0,
            bit_type: "PDC".to_string(),
        }
    } else {
        // Hard
        FormationParameters {
            wob_klbs: ParameterRange { min: 20.0, optimal: 30.0, max: 40.0 },
            rpm: ParameterRange { min: 60.0, optimal: 90.0, max: 120.0 },
            flow_gpm: ParameterRange { min: 500.0, optimal: 550.0, max: 650.0 },
            mud_weight_ppg: 11.0,
            bit_type: "PDC".to_string(),
        }
    }
}

/// Collect all post-well performance data for a formation from sibling wells
fn collect_offset_data(
    config: &KnowledgeBaseConfig,
    siblings: &[String],
    formation_name: &str,
) -> Vec<PostWellFormationPerformance> {
    let mut results = Vec::new();

    // Sanitize formation name for filename matching
    let safe_name = formation_name.replace(' ', "_").replace(['/', '\\', '(', ')'], "");

    for well in siblings {
        let perf_files = match config.list_post_well_performance(well) {
            Ok(f) => f,
            Err(_) => continue,
        };

        for path in perf_files {
            let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // Match performance_{formation_name}.toml or .toml.zst
            let expected_prefix = format!("performance_{}", safe_name);
            if !fname.starts_with(&expected_prefix) {
                continue;
            }

            match compressor::read_toml::<PostWellFormationPerformance>(&path) {
                Ok(perf) => results.push(perf),
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "Failed to read offset performance file");
                }
            }
        }
    }

    results
}

/// Aggregate multiple offset well performance records into a single OffsetPerformance
fn aggregate_offset_performance(data: &[PostWellFormationPerformance]) -> OffsetPerformance {
    if data.is_empty() {
        return OffsetPerformance {
            wells: Vec::new(),
            avg_rop_ft_hr: 0.0,
            best_rop_ft_hr: 0.0,
            avg_mse_psi: 0.0,
            best_params: BestParams { wob_klbs: 0.0, rpm: 0.0 },
            notes: String::new(),
        };
    }

    let total_snapshots: usize = data.iter().map(|d| d.total_snapshots).sum();
    let total_snapshots_f = total_snapshots.max(1) as f64;

    // Weighted average by total_snapshots
    let avg_rop = data.iter()
        .map(|d| d.avg_rop_ft_hr * d.total_snapshots as f64)
        .sum::<f64>() / total_snapshots_f;

    let best_rop = data.iter()
        .map(|d| d.best_rop_ft_hr)
        .fold(0.0_f64, f64::max);

    let avg_mse = data.iter()
        .map(|d| d.avg_mse_psi * d.total_snapshots as f64)
        .sum::<f64>() / total_snapshots_f;

    // Best params from the well with highest best_rop
    let best_well = data.iter()
        .max_by(|a, b| a.best_rop_ft_hr.partial_cmp(&b.best_rop_ft_hr).unwrap_or(std::cmp::Ordering::Equal))
        .expect("data is non-empty");

    let wells: Vec<String> = data.iter().map(|d| d.well_id.clone()).collect();

    // Collect unique notes
    let mut notes_set: Vec<&str> = data.iter()
        .map(|d| d.notes.as_str())
        .filter(|n| !n.is_empty())
        .collect();
    notes_set.dedup();
    let notes = notes_set.join(" | ");

    OffsetPerformance {
        wells,
        avg_rop_ft_hr: avg_rop,
        best_rop_ft_hr: best_rop,
        avg_mse_psi: avg_mse,
        best_params: best_well.best_params.clone(),
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge_base::compressor::write_toml;
    use crate::types::{FieldGeology, GeologicalFormation};

    fn make_geology() -> FieldGeology {
        FieldGeology {
            field: "TestField".to_string(),
            formations: vec![
                GeologicalFormation {
                    name: "Shallow".to_string(),
                    depth_top_ft: 0.0,
                    depth_base_ft: 3000.0,
                    lithology: "clay".to_string(),
                    hardness: 2.0,
                    drillability: "soft".to_string(),
                    pore_pressure_ppg: 8.6,
                    fracture_gradient_ppg: 12.5,
                    hazards: vec!["shallow gas".to_string()],
                },
                GeologicalFormation {
                    name: "Deep".to_string(),
                    depth_top_ft: 3000.0,
                    depth_base_ft: 6000.0,
                    lithology: "sandstone".to_string(),
                    hardness: 7.0,
                    drillability: "hard".to_string(),
                    pore_pressure_ppg: 10.5,
                    fracture_gradient_ppg: 15.0,
                    hazards: Vec::new(),
                },
            ],
        }
    }

    #[test]
    fn test_assemble_geology_only() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = KnowledgeBaseConfig {
            root: tmp.path().to_path_buf(),
            field: "TestField".to_string(),
            well: "Well-A".to_string(),
            ..Default::default()
        };
        config.ensure_dirs().expect("dirs");

        let geology = make_geology();
        write_toml(&config.geology_path(), &geology).expect("write geology");

        let prognosis = assemble_prognosis(&config).expect("should assemble");
        assert_eq!(prognosis.formations.len(), 2);
        assert_eq!(prognosis.formations[0].name, "Shallow");
        assert_eq!(prognosis.formations[1].name, "Deep");

        // Soft formation should get default soft parameters
        assert!(prognosis.formations[0].parameters.wob_klbs.max <= 15.0);
        // Hard formation should get default hard parameters
        assert!(prognosis.formations[1].parameters.wob_klbs.min >= 20.0);
    }

    #[test]
    fn test_assemble_no_geology_returns_none() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = KnowledgeBaseConfig {
            root: tmp.path().to_path_buf(),
            field: "Empty".to_string(),
            well: "Well-A".to_string(),
            ..Default::default()
        };

        assert!(assemble_prognosis(&config).is_none());
    }

    #[test]
    fn test_assemble_with_offset_data() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = KnowledgeBaseConfig {
            root: tmp.path().to_path_buf(),
            field: "TestField".to_string(),
            well: "Well-A".to_string(),
            ..Default::default()
        };
        config.ensure_dirs().expect("dirs");

        let geology = make_geology();
        write_toml(&config.geology_path(), &geology).expect("write geology");

        // Create sibling well with post-well performance
        let sibling_post = config.post_well_dir("Well-B");
        std::fs::create_dir_all(&sibling_post).expect("mkdir sibling");

        let perf = PostWellFormationPerformance {
            well_id: "Well-B".to_string(),
            field: "TestField".to_string(),
            formation_name: "Shallow".to_string(),
            depth_top_ft: 0.0,
            depth_base_ft: 3000.0,
            avg_rop_ft_hr: 100.0,
            best_rop_ft_hr: 150.0,
            avg_mse_psi: 10000.0,
            best_params: BestParams { wob_klbs: 12.0, rpm: 130.0 },
            avg_wob_range: ParameterRange { min: 5.0, optimal: 12.0, max: 15.0 },
            avg_rpm_range: ParameterRange { min: 80.0, optimal: 130.0, max: 160.0 },
            avg_flow_range: ParameterRange { min: 400.0, optimal: 500.0, max: 600.0 },
            total_snapshots: 50,
            avg_confidence: 0.8,
            avg_stability: 0.9,
            notes: "Good performance in upper section".to_string(),
            completed_timestamp: 1700000000,
            sustained_only: None,
        };
        write_toml(&sibling_post.join("performance_Shallow.toml"), &perf).expect("write perf");

        let prognosis = assemble_prognosis(&config).expect("should assemble");
        let shallow = &prognosis.formations[0];
        assert_eq!(shallow.offset_performance.wells, vec!["Well-B"]);
        assert!((shallow.offset_performance.avg_rop_ft_hr - 100.0).abs() < 0.01);
        assert!((shallow.offset_performance.best_rop_ft_hr - 150.0).abs() < 0.01);
    }

    #[test]
    fn test_formations_sorted_by_depth() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = KnowledgeBaseConfig {
            root: tmp.path().to_path_buf(),
            field: "TestField".to_string(),
            well: "Well-A".to_string(),
            ..Default::default()
        };
        config.ensure_dirs().expect("dirs");

        // Write geology with reversed order
        let geology = FieldGeology {
            field: "TestField".to_string(),
            formations: vec![
                GeologicalFormation {
                    name: "Deep".to_string(),
                    depth_top_ft: 5000.0,
                    depth_base_ft: 10000.0,
                    lithology: "sandstone".to_string(),
                    hardness: 7.0,
                    drillability: "hard".to_string(),
                    pore_pressure_ppg: 10.5,
                    fracture_gradient_ppg: 15.0,
                    hazards: Vec::new(),
                },
                GeologicalFormation {
                    name: "Shallow".to_string(),
                    depth_top_ft: 0.0,
                    depth_base_ft: 5000.0,
                    lithology: "clay".to_string(),
                    hardness: 2.0,
                    drillability: "soft".to_string(),
                    pore_pressure_ppg: 8.6,
                    fracture_gradient_ppg: 12.5,
                    hazards: Vec::new(),
                },
            ],
        };
        write_toml(&config.geology_path(), &geology).expect("write");

        let prognosis = assemble_prognosis(&config).expect("assemble");
        assert_eq!(prognosis.formations[0].name, "Shallow");
        assert_eq!(prognosis.formations[1].name, "Deep");
    }

    #[test]
    fn test_derive_default_parameters() {
        let soft = derive_default_parameters(2.0);
        assert!((soft.wob_klbs.optimal - 10.0).abs() < 0.01);

        let medium = derive_default_parameters(4.5);
        assert!((medium.wob_klbs.optimal - 25.0).abs() < 0.01);

        let hard = derive_default_parameters(7.0);
        assert!((hard.wob_klbs.optimal - 30.0).abs() < 0.01);
    }
}
