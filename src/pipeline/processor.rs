//! Vibration Processing Pipeline
//!
//! The core orchestration component that coordinates:
//! - Sensor data buffering (rolling 60-second window)
//! - FFT computation on buffered samples
//! - Baseline learning (first 5 minutes)
//! - LLM-based health analysis
//! - State updates for API/alerts
//!
//! # Architecture
//!
//! The processor maintains a rolling buffer of vibration samples and triggers
//! analysis every 60 seconds (1200 samples at 20 Hz). During the initial
//! learning phase (5 minutes), it builds a baseline spectrum. After learning,
//! each analysis compares the current spectrum against baseline.
//!
//! # Example
//!
//! ```ignore
//! use tds_guardian::pipeline::{VibrationProcessor, AppState};

#![allow(dead_code)]
//! use tds_guardian::director::LlmDirector;
//! use std::sync::Arc;
//! use tokio::sync::RwLock;
//!
//! #[tokio::main]
//! async fn main() {
//!     let llm = LlmDirector::new_disabled();
//!     let mut processor = VibrationProcessor::new(llm);
//!     let app_state = Arc::new(RwLock::new(AppState::default()));
//!     
//!     // Spawn processing task
//!     tokio::spawn(async move {
//!         processor.run(sensor_rx, app_state).await;
//!     });
//! }
//! ```

use crate::acquisition::SensorReading;
use crate::director::{HealthAssessment, LlmDirector, Severity};
use crate::processing::{compute_fft, FrequencySpectrum};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc::Receiver;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

// ============================================================================
// Constants
// ============================================================================

/// Sampling rate in Hz (20 samples per second)
pub const SAMPLE_RATE_HZ: f64 = 100.0;

/// Buffer size for 60-second window (20 Hz × 60 seconds = 1200 samples)
pub const BUFFER_SIZE: usize = 256;  // 2.56 seconds at 100 Hz - shorter window reduces frequency smearing from RPM variation

/// Learning phase duration in samples (60 seconds × 20 Hz = 1200 samples)
pub const LEARNING_SAMPLES: usize = 1200;

/// Number of baseline spectra to average during learning
pub const LEARNING_WINDOWS: usize = 5; // 5 windows of 60 seconds each

// ============================================================================
// Application State
// ============================================================================

/// Shared application state accessible from API handlers and other components.
///
/// This struct is wrapped in `Arc<RwLock<>>` for thread-safe access across
/// the async runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    /// Latest health assessment from the LLM Director
    pub latest_health: Option<HealthAssessment>,

    /// Most recent frequency spectrum
    pub latest_spectrum: Option<FrequencySpectrum>,

    /// Current operating RPM
    pub current_rpm: f64,

    /// System uptime (serializes as seconds)
    #[serde(skip, default = "Instant::now")]
    pub uptime: Instant,

    /// Total number of analyses performed
    pub total_analyses: u64,

    /// Whether the system is in learning phase
    pub learning_phase: bool,

    /// Learning progress (0.0 to 1.0)
    pub learning_progress: f64,

    /// Samples collected during current session
    pub samples_collected: usize,

    /// Last analysis timestamp
    pub last_analysis_time: Option<chrono::DateTime<chrono::Utc>>,

    /// Analysis interval in seconds
    pub analysis_interval_secs: u64,

    /// Current system status
    pub status: SystemStatus,

    /// Motor temperatures (4 sensors) in °C
    pub motor_temps: [f64; 4],

    /// Gearbox temperatures (2 sensors) in °C
    pub gearbox_temps: [f64; 2],

    /// Current hookload in Newtons (for physics calculations)
    pub hookload: f64,

    /// Current flow rate in barrels per minute (bbl/min)
    pub flow_rate: f64,

    /// Bearing L10 life in hours (Time to Failure estimate)
    pub l10_life_hours: f64,

    /// Cumulative damage index (Miner's rule, 0-1 where 1 = theoretical failure)
    pub cumulative_damage: f64,

    /// Wear acceleration factor (2nd derivative of damage history)
    pub wear_acceleration: f64,

    /// Latest verification result from strategic agent
    pub latest_verification: Option<crate::types::VerificationResult>,

    /// Count of verified (confirmed) faults
    pub verified_faults: u64,

    /// Count of rejected fault tickets
    pub rejected_faults: u64,

    /// Latest strategic advisory from drilling analysis
    pub latest_strategic_report: Option<crate::types::StrategicAdvisory>,

    /// Latest strategic advisory (alias for dashboard access)
    pub latest_advisory: Option<crate::types::StrategicAdvisory>,

    /// Latest WITS packet for dashboard display
    pub latest_wits_packet: Option<crate::types::WitsPacket>,

    /// Latest drilling metrics for dashboard display
    pub latest_drilling_metrics: Option<crate::types::DrillingMetrics>,

    /// Current campaign type (Production or P&A)
    pub campaign: crate::types::Campaign,

    /// Campaign-specific thresholds (derived from campaign)
    #[serde(skip)]
    pub campaign_thresholds: crate::types::CampaignThresholds,

    // === ML Engine Fields (V2.1) ===
    /// Well identifier for ML storage
    pub well_id: String,

    /// Field/asset name for cross-well queries
    pub field_name: String,

    /// Cumulative bit hours (for ML context)
    pub bit_hours: f64,

    /// Depth drilled on current bit in ft (for ML context)
    pub bit_depth_drilled: f64,

    /// Latest ML insights report
    pub latest_ml_report: Option<crate::types::MLInsightsReport>,

    /// WITS packet history for ML analysis
    #[serde(skip)]
    pub wits_history: std::collections::VecDeque<crate::types::WitsPacket>,

    /// Drilling metrics history for ML analysis
    #[serde(skip)]
    pub metrics_history: std::collections::VecDeque<crate::types::DrillingMetrics>,

    // === Advisory Acknowledgment & Shift Tracking ===

    /// Acknowledged advisory audit trail
    #[serde(skip)]
    pub acknowledgments: Vec<crate::api::handlers::AcknowledgmentRecord>,

    /// Total packets processed (for shift summary)
    pub packets_processed: u64,

    /// Total advisory tickets created (for shift summary)
    pub tickets_created: u64,

    /// Total tickets verified/confirmed (for shift summary)
    pub tickets_verified: u64,

    /// Total tickets rejected as transient (for shift summary)
    pub tickets_rejected: u64,

    /// Peak severity observed during current session
    #[serde(skip)]
    pub peak_severity: crate::types::TicketSeverity,

    /// Running average MSE efficiency (None if no drilling data yet)
    pub avg_mse_efficiency: Option<f64>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            latest_health: None,
            latest_spectrum: None,
            current_rpm: 0.0,
            uptime: Instant::now(),
            total_analyses: 0,
            learning_phase: true,
            learning_progress: 0.0,
            samples_collected: 0,
            last_analysis_time: None,
            analysis_interval_secs: 60,
            status: SystemStatus::Initializing,
            motor_temps: [55.0, 57.0, 59.0, 61.0],
            gearbox_temps: [48.0, 51.0],
            hookload: 60000.0,
            flow_rate: 0.0,
            l10_life_hours: f64::MAX,
            cumulative_damage: 0.0,
            wear_acceleration: 0.0,
            latest_verification: None,
            verified_faults: 0,
            rejected_faults: 0,
            latest_strategic_report: None,
            latest_advisory: None,
            latest_wits_packet: None,
            latest_drilling_metrics: None,
            campaign: {
                // Check CAMPAIGN env var: "pa" or "production" (default)
                match std::env::var("CAMPAIGN").as_deref() {
                    Ok("pa") | Ok("PA") | Ok("p&a") | Ok("P&A") | Ok("plug_abandonment") => {
                        crate::types::Campaign::PlugAbandonment
                    }
                    _ => crate::types::Campaign::Production,
                }
            },
            campaign_thresholds: {
                match std::env::var("CAMPAIGN").as_deref() {
                    Ok("pa") | Ok("PA") | Ok("p&a") | Ok("P&A") | Ok("plug_abandonment") => {
                        crate::types::CampaignThresholds::plug_abandonment()
                    }
                    _ => crate::types::CampaignThresholds::production(),
                }
            },
            // ML Engine fields
            well_id: std::env::var("WELL_ID").unwrap_or_else(|_| "WELL-001".to_string()),
            field_name: std::env::var("FIELD_NAME").unwrap_or_else(|_| "DEFAULT".to_string()),
            bit_hours: 0.0,
            bit_depth_drilled: 0.0,
            latest_ml_report: None,
            wits_history: std::collections::VecDeque::with_capacity(7200), // 2 hours at 1 Hz
            metrics_history: std::collections::VecDeque::with_capacity(7200),
            // Advisory tracking
            acknowledgments: Vec::new(),
            packets_processed: 0,
            tickets_created: 0,
            tickets_verified: 0,
            tickets_rejected: 0,
            peak_severity: crate::types::TicketSeverity::Low,
            avg_mse_efficiency: None,
        }
    }
}

