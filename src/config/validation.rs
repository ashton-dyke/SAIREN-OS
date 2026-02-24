//! Config validation: unknown-key detection with Levenshtein suggestions
//! and physical range checks.
//!
//! Two-pass parse approach: first deserialize raw TOML into `toml::Value`,
//! walk the key tree, compare against known field names, and emit warnings
//! with "did you mean?" suggestions. Then proceed with normal serde
//! deserialization. Warnings never break existing configs.

use std::collections::HashSet;

/// A non-fatal config warning (typo, suspicious value).
#[derive(Debug, Clone)]
pub struct ValidationWarning {
    pub field: String,
    pub message: String,
    pub suggestion: Option<String>,
}

impl std::fmt::Display for ValidationWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(ref s) = self.suggestion {
            write!(f, " — did you mean '{s}'?")?;
        }
        Ok(())
    }
}

// ============================================================================
// Known Config Keys
// ============================================================================

/// Returns the complete set of valid dotted key paths for WellConfig.
///
/// This is maintained manually to match the struct hierarchy in well_config.rs.
/// Any new field added to WellConfig must be added here too.
pub fn known_config_keys() -> HashSet<&'static str> {
    let keys: &[&str] = &[
        // [well]
        "well",
        "well.name",
        "well.field",
        "well.rig",
        "well.bit_diameter_inches",
        "well.campaign",
        // [server]
        "server",
        "server.addr",
        // [thresholds]
        "thresholds",
        // [thresholds.well_control]
        "thresholds.well_control",
        "thresholds.well_control.flow_imbalance_warning_gpm",
        "thresholds.well_control.flow_imbalance_critical_gpm",
        "thresholds.well_control.pit_gain_warning_bbl",
        "thresholds.well_control.pit_gain_critical_bbl",
        "thresholds.well_control.pit_rate_warning_bbl_hr",
        "thresholds.well_control.pit_rate_critical_bbl_hr",
        "thresholds.well_control.gas_units_warning",
        "thresholds.well_control.gas_units_critical",
        "thresholds.well_control.h2s_warning_ppm",
        "thresholds.well_control.h2s_critical_ppm",
        // [thresholds.mse]
        "thresholds.mse",
        "thresholds.mse.efficiency_warning_percent",
        "thresholds.mse.efficiency_poor_percent",
        // [thresholds.hydraulics]
        "thresholds.hydraulics",
        "thresholds.hydraulics.normal_mud_weight_ppg",
        "thresholds.hydraulics.fracture_gradient_ppg",
        "thresholds.hydraulics.ecd_margin_warning_ppg",
        "thresholds.hydraulics.ecd_margin_critical_ppg",
        "thresholds.hydraulics.spp_deviation_warning_psi",
        "thresholds.hydraulics.spp_deviation_critical_psi",
        "thresholds.hydraulics.annular_pressure_loss_coefficient",
        // [thresholds.mechanical]
        "thresholds.mechanical",
        "thresholds.mechanical.torque_increase_warning",
        "thresholds.mechanical.torque_increase_critical",
        "thresholds.mechanical.stick_slip_cv_warning",
        "thresholds.mechanical.stick_slip_cv_critical",
        "thresholds.mechanical.packoff_spp_increase_threshold",
        "thresholds.mechanical.packoff_rop_decrease_threshold",
        "thresholds.mechanical.stick_slip_min_samples",
        // [thresholds.founder]
        "thresholds.founder",
        "thresholds.founder.wob_increase_min",
        "thresholds.founder.rop_response_min",
        "thresholds.founder.severity_warning",
        "thresholds.founder.severity_high",
        "thresholds.founder.min_samples",
        "thresholds.founder.quick_wob_delta_percent",
        // [thresholds.formation]
        "thresholds.formation",
        "thresholds.formation.dexp_decrease_warning",
        "thresholds.formation.mse_change_significant",
        "thresholds.formation.dxc_trend_threshold",
        "thresholds.formation.dxc_pressure_threshold",
        "thresholds.formation.mse_pressure_tolerance",
        // [thresholds.rig_state]
        "thresholds.rig_state",
        "thresholds.rig_state.idle_rpm_max",
        "thresholds.rig_state.circulation_flow_min",
        "thresholds.rig_state.drilling_wob_min",
        "thresholds.rig_state.reaming_depth_offset",
        "thresholds.rig_state.trip_out_hook_load_min",
        "thresholds.rig_state.trip_in_hook_load_max",
        "thresholds.rig_state.tripping_flow_max",
        // [thresholds.operation_detection]
        "thresholds.operation_detection",
        "thresholds.operation_detection.milling_torque_min",
        "thresholds.operation_detection.milling_rop_max",
        "thresholds.operation_detection.cement_drillout_wob_min",
        "thresholds.operation_detection.cement_drillout_torque_min",
        "thresholds.operation_detection.cement_drillout_rop_max",
        "thresholds.operation_detection.no_rotation_rpm_max",
        "thresholds.operation_detection.off_bottom_wob_max",
        // [thresholds.strategic_verification]
        "thresholds.strategic_verification",
        "thresholds.strategic_verification.flow_balance_confirmation_gpm",
        "thresholds.strategic_verification.flow_balance_critical_gpm",
        "thresholds.strategic_verification.pit_rate_confirmation_bbl_hr",
        "thresholds.strategic_verification.pit_rate_critical_bbl_hr",
        "thresholds.strategic_verification.flow_balance_transient_gpm",
        "thresholds.strategic_verification.pit_rate_transient_bbl_hr",
        "thresholds.strategic_verification.spp_deviation_sustained_psi",
        "thresholds.strategic_verification.spp_deviation_normal_psi",
        "thresholds.strategic_verification.trend_consistency_threshold",
        "thresholds.strategic_verification.formation_trend_consistency",
        "thresholds.strategic_verification.dxc_change_threshold",
        // [baseline_learning]
        "baseline_learning",
        "baseline_learning.warning_sigma",
        "baseline_learning.critical_sigma",
        "baseline_learning.min_samples_for_lock",
        "baseline_learning.min_std_floor",
        "baseline_learning.max_outlier_percentage",
        "baseline_learning.outlier_sigma_threshold",
        // [advisory]
        "advisory",
        "advisory.default_cooldown_seconds",
        "advisory.critical_bypass_cooldown",
        // [ensemble_weights]
        "ensemble_weights",
        "ensemble_weights.mse",
        "ensemble_weights.hydraulic",
        "ensemble_weights.well_control",
        "ensemble_weights.formation",
        // [physics]
        "physics",
        "physics.formation_hardness_base_psi",
        "physics.formation_hardness_multiplier",
        "physics.kick_min_indicators",
        "physics.loss_min_indicators",
        "physics.kick_gas_increase_threshold",
        "physics.kick_flow_severity_divisor",
        "physics.kick_pit_severity_divisor",
        "physics.kick_gas_severity_divisor",
        "physics.confidence_full_window",
        "physics.min_rop_for_mse",
        // [ml]
        "ml",
        "ml.rop_lag_seconds",
        "ml.interval_secs",
    ];
    keys.iter().copied().collect()
}

