//! Well Configuration - All drilling thresholds as operator-tunable TOML values
//!
//! Every threshold that was previously hardcoded is now a field in this module.
//! Each struct implements `Default` with values matching the original constants,
//! ensuring zero-change behavior when no config file is present.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

// ============================================================================
// Config Provenance — tracks which keys the user explicitly set
// ============================================================================

/// Tracks which configuration keys were explicitly present in the user's TOML file.
///
/// After deserialization, all `#[serde(default)]` fields have values regardless of
/// whether the user set them. This struct preserves that distinction so auto-detection
/// can safely override defaults without clobbering explicit user choices.
#[derive(Debug, Clone, Default)]
pub struct ConfigProvenance {
    /// Dotted key paths explicitly present in the user's TOML file
    pub explicit_keys: HashSet<String>,
}

impl ConfigProvenance {
    /// Check whether a dotted key path was explicitly set by the user.
    ///
    /// Example: `provenance.is_user_set("thresholds.hydraulics.normal_mud_weight_ppg")`
    pub fn is_user_set(&self, dotted_key: &str) -> bool {
        self.explicit_keys.contains(dotted_key)
    }
}

// ============================================================================
// Top-Level Config
// ============================================================================

/// Root configuration for a well / rig deployment.
///
/// Load with `WellConfig::load()` which searches:
/// 1. `$SAIREN_CONFIG` env var
/// 2. `./well_config.toml`
/// 3. Built-in defaults
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WellConfig {
    /// Well / rig identification
    #[serde(default)]
    pub well: WellInfo,

    /// All drilling detection thresholds
    #[serde(default)]
    pub thresholds: ThresholdConfig,

    /// Baseline learning parameters
    #[serde(default)]
    pub baseline_learning: BaselineLearningConfig,

    /// Advisory ticket timing
    #[serde(default)]
    pub advisory: AdvisoryConfig,

    /// Ensemble voting weights
    #[serde(default)]
    pub ensemble_weights: EnsembleWeightsConfig,

    /// Physics engine tuning constants
    #[serde(default)]
    pub physics: PhysicsConfig,

    /// HTTP server configuration
    #[serde(default)]
    pub server: ServerConfig,

    /// ML engine tuning
    #[serde(default)]
    pub ml: MlConfig,
}

impl Default for WellConfig {
    fn default() -> Self {
        Self {
            well: WellInfo::default(),
            thresholds: ThresholdConfig::default(),
            baseline_learning: BaselineLearningConfig::default(),
            advisory: AdvisoryConfig::default(),
            ensemble_weights: EnsembleWeightsConfig::default(),
            physics: PhysicsConfig::default(),
            server: ServerConfig::default(),
            ml: MlConfig::default(),
        }
    }
}

impl WellConfig {
    /// Load configuration using the standard search order:
    /// 1. `$SAIREN_CONFIG` environment variable
    /// 2. `./well_config.toml` in the current working directory
    /// 3. Built-in defaults (original hardcoded values)
    pub fn load() -> Self {
        // 1. Check env var
        if let Ok(path) = std::env::var("SAIREN_CONFIG") {
            let p = PathBuf::from(&path);
            if p.exists() {
                match Self::load_from_file(&p) {
                    Ok(config) => {
                        info!(path = %p.display(), well = %config.well.name, "Loaded well config from SAIREN_CONFIG");
                        return config;
                    }
                    Err(e) => {
                        warn!(path = %p.display(), error = %e, "Failed to load config from SAIREN_CONFIG, falling back");
                    }
                }
            } else {
                warn!(path = %path, "SAIREN_CONFIG points to non-existent file, falling back");
            }
        }

        // 2. Check ./well_config.toml
        let local = PathBuf::from("well_config.toml");
        if local.exists() {
            match Self::load_from_file(&local) {
                Ok(config) => {
                    info!(well = %config.well.name, "Loaded well config from ./well_config.toml");
                    return config;
                }
                Err(e) => {
                    warn!(error = %e, "Failed to load ./well_config.toml, using defaults");
                }
            }
        }

        // 3. Defaults
        info!("No well_config.toml found — using built-in defaults");
        Self::default()
    }

    /// Load from a specific TOML file path.
    pub fn load_from_file(path: &Path) -> Result<Self, ConfigError> {
        let (config, _provenance) = Self::load_from_file_with_provenance(path)?;
        Ok(config)
    }