impl AppState {
    /// Set the campaign type and update thresholds accordingly
    pub fn set_campaign(&mut self, campaign: crate::types::Campaign) {
        self.campaign = campaign;
        self.campaign_thresholds = crate::types::CampaignThresholds::for_campaign(campaign);
        tracing::info!(
            campaign = %campaign.display_name(),
            "Campaign switched - thresholds updated"
        );
    }
}

impl AppState {
    /// Get uptime in seconds
    pub fn uptime_secs(&self) -> u64 {
        self.uptime.elapsed().as_secs()
    }

    /// Check if system is healthy
    pub fn is_healthy(&self) -> bool {
        match &self.latest_health {
            Some(health) => health.severity == Severity::Healthy,
            None => true, // No data yet, assume healthy
        }
    }

    /// Get current health score (0-100)
    pub fn health_score(&self) -> Option<f64> {
        self.latest_health.as_ref().map(|h| h.health_score)
    }
}

/// System operational status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SystemStatus {
    /// System is starting up
    Initializing,
    /// Learning baseline vibration patterns
    Learning,
    /// Normal operation, monitoring active
    Monitoring,
    /// Analysis detected issues requiring attention
    Alert,
    /// System error or degraded operation
    Error,
}

impl std::fmt::Display for SystemStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SystemStatus::Initializing => write!(f, "Initializing"),
            SystemStatus::Learning => write!(f, "Learning"),
            SystemStatus::Monitoring => write!(f, "Monitoring"),
            SystemStatus::Alert => write!(f, "Alert"),
            SystemStatus::Error => write!(f, "Error"),
        }
    }
}

// ============================================================================
// Vibration Processor
// ============================================================================

/// Main vibration processing pipeline.
///
/// Coordinates the flow of sensor data through buffering, FFT analysis,
/// and LLM-based health assessment.
pub struct VibrationProcessor {
    /// Rolling buffer of vibration samples (60-second window)
    buffer: VecDeque<f64>,

    /// LLM Director for health analysis
    llm_director: LlmDirector,

    /// Historical analysis storage
    storage: Option<crate::storage::AnalysisStorage>,

    /// Strategic actor handle for sending tactical analyses
    #[cfg(feature = "llm")]
    strategic_handle: Option<crate::strategic::StrategicActorHandle>,
    #[cfg(not(feature = "llm"))]
    strategic_handle: Option<()>,

