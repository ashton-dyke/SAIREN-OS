//! Formation Boundary Segmentation (V2)
//!
//! Detects formation boundaries by analyzing d-exponent shifts.
//! A formation boundary is detected when d-exponent changes >15% over a rolling window.
//!
//! This prevents averaging optimal parameters across different rock types,
//! which would produce invalid recommendations.

use crate::types::{ml_quality_thresholds::FORMATION_BOUNDARY_SHIFT, FormationSegment, WitsPacket};

/// Formation boundary detector and segmenter
pub struct FormationSegmenter;

impl FormationSegmenter {
    /// Detect formation boundaries and split into segments
    ///
    /// A boundary is detected when d-exponent shifts >15% over 60 samples (1-minute window).
    /// Returns a list of formation segments, each with estimated formation type.
    ///
    /// # Arguments
    /// * `packets` - Slice of valid WITS packets (post quality filter)
    ///
    /// # Returns
    /// Vector of FormationSegment, each representing a contiguous zone
    pub fn segment(packets: &[&WitsPacket]) -> Vec<FormationSegment> {
        if packets.len() < 120 {
            // Too few samples to detect boundaries reliably
            // Return single segment covering all data
            return vec![Self::single_segment(packets, 0)];
        }

        let mut segments = Vec::new();
        let mut segment_start = 0;
        let window_size = 60; // 1-minute rolling window at 1 Hz

        // Extract d-exponent values
        let d_exp_values: Vec<f64> = packets.iter().map(|p| p.d_exponent).collect();

        for i in window_size..packets.len() {
            // Compare previous half-window to current half-window
            let prev_start = i - window_size;
            let prev_end = i - window_size / 2;
            let curr_start = prev_end;
            let curr_end = i;

            let prev_avg = Self::mean(&d_exp_values[prev_start..prev_end]);
            let curr_avg = Self::mean(&d_exp_values[curr_start..curr_end]);

            // Check for >15% shift
            if prev_avg > 0.1 {
                // Avoid division by near-zero
                let shift = (curr_avg - prev_avg).abs() / prev_avg;
                if shift > FORMATION_BOUNDARY_SHIFT {
                    // Formation boundary detected - close previous segment
                    if curr_start > segment_start {
                        segments.push(Self::create_segment(
                            packets,
                            &d_exp_values,
                            segment_start,
                            curr_start,
                        ));
                    }
                    segment_start = curr_start;
                }
            }
        }

        // Add final segment
        if packets.len() > segment_start {
            segments.push(Self::create_segment(
                packets,
                &d_exp_values,
                segment_start,
                packets.len(),
            ));
        }

        // Ensure we have at least one segment
        if segments.is_empty() {
            segments.push(Self::single_segment(packets, 0));
        }

        segments
    }

    /// Create a segment for a given range
    fn create_segment(
        _packets: &[&WitsPacket],
        d_exp_values: &[f64],
        start: usize,
        end: usize,
    ) -> FormationSegment {
        let avg_d_exp = Self::mean(&d_exp_values[start..end]);
        FormationSegment {
            packet_range: (start, end),
            formation_type: Self::estimate_formation(avg_d_exp),
            avg_d_exponent: avg_d_exp,
            valid_sample_count: end - start,
        }
    }

    /// Create a single segment from all packets
    fn single_segment(packets: &[&WitsPacket], start_offset: usize) -> FormationSegment {
        let d_exp_values: Vec<f64> = packets.iter().map(|p| p.d_exponent).collect();
        let avg = Self::mean(&d_exp_values);
        FormationSegment {
            packet_range: (start_offset, start_offset + packets.len()),
            formation_type: Self::estimate_formation(avg),
            avg_d_exponent: avg,
            valid_sample_count: packets.len(),
        }
    }

