//! LLM Director Module
//!
//! Provides AI-powered vibration analysis using GGUF quantized LLMs.
//! The LLM Director interprets FFT spectrum data directly, providing
//! natural language health assessments and maintenance recommendations.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐     ┌─────────────┐     ┌──────────────────┐
//! │ FFT Spectrum │────▶│ format_for  │────▶│   LLM Backend    │
//! │    Data      │     │    _llm()   │     │   (4-bit GGUF)   │
//! └─────────────┘     └─────────────┘     └────────┬─────────┘
//!                                                  │
//!                                                  ▼
//!                                         ┌──────────────────┐
//!                                         │ HealthAssessment │
//!                                         │ - score: 0-100   │
//!                                         │ - severity       │
//!                                         │ - diagnosis      │
//!                                         │ - action         │
//!                                         └──────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use tds_guardian::director::{LlmDirector, HealthAssessment};
//! use tds_guardian::processing::{compute_fft, FrequencySpectrum};
//!
//! // Load the LLM
//! let director = LlmDirector::new("models/qwen2.5-1.5b-instruct-q4_k_m.gguf").await?;
//!
//! // Analyze current vibration against baseline
//! let assessment = director.analyze(&current_spectrum, &baseline_spectrum, rpm).await?;
//!
//! println!("Health Score: {}/100", assessment.health_score);
//! println!("Severity: {:?}", assessment.severity);
//! println!("Diagnosis: {}", assessment.diagnosis);
//! ```

mod llm_director;

pub use llm_director::*;