    /// Baseline spectrum learned from initial operation
    baseline_spectrum: Option<FrequencySpectrum>,

    /// Whether we're in the learning phase
    learning_phase: bool,

    /// Total samples collected
    sample_count: usize,

    /// Spectra collected during learning for averaging
    learning_spectra: Vec<FrequencySpectrum>,

    /// Latest RPM reading
    current_rpm: f64,

    /// Samples since last analysis
    samples_since_analysis: usize,

    /// Motor temperatures (4 sensors) in °C
    motor_temps: [f64; 4],

    /// Gearbox temperatures (2 sensors) in °C
    gearbox_temps: [f64; 2],

    /// Latest vibration readings from all 4 channels
    vib_channels: [f64; 4],

    /// Current hookload reading (Newtons)
    hookload: f64,

    /// Current flow rate (bbl/min)
    flow_rate: f64,

    /// Cumulative damage accumulator
    cumulative_damage: f64,

    /// Recent damage contributions for wear acceleration calculation
    damage_history: VecDeque<f64>,
}

impl VibrationProcessor {
    /// Create a new vibration processor with the given LLM Director.
    ///
    /// The processor starts in learning mode and will build a baseline
    /// spectrum over the first 5 minutes of operation.
    ///
    /// # Arguments
    ///
    /// * `llm_director` - Configured LLM Director for health analysis
    ///
    /// # Example
    ///
    /// ```ignore
    /// let llm = LlmDirector::new_disabled();
    /// let processor = VibrationProcessor::new(llm);
    /// ```
    pub fn new(llm_director: LlmDirector) -> Self {
        Self::new_with_storage(llm_director, None, None)
    }

    /// Create a new vibration processor with storage
    #[cfg(feature = "llm")]
    pub fn new_with_storage(
        llm_director: LlmDirector,
        storage: Option<crate::storage::AnalysisStorage>,
        strategic_handle: Option<crate::strategic::StrategicActorHandle>,
    ) -> Self {
        Self::new_with_storage_inner(llm_director, storage, strategic_handle)
    }

    /// Create a new vibration processor with storage (no LLM)
    #[cfg(not(feature = "llm"))]
    pub fn new_with_storage(
        llm_director: LlmDirector,
        storage: Option<crate::storage::AnalysisStorage>,
        strategic_handle: Option<()>,
    ) -> Self {
        Self::new_with_storage_inner(llm_director, storage, strategic_handle)
    }

    fn new_with_storage_inner(
        llm_director: LlmDirector,
        storage: Option<crate::storage::AnalysisStorage>,
        #[cfg(feature = "llm")] strategic_handle: Option<crate::strategic::StrategicActorHandle>,
        #[cfg(not(feature = "llm"))] strategic_handle: Option<()>,
    ) -> Self {
        info!(
            buffer_size = BUFFER_SIZE,
            sample_rate = SAMPLE_RATE_HZ,
            learning_samples = LEARNING_SAMPLES,
            has_storage = storage.is_some(),
            has_strategic = strategic_handle.is_some(),
            "Initializing VibrationProcessor"
        );

        Self {
            buffer: VecDeque::with_capacity(BUFFER_SIZE),
            llm_director,
            storage,
            strategic_handle,
            baseline_spectrum: None,
            learning_phase: true,
            sample_count: 0,
            learning_spectra: Vec::with_capacity(LEARNING_WINDOWS),
            current_rpm: 0.0,
            samples_since_analysis: 0,
            motor_temps: [55.0, 57.0, 59.0, 61.0],
            gearbox_temps: [48.0, 51.0],
            vib_channels: [0.0; 4],
            hookload: 60000.0,
            flow_rate: 0.0,
            cumulative_damage: 0.0,
            damage_history: VecDeque::with_capacity(60),
        }
    }