    /// Load from a specific TOML file path, also returning provenance
    /// so callers can distinguish user-set values from defaults.
    pub fn load_from_file_with_provenance(
        path: &Path,
    ) -> Result<(Self, ConfigProvenance), ConfigError> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::Io(path.to_path_buf(), e))?;

        // Two-pass: check for unknown keys first (warnings only)
        let typo_warnings = super::validation::validate_unknown_keys(&contents);
        for w in &typo_warnings {
            warn!("{}", w);
        }

        // Collect explicit key paths from the raw TOML
        let provenance = ConfigProvenance {
            explicit_keys: super::validation::walk_toml_keys(
                &contents
                    .parse::<toml::Value>()
                    .unwrap_or(toml::Value::Table(Default::default())),
                "",
            )
            .into_iter()
            .collect(),
        };

        let config: Self = toml::from_str(&contents)
            .map_err(|e| ConfigError::Parse(path.to_path_buf(), e))?;
        config.validate()?;
        Ok((config, provenance))
    }

    /// Load configuration using standard search order, returning provenance.
    ///
    /// Same search order as `load()` but also returns which keys the user
    /// explicitly set, enabling auto-detection to fill in the rest.
    pub fn load_with_provenance() -> (Self, ConfigProvenance) {
        // 1. Check env var
        if let Ok(path) = std::env::var("SAIREN_CONFIG") {
            let p = PathBuf::from(&path);
            if p.exists() {
                match Self::load_from_file_with_provenance(&p) {
                    Ok((config, provenance)) => {
                        info!(path = %p.display(), well = %config.well.name, "Loaded well config from SAIREN_CONFIG");
                        return (config, provenance);
                    }
                    Err(e) => {
                        warn!(path = %p.display(), error = %e, "Failed to load config from SAIREN_CONFIG, falling back");
                    }
                }
            } else {
                warn!(path = %path, "SAIREN_CONFIG points to non-existent file, falling back");
            }
        }

        // 2. Check ./well_config.toml
        let local = PathBuf::from("well_config.toml");
        if local.exists() {
            match Self::load_from_file_with_provenance(&local) {
                Ok((config, provenance)) => {
                    info!(well = %config.well.name, "Loaded well config from ./well_config.toml");
                    return (config, provenance);
                }
                Err(e) => {
                    warn!(error = %e, "Failed to load ./well_config.toml, using defaults");
                }
            }
        }

        // 3. Defaults — no file, so nothing is user-set
        info!("No well_config.toml found — using built-in defaults");
        (Self::default(), ConfigProvenance::default())
    }

    /// Serialize the current config to a TOML string.
    pub fn to_toml(&self) -> Result<String, ConfigError> {
        toml::to_string_pretty(self).map_err(ConfigError::Serialize)
    }

    /// Save config to a file (for runtime updates via API).
    pub fn save_to_file(&self, path: &Path) -> Result<(), ConfigError> {
        let contents = self.to_toml()?;
        std::fs::write(path, contents)
            .map_err(|e| ConfigError::Io(path.to_path_buf(), e))?;
        info!(path = %path.display(), "Well config saved");
        Ok(())
    }

    /// Validate all thresholds for internal consistency.
    ///
    /// Rules:
    /// - Critical thresholds must be >= warning thresholds (for absolute values)
    /// - Ensemble weights must sum to approximately 1.0
    /// - Sigma multipliers must be positive
    /// - Min-samples must be > 0
    pub fn validate(&self) -> Result<(), ConfigError> {
        let t = &self.thresholds;
        let mut errors: Vec<String> = Vec::new();

        // Well control: critical >= warning
        Self::check_escalation(
            t.well_control.flow_imbalance_warning_gpm,
            t.well_control.flow_imbalance_critical_gpm,
            "well_control.flow_imbalance",
            &mut errors,
        );
        Self::check_escalation(
            t.well_control.pit_gain_warning_bbl,
            t.well_control.pit_gain_critical_bbl,
            "well_control.pit_gain",
            &mut errors,
        );
        Self::check_escalation(
            t.well_control.pit_rate_warning_bbl_hr,
            t.well_control.pit_rate_critical_bbl_hr,
            "well_control.pit_rate",
            &mut errors,
        );
        Self::check_escalation(
            t.well_control.gas_units_warning,
            t.well_control.gas_units_critical,
            "well_control.gas_units",
            &mut errors,
        );
        Self::check_escalation(
            t.well_control.h2s_warning_ppm,
            t.well_control.h2s_critical_ppm,
            "well_control.h2s",
            &mut errors,
        );

        // Hydraulics: warning margin > critical margin (lower margin = more dangerous)
        if t.hydraulics.ecd_margin_critical_ppg >= t.hydraulics.ecd_margin_warning_ppg {
            errors.push(format!(
                "hydraulics.ecd_margin_critical ({:.2}) must be less than ecd_margin_warning ({:.2})",
                t.hydraulics.ecd_margin_critical_ppg, t.hydraulics.ecd_margin_warning_ppg
            ));
        }

        // SPP deviations
        Self::check_escalation(
            t.hydraulics.spp_deviation_warning_psi,
            t.hydraulics.spp_deviation_critical_psi,
            "hydraulics.spp_deviation",
            &mut errors,
        );

        // Mechanical
        Self::check_escalation(
            t.mechanical.torque_increase_warning,
            t.mechanical.torque_increase_critical,
            "mechanical.torque_increase",
            &mut errors,
        );
        Self::check_escalation(
            t.mechanical.stick_slip_cv_warning,
            t.mechanical.stick_slip_cv_critical,
            "mechanical.stick_slip_cv",
            &mut errors,
        );

        // Founder
        Self::check_escalation(
            t.founder.severity_warning,
            t.founder.severity_high,
            "founder.severity",
            &mut errors,
        );
        if t.founder.min_samples == 0 {
            errors.push("founder.min_samples must be > 0".to_string());
        }

        // MSE: optimal > warning > poor
        if t.mse.efficiency_warning_percent <= t.mse.efficiency_poor_percent {
            errors.push(format!(
                "mse.efficiency_warning ({:.0}) must be > efficiency_poor ({:.0})",
                t.mse.efficiency_warning_percent, t.mse.efficiency_poor_percent
            ));
        }

        // Ensemble weights: should sum to ~1.0 (allow 0.95-1.05)
        let w = &self.ensemble_weights;
        let weight_sum = w.mse + w.hydraulic + w.well_control + w.formation;
        if !(0.95..=1.05).contains(&weight_sum) {
            errors.push(format!(
                "ensemble_weights must sum to ~1.0, got {:.2}",
                weight_sum
            ));
        }

        // Baseline learning
        let bl = &self.baseline_learning;
        if bl.warning_sigma <= 0.0 {
            errors.push("baseline_learning.warning_sigma must be > 0".to_string());
        }
        if bl.critical_sigma <= bl.warning_sigma {
            errors.push(format!(
                "baseline_learning.critical_sigma ({:.1}) must be > warning_sigma ({:.1})",
                bl.critical_sigma, bl.warning_sigma
            ));
        }
        if bl.min_samples_for_lock == 0 {
            errors.push("baseline_learning.min_samples_for_lock must be > 0".to_string());
        }

        // Physics: divisors must be positive (used in division)
        let p = &self.physics;
        if p.formation_hardness_multiplier <= 0.0 {
            errors.push("physics.formation_hardness_multiplier must be > 0".to_string());
        }
        if p.kick_flow_severity_divisor <= 0.0 {
            errors.push("physics.kick_flow_severity_divisor must be > 0".to_string());
        }
        if p.kick_pit_severity_divisor <= 0.0 {
            errors.push("physics.kick_pit_severity_divisor must be > 0".to_string());
        }
        if p.kick_gas_severity_divisor <= 0.0 {
            errors.push("physics.kick_gas_severity_divisor must be > 0".to_string());
        }
        if p.confidence_full_window == 0 {
            errors.push("physics.confidence_full_window must be > 0".to_string());
        }

        // Physical range validation
        let (range_errors, range_warnings) = super::validation::validate_physical_ranges(self);
        errors.extend(range_errors);
        for w in &range_warnings {
            warn!("{}", w);
        }

        // Reject NaN/Inf in any config value (sweep all f64 fields via serialization)
        let serialized = toml::to_string(self);
        if let Ok(ref s) = serialized {
            if s.contains("nan") || s.contains("inf") {
                errors.push("Config contains NaN or Inf values — all thresholds must be finite numbers".to_string());
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(ConfigError::Validation(errors))
        }
    }

    fn check_escalation(warning: f64, critical: f64, name: &str, errors: &mut Vec<String>) {
        // NaN/Inf comparisons silently pass — catch them explicitly
        if !warning.is_finite() || !critical.is_finite() {
            errors.push(format!(
                "{name}: values must be finite (got warning={warning}, critical={critical})"
            ));
            return;
        }
        if critical < warning {
            errors.push(format!(
                "{name}: critical ({critical:.3}) must be >= warning ({warning:.3})"
            ));
        }
    }
}