    /// Estimate formation type based on d-exponent value
    ///
    /// D-exponent correlates with formation hardness:
    /// - Low d-exp (< 1.2): Soft formations (shale, clay)
    /// - Medium d-exp (1.2-1.6): Medium formations (siltstone, soft sandstone)
    /// - High d-exp (1.6-2.0): Hard formations (sandstone, limestone)
    /// - Very high d-exp (> 2.0): Very hard formations (dolomite, granite)
    pub fn estimate_formation(avg_d_exp: f64) -> String {
        match avg_d_exp {
            d if d < 1.2 => "Soft Shale".to_string(),
            d if d < 1.6 => "Medium Shale/Siltstone".to_string(),
            d if d < 2.0 => "Hard Sandstone".to_string(),
            _ => "Very Hard Formation".to_string(),
        }
    }

    /// Calculate mean of a slice
    fn mean(values: &[f64]) -> f64 {
        if values.is_empty() {
            0.0
        } else {
            values.iter().sum::<f64>() / values.len() as f64
        }
    }

    /// Detect formation boundaries using both d-exponent shifts AND CfC transition timestamps.
    ///
    /// Merges boundaries from two sources:
    /// 1. D-exponent >15% shift detector (existing algorithm)
    /// 2. CfC feature surprise timestamps (early indicator)
    ///
    /// Duplicate/near-duplicate boundaries are deduplicated.
    pub fn segment_with_cfc_boundaries(
        packets: &[&WitsPacket],
        cfc_transition_timestamps: &[u64],
    ) -> Vec<FormationSegment> {
        if packets.len() < 120 {
            return vec![Self::single_segment(packets, 0)];
        }

        // Step 1: Get d-exponent boundary indices
        let d_exp_segments = Self::segment(packets);
        let mut boundary_indices: Vec<usize> = d_exp_segments
            .iter()
            .map(|s| s.packet_range.0)
            .filter(|&idx| idx > 0) // skip the first segment start (always 0)
            .collect();

        // Step 2: Convert CfC timestamps to packet indices
        for &ts in cfc_transition_timestamps {
            if let Some(idx) = packets.iter().position(|p| p.timestamp >= ts) {
                boundary_indices.push(idx);
            }
        }

        // Step 3: Deduplicate and sort
        boundary_indices.sort_unstable();
        boundary_indices.dedup();
        // Remove boundaries that are too close together (within 30 samples)
        let mut deduped = Vec::new();
        for &idx in &boundary_indices {
            if deduped.last().map_or(true, |&last: &usize| idx.saturating_sub(last) >= 30) {
                deduped.push(idx);
            }
        }

        // Step 4: Split into segments
        let d_exp_values: Vec<f64> = packets.iter().map(|p| p.d_exponent).collect();
        let mut segments = Vec::new();
        let mut prev = 0;
        for &boundary in &deduped {
            if boundary > prev && boundary < packets.len() {
                segments.push(Self::create_segment(packets, &d_exp_values, prev, boundary));
                prev = boundary;
            }
        }
        // Final segment
        if prev < packets.len() {
            segments.push(Self::create_segment(packets, &d_exp_values, prev, packets.len()));
        }

        if segments.is_empty() {
            segments.push(Self::single_segment(packets, 0));
        }

        segments
    }

