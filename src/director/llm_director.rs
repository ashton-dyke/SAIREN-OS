//! LLM Director for Vibration Analysis
//!
//! This module provides AI-powered interpretation of FFT vibration data using
//! GGUF quantized LLMs. The LLM acts as a domain expert, analyzing frequency spectra
//! and providing natural language health assessments.
//!
//! # Model Loading
//!
//! The director uses mistral.rs to load GGUF-quantized models via the LLM scheduler:
//!
//! ```ignore
//! let director = LlmDirector::new_with_scheduler(scheduler_handle);
//! ```
//!
//! When the `llm` feature is not enabled, use the disabled director:
//!
//! ```ignore
//! let director = LlmDirector::new_disabled();
//! ```
//!

#![allow(dead_code)]
//! # System Prompt
//!
//! The LLM is prompted with ISO 10816 standards and TDS-11SA specifications:
//!
//! ```text
//! You are a vibration analysis expert for offshore drilling equipment.
//! You are analyzing a TDS-11SA top drive (500-ton, NOV manufacturer).
//!
//! Reference standards:
//! - ISO 10816-3 for rotating machinery vibration
//! - Zone A (Excellent): < 2.8 mm/s RMS
//! - Zone B (Acceptable): 2.8 - 7.1 mm/s RMS
//! - Zone C (Unsatisfactory): 7.1 - 18.0 mm/s RMS
//! - Zone D (Unacceptable): > 18.0 mm/s RMS
//! ```

#[cfg(feature = "llm")]
use crate::llm::Backend;
use crate::processing::{format_comparison_for_llm, format_for_llm, FrequencySpectrum};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ============================================================================
// Error Types
// ============================================================================

/// Errors from the LLM Director
#[derive(Error, Debug)]
pub enum DirectorError {
    /// Failed to load the model
    #[error("Failed to load model from {path}: {message}")]
    ModelLoadError { path: String, message: String },

    /// Inference failed
    #[error("Inference failed: {0}")]
    InferenceError(String),

    /// Failed to parse LLM response
    #[error("Failed to parse LLM response: {0}")]
    ParseError(String),

    /// Model not ready
    #[error("Model not loaded or not ready")]
    NotReady,

    /// Timeout during inference
    #[error("Inference timeout after {0} seconds")]
    Timeout(u64),
}

// ============================================================================
// Health Assessment Types
// ============================================================================

/// Severity levels for health assessments
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    /// Equipment in excellent condition (health score 80-100)
    Healthy,
    /// Minor deviations detected, monitor closely (health score 60-79)
    Watch,
    /// Significant issues detected, schedule maintenance (health score 40-59)
    Warning,
    /// Critical condition, immediate action required (health score 0-39)
    Critical,
}

impl Severity {
    /// Convert from string (case-insensitive)
    ///
    /// Handles many common variations LLMs might produce:
    /// - Critical: "critical", "danger", "dangerous", "severe", "alarm", "emergency", "immediate", "failure"
    /// - Warning: "warning", "caution", "high", "elevated", "significant"
    /// - Watch: "watch", "monitor", "attention", "moderate", "elevated"
    /// - Healthy: "healthy", "normal", "good", "ok", "excellent", "operational"
    pub fn from_str_loose(s: &str) -> Self {
        let lower = s.to_lowercase();

        // Check for Critical indicators (most urgent first)
        if lower.contains("critical")
            || lower.contains("danger")
            || lower.contains("severe")
            || lower.contains("alarm")
            || lower.contains("emergency")
            || lower.contains("immediate")
            || lower.contains("failure")
            || lower.contains("failing")
        {
            Severity::Critical
        // Check for Warning indicators
        } else if lower.contains("warning")
            || lower.contains("caution")
            || lower.contains("high")
            || lower.contains("significant")
        {
            Severity::Warning
        // Check for Watch indicators
        } else if lower.contains("watch")
            || lower.contains("monitor")
            || lower.contains("attention")
            || lower.contains("moderate")
            || lower.contains("elevated")
        {
            Severity::Watch
        // Check for Healthy indicators (or default)
        } else if lower.contains("healthy")
            || lower.contains("normal")
            || lower.contains("good")
            || lower.contains("excellent")
            || lower.contains("operational")
            || lower.contains("ok")
            || lower.contains("nominal")
        {
            Severity::Healthy
        } else {
            // Default to Watch if we can't determine - safer than assuming healthy
            Severity::Watch
        }
    }

    /// Get severity from health score
    pub fn from_score(score: f64) -> Self {
        match score {
            s if s >= 80.0 => Severity::Healthy,
            s if s >= 60.0 => Severity::Watch,
            s if s >= 40.0 => Severity::Warning,
            _ => Severity::Critical,
        }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Healthy => write!(f, "Healthy"),
            Severity::Watch => write!(f, "Watch"),
            Severity::Warning => write!(f, "Warning"),
            Severity::Critical => write!(f, "Critical"),
        }
    }
}