// ============================================================================
// TOML Key Walking
// ============================================================================

/// Recursively walks a `toml::Value` tree and collects all dotted key paths.
///
/// For example, a table `{ a = { b = 1, c = 2 } }` yields:
/// `["a", "a.b", "a.c"]`
pub fn walk_toml_keys(value: &toml::Value, prefix: &str) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(table) = value.as_table() {
        for (k, v) in table {
            let path = if prefix.is_empty() {
                k.clone()
            } else {
                format!("{prefix}.{k}")
            };
            keys.push(path.clone());
            if v.is_table() {
                keys.extend(walk_toml_keys(v, &path));
            }
        }
    }
    keys
}

// ============================================================================
// Levenshtein Distance
// ============================================================================

/// Compute the Levenshtein edit distance between two strings.
fn levenshtein(a: &str, b: &str) -> usize {
    let a_len = a.len();
    let b_len = b.len();
    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0; b_len + 1];

    for (i, ca) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.chars().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1)
                .min(curr[j] + 1)
                .min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

/// Suggest the closest known key for an unknown key, if within edit distance 3.
pub fn suggest_correction(unknown: &str, known: &HashSet<&str>) -> Option<String> {
    let mut best: Option<(&str, usize)> = None;
    for &k in known {
        let dist = levenshtein(unknown, k);
        if dist <= 3 {
            if let Some((_, best_dist)) = best {
                if dist < best_dist {
                    best = Some((k, dist));
                }
            } else {
                best = Some((k, dist));
            }
        }
    }
    best.map(|(k, _)| k.to_string())
}