    /// Run the processing loop.
    ///
    /// This method runs continuously, receiving sensor data from the channel,
    /// buffering samples, and triggering analysis every 60 seconds.
    ///
    /// # Arguments
    ///
    /// * `sensor_rx` - Receiver for sensor readings
    /// * `app_state` - Shared application state
    /// * `shutdown` - Atomic flag to signal shutdown
    ///
    /// # Flow
    ///
    /// 1. Receive sensor readings from channel
    /// 2. Extract VIB-CH1 samples and RPM
    /// 3. Push to rolling buffer
    /// 4. Every 1200 samples (60 seconds):
    ///    - Compute FFT
    ///    - If learning: add to baseline spectra
    ///    - If monitoring: run LLM analysis
    ///    - Update app state
    pub async fn run(
        &mut self,
        mut sensor_rx: Receiver<SensorReading>,
        app_state: Arc<RwLock<AppState>>,
        shutdown: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<()> {
        info!("VibrationProcessor starting main loop");

        // Update initial state
        {
            let mut state = app_state.write().await;
            state.status = SystemStatus::Learning;
            state.learning_phase = true;
        }

        while !shutdown.load(std::sync::atomic::Ordering::Relaxed) {
            // Try to receive with timeout to check shutdown flag periodically
            match tokio::time::timeout(tokio::time::Duration::from_millis(100), sensor_rx.recv())
                .await
            {
                Ok(Some(reading)) => {
                    // Process single reading
                    self.process_reading(&reading);

                    // Update state with sample count
                    {
                        let mut state = app_state.write().await;
                        state.samples_collected = self.sample_count;
                        state.current_rpm = self.current_rpm;
                        state.motor_temps = self.motor_temps;
                        state.gearbox_temps = self.gearbox_temps;
                        state.learning_progress = if self.learning_phase {
                            (self.sample_count as f64 / LEARNING_SAMPLES as f64).min(1.0)
                        } else {
                            1.0
                        };
                    }

                    // Check if we have enough samples for analysis
                    if self.samples_since_analysis >= BUFFER_SIZE
                        && self.buffer.len() >= BUFFER_SIZE
                    {
                        if let Err(e) = self.run_analysis(&app_state).await {
                            error!(error = %e, "Analysis failed");
                            let mut state = app_state.write().await;
                            state.status = SystemStatus::Error;
                        }
                        self.samples_since_analysis = 0;
                    }
                }
                Ok(None) => {
                    // Channel closed
                    break;
                }
                Err(_) => {
                    // Timeout, check shutdown and continue
                    continue;
                }
            }
        }

        info!("VibrationProcessor shutting down");
        Ok(())
    }

    /// Run the processing loop with batched readings.
    ///
    /// Alternative version that accepts batches of readings.
    pub async fn run_batched(
        &mut self,
        mut sensor_rx: Receiver<Vec<SensorReading>>,
        app_state: Arc<RwLock<AppState>>,
    ) {
        info!("VibrationProcessor starting main loop (batched)");

        // Update initial state
        {
            let mut state = app_state.write().await;
            state.status = SystemStatus::Learning;
            state.learning_phase = true;
        }

        while let Some(readings) = sensor_rx.recv().await {
            // Process batch of readings
            for reading in readings {
                self.process_reading(&reading);
            }

            // Update state with sample count
            {
                let mut state = app_state.write().await;
                state.samples_collected = self.sample_count;
                state.current_rpm = self.current_rpm;
                state.motor_temps = self.motor_temps;
                state.gearbox_temps = self.gearbox_temps;
                state.learning_progress = if self.learning_phase {
                    (self.sample_count as f64 / LEARNING_SAMPLES as f64).min(1.0)
                } else {
                    1.0
                };
            }

            // Check if we have enough samples for analysis
            if self.samples_since_analysis >= BUFFER_SIZE && self.buffer.len() >= BUFFER_SIZE {
                if let Err(e) = self.run_analysis(&app_state).await {
                    error!(error = %e, "Analysis failed");
                    let mut state = app_state.write().await;
                    state.status = SystemStatus::Error;
                }
                self.samples_since_analysis = 0;
            }
        }

        info!("VibrationProcessor shutting down - channel closed");
    }

    /// Process a single sensor reading.
    fn process_reading(&mut self, reading: &SensorReading) {
        // Extract vibration from VIB-CH1 (primary channel for FFT buffer)
        if reading.sensor_id == "VIB-CH1" {
            // Add to buffer
            if self.buffer.len() >= BUFFER_SIZE {
                self.buffer.pop_front();
            }
            self.buffer.push_back(reading.value);

            self.sample_count += 1;
            self.samples_since_analysis += 1;

            // Debug logging every 1000 samples
            if self.sample_count % 1000 == 0 {
                debug!(
                    samples = self.sample_count,
                    buffer_len = self.buffer.len(),
                    learning = self.learning_phase,
                    "Processing progress"
                );
            }
        }

        // Track all vibration channels
        match reading.sensor_id.as_str() {
            "VIB-CH1" => self.vib_channels[0] = reading.value,
            "VIB-CH2" => self.vib_channels[1] = reading.value,
            "VIB-CH3" => self.vib_channels[2] = reading.value,
            "VIB-CH4" => self.vib_channels[3] = reading.value,
            _ => {}
        }

        // Extract RPM from RPM sensor
        if reading.sensor_id == "RPM-MAIN" {
            self.current_rpm = reading.value;
        }

        // Extract motor temperatures and hookload
        match reading.sensor_id.as_str() {
            "MOTOR-TEMP-1" => self.motor_temps[0] = reading.value,
            "MOTOR-TEMP-2" => self.motor_temps[1] = reading.value,
            "MOTOR-TEMP-3" => self.motor_temps[2] = reading.value,
            "MOTOR-TEMP-4" => self.motor_temps[3] = reading.value,
            "GEARBOX-TEMP-1" => self.gearbox_temps[0] = reading.value,
            "GEARBOX-TEMP-2" => self.gearbox_temps[1] = reading.value,
            "HOOKLOAD" => self.hookload = reading.value,
            _ => {}
        }
    }

    /// Calculate TTF metrics and update cumulative damage
    fn update_ttf_metrics(&mut self) {
        // Calculate L10 bearing life based on current conditions
        // Using bearing rating constant from thresholds
        const BEARING_RATING: f64 = 120_000.0;

        // Calculate damage contribution for this analysis cycle
        // cycles = RPM × analysis_interval_minutes
        let cycles = self.current_rpm; // RPM = cycles per minute, 1 analysis per minute
        let damage_contribution = if BEARING_RATING > 0.0 && cycles > 0.0 {
            cycles / (BEARING_RATING * 1_000_000.0)
        } else {
            0.0
        };

        // Update cumulative damage
        self.cumulative_damage += damage_contribution;

        // Store in damage history for wear acceleration calculation
        if self.damage_history.len() >= 60 {
            self.damage_history.pop_front();
        }
        self.damage_history.push_back(damage_contribution);
    }

    /// Get current L10 life estimate in hours
    fn get_l10_life(&self) -> f64 {
        use crate::physics_engine;
        const BEARING_RATING: f64 = 120_000.0;
        physics_engine::l10_life(self.current_rpm, self.hookload, BEARING_RATING)
    }

    /// Get wear acceleration from damage history
    fn get_wear_acceleration(&self) -> f64 {
        use crate::physics_engine;
        let damage_vec: Vec<f64> = self.damage_history.iter().copied().collect();
        physics_engine::wear_acceleration(&damage_vec)
    }

    /// Run FFT analysis on the current buffer.
    async fn run_analysis(&mut self, app_state: &Arc<RwLock<AppState>>) -> Result<()> {
        // Convert buffer to slice for FFT
        let samples: Vec<f64> = self.buffer.iter().copied().collect();

        // Debug: log buffer statistics
        let buf_min = samples.iter().cloned().fold(f64::INFINITY, f64::min);
        let buf_max = samples.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let buf_mean = samples.iter().sum::<f64>() / samples.len() as f64;
        let buf_std = (samples.iter().map(|x| (x - buf_mean).powi(2)).sum::<f64>() / samples.len() as f64).sqrt();

        info!(
            samples_len = samples.len(),
            rpm = self.current_rpm,
            buf_min = buf_min,
            buf_max = buf_max,
            buf_mean = buf_mean,
            buf_std = buf_std,
            "Running FFT analysis - buffer stats"
        );

        // Compute FFT
        let spectrum = compute_fft(&samples, SAMPLE_RATE_HZ).context("FFT computation failed")?;

        if self.learning_phase {
            self.handle_learning_phase(&spectrum, app_state).await?;
        } else {
            self.handle_monitoring_phase(&spectrum, app_state, buf_std).await?;
        }

        Ok(())
    }

    /// Handle analysis during learning phase.
    async fn handle_learning_phase(
        &mut self,
        spectrum: &FrequencySpectrum,
        app_state: &Arc<RwLock<AppState>>,
    ) -> Result<()> {
        self.learning_spectra.push(spectrum.clone());

        info!(
            spectra_count = self.learning_spectra.len(),
            target = LEARNING_WINDOWS,
            "Learning: collected spectrum"
        );

        // Check if we've collected enough spectra
        if self.learning_spectra.len() >= LEARNING_WINDOWS {
            // Average the spectra to create baseline
            let baseline = self.average_spectra(&self.learning_spectra);
            self.baseline_spectrum = Some(baseline.clone());
            self.learning_phase = false;

            info!(
                "Learning complete - baseline established with {} spectra",
                self.learning_spectra.len()
            );

            // Update state
            let mut state = app_state.write().await;
            state.learning_phase = false;
            state.learning_progress = 1.0;
            state.status = SystemStatus::Monitoring;
            state.latest_spectrum = Some(baseline);

            // Clear learning spectra to free memory
            self.learning_spectra.clear();
        } else {
            // Update learning progress
            let mut state = app_state.write().await;
            state.learning_progress = self.learning_spectra.len() as f64 / LEARNING_WINDOWS as f64;
        }

        Ok(())
    }

    /// Handle analysis during monitoring phase.
    async fn handle_monitoring_phase(
        &mut self,
        spectrum: &FrequencySpectrum,
        app_state: &Arc<RwLock<AppState>>,
        buf_std: f64,
    ) -> Result<()> {
        let baseline = self
            .baseline_spectrum
            .clone()
            .context("Baseline spectrum not available")?;

        info!(
            rpm = self.current_rpm,
            motor_temps = ?self.motor_temps,
            gearbox_temps = ?self.gearbox_temps,
            "Running health analysis"
        );

        // Calculate bearing frequencies and fault amplitudes for scoring
        // Uses harmonic summation (1x + 2x + 3x) to capture fault energy spread across harmonics
        // Combined with lowered thresholds in health_scoring.rs to compensate for FFT underdetection
        use crate::processing::{calculate_bearing_frequencies, extract_harmonic_amplitude};
        let bearing_freqs = calculate_bearing_frequencies(self.current_rpm);

        // Sum energy across 1x, 2x, 3x harmonics for better fault detection
        // This captures ~1.7x more energy than single-frequency peak detection
        let bpfo_amp = extract_harmonic_amplitude(spectrum, bearing_freqs.bpfo, 5.0, 3);
        let bpfi_amp = extract_harmonic_amplitude(spectrum, bearing_freqs.bpfi, 5.0, 3);

        // Debug: find the actual max magnitude in spectrum and its frequency
        let max_mag_idx = spectrum
            .magnitudes
            .iter()
            .enumerate()
            .skip(1) // skip DC
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(i, _)| i)
            .unwrap_or(0);
        let max_mag = spectrum.magnitudes.get(max_mag_idx).copied().unwrap_or(0.0);
        let max_freq = spectrum.frequencies.get(max_mag_idx).copied().unwrap_or(0.0);

        // Log bearing fault amplitudes (harmonic sum) for debugging
        tracing::info!(
            bpfo_freq = bearing_freqs.bpfo,
            bpfo_amp = bpfo_amp,
            bpfi_freq = bearing_freqs.bpfi,
            bpfi_amp = bpfi_amp,
            spectrum_rms = spectrum.rms,
            spectrum_peak_freq = spectrum.peak_frequency,
            max_spectrum_mag = max_mag,
            max_spectrum_freq = max_freq,
            "Bearing fault amplitudes (harmonic sum 1x+2x+3x)"
        );

        // Calculate health score DETERMINISTICALLY (not from LLM)
        // Uses:
        // - Harmonic summation for bearing fault amplitudes (captures ~1.7x more energy)
        // - Buffer std as alternative amplitude estimate (more robust to FFT frequency smearing)
        // - Buffer std * sqrt(2) approximates peak amplitude for sinusoidal faults
        let (health_score, severity_str) = {
            let (score, sev) = crate::processing::calculate_health_score_with_buffer(
                spectrum,
                &baseline,
                &self.motor_temps,
                &self.gearbox_temps,
                bpfo_amp,
                bpfi_amp,
                Some(buf_std),
            );
            // Guard against NaN/Inf from bad FFT data — default to 50.0 (unknown)
            if score.is_finite() { (score, sev) } else { (50.0, "UNKNOWN".to_string()) }
        };

        info!(
            health_score = health_score,
            severity = %severity_str,
            bpfo_amp = bpfo_amp,
            bpfi_amp = bpfi_amp,
            "Deterministic health score calculated"
        );

        // Prepare temperature data for LLM
        let temps = crate::director::TemperatureData::new(self.motor_temps, self.gearbox_temps);

        // Run LLM analysis with pre-calculated score for diagnosis/action
        let mut assessment = self
            .llm_director
            .analyze_with_score(spectrum, &baseline, self.current_rpm, &temps, health_score, &severity_str)
            .await
            .context("LLM analysis failed")?;

        // Ensure assessment uses the deterministic score (in case LLM hallucinated)
        assessment.health_score = health_score;
        assessment.severity = crate::director::Severity::from_str_loose(&severity_str);

        // Log the result
        info!(
            health_score = assessment.health_score,
            severity = %assessment.severity,
            diagnosis = %assessment.diagnosis,
            action = %assessment.recommended_action,
            "Health assessment complete"
        );

        // Persist to storage if available
        if let Some(storage) = &self.storage {
            if let Err(e) = storage.store(&assessment) {
                error!("Failed to persist assessment to storage: {}", e);
            } else {
                debug!("Assessment persisted to storage");
            }
        }

        // Send tactical analysis to strategic actor if available
        #[cfg(feature = "llm")]
        if let Some(strategic_handle) = &self.strategic_handle {
            use crate::strategic::TacticalAnalysis;

            // Use bearing fault amplitudes already calculated above
            let tactical = TacticalAnalysis {
                timestamp: assessment.timestamp,
                health_score: assessment.health_score,
                severity: format!("{:?}", assessment.severity),
                rpm: self.current_rpm,
                motor_temp_avg: self.motor_temps.iter().sum::<f64>() / self.motor_temps.len() as f64,
                gearbox_temp_avg: self.gearbox_temps.iter().sum::<f64>() / self.gearbox_temps.len() as f64,
                rms: spectrum.rms,
                bpfo_amp,
                bpfi_amp,
            };

            if let Err(e) = strategic_handle.send_tactical(tactical).await {
                error!("Failed to send tactical analysis to strategic actor: {}", e);
            } else {
                debug!("Tactical analysis sent to strategic actor");
            }
        }

        // Determine system status based on severity
        let status = match assessment.severity {
            Severity::Healthy | Severity::Watch => SystemStatus::Monitoring,
            Severity::Warning | Severity::Critical => SystemStatus::Alert,
        };

        // Update TTF metrics
        self.update_ttf_metrics();

        // Update app state
        let mut state = app_state.write().await;
        state.latest_health = Some(assessment);
        state.latest_spectrum = Some(spectrum.clone());
        state.current_rpm = self.current_rpm;
        state.motor_temps = self.motor_temps;
        state.gearbox_temps = self.gearbox_temps;
        state.hookload = self.hookload;
        state.flow_rate = self.flow_rate;
        state.l10_life_hours = self.get_l10_life();
        state.cumulative_damage = self.cumulative_damage;
        state.wear_acceleration = self.get_wear_acceleration();
        state.total_analyses += 1;
        state.last_analysis_time = Some(chrono::Utc::now());
        state.status = status;

        Ok(())
    }