// ============================================================================
// Error Type
// ============================================================================

#[derive(Debug)]
pub enum ConfigError {
    Io(PathBuf, std::io::Error),
    Parse(PathBuf, toml::de::Error),
    Serialize(toml::ser::Error),
    Validation(Vec<String>),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(path, e) => write!(f, "Config I/O error ({}): {}", path.display(), e),
            ConfigError::Parse(path, e) => {
                write!(f, "Config parse error ({}): {}", path.display(), e)
            }
            ConfigError::Serialize(e) => write!(f, "Config serialization error: {}", e),
            ConfigError::Validation(errors) => {
                writeln!(f, "Config validation failed:")?;
                for e in errors {
                    writeln!(f, "  - {}", e)?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for ConfigError {}

// ============================================================================
// Well Info
// ============================================================================

/// Identification metadata — not used for logic, but appears in logs and reports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WellInfo {
    /// Well name / identifier
    #[serde(default = "default_well_name")]
    pub name: String,

    /// Field name
    #[serde(default)]
    pub field: String,

    /// Rig name
    #[serde(default)]
    pub rig: String,

    /// Bit diameter in inches (used by physics calculations)
    #[serde(default = "default_bit_diameter")]
    pub bit_diameter_inches: f64,

    /// Campaign type: "production" or "plug_abandonment"
    #[serde(default = "default_campaign")]
    pub campaign: String,
}

fn default_well_name() -> String {
    "DEFAULT".to_string()
}
fn default_bit_diameter() -> f64 {
    8.5
}
fn default_campaign() -> String {
    "production".to_string()
}

impl Default for WellInfo {
    fn default() -> Self {
        Self {
            name: default_well_name(),
            field: String::new(),
            rig: String::new(),
            bit_diameter_inches: default_bit_diameter(),
            campaign: default_campaign(),
        }
    }
}

// ============================================================================
// Threshold Config (master container)
// ============================================================================

/// All detection thresholds, grouped by discipline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdConfig {
    #[serde(default)]
    pub well_control: WellControlThresholds,

    #[serde(default)]
    pub mse: MseThresholds,

    #[serde(default)]
    pub hydraulics: HydraulicsThresholds,

    #[serde(default)]
    pub mechanical: MechanicalThresholds,

    #[serde(default)]
    pub founder: FounderThresholds,

    #[serde(default)]
    pub formation: FormationThresholds,

    #[serde(default)]
    pub rig_state: RigStateThresholds,

    #[serde(default)]
    pub operation_detection: OperationDetectionThresholds,

    #[serde(default)]
    pub strategic_verification: StrategicVerificationThresholds,
}

impl Default for ThresholdConfig {
    fn default() -> Self {
        Self {
            well_control: WellControlThresholds::default(),
            mse: MseThresholds::default(),
            hydraulics: HydraulicsThresholds::default(),
            mechanical: MechanicalThresholds::default(),
            founder: FounderThresholds::default(),
            formation: FormationThresholds::default(),
            rig_state: RigStateThresholds::default(),
            operation_detection: OperationDetectionThresholds::default(),
            strategic_verification: StrategicVerificationThresholds::default(),
        }
    }
}

// ============================================================================
// Well Control Thresholds (SAFETY-CRITICAL)
// ============================================================================

/// Kick, loss, gas, and H2S detection thresholds.
///
/// These are the most safety-critical values in the system.
/// Changes should be reviewed by the company man and toolpusher.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WellControlThresholds {
    /// Flow imbalance warning threshold (gpm).
    /// Tactical agent triggers advisory when |flow_out - flow_in| exceeds this.
    #[serde(default = "default_flow_imbalance_warning")]
    pub flow_imbalance_warning_gpm: f64,

    /// Flow imbalance critical threshold (gpm).
    #[serde(default = "default_flow_imbalance_critical")]
    pub flow_imbalance_critical_gpm: f64,

    /// Pit gain warning threshold (bbl).
    #[serde(default = "default_pit_gain_warning")]
    pub pit_gain_warning_bbl: f64,

    /// Pit gain critical threshold (bbl).
    #[serde(default = "default_pit_gain_critical")]
    pub pit_gain_critical_bbl: f64,

    /// Pit rate warning threshold (bbl/hr).
    #[serde(default = "default_pit_rate_warning")]
    pub pit_rate_warning_bbl_hr: f64,

    /// Pit rate critical threshold (bbl/hr).
    #[serde(default = "default_pit_rate_critical")]
    pub pit_rate_critical_bbl_hr: f64,

    /// Total gas warning threshold (gas units).
    #[serde(default = "default_gas_units_warning")]
    pub gas_units_warning: f64,

    /// Total gas critical threshold (gas units).
    #[serde(default = "default_gas_units_critical")]
    pub gas_units_critical: f64,

    /// H2S warning threshold (ppm).
    #[serde(default = "default_h2s_warning")]
    pub h2s_warning_ppm: f64,

    /// H2S critical threshold (ppm).
    #[serde(default = "default_h2s_critical")]
    pub h2s_critical_ppm: f64,
}

fn default_flow_imbalance_warning() -> f64 { 10.0 }
fn default_flow_imbalance_critical() -> f64 { 20.0 }
fn default_pit_gain_warning() -> f64 { 5.0 }
fn default_pit_gain_critical() -> f64 { 10.0 }
fn default_pit_rate_warning() -> f64 { 5.0 }
fn default_pit_rate_critical() -> f64 { 15.0 }
fn default_gas_units_warning() -> f64 { 100.0 }
fn default_gas_units_critical() -> f64 { 500.0 }
fn default_h2s_warning() -> f64 { 10.0 }
fn default_h2s_critical() -> f64 { 20.0 }

impl Default for WellControlThresholds {
    fn default() -> Self {
        Self {
            flow_imbalance_warning_gpm: default_flow_imbalance_warning(),
            flow_imbalance_critical_gpm: default_flow_imbalance_critical(),
            pit_gain_warning_bbl: default_pit_gain_warning(),
            pit_gain_critical_bbl: default_pit_gain_critical(),
            pit_rate_warning_bbl_hr: default_pit_rate_warning(),
            pit_rate_critical_bbl_hr: default_pit_rate_critical(),
            gas_units_warning: default_gas_units_warning(),
            gas_units_critical: default_gas_units_critical(),
            h2s_warning_ppm: default_h2s_warning(),
            h2s_critical_ppm: default_h2s_critical(),
        }
    }
}

