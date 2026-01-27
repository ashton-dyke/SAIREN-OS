//! FFT computation using rustfft
//!
//! Optimized FFT for real-time vibration analysis of TDS-11SA top drive.
//!
//! # Features
//!
//! - Pre-planned FFT for consistent performance
//! - TDS-11SA bearing frequency calculations (BPFO/BPFI/BSF/FTF)
//! - Peak extraction with configurable bandwidth
//! - LLM-friendly text formatting for AI analysis
//!
//! # Example
//!
//! ```ignore
//! use tds_guardian::processing::{compute_fft, format_for_llm, calculate_bearing_frequencies};
//!
//! let samples: Vec<f64> = collect_vibration_data();
//! let spectrum = compute_fft(&samples, 10000.0)?;
//! let bearing_freqs = calculate_bearing_frequencies(100.0); // 100 RPM
//! let llm_text = format_for_llm(&spectrum, 100.0);
//! ```

use ndarray::Array1;
use num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::{FrequencySpectrum, ProcessingError};

// ============================================================================
// TDS-11SA Bearing Geometry Constants
// ============================================================================

/// TDS-11SA main bearing geometry (typical values for large top drive bearings)
/// These are based on SKF 29434 thrust bearing specifications commonly used in top drives.
pub mod tds11sa_geometry {
    /// Number of rolling elements (balls/rollers)
    pub const N_ELEMENTS: f64 = 18.0;
    /// Ball/roller diameter in mm
    pub const BALL_DIAMETER: f64 = 38.0;
    /// Pitch diameter in mm (centerline of rolling elements)
    pub const PITCH_DIAMETER: f64 = 280.0;
    /// Contact angle in degrees (typical for angular contact bearings)
    pub const CONTACT_ANGLE_DEG: f64 = 15.0;
}

// ============================================================================
// Bearing Frequencies
// ============================================================================

/// Characteristic bearing fault frequencies for TDS-11SA.
///
/// These frequencies are calculated based on bearing geometry and shaft RPM.
/// When a bearing defect exists, vibration energy appears at these frequencies.
///
/// # ISO 10816-3 Reference
///
/// Class I machinery (large rotating equipment like top drives) should have:
/// - Zone A (New/Excellent): < 2.8 mm/s RMS
/// - Zone B (Acceptable): 2.8 - 7.1 mm/s RMS  
/// - Zone C (Unsatisfactory): 7.1 - 18.0 mm/s RMS
/// - Zone D (Unacceptable): > 18.0 mm/s RMS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BearingFrequencies {
    /// Shaft rotational frequency (Hz) = RPM / 60
    pub shaft_frequency: f64,

    /// Ball Pass Frequency Outer race (BPFO) - Hz
    /// Defect on outer race produces impulses at this frequency
    pub bpfo: f64,

    /// Ball Pass Frequency Inner race (BPFI) - Hz
    /// Defect on inner race produces modulated impulses
    pub bpfi: f64,

    /// Ball Spin Frequency (BSF) - Hz
    /// Defect on rolling element produces this frequency (×2 for impact)
    pub bsf: f64,

    /// Fundamental Train Frequency (FTF) / Cage frequency - Hz
    /// Cage defect or lubrication issues
    pub ftf: f64,

    /// 1× RPM component - fundamental unbalance
    pub one_x: f64,

    /// 2× RPM component - misalignment indicator
    pub two_x: f64,

    /// Operating RPM used for calculation
    pub rpm: f64,
}

impl BearingFrequencies {
    /// Format bearing frequencies for LLM consumption.
    pub fn format_for_llm(&self) -> String {
        format!(
            "Operating Speed: {:.1} RPM ({:.2} Hz)\n\
             Key Frequencies:\n\
             - 1×: {:.2} Hz (unbalance)\n\
             - 2×: {:.2} Hz (misalignment)\n\
             - BPFO: {:.2} Hz (outer race defect)\n\
             - BPFI: {:.2} Hz (inner race defect)\n\
             - BSF: {:.2} Hz (rolling element defect)\n\
             - FTF: {:.2} Hz (cage defect)",
            self.rpm,
            self.shaft_frequency,
            self.one_x,
            self.two_x,
            self.bpfo,
            self.bpfi,
            self.bsf,
            self.ftf
        )
    }
}

