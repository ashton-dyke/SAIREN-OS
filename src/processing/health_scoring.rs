//! Health Scoring Module
//!
//! Deterministic, rule-based health score calculation for TDS-11SA top drive equipment.
//! This module replaces LLM-based health scoring to eliminate hallucination and ensure
//! consistent, trustworthy scores based on sensor thresholds.
//!
//! The LLM focuses on what it does best: diagnosis and recommended actions.
//! The health score is calculated using industry-standard thresholds and weighted factors.

use crate::processing::FrequencySpectrum;

/// Calculate health score (0-100) based on vibration, temperature, and fault metrics.
///
/// # Scoring Algorithm
///
/// The health score is a weighted average of multiple factors:
/// - 30% Vibration RMS (ISO 10816-3 zones)
/// - 40% Bearing fault indicators (BPFO/BPFI absolute amplitude thresholds)
/// - 20% Temperature status (motor and gearbox)
/// - 10% Spectral changes (current vs baseline comparison)
///
/// Bearing faults have the highest weight (40%) because they are the most
/// critical indicator for rotating machinery health and can cause catastrophic
/// failure if not addressed.
///
/// # Arguments
///
/// * `current` - Current frequency spectrum
/// * `baseline` - Baseline frequency spectrum for comparison
/// * `motor_temps` - Motor temperature readings [4 sensors] in °C
/// * `gearbox_temps` - Gearbox temperature readings [2 sensors] in °C
/// * `bpfo_amp` - Ball Pass Frequency Outer race amplitude
/// * `bpfi_amp` - Ball Pass Frequency Inner race amplitude
///
/// # Returns
///
/// Tuple of (health_score, severity_string)
/// - health_score: 0-100 (100 = perfect health)
/// - severity: "Healthy", "Watch", "Warning", or "Critical"
pub fn calculate_health_score(
    current: &FrequencySpectrum,
    baseline: &FrequencySpectrum,
    motor_temps: &[f64; 4],
    gearbox_temps: &[f64; 2],
    bpfo_amp: f64,
    bpfi_amp: f64,
) -> (f64, String) {
    // Call extended version with no buffer std (backward compatible)
    calculate_health_score_with_buffer(current, baseline, motor_temps, gearbox_temps, bpfo_amp, bpfi_amp, None)
}

/// Calculate health score with optional buffer-based amplitude estimation.
///
/// When `buffer_std` is provided, it's used as an alternative amplitude estimate
/// that's more robust to FFT frequency smearing from RPM variation.
/// Buffer std * sqrt(2) ≈ peak amplitude for sinusoidal signals.
pub fn calculate_health_score_with_buffer(
    current: &FrequencySpectrum,
    baseline: &FrequencySpectrum,
    motor_temps: &[f64; 4],
    gearbox_temps: &[f64; 2],
    bpfo_amp: f64,
    bpfi_amp: f64,
    buffer_std: Option<f64>,
) -> (f64, String) {
    // 1. Vibration RMS score (30% weight) - ISO 10816-3 Class I machinery
    let vibration_score = score_vibration_rms(current.rms);

    // 2. Bearing fault score (40% weight) - critical for rotating machinery
    // OPTION 1: Use buffer std as amplitude estimate when FFT underdetects
    // Buffer std * sqrt(2) ≈ peak amplitude for sinusoidal faults
    let effective_fault_amp = if let Some(buf_std) = buffer_std {
        // Estimate peak amplitude from buffer std (RMS of AC component)
        // For a pure sine wave: std = amplitude / sqrt(2), so amplitude = std * sqrt(2)
        let buffer_estimated_amp = buf_std * std::f64::consts::SQRT_2;
        // Use the HIGHER of FFT-detected or buffer-estimated amplitude
        // This ensures we don't miss faults due to FFT frequency smearing
        let max_fft_amp = bpfo_amp.max(bpfi_amp);
        let effective = buffer_estimated_amp.max(max_fft_amp);
        tracing::debug!(
            buffer_std = buf_std,
            buffer_estimated_amp = buffer_estimated_amp,
            fft_bpfo = bpfo_amp,
            fft_bpfi = bpfi_amp,
            effective_amp = effective,
            "OPTION 1: Using buffer std for amplitude estimation"
        );
        effective
    } else {
        bpfo_amp.max(bpfi_amp)
    };

    // Use the effective amplitude for both BPFO and BPFI scoring
    let bearing_score = score_bearing_faults(effective_fault_amp, effective_fault_amp, baseline);

    // 3. Temperature score (20% weight)
    let temp_score = score_temperatures(motor_temps, gearbox_temps);

    // 4. Spectral change score (10% weight)
    let spectral_score = score_spectral_changes(current, baseline);

    // Weighted average
    // Bearing faults are critical for rotating machinery - increased weight from 25% to 40%
    let health_score = (vibration_score * 0.30)
        + (bearing_score * 0.40)
        + (temp_score * 0.20)
        + (spectral_score * 0.10);

    // CRITICAL: Apply floor based on bearing fault severity
    // Bearing faults are catastrophic failures - they should not be masked by other factors
    // If bearing_score indicates Alert/Critical, cap the overall health score
    let health_score = if bearing_score < 25.0 {
        // Critical bearing fault (> 0.5g): cap at 35 (Critical severity)
        health_score.min(35.0)
    } else if bearing_score < 50.0 {
        // Alert bearing fault (0.2-0.5g): cap at 55 (Warning severity)
        health_score.min(55.0)
    } else if bearing_score < 70.0 {
        // Warning bearing fault (0.1-0.2g): cap at 70 (Watch severity)
        health_score.min(70.0)
    } else {
        health_score
    };

    // Clamp to 0-100 range
    let health_score = health_score.max(0.0).min(100.0);

    // Determine severity from score
    let severity = severity_from_score(health_score);

    (health_score, severity)
}

