//! Config Validation Tests
//!
//! Tests for Phase 1A (typo detection) and Phase 1B (range validation) of
//! the SAIREN-OS simplification plan.
//!
//! These tests exercise the config validation layer independently from the
//! rest of the pipeline.

use sairen_os::config::validation::{
    known_config_keys, suggest_correction, validate_physical_ranges, validate_unknown_keys,
};
use sairen_os::config::WellConfig;

// ============================================================================
// Phase 1A: Typo Detection Tests
// ============================================================================

#[test]
fn typo_in_mse_threshold_warns_with_suggestion() {
    let toml_str = r#"
[thresholds.mse]
efficency_warning_percent = 65.0
"#;
    let warnings = validate_unknown_keys(toml_str);
    assert_eq!(warnings.len(), 1, "Expected exactly 1 warning");
    assert!(warnings[0].field.contains("efficency_warning_percent"));
    assert!(
        warnings[0].suggestion.is_some(),
        "Should suggest a correction"
    );
    assert!(
        warnings[0]
            .suggestion
            .as_ref()
            .unwrap()
            .contains("efficiency_warning_percent"),
        "Should suggest the correct spelling"
    );
}

#[test]
fn typo_in_well_section_warns() {
    let toml_str = r#"
[well]
naem = "Test-Well"
"#;
    let warnings = validate_unknown_keys(toml_str);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].field.contains("naem"));
    // "naem" is distance 2 from "name" → should suggest
    assert_eq!(warnings[0].suggestion.as_deref(), Some("well.name"));
}

#[test]
fn valid_config_produces_zero_warnings() {
    let toml_str = r#"
[well]
name = "Endeavour-7"
field = "North Sea"
rig = "Maersk Invincible"
bit_diameter_inches = 12.25
campaign = "production"

[server]
addr = "0.0.0.0:9090"

[thresholds.well_control]
flow_imbalance_warning_gpm = 12.0
flow_imbalance_critical_gpm = 25.0

[thresholds.mse]
efficiency_warning_percent = 70.0

[thresholds.hydraulics]
normal_mud_weight_ppg = 9.2
fracture_gradient_ppg = 14.5

[baseline_learning]
warning_sigma = 3.0
critical_sigma = 5.0
min_samples_for_lock = 100

[ensemble_weights]
mse = 0.25
hydraulic = 0.25
well_control = 0.30
formation = 0.20

[physics]
min_rop_for_mse = 0.1

[advisory]
default_cooldown_seconds = 60

[ml]
rop_lag_seconds = 60
interval_secs = 1800
"#;
    let warnings = validate_unknown_keys(toml_str);
    assert!(
        warnings.is_empty(),
        "Valid config should produce 0 warnings, got: {:?}",
        warnings.iter().map(|w| &w.field).collect::<Vec<_>>()
    );
}

#[test]
fn unknown_section_warns() {
    let toml_str = r#"
[thresholds.nonexistent_section]
some_field = 42
"#;
    let warnings = validate_unknown_keys(toml_str);
    assert!(
        !warnings.is_empty(),
        "Unknown section should produce at least 1 warning"
    );
    assert!(warnings
        .iter()
        .any(|w| w.field.contains("nonexistent_section")));
}

#[test]
fn multiple_typos_all_warned() {
    let toml_str = r#"
[well]
naem = "Test"

[thresholds.mse]
efficency_warning_percent = 70.0
"#;
    let warnings = validate_unknown_keys(toml_str);
    assert_eq!(
        warnings.len(),
        2,
        "Expected 2 warnings for 2 typos, got {}",
        warnings.len()
    );
}

#[test]
fn empty_toml_produces_zero_warnings() {
    let warnings = validate_unknown_keys("");
    assert!(warnings.is_empty());
}

#[test]
fn known_keys_set_is_complete() {
    // Serialize default config to TOML and check that all keys appear in known_config_keys
    let config = WellConfig::default();
    let toml_str = config.to_toml().expect("Default config should serialize");
    let warnings = validate_unknown_keys(&toml_str);
    assert!(
        warnings.is_empty(),
        "Default config serialization should produce 0 unknown-key warnings, got: {:?}",
        warnings.iter().map(|w| &w.field).collect::<Vec<_>>()
    );
}

#[test]
fn suggest_correction_finds_close_match() {
    let known = known_config_keys();
    // "efficency" → "efficiency" (edit distance 1)
    let s = suggest_correction("thresholds.mse.efficency_warning_percent", &known);
    assert_eq!(
        s.as_deref(),
        Some("thresholds.mse.efficiency_warning_percent")
    );
}

#[test]
fn suggest_correction_returns_none_for_garbage() {
    let known = known_config_keys();
    let s = suggest_correction("zzz_completely_invalid_xyz_12345", &known);
    assert!(s.is_none(), "Garbage string should not match anything");
}

// ============================================================================
// Phase 1B: Range Validation Tests
// ============================================================================