// ============================================================================
// MSE Efficiency Thresholds
// ============================================================================

/// Mechanical Specific Energy efficiency classification thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MseThresholds {
    /// Efficiency below this triggers an advisory (%).
    #[serde(default = "default_mse_warning")]
    pub efficiency_warning_percent: f64,

    /// Efficiency below this is considered poor (%).
    #[serde(default = "default_mse_poor")]
    pub efficiency_poor_percent: f64,
}

fn default_mse_warning() -> f64 { 70.0 }
fn default_mse_poor() -> f64 { 50.0 }

impl Default for MseThresholds {
    fn default() -> Self {
        Self {
            efficiency_warning_percent: default_mse_warning(),
            efficiency_poor_percent: default_mse_poor(),
        }
    }
}

// ============================================================================
// Hydraulics Thresholds
// ============================================================================

/// Mud weight, ECD, and standpipe pressure thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HydraulicsThresholds {
    /// Normal hydrostatic mud weight gradient (ppg).
    /// Used for d-exponent correction. Typically 8.5-9.0 ppg (0.465 psi/ft).
    /// THIS VALUE HAS THE LARGEST IMPACT ON D-EXPONENT ACCURACY.
    #[serde(default = "default_normal_mud_weight")]
    pub normal_mud_weight_ppg: f64,

    /// Typical fracture gradient (ppg).
    /// Used for ECD margin calculation.
    #[serde(default = "default_fracture_gradient")]
    pub fracture_gradient_ppg: f64,

    /// ECD margin warning threshold (ppg to fracture gradient).
    /// Advisory when ECD is within this margin of the fracture gradient.
    #[serde(default = "default_ecd_margin_warning")]
    pub ecd_margin_warning_ppg: f64,

    /// ECD margin critical threshold (ppg to fracture gradient).
    #[serde(default = "default_ecd_margin_critical")]
    pub ecd_margin_critical_ppg: f64,

    /// SPP deviation warning threshold (psi from baseline).
    #[serde(default = "default_spp_deviation_warning")]
    pub spp_deviation_warning_psi: f64,

    /// SPP deviation critical threshold (psi from baseline).
    #[serde(default = "default_spp_deviation_critical")]
    pub spp_deviation_critical_psi: f64,

    /// Annular pressure loss coefficient for simplified estimation.
    /// APL ≈ coefficient × flow_rate × depth / 1000.
    #[serde(default = "default_apl_coefficient")]
    pub annular_pressure_loss_coefficient: f64,
}

fn default_normal_mud_weight() -> f64 { 8.6 }
fn default_fracture_gradient() -> f64 { 14.0 }
fn default_ecd_margin_warning() -> f64 { 0.3 }
fn default_ecd_margin_critical() -> f64 { 0.1 }
fn default_spp_deviation_warning() -> f64 { 100.0 }
fn default_spp_deviation_critical() -> f64 { 200.0 }
fn default_apl_coefficient() -> f64 { 0.1 }

impl Default for HydraulicsThresholds {
    fn default() -> Self {
        Self {
            normal_mud_weight_ppg: default_normal_mud_weight(),
            fracture_gradient_ppg: default_fracture_gradient(),
            ecd_margin_warning_ppg: default_ecd_margin_warning(),
            ecd_margin_critical_ppg: default_ecd_margin_critical(),
            spp_deviation_warning_psi: default_spp_deviation_warning(),
            spp_deviation_critical_psi: default_spp_deviation_critical(),
            annular_pressure_loss_coefficient: default_apl_coefficient(),
        }
    }
}

// ============================================================================
// Mechanical Thresholds
// ============================================================================

/// Torque, pack-off, and stick-slip detection thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MechanicalThresholds {
    /// Torque increase warning threshold (fraction, e.g. 0.15 = 15%).
    #[serde(default = "default_torque_warning")]
    pub torque_increase_warning: f64,

    /// Torque increase critical threshold (fraction).
    #[serde(default = "default_torque_critical")]
    pub torque_increase_critical: f64,

    /// Stick-slip coefficient of variation warning threshold.
    #[serde(default = "default_stick_slip_warning")]
    pub stick_slip_cv_warning: f64,

    /// Stick-slip coefficient of variation critical threshold.
    #[serde(default = "default_stick_slip_critical")]
    pub stick_slip_cv_critical: f64,

    /// SPP increase threshold for pack-off detection (fraction).
    #[serde(default = "default_packoff_spp")]
    pub packoff_spp_increase_threshold: f64,

    /// ROP decrease threshold for pack-off detection (fraction).
    #[serde(default = "default_packoff_rop")]
    pub packoff_rop_decrease_threshold: f64,

    /// Minimum torque samples required for stick-slip analysis.
    #[serde(default = "default_stick_slip_min_samples")]
    pub stick_slip_min_samples: usize,
}

fn default_torque_warning() -> f64 { 0.15 }
fn default_torque_critical() -> f64 { 0.25 }
fn default_stick_slip_warning() -> f64 { 0.15 }
fn default_stick_slip_critical() -> f64 { 0.25 }
fn default_packoff_spp() -> f64 { 0.10 }
fn default_packoff_rop() -> f64 { 0.20 }
fn default_stick_slip_min_samples() -> usize { 5 }

impl Default for MechanicalThresholds {
    fn default() -> Self {
        Self {
            torque_increase_warning: default_torque_warning(),
            torque_increase_critical: default_torque_critical(),
            stick_slip_cv_warning: default_stick_slip_warning(),
            stick_slip_cv_critical: default_stick_slip_critical(),
            packoff_spp_increase_threshold: default_packoff_spp(),
            packoff_rop_decrease_threshold: default_packoff_rop(),
            stick_slip_min_samples: default_stick_slip_min_samples(),
        }
    }
}

// ============================================================================
// Founder Detection Thresholds
// ============================================================================

/// Bit balling / excessive WOB detection thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FounderThresholds {
    /// Minimum WOB increase per sample period (fraction) to consider "increasing".
    #[serde(default = "default_founder_wob_min")]
    pub wob_increase_min: f64,

    /// ROP response threshold — below this fraction, ROP is "not responding".
    #[serde(default = "default_founder_rop_min")]
    pub rop_response_min: f64,

    /// Founder severity threshold for warning level.
    #[serde(default = "default_founder_severity_warning")]
    pub severity_warning: f64,

    /// Founder severity threshold for high severity.
    #[serde(default = "default_founder_severity_high")]
    pub severity_high: f64,

    /// Minimum samples needed for reliable founder trend detection.
    #[serde(default = "default_founder_min_samples")]
    pub min_samples: usize,

    /// WOB delta percent for quick (two-packet) founder check.
    #[serde(default = "default_founder_quick_wob_delta")]
    pub quick_wob_delta_percent: f64,
}