/// Complete health assessment from the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthAssessment {
    /// Overall health score (0-100, higher is better)
    pub health_score: f64,

    /// Severity classification
    pub severity: Severity,

    /// Natural language diagnosis explaining the findings
    pub diagnosis: String,

    /// Recommended maintenance action
    pub recommended_action: String,

    /// Confidence in the assessment (0.0-1.0)
    pub confidence: f64,

    /// Raw LLM response for debugging
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_response: Option<String>,

    /// Analysis timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,

    /// Operating conditions during analysis
    pub rpm: f64,
}

impl HealthAssessment {
    /// Check if immediate action is required
    pub fn requires_immediate_action(&self) -> bool {
        self.severity == Severity::Critical
    }

    /// Check if maintenance should be scheduled
    pub fn requires_scheduled_maintenance(&self) -> bool {
        matches!(self.severity, Severity::Warning | Severity::Critical)
    }
}

// ============================================================================
// Temperature Data for Analysis
// ============================================================================

/// Temperature sensor data for LLM analysis
#[derive(Debug, Clone, Default)]
pub struct TemperatureData {
    /// Motor temperatures (4 sensors) in °C
    pub motor_temps: [f64; 4],
    /// Gearbox temperatures (2 sensors) in °C
    pub gearbox_temps: [f64; 2],
    /// Motor baseline temperature (°C)
    pub motor_baseline: f64,
    /// Gearbox baseline temperature (°C)
    pub gearbox_baseline: f64,
}

impl TemperatureData {
    /// Create temperature data with baselines
    pub fn new(motor_temps: [f64; 4], gearbox_temps: [f64; 2]) -> Self {
        Self {
            motor_temps,
            gearbox_temps,
            motor_baseline: 55.0,
            gearbox_baseline: 48.0,
        }
    }

    /// Calculate average motor temperature
    pub fn motor_avg(&self) -> f64 {
        self.motor_temps.iter().sum::<f64>() / 4.0
    }

    /// Calculate average gearbox temperature
    pub fn gearbox_avg(&self) -> f64 {
        self.gearbox_temps.iter().sum::<f64>() / 2.0
    }

    /// Get thermal trend description
    pub fn thermal_trend(&self) -> &'static str {
        let motor_delta = self.motor_avg() - self.motor_baseline;
        let gearbox_delta = self.gearbox_avg() - self.gearbox_baseline;

        if motor_delta > 15.0 || gearbox_delta > 15.0 {
            "CRITICAL - Significant overheating detected"
        } else if motor_delta > 10.0 || gearbox_delta > 10.0 {
            "WARNING - Elevated temperatures, possible bearing friction"
        } else if motor_delta > 5.0 || gearbox_delta > 5.0 {
            "WATCH - Slight temperature rise, monitor closely"
        } else if motor_delta < -5.0 || gearbox_delta < -5.0 {
            "COLD - Below normal operating temperature"
        } else {
            "NORMAL - Within expected operating range"
        }
    }
}

// ============================================================================
// LLM Director Configuration
// ============================================================================

/// Configuration for the LLM Director
#[derive(Debug, Clone)]
pub struct DirectorConfig {
    /// Path to the GGUF model file
    pub model_path: String,

    /// Maximum tokens to generate
    pub max_tokens: usize,

    /// Temperature for generation (0.0-1.0)
    pub temperature: f32,

    /// Use GPU acceleration
    pub use_gpu: bool,

    /// Inference timeout in seconds
    pub timeout_secs: u64,

    /// Number of GPU layers to offload (0 = CPU only)
    pub gpu_layers: usize,
}

impl Default for DirectorConfig {
    fn default() -> Self {
        Self {
            model_path: "models/qwen2.5-1.5b-instruct-q4_k_m.gguf".to_string(),
            max_tokens: 512,
            temperature: 0.1, // Low temperature for consistent analysis
            use_gpu: true,
            timeout_secs: 30,
            gpu_layers: 35, // Offload most layers for 4-bit model
        }
    }
}

// ============================================================================
// System Prompt
// ============================================================================

/// System prompt for the vibration analysis LLM
pub const SYSTEM_PROMPT: &str = r#"You are a vibration analysis expert for offshore drilling equipment. You are analyzing FFT vibration data from a TDS-11SA top drive (500-ton capacity, NOV manufacturer).

REFERENCE STANDARDS - ISO 10816-3 for Class I machinery:
- Zone A (Excellent): < 2.8 mm/s RMS velocity, < 0.5 g acceleration
- Zone B (Acceptable): 2.8 - 7.1 mm/s RMS, 0.5 - 1.5 g
- Zone C (Unsatisfactory): 7.1 - 18.0 mm/s RMS, 1.5 - 4.0 g  
- Zone D (Unacceptable): > 18.0 mm/s RMS, > 4.0 g

BEARING FAULT SIGNATURES:
- BPFO (Ball Pass Frequency Outer): Outer race defect - most common (40% of failures)
- BPFI (Ball Pass Frequency Inner): Inner race defect - amplitude modulated by shaft speed
- BSF (Ball Spin Frequency): Rolling element defect - often appears at 2×BSF
- FTF (Fundamental Train Frequency): Cage defect or lubrication starvation

FAULT SEVERITY INDICATORS:
- Early stage: 20-50% amplitude increase at fault frequency
- Developing: 50-100% increase, sidebands appearing
- Advanced: >100% increase, harmonics present, broadband noise
- Critical: Amplitude 3-5× baseline, subsynchronous components

