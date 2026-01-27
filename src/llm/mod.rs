//! LLM Backend Module
//!
//! Provides a unified interface for LLM backends.
//!
//! ## Dual-Model Architecture
//!
//! - **TacticalLLM**: Qwen 2.5 1.5B Instruct for fast anomaly classification (60ms target)
//! - **StrategicLLM**: DeepSeek R1 Distill Qwen 7B for comprehensive diagnosis (800ms OK)
//!
//! Both models are loaded via mistral.rs with CUDA acceleration.

use anyhow::Result;
use async_trait::async_trait;
#[cfg(feature = "llm")]
use std::sync::Arc;

#[cfg(feature = "llm")]
mod mistral_rs;
#[cfg(feature = "llm")]
pub use mistral_rs::MistralRsBackend;

#[cfg(feature = "llm")]
pub mod scheduler;
#[cfg(feature = "llm")]
pub use scheduler::{LlmScheduler, SchedulerConfig, SchedulerHandle};

// Dual-model specialized interfaces
pub mod tactical_llm;
pub mod strategic_llm;

pub use tactical_llm::TacticalLLM;
pub use strategic_llm::StrategicLLM;

/// Unified trait for LLM backends
#[async_trait]
pub trait LlmBackend: Send + Sync {
    /// Generate a response from the LLM given a prompt
    async fn generate(&self, prompt: &str) -> Result<String>;

    /// Get the backend name for logging
    fn backend_name(&self) -> &'static str;

    /// Check if this backend uses GPU
    fn uses_gpu(&self) -> bool;
}

/// LLM Backend wrapper
#[cfg(feature = "llm")]
pub enum Backend {
    /// Real Mistral.rs backend with GPU support
    MistralRs(Arc<MistralRsBackend>),
}

#[cfg(feature = "llm")]
impl Backend {
    /// Get the backend name
    pub fn name(&self) -> &'static str {
        match self {
            Backend::MistralRs(b) => b.backend_name(),
        }
    }

    /// Check if backend uses GPU
    pub fn uses_gpu(&self) -> bool {
        match self {
            Backend::MistralRs(b) => b.uses_gpu(),
        }
    }

    /// Generate text from prompt
    pub async fn generate(&self, prompt: &str) -> Result<String> {
        match self {
            Backend::MistralRs(b) => b.generate(prompt).await,
        }
    }
}

/// Factory for creating LLM backends
#[cfg(feature = "llm")]
pub struct LlmFactory;

#[cfg(feature = "llm")]
impl LlmFactory {
    /// Create an LLM backend
    ///
    /// # Arguments
    ///
    /// * `model_path` - Path to GGUF model file
    ///
    /// # Returns
    ///
    /// A Backend enum wrapping the successfully loaded backend
    ///
    /// # Errors
    ///
    /// Returns an error if the model cannot be loaded
    pub async fn create(model_path: &str) -> Result<Backend> {
        tracing::info!(
            model_path = %model_path,
            "Attempting to load Mistral.rs backend with GPU support"
        );

        let backend = MistralRsBackend::load(model_path).await?;
        let backend = Arc::new(backend);

        tracing::info!(
            backend = backend.backend_name(),
            uses_gpu = backend.uses_gpu(),
            "Mistral.rs backend loaded successfully"
        );

        Ok(Backend::MistralRs(backend))
    }
}
