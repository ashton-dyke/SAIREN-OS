//! Tactical LLM - Fast Drilling Anomaly Classification
//!
//! Uses Qwen 2.5 1.5B Instruct for real-time drilling anomaly verification.
//! Target latency: ~60ms (GPU) / ~2-5s (CPU)
//!
//! The tactical LLM acts as a smart filter to reduce false positives from
//! the physics-based detection by analyzing drilling parameter context.

use crate::types::DrillingMetrics;
use anyhow::{Context, Result};
use std::sync::{Arc, OnceLock};
use std::time::Instant;
use tokio::sync::Mutex;

use super::{LlmBackend, MistralRsBackend};

/// Default model path for tactical model (Qwen 2.5 1.5B - used for both GPU and CPU)
const DEFAULT_TACTICAL_MODEL: &str = "models/qwen2.5-1.5b-instruct-q4_k_m.gguf";

/// Global singleton for TacticalLLM
static TACTICAL_INSTANCE: OnceLock<Arc<TacticalLLM>> = OnceLock::new();

/// Statistics tracking for tactical inference
#[derive(Debug, Default)]
struct TacticalStats {
    inference_count: u64,
    total_latency_ms: f64,
    confirmed_anomalies: u64,
    noise_filtered: u64,
}

/// Tactical LLM for fast drilling anomaly classification
///
/// Uses a small 1.5B model to quickly verify if detected anomalies
/// are real drilling issues or operational noise.
pub struct TacticalLLM {
    backend: Arc<MistralRsBackend>,
    stats: Mutex<TacticalStats>,
}

impl TacticalLLM {
    /// Initialize the tactical LLM singleton
    pub async fn init() -> Result<Arc<Self>> {
        if let Some(existing) = TACTICAL_INSTANCE.get() {
            return Ok(Arc::clone(existing));
        }

        let uses_gpu = super::is_cuda_available();
        let model_path = std::env::var("TACTICAL_MODEL_PATH")
            .unwrap_or_else(|_| DEFAULT_TACTICAL_MODEL.to_string());

        tracing::info!(
            model_path = %model_path,
            uses_gpu = uses_gpu,
            "Initializing Tactical LLM for drilling intelligence ({})",
            if uses_gpu { "GPU" } else { "CPU" }
        );

        let backend = MistralRsBackend::load(&model_path)
            .await
            .context("Failed to load tactical model")?;

        let instance = Arc::new(Self {
            backend: Arc::new(backend),
            stats: Mutex::new(TacticalStats::default()),
        });

        let _ = TACTICAL_INSTANCE.set(Arc::clone(&instance));

        tracing::info!(
            "Tactical LLM initialized successfully ({})",
            if uses_gpu { "GPU - target ~60ms" } else { "CPU - target ~2-5s" }
        );
        Ok(instance)
    }

    /// Get the global singleton instance
    pub fn get() -> Option<Arc<Self>> {
        TACTICAL_INSTANCE.get().cloned()
    }

    /// Classify whether the detected drilling anomaly is significant
    ///
    /// Returns `true` if this is a significant issue that should generate an advisory.
    /// Returns `false` if this is operational noise that should be filtered out.
    pub async fn classify(&self, metrics: &DrillingMetrics) -> Result<bool> {
        if !metrics_are_valid(metrics) {
            tracing::warn!("Skipping LLM classification: non-finite metrics");
            return Ok(false);
        }

        let start = Instant::now();

        let prompt = format!(
            r#"Analyze these drilling metrics and determine if this is a significant issue or operational noise.

DRILLING DATA:
- Rig State: {:?}
- MSE: {:.0} psi (Deviation: {:.0}%)
- D-exponent: {:.2}, DXC: {:.2}
- Flow Balance: {:.1} bbl/hr
- Pit Rate: {:.1} bbl/hr
- ECD Margin: {:.2} ppg
- Anomaly Category: {:?}

RULES:
- Flow imbalance > 10 bbl/hr during DRILLING = well control concern
- MSE deviation > 30% during DRILLING = efficiency issue
- D-exponent trend change = formation change
- High torque during REAMING = possible pack-off

Is this a SIGNIFICANT drilling issue? Answer only: YES or NO"#,
            metrics.state,
            metrics.mse,
            metrics.mse_delta_percent,
            metrics.d_exponent,
            metrics.dxc,
            metrics.flow_balance,
            metrics.pit_rate,
            metrics.ecd_margin,
            metrics.anomaly_category
        );

        let response = self
            .backend
            .generate_with_params(&prompt, 10, 0.1)
            .await
            .context("Tactical inference failed")?;

        let elapsed = start.elapsed();
        let is_significant = response.trim().to_uppercase().contains("YES");

        {
            let mut stats = self.stats.lock().await;
            stats.inference_count += 1;
            stats.total_latency_ms += elapsed.as_secs_f64() * 1000.0;
            if is_significant {
                stats.confirmed_anomalies += 1;
            } else {
                stats.noise_filtered += 1;
            }
        }

        tracing::debug!(
            latency_ms = elapsed.as_millis(),
            is_significant = is_significant,
            mse = metrics.mse,
            flow_balance = metrics.flow_balance,
            state = ?metrics.state,
            "Tactical drilling classification complete"
        );

        // Latency thresholds: 60ms for GPU, 5000ms for CPU
        let target_ms: u128 = if self.backend.uses_gpu() { 60 } else { 5000 };
        if elapsed.as_millis() > target_ms {
            tracing::warn!(
                latency_ms = elapsed.as_millis(),
                target_ms = target_ms,
                uses_gpu = self.backend.uses_gpu(),
                "Tactical inference exceeded target latency"
            );
        }

        Ok(is_significant)
    }