/// Score vibration RMS based on ISO 10816-3 Class I machinery standards.
///
/// Zones:
/// - Zone A (Excellent): < 2.8 mm/s RMS → 95-100 points
/// - Zone B (Acceptable): 2.8 - 7.1 mm/s → 70-95 points
/// - Zone C (Unsatisfactory): 7.1 - 18.0 mm/s → 30-70 points
/// - Zone D (Unacceptable): > 18.0 mm/s → 0-30 points
fn score_vibration_rms(rms: f64) -> f64 {
    if rms < 2.8 {
        // Zone A: Excellent
        // Linear scale: 0 mm/s = 100, 2.8 mm/s = 95
        100.0 - (rms / 2.8) * 5.0
    } else if rms < 7.1 {
        // Zone B: Acceptable
        // Linear scale: 2.8 = 95, 7.1 = 70
        95.0 - ((rms - 2.8) / (7.1 - 2.8)) * 25.0
    } else if rms < 18.0 {
        // Zone C: Unsatisfactory
        // Linear scale: 7.1 = 70, 18.0 = 30
        70.0 - ((rms - 7.1) / (18.0 - 7.1)) * 40.0
    } else {
        // Zone D: Unacceptable
        // Exponential decay: 18.0 = 30, approaches 0
        30.0 * ((-0.1 * (rms - 18.0)).exp())
    }
}

/// Score bearing fault indicators based on ABSOLUTE amplitude thresholds.
///
/// Thresholds are calibrated 3x more sensitive than industry-standard values
/// to compensate for FFT underdetection caused by:
/// - Frequency smearing from RPM variation
/// - Windowing effects
/// - Energy spread across harmonics (partially captured via harmonic summation)
///
/// Calibrated thresholds (with harmonic summation):
/// - Normal:   < 0.03g  → 90-100 points (Healthy)
/// - Watch:    0.03-0.1g → 70-90 points (Watch)
/// - Warning:  0.1-0.2g → 50-70 points (Warning)
/// - Alert:    0.2-0.5g → 25-50 points (Warning/Critical)
/// - Critical: > 0.5g  → 0-25 points (Critical)
///
/// Note: bpfo_amp/bpfi_amp should be harmonic sums (1x+2x+3x) for best results.
fn score_bearing_faults(bpfo_amp: f64, bpfi_amp: f64, _baseline: &FrequencySpectrum) -> f64 {
    // Use the worse (higher) of the two fault amplitudes
    let max_amp = bpfo_amp.max(bpfi_amp);

    // Calibrated thresholds (3x more sensitive than industry standard)
    if max_amp < 0.03 {
        // Normal: < 0.03g - equipment in good condition
        // Linear scale: 0g = 100, 0.03g = 90
        100.0 - (max_amp / 0.03) * 10.0
    } else if max_amp < 0.1 {
        // Watch: 0.03-0.1g - early stage fault developing
        // Linear scale: 0.03g = 90, 0.1g = 70
        90.0 - ((max_amp - 0.03) / 0.07) * 20.0
    } else if max_amp < 0.2 {
        // Warning: 0.1-0.2g - fault progressing, schedule maintenance
        // Linear scale: 0.1g = 70, 0.2g = 50
        70.0 - ((max_amp - 0.1) / 0.1) * 20.0
    } else if max_amp < 0.5 {
        // Alert: 0.2-0.5g - significant fault, maintenance required soon
        // Linear scale: 0.2g = 50, 0.5g = 25
        50.0 - ((max_amp - 0.2) / 0.3) * 25.0
    } else {
        // Critical: > 0.5g - severe fault, immediate action required
        // Exponential decay: 0.5g = 25, approaches 0
        25.0 * ((-1.0 * (max_amp - 0.5)).exp())
    }
}

