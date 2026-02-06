//! Tactical LLM - Fast Drilling Anomaly Classification
//!
//! Uses Qwen 2.5 1.5B Instruct for real-time drilling anomaly verification.
//! Target latency: ~60ms (GPU) / ~2-5s (CPU)
//!
//! The tactical LLM acts as a smart filter to reduce false positives from
//! the physics-based detection by analyzing drilling parameter context.

use crate::types::DrillingMetrics;
use anyhow::Result;
use std::sync::{Arc, OnceLock};
use std::time::Instant;
use tokio::sync::Mutex;

#[cfg(feature = "llm")]
use anyhow::Context;
#[cfg(feature = "llm")]
use std::env;
#[cfg(feature = "llm")]
use super::MistralRsBackend;

/// Default model path for tactical model (Qwen 2.5 1.5B - used for both GPU and CPU)
#[cfg(feature = "llm")]
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
    #[cfg(feature = "llm")]
    backend: Arc<MistralRsBackend>,
    stats: Mutex<TacticalStats>,
    #[cfg(not(feature = "llm"))]
    _phantom: std::marker::PhantomData<()>,
}

impl TacticalLLM {
    /// Initialize the tactical LLM singleton
    #[cfg(feature = "llm")]
    pub async fn init() -> Result<Arc<Self>> {
        if let Some(existing) = TACTICAL_INSTANCE.get() {
            return Ok(Arc::clone(existing));
        }

        let uses_gpu = super::is_cuda_available();
        let model_path = env::var("TACTICAL_MODEL_PATH")
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

    /// Initialize mock tactical LLM (when llm feature disabled)
    #[cfg(not(feature = "llm"))]
    pub async fn init() -> Result<Arc<Self>> {
        if let Some(existing) = TACTICAL_INSTANCE.get() {
            return Ok(Arc::clone(existing));
        }

        tracing::info!("Initializing Tactical LLM (MOCK - llm feature disabled)");

        let instance = Arc::new(Self {
            stats: Mutex::new(TacticalStats::default()),
            _phantom: std::marker::PhantomData,
        });

        let _ = TACTICAL_INSTANCE.set(Arc::clone(&instance));
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
    #[cfg(feature = "llm")]
    pub async fn classify(&self, metrics: &DrillingMetrics) -> Result<bool> {
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

    /// Mock classification when LLM feature is disabled
    #[cfg(not(feature = "llm"))]
    pub async fn classify(&self, metrics: &DrillingMetrics) -> Result<bool> {
        let start = Instant::now();

        // Mock classification based on physics anomaly flag
        let is_significant = metrics.is_anomaly;

        let elapsed = start.elapsed();

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
            is_significant = is_significant,
            mse = metrics.mse,
            flow_balance = metrics.flow_balance,
            state = ?metrics.state,
            "(MOCK) Tactical drilling classification"
        );

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
            is_anomaly,
            anomaly_category: if is_anomaly { AnomalyCategory::DrillingEfficiency } else { AnomalyCategory::None },
            anomaly_description: if is_anomaly { Some("Test anomaly".to_string()) } else { None },
        }
    }

    #[tokio::test]
    async fn test_mock_tactical_classification() {
        let llm = TacticalLLM::init().await.unwrap();

        let anomaly_metrics = create_test_metrics(true);
        let result = llm.classify(&anomaly_metrics).await.unwrap();
        assert!(result, "Should classify anomaly as significant");

        let normal_metrics = create_test_metrics(false);
        let result = llm.classify(&normal_metrics).await.unwrap();
        assert!(!result, "Should filter out normal operation");
    }

    #[tokio::test]
    async fn test_stats_tracking() {
        let llm = TacticalLLM::init().await.unwrap();
        llm.reset_stats().await;

        let metrics = create_test_metrics(true);
        llm.classify(&metrics).await.unwrap();
        llm.classify(&metrics).await.unwrap();

        let stats = llm.stats().await;
        assert_eq!(stats.inference_count, 2);
        assert_eq!(stats.confirmed_anomalies, 2);
    }
}
