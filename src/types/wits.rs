//! WITS packet types

use serde::{Deserialize, Serialize};

use super::RigState;

/// WITS Level 0 packet containing full drilling parameters
///
/// Contains ~40+ channels covering drilling, hydraulics, mud, and well control data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitsPacket {
    pub timestamp: u64,

    // === Drilling Parameters ===
    /// Bit depth (ft) - WITS 0108
    pub bit_depth: f64,
    /// Hole depth (ft) - WITS 0110
    pub hole_depth: f64,
    /// Rate of penetration (ft/hr) - WITS 0113
    pub rop: f64,
    /// Hook load (klbs) - WITS 0114
    pub hook_load: f64,
    /// Weight on bit (klbs) - WITS 0116
    pub wob: f64,
    /// Rotary RPM - WITS 0117
    pub rpm: f64,
    /// Surface torque (kft-lbs) - WITS 0118
    pub torque: f64,
    /// Bit diameter (inches)
    pub bit_diameter: f64,

    // === Hydraulics Parameters ===
    /// Standpipe pressure (psi) - WITS 0119
    pub spp: f64,
    /// Pump strokes per minute - WITS 0120
    pub pump_spm: f64,
    /// Flow rate in (gpm) - WITS 0121
    pub flow_in: f64,
    /// Flow rate out (gpm) - WITS 0122
    pub flow_out: f64,
    /// Total pit volume (bbl) - WITS 0123
    pub pit_volume: f64,
    /// Pit volume change from baseline (bbl)
    #[serde(default)]
    pub pit_volume_change: f64,

    // === Mud Parameters ===
    /// Mud weight in (ppg) - WITS 0124
    pub mud_weight_in: f64,
    /// Mud weight out (ppg) - WITS 0125
    pub mud_weight_out: f64,
    /// Equivalent circulating density (ppg) - calculated or from sensor
    pub ecd: f64,
    /// Mud temperature in (°F) - WITS 0126
    pub mud_temp_in: f64,
    /// Mud temperature out (°F) - WITS 0127
    pub mud_temp_out: f64,

    // === Well Control Parameters ===
    /// Total gas units - WITS 0140
    pub gas_units: f64,
    /// Background gas (units)
    #[serde(default)]
    pub background_gas: f64,
    /// Connection gas (units)
    #[serde(default)]
    pub connection_gas: f64,
    /// H2S concentration (ppm) - WITS 0145
    #[serde(default)]
    pub h2s: f64,
    /// CO2 concentration (%) - WITS 0146
    #[serde(default)]
    pub co2: f64,
    /// Casing pressure (psi) - WITS 0130
    #[serde(default)]
    pub casing_pressure: f64,
    /// Annular pressure (psi)
    #[serde(default)]
    pub annular_pressure: f64,

    // === Formation Parameters ===
    /// Formation pore pressure estimate (ppg)
    #[serde(default)]
    pub pore_pressure: f64,
    /// Fracture gradient estimate (ppg)
    #[serde(default)]
    pub fracture_gradient: f64,

    // === Derived/Calculated Parameters ===
    /// Mechanical Specific Energy (psi) - calculated
    #[serde(default)]
    pub mse: f64,
    /// D-exponent - calculated
    #[serde(default)]
    pub d_exponent: f64,
    /// Corrected d-exponent (dxc)
    #[serde(default)]
    pub dxc: f64,
    /// ROP change from previous packet (ft/hr)
    #[serde(default)]
    pub rop_delta: f64,
    /// Torque change from baseline (%)
    #[serde(default)]
    pub torque_delta_percent: f64,
    /// SPP change from baseline (psi)
    #[serde(default)]
    pub spp_delta: f64,

    // === Rig State ===
    /// Current operational state of the rig
    #[serde(default)]
    pub rig_state: RigState,

    // === Regime Clustering ===
    /// Regime ID from CfC motor output k-means clustering (0-3)
    #[serde(default)]
    pub regime_id: u8,

    /// Seconds since last significant WOB/RPM change (for sustained-sample filtering)
    #[serde(default)]
    pub seconds_since_param_change: u64,

}

impl Default for WitsPacket {
    fn default() -> Self {
        Self {
            timestamp: 0,
            bit_depth: 0.0,
            hole_depth: 0.0,
            rop: 0.0,
            hook_load: 0.0,
            wob: 0.0,
            rpm: 0.0,
            torque: 0.0,
            bit_diameter: 8.5, // Common default
            spp: 0.0,
            pump_spm: 0.0,
            flow_in: 0.0,
            flow_out: 0.0,
            pit_volume: 0.0,
            pit_volume_change: 0.0,
            mud_weight_in: 0.0,
            mud_weight_out: 0.0,
            ecd: 0.0,
            mud_temp_in: 0.0,
            mud_temp_out: 0.0,
            gas_units: 0.0,
            background_gas: 0.0,
            connection_gas: 0.0,
            h2s: 0.0,
            co2: 0.0,
            casing_pressure: 0.0,
            annular_pressure: 0.0,
            pore_pressure: 0.0,
            fracture_gradient: 0.0,
            mse: 0.0,
            d_exponent: 0.0,
            dxc: 0.0,
            rop_delta: 0.0,
            torque_delta_percent: 0.0,
            spp_delta: 0.0,
            rig_state: RigState::Idle,
            regime_id: 0,
            seconds_since_param_change: 0,
        }
    }
}

impl WitsPacket {
    /// Calculate flow balance (positive = gain, negative = loss)
    pub fn flow_balance(&self) -> f64 {
        self.flow_out - self.flow_in
    }

    /// Get ECD margin to fracture gradient
    /// Returns the margin in ppg, or a safe default (1.5 ppg) if fracture gradient unavailable
    pub fn ecd_margin(&self) -> f64 {
        if self.fracture_gradient > 0.0 && self.ecd > 0.0 {
            self.fracture_gradient - self.ecd
        } else {
            // Return safe default when fracture gradient unavailable
            // 1.5 ppg is a typical comfortable margin
            1.5
        }
    }

    /// Check if drilling (RPM > 0 and WOB > 0)
    pub fn is_drilling(&self) -> bool {
        self.rpm > 5.0 && self.wob > 1.0
    }

    /// Check if circulating (flow > 0 but not drilling)
    pub fn is_circulating(&self) -> bool {
        self.flow_in > 50.0 && !self.is_drilling()
    }

    /// Calculate mud weight delta (in vs out)
    pub fn mud_weight_delta(&self) -> f64 {
        self.mud_weight_out - self.mud_weight_in
    }

    /// Calculate mud temperature delta (out vs in)
    pub fn mud_temp_delta(&self) -> f64 {
        self.mud_temp_out - self.mud_temp_in
    }
}