fn default_founder_wob_min() -> f64 { 0.02 }
fn default_founder_rop_min() -> f64 { 0.01 }
fn default_founder_severity_warning() -> f64 { 0.3 }
fn default_founder_severity_high() -> f64 { 0.7 }
fn default_founder_min_samples() -> usize { 5 }
fn default_founder_quick_wob_delta() -> f64 { 0.05 }

impl Default for FounderThresholds {
    fn default() -> Self {
        Self {
            wob_increase_min: default_founder_wob_min(),
            rop_response_min: default_founder_rop_min(),
            severity_warning: default_founder_severity_warning(),
            severity_high: default_founder_severity_high(),
            min_samples: default_founder_min_samples(),
            quick_wob_delta_percent: default_founder_quick_wob_delta(),
        }
    }
}

// ============================================================================
// Formation Change Thresholds
// ============================================================================

/// D-exponent and MSE-based formation change detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormationThresholds {
    /// D-exponent decrease rate warning (per 100ft) — soft stringer / pressure.
    #[serde(default = "default_dexp_decrease")]
    pub dexp_decrease_warning: f64,

    /// MSE change percent threshold for formation change detection.
    #[serde(default = "default_mse_change_significant")]
    pub mse_change_significant: f64,

    /// D-exponent trend threshold for formation type classification.
    #[serde(default = "default_dxc_trend_threshold")]
    pub dxc_trend_threshold: f64,

    /// D-exponent trend threshold for abnormal pressure detection.
    #[serde(default = "default_dxc_pressure_threshold")]
    pub dxc_pressure_threshold: f64,

    /// MSE change tolerance for pressure detection (no MSE change + dxc decrease).
    #[serde(default = "default_mse_pressure_tolerance")]
    pub mse_pressure_tolerance: f64,
}

fn default_dexp_decrease() -> f64 { -0.15 }
fn default_mse_change_significant() -> f64 { 0.20 }
fn default_dxc_trend_threshold() -> f64 { 0.05 }
fn default_dxc_pressure_threshold() -> f64 { -0.05 }
fn default_mse_pressure_tolerance() -> f64 { 0.10 }

impl Default for FormationThresholds {
    fn default() -> Self {
        Self {
            dexp_decrease_warning: default_dexp_decrease(),
            mse_change_significant: default_mse_change_significant(),
            dxc_trend_threshold: default_dxc_trend_threshold(),
            dxc_pressure_threshold: default_dxc_pressure_threshold(),
            mse_pressure_tolerance: default_mse_pressure_tolerance(),
        }
    }
}

// ============================================================================
// Rig State Classification Thresholds
// ============================================================================

/// Thresholds for classifying rig operational state from WITS data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RigStateThresholds {
    /// RPM below this is considered "not rotating".
    #[serde(default = "default_idle_rpm_max")]
    pub idle_rpm_max: f64,

    /// Flow rate above this indicates pumps running (gpm).
    #[serde(default = "default_circulation_flow_min")]
    pub circulation_flow_min: f64,

    /// WOB above this indicates "on bottom" (klbs).
    #[serde(default = "default_drilling_wob_min")]
    pub drilling_wob_min: f64,

    /// Bit depth offset below hole depth for reaming classification (ft).
    #[serde(default = "default_reaming_depth_offset")]
    pub reaming_depth_offset: f64,

    /// Hook load above this during no-rotation / low-flow = tripping out (klbs).
    #[serde(default = "default_trip_out_hookload")]
    pub trip_out_hook_load_min: f64,

    /// Hook load below this during no-rotation / low-flow = tripping in (klbs).
    #[serde(default = "default_trip_in_hookload")]
    pub trip_in_hook_load_max: f64,

    /// Maximum flow rate to consider tripping state (gpm).
    #[serde(default = "default_tripping_flow_max")]
    pub tripping_flow_max: f64,
}

fn default_idle_rpm_max() -> f64 { 5.0 }
fn default_circulation_flow_min() -> f64 { 50.0 }
fn default_drilling_wob_min() -> f64 { 1.0 }
fn default_reaming_depth_offset() -> f64 { 5.0 }
fn default_trip_out_hookload() -> f64 { 200.0 }
fn default_trip_in_hookload() -> f64 { 50.0 }
fn default_tripping_flow_max() -> f64 { 100.0 }

impl Default for RigStateThresholds {
    fn default() -> Self {
        Self {
            idle_rpm_max: default_idle_rpm_max(),
            circulation_flow_min: default_circulation_flow_min(),
            drilling_wob_min: default_drilling_wob_min(),
            reaming_depth_offset: default_reaming_depth_offset(),
            trip_out_hook_load_min: default_trip_out_hookload(),
            trip_in_hook_load_max: default_trip_in_hookload(),
            tripping_flow_max: default_tripping_flow_max(),
        }
    }
}

// ============================================================================
// Operation Detection Thresholds
// ============================================================================

/// Automatic operation classification (milling, cement drill-out, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationDetectionThresholds {
    /// Minimum torque for milling detection (kft-lb).
    #[serde(default = "default_milling_torque")]
    pub milling_torque_min: f64,

    /// Maximum ROP for milling detection — milling has very low ROP (ft/hr).
    #[serde(default = "default_milling_rop")]
    pub milling_rop_max: f64,

    /// Minimum WOB for cement drill-out detection (klbs).
    #[serde(default = "default_cement_wob")]
    pub cement_drillout_wob_min: f64,

    /// Minimum torque for cement drill-out detection (kft-lb).
    #[serde(default = "default_cement_torque")]
    pub cement_drillout_torque_min: f64,

    /// Maximum ROP for cement drill-out detection (ft/hr).
    #[serde(default = "default_cement_rop")]
    pub cement_drillout_rop_max: f64,

    /// RPM below this is "not rotating" for circulation/static detection.
    #[serde(default = "default_no_rotation_rpm")]
    pub no_rotation_rpm_max: f64,

    /// WOB below this is "off bottom" (klbs).
    #[serde(default = "default_off_bottom_wob")]
    pub off_bottom_wob_max: f64,
}

fn default_milling_torque() -> f64 { 15.0 }
fn default_milling_rop() -> f64 { 5.0 }
fn default_cement_wob() -> f64 { 15.0 }
fn default_cement_torque() -> f64 { 12.0 }
fn default_cement_rop() -> f64 { 20.0 }
fn default_no_rotation_rpm() -> f64 { 10.0 }
fn default_off_bottom_wob() -> f64 { 5.0 }