    /// Check if multiple segments indicate unstable formation
    ///
    /// Returns true if there are multiple segments and the largest
    /// segment is too small for reliable analysis.
    pub fn is_unstable(segments: &[FormationSegment], min_samples: usize) -> bool {
        if segments.len() <= 1 {
            return false;
        }

        // Check if largest segment has enough samples
        let max_segment_size = segments
            .iter()
            .map(|s| s.valid_sample_count)
            .max()
            .unwrap_or(0);

        max_segment_size < min_samples
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RigState;
    use std::sync::Arc;

    fn make_packet_with_d_exp(d_exp: f64) -> WitsPacket {
        WitsPacket {
            timestamp: 1000,
            bit_depth: 5000.0,
            hole_depth: 5000.0,
            rop: 50.0,
            hook_load: 200.0,
            wob: 20.0,
            rpm: 120.0,
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
            regime_id: 0,
            seconds_since_param_change: 0,        }
    }

    #[test]
    fn test_single_formation_no_boundary() {
        // Create 200 samples with consistent d-exponent (no boundary)
        let packets: Vec<_> = (0..200).map(|_| make_packet_with_d_exp(1.5)).collect();
        let packet_refs: Vec<_> = packets.iter().collect();

        let segments = FormationSegmenter::segment(&packet_refs);

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].valid_sample_count, 200);
        assert!((segments[0].avg_d_exponent - 1.5).abs() < 0.01);
    }

    #[test]
    fn test_formation_boundary_detection() {
        // Create 200 samples: first 100 with d-exp 1.5, next 100 with d-exp 2.0
        // This is a 33% shift, which should trigger boundary detection
        let mut packets: Vec<WitsPacket> = Vec::new();
        for _ in 0..100 {
            packets.push(make_packet_with_d_exp(1.5));
        }
        for _ in 0..100 {
            packets.push(make_packet_with_d_exp(2.0));
        }
        let packet_refs: Vec<_> = packets.iter().collect();

        let segments = FormationSegmenter::segment(&packet_refs);

        // Should detect at least 2 segments
        assert!(
            segments.len() >= 2,
            "Expected at least 2 segments, got {}",
            segments.len()
        );

        // First segment should have lower d-exp
        assert!(
            segments[0].avg_d_exponent < 1.7,
            "First segment d-exp should be ~1.5"
        );

        // Last segment should have higher d-exp
        let last = segments.last().expect("should have segments");
        assert!(
            last.avg_d_exponent > 1.8,
            "Last segment d-exp should be ~2.0"
        );
    }

    #[test]
    fn test_small_dataset_single_segment() {
        // Create only 50 samples (less than 120 minimum for boundary detection)
        let packets: Vec<_> = (0..50).map(|_| make_packet_with_d_exp(1.5)).collect();
        let packet_refs: Vec<_> = packets.iter().collect();

        let segments = FormationSegmenter::segment(&packet_refs);

        // Should return single segment for small datasets
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].valid_sample_count, 50);
    }

    #[test]
    fn test_formation_type_estimation() {
        assert_eq!(
            FormationSegmenter::estimate_formation(1.0),
            "Soft Shale"
        );
        assert_eq!(
            FormationSegmenter::estimate_formation(1.4),
            "Medium Shale/Siltstone"
        );
        assert_eq!(
            FormationSegmenter::estimate_formation(1.8),
            "Hard Sandstone"
        );
        assert_eq!(
            FormationSegmenter::estimate_formation(2.5),
            "Very Hard Formation"
        );
    }

    #[test]
    fn test_unstable_formation_detection() {
        // Simulate segments that are too small for analysis
        let small_segments = vec![
            FormationSegment {
                packet_range: (0, 100),
                formation_type: "Soft Shale".to_string(),
                avg_d_exponent: 1.2,
                valid_sample_count: 100,
            },
            FormationSegment {
                packet_range: (100, 200),
                formation_type: "Hard Sandstone".to_string(),
                avg_d_exponent: 1.8,
                valid_sample_count: 100,
            },
        ];

        // With min_samples = 360, both segments are too small
        assert!(FormationSegmenter::is_unstable(&small_segments, 360));

        // With min_samples = 50, segments are large enough
        assert!(!FormationSegmenter::is_unstable(&small_segments, 50));
    }

    #[test]
    fn test_single_segment_not_unstable() {
        let single_segment = vec![FormationSegment {
            packet_range: (0, 100),
            formation_type: "Soft Shale".to_string(),
            avg_d_exponent: 1.2,
            valid_sample_count: 100,
        }];

        // Single segment is never "unstable"
        assert!(!FormationSegmenter::is_unstable(&single_segment, 360));
    }

    #[test]
    fn test_gradual_shift_no_boundary() {
        // Create samples with gradual d-exponent increase (less than 15% per window)
        let packets: Vec<_> = (0..200)
            .map(|i| {
                // Increase from 1.5 to 1.6 over 200 samples (6.7% total, gradual)
                let d_exp = 1.5 + (i as f64 * 0.0005);
                make_packet_with_d_exp(d_exp)
            })
            .collect();
        let packet_refs: Vec<_> = packets.iter().collect();

        let segments = FormationSegmenter::segment(&packet_refs);

        // Gradual shift should not trigger boundary detection
        assert_eq!(
            segments.len(),
            1,
            "Gradual shift should not create multiple segments"
        );
    }
}