    /// Average multiple spectra to create a baseline.
    fn average_spectra(&self, spectra: &[FrequencySpectrum]) -> FrequencySpectrum {
        if spectra.is_empty() {
            return FrequencySpectrum {
                frequencies: vec![],
                magnitudes: vec![],
                rms: 0.0,
                peak_frequency: 0.0,
                sample_rate: SAMPLE_RATE_HZ,
                timestamp: chrono::Utc::now(),
            };
        }

        let n = spectra.len() as f64;
        let len = spectra[0].magnitudes.len();

        // Average magnitudes across all spectra
        let mut avg_magnitudes = vec![0.0; len];

        for spectrum in spectra {
            for (i, &mag) in spectrum.magnitudes.iter().enumerate() {
                if i < len {
                    avg_magnitudes[i] += mag / n;
                }
            }
        }

        // Calculate RMS from averaged magnitudes
        let avg_rms = (avg_magnitudes.iter().map(|x| x.powi(2)).sum::<f64>()
            / avg_magnitudes.len() as f64)
            .sqrt();

        // Find peak frequency
        let peak_idx = avg_magnitudes
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(i, _)| i)
            .unwrap_or(0);

        FrequencySpectrum {
            frequencies: spectra[0].frequencies.clone(),
            magnitudes: avg_magnitudes,
            rms: avg_rms,
            peak_frequency: spectra[0].frequencies.get(peak_idx).copied().unwrap_or(0.0),
            sample_rate: SAMPLE_RATE_HZ,
            timestamp: chrono::Utc::now(),
        }
    }

    /// Get the current sample count.
    pub fn sample_count(&self) -> usize {
        self.sample_count
    }

    /// Check if still in learning phase.
    pub fn is_learning(&self) -> bool {
        self.learning_phase
    }

    /// Get the baseline spectrum if available.
    pub fn baseline(&self) -> Option<&FrequencySpectrum> {
        self.baseline_spectrum.as_ref()
    }

    /// Get current buffer length.
    pub fn buffer_len(&self) -> usize {
        self.buffer.len()
    }

    /// Get buffer capacity (max samples).
    pub fn buffer_capacity(&self) -> usize {
        BUFFER_SIZE
    }

    /// Force transition from learning to monitoring (for testing).
    #[cfg(test)]
    pub fn force_monitoring_mode(&mut self, baseline: FrequencySpectrum) {
        self.baseline_spectrum = Some(baseline);
        self.learning_phase = false;
    }
}