impl Default for OperationDetectionThresholds {
    fn default() -> Self {
        Self {
            milling_torque_min: default_milling_torque(),
            milling_rop_max: default_milling_rop(),
            cement_drillout_wob_min: default_cement_wob(),
            cement_drillout_torque_min: default_cement_torque(),
            cement_drillout_rop_max: default_cement_rop(),
            no_rotation_rpm_max: default_no_rotation_rpm(),
            off_bottom_wob_max: default_off_bottom_wob(),
        }
    }
}

// ============================================================================
// Strategic Verification Thresholds
// ============================================================================

/// Thresholds used by the Strategic Agent to confirm/reject tactical advisories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategicVerificationThresholds {
    /// Flow balance sustained confirmation threshold (gpm).
    #[serde(default = "default_sv_flow_confirm")]
    pub flow_balance_confirmation_gpm: f64,

    /// Flow balance critical escalation threshold (gpm).
    #[serde(default = "default_sv_flow_critical")]
    pub flow_balance_critical_gpm: f64,

    /// Pit rate sustained confirmation threshold (bbl/hr).
    #[serde(default = "default_sv_pit_confirm")]
    pub pit_rate_confirmation_bbl_hr: f64,

    /// Pit rate critical escalation threshold (bbl/hr).
    #[serde(default = "default_sv_pit_critical")]
    pub pit_rate_critical_bbl_hr: f64,

    /// Flow balance below this is considered transient (gpm).
    #[serde(default = "default_sv_flow_transient")]
    pub flow_balance_transient_gpm: f64,

    /// Pit rate below this is considered transient (bbl/hr).
    #[serde(default = "default_sv_pit_transient")]
    pub pit_rate_transient_bbl_hr: f64,

    /// SPP deviation sustained threshold (psi).
    #[serde(default = "default_sv_spp_sustained")]
    pub spp_deviation_sustained_psi: f64,

    /// SPP deviation below this is considered normal (psi).
    #[serde(default = "default_sv_spp_normal")]
    pub spp_deviation_normal_psi: f64,

    /// Trend consistency threshold for mechanical confirmation.
    #[serde(default = "default_sv_trend_consistency")]
    pub trend_consistency_threshold: f64,

    /// Trend consistency threshold for formation confirmation.
    #[serde(default = "default_sv_formation_consistency")]
    pub formation_trend_consistency: f64,

    /// D-exponent trend absolute value for formation change confirmation.
    #[serde(default = "default_sv_dxc_change")]
    pub dxc_change_threshold: f64,
}

fn default_sv_flow_confirm() -> f64 { 15.0 }
fn default_sv_flow_critical() -> f64 { 20.0 }
fn default_sv_pit_confirm() -> f64 { 10.0 }
fn default_sv_pit_critical() -> f64 { 15.0 }
fn default_sv_flow_transient() -> f64 { 5.0 }
fn default_sv_pit_transient() -> f64 { 3.0 }
fn default_sv_spp_sustained() -> f64 { 150.0 }
fn default_sv_spp_normal() -> f64 { 50.0 }
fn default_sv_trend_consistency() -> f64 { 0.5 }
fn default_sv_formation_consistency() -> f64 { 0.6 }
fn default_sv_dxc_change() -> f64 { 0.1 }

impl Default for StrategicVerificationThresholds {
    fn default() -> Self {
        Self {
            flow_balance_confirmation_gpm: default_sv_flow_confirm(),
            flow_balance_critical_gpm: default_sv_flow_critical(),
            pit_rate_confirmation_bbl_hr: default_sv_pit_confirm(),
            pit_rate_critical_bbl_hr: default_sv_pit_critical(),
            flow_balance_transient_gpm: default_sv_flow_transient(),
            pit_rate_transient_bbl_hr: default_sv_pit_transient(),
            spp_deviation_sustained_psi: default_sv_spp_sustained(),
            spp_deviation_normal_psi: default_sv_spp_normal(),
            trend_consistency_threshold: default_sv_trend_consistency(),
            formation_trend_consistency: default_sv_formation_consistency(),
            dxc_change_threshold: default_sv_dxc_change(),
        }
    }
}

// ============================================================================
// Baseline Learning Config
// ============================================================================

/// Parameters controlling the dynamic threshold baseline learning system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineLearningConfig {
    /// Sigma multiplier for warning threshold (z-score).
    #[serde(default = "default_bl_warning_sigma")]
    pub warning_sigma: f64,

    /// Sigma multiplier for critical threshold (z-score).
    #[serde(default = "default_bl_critical_sigma")]
    pub critical_sigma: f64,

    /// Minimum samples before baseline can be locked.
    #[serde(default = "default_bl_min_samples")]
    pub min_samples_for_lock: usize,

    /// Minimum standard deviation floor to prevent divide-by-zero.
    #[serde(default = "default_bl_std_floor")]
    pub min_std_floor: f64,

    /// Maximum outlier percentage before baseline is flagged contaminated.
    #[serde(default = "default_bl_max_outlier")]
    pub max_outlier_percentage: f64,

    /// Sigma threshold for outlier detection during learning.
    #[serde(default = "default_bl_outlier_sigma")]
    pub outlier_sigma_threshold: f64,
}

fn default_bl_warning_sigma() -> f64 { 3.0 }
fn default_bl_critical_sigma() -> f64 { 5.0 }
fn default_bl_min_samples() -> usize { 100 }
fn default_bl_std_floor() -> f64 { 0.001 }
fn default_bl_max_outlier() -> f64 { 0.05 }
fn default_bl_outlier_sigma() -> f64 { 3.0 }

impl Default for BaselineLearningConfig {
    fn default() -> Self {
        Self {
            warning_sigma: default_bl_warning_sigma(),
            critical_sigma: default_bl_critical_sigma(),
            min_samples_for_lock: default_bl_min_samples(),
            min_std_floor: default_bl_std_floor(),
            max_outlier_percentage: default_bl_max_outlier(),
            outlier_sigma_threshold: default_bl_outlier_sigma(),
        }
    }
}

// ============================================================================
// Advisory Config
// ============================================================================

/// Advisory ticket timing and behaviour.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvisoryConfig {
    /// Default cooldown between tickets for the same anomaly type (seconds).
    #[serde(default = "default_cooldown_seconds")]
    pub default_cooldown_seconds: u64,

    /// Whether critical-severity tickets bypass the cooldown.
    #[serde(default = "default_critical_bypass")]
    pub critical_bypass_cooldown: bool,
}