    /// Get inference statistics
    pub async fn stats(&self) -> TacticalLLMStats {
        let stats = self.stats.lock().await;
        TacticalLLMStats {
            inference_count: stats.inference_count,
            avg_latency_ms: if stats.inference_count > 0 {
                stats.total_latency_ms / stats.inference_count as f64
            } else {
                0.0
            },
            confirmed_anomalies: stats.confirmed_anomalies,
            noise_filtered: stats.noise_filtered,
        }
    }

    /// Reset statistics (for testing only)
    #[cfg(test)]
    pub async fn reset_stats(&self) {
        let mut stats = self.stats.lock().await;
        *stats = TacticalStats::default();
    }
}

/// Statistics from tactical LLM
#[derive(Debug, Clone)]
pub struct TacticalLLMStats {
    pub inference_count: u64,
    pub avg_latency_ms: f64,
    pub confirmed_anomalies: u64,
    pub noise_filtered: u64,
}

impl std::fmt::Display for TacticalLLMStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "TacticalLLM: {} inferences ({:.1}ms avg) - {} confirmed, {} filtered",
            self.inference_count,
            self.avg_latency_ms,
            self.confirmed_anomalies,
            self.noise_filtered
        )
    }
}

/// Check that all numeric fields interpolated into the LLM prompt are finite.
/// Non-finite values (NaN/Infinity) produce garbage prompts and waste inference.
fn metrics_are_valid(m: &DrillingMetrics) -> bool {
    m.mse.is_finite()
        && m.mse_delta_percent.is_finite()
        && m.d_exponent.is_finite()
        && m.dxc.is_finite()
        && m.flow_balance.is_finite()
        && m.pit_rate.is_finite()
        && m.ecd_margin.is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AnomalyCategory, RigState};

    fn create_test_metrics(is_anomaly: bool) -> DrillingMetrics {
        DrillingMetrics {
            state: RigState::Drilling,
            operation: crate::types::Operation::ProductionDrilling,
            mse: 45000.0,
            mse_efficiency: 75.0,
            d_exponent: 1.5,
            dxc: 1.4,
            mse_delta_percent: 25.0,
            flow_balance: 5.0,
            pit_rate: 2.0,
            ecd_margin: 0.4,
            torque_delta_percent: 10.0,
            spp_delta: 50.0,
            flow_data_available: true,
            is_anomaly,
            anomaly_category: if is_anomaly { AnomalyCategory::DrillingEfficiency } else { AnomalyCategory::None },
            anomaly_description: if is_anomaly { Some("Test anomaly".to_string()) } else { None },
            current_formation: None,
            formation_depth_in_ft: None,
        }
    }

    // Note: Tests require actual model files, so they're not runnable in CI.
    // The tactical LLM is tested via integration tests with the full pipeline.

    #[test]
    fn test_stats_display() {
        let stats = TacticalLLMStats {
            inference_count: 10,
            avg_latency_ms: 55.0,
            confirmed_anomalies: 6,
            noise_filtered: 4,
        };
        let display = format!("{}", stats);
        assert!(display.contains("10 inferences"));
        assert!(display.contains("55.0ms"));
    }
}