// ============================================================================
// Pipeline Builder (for fluent configuration)
// ============================================================================

/// Builder for configuring the processing pipeline.
pub struct PipelineBuilder {
    sample_rate: f64,
    buffer_size: usize,
    learning_windows: usize,
}

impl Default for PipelineBuilder {
    fn default() -> Self {
        Self {
            sample_rate: SAMPLE_RATE_HZ,
            buffer_size: BUFFER_SIZE,
            learning_windows: LEARNING_WINDOWS,
        }
    }
}

impl PipelineBuilder {
    /// Create a new pipeline builder with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the sample rate in Hz.
    pub fn sample_rate(mut self, rate: f64) -> Self {
        self.sample_rate = rate;
        self
    }

    /// Set the buffer size (samples per analysis window).
    pub fn buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }

    /// Set the number of windows to collect during learning.
    pub fn learning_windows(mut self, windows: usize) -> Self {
        self.learning_windows = windows;
        self
    }

    /// Build the processor with the given LLM Director.
    pub fn build(self, llm_director: LlmDirector) -> VibrationProcessor {
        info!(
            sample_rate = self.sample_rate,
            buffer_size = self.buffer_size,
            learning_windows = self.learning_windows,
            "Building VibrationProcessor with custom config"
        );

        VibrationProcessor {
            buffer: VecDeque::with_capacity(self.buffer_size),
            llm_director,
            storage: None,
            strategic_handle: None,
            baseline_spectrum: None,
            learning_phase: true,
            sample_count: 0,
            learning_spectra: Vec::with_capacity(self.learning_windows),
            current_rpm: 0.0,
            samples_since_analysis: 0,
            motor_temps: [55.0, 57.0, 59.0, 61.0],
            gearbox_temps: [48.0, 51.0],
            vib_channels: [0.0; 4],
            hookload: 60000.0,
            flow_rate: 0.0,
            cumulative_damage: 0.0,
            damage_history: VecDeque::with_capacity(60),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acquisition::SensorType;
    use crate::director::LlmDirector;

    fn create_test_spectrum() -> FrequencySpectrum {
        FrequencySpectrum {
            frequencies: (0..100).map(|i| i as f64 * 10.0).collect(),
            magnitudes: (0..100).map(|i| if i == 10 { 0.5 } else { 0.05 }).collect(),
            rms: 0.1,
            peak_frequency: 100.0,
            sample_rate: 20.0,
            timestamp: chrono::Utc::now(),
        }
    }

    fn create_test_readings(n: usize, vib_value: f64, rpm: f64) -> Vec<SensorReading> {
        use chrono::Utc;

        let mut readings = Vec::new();

        readings.push(SensorReading {
            sensor_id: "VIB-CH1".to_string(),
            timestamp: Utc::now(),
            sensor_type: SensorType::VibrationX,
            value: vib_value,
            unit: "g".to_string(),
            quality: 1.0,
        });

        readings.push(SensorReading {
            sensor_id: "RPM-MAIN".to_string(),
            timestamp: Utc::now(),
            sensor_type: SensorType::Rpm,
            value: rpm,
            unit: "RPM".to_string(),
            quality: 1.0,
        });

        readings
    }

    #[test]
    fn test_processor_creation() {
        let llm = LlmDirector::new_disabled();
        let processor = VibrationProcessor::new(llm);

        assert!(processor.is_learning());
        assert_eq!(processor.sample_count(), 0);
        assert_eq!(processor.buffer_len(), 0);
        assert!(processor.baseline().is_none());
    }

    #[test]
    fn test_process_reading() {
        let llm = LlmDirector::new_disabled();
        let mut processor = VibrationProcessor::new(llm);

        // Create a reading
        let reading = SensorReading {
            sensor_id: "VIB-CH1".to_string(),
            timestamp: chrono::Utc::now(),
            sensor_type: SensorType::VibrationX,
            value: 0.5,
            unit: "g".to_string(),
            quality: 1.0,
        };

        processor.process_reading(&reading);

        assert_eq!(processor.sample_count(), 1);
        assert_eq!(processor.buffer_len(), 1);
    }

    #[test]
    fn test_rpm_extraction() {
        let llm = LlmDirector::new_disabled();
        let mut processor = VibrationProcessor::new(llm);

        let rpm_reading = SensorReading {
            sensor_id: "RPM-MAIN".to_string(),
            timestamp: chrono::Utc::now(),
            sensor_type: SensorType::Rpm,
            value: 100.0,
            unit: "RPM".to_string(),
            quality: 1.0,
        };

        processor.process_reading(&rpm_reading);

        assert!((processor.current_rpm - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_buffer_rolling() {
        let llm = LlmDirector::new_disabled();
        let mut processor = VibrationProcessor::new(llm);

        // Fill buffer beyond capacity
        for i in 0..(BUFFER_SIZE + 100) {
            let reading = SensorReading {
                sensor_id: "VIB-CH1".to_string(),
                timestamp: chrono::Utc::now(),
                sensor_type: SensorType::VibrationX,
                value: i as f64,
                unit: "g".to_string(),
                quality: 1.0,
            };
            processor.process_reading(&reading);
        }

        // Buffer should be at capacity
        assert_eq!(processor.buffer_len(), BUFFER_SIZE);
        // But sample count should be total
        assert_eq!(processor.sample_count(), BUFFER_SIZE + 100);
    }

    #[test]
    fn test_average_spectra() {
        let llm = LlmDirector::new_disabled();
        let processor = VibrationProcessor::new(llm);

        let spectrum1 = FrequencySpectrum {
            frequencies: vec![0.0, 10.0, 20.0],
            magnitudes: vec![1.0, 2.0, 3.0],
            rms: 2.0,
            peak_frequency: 20.0,
            sample_rate: 20.0,
            timestamp: chrono::Utc::now(),
        };

        let spectrum2 = FrequencySpectrum {
            frequencies: vec![0.0, 10.0, 20.0],
            magnitudes: vec![3.0, 4.0, 5.0],
            rms: 4.0,
            peak_frequency: 20.0,
            sample_rate: 20.0,
            timestamp: chrono::Utc::now(),
        };

        let avg = processor.average_spectra(&[spectrum1, spectrum2]);

        assert_eq!(avg.frequencies, vec![0.0, 10.0, 20.0]);
        assert!((avg.magnitudes[0] - 2.0).abs() < 0.001);
        assert!((avg.magnitudes[1] - 3.0).abs() < 0.001);
        assert!((avg.magnitudes[2] - 4.0).abs() < 0.001);
    }

    #[test]
    fn test_app_state_default() {
        let state = AppState::default();

        assert!(state.latest_health.is_none());
        assert!(state.latest_spectrum.is_none());
        assert_eq!(state.current_rpm, 0.0);
        assert_eq!(state.total_analyses, 0);
        assert!(state.learning_phase);
        assert_eq!(state.status, SystemStatus::Initializing);
    }

    #[test]
    fn test_app_state_is_healthy() {
        let mut state = AppState::default();
        assert!(state.is_healthy()); // No data = assume healthy

        state.latest_health = Some(HealthAssessment {
            health_score: 90.0,
            severity: Severity::Healthy,
            diagnosis: "Good".to_string(),
            recommended_action: "Continue".to_string(),
            confidence: 0.95,
            raw_response: None,
            timestamp: chrono::Utc::now(),
            rpm: 100.0,
        });

        assert!(state.is_healthy());
        assert_eq!(state.health_score(), Some(90.0));
    }

    #[test]
    fn test_system_status_display() {
        assert_eq!(format!("{}", SystemStatus::Initializing), "Initializing");
        assert_eq!(format!("{}", SystemStatus::Learning), "Learning");
        assert_eq!(format!("{}", SystemStatus::Monitoring), "Monitoring");
        assert_eq!(format!("{}", SystemStatus::Alert), "Alert");
        assert_eq!(format!("{}", SystemStatus::Error), "Error");
    }

    #[test]
    fn test_pipeline_builder() {
        let llm = LlmDirector::new_disabled();
        let processor = PipelineBuilder::new()
            .sample_rate(10.0)
            .buffer_size(600)
            .learning_windows(3)
            .build(llm);

        assert!(processor.is_learning());
    }

    #[tokio::test]
    async fn test_processor_with_channel() {
        use std::sync::atomic::AtomicBool;
        use tokio::sync::mpsc;

        let llm = LlmDirector::new_disabled();
        let mut processor = VibrationProcessor::new(llm);
        let app_state = Arc::new(RwLock::new(AppState::default()));

        let (tx, rx) = mpsc::channel(100);
        let shutdown = Arc::new(AtomicBool::new(false));

        // Spawn processor
        let state_clone = app_state.clone();
        let shutdown_clone = shutdown.clone();
        let handle = tokio::spawn(async move {
            let _ = processor.run(rx, state_clone, shutdown_clone).await;
        });

        // Send some readings
        for i in 0..100 {
            let readings = create_test_readings(1, 0.5 + (i as f64 * 0.001), 100.0);
            for reading in readings {
                if tx.send(reading).await.is_err() {
                    break;
                }
            }
        }

        // Give processor time to process
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Signal shutdown
        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);

        // Close channel
        drop(tx);

        // Wait for processor to finish
        handle.await.expect("Processor task panicked");

        // Check state was updated
        let state = app_state.read().await;
        assert!(state.samples_collected > 0);
    }

    #[tokio::test]
    async fn test_monitoring_phase_analysis() {
        let llm = LlmDirector::new_disabled();
        let mut processor = VibrationProcessor::new(llm);
        let app_state = Arc::new(RwLock::new(AppState::default()));

        // Force into monitoring mode with a baseline
        let baseline = create_test_spectrum();
        processor.force_monitoring_mode(baseline.clone());

        // Fill buffer with enough samples
        for i in 0..BUFFER_SIZE {
            let reading = SensorReading {
                sensor_id: "VIB-CH1".to_string(),
                timestamp: chrono::Utc::now(),
                sensor_type: SensorType::VibrationX,
                value: (i as f64 * 0.01).sin(),
                unit: "g".to_string(),
                quality: 1.0,
            };
            processor.process_reading(&reading);
        }

        // Also set RPM
        let rpm_reading = SensorReading {
            sensor_id: "RPM-MAIN".to_string(),
            timestamp: chrono::Utc::now(),
            sensor_type: SensorType::Rpm,
            value: 100.0,
            unit: "RPM".to_string(),
            quality: 1.0,
        };
        processor.process_reading(&rpm_reading);

        // Run analysis
        processor.samples_since_analysis = BUFFER_SIZE;
        let result = processor.run_analysis(&app_state).await;
        assert!(result.is_ok());

        // Check state was updated with health assessment
        let state = app_state.read().await;
        assert!(state.latest_health.is_some());
        assert_eq!(state.total_analyses, 1);

        let health = state.latest_health.as_ref().unwrap();
        assert_eq!(health.severity, Severity::Warning);
    }
}