fn default_cooldown_seconds() -> u64 { 60 }
fn default_critical_bypass() -> bool { true }

impl Default for AdvisoryConfig {
    fn default() -> Self {
        Self {
            default_cooldown_seconds: default_cooldown_seconds(),
            critical_bypass_cooldown: default_critical_bypass(),
        }
    }
}

// ============================================================================
// Ensemble Voting Weights
// ============================================================================

/// Phase 8 ensemble voting weights for specialist agents.
///
/// Weights must sum to approximately 1.0.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnsembleWeightsConfig {
    /// MSE / drilling efficiency specialist weight.
    #[serde(default = "default_weight_mse")]
    pub mse: f64,

    /// Hydraulic specialist weight.
    #[serde(default = "default_weight_hydraulic")]
    pub hydraulic: f64,

    /// Well control specialist weight (highest for safety).
    #[serde(default = "default_weight_well_control")]
    pub well_control: f64,

    /// Formation specialist weight.
    #[serde(default = "default_weight_formation")]
    pub formation: f64,
}

fn default_weight_mse() -> f64 { 0.25 }
fn default_weight_hydraulic() -> f64 { 0.25 }
fn default_weight_well_control() -> f64 { 0.30 }
fn default_weight_formation() -> f64 { 0.20 }

impl Default for EnsembleWeightsConfig {
    fn default() -> Self {
        Self {
            mse: default_weight_mse(),
            hydraulic: default_weight_hydraulic(),
            well_control: default_weight_well_control(),
            formation: default_weight_formation(),
        }
    }
}

// ============================================================================
// Physics Engine Config
// ============================================================================

/// Tuning constants for physics engine calculations.
///
/// Most values here are empirical approximations. The defaults are
/// standard industry values. Only change if you have rig-specific data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicsConfig {
    /// Formation hardness base MSE (psi) for optimal MSE estimation.
    /// Represents MSE for hardness=0 (very soft formation).
    #[serde(default = "default_hardness_base")]
    pub formation_hardness_base_psi: f64,

    /// Formation hardness multiplier for optimal MSE estimation.
    /// optimal_mse = base + hardness * multiplier.
    #[serde(default = "default_hardness_multiplier")]
    pub formation_hardness_multiplier: f64,

    /// Minimum indicators required to confirm a kick.
    #[serde(default = "default_kick_min_indicators")]
    pub kick_min_indicators: usize,

    /// Minimum indicators required to confirm lost circulation.
    #[serde(default = "default_loss_min_indicators")]
    pub loss_min_indicators: usize,

    /// Gas increase threshold above background for kick detection (gas units).
    #[serde(default = "default_kick_gas_threshold")]
    pub kick_gas_increase_threshold: f64,

    /// Severity divisor for flow-based kick severity calculation.
    #[serde(default = "default_kick_flow_divisor")]
    pub kick_flow_severity_divisor: f64,

    /// Severity divisor for pit-based kick severity calculation.
    #[serde(default = "default_kick_pit_divisor")]
    pub kick_pit_severity_divisor: f64,

    /// Severity divisor for gas-based kick severity calculation.
    #[serde(default = "default_kick_gas_divisor")]
    pub kick_gas_severity_divisor: f64,

    /// Number of history packets for full confidence (1 Hz = seconds).
    #[serde(default = "default_confidence_window")]
    pub confidence_full_window: usize,

    /// Minimum ROP for rotary MSE component calculation (ft/hr).
    /// Below this, only axial component is used.
    #[serde(default = "default_min_rop_for_mse")]
    pub min_rop_for_mse: f64,
}

fn default_hardness_base() -> f64 { 5000.0 }
fn default_hardness_multiplier() -> f64 { 8000.0 }
fn default_kick_min_indicators() -> usize { 2 }
fn default_loss_min_indicators() -> usize { 2 }
fn default_kick_gas_threshold() -> f64 { 50.0 }
fn default_kick_flow_divisor() -> f64 { 50.0 }
fn default_kick_pit_divisor() -> f64 { 20.0 }
fn default_kick_gas_divisor() -> f64 { 500.0 }
fn default_confidence_window() -> usize { 60 }
fn default_min_rop_for_mse() -> f64 { 0.1 }

impl Default for PhysicsConfig {
    fn default() -> Self {
        Self {
            formation_hardness_base_psi: default_hardness_base(),
            formation_hardness_multiplier: default_hardness_multiplier(),
            kick_min_indicators: default_kick_min_indicators(),
            loss_min_indicators: default_loss_min_indicators(),
            kick_gas_increase_threshold: default_kick_gas_threshold(),
            kick_flow_severity_divisor: default_kick_flow_divisor(),
            kick_pit_severity_divisor: default_kick_pit_divisor(),
            kick_gas_severity_divisor: default_kick_gas_divisor(),
            confidence_full_window: default_confidence_window(),
            min_rop_for_mse: default_min_rop_for_mse(),
        }
    }
}

// ============================================================================
// ML Engine Config
// ============================================================================

/// ML engine tuning parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MlConfig {
    /// ROP lag compensation in seconds.
    ///
    /// When the driller changes WOB at surface, the ROP response at the bit
    /// takes time to propagate through the drillstring. This lag means a WOB
    /// change at time T produces a measurable ROP change at time T + lag.
    /// By pairing each sample's WOB/RPM with the ROP from `rop_lag_seconds`
    /// later, the grid sees steady-state responses instead of transients.
    ///
    /// Typical values: 30–90 s depending on well depth and drillstring length.
    #[serde(default = "default_rop_lag_seconds")]
    pub rop_lag_seconds: u64,

    /// ML analysis interval in seconds.
    ///
    /// How often the ML scheduler runs its analysis pass.
    /// Can be overridden by `ML_INTERVAL_SECS` env var for backward compat.
    #[serde(default = "default_ml_interval_secs")]
    pub interval_secs: u64,
}

fn default_rop_lag_seconds() -> u64 { 60 }
fn default_ml_interval_secs() -> u64 { 3600 }

impl Default for MlConfig {
    fn default() -> Self {
        Self {
            rop_lag_seconds: default_rop_lag_seconds(),
            interval_secs: default_ml_interval_secs(),
        }
    }
}

// ============================================================================
// Server Config
// ============================================================================

/// HTTP server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// HTTP server bind address.
    ///
    /// Can be overridden by `SAIREN_SERVER_ADDR` env var or `--addr` CLI flag.
    #[serde(default = "default_server_addr")]
    pub addr: String,
}