/// Calculate characteristic bearing frequencies for TDS-11SA at given RPM.
///
/// Uses standard bearing frequency formulas with TDS-11SA geometry constants.
///
/// # Arguments
/// * `rpm` - Shaft rotational speed in revolutions per minute
///
/// # Returns
/// [`BearingFrequencies`] struct with all characteristic frequencies
///
/// # Example
/// ```ignore
/// let freqs = calculate_bearing_frequencies(100.0);
/// println!("BPFO at 100 RPM: {:.2} Hz", freqs.bpfo);
/// ```
pub fn calculate_bearing_frequencies(rpm: f64) -> BearingFrequencies {
    use std::f64::consts::PI;
    use tds11sa_geometry::*;

    let shaft_freq = rpm / 60.0; // Hz
    let contact_angle_rad = CONTACT_ANGLE_DEG * PI / 180.0;
    let d_over_d = BALL_DIAMETER / PITCH_DIAMETER;
    let cos_angle = contact_angle_rad.cos();

    // BPFO = (N/2) × Fr × (1 - d/D × cos(θ))
    let bpfo = (N_ELEMENTS / 2.0) * shaft_freq * (1.0 - d_over_d * cos_angle);

    // BPFI = (N/2) × Fr × (1 + d/D × cos(θ))
    let bpfi = (N_ELEMENTS / 2.0) * shaft_freq * (1.0 + d_over_d * cos_angle);

    // BSF = (D/2d) × Fr × (1 - (d/D × cos(θ))²)
    let bsf = (PITCH_DIAMETER / (2.0 * BALL_DIAMETER))
        * shaft_freq
        * (1.0 - (d_over_d * cos_angle).powi(2));

    // FTF = (1/2) × Fr × (1 - d/D × cos(θ))
    let ftf = 0.5 * shaft_freq * (1.0 - d_over_d * cos_angle);

    BearingFrequencies {
        shaft_frequency: shaft_freq,
        bpfo,
        bpfi,
        bsf,
        ftf,
        one_x: shaft_freq,
        two_x: shaft_freq * 2.0,
        rpm,
    }
}

// ============================================================================
// Standalone FFT Functions
// ============================================================================

/// Compute FFT of time-domain samples.
///
/// This is the primary entry point for FFT computation. It automatically
/// selects an appropriate FFT size and applies proper normalization.
///
/// # Arguments
/// * `samples` - Time-domain vibration samples (acceleration in g)
/// * `sample_rate` - Sampling rate in Hz
///
/// # Returns
/// [`FrequencySpectrum`] with frequencies, magnitudes, and phases
///
/// # Example
/// ```ignore
/// let samples: Vec<f64> = read_accelerometer_data();
/// let spectrum = compute_fft(&samples, 10000.0)?; // 10 kHz sample rate
/// ```
pub fn compute_fft(
    samples: &[f64],
    sample_rate: f64,
) -> Result<FrequencySpectrum, ProcessingError> {
    if samples.is_empty() {
        return Err(ProcessingError::InsufficientData {
            needed: 1,
            available: 0,
        });
    }

    if sample_rate <= 0.0 {
        return Err(ProcessingError::InvalidSamplingRate(sample_rate));
    }

    // Use next power of 2 for FFT efficiency
    let fft_size = samples.len().next_power_of_two();
    let processor = FftProcessor::new(fft_size, sample_rate)?;

    // Zero-pad if necessary
    let mut padded = Array1::zeros(fft_size);
    for (i, &s) in samples.iter().enumerate().take(fft_size) {
        padded[i] = s;
    }

    processor.compute(&padded)
}

/// Extract peak amplitude at a specific frequency with given bandwidth.
///
/// Finds the maximum amplitude within ±bandwidth/2 of the target frequency.
/// This is useful for extracting bearing fault frequency amplitudes.
///
/// # Arguments
/// * `spectrum` - Frequency spectrum from FFT
/// * `target_freq` - Center frequency to search (Hz)
/// * `bandwidth` - Search bandwidth (Hz) - searches ±bandwidth/2
///
/// # Returns
/// Maximum amplitude found within the frequency band
///
/// # Example
/// ```ignore
/// let bpfo_amplitude = extract_peak_amplitude(&spectrum, bearing_freqs.bpfo, 5.0);
/// ```
pub fn extract_peak_amplitude(
    spectrum: &FrequencySpectrum,
    target_freq: f64,
    bandwidth: f64,
) -> f64 {
    let half_bw = bandwidth / 2.0;
    let low_freq = target_freq - half_bw;
    let high_freq = target_freq + half_bw;

    spectrum
        .frequencies
        .iter()
        .zip(spectrum.magnitudes.iter())
        .filter(|(&f, _)| f >= low_freq && f <= high_freq)
        .map(|(_, &m)| m)
        .fold(0.0_f64, f64::max)
}

