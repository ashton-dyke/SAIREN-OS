//! Formation prognosis types for structured pre-drill geological data

use serde::{Deserialize, Serialize};

/// Range with min/optimal/max for a drilling parameter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterRange {
    pub min: f64,
    pub optimal: f64,
    pub max: f64,
}

/// Recommended drilling parameters for a formation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormationParameters {
    pub wob_klbs: ParameterRange,
    pub rpm: ParameterRange,
    pub flow_gpm: ParameterRange,
    pub mud_weight_ppg: f64,
    pub bit_type: String,
}

/// Offset well performance data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OffsetPerformance {
    pub wells: Vec<String>,
    pub avg_rop_ft_hr: f64,
    pub best_rop_ft_hr: f64,
    pub avg_mse_psi: f64,
    pub best_params: BestParams,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BestParams {
    pub wob_klbs: f64,
    pub rpm: f64,
}

/// A single formation interval in the prognosis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormationInterval {
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
    pub parameters: FormationParameters,
    pub offset_performance: OffsetPerformance,
}

/// Casing point definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CasingPoint {
    pub name: String,
    pub depth_ft: f64,
    pub size_in: f64,
    pub cement_top_ft: f64,
}

/// Well-level metadata in the prognosis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrognosisWellInfo {
    pub name: String,
    pub field: String,
    #[serde(default)]
    pub spud_date: String,
    pub target_depth_ft: f64,
    #[serde(default)]
    pub coordinate_system: String,
}

/// Complete well formation prognosis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormationPrognosis {
    pub well: PrognosisWellInfo,
    #[serde(rename = "formation")]
    pub formations: Vec<FormationInterval>,
    #[serde(default, rename = "casing")]
    pub casings: Vec<CasingPoint>,
}
