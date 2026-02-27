//! LLM Backend Module
//!
//! ## Architecture
//!
//! - **Edge client** (default build, no `llm`/`cuda` feature): template-only
//!   advisory generation. Zero LLM inference, zero model files required.
//!
//! - **Hub server** (built with `--features llm,cuda`): embeds mistralrs as a
//!   Rust library and calls it directly from async workers. No separate HTTP
//!   inference server is spawned.
//!
//! ## Hub usage
//!
//! ```rust,ignore
//! use sairen_os::llm::{MistralRsBackend, strategic_llm};
//!
//! let backend = MistralRsBackend::load("models/qwen2.5-7b-instruct-q4_k_m.gguf").await?;
//! let prompt  = strategic_llm::build_prompt(&ticket, &metrics, &physics, &ctx, None, campaign);
//! let raw     = backend.generate_with_params(&prompt, 300, 0.3).await?;
//! let parsed  = strategic_llm::parse_response(&raw)?;
//! ```

use anyhow::Result;
use async_trait::async_trait;

/// Common trait for LLM backends
#[async_trait]
pub trait LlmBackend: Send + Sync {
    /// Generate a response from a prompt
    async fn generate(&self, prompt: &str) -> Result<String>;
    /// Backend name for logging
    fn backend_name(&self) -> &'static str;
    /// Whether this backend uses GPU acceleration
    fn uses_gpu(&self) -> bool;
}

// mistralrs backend — only compiled when the `llm` feature is enabled
#[cfg(feature = "llm")]
mod mistral_rs;
#[cfg(feature = "llm")]
pub use mistral_rs::MistralRsBackend;
#[cfg(feature = "llm")]
pub use mistral_rs::is_cuda_available;

// Tactical LLM (requires mistralrs backend)
#[cfg(feature = "llm")]
pub mod tactical_llm;
#[cfg(feature = "llm")]
pub use tactical_llm::TacticalLLM;

// LLM inference scheduler (requires mistralrs backend)
#[cfg(feature = "llm")]
pub mod scheduler;
#[cfg(feature = "llm")]
pub use scheduler::SchedulerHandle;

// Prompt templates and response parsing — always available (no inference required)
pub mod strategic_llm;