/// Find the frequency of peak amplitude within a band.
///
/// Returns both the frequency and amplitude of the maximum within the band.
pub fn find_peak_in_band(
    spectrum: &FrequencySpectrum,
    low_freq: f64,
    high_freq: f64,
) -> Option<(f64, f64)> {
    spectrum
        .frequencies
        .iter()
        .zip(spectrum.magnitudes.iter())
        .filter(|(&f, _)| f >= low_freq && f <= high_freq)
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(&f, &m)| (f, m))
}

/// Extract amplitude by summing energy across harmonics (OPTION 2).
///
/// Bearing faults often spread energy across harmonic frequencies (1x, 2x, 3x BPFO).
/// This function sums the RMS of amplitudes at each harmonic to capture total fault energy.
///
/// # Arguments
/// * `spectrum` - Frequency spectrum from FFT
/// * `base_freq` - Fundamental fault frequency (e.g., BPFO)
/// * `bandwidth` - Search bandwidth around each harmonic (Hz)
/// * `num_harmonics` - Number of harmonics to sum (default: 3)
///
/// # Returns
/// Total RMS amplitude across all harmonics: sqrt(amp1² + amp2² + amp3²)
pub fn extract_harmonic_amplitude(
    spectrum: &FrequencySpectrum,
    base_freq: f64,
    bandwidth: f64,
    num_harmonics: usize,
) -> f64 {
    let mut sum_squares = 0.0;

    for h in 1..=num_harmonics {
        let harmonic_freq = base_freq * h as f64;
        let amp = extract_peak_amplitude(spectrum, harmonic_freq, bandwidth);
        sum_squares += amp * amp;

        tracing::trace!(
            harmonic = h,
            freq = harmonic_freq,
            amplitude = amp,
            "OPTION 2: Harmonic amplitude"
        );
    }

    let total = sum_squares.sqrt();
    tracing::debug!(
        base_freq = base_freq,
        num_harmonics = num_harmonics,
        total_harmonic_amplitude = total,
        "OPTION 2: Total harmonic amplitude"
    );

    total
}

/// Calculate RMS (Root Mean Square) of spectrum magnitudes.
///
/// This provides an overall vibration level metric.
pub fn calculate_spectrum_rms(spectrum: &FrequencySpectrum) -> f64 {
    if spectrum.magnitudes.is_empty() {
        return 0.0;
    }

    let sum_squares: f64 = spectrum.magnitudes.iter().map(|m| m * m).sum();
    (sum_squares / spectrum.magnitudes.len() as f64).sqrt()
}