fn default_server_addr() -> String {
    "0.0.0.0:8080".to_string()
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            addr: default_server_addr(),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_validates() {
        let config = WellConfig::default();
        assert!(config.validate().is_ok(), "Default config must always validate");
    }

    #[test]
    fn test_empty_toml_produces_defaults() {
        let config: WellConfig = toml::from_str("").expect("empty TOML should parse");
        assert_eq!(config.thresholds.well_control.flow_imbalance_warning_gpm, 10.0);
        assert_eq!(config.thresholds.mse.efficiency_warning_percent, 70.0);
        assert_eq!(config.thresholds.hydraulics.normal_mud_weight_ppg, 8.6);
        assert_eq!(config.baseline_learning.warning_sigma, 3.0);
        assert_eq!(config.ensemble_weights.well_control, 0.30);
    }

    #[test]
    fn test_partial_toml_override() {
        let toml_str = r#"
[well]
name = "Test-Well-1"

[thresholds.well_control]
flow_imbalance_warning_gpm = 25.0
flow_imbalance_critical_gpm = 50.0
"#;
        let config: WellConfig = toml::from_str(toml_str).expect("partial TOML should parse");
        // Overridden values
        assert_eq!(config.well.name, "Test-Well-1");
        assert_eq!(config.thresholds.well_control.flow_imbalance_warning_gpm, 25.0);
        assert_eq!(config.thresholds.well_control.flow_imbalance_critical_gpm, 50.0);
        // Non-overridden values retain defaults
        assert_eq!(config.thresholds.well_control.pit_gain_warning_bbl, 5.0);
        assert_eq!(config.thresholds.mse.efficiency_warning_percent, 70.0);
    }

    #[test]
    fn test_validation_catches_inverted_thresholds() {
        let mut config = WellConfig::default();
        config.thresholds.well_control.flow_imbalance_warning_gpm = 20.0;
        config.thresholds.well_control.flow_imbalance_critical_gpm = 10.0;
        let result = config.validate();
        assert!(result.is_err(), "Inverted thresholds should fail validation");
        if let Err(ConfigError::Validation(errors)) = result {
            assert!(errors.iter().any(|e| e.contains("flow_imbalance")));
        }
    }

    #[test]
    fn test_validation_catches_bad_weights() {
        let mut config = WellConfig::default();
        config.ensemble_weights.mse = 0.5;
        config.ensemble_weights.hydraulic = 0.5;
        config.ensemble_weights.well_control = 0.5;
        config.ensemble_weights.formation = 0.5;
        let result = config.validate();
        assert!(result.is_err(), "Weights summing to 2.0 should fail");
    }

    #[test]
    fn test_validation_catches_bad_baseline() {
        let mut config = WellConfig::default();
        config.baseline_learning.critical_sigma = 1.0;
        config.baseline_learning.warning_sigma = 3.0;
        let result = config.validate();
        assert!(result.is_err(), "Critical sigma < warning sigma should fail");
    }

    #[test]
    fn test_roundtrip_toml() {
        let original = WellConfig::default();
        let toml_str = original.to_toml().expect("serialization should work");
        let roundtripped: WellConfig = toml::from_str(&toml_str).expect("deserialization should work");
        assert_eq!(
            original.thresholds.well_control.flow_imbalance_warning_gpm,
            roundtripped.thresholds.well_control.flow_imbalance_warning_gpm
        );
        assert_eq!(
            original.thresholds.hydraulics.normal_mud_weight_ppg,
            roundtripped.thresholds.hydraulics.normal_mud_weight_ppg
        );
    }

    #[test]
    fn test_ecd_margin_validation() {
        let mut config = WellConfig::default();
        // Critical margin must be less than warning margin
        config.thresholds.hydraulics.ecd_margin_critical_ppg = 0.5;
        config.thresholds.hydraulics.ecd_margin_warning_ppg = 0.3;
        let result = config.validate();
        assert!(result.is_err(), "Critical ECD margin > warning should fail");
    }

    #[test]
    fn test_all_fields_serialize() {
        // Ensure no fields are silently skipped during serialization
        let config = WellConfig::default();
        let toml_str = config.to_toml().expect("serialization should work");
        // Spot check that key sections exist in output
        assert!(toml_str.contains("[well]"), "Missing [well] section");
        assert!(toml_str.contains("[thresholds.well_control]"), "Missing well_control section");
        assert!(toml_str.contains("[thresholds.hydraulics]"), "Missing hydraulics section");
        assert!(toml_str.contains("[baseline_learning]"), "Missing baseline_learning section");
        assert!(toml_str.contains("[ensemble_weights]"), "Missing ensemble_weights section");
        assert!(toml_str.contains("[physics]"), "Missing physics section");
        assert!(toml_str.contains("normal_mud_weight_ppg"), "Missing normal_mud_weight_ppg field");
    }

    // ========================================================================
    // ConfigProvenance tests
    // ========================================================================

    #[test]
    fn test_provenance_tracks_explicit_keys() {
        let toml_str = r#"
[thresholds.hydraulics]
normal_mud_weight_ppg = 9.2
"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let keys: std::collections::HashSet<String> =
            super::super::validation::walk_toml_keys(&value, "").into_iter().collect();
        let provenance = ConfigProvenance { explicit_keys: keys };

        assert!(provenance.is_user_set("thresholds.hydraulics.normal_mud_weight_ppg"));
        assert!(provenance.is_user_set("thresholds.hydraulics"));
        assert!(!provenance.is_user_set("thresholds.well_control.flow_imbalance_warning_gpm"));
    }

    #[test]
    fn test_provenance_default_has_zero_keys() {
        let provenance = ConfigProvenance::default();
        assert!(provenance.explicit_keys.is_empty());
        assert!(!provenance.is_user_set("thresholds.hydraulics.normal_mud_weight_ppg"));
    }

    #[test]
    fn test_provenance_partial_toml() {
        let toml_str = r#"
[well]
name = "Test-Well"

[thresholds.well_control]
flow_imbalance_warning_gpm = 25.0
"#;
        let value: toml::Value = toml_str.parse().unwrap();
        let keys: std::collections::HashSet<String> =
            super::super::validation::walk_toml_keys(&value, "").into_iter().collect();
        let provenance = ConfigProvenance { explicit_keys: keys };

        // Explicitly set keys
        assert!(provenance.is_user_set("well.name"));
        assert!(provenance.is_user_set("thresholds.well_control.flow_imbalance_warning_gpm"));

        // Not set
        assert!(!provenance.is_user_set("thresholds.hydraulics.normal_mud_weight_ppg"));
        assert!(!provenance.is_user_set("baseline_learning.warning_sigma"));
    }
}