#[test]
fn mud_weight_2_ppg_is_error() {
    let mut config = WellConfig::default();
    config.thresholds.hydraulics.normal_mud_weight_ppg = 2.0;
    let (errors, _) = validate_physical_ranges(&config);
    assert!(
        errors.iter().any(|e| e.contains("normal_mud_weight_ppg")),
        "Mud weight 2.0 ppg should be a physical range error"
    );
}

#[test]
fn mud_weight_8_6_ppg_is_valid() {
    let config = WellConfig::default(); // default is 8.6
    let (errors, _) = validate_physical_ranges(&config);
    assert!(
        !errors
            .iter()
            .any(|e| e.contains("normal_mud_weight_ppg")),
        "Mud weight 8.6 ppg should be valid"
    );
}

#[test]
fn mud_weight_30_ppg_is_error() {
    let mut config = WellConfig::default();
    config.thresholds.hydraulics.normal_mud_weight_ppg = 30.0;
    let (errors, _) = validate_physical_ranges(&config);
    assert!(!errors.is_empty(), "Mud weight 30.0 ppg should be an error");
}

#[test]
fn bit_diameter_too_small_is_error() {
    let mut config = WellConfig::default();
    config.well.bit_diameter_inches = 0.5;
    let (errors, _) = validate_physical_ranges(&config);
    assert!(
        errors.iter().any(|e| e.contains("bit_diameter_inches")),
        "Bit diameter 0.5\" should be an error"
    );
}

#[test]
fn bit_diameter_too_large_is_error() {
    let mut config = WellConfig::default();
    config.well.bit_diameter_inches = 48.0;
    let (errors, _) = validate_physical_ranges(&config);
    assert!(
        errors.iter().any(|e| e.contains("bit_diameter_inches")),
        "Bit diameter 48\" should be an error"
    );
}

#[test]
fn h2s_negative_is_error() {
    let mut config = WellConfig::default();
    config.thresholds.well_control.h2s_warning_ppm = -5.0;
    let (errors, _) = validate_physical_ranges(&config);
    assert!(
        errors.iter().any(|e| e.contains("h2s_warning_ppm")),
        "Negative H2S should be an error"
    );
}

#[test]
fn min_rop_zero_is_error() {
    let mut config = WellConfig::default();
    config.physics.min_rop_for_mse = 0.0;
    let (errors, _) = validate_physical_ranges(&config);
    assert!(
        errors.iter().any(|e| e.contains("min_rop_for_mse")),
        "min_rop_for_mse = 0 should be an error (division by zero)"
    );
}

#[test]
fn min_rop_negative_is_error() {
    let mut config = WellConfig::default();
    config.physics.min_rop_for_mse = -1.0;
    let (errors, _) = validate_physical_ranges(&config);
    assert!(
        errors.iter().any(|e| e.contains("min_rop_for_mse")),
        "min_rop_for_mse = -1.0 should be an error"
    );
}

#[test]
fn flow_imbalance_9999_is_warning() {
    let mut config = WellConfig::default();
    config.thresholds.well_control.flow_imbalance_warning_gpm = 9999.0;
    let (_, warnings) = validate_physical_ranges(&config);
    assert!(
        warnings
            .iter()
            .any(|w| w.field.contains("flow_imbalance_warning")),
        "Flow imbalance 9999 gpm should produce a warning"
    );
}

#[test]
fn fracture_gradient_extreme_is_warning() {
    let mut config = WellConfig::default();
    config.thresholds.hydraulics.fracture_gradient_ppg = 30.0;
    let (_, warnings) = validate_physical_ranges(&config);
    assert!(
        warnings
            .iter()
            .any(|w| w.field.contains("fracture_gradient_ppg")),
        "Fracture gradient 30 ppg should produce a warning"
    );
}

#[test]
fn all_defaults_pass_validation() {
    let config = WellConfig::default();
    // Physical ranges
    let (errors, warnings) = validate_physical_ranges(&config);
    assert!(
        errors.is_empty(),
        "Default config should have 0 range errors: {:?}",
        errors
    );
    assert!(
        warnings.is_empty(),
        "Default config should have 0 range warnings: {:?}",
        warnings.iter().map(|w| &w.field).collect::<Vec<_>>()
    );
    // Full validation (includes escalation checks, weight sums, etc.)
    assert!(
        config.validate().is_ok(),
        "Default config must always pass full validation"
    );
}

// ============================================================================
// Config Roundtrip Tests
// ============================================================================

