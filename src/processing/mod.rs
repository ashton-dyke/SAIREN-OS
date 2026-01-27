//! Signal processing module - FFT computation for vibration analysis

#![allow(dead_code)]

mod fft;
mod health_scoring;

pub use fft::*;
pub use health_scoring::{calculate_health_score, calculate_health_score_with_buffer};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors in signal processing
#[derive(Error, Debug)]
pub enum ProcessingError {
    #[error("Insufficient data: need {needed}, have {available}")]
    InsufficientData { needed: usize, available: usize },

    #[error("FFT error: {0}")]
    FftError(String),

    #[error("Invalid sampling rate: {0}")]
    InvalidSamplingRate(f64),
}

/// Frequency spectrum data from FFT analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrequencySpectrum {
    /// Frequency bins (Hz)
    pub frequencies: Vec<f64>,
    /// Magnitude at each frequency
    pub magnitudes: Vec<f64>,
    /// RMS value
    pub rms: f64,
    /// Peak frequency
    pub peak_frequency: f64,
    /// Sample rate used
    pub sample_rate: f64,
    /// Timestamp of analysis
    pub timestamp: chrono::DateTime<chrono::Utc>,
}