/// Format spectrum data for LLM consumption.
///
/// Converts the frequency spectrum into a human-readable text format
/// that an LLM can interpret for health assessment.
///
/// # Arguments
/// * `spectrum` - Frequency spectrum from FFT
/// * `rpm` - Current operating RPM
///
/// # Returns
/// Formatted string suitable for LLM prompt
///
/// # Example Output
/// ```text
/// Vibration Spectrum Analysis
/// ===========================
/// Overall RMS: 0.85 g
/// Frequency Range: 0.0 - 5000.0 Hz
///
/// Top 5 Frequency Peaks:
/// 1. 98.5 Hz: 0.234 g (likely 1× RPM component)
/// 2. 197.2 Hz: 0.156 g (likely 2× RPM component)
/// 3. 742.8 Hz: 0.089 g
/// 4. 1485.6 Hz: 0.067 g
/// 5. 2228.4 Hz: 0.045 g
///
/// Bearing Frequency Amplitudes:
/// - BPFO (742.5 Hz): 0.089 g
/// - BPFI (864.2 Hz): 0.023 g
/// - BSF (312.1 Hz): 0.012 g
/// - FTF (41.3 Hz): 0.008 g
/// ```
pub fn format_for_llm(spectrum: &FrequencySpectrum, rpm: f64) -> String {
    let rms = calculate_spectrum_rms(spectrum);
    let peaks = find_dominant_frequencies(spectrum, 5);
    let bearing_freqs = calculate_bearing_frequencies(rpm);

    let max_freq = spectrum.frequencies.last().copied().unwrap_or(0.0);

    let mut output = String::new();
    output.push_str("Vibration Spectrum Analysis\n");
    output.push_str("===========================\n");
    output.push_str(&format!("Overall RMS: {:.4} g\n", rms));
    output.push_str(&format!("Frequency Range: 0.0 - {:.1} Hz\n\n", max_freq));

    // Add bearing frequencies context
    output.push_str(&bearing_freqs.format_for_llm());
    output.push_str("\n\n");

    // Top peaks
    output.push_str("Top 5 Frequency Peaks:\n");
    for (i, (freq, amp)) in peaks.iter().enumerate() {
        let mut note = String::new();

        // Annotate if near known frequencies
        if (freq - bearing_freqs.one_x).abs() < 2.0 {
            note = " (1× RPM - unbalance indicator)".to_string();
        } else if (freq - bearing_freqs.two_x).abs() < 2.0 {
            note = " (2× RPM - misalignment indicator)".to_string();
        } else if (freq - bearing_freqs.bpfo).abs() < 5.0 {
            note = " (near BPFO - outer race defect)".to_string();
        } else if (freq - bearing_freqs.bpfi).abs() < 5.0 {
            note = " (near BPFI - inner race defect)".to_string();
        } else if (freq - bearing_freqs.bsf).abs() < 3.0 {
            note = " (near BSF - rolling element defect)".to_string();
        } else if (freq - bearing_freqs.ftf).abs() < 2.0 {
            note = " (near FTF - cage frequency)".to_string();
        }

        output.push_str(&format!(
            "{}. {:.1} Hz: {:.4} g{}\n",
            i + 1,
            freq,
            amp,
            note
        ));
    }

    // Extract bearing frequency amplitudes
    output.push_str("\nBearing Frequency Amplitudes:\n");
    let bpfo_amp = extract_peak_amplitude(spectrum, bearing_freqs.bpfo, 5.0);
    let bpfi_amp = extract_peak_amplitude(spectrum, bearing_freqs.bpfi, 5.0);
    let bsf_amp = extract_peak_amplitude(spectrum, bearing_freqs.bsf, 3.0);
    let ftf_amp = extract_peak_amplitude(spectrum, bearing_freqs.ftf, 2.0);

    output.push_str(&format!(
        "- BPFO ({:.1} Hz): {:.4} g\n",
        bearing_freqs.bpfo, bpfo_amp
    ));
    output.push_str(&format!(
        "- BPFI ({:.1} Hz): {:.4} g\n",
        bearing_freqs.bpfi, bpfi_amp
    ));
    output.push_str(&format!(
        "- BSF ({:.1} Hz): {:.4} g\n",
        bearing_freqs.bsf, bsf_amp
    ));
    output.push_str(&format!(
        "- FTF ({:.1} Hz): {:.4} g\n",
        bearing_freqs.ftf, ftf_amp
    ));

    output
}

/// Generate a comparison summary between current and baseline spectra.
///
/// This is useful for trending and anomaly detection.
pub fn format_comparison_for_llm(
    current: &FrequencySpectrum,
    baseline: &FrequencySpectrum,
    rpm: f64,
) -> String {
    let current_rms = calculate_spectrum_rms(current);
    let baseline_rms = calculate_spectrum_rms(baseline);
    let rms_change_pct = if baseline_rms > 0.0 {
        ((current_rms - baseline_rms) / baseline_rms) * 100.0
    } else {
        0.0
    };

    let bearing_freqs = calculate_bearing_frequencies(rpm);

    let mut output = String::new();
    output.push_str("Spectrum Comparison (Current vs Baseline)\n");
    output.push_str("=========================================\n\n");

    output.push_str(&format!(
        "Overall RMS: {:.4} g (baseline: {:.4} g, change: {:+.1}%)\n\n",
        current_rms, baseline_rms, rms_change_pct
    ));

    // Compare bearing frequencies
    output.push_str("Bearing Frequency Changes:\n");

    let freqs_to_check = [
        ("BPFO", bearing_freqs.bpfo, 5.0),
        ("BPFI", bearing_freqs.bpfi, 5.0),
        ("BSF", bearing_freqs.bsf, 3.0),
        ("FTF", bearing_freqs.ftf, 2.0),
        ("1×", bearing_freqs.one_x, 2.0),
        ("2×", bearing_freqs.two_x, 2.0),
    ];

    for (name, freq, bw) in freqs_to_check {
        let curr_amp = extract_peak_amplitude(current, freq, bw);
        let base_amp = extract_peak_amplitude(baseline, freq, bw);
        let change_pct = if base_amp > 1e-9 {
            ((curr_amp - base_amp) / base_amp) * 100.0
        } else {
            0.0
        };

        let trend = if change_pct > 20.0 {
            "⚠ INCREASING"
        } else if change_pct < -20.0 {
            "↓ decreasing"
        } else {
            "→ stable"
        };

        output.push_str(&format!(
            "- {} ({:.1} Hz): {:.4} g (was {:.4} g, {:+.1}%) {}\n",
            name, freq, curr_amp, base_amp, change_pct, trend
        ));
    }

    // Current spectrum details
    output.push_str("\n--- Current Spectrum Details ---\n");
    output.push_str(&format_for_llm(current, rpm));

    output
}