ANALYSIS RULES:
1. Compare current spectrum to baseline - look for changes
2. Check 1× and 2× RPM for unbalance/misalignment
3. Examine bearing frequencies for defect indicators
4. Consider operating conditions (RPM affects all frequencies)
5. Look for harmonics (2×, 3× of fault frequencies) as severity indicator

You MUST respond in this EXACT format:
HEALTH_SCORE: [integer 0-100]
SEVERITY: [Healthy|Watch|Warning|Critical]
DIAGNOSIS: [Your detailed analysis in one paragraph]
ACTION: [Specific recommended action]"#;


// ============================================================================
// LLM Director
// ============================================================================

/// LLM Director for vibration analysis using Mistral 7B.
///
/// The director wraps a Mistral 7B model (4-bit quantized) and provides
/// high-level methods for analyzing FFT spectrum data.
///
/// # GPU Requirements
///
/// - Minimum: 6 GB VRAM for 4-bit quantized model
/// - Recommended: 8 GB VRAM for faster inference
/// - Fallback: CPU inference available but slower (~10-30 seconds)
///
/// # Thread Safety
///
/// The director uses internal mutexes and is safe to share across threads.
/// However, only one inference can run at a time per director instance.
pub struct LlmDirector {
    /// Scheduler handle (if using priority scheduler)
    #[cfg(feature = "llm")]
    scheduler: Option<crate::llm::SchedulerHandle>,

    /// LLM backend - used when scheduler is None
    #[cfg(feature = "llm")]
    backend: Option<Backend>,

    /// Whether the director is ready for inference
    ready: std::sync::atomic::AtomicBool,
}

impl LlmDirector {
    /// Create a new LLM Director using the scheduler (priority queue mode).
    ///
    /// This mode uses the LlmScheduler for tactical analysis with priority guarantees.
    #[cfg(feature = "llm")]
    pub fn new_with_scheduler(scheduler: crate::llm::SchedulerHandle) -> Self {
        tracing::info!("LLM Director initialized with scheduler (priority queue mode)");

        Self {
            scheduler: Some(scheduler),
            backend: None,
            ready: std::sync::atomic::AtomicBool::new(true),
        }
    }

