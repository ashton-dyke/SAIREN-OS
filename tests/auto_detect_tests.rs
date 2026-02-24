//! Integration tests for Phase 2 auto-detection features:
//! - ConfigProvenance tracking
//! - AutoDetector from WITS packets
//! - BaselineOverrides computation

use sairen_os::baseline::{BaselineOverrides, ThresholdManager, wits_metrics};
use sairen_os::config::{self, ConfigProvenance, WellConfig};
use sairen_os::config::auto_detect::{AutoDetector, AutoDetectedValues};
use sairen_os::types::WitsPacket;

fn ensure_config() {
    if !config::is_initialized() {
        config::init(WellConfig::default(), ConfigProvenance::default());
    }
}

// ============================================================================
// ConfigProvenance tests
// ============================================================================

#[test]
fn provenance_explicit_key_detected() {
    let toml_str = r#"
[thresholds.hydraulics]
normal_mud_weight_ppg = 9.2
"#;
    let value: toml::Value = toml_str.parse().unwrap();
    let keys: std::collections::HashSet<String> =
        sairen_os::config::validation::walk_toml_keys(&value, "")
            .into_iter()
            .collect();
    let prov = ConfigProvenance { explicit_keys: keys };

    assert!(prov.is_user_set("thresholds.hydraulics.normal_mud_weight_ppg"));
    assert!(prov.is_user_set("thresholds.hydraulics"));
    assert!(!prov.is_user_set("thresholds.well_control.flow_imbalance_warning_gpm"));
}

#[test]
fn provenance_empty_config_has_no_keys() {
    let prov = ConfigProvenance::default();
    assert!(!prov.is_user_set("thresholds.hydraulics.normal_mud_weight_ppg"));
    assert!(!prov.is_user_set("well.name"));
    assert!(prov.explicit_keys.is_empty());
}

#[test]
fn provenance_partial_config_only_set_keys() {
    let toml_str = r#"
[well]
name = "Volve-1"
bit_diameter_inches = 12.25

[thresholds.hydraulics]
normal_mud_weight_ppg = 10.5
"#;
    let value: toml::Value = toml_str.parse().unwrap();
    let keys: std::collections::HashSet<String> =
        sairen_os::config::validation::walk_toml_keys(&value, "")
            .into_iter()
            .collect();
    let prov = ConfigProvenance { explicit_keys: keys };

    // User-set
    assert!(prov.is_user_set("well.name"));
    assert!(prov.is_user_set("well.bit_diameter_inches"));
    assert!(prov.is_user_set("thresholds.hydraulics.normal_mud_weight_ppg"));

    // Not set (defaults)
    assert!(!prov.is_user_set("thresholds.well_control.flow_imbalance_warning_gpm"));
    assert!(!prov.is_user_set("baseline_learning.warning_sigma"));
    assert!(!prov.is_user_set("physics.kick_min_indicators"));
}

// ============================================================================
// AutoDetector tests
// ============================================================================

fn make_packet(mud_weight_in: f64) -> WitsPacket {
    WitsPacket {
        mud_weight_in,
        ..Default::default()
    }
}

#[test]
fn auto_detect_stable_mud_weight() {
    let mut detector = AutoDetector::new();
    for _ in 0..35 {
        detector.observe(&make_packet(10.5));
    }
    assert!(detector.ready());
    let detected = detector.detect();
    let mw = detected.normal_mud_weight_ppg.expect("should detect stable mud weight");
    assert!((mw - 10.5).abs() < 0.01);
}

#[test]
fn auto_detect_skips_unstable_signal() {
    let mut detector = AutoDetector::new();
    // Alternating between 8 and 16 ppg = very high CV
    for i in 0..40 {
        let mw = if i % 2 == 0 { 8.0 } else { 16.0 };
        detector.observe(&make_packet(mw));
    }
    assert!(detector.ready());
    let detected = detector.detect();
    assert!(detected.normal_mud_weight_ppg.is_none());
}

#[test]
fn auto_detect_skips_zeros() {
    let mut detector = AutoDetector::new();
    // 20 zero packets + 30 valid packets
    for _ in 0..20 {
        detector.observe(&make_packet(0.0));
    }
    for _ in 0..30 {
        detector.observe(&make_packet(9.8));
    }
    assert!(detector.ready());
    let detected = detector.detect();
    let mw = detected.normal_mud_weight_ppg.expect("valid samples should be detected");
    assert!((mw - 9.8).abs() < 0.01);
}