#[test]
fn config_roundtrip_preserves_values() {
    let mut original = WellConfig::default();
    original.well.name = "Roundtrip-Test".to_string();
    original.thresholds.hydraulics.normal_mud_weight_ppg = 10.5;
    original.ensemble_weights.mse = 0.20;
    original.ensemble_weights.formation = 0.25;

    let toml_str = original.to_toml().expect("Serialization should work");
    let roundtripped: WellConfig =
        toml::from_str(&toml_str).expect("Deserialization should work");

    assert_eq!(roundtripped.well.name, "Roundtrip-Test");
    assert!(
        (roundtripped.thresholds.hydraulics.normal_mud_weight_ppg - 10.5).abs() < f64::EPSILON
    );
    assert!((roundtripped.ensemble_weights.mse - 0.20).abs() < f64::EPSILON);
    assert!((roundtripped.ensemble_weights.formation - 0.25).abs() < f64::EPSILON);
}

#[test]
fn config_roundtrip_passes_validation() {
    let original = WellConfig::default();
    let toml_str = original.to_toml().expect("Serialization should work");
    let roundtripped: WellConfig =
        toml::from_str(&toml_str).expect("Deserialization should work");
    assert!(
        roundtripped.validate().is_ok(),
        "Roundtripped config should pass validation"
    );
}

// ============================================================================
// Integration: validate() catches physical range errors
// ============================================================================

#[test]
fn validate_rejects_impossible_mud_weight() {
    let mut config = WellConfig::default();
    config.thresholds.hydraulics.normal_mud_weight_ppg = 2.0;
    let result = config.validate();
    assert!(
        result.is_err(),
        "validate() should fail for mud weight 2.0 ppg"
    );
}

#[test]
fn validate_rejects_impossible_bit_diameter() {
    let mut config = WellConfig::default();
    config.well.bit_diameter_inches = 0.1;
    let result = config.validate();
    assert!(
        result.is_err(),
        "validate() should fail for bit diameter 0.1\""
    );
}

#[test]
fn validate_rejects_zero_min_rop() {
    let mut config = WellConfig::default();
    config.physics.min_rop_for_mse = 0.0;
    let result = config.validate();
    assert!(
        result.is_err(),
        "validate() should fail for min_rop_for_mse = 0"
    );
}

// ============================================================================
// Phase 5: Config Consolidation Tests
// ============================================================================

#[test]
fn server_addr_defaults_to_standard() {
    let config = WellConfig::default();
    assert_eq!(config.server.addr, "0.0.0.0:8080");
}

#[test]
fn well_campaign_defaults_to_production() {
    let config = WellConfig::default();
    assert_eq!(config.well.campaign, "production");
}

#[test]
fn well_campaign_from_toml() {
    let toml_str = r#"
[well]
campaign = "plug_abandonment"
"#;
    let config: WellConfig = toml::from_str(toml_str).expect("should parse");
    assert_eq!(config.well.campaign, "plug_abandonment");
}

#[test]
fn server_addr_from_toml() {
    let toml_str = r#"
[server]
addr = "127.0.0.1:9090"
"#;
    let config: WellConfig = toml::from_str(toml_str).expect("should parse");
    assert_eq!(config.server.addr, "127.0.0.1:9090");
}

#[test]
fn ml_interval_from_toml() {
    let toml_str = r#"
[ml]
interval_secs = 300
"#;
    let config: WellConfig = toml::from_str(toml_str).expect("should parse");
    assert_eq!(config.ml.interval_secs, 300);
}

#[test]
fn old_config_with_removed_fields_still_loads() {
    // Backward compatibility: old TOML with operator, efficiency_optimal_percent,
    // and [campaign.production] should still parse (serde ignores unknown keys).
    let toml_str = r#"
[well]
name = "Old-Config-Well"
operator = "Equinor"

[thresholds.mse]
efficiency_optimal_percent = 85.0
efficiency_warning_percent = 70.0

[campaign.production]
mse_efficiency_warning = 70.0
flow_imbalance_warning = 10.0
"#;
    // serde(deny_unknown_fields) is NOT set, so this parses fine
    let config: WellConfig = toml::from_str(toml_str).expect(
        "Old config with removed fields should still parse"
    );
    assert_eq!(config.well.name, "Old-Config-Well");
    assert_eq!(config.thresholds.mse.efficiency_warning_percent, 70.0);
}

#[test]
fn removed_fields_produce_unknown_key_warnings() {
    let toml_str = r#"
[well]
operator = "Equinor"

[thresholds.mse]
efficiency_optimal_percent = 85.0

[thresholds.formation]
dexp_increase_warning = 0.1
"#;
    let warnings = validate_unknown_keys(toml_str);
    let warned_fields: Vec<&str> = warnings.iter().map(|w| w.field.as_str()).collect();
    assert!(
        warned_fields.contains(&"well.operator"),
        "Should warn about removed 'well.operator', got: {:?}",
        warned_fields
    );
    assert!(
        warned_fields.contains(&"thresholds.mse.efficiency_optimal_percent"),
        "Should warn about removed 'efficiency_optimal_percent', got: {:?}",
        warned_fields
    );
    assert!(
        warned_fields.contains(&"thresholds.formation.dexp_increase_warning"),
        "Should warn about removed 'dexp_increase_warning', got: {:?}",
        warned_fields
    );
}
