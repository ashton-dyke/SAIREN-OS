//! Proactive Optimization Engine
//!
//! Compares real-time drilling parameters against formation prognosis recommended
//! ranges and produces bounded, confidence-scored recommendations. Entirely
//! algorithmic — no LLM involved.

pub mod bit_wear;
mod confidence;
pub mod look_ahead;
mod optimizer;
mod rate_limiter;
pub mod templates;

pub use optimizer::ParameterOptimizer;