// ============================================================================
// FFT Processor (Pre-planned for repeated use)
// ============================================================================

/// FFT processor with pre-planned transforms for repeated computation.
///
/// Use this when computing many FFTs of the same size for better performance.
pub struct FftProcessor {
    fft: Arc<dyn Fft<f64>>,
    size: usize,
    sampling_rate: f64,
}

impl FftProcessor {
    /// Create a new FFT processor
    ///
    /// # Arguments
    /// * `size` - FFT size (will use next power of 2 internally)
    /// * `sampling_rate` - Sampling rate in Hz
    pub fn new(size: usize, sampling_rate: f64) -> Result<Self, ProcessingError> {
        if sampling_rate <= 0.0 {
            return Err(ProcessingError::InvalidSamplingRate(sampling_rate));
        }

        let actual_size = size.next_power_of_two();
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(actual_size);

        Ok(Self {
            fft,
            size: actual_size,
            sampling_rate,
        })
    }

    /// Compute FFT of a real-valued signal
    ///
    /// # Arguments
    /// * `signal` - Input signal (time domain)
    ///
    /// # Returns
    /// Frequency spectrum with magnitudes and phases
    pub fn compute(&self, signal: &Array1<f64>) -> Result<FrequencySpectrum, ProcessingError> {
        if signal.len() < self.size {
            return Err(ProcessingError::InsufficientData {
                needed: self.size,
                available: signal.len(),
            });
        }

        // Convert to complex
        let mut buffer: Vec<Complex<f64>> = signal
            .iter()
            .take(self.size)
            .map(|&x| Complex::new(x, 0.0))
            .collect();

        // Compute FFT in-place
        self.fft.process(&mut buffer);

        // Extract positive frequencies only (Nyquist)
        let n_positive = self.size / 2 + 1;
        let freq_resolution = self.sampling_rate / self.size as f64;

        let frequencies: Vec<f64> = (0..n_positive)
            .map(|i| i as f64 * freq_resolution)
            .collect();

        // Apply proper scaling: 2/N for one-sided spectrum (except DC and Nyquist)
        let magnitudes: Vec<f64> = buffer
            .iter()
            .take(n_positive)
            .enumerate()
            .map(|(i, c)| {
                let scale = if i == 0 || i == n_positive - 1 {
                    1.0 / self.size as f64
                } else {
                    2.0 / self.size as f64
                };
                c.norm() * scale
            })
            .collect();

        // Calculate RMS
        let rms =
            (magnitudes.iter().map(|x| x.powi(2)).sum::<f64>() / magnitudes.len() as f64).sqrt();

        // Find peak frequency
        let peak_idx = magnitudes
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(i, _)| i)
            .unwrap_or(0);
        let peak_frequency = frequencies.get(peak_idx).copied().unwrap_or(0.0);

        Ok(FrequencySpectrum {
            frequencies,
            magnitudes,
            rms,
            peak_frequency,
            sample_rate: self.sampling_rate,
            timestamp: chrono::Utc::now(),
        })
    }

    /// Get frequency bins for this FFT configuration
    pub fn frequency_bins(&self) -> Vec<f64> {
        let n_positive = self.size / 2 + 1;
        let freq_resolution = self.sampling_rate / self.size as f64;
        (0..n_positive)
            .map(|i| i as f64 * freq_resolution)
            .collect()
    }

    /// Get the FFT size
    pub fn size(&self) -> usize {
        self.size
    }

    /// Get the frequency resolution (Hz per bin)
    pub fn frequency_resolution(&self) -> f64 {
        self.sampling_rate / self.size as f64
    }
}