/// Score temperature status based on TDS-11SA operating limits.
///
/// Motor temperature limits:
/// - Optimal: 50-65°C → 95-100 points
/// - Acceptable: 65-85°C → 70-95 points
/// - Warning: 85-100°C → 40-70 points
/// - Critical: > 100°C → 0-40 points
///
/// Gearbox temperature limits:
/// - Optimal: 45-60°C → 95-100 points
/// - Acceptable: 60-80°C → 70-95 points
/// - Warning: 80-95°C → 40-70 points
/// - Critical: > 95°C → 0-40 points
fn score_temperatures(motor_temps: &[f64; 4], gearbox_temps: &[f64; 2]) -> f64 {
    // Calculate averages
    let motor_avg = motor_temps.iter().sum::<f64>() / motor_temps.len() as f64;
    let gearbox_avg = gearbox_temps.iter().sum::<f64>() / gearbox_temps.len() as f64;

    // Find hottest individual sensor (more critical than average)
    let motor_max = motor_temps.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let gearbox_max = gearbox_temps.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    // Score motor (use worse of avg or max)
    let motor_score = if motor_max < 65.0 {
        100.0 - ((motor_avg - 50.0).max(0.0) / 15.0) * 5.0
    } else if motor_max < 85.0 {
        95.0 - ((motor_max - 65.0) / 20.0) * 25.0
    } else if motor_max < 100.0 {
        70.0 - ((motor_max - 85.0) / 15.0) * 30.0
    } else {
        40.0 * ((-0.05 * (motor_max - 100.0)).exp())
    };

    // Score gearbox (use worse of avg or max)
    let gearbox_score = if gearbox_max < 60.0 {
        100.0 - ((gearbox_avg - 45.0).max(0.0) / 15.0) * 5.0
    } else if gearbox_max < 80.0 {
        95.0 - ((gearbox_max - 60.0) / 20.0) * 25.0
    } else if gearbox_max < 95.0 {
        70.0 - ((gearbox_max - 80.0) / 15.0) * 30.0
    } else {
        40.0 * ((-0.05 * (gearbox_max - 95.0)).exp())
    };

    // Return the worse (lower) of the two scores
    motor_score.min(gearbox_score)
}

/// Score spectral changes from baseline.
///
/// Compares overall spectral energy distribution and identifies:
/// - Broadband noise increases (general wear)
/// - Harmonic content changes (specific faults)
/// - Frequency shifts (alignment issues)
fn score_spectral_changes(current: &FrequencySpectrum, baseline: &FrequencySpectrum) -> f64 {
    // Calculate RMS ratio (overall energy change)
    let rms_ratio = current.rms / baseline.rms.max(0.001);
    let rms_change_pct = ((rms_ratio - 1.0) * 100.0).abs();

    // Calculate peak magnitude change
    let current_peak_mag = current
        .magnitudes
        .iter()
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max);
    let baseline_peak_mag = baseline
        .magnitudes
        .iter()
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max);
    let peak_ratio = current_peak_mag / baseline_peak_mag.max(0.001);
    let peak_change_pct = ((peak_ratio - 1.0) * 100.0).abs();

    // Use the worse (larger) change
    let max_change = rms_change_pct.max(peak_change_pct);

    if max_change < 10.0 {
        // Minimal change: < 10%
        100.0 - (max_change / 10.0) * 5.0
    } else if max_change < 25.0 {
        // Moderate change: 10-25%
        95.0 - ((max_change - 10.0) / 15.0) * 25.0
    } else if max_change < 50.0 {
        // Significant change: 25-50%
        70.0 - ((max_change - 25.0) / 25.0) * 30.0
    } else if max_change < 100.0 {
        // Major change: 50-100%
        40.0 - ((max_change - 50.0) / 50.0) * 25.0
    } else {
        // Extreme change: > 100%
        15.0 * ((-0.01 * (max_change - 100.0)).exp())
    }
}

