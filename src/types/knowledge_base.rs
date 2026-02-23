//! Knowledge base types for structured per-well geological and performance data

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::{
    BestParams, Campaign, CasingPoint, ConfidenceLevel, FormationParameters, OptimalParams,
    ParameterRange, PrognosisWellInfo,
};

/// Field-level geology (shared across all wells in a field)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldGeology {
    pub field: String,
    #[serde(rename = "formation")]
    pub formations: Vec<GeologicalFormation>,
}

/// Pure geological data — no engineering parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeologicalFormation {
    pub name: String,
    pub depth_top_ft: f64,
    pub depth_base_ft: f64,
    pub lithology: String,
    pub hardness: f64,
    pub drillability: String,
    pub pore_pressure_ppg: f64,
    pub fracture_gradient_ppg: f64,
    #[serde(default)]
    pub hazards: Vec<String>,
}

/// Well-specific pre-spud engineering plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreSpudPrognosis {
    pub well: PrognosisWellInfo,
    #[serde(rename = "formation")]
    pub formations: Vec<PreSpudFormation>,
    #[serde(default)]
    pub casings: Vec<CasingPoint>,
}

/// Per-formation engineering overrides
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreSpudFormation {
    pub name: String,
    /// Override geology depth if well differs
    pub depth_top_ft: Option<f64>,
    /// Override geology depth if well differs
    pub depth_base_ft: Option<f64>,
    pub parameters: FormationParameters,
    #[serde(default)]
    pub manual_offset: Option<OffsetPerformanceOverride>,
}

/// Manual offset performance data (from pre-spud engineering plan)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OffsetPerformanceOverride {
    pub wells: Vec<String>,
    pub avg_rop_ft_hr: f64,
    pub best_rop_ft_hr: f64,
    pub avg_mse_psi: f64,
    pub best_params: BestParams,
    #[serde(default)]
    pub notes: String,
}

/// Mid-well ML snapshot (written hourly during drilling)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidWellSnapshot {
    pub timestamp: u64,
    pub well_id: String,
    pub formation_name: String,
    pub depth_range: (f64, f64),
    pub campaign: Campaign,
    pub bit_hours: f64,
    pub optimal_params: OptimalParams,
    pub sample_count: usize,
    pub confidence: ConfidenceLevel,
    #[serde(default)]
    pub sustained_stats: Option<SustainedStats>,
}

/// Per-snapshot aggregate stats from sustained-only samples (seconds_since_param_change > 120)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SustainedStats {
    pub avg_rop_ft_hr: f64,
    pub best_rop_ft_hr: f64,
    pub avg_mse_psi: f64,
    pub avg_wob_klbs: f64,
    pub avg_rpm: f64,
    pub sample_count: usize,
}

/// Per-formation post-well performance (what other wells consume as offset data)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostWellFormationPerformance {
    pub well_id: String,
    pub field: String,
    pub formation_name: String,
    pub depth_top_ft: f64,
    pub depth_base_ft: f64,
    pub avg_rop_ft_hr: f64,
    pub best_rop_ft_hr: f64,
    pub avg_mse_psi: f64,
    pub best_params: BestParams,
    pub avg_wob_range: ParameterRange,
    pub avg_rpm_range: ParameterRange,
    pub avg_flow_range: ParameterRange,
    pub total_snapshots: usize,
    pub avg_confidence: f64,
    pub avg_stability: f64,
    #[serde(default)]
    pub notes: String,
    pub completed_timestamp: u64,
    /// Performance from sustained-only samples (seconds_since_param_change > 120)
    #[serde(default)]
    pub sustained_only: Option<SustainedFormationStats>,
}

/// Aggregated sustained-only performance across snapshots for a formation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SustainedFormationStats {
    pub avg_rop_ft_hr: f64,
    pub best_rop_ft_hr: f64,
    pub avg_mse_psi: f64,
    pub avg_wob_klbs: f64,
    pub avg_rpm: f64,
    pub total_sustained_samples: usize,
    pub low_sample_count: bool,
}

/// Complete post-well summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostWellSummary {
    pub well_id: String,
    pub field: String,
    pub completion_date: String,
    pub total_depth_ft: f64,
    pub total_bit_hours: f64,
    #[serde(rename = "formation_performance")]
    pub formations: Vec<PostWellFormationPerformance>,
}

/// Knowledge base runtime configuration
#[derive(Debug, Clone)]
pub struct KnowledgeBaseConfig {
    pub root: PathBuf,
    pub field: String,
    pub well: String,
    /// Maximum mid-well snapshots to keep uncompressed (default 168 = 7 days hourly)
    pub max_mid_well_snapshots: usize,
    /// Days to retain compressed cold files before deletion (default 30)
    pub cold_retention_days: u32,
}

impl Default for KnowledgeBaseConfig {
    fn default() -> Self {
        Self {
            root: PathBuf::from("./knowledge-base"),
            field: String::new(),
            well: String::new(),
            max_mid_well_snapshots: 168,
            cold_retention_days: 30,
        }
    }
}
