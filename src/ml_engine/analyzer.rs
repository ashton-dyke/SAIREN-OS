//! Core ML Analyzer & Report Builder (V2.2)
//!
//! Main orchestrator for ML analysis that:
//! 1. Applies quality filtering (WOB>5, RPM>40, valid MSE/ROP)
//! 2. **Applies dysfunction filtering** (reject stick-slip, pack-off, founder samples)
//! 3. Segments by formation boundaries
//! 4. Calculates correlations (relaxed requirements in V2.2)
//! 5. Finds optimal parameters using grid-based binning with stability penalty
//! 6. Builds MLInsightsReport with success or explicit failure reasons

use crate::types::{
    ml_quality_thresholds::MIN_ANALYSIS_SAMPLES, AnalysisFailure, AnalysisInsights,
    AnalysisResult, ConfidenceLevel, HourlyDataset, MLInsightsReport, SignificantCorrelation,
};

use super::{
    correlations::CorrelationEngine,
    dysfunction_filter::DysfunctionFilter,
    formation_segmenter::FormationSegmenter,
    optimal_finder::OptimalFinder,
    quality_filter::DataQualityFilter,
};

/// Core ML analyzer that orchestrates the full analysis pipeline
pub struct HourlyAnalyzer;

impl HourlyAnalyzer {
    /// Run full analysis pipeline on dataset
    ///
    /// # Pipeline Steps (V2.2)
    /// 1. Quality filtering (WOB>5, RPM>40, valid MSE/ROP)
    /// 2. Dysfunction filtering (reject stick-slip, pack-off, founder samples)
    /// 3. Formation segmentation (detect >15% d-exp shifts)
    /// 4. Correlation analysis (relaxed - proceed even if p > 0.05)
    /// 5. Optimal parameter finding (grid-based binning with stability penalty)
    /// 6. Report building (success or explicit failure)
    ///
    /// # Returns
    /// MLInsightsReport with either AnalysisResult::Success or AnalysisResult::Failure
    pub fn analyze(dataset: &HourlyDataset) -> MLInsightsReport {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Step 1: Quality filtering
        let filter_result = DataQualityFilter::filter(&dataset.packets, &dataset.metrics);

        // Check for all data rejected
        if filter_result.valid_packets.is_empty() {
            return Self::build_failure_report(
                dataset,
                timestamp,
                AnalysisFailure::AllDataRejected {
                    rejection_reason: filter_result
                        .rejection_reason
                        .unwrap_or_else(|| "No samples passed quality filter".to_string()),
                },
            );
        }

        // V2: Check minimum sample requirement (before dysfunction filter)
        if filter_result.valid_packets.len() < MIN_ANALYSIS_SAMPLES {
            return Self::build_failure_report(
                dataset,
                timestamp,
                AnalysisFailure::InsufficientData {
                    valid_samples: filter_result.valid_packets.len(),
                    required: MIN_ANALYSIS_SAMPLES,
                },
            );
        }

        // Step 2: Dysfunction filtering (V2.2)
        // Reject samples where instability was detected (stick-slip, pack-off, founder, etc.)
        let dysfunction_result = DysfunctionFilter::filter(
            &filter_result.valid_packets,
            &filter_result.valid_metrics,
        );

        let dysfunction_filtered = dysfunction_result.rejected_count > 0;

        // Check if too many samples were dysfunctional
        if dysfunction_result.stable_packets.len() < MIN_ANALYSIS_SAMPLES {
            // If we have very few stable samples, report this as a special failure
            let primary = dysfunction_result
                .rejection_breakdown
                .primary_reason()
                .map(|(r, _)| r.to_string())
                .unwrap_or_else(|| "Unknown".to_string());

            return Self::build_failure_report(
                dataset,
                timestamp,
                AnalysisFailure::AllDataRejected {
                    rejection_reason: format!(
                        "Dysfunction filtering rejected {} of {} samples (primary: {}). \
                         Only {} stable samples remain (need {})",
                        dysfunction_result.rejected_count,
                        filter_result.valid_packets.len(),
                        primary,
                        dysfunction_result.stable_packets.len(),
                        MIN_ANALYSIS_SAMPLES
                    ),
                },
            );
        }

        // Step 3: Formation segmentation (on stable samples only)
        let segments = FormationSegmenter::segment(&dysfunction_result.stable_packets);

        // V2: Check for unstable formation
        if FormationSegmenter::is_unstable(&segments, MIN_ANALYSIS_SAMPLES) {
            let max_segment = segments
                .iter()
                .map(|s| s.valid_sample_count)
                .max()
                .unwrap_or(0);
            return Self::build_failure_report(
                dataset,
                timestamp,
                AnalysisFailure::UnstableFormation {
                    segment_count: segments.len(),
                    max_segment_size: max_segment,
                },
            );
        }

        // Use largest segment for analysis
        let best_segment = segments
            .iter()
            .max_by_key(|s| s.valid_sample_count)
            .expect("should have at least one segment");

        let (start, end) = best_segment.packet_range;
        let segment_packets: Vec<_> = dysfunction_result.stable_packets[start..end].to_vec();
        let segment_metrics: Vec<_> = dysfunction_result.stable_metrics[start..end].to_vec();

        // Step 4: Correlation analysis (V2.2: relaxed requirements)
        // We still calculate correlations for insights, but don't fail if none are significant
        let (correlations, _best_p) =
            CorrelationEngine::analyze_drilling_correlations(&segment_packets);

        // V2.2: Instead of failing on no correlations, just flag as low confidence
        // This allows optimization to proceed even in variable drilling conditions
        let low_correlation_confidence = correlations.is_empty();

        // Step 5: Optimal parameter finding with grid-based binning and stability penalty
        let optimal_params = match OptimalFinder::find_optimal(
            &segment_packets,
            &segment_metrics,
            dataset.campaign,
            dysfunction_filtered,
        ) {
            Some(params) => params,
            None => {
                return Self::build_failure_report(
                    dataset,
                    timestamp,
                    AnalysisFailure::InsufficientData {
                        valid_samples: segment_packets.len(),
                        required: MIN_ANALYSIS_SAMPLES,
                    },
                );
            }
        };

        // Step 6: Build success report
        // Confidence is based on sample count but downgraded if correlations were weak
        let mut confidence = ConfidenceLevel::from_sample_count(segment_packets.len());
        if low_correlation_confidence && confidence == ConfidenceLevel::High {
            confidence = ConfidenceLevel::Medium;
        }

        let summary = Self::build_summary(
            &optimal_params,
            &correlations,
            &best_segment.formation_type,
            dataset,
            confidence,
            dysfunction_result.stability_score,
            low_correlation_confidence,
        );

        // Determine depth range from segment
        let depth_range = (
            segment_packets
                .first()
                .map(|p| p.bit_depth)
                .unwrap_or(dataset.avg_depth),
            segment_packets
                .last()
                .map(|p| p.bit_depth)
                .unwrap_or(dataset.avg_depth),
        );

        MLInsightsReport {
            timestamp,
            campaign: dataset.campaign,
            depth_range,
            well_id: dataset.well_id.clone(),
            field_name: dataset.field_name.clone(),
            bit_hours: dataset.bit_hours,
            bit_depth: dataset.bit_depth,
            formation_type: best_segment.formation_type.clone(),
            result: AnalysisResult::Success(AnalysisInsights {
                optimal_params,
                correlations,
                summary_text: summary,
                confidence,
                sample_count: segment_packets.len(),
            }),
        }
    }