/// Find dominant frequencies in a spectrum using true peak detection.
///
/// Identifies local maxima (peaks) where the amplitude is higher than
/// both neighboring bins, then returns the top N by amplitude.
///
/// # Arguments
/// * `spectrum` - Frequency spectrum
/// * `n_peaks` - Number of peaks to find
///
/// # Returns
/// Vector of (frequency, magnitude) tuples sorted by magnitude descending
pub fn find_dominant_frequencies(spectrum: &FrequencySpectrum, n_peaks: usize) -> Vec<(f64, f64)> {
    if spectrum.magnitudes.len() < 3 {
        return spectrum
            .frequencies
            .iter()
            .zip(spectrum.magnitudes.iter())
            .map(|(&f, &m)| (f, m))
            .collect();
    }

    // Find local maxima (true peaks)
    let mut peaks: Vec<(f64, f64)> = Vec::new();

    for i in 1..spectrum.magnitudes.len() - 1 {
        let prev = spectrum.magnitudes[i - 1];
        let curr = spectrum.magnitudes[i];
        let next = spectrum.magnitudes[i + 1];

        if curr > prev && curr > next {
            peaks.push((spectrum.frequencies[i], curr));
        }
    }

    // Sort by magnitude descending
    peaks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    peaks.truncate(n_peaks);
    peaks
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array1;
    use std::f64::consts::PI;

    #[test]
    fn test_bearing_frequencies_100rpm() {
        let freqs = calculate_bearing_frequencies(100.0);

        assert!((freqs.shaft_frequency - 100.0 / 60.0).abs() < 0.01);
        assert!((freqs.one_x - 100.0 / 60.0).abs() < 0.01);
        assert!((freqs.two_x - 200.0 / 60.0).abs() < 0.01);

        // BPFO should be roughly N/2 × Fr for typical geometry
        assert!(freqs.bpfo > 10.0); // Reasonable value
        assert!(freqs.bpfi > freqs.bpfo); // BPFI > BPFO always

        println!("Bearing frequencies at 100 RPM:");
        println!("{}", freqs.format_for_llm());
    }

    #[test]
    fn test_fft_processor_creation() {
        let processor = FftProcessor::new(1024, 10000.0);
        assert!(processor.is_ok());

        let processor = processor.unwrap();
        assert_eq!(processor.size(), 1024);
        assert!((processor.frequency_resolution() - 10000.0 / 1024.0).abs() < 0.001);
    }

    #[test]
    fn test_fft_sine_wave() {
        let processor = FftProcessor::new(1024, 1000.0).expect("Failed to create processor");

        // Generate 100Hz sine wave
        let freq = 100.0;
        let signal: Array1<f64> =
            Array1::from_iter((0..1024).map(|i| (2.0 * PI * freq * i as f64 / 1000.0).sin()));

        let spectrum = processor.compute(&signal).expect("FFT failed");

        // Find peak - should be near 100Hz
        let peaks = find_dominant_frequencies(&spectrum, 1);
        assert!(!peaks.is_empty());

        // Peak should be within ±5Hz of 100Hz
        let (peak_freq, _) = peaks[0];
        assert!((peak_freq - 100.0).abs() < 5.0);
    }

    #[test]
    fn test_compute_fft_standalone() {
        // Generate simple sine wave at 50 Hz
        let sample_rate = 1000.0;
        let samples: Vec<f64> = (0..512)
            .map(|i| (2.0 * PI * 50.0 * i as f64 / sample_rate).sin())
            .collect();

        let spectrum = compute_fft(&samples, sample_rate).expect("FFT failed");

        assert!(!spectrum.frequencies.is_empty());
        assert_eq!(spectrum.frequencies.len(), spectrum.magnitudes.len());

        // Find dominant frequency
        let peaks = find_dominant_frequencies(&spectrum, 1);
        let (peak_freq, _) = peaks[0];
        assert!(
            (peak_freq - 50.0).abs() < 5.0,
            "Peak at {}, expected ~50 Hz",
            peak_freq
        );
    }

    #[test]
    fn test_extract_peak_amplitude() {
        let spectrum = FrequencySpectrum {
            frequencies: vec![0.0, 10.0, 20.0, 30.0, 40.0, 50.0],
            magnitudes: vec![0.1, 0.2, 0.5, 0.3, 0.15, 0.1],
            rms: 0.0,
            peak_frequency: 0.0,
            sample_rate: 20.0,
            timestamp: chrono::Utc::now(),
        };

        // Peak at 20 Hz should be 0.5
        let amp = extract_peak_amplitude(&spectrum, 20.0, 5.0);
        assert!((amp - 0.5).abs() < 0.001);

        // Wider bandwidth should still find it
        let amp = extract_peak_amplitude(&spectrum, 25.0, 20.0);
        assert!((amp - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_spectrum_rms() {
        let spectrum = FrequencySpectrum {
            frequencies: vec![0.0, 10.0, 20.0],
            magnitudes: vec![1.0, 1.0, 1.0],
            rms: 0.0,
            peak_frequency: 0.0,
            sample_rate: 20.0,
            timestamp: chrono::Utc::now(),
        };

        let rms = calculate_spectrum_rms(&spectrum);
        assert!((rms - 1.0).abs() < 0.001); // RMS of [1,1,1] = 1
    }

    #[test]
    fn test_format_for_llm() {
        // Create a simple spectrum
        let spectrum = FrequencySpectrum {
            frequencies: (0..100).map(|i| i as f64 * 10.0).collect(),
            magnitudes: (0..100).map(|i| if i == 10 { 0.5 } else { 0.01 }).collect(),
            rms: 0.0,
            peak_frequency: 0.0,
            sample_rate: 20.0,
            timestamp: chrono::Utc::now(),
        };

        let text = format_for_llm(&spectrum, 100.0);

        // Should contain key sections
        assert!(text.contains("Vibration Spectrum Analysis"));
        assert!(text.contains("Overall RMS"));
        assert!(text.contains("Top 5 Frequency Peaks"));
        assert!(text.contains("BPFO"));
        assert!(text.contains("BPFI"));

        println!("LLM formatted output:\n{}", text);
    }

    #[test]
    fn test_format_comparison_for_llm() {
        let baseline = FrequencySpectrum {
            frequencies: (0..50).map(|i| i as f64 * 20.0).collect(),
            magnitudes: vec![0.1; 50],
            rms: 0.0,
            peak_frequency: 0.0,
            sample_rate: 20.0,
            timestamp: chrono::Utc::now(),
        };

        let current = FrequencySpectrum {
            frequencies: (0..50).map(|i| i as f64 * 20.0).collect(),
            magnitudes: (0..50).map(|i| if i == 5 { 0.3 } else { 0.12 }).collect(), // 20% increase
            rms: 0.0,
            peak_frequency: 0.0,
            sample_rate: 20.0,
            timestamp: chrono::Utc::now(),
        };

        let comparison = format_comparison_for_llm(&current, &baseline, 100.0);

        assert!(comparison.contains("Comparison"));
        assert!(comparison.contains("change"));

        println!("Comparison output:\n{}", comparison);
    }

    #[test]
    fn test_find_peak_in_band() {
        let spectrum = FrequencySpectrum {
            frequencies: vec![10.0, 20.0, 30.0, 40.0, 50.0],
            magnitudes: vec![0.1, 0.5, 0.3, 0.4, 0.2],
            rms: 0.0,
            peak_frequency: 0.0,
            sample_rate: 20.0,
            timestamp: chrono::Utc::now(),
        };

        // Find peak in 15-35 Hz band
        let result = find_peak_in_band(&spectrum, 15.0, 35.0);
        assert!(result.is_some());
        let (freq, amp) = result.unwrap();
        assert!((freq - 20.0).abs() < 0.001);
        assert!((amp - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_true_peak_detection() {
        // Create spectrum with clear peaks
        let mut magnitudes = vec![0.1; 100];
        magnitudes[10] = 0.5; // Peak at bin 10
        magnitudes[30] = 0.8; // Peak at bin 30
        magnitudes[50] = 0.3; // Peak at bin 50

        let spectrum = FrequencySpectrum {
            frequencies: (0..100).map(|i| i as f64).collect(),
            magnitudes,
            rms: 0.0,
            peak_frequency: 0.0,
            sample_rate: 20.0,
            timestamp: chrono::Utc::now(),
        };

        let peaks = find_dominant_frequencies(&spectrum, 3);

        assert_eq!(peaks.len(), 3);
        assert!((peaks[0].0 - 30.0).abs() < 0.001); // Highest peak at 30
        assert!((peaks[1].0 - 10.0).abs() < 0.001); // Second at 10
        assert!((peaks[2].0 - 50.0).abs() < 0.001); // Third at 50
    }
}
