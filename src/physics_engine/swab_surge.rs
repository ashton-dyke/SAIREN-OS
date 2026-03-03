//! Swab/Surge Pressure Estimation (Burkhardt Simplified Model)
//!
//! Estimates pressure changes during tripping operations caused by pipe
//! movement through the wellbore. Swab (pulling out) reduces pressure,
//! surge (running in) increases pressure — both can cause well control events.
//!
//! ## Model
//!
//! Uses the Burkhardt simplified approach:
//! 1. Compute annular velocity from trip speed and geometry
//! 2. Estimate friction pressure from rheology (PV/YP)
//! 3. Apply clinging factor (0.45) for pressure change
//! 4. Convert to equivalent mud weight for margin analysis

use serde::Serialize;

/// Risk level for swab/surge pressure
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum SwabSurgeRisk {
    Safe,
    Warning,
    Critical,
}

/// Complete swab/surge estimation result
#[derive(Debug, Clone, Serialize)]
pub struct SwabSurgeEstimate {
    /// Trip speed used for calculation (ft/min)
    pub trip_speed_ft_min: f64,
    /// Pressure change (psi) — negative = swab, positive = surge
    pub pressure_change_psi: f64,
    /// Equivalent mud weight including swab/surge (ppg)
    pub equivalent_mud_weight_ppg: f64,
    /// Margin to pore pressure (ppg) — negative = underbalanced (kick risk)
    pub margin_to_pore_pressure_ppg: f64,
    /// Margin to fracture gradient (ppg) — negative = fracture risk
    pub margin_to_frac_gradient_ppg: f64,
    /// Risk assessment
    pub risk_level: SwabSurgeRisk,
}

/// Burkhardt clinging factor (empirical constant)
const CLINGING_FACTOR: f64 = 0.45;

/// Pressure gradient conversion: psi per ft per ppg
const PSI_PER_FT_PER_PPG: f64 = 0.052;