    /// Build a failure report
    fn build_failure_report(
        dataset: &HourlyDataset,
        timestamp: u64,
        failure: AnalysisFailure,
    ) -> MLInsightsReport {
        MLInsightsReport {
            timestamp,
            campaign: dataset.campaign,
            depth_range: (dataset.avg_depth, dataset.avg_depth),
            well_id: dataset.well_id.clone(),
            field_name: dataset.field_name.clone(),
            bit_hours: dataset.bit_hours,
            bit_depth: dataset.bit_depth,
            formation_type: dataset.formation_estimate.clone(),
            result: AnalysisResult::Failure(failure),
        }
    }

    /// Build natural language summary for LLM context (V2.2)
    fn build_summary(
        params: &crate::types::OptimalParams,
        correlations: &[SignificantCorrelation],
        formation: &str,
        dataset: &HourlyDataset,
        confidence: ConfidenceLevel,
        stability_score: f64,
        low_correlation_confidence: bool,
    ) -> String {
        let confidence_str = match confidence {
            ConfidenceLevel::High => "HIGH confidence",
            ConfidenceLevel::Medium => "MEDIUM confidence",
            ConfidenceLevel::Low => "LOW confidence (use with caution)",
            ConfidenceLevel::Insufficient => "INSUFFICIENT data",
        };

        // V2.2: Interpret composite score for LLM context
        let efficiency_rating = OptimalFinder::interpret_composite_score(params.composite_score);

        // V2.2: Stability interpretation
        let stability_str = if stability_score > 0.90 {
            "excellent stability"
        } else if stability_score > 0.75 {
            "good stability"
        } else if stability_score > 0.60 {
            "moderate stability (some dysfunction filtered)"
        } else {
            "marginal stability (significant dysfunction filtered)"
        };

        // Find strongest correlation for summary (or note if none)
        let correlation_str = if low_correlation_confidence {
            "Weak correlations detected - recommendations based on binned performance".to_string()
        } else {
            correlations
                .iter()
                .max_by(|a, b| {
                    a.r_value
                        .abs()
                        .partial_cmp(&b.r_value.abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|c| {
                    format!(
                        "{} shows r={:.2} correlation with {} (p={:.4})",
                        c.x_param, c.r_value, c.y_param, c.p_value
                    )
                })
                .unwrap_or_else(|| "No significant correlations".to_string())
        };

        // V2.2: Include operating ranges in summary
        format!(
            "ML Analysis for {} in {} formation ({}, bit: {:.0}hrs/{:.0}ft). \
             Optimal: WOB={:.1} klbs [{:.1}-{:.1}], RPM={:.0} [{:.0}-{:.0}], Flow={:.0} gpm [{:.0}-{:.0}]. \
             Achieved ROP={:.1} ft/hr with MSE efficiency {:.0}% \
             (composite score: {:.2} - {}, {}). \
             Key finding: {}.",
            dataset.well_id,
            formation,
            confidence_str,
            dataset.bit_hours,
            dataset.bit_depth,
            params.best_wob,
            params.wob_min,
            params.wob_max,
            params.best_rpm,
            params.rpm_min,
            params.rpm_max,
            params.best_flow,
            params.flow_min,
            params.flow_max,
            params.achieved_rop,
            params.mse_efficiency,
            params.composite_score,
            efficiency_rating,
            stability_str,
            correlation_str
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AnomalyCategory, Campaign, DrillingMetrics, RigState, WitsPacket};
    use std::collections::HashMap;
    use std::sync::Arc;

    fn make_packet(wob: f64, rpm: f64, rop: f64, d_exp: f64) -> WitsPacket {
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
            d_exponent: d_exp,
            dxc: d_exp * 0.95,
            rop_delta: 0.0,
            torque_delta_percent: 0.0,
            spp_delta: 0.0,
            rig_state: RigState::Drilling,
            waveform_snapshot: Arc::new(Vec::new()),
        }
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
            is_anomaly: false,
            anomaly_category: AnomalyCategory::None,
            anomaly_description: None,
        }
    }

    fn make_dataset(packets: Vec<WitsPacket>, metrics: Vec<DrillingMetrics>) -> HourlyDataset {
        HourlyDataset {
            packets,
            metrics,
            time_range: (0, 3600),
            avg_depth: 5000.0,
            formation_estimate: "Test Formation".to_string(),
            campaign: Campaign::Production,
            rig_states_breakdown: HashMap::new(),
            well_id: "WELL-001".to_string(),
            field_name: "TEST-FIELD".to_string(),
            bit_hours: 24.0,
            bit_depth: 500.0,
            rejected_sample_count: 0,
            formation_segments: Vec::new(),
        }
    }

    #[test]
    fn test_successful_analysis() {
        // Create dataset with enough valid samples and correlated data
        let mut packets = Vec::new();
        let mut metrics = Vec::new();

        for i in 0..500 {
            let wob = 15.0 + (i % 20) as f64;
            let rpm = 100.0 + (i % 10) as f64;
            let rop = wob * 2.0 + rpm * 0.5; // Strong correlation
            packets.push(make_packet(wob, rpm, rop, 1.5));
            metrics.push(make_metric(20000.0, 70.0 + (i % 15) as f64));
        }

        let dataset = make_dataset(packets, metrics);
        let report = HourlyAnalyzer::analyze(&dataset);

        match report.result {
            AnalysisResult::Success(insights) => {
                assert!(!insights.correlations.is_empty(), "Should have correlations");
                assert!(insights.sample_count >= 360, "Should have enough samples");
                assert!(
                    !insights.summary_text.is_empty(),
                    "Should have summary"
                );
            }
            AnalysisResult::Failure(f) => {
                panic!("Expected success, got failure: {}", f);
            }
        }
    }

    #[test]
    fn test_insufficient_data_failure() {
        // Create dataset with too few samples
        let packets: Vec<_> = (0..100)
            .map(|i| make_packet(20.0, 100.0, 50.0 + i as f64, 1.5))
            .collect();
        let metrics: Vec<_> = (0..100)
            .map(|_| make_metric(20000.0, 75.0))
            .collect();

        let dataset = make_dataset(packets, metrics);
        let report = HourlyAnalyzer::analyze(&dataset);

        match report.result {
            AnalysisResult::Failure(AnalysisFailure::InsufficientData { valid_samples, required }) => {
                assert!(valid_samples < required);
            }
            other => panic!("Expected InsufficientData failure, got {:?}", other),
        }
    }

    #[test]
    fn test_all_data_rejected_failure() {
        // Create dataset with all invalid samples (WOB too low)
        let packets: Vec<_> = (0..500)
            .map(|_| make_packet(2.0, 100.0, 50.0, 1.5)) // WOB < 5
            .collect();
        let metrics: Vec<_> = (0..500)
            .map(|_| make_metric(20000.0, 75.0))
            .collect();

        let dataset = make_dataset(packets, metrics);
        let report = HourlyAnalyzer::analyze(&dataset);

        match report.result {
            AnalysisResult::Failure(AnalysisFailure::AllDataRejected { .. }) => {
                // Expected
            }
            other => panic!("Expected AllDataRejected failure, got {:?}", other),
        }
    }

    #[test]
    fn test_report_contains_well_info() {
        let mut packets = Vec::new();
        let mut metrics = Vec::new();

        for i in 0..500 {
            let wob = 15.0 + (i % 20) as f64;
            let rop = wob * 2.0;
            packets.push(make_packet(wob, 100.0, rop, 1.5));
            metrics.push(make_metric(20000.0, 75.0));
        }

        let mut dataset = make_dataset(packets, metrics);
        dataset.well_id = "TEST-WELL-42".to_string();
        dataset.field_name = "NORTH-SEA".to_string();
        dataset.bit_hours = 48.0;
        dataset.bit_depth = 1200.0;

        let report = HourlyAnalyzer::analyze(&dataset);

        assert_eq!(report.well_id, "TEST-WELL-42");
        assert_eq!(report.field_name, "NORTH-SEA");
        assert!((report.bit_hours - 48.0).abs() < 0.1);
        assert!((report.bit_depth - 1200.0).abs() < 0.1);
    }

    #[test]
    fn test_summary_contains_key_info() {
        let mut packets = Vec::new();
        let mut metrics = Vec::new();

        for i in 0..500 {
            let wob = 15.0 + (i % 20) as f64;
            let rop = wob * 2.0;
            packets.push(make_packet(wob, 100.0, rop, 1.5));
            metrics.push(make_metric(20000.0, 75.0));
        }

        let dataset = make_dataset(packets, metrics);
        let report = HourlyAnalyzer::analyze(&dataset);

        if let AnalysisResult::Success(insights) = report.result {
            assert!(insights.summary_text.contains("WOB="));
            assert!(insights.summary_text.contains("RPM="));
            assert!(insights.summary_text.contains("ROP="));
            assert!(insights.summary_text.contains("WELL-001"));
        }
    }
}