// ============================================================================
// Unknown Key Validation (entry point)
// ============================================================================

/// Parse a raw TOML string and return warnings for any unknown config keys.
///
/// This does NOT fail on unknown keys — it only warns. Existing configs
/// always continue to work.
pub fn validate_unknown_keys(raw_toml: &str) -> Vec<ValidationWarning> {
    let value: toml::Value = match raw_toml.parse() {
        Ok(v) => v,
        Err(_) => return Vec::new(), // parse errors are handled by serde later
    };

    let known = known_config_keys();
    let found = walk_toml_keys(&value, "");
    let mut warnings = Vec::new();

    for key in &found {
        if !known.contains(key.as_str()) {
            let suggestion = suggest_correction(key, &known);
            let message = format!("Unknown config key '{key}'");
            warnings.push(ValidationWarning {
                field: key.clone(),
                message,
                suggestion,
            });
        }
    }

    warnings
}

// ============================================================================
// Physical Range Validation
// ============================================================================

/// Validate physical ranges on a parsed WellConfig.
///
/// Returns (errors, warnings) — errors are impossible values that must
/// prevent startup; warnings are suspicious but not fatal.
pub fn validate_physical_ranges(
    config: &super::WellConfig,
) -> (Vec<String>, Vec<ValidationWarning>) {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    let h = &config.thresholds.hydraulics;

    // Mud weight: 5-25 ppg is the physically possible range for drilling fluids
    if h.normal_mud_weight_ppg < 5.0 || h.normal_mud_weight_ppg > 25.0 {
        errors.push(format!(
            "hydraulics.normal_mud_weight_ppg = {:.1} is outside physical range (5-25 ppg)",
            h.normal_mud_weight_ppg
        ));
    }

    // Bit diameter: 2-36 inches covers everything from coiled-tubing to conductor
    let bit = config.well.bit_diameter_inches;
    if bit < 2.0 || bit > 36.0 {
        errors.push(format!(
            "well.bit_diameter_inches = {:.1} is outside physical range (2-36 inches)",
            bit
        ));
    }

    // H2S: cannot be negative
    if config.thresholds.well_control.h2s_warning_ppm < 0.0 {
        errors.push(format!(
            "well_control.h2s_warning_ppm = {:.1} cannot be negative",
            config.thresholds.well_control.h2s_warning_ppm
        ));
    }
    if config.thresholds.well_control.h2s_critical_ppm < 0.0 {
        errors.push(format!(
            "well_control.h2s_critical_ppm = {:.1} cannot be negative",
            config.thresholds.well_control.h2s_critical_ppm
        ));
    }

    // min_rop_for_mse: must be positive (used as divisor)
    if config.physics.min_rop_for_mse <= 0.0 {
        errors.push(format!(
            "physics.min_rop_for_mse = {:.4} must be > 0 (used as divisor)",
            config.physics.min_rop_for_mse
        ));
    }

    // Fracture gradient: suspicious if outside 5-25 ppg
    if h.fracture_gradient_ppg < 5.0 || h.fracture_gradient_ppg > 25.0 {
        warnings.push(ValidationWarning {
            field: "thresholds.hydraulics.fracture_gradient_ppg".to_string(),
            message: format!(
                "fracture_gradient_ppg = {:.1} is outside typical range (5-25 ppg)",
                h.fracture_gradient_ppg
            ),
            suggestion: None,
        });
    }

    // Flow imbalance: suspicious if outside 1-500 gpm
    let fi = config.thresholds.well_control.flow_imbalance_warning_gpm;
    if fi < 1.0 || fi > 500.0 {
        warnings.push(ValidationWarning {
            field: "thresholds.well_control.flow_imbalance_warning_gpm".to_string(),
            message: format!(
                "flow_imbalance_warning_gpm = {:.1} is outside typical range (1-500 gpm)",
                fi
            ),
            suggestion: None,
        });
    }
    let fic = config.thresholds.well_control.flow_imbalance_critical_gpm;
    if fic < 1.0 || fic > 500.0 {
        warnings.push(ValidationWarning {
            field: "thresholds.well_control.flow_imbalance_critical_gpm".to_string(),
            message: format!(
                "flow_imbalance_critical_gpm = {:.1} is outside typical range (1-500 gpm)",
                fic
            ),
            suggestion: None,
        });
    }

    (errors, warnings)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_levenshtein_identical() {
        assert_eq!(levenshtein("hello", "hello"), 0);
    }

    #[test]
    fn test_levenshtein_one_edit() {
        assert_eq!(levenshtein("efficency", "efficiency"), 1);
    }

    #[test]
    fn test_levenshtein_empty() {
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
    }

    #[test]
    fn test_walk_toml_keys_flat() {
        let toml: toml::Value = r#"
            a = 1
            b = "hello"
        "#
        .parse()
        .unwrap();
        let keys = walk_toml_keys(&toml, "");
        assert!(keys.contains(&"a".to_string()));
        assert!(keys.contains(&"b".to_string()));
    }

    #[test]
    fn test_walk_toml_keys_nested() {
        let toml: toml::Value = r#"
            [thresholds]
            [thresholds.mse]
            efficiency_warning_percent = 70.0
        "#
        .parse()
        .unwrap();
        let keys = walk_toml_keys(&toml, "");
        assert!(keys.contains(&"thresholds".to_string()));
        assert!(keys.contains(&"thresholds.mse".to_string()));
        assert!(keys.contains(&"thresholds.mse.efficiency_warning_percent".to_string()));
    }

    #[test]
    fn test_typo_key_produces_warning_with_suggestion() {
        let toml_str = r#"
[thresholds.mse]
efficency_warning_percent = 70.0
"#;
        let warnings = validate_unknown_keys(toml_str);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].field.contains("efficency_warning_percent"));
        assert_eq!(
            warnings[0].suggestion.as_deref(),
            Some("thresholds.mse.efficiency_warning_percent")
        );
    }

    #[test]
    fn test_all_valid_keys_produce_zero_warnings() {
        let toml_str = r#"
[well]
name = "Test-1"
field = "North Sea"

[thresholds.hydraulics]
normal_mud_weight_ppg = 9.0

[ensemble_weights]
mse = 0.25
"#;
        let warnings = validate_unknown_keys(toml_str);
        assert!(
            warnings.is_empty(),
            "Expected 0 warnings, got: {:?}",
            warnings
        );
    }

    #[test]
    fn test_unknown_section_produces_warning() {
        let toml_str = r#"
[thresholds.typo_section]
some_field = 42
"#;
        let warnings = validate_unknown_keys(toml_str);
        assert!(
            !warnings.is_empty(),
            "Expected warnings for unknown section"
        );
        assert!(warnings
            .iter()
            .any(|w| w.field.contains("typo_section")));
    }

    #[test]
    fn test_suggest_correction_finds_close_match() {
        let known = known_config_keys();
        let suggestion = suggest_correction("thresholds.mse.efficency_warning_percent", &known);
        assert_eq!(
            suggestion.as_deref(),
            Some("thresholds.mse.efficiency_warning_percent")
        );
    }

    #[test]
    fn test_suggest_correction_no_match_for_garbage() {
        let known = known_config_keys();
        let suggestion = suggest_correction("completely_unrelated_garbage_key_xyz", &known);
        assert!(suggestion.is_none());
    }

    #[test]
    fn test_known_keys_covers_all_sections() {
        let known = known_config_keys();
        // Spot-check that every top-level section is represented
        assert!(known.contains("well"));
        assert!(known.contains("thresholds"));
        assert!(known.contains("baseline_learning"));
        assert!(known.contains("advisory"));
        assert!(known.contains("ensemble_weights"));
        assert!(known.contains("physics"));
        assert!(known.contains("server"));
        assert!(known.contains("ml"));
        // Spot-check leaf keys
        assert!(known.contains("thresholds.well_control.flow_imbalance_warning_gpm"));
        assert!(known.contains("physics.min_rop_for_mse"));
        assert!(known.contains("ml.rop_lag_seconds"));
    }

    #[test]
    fn test_physical_range_mud_weight_too_low() {
        let mut config = crate::config::WellConfig::default();
        config.thresholds.hydraulics.normal_mud_weight_ppg = 2.0;
        let (errors, _) = validate_physical_ranges(&config);
        assert!(!errors.is_empty(), "Mud weight 2.0 should be an error");
        assert!(errors[0].contains("normal_mud_weight_ppg"));
    }

    #[test]
    fn test_physical_range_mud_weight_valid() {
        let mut config = crate::config::WellConfig::default();
        config.thresholds.hydraulics.normal_mud_weight_ppg = 8.6;
        let (errors, _) = validate_physical_ranges(&config);
        let mud_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.contains("normal_mud_weight_ppg"))
            .collect();
        assert!(mud_errors.is_empty(), "Mud weight 8.6 should be valid");
    }

    #[test]
    fn test_physical_range_flow_imbalance_suspicious() {
        let mut config = crate::config::WellConfig::default();
        config.thresholds.well_control.flow_imbalance_warning_gpm = 9999.0;
        let (_, warnings) = validate_physical_ranges(&config);
        assert!(
            warnings
                .iter()
                .any(|w| w.field.contains("flow_imbalance_warning")),
            "Flow imbalance 9999 should produce a warning"
        );
    }

    #[test]
    fn test_physical_range_defaults_clean() {
        let config = crate::config::WellConfig::default();
        let (errors, warnings) = validate_physical_ranges(&config);
        assert!(
            errors.is_empty(),
            "Defaults should produce no errors: {:?}",
            errors
        );
        assert!(
            warnings.is_empty(),
            "Defaults should produce no warnings: {:?}",
            warnings
        );
    }

    #[test]
    fn test_physical_range_h2s_negative() {
        let mut config = crate::config::WellConfig::default();
        config.thresholds.well_control.h2s_warning_ppm = -5.0;
        let (errors, _) = validate_physical_ranges(&config);
        assert!(
            errors.iter().any(|e| e.contains("h2s_warning_ppm")),
            "Negative H2S should be an error"
        );
    }

    #[test]
    fn test_physical_range_min_rop_zero() {
        let mut config = crate::config::WellConfig::default();
        config.physics.min_rop_for_mse = 0.0;
        let (errors, _) = validate_physical_ranges(&config);
        assert!(
            errors.iter().any(|e| e.contains("min_rop_for_mse")),
            "min_rop_for_mse = 0 should be an error (division by zero)"
        );
    }

    #[test]
    fn test_bit_diameter_out_of_range() {
        let mut config = crate::config::WellConfig::default();
        config.well.bit_diameter_inches = 0.5;
        let (errors, _) = validate_physical_ranges(&config);
        assert!(
            errors.iter().any(|e| e.contains("bit_diameter_inches")),
            "Bit diameter 0.5 should be an error"
        );
    }
}