/// Determine severity level from health score.
///
/// Thresholds:
/// - Healthy: 80-100
/// - Watch: 60-79
/// - Warning: 40-59
/// - Critical: 0-39
fn severity_from_score(score: f64) -> String {
    if score >= 80.0 {
        "Healthy".to_string()
    } else if score >= 60.0 {
        "Watch".to_string()
    } else if score >= 40.0 {
        "Warning".to_string()
    } else {
        "Critical".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_spectrum(rms: f64, peak_mag: f64) -> FrequencySpectrum {
        FrequencySpectrum {
            frequencies: vec![0.0, 10.0, 20.0, 30.0, 40.0, 50.0],
            magnitudes: vec![0.1, 0.2, peak_mag, 0.15, 0.1, 0.05],
            sample_rate: 10000.0,
            rms,
            peak_frequency: 20.0,
            timestamp: chrono::Utc::now(),
        }
    }

    #[test]
    fn test_vibration_score_excellent() {
        let score = score_vibration_rms(1.5); // Zone A
        assert!(score >= 95.0 && score <= 100.0, "Score: {}", score);
    }

    #[test]
    fn test_vibration_score_acceptable() {
        let score = score_vibration_rms(5.0); // Zone B
        assert!(score >= 70.0 && score < 95.0, "Score: {}", score);
    }

    #[test]
    fn test_vibration_score_unsatisfactory() {
        let score = score_vibration_rms(12.0); // Zone C
        assert!(score >= 30.0 && score < 70.0, "Score: {}", score);
    }

    #[test]
    fn test_vibration_score_unacceptable() {
        let score = score_vibration_rms(20.0); // Zone D
        assert!(score < 30.0, "Score: {}", score);
    }

    #[test]
    fn test_bearing_fault_healthy() {
        let baseline = create_test_spectrum(2.0, 0.5);
        // < 0.03g is Normal
        let score = score_bearing_faults(0.02, 0.01, &baseline);
        assert!(score >= 90.0, "Score: {}", score);
    }

    #[test]
    fn test_bearing_fault_watch() {
        let baseline = create_test_spectrum(2.0, 0.5);
        // 0.03-0.1g is Watch
        let score = score_bearing_faults(0.06, 0.04, &baseline);
        assert!(score >= 70.0 && score < 90.0, "Score: {}", score);
    }

    #[test]
    fn test_bearing_fault_warning() {
        let baseline = create_test_spectrum(2.0, 0.5);
        // 0.1-0.2g is Warning
        let score = score_bearing_faults(0.15, 0.12, &baseline);
        assert!(score >= 50.0 && score < 70.0, "Score: {}", score);
    }

    #[test]
    fn test_bearing_fault_alert() {
        let baseline = create_test_spectrum(2.0, 0.5);
        // 0.2-0.5g is Alert
        let score = score_bearing_faults(0.35, 0.25, &baseline);
        assert!(score >= 25.0 && score < 50.0, "Score: {}", score);
    }

    #[test]
    fn test_bearing_fault_critical() {
        let baseline = create_test_spectrum(2.0, 0.5);
        // > 0.5g is Critical
        let score = score_bearing_faults(0.7, 0.5, &baseline);
        assert!(score < 25.0, "Score: {}", score);
    }

    #[test]
    fn test_temperature_optimal() {
        let motor = [55.0, 57.0, 59.0, 61.0];
        let gearbox = [48.0, 51.0];
        let score = score_temperatures(&motor, &gearbox);
        assert!(score >= 95.0, "Score: {}", score);
    }

    #[test]
    fn test_temperature_warning() {
        let motor = [88.0, 90.0, 92.0, 94.0]; // High
        let gearbox = [48.0, 51.0];
        let score = score_temperatures(&motor, &gearbox);
        assert!(score >= 40.0 && score < 70.0, "Score: {}", score);
    }

    #[test]
    fn test_temperature_critical() {
        let motor = [105.0, 108.0, 110.0, 112.0]; // Very high
        let gearbox = [48.0, 51.0];
        let score = score_temperatures(&motor, &gearbox);
        assert!(score < 40.0, "Score: {}", score);
    }

    #[test]
    fn test_spectral_change_minimal() {
        let baseline = create_test_spectrum(2.0, 0.5);
        let current = create_test_spectrum(2.1, 0.52); // ~5% change
        let score = score_spectral_changes(&current, &baseline);
        assert!(score >= 95.0, "Score: {}", score);
    }

    #[test]
    fn test_spectral_change_significant() {
        let baseline = create_test_spectrum(2.0, 0.5);
        let current = create_test_spectrum(2.8, 0.65); // ~40% change
        let score = score_spectral_changes(&current, &baseline);
        assert!(score >= 40.0 && score < 70.0, "Score: {}", score);
    }

    #[test]
    fn test_severity_from_score() {
        assert_eq!(severity_from_score(95.0), "Healthy");
        assert_eq!(severity_from_score(75.0), "Watch");
        assert_eq!(severity_from_score(50.0), "Warning");
        assert_eq!(severity_from_score(25.0), "Critical");
    }

    #[test]
    fn test_full_health_calculation_healthy() {
        let current = create_test_spectrum(1.8, 0.4);
        let baseline = create_test_spectrum(2.0, 0.5);
        let motor = [55.0, 57.0, 59.0, 61.0];
        let gearbox = [48.0, 51.0];

        let (score, severity) = calculate_health_score(
            &current,
            &baseline,
            &motor,
            &gearbox,
            0.02, // BPFO - Normal (< 0.03g)
            0.01, // BPFI - Normal
        );

        assert!(score >= 80.0, "Score: {}", score);
        assert_eq!(severity, "Healthy");
    }

    #[test]
    fn test_full_health_calculation_warning() {
        let current = create_test_spectrum(10.0, 2.0); // High RMS
        let baseline = create_test_spectrum(2.0, 0.5);
        let motor = [85.0, 87.0, 89.0, 91.0]; // High temp
        let gearbox = [78.0, 81.0];

        let (score, severity) = calculate_health_score(
            &current,
            &baseline,
            &motor,
            &gearbox,
            0.3, // BPFO - Alert level (0.2-0.5g)
            0.15, // BPFI - Warning level
        );

        assert!(score >= 40.0 && score < 60.0, "Score: {}", score);
        assert_eq!(severity, "Warning");
    }

    #[test]
    fn test_full_health_calculation_critical_bearing() {
        let current = create_test_spectrum(2.0, 0.5); // Normal vibration
        let baseline = create_test_spectrum(2.0, 0.5);
        let motor = [60.0, 62.0, 64.0, 66.0]; // Normal temp
        let gearbox = [55.0, 58.0];

        let (score, severity) = calculate_health_score(
            &current,
            &baseline,
            &motor,
            &gearbox,
            0.7, // BPFO - Critical (> 0.5g)
            0.4, // BPFI - Alert
        );

        // Critical bearing fault (>0.5g) should cap score at 35
        assert!(score <= 35.0, "Score: {} - should be <= 35 with 0.7g BPFO (critical)", score);
        assert_eq!(severity, "Critical", "Severity should be Critical for 0.7g BPFO");
    }

    #[test]
    fn test_bearing_fault_floor_at_warning() {
        let current = create_test_spectrum(2.0, 0.5); // Normal vibration
        let baseline = create_test_spectrum(2.0, 0.5);
        let motor = [60.0, 62.0, 64.0, 66.0]; // Normal temp
        let gearbox = [55.0, 58.0];

        let (score, severity) = calculate_health_score(
            &current,
            &baseline,
            &motor,
            &gearbox,
            0.25, // BPFO - Alert level (0.2-0.5g)
            0.1, // BPFI - Watch/Warning boundary
        );

        // Alert bearing fault (0.2-0.5g) should cap score at 55
        assert!(score <= 55.0, "Score: {} - should be <= 55 with 0.25g BPFO (alert)", score);
        assert_eq!(severity, "Warning", "Severity should be Warning for 0.25g BPFO");
    }
}