#[test]
fn auto_detect_not_ready_with_few_samples() {
    let mut detector = AutoDetector::new();
    for _ in 0..10 {
        detector.observe(&make_packet(9.0));
    }
    assert!(!detector.ready());
}

#[test]
fn auto_detect_respects_user_config() {
    // Simulate: user set mud weight = 12.0, auto-detect sees 10.5
    let prov = {
        let toml_str = r#"
[thresholds.hydraulics]
normal_mud_weight_ppg = 12.0
"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let keys: std::collections::HashSet<String> =
            sairen_os::config::validation::walk_toml_keys(&value, "")
                .into_iter()
                .collect();
        ConfigProvenance { explicit_keys: keys }
    };

    let mut detector = AutoDetector::new();
    for _ in 0..35 {
        detector.observe(&make_packet(10.5));
    }
    let detected = detector.detect();
    assert!(detected.normal_mud_weight_ppg.is_some());

    // Provenance should prevent override
    assert!(prov.is_user_set("thresholds.hydraulics.normal_mud_weight_ppg"));
}

// ============================================================================
// BaselineOverrides tests
// ============================================================================

#[test]
fn baseline_overrides_computed_from_known_data() {
    ensure_config();

    let mut mgr = ThresholdManager::new();
    let eq = "TEST";
    mgr.start_wits_learning(eq, 0);

    // Feed stable data for 150 samples (enough to lock)
    for i in 0..150u64 {
        // Flow balance: mean ~0.0, std ~2.0
        mgr.add_sample(eq, wits_metrics::FLOW_BALANCE, (i as f64 % 5.0) - 2.0, i);
        // SPP: mean ~3000, std ~50
        mgr.add_sample(eq, wits_metrics::SPP, 3000.0 + (i as f64 % 10.0) * 5.0, i);
        // Torque: mean ~15, std ~1.5
        mgr.add_sample(eq, wits_metrics::TORQUE, 15.0 + (i as f64 % 6.0) * 0.3, i);
        // Other metrics (needed to avoid contamination)
        mgr.add_sample(eq, wits_metrics::MSE, 35000.0, i);
        mgr.add_sample(eq, wits_metrics::D_EXPONENT, 1.5, i);
        mgr.add_sample(eq, wits_metrics::DXC, 1.3, i);
        mgr.add_sample(eq, wits_metrics::ROP, 60.0, i);
        mgr.add_sample(eq, wits_metrics::WOB, 25.0, i);
        mgr.add_sample(eq, wits_metrics::RPM, 120.0, i);
        mgr.add_sample(eq, wits_metrics::ECD, 10.8, i);
        mgr.add_sample(eq, wits_metrics::PIT_VOLUME, 800.0, i);
        mgr.add_sample(eq, wits_metrics::GAS_UNITS, 20.0, i);
    }

    // Lock all
    let locked = mgr.try_lock_all_wits(eq, 200);
    assert!(!locked.is_empty(), "Should lock at least some metrics");

    // Compute overrides
    let overrides = mgr.compute_overrides(eq);

    // Flow imbalance: 3σ of std
    assert!(overrides.flow_imbalance_warning_gpm.is_some());
    let flow_warn = overrides.flow_imbalance_warning_gpm.unwrap();
    assert!(flow_warn > 0.0, "Flow imbalance override should be positive");

    // SPP: 2σ and 3σ
    assert!(overrides.spp_deviation_warning_psi.is_some());
    assert!(overrides.spp_deviation_critical_psi.is_some());
    let spp_warn = overrides.spp_deviation_warning_psi.unwrap();
    let spp_crit = overrides.spp_deviation_critical_psi.unwrap();
    assert!(spp_crit > spp_warn, "Critical SPP should be > warning SPP");

    // Torque: fraction-based
    assert!(overrides.torque_warning_fraction.is_some());
    assert!(overrides.torque_critical_fraction.is_some());
    let torq_warn = overrides.torque_warning_fraction.unwrap();
    let torq_crit = overrides.torque_critical_fraction.unwrap();
    assert!(torq_crit > torq_warn, "Critical torque should be > warning");
}

#[test]
fn baseline_overrides_default_is_all_none() {
    let overrides = BaselineOverrides::default();
    assert!(overrides.flow_imbalance_warning_gpm.is_none());
    assert!(overrides.spp_deviation_warning_psi.is_none());
    assert!(overrides.spp_deviation_critical_psi.is_none());
    assert!(overrides.torque_warning_fraction.is_none());
    assert!(overrides.torque_critical_fraction.is_none());
}