    /// Create a disabled LLM Director that cannot perform inference.
    ///
    /// Use this when the LLM feature is not enabled or when model loading fails.
    /// The director will return errors for all inference requests.
    #[cfg(feature = "llm")]
    pub fn new_disabled() -> Self {
        tracing::warn!("LLM Director created in disabled mode - no LLM inference available");
        Self {
            scheduler: None,
            backend: None,
            ready: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Create a disabled LLM Director that cannot perform inference.
    ///
    /// Use this when the LLM feature is not enabled or when model loading fails.
    /// The director will return errors for all inference requests.
    #[cfg(not(feature = "llm"))]
    pub fn new_disabled() -> Self {
        tracing::warn!("LLM Director created in disabled mode - no LLM inference available");
        Self {
            ready: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Check if the director is ready for inference.
    pub fn is_ready(&self) -> bool {
        self.ready.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Analyze vibration spectrum data and return health assessment.
    ///
    /// This is the main entry point for vibration analysis. The method:
    /// 1. Formats current and baseline spectra for LLM consumption
    /// 2. Builds a prompt with operating conditions
    /// 3. Runs inference on the LLM
    /// 4. Parses the response into a structured assessment
    ///
    /// # Arguments
    ///
    /// * `current` - Current FFT spectrum
    /// * `baseline` - Baseline (healthy) spectrum for comparison
    /// * `rpm` - Current operating RPM
    ///
    /// # Returns
    ///
    /// [`HealthAssessment`] with score, severity, diagnosis, and recommended action
    ///
    /// # Errors
    ///
    /// - [`DirectorError::NotReady`] if model not loaded
    /// - [`DirectorError::InferenceError`] if generation fails
    /// - [`DirectorError::ParseError`] if response format is invalid
    /// - [`DirectorError::Timeout`] if inference exceeds timeout
    pub async fn analyze(
        &self,
        current: &FrequencySpectrum,
        baseline: &FrequencySpectrum,
        rpm: f64,
        temps: &TemperatureData,
    ) -> Result<HealthAssessment> {
        if !self.is_ready() {
            return Err(DirectorError::NotReady.into());
        }

        let start_time = std::time::Instant::now();

        // Build the prompt with temperature data
        let prompt = self.build_prompt(current, baseline, rpm, temps);
        tracing::debug!(prompt_length = prompt.len(), "Built analysis prompt");

        // Run inference
        let response = self.run_inference(&prompt).await?;

        let inference_time = start_time.elapsed();
        tracing::info!(
            inference_time_ms = inference_time.as_millis(),
            response_length = response.len(),
            "LLM inference complete"
        );

        // Parse the response
        let mut assessment = self.parse_response(&response)?;
        assessment.rpm = rpm;
        assessment.raw_response = Some(response);

        Ok(assessment)
    }

    /// Analyze spectrum with pre-calculated health score (deterministic scoring).
    ///
    /// This method uses a deterministic health score calculated from sensor data,
    /// and asks the LLM only for diagnosis and recommended action. This prevents
    /// LLM hallucination on numerical scores.
    ///
    /// # Arguments
    ///
    /// * `current` - Current frequency spectrum
    /// * `baseline` - Baseline spectrum for comparison
    /// * `rpm` - Current RPM
    /// * `temps` - Temperature data
    /// * `health_score` - Pre-calculated deterministic health score (0-100)
    /// * `severity` - Pre-calculated severity level
    ///
    /// # Returns
    ///
    /// A `HealthAssessment` with the provided score and LLM-generated diagnosis/action
    ///
    /// # Errors
    ///
    /// - [`DirectorError::NotReady`] if model not loaded
    /// - [`DirectorError::InferenceError`] if generation fails
    /// - [`DirectorError::ParseError`] if response format is invalid
    pub async fn analyze_with_score(
        &self,
        current: &FrequencySpectrum,
        baseline: &FrequencySpectrum,
        rpm: f64,
        temps: &TemperatureData,
        health_score: f64,
        severity: &str,
    ) -> Result<HealthAssessment> {
        if !self.is_ready() {
            // Return a mock assessment when LLM is disabled
            // This allows the pipeline to continue without LLM inference
            let severity_enum = Severity::from_str_loose(severity);
            let diagnosis = match severity_enum {
                Severity::Critical => format!(
                    "Critical condition detected. Health score: {:.0}%. Motor temps: {:.1}°C avg, Gearbox temps: {:.1}°C avg.",
                    health_score,
                    temps.motor_avg(),
                    temps.gearbox_avg()
                ),
                Severity::Warning => format!(
                    "Warning level anomalies detected. Health score: {:.0}%. Recommend scheduling maintenance inspection.",
                    health_score
                ),
                Severity::Watch => format!(
                    "Minor deviations from baseline. Health score: {:.0}%. Continue monitoring.",
                    health_score
                ),
                Severity::Healthy => format!(
                    "System operating normally. Health score: {:.0}%. All parameters within acceptable limits.",
                    health_score
                ),
            };
            let action = match severity_enum {
                Severity::Critical => "Immediate shutdown recommended. Inspect bearing condition urgently.".to_string(),
                Severity::Warning => "Schedule maintenance within 24-48 hours.".to_string(),
                Severity::Watch => "Monitor closely. Re-evaluate in 1 hour.".to_string(),
                Severity::Healthy => "Continue normal operation.".to_string(),
            };
            return Ok(HealthAssessment {
                health_score,
                severity: severity_enum,
                diagnosis,
                recommended_action: action,
                confidence: 0.8, // Lower confidence without LLM
                raw_response: None,
                timestamp: chrono::Utc::now(),
                rpm,
            });
        }

        let start_time = std::time::Instant::now();

        // Build the prompt with pre-calculated score
        let prompt = self.build_prompt_with_score(current, baseline, rpm, temps, health_score, severity);
        tracing::debug!(prompt_length = prompt.len(), "Built analysis prompt with pre-calculated score");

        // Run inference
        let response = self.run_inference(&prompt).await?;

        let inference_time = start_time.elapsed();
        tracing::info!(
            inference_time_ms = inference_time.as_millis(),
            response_length = response.len(),
            "LLM inference complete"
        );

        // Parse the response (only diagnosis and action, not score)
        let mut assessment = self.parse_response_without_score(&response, health_score, severity)?;
        assessment.rpm = rpm;
        assessment.raw_response = Some(response);

        Ok(assessment)
    }

    /// Analyze a single spectrum without baseline comparison.
    ///
    /// Useful for initial baseline establishment or quick checks.
    pub async fn analyze_single(
        &self,
        spectrum: &FrequencySpectrum,
        rpm: f64,
    ) -> Result<HealthAssessment> {
        if !self.is_ready() {
            return Err(DirectorError::NotReady.into());
        }

        let spectrum_text = format_for_llm(spectrum, rpm);

        let prompt = format!(
            "Analyze this spectrum and provide your assessment:\n\n{}",
            spectrum_text
        );

        let response = self.run_inference(&prompt).await?;
        let mut assessment = self.parse_response(&response)?;
        assessment.rpm = rpm;
        assessment.raw_response = Some(response);

        Ok(assessment)
    }

    /// Build the analysis prompt from spectrum and temperature data.
    /// Minimal prompt optimized for short, structured output.
    fn build_prompt(
        &self,
        current: &FrequencySpectrum,
        baseline: &FrequencySpectrum,
        rpm: f64,
        temps: &TemperatureData,
    ) -> String {
        let comparison = format_comparison_for_llm(current, baseline, rpm);

        let motor_avg = temps.motor_avg();
        let gearbox_avg = temps.gearbox_avg();

        // Minimal prompt - force short structured output
        format!(
            r#"Bearing vibration analysis.
{}
Motor: {:.1}°C, Gearbox: {:.1}°C

Reply ONLY:
HEALTHSCORE: <0-100>
SEVERITY: <HEALTHY|WATCH|WARNING|CRITICAL>
DIAGNOSIS: <10 words max>
ACTION: <10 words max>"#,
            comparison,
            motor_avg,
            gearbox_avg
        )
    }

    /// Build the analysis prompt with pre-calculated health score.
    ///
    /// Minimal prompt optimized for short, structured output.
    fn build_prompt_with_score(
        &self,
        current: &FrequencySpectrum,
        baseline: &FrequencySpectrum,
        rpm: f64,
        temps: &TemperatureData,
        health_score: f64,
        severity: &str,
    ) -> String {
        let comparison = format_comparison_for_llm(current, baseline, rpm);

        let motor_avg = temps.motor_avg();
        let gearbox_avg = temps.gearbox_avg();

        // Context-aware prompt based on severity
        let context = if health_score >= 90.0 {
            "System is HEALTHY. BPFO/BPFI amplitudes below 0.1g are normal noise. Do NOT recommend maintenance for healthy equipment."
        } else if health_score >= 70.0 {
            "Minor deviations detected. Only mention issues if amplitudes exceed 0.15g."
        } else if health_score >= 50.0 {
            "Warning level. Significant amplitude increases detected."
        } else {
            "CRITICAL condition. Immediate attention required."
        };

        // Minimal prompt - force short structured output
        format!(
            r#"Bearing health: {:.0}/100 ({}).
CONTEXT: {}
{}
Motor: {:.1}°C, Gearbox: {:.1}°C

Reply ONLY (no markdown, no asterisks):
DIAGNOSIS: <10 words max, match severity level>
ACTION: <10 words max>"#,
            health_score,
            severity,
            context,
            comparison,
            motor_avg,
            gearbox_avg
        )
    }

    /// Run inference on the LLM backend or scheduler.
    #[cfg(feature = "llm")]
    async fn run_inference(&self, prompt: &str) -> Result<String> {
        if let Some(scheduler) = &self.scheduler {
            return scheduler.infer_tactical(prompt.to_string()).await;
        }

        if let Some(backend) = &self.backend {
            backend.generate(prompt).await
        } else {
            anyhow::bail!("No backend or scheduler configured")
        }
    }

    /// Run inference - disabled when LLM feature is not enabled.
    #[cfg(not(feature = "llm"))]
    async fn run_inference(&self, _prompt: &str) -> Result<String> {
        anyhow::bail!("LLM feature is not enabled")
    }

    /// Parse LLM response into structured HealthAssessment.
    ///
    /// This parser is designed to be fault-tolerant and never panic.
    /// It handles various edge cases and provides safe defaults.
    fn parse_response(&self, response: &str) -> Result<HealthAssessment> {
        // Step 0: Strip DeepSeek-R1 <think> blocks
        // The model outputs reasoning in <think>...</think> before the actual response
        let response = Self::strip_think_tags(response);

        // Step 1: Normalize the response string
        // Replace various newline representations with actual newlines
        let normalized = response
            .replace("<0x0A>", "\n") // Hex representation
            .replace("\\n", "\n") // Escaped newlines
            .replace("\\r\\n", "\n") // Windows line endings
            .replace("\\r", "\n"); // Old Mac line endings

        // Step 2: Strip markdown code fences and extra whitespace
        let cleaned = normalized
            .trim()
            .trim_start_matches("```")
            .trim_start_matches("json")
            .trim_start_matches("text")
            .trim_end_matches("```")
            .trim();

        tracing::debug!(
            "Parsing LLM response (original_len: {}, cleaned_len: {}, lines: {}):\n---\n{}\n---",
            response.len(),
            cleaned.len(),
            cleaned.lines().count(),
            cleaned
        );

        // Step 3: Case-insensitive keyword scanning
        let response_upper = cleaned.to_uppercase();

        let mut health_score: Option<f64> = None;
        let mut severity: Option<Severity> = None;
        let mut diagnosis = String::new();
        let mut action = String::new();
        let mut parse_failures = Vec::new();

        // Extract HEALTHSCORE (try multiple variants - common LLM output variations)
        for variant in [
            "HEALTHSCORE:",
            "HEALTH_SCORE:",
            "HEALTH SCORE:",
            "HEALTH-SCORE:",
            "OVERALL SCORE:",
            "SCORE:",
            "HEALTH:",
            "CONDITION SCORE:",
        ] {
            if let Some(pos) = response_upper.find(variant) {
                let after_keyword = &cleaned[pos + variant.len()..];
                // Extract first number, handling various formats like "85", "85.0", "85%", "85/100"
                let num_str: String = after_keyword
                    .chars()
                    .skip_while(|c| c.is_whitespace() || *c == ':' || *c == '=')
                    .take_while(|&c| c.is_ascii_digit() || c == '.')
                    .collect();

                if !num_str.is_empty() {
                    match num_str.parse::<f64>() {
                        Ok(score) => {
                            // If score is 0-10, scale to 0-100
                            health_score = Some(if score <= 10.0 { score * 10.0 } else { score });
                            break;
                        }
                        Err(e) => {
                            parse_failures
                                .push(format!("Failed to parse HEALTHSCORE '{}': {}", num_str, e));
                        }
                    }
                }
            }
        }

        // Extract SEVERITY (try multiple variants)
        let severity_keywords = [
            "SEVERITY:",
            "STATUS:",
            "CONDITION:",
            "ALERT LEVEL:",
            "RISK LEVEL:",
            "PRIORITY:",
        ];
        for keyword in &severity_keywords {
            if let Some(pos) = response_upper.find(keyword) {
                let after_keyword = &cleaned[pos + keyword.len()..];
                // Get first line after keyword, handling various delimiters
                let value = after_keyword
                    .lines()
                    .next()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("");

                if !value.is_empty() {
                    severity = Some(Severity::from_str_loose(value));
                    break; // Found a valid severity, stop searching
                }
            }
        }

        // Log if no severity keyword was found
        if severity.is_none() {
            parse_failures.push("No SEVERITY/STATUS keyword found".to_string());
        }

        // Extract DIAGNOSIS (try multiple variants)
        let diagnosis_keywords = [
            "DIAGNOSIS:",
            "ASSESSMENT:",
            "ANALYSIS:",
            "FINDINGS:",
            "SUMMARY:",
            "EVALUATION:",
        ];
        for keyword in &diagnosis_keywords {
            if let Some(pos) = response_upper.find(keyword) {
                let after_keyword = &cleaned[pos + keyword.len()..];
                // Extract text until next keyword (ACTION) or end of string
                let end_markers = ["ACTION:", "RECOMMENDED", "CONCLUSION:", "NEXT STEPS:"];
                let mut end_pos = after_keyword.len();
                for marker in &end_markers {
                    if let Some(marker_pos) = response_upper[pos + keyword.len()..].find(marker) {
                        if marker_pos < end_pos {
                            end_pos = marker_pos;
                        }
                    }
                }

                diagnosis = after_keyword[..end_pos.min(after_keyword.len())]
                    .trim()
                    .to_string();

                if !diagnosis.is_empty() {
                    break;
                }
            }
        }

        if diagnosis.is_empty() {
            parse_failures.push("No DIAGNOSIS/ASSESSMENT keyword found".to_string());
        }

        // Extract ACTION (try multiple variants)
        let action_keywords = [
            "ACTION:",
            "RECOMMENDED ACTION:",
            "RECOMMENDED_ACTION:",
            "RECOMMENDATION:",
            "RECOMMENDATIONS:",
            "NEXT STEPS:",
            "SUGGESTED ACTION:",
            "REQUIRED ACTION:",
        ];
        for keyword in &action_keywords {
            if let Some(pos) = response_upper.find(keyword) {
                let after_keyword = &cleaned[pos + keyword.len()..];
                // Get text until end of response (action is typically last)
                action = after_keyword.trim().to_string();
                if !action.is_empty() {
                    break;
                }
            }
        }

        if action.is_empty() {
            parse_failures.push("No ACTION/RECOMMENDATION keyword found".to_string());
        }

        // Step 4: Log parsing results and failures
        tracing::debug!(
            "Parse results: health_score={:?}, severity={:?}, diagnosis_len={}, action_len={}, failures={}",
            health_score,
            severity,
            diagnosis.len(),
            action.len(),
            parse_failures.len()
        );

        // Log failures at debug level
        if !parse_failures.is_empty() {
            tracing::debug!("Parsing issues encountered: {:?}", parse_failures);
        }

        // Step 5: Apply safe defaults
        let score = health_score.unwrap_or_else(|| {
            tracing::warn!("No valid HEALTHSCORE found, defaulting to 50.0");
            tracing::debug!(
                "Raw LLM output (first 500 chars): {}",
                &cleaned.chars().take(500).collect::<String>()
            );
            50.0
        });

        // Clamp score to valid range [0, 100]
        let score = score.clamp(0.0, 100.0);

        // Derive severity from score if not parsed
        let sev = severity.unwrap_or_else(|| {
            tracing::warn!(
                "No valid SEVERITY found, deriving from health score {}",
                score
            );
            Severity::from_score(score)
        });

        // Fallback: If diagnosis is empty, use the entire cleaned response
        if diagnosis.is_empty() {
            tracing::warn!("No valid DIAGNOSIS found, using entire LLM response as fallback");
            tracing::debug!("Full raw LLM output:\n{}", cleaned);

            // Use entire response as diagnosis, limiting to reasonable length
            diagnosis = if cleaned.len() > 500 {
                format!("{}... [truncated]", &cleaned[..500])
            } else {
                cleaned.to_string()
            };
        }

        // Fallback: Generate reasonable default action based on severity
        if action.is_empty() {
            tracing::warn!("No valid ACTION found, using severity-based default");
            action = match sev {
                Severity::Healthy => {
                    "Continue normal operations with standard monitoring.".to_string()
                }
                Severity::Watch => {
                    "Increase monitoring frequency and schedule inspection.".to_string()
                }
                Severity::Warning => {
                    "Schedule maintenance inspection within 24-48 hours.".to_string()
                }
                Severity::Critical => "Immediate shutdown and inspection required.".to_string(),
            };
        }

        // Calculate confidence based on parsing success
        let confidence = if health_score.is_some() && severity.is_some() {
            0.95
        } else if health_score.is_some() {
            0.80
        } else {
            0.50
        };

        Ok(HealthAssessment {
            health_score: score,
            severity: sev,
            diagnosis,
            recommended_action: action,
            confidence,
            raw_response: None, // Filled in by caller
            timestamp: chrono::Utc::now(),
            rpm: 0.0, // Filled in by caller
        })
    }

    /// Strip DeepSeek-R1 reasoning blocks from response.
    ///
    /// DeepSeek-R1-Distill models output their reasoning in `<think>...</think>` blocks
    /// before the actual response. This extracts just the final answer.
    /// Also handles unclosed `<think>` tags, HTML artifacts, and raw reasoning text.
    fn strip_think_tags(response: &str) -> String {
        // First, strip any HTML artifacts (e.g., </div> that appears sometimes)
        let response = response
            .replace("</div>", "")
            .replace("<div>", "")
            .trim()
            .to_string();

        let lower = response.to_lowercase();

        // Case 1: Complete <think>...</think> block - strip it
        if let Some(end_pos) = lower.find("</think>") {
            let after_think = &response[end_pos + "</think>".len()..];
            tracing::debug!(
                "Stripped complete <think> block ({} chars reasoning, {} chars response)",
                end_pos,
                after_think.len()
            );
            return after_think.trim().to_string();
        }

        // Case 2: Unclosed <think> tag - try to find DIAGNOSIS/ACTION within
        if let Some(think_start) = lower.find("<think>") {
            let after_think_start = &response[think_start + "<think>".len()..];
            let after_lower = after_think_start.to_lowercase();

            if let Some(diag_pos) = after_lower.find("diagnosis:") {
                tracing::debug!("Found DIAGNOSIS inside unclosed <think> block");
                return after_think_start[diag_pos..].trim().to_string();
            }
            if let Some(action_pos) = after_lower.find("action:") {
                tracing::debug!("Found ACTION inside unclosed <think> block");
                return after_think_start[action_pos..].trim().to_string();
            }

            let before_think = response[..think_start].trim();
            if !before_think.is_empty() {
                tracing::debug!("Returning content before unclosed <think> tag");
                return before_think.to_string();
            }

            tracing::debug!("Unclosed <think> with no extractable content");
            return after_think_start.trim().to_string();
        }

        // Case 3: No <think> tags - model output raw reasoning
        // Look for conclusion markers and extract the final statement
        let conclusion_markers = [
            "therefore,", "so,", "in conclusion,", "the diagnosis is",
            "this indicates", "this suggests", "based on this",
            "the vibration data shows", "the data indicates",
        ];

        for marker in conclusion_markers {
            if let Some(pos) = lower.rfind(marker) {
                let after_marker = &response[pos..];
                // Take the sentence containing the marker
                if let Some(end) = after_marker.find('.') {
                    let sentence = &after_marker[..end + 1];
                    if sentence.len() > 20 {
                        tracing::debug!("Extracted conclusion after '{}': {}", marker, sentence);
                        return format!("DIAGNOSIS: {}\nACTION: Continue monitoring.", sentence.trim());
                    }
                }
            }
        }

        response
    }

    /// Clean markdown artifacts from LLM output (asterisks, bold markers, etc.)
    fn clean_markdown_artifacts(text: &str) -> String {
        text.replace("**", "")
            .replace("*", "")
            .replace("__", "")
            .replace("_", " ")
            .replace("##", "")
            .replace("#", "")
            .trim()
            .to_string()
    }

    /// Parse LLM response that only contains diagnosis and action (not score/severity).
    ///
    /// This parser is used when health score is calculated deterministically.
    fn parse_response_without_score(
        &self,
        response: &str,
        health_score: f64,
        severity: &str,
    ) -> Result<HealthAssessment> {
        // Step 0: Strip DeepSeek-R1 <think> blocks
        let response = Self::strip_think_tags(response);

        // Step 1: Normalize the response string
        let normalized = response
            .replace("<0x0A>", "\n")
            .replace("\\n", "\n")
            .replace("\\r\\n", "\n")
            .replace("\\r", "\n");

        // Step 2: Strip markdown code fences and artifacts
        let cleaned = normalized
            .trim()
            .trim_start_matches("```")
            .trim_start_matches("json")
            .trim_start_matches("text")
            .trim_end_matches("```")
            .trim();
        let cleaned = Self::clean_markdown_artifacts(cleaned);

        tracing::debug!(
            "Parsing LLM response without score (len: {}, lines: {}):\n---\n{}\n---",
            cleaned.len(),
            cleaned.lines().count(),
            cleaned
        );

        let response_upper = cleaned.to_uppercase();
        let mut diagnosis = String::new();
        let mut action = String::new();

        // Extract DIAGNOSIS - check if keyword present or if response starts directly with diagnosis
        if let Some(pos) = response_upper.find("DIAGNOSIS:") {
            // Traditional format with DIAGNOSIS: keyword
            let after_keyword = &cleaned[pos + "DIAGNOSIS:".len()..];
            let end_pos = response_upper[pos..]
                .find("ACTION:")
                .map(|p| p.saturating_sub("DIAGNOSIS:".len()))
                .unwrap_or(after_keyword.len());

            diagnosis = after_keyword[..end_pos.min(after_keyword.len())]
                .trim()
                .to_string();
        } else if let Some(action_pos) = response_upper.find("ACTION:") {
            // Prefixed prompt format: response starts with diagnosis directly, then ACTION:
            diagnosis = cleaned[..action_pos].trim().to_string();
        }

        // Extract ACTION
        for keyword in ["ACTION:", "RECOMMENDED ACTION:", "RECOMMENDED_ACTION:", "RECOMMENDATION:"] {
            if let Some(pos) = response_upper.find(keyword) {
                let after_keyword = &cleaned[pos + keyword.len()..];
                // Take until end of line or end of string
                let action_text = after_keyword
                    .lines()
                    .next()
                    .unwrap_or(after_keyword)
                    .trim();
                if !action_text.is_empty() {
                    action = action_text.to_string();
                    break;
                }
            }
        }

        // Fallback: Use entire response if no keywords found
        if diagnosis.is_empty() && action.is_empty() {
            tracing::warn!("No DIAGNOSIS or ACTION keywords found, treating entire response as diagnosis");
            diagnosis = if cleaned.len() > 500 {
                format!("{}... [truncated]", &cleaned[..500])
            } else {
                cleaned.to_string()
            };
        }

        // Generate default action if missing
        if action.is_empty() {
            action = match severity {
                "Healthy" => "Continue normal operations with standard monitoring.".to_string(),
                "Watch" => "Increase monitoring frequency and schedule inspection.".to_string(),
                "Warning" => "Schedule maintenance inspection within 24-48 hours.".to_string(),
                "Critical" => "Immediate shutdown and inspection required.".to_string(),
                _ => "Monitor equipment closely and follow standard procedures.".to_string(),
            };
        }

        // Convert severity string to enum
        let severity_enum = Severity::from_str_loose(severity);

        Ok(HealthAssessment {
            health_score,
            severity: severity_enum,
            diagnosis,
            recommended_action: action,
            confidence: 0.95, // High confidence since score is deterministic
            raw_response: None,
            timestamp: chrono::Utc::now(),
            rpm: 0.0,
        })
    }
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Quick health check using mock backend (for testing).
pub fn quick_health_check(current_rms: f64, baseline_rms: f64) -> (f64, Severity) {
    let ratio = if baseline_rms > 0.0 {
        current_rms / baseline_rms
    } else {
        1.0
    };

    let score = match ratio {
        r if r <= 1.2 => 90.0 - (r - 1.0) * 50.0, // 0-20% increase: 90-80
        r if r <= 1.5 => 80.0 - (r - 1.2) * 66.67, // 20-50% increase: 80-60
        r if r <= 2.0 => 60.0 - (r - 1.5) * 40.0, // 50-100% increase: 60-40
        r if r <= 3.0 => 40.0 - (r - 2.0) * 20.0, // 100-200% increase: 40-20
        _ => 20.0_f64.max(100.0 - ratio * 25.0),  // >200% increase: <20
    };

    let score = score.clamp(0.0, 100.0);
    (score, Severity::from_score(score))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_from_score() {
        assert_eq!(Severity::from_score(95.0), Severity::Healthy);
        assert_eq!(Severity::from_score(80.0), Severity::Healthy);
        assert_eq!(Severity::from_score(70.0), Severity::Watch);
        assert_eq!(Severity::from_score(50.0), Severity::Warning);
        assert_eq!(Severity::from_score(30.0), Severity::Critical);
        assert_eq!(Severity::from_score(0.0), Severity::Critical);
    }

    #[test]
    fn test_severity_from_str() {
        assert_eq!(Severity::from_str_loose("Healthy"), Severity::Healthy);
        assert_eq!(Severity::from_str_loose("CRITICAL"), Severity::Critical);
        assert_eq!(Severity::from_str_loose("warning level"), Severity::Warning);
        assert_eq!(Severity::from_str_loose("watch"), Severity::Watch);
        assert_eq!(Severity::from_str_loose("unknown"), Severity::Watch); // Default to Watch for safety
    }

    #[test]
    fn test_quick_health_check() {
        // Same RMS = healthy
        let (score, severity) = quick_health_check(1.0, 1.0);
        assert!(score >= 85.0);
        assert_eq!(severity, Severity::Healthy);

        // 50% increase = watch/warning
        let (score, severity) = quick_health_check(1.5, 1.0);
        assert!(score >= 55.0 && score <= 65.0);
        assert!(matches!(severity, Severity::Watch | Severity::Warning));

        // 200% increase = critical
        let (score, severity) = quick_health_check(3.0, 1.0);
        assert!(score <= 40.0);
        assert_eq!(severity, Severity::Critical);
    }

    #[test]
    fn test_health_assessment_flags() {
        let healthy = HealthAssessment {
            health_score: 90.0,
            severity: Severity::Healthy,
            diagnosis: "All good".to_string(),
            recommended_action: "Continue".to_string(),
            confidence: 0.95,
            raw_response: None,
            timestamp: chrono::Utc::now(),
            rpm: 100.0,
        };

        assert!(!healthy.requires_immediate_action());
        assert!(!healthy.requires_scheduled_maintenance());

        let critical = HealthAssessment {
            health_score: 20.0,
            severity: Severity::Critical,
            diagnosis: "Bad".to_string(),
            recommended_action: "Stop".to_string(),
            confidence: 0.95,
            raw_response: None,
            timestamp: chrono::Utc::now(),
            rpm: 100.0,
        };

        assert!(critical.requires_immediate_action());
        assert!(critical.requires_scheduled_maintenance());
    }
}
