//! ML Engine Scheduler (V2.1)
//!
//! Configurable interval scheduler for running ML analysis.
//! Uses `ML_INTERVAL_SECS` environment variable (default: 3600 = 1 hour).
//!
//! # Example
//! ```bash
//! # Run analysis every 5 minutes for testing
//! ML_INTERVAL_SECS=300 cargo run --release
//! ```

use std::time::Duration;
use tracing::{info, warn};

use crate::types::{AnalysisResult, Campaign, HourlyDataset, MLInsightsReport};

use super::analyzer::HourlyAnalyzer;

/// Get the ML analysis interval.
///
/// Precedence: `ML_INTERVAL_SECS` env var > `ml.interval_secs` TOML > 3600
pub fn get_interval_secs() -> u64 {
    std::env::var("ML_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| crate::config::get().ml.interval_secs)
}

/// Get the ML analysis interval as a Duration
pub fn get_interval() -> Duration {
    Duration::from_secs(get_interval_secs())
}

/// ML Analysis Scheduler
///
/// Runs ML analysis at configurable intervals and stores results.
pub struct MLScheduler;

impl MLScheduler {
    /// Create a new scheduler with the configured interval
    pub fn new() -> Self {
        let interval_secs = get_interval_secs();
        info!(
            interval_secs = interval_secs,
            "ML Engine scheduler created (env: ML_INTERVAL_SECS)"
        );
        Self
    }

    /// Run analysis on a dataset
    ///
    /// This is the core analysis function that can be called manually or by the scheduler.
    pub fn run_analysis(dataset: &HourlyDataset) -> MLInsightsReport {
        let report = HourlyAnalyzer::analyze(dataset);

        match &report.result {
            AnalysisResult::Success(insights) => {
                info!(
                    well_id = %report.well_id,
                    formation = %report.formation_type,
                    confidence = %insights.confidence,
                    sample_count = insights.sample_count,
                    correlations = insights.correlations.len(),
                    composite_score = insights.optimal_params.composite_score,
                    "ML analysis complete"
                );
            }
            AnalysisResult::Failure(failure) => {
                warn!(
                    well_id = %report.well_id,
                    reason = %failure,
                    "ML analysis failed"
                );
            }
        }

        report
    }

    /// Build a dataset from history data
    ///
    /// This helper creates an HourlyDataset from raw packets/metrics.
    /// In full integration, this would pull from HistoryStorage.
    pub fn build_dataset(
        packets: Vec<crate::types::WitsPacket>,
        metrics: Vec<crate::types::DrillingMetrics>,
        well_id: &str,
        field_name: &str,
        campaign: Campaign,
        bit_hours: f64,
        bit_depth: f64,
        cfc_transition_timestamps: &[u64],
        regime_centroids: [[f64; 8]; 4],
    ) -> HourlyDataset {
        use std::collections::HashMap;

        let time_range = (
            packets.first().map(|p| p.timestamp).unwrap_or(0),
            packets.last().map(|p| p.timestamp).unwrap_or(0),
        );

        let avg_depth = if packets.is_empty() {
            0.0
        } else {
            packets.iter().map(|p| p.bit_depth).sum::<f64>() / packets.len() as f64
        };

        // Count rig states
        let mut rig_states_breakdown = HashMap::new();
        for packet in &packets {
            *rig_states_breakdown.entry(packet.rig_state).or_insert(0) += 1;
        }

        // Estimate formation from d-exponent average
        let avg_d_exp = if packets.is_empty() {
            1.5
        } else {
            packets.iter().map(|p| p.d_exponent).sum::<f64>() / packets.len() as f64
        };
        let formation_estimate = crate::ml_engine::FormationSegmenter::estimate_formation(avg_d_exp);

        HourlyDataset {
            packets,
            metrics,
            time_range,
            avg_depth,
            formation_estimate,
            campaign,
            rig_states_breakdown,
            well_id: well_id.to_string(),
            field_name: field_name.to_string(),
            bit_hours,
            bit_depth,
            rejected_sample_count: 0,
            formation_segments: Vec::new(),
            cfc_transition_timestamps: cfc_transition_timestamps.to_vec(),
            regime_centroids,
        }
    }
}

impl Default for MLScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AnomalyCategory, DrillingMetrics, RigState, WitsPacket};
    use std::sync::Arc;

    fn make_packet(wob: f64, rpm: f64, rop: f64) -> WitsPacket {
        WitsPacket {
            timestamp: 1000,
            bit_depth: 5000.0,
            hole_depth: 5000.0,
            rop,
            hook_load: 200.0,
            wob,
            rpm,
            torque: 10.0,
            bit_diameter: 8.5,
            spp: 3000.0,
            pump_spm: 60.0,
            flow_in: 500.0,
            flow_out: 495.0,
            pit_volume: 800.0,
            pit_volume_change: 0.0,
            mud_weight_in: 10.0,
            mud_weight_out: 10.1,
            ecd: 10.5,
            mud_temp_in: 80.0,
            mud_temp_out: 95.0,
            gas_units: 10.0,
            background_gas: 5.0,
            connection_gas: 0.0,
            h2s: 0.0,
            co2: 0.0,
            casing_pressure: 100.0,
            annular_pressure: 150.0,
            pore_pressure: 8.6,
            fracture_gradient: 14.0,
            mse: 20000.0,
            d_exponent: 1.5,
            dxc: 1.4,
            rop_delta: 0.0,
            torque_delta_percent: 0.0,
            spp_delta: 0.0,
            rig_state: RigState::Drilling,
            regime_id: 0,
            seconds_since_param_change: 0,        }
    }

    fn make_metric(mse: f64, mse_efficiency: f64) -> DrillingMetrics {
        DrillingMetrics {
            state: RigState::Drilling,
            operation: crate::types::Operation::ProductionDrilling,
            mse,
            mse_efficiency,
            d_exponent: 1.5,
            dxc: 1.4,
            mse_delta_percent: 0.0,
            flow_balance: 0.0,
            pit_rate: 0.0,
            ecd_margin: 1.0,
            torque_delta_percent: 0.0,
            spp_delta: 0.0,
            flow_data_available: true,
            is_anomaly: false,
            anomaly_category: AnomalyCategory::None,
            anomaly_description: None,
            current_formation: None,
            formation_depth_in_ft: None,
        }
    }

    #[test]
    fn test_default_interval() {
        // Default should be 3600 seconds (1 hour) from config
        let config = crate::config::WellConfig::default();
        assert_eq!(config.ml.interval_secs, 3600);
    }

    #[test]
    fn test_scheduler_creation() {
        let _scheduler = MLScheduler::new();
        // Scheduler uses env-configured interval (default 3600s)
    }

    #[test]
    fn test_build_dataset() {
        let packets: Vec<_> = (0..100)
            .map(|i| make_packet(20.0, 100.0, 50.0 + i as f64))
            .collect();
        let metrics: Vec<_> = (0..100)
            .map(|_| make_metric(20000.0, 75.0))
            .collect();

        let dataset = MLScheduler::build_dataset(
            packets,
            metrics,
            "TEST-WELL",
            "TEST-FIELD",
            Campaign::Production,
            48.0,
            1200.0,
            &[],
            [[0.0; 8]; 4],
        );

        assert_eq!(dataset.well_id, "TEST-WELL");
        assert_eq!(dataset.field_name, "TEST-FIELD");
        assert_eq!(dataset.packets.len(), 100);
        assert!((dataset.bit_hours - 48.0).abs() < 0.1);
    }

    #[test]
    fn test_run_analysis() {
        // Create enough data for successful analysis with correlation
        let mut packets = Vec::new();
        let mut metrics = Vec::new();

        for i in 0..500 {
            let wob = 15.0 + (i % 20) as f64;
            let rop = wob * 2.0; // Strong correlation
            packets.push(make_packet(wob, 100.0, rop));
            metrics.push(make_metric(20000.0, 75.0));
        }

        let dataset = MLScheduler::build_dataset(
            packets,
            metrics,
            "TEST-WELL",
            "TEST-FIELD",
            Campaign::Production,
            24.0,
            500.0,
            &[],
            [[0.0; 8]; 4],
        );

        let report = MLScheduler::run_analysis(&dataset);

        assert_eq!(report.well_id, "TEST-WELL");
        assert_eq!(report.field_name, "TEST-FIELD");
        assert!(matches!(report.result, AnalysisResult::Success(_)));
    }

    #[test]
    fn test_run_analysis_with_insufficient_data() {
        let packets: Vec<_> = (0..50)
            .map(|_| make_packet(20.0, 100.0, 50.0))
            .collect();
        let metrics: Vec<_> = (0..50)
            .map(|_| make_metric(20000.0, 75.0))
            .collect();

        let dataset = MLScheduler::build_dataset(
            packets,
            metrics,
            "TEST-WELL",
            "TEST-FIELD",
            Campaign::Production,
            24.0,
            500.0,
            &[],
            [[0.0; 8]; 4],
        );

        let report = MLScheduler::run_analysis(&dataset);

        assert!(matches!(report.result, AnalysisResult::Failure(_)));
    }
}