/// Estimate swab/surge pressure from trip parameters.
///
/// # Arguments
/// * `trip_speed_ft_min` - Pipe movement speed (ft/min), estimated from hookload delta
/// * `depth_ft` - Current bit depth (ft)
/// * `mud_weight_ppg` - Current mud weight (ppg)
/// * `pore_pressure_ppg` - Formation pore pressure (ppg)
/// * `frac_gradient_ppg` - Formation fracture gradient (ppg)
/// * `pipe_od_in` - Pipe outer diameter (inches)
/// * `hole_diameter_in` - Hole diameter (inches)
/// * `plastic_viscosity_cp` - Mud plastic viscosity (cP)
/// * `yield_point` - Mud yield point (lbf/100ft²)
/// * `is_tripping_in` - true = surge (running in), false = swab (pulling out)
pub fn estimate_swab_surge(
    trip_speed_ft_min: f64,
    depth_ft: f64,
    mud_weight_ppg: f64,
    pore_pressure_ppg: f64,
    frac_gradient_ppg: f64,
    pipe_od_in: f64,
    hole_diameter_in: f64,
    plastic_viscosity_cp: f64,
    yield_point: f64,
    is_tripping_in: bool,
) -> SwabSurgeEstimate {
    // Zero speed → zero pressure change
    if trip_speed_ft_min.abs() < 0.01 || depth_ft <= 0.0 {
        return SwabSurgeEstimate {
            trip_speed_ft_min,
            pressure_change_psi: 0.0,
            equivalent_mud_weight_ppg: mud_weight_ppg,
            margin_to_pore_pressure_ppg: mud_weight_ppg - pore_pressure_ppg,
            margin_to_frac_gradient_ppg: frac_gradient_ppg - mud_weight_ppg,
            risk_level: SwabSurgeRisk::Safe,
        };
    }

    // Non-positive PP or FG → margins are meaningless; return warning
    if pore_pressure_ppg <= 0.0 || frac_gradient_ppg <= 0.0 {
        return SwabSurgeEstimate {
            trip_speed_ft_min,
            pressure_change_psi: 0.0,
            equivalent_mud_weight_ppg: mud_weight_ppg,
            margin_to_pore_pressure_ppg: 0.0,
            margin_to_frac_gradient_ppg: 0.0,
            risk_level: SwabSurgeRisk::Warning,
        };
    }

    // Cross-sectional areas (in²)
    let pipe_area = std::f64::consts::PI / 4.0 * pipe_od_in * pipe_od_in;
    let hole_area = std::f64::consts::PI / 4.0 * hole_diameter_in * hole_diameter_in;
    let annular_area = hole_area - pipe_area;

    // Annular velocity (ft/min)
    let annular_velocity = if annular_area > 0.0 {
        trip_speed_ft_min * pipe_area / annular_area
    } else {
        0.0
    };

    // Annular gap (inches)
    let annular_gap = (hole_diameter_in - pipe_od_in) / 2.0;

    // Friction pressure (simplified Bingham plastic model)
    // Pressure loss = (PV × velocity × depth) / (60000 × gap²) + (YP × depth) / (200 × gap)
    let friction_pressure = if annular_gap > 0.0 {
        let viscous_term =
            plastic_viscosity_cp * annular_velocity * depth_ft / (60000.0 * annular_gap * annular_gap);
        let yield_term = yield_point * depth_ft / (200.0 * annular_gap);
        viscous_term + yield_term
    } else {
        0.0
    };

    // Pressure change with clinging factor
    let pressure_change = CLINGING_FACTOR * friction_pressure;

    // Apply sign: surge = positive (increased pressure), swab = negative
    let signed_pressure = if is_tripping_in {
        pressure_change
    } else {
        -pressure_change
    };

    // Convert to EMW
    let hydrostatic_gradient = PSI_PER_FT_PER_PPG * depth_ft;
    let emw = if hydrostatic_gradient > 0.0 {
        mud_weight_ppg + signed_pressure / hydrostatic_gradient
    } else {
        mud_weight_ppg
    };

    // Margins
    let margin_pp = emw - pore_pressure_ppg;
    let margin_fg = frac_gradient_ppg - emw;

    // Risk assessment
    let risk = if margin_pp < 0.0 || margin_fg < 0.0 {
        SwabSurgeRisk::Critical
    } else if margin_pp < 0.3 || margin_fg < 0.3 {
        SwabSurgeRisk::Warning
    } else {
        SwabSurgeRisk::Safe
    };

    SwabSurgeEstimate {
        trip_speed_ft_min,
        pressure_change_psi: signed_pressure,
        equivalent_mud_weight_ppg: emw,
        margin_to_pore_pressure_ppg: margin_pp,
        margin_to_frac_gradient_ppg: margin_fg,
        risk_level: risk,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_speed_zero_pressure() {
        let result = estimate_swab_surge(
            0.0,    // zero speed
            10000.0,
            12.0,
            10.0,
            15.0,
            5.0,
            8.5,
            15.0,
            10.0,
            true,
        );
        assert_eq!(result.pressure_change_psi, 0.0);
        assert_eq!(result.equivalent_mud_weight_ppg, 12.0);
        assert_eq!(result.risk_level, SwabSurgeRisk::Safe);
    }

    #[test]
    fn test_reasonable_surge_pressure() {
        let result = estimate_swab_surge(
            30.0,   // 30 ft/min trip speed
            10000.0,
            12.0,
            10.0,
            15.0,
            5.0,    // 5" pipe
            8.5,    // 8.5" hole
            15.0,   // PV = 15 cP
            10.0,   // YP = 10
            true,   // surge
        );
        // Pressure should be positive (surge)
        assert!(
            result.pressure_change_psi > 0.0,
            "Surge should produce positive pressure: {}",
            result.pressure_change_psi
        );
        // EMW should be above static mud weight
        assert!(result.equivalent_mud_weight_ppg > 12.0);
        // Should be safe with these margins
        assert_eq!(result.risk_level, SwabSurgeRisk::Safe);
    }

    #[test]
    fn test_zero_pore_pressure_returns_warning() {
        let result = estimate_swab_surge(
            30.0, 10000.0, 12.0, 0.0, 15.0, 5.0, 8.5, 15.0, 10.0, false,
        );
        assert_eq!(result.risk_level, SwabSurgeRisk::Warning);
        assert_eq!(result.pressure_change_psi, 0.0);
    }

    #[test]
    fn test_negative_frac_gradient_returns_warning() {
        let result = estimate_swab_surge(
            30.0, 10000.0, 12.0, 10.0, -1.0, 5.0, 8.5, 15.0, 10.0, true,
        );
        assert_eq!(result.risk_level, SwabSurgeRisk::Warning);
        assert_eq!(result.pressure_change_psi, 0.0);
    }

    #[test]
    fn test_emw_crossing_frac_gradient_critical() {
        // Tight margins: MW very close to frac gradient
        let result = estimate_swab_surge(
            60.0,    // fast trip
            10000.0,
            14.8,    // MW close to FG
            10.0,
            15.0,    // FG = 15.0
            5.0,
            8.5,
            25.0,    // high PV
            20.0,    // high YP
            true,    // surge
        );
        // With tight margins and fast trip, should be Warning or Critical
        assert!(
            result.risk_level != SwabSurgeRisk::Safe,
            "Tight margins with fast trip should not be Safe, margin_fg={:.2}",
            result.margin_to_frac_gradient_ppg
        );
    }
}
