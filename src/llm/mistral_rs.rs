//! Mistral.rs LLM Backend
//!
//! Provides LLM inference using mistral.rs with GGUF models.
//! Automatically detects CUDA availability at runtime:
//! - **CUDA available** (requires `cuda` feature): GPU inference with larger models
//! - **CPU fallback**: CPU inference with smaller, optimised models

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::sync::Arc;

use super::LlmBackend;

/// Model family for prompt formatting
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFamily {
    /// Qwen models use <|im_start|> chat template
    Qwen,
    /// Mistral models use [INST] chat template
    Mistral,
    /// Unknown/generic - use simple prompt
    Generic,
}

/// Mistral.rs backend using GGUF models with optional CUDA GPU support
pub struct MistralRsBackend {
    /// The loaded mistralrs instance
    mistralrs: Arc<mistralrs::MistralRs>,
    /// Model path for logging
    model_path: String,
    /// Whether GPU is being used
    uses_gpu: bool,
    /// Model family for prompt formatting
    model_family: ModelFamily,
}

/// Check if CUDA is available at runtime.
///
/// Returns `true` only if the binary was compiled with the `cuda` feature
/// AND CUDA libraries/drivers are detected on the system.
pub fn is_cuda_available() -> bool {
    #[cfg(feature = "cuda")]
    {
        // Check for CUDA environment variables and libraries
        std::env::var("CUDA_VISIBLE_DEVICES").is_ok()
            || std::path::Path::new("/usr/local/cuda").exists()
            || std::path::Path::new("/opt/cuda").exists()
            || std::path::Path::new("/usr/lib/x86_64-linux-gnu/libcuda.so").exists()
    }
    #[cfg(not(feature = "cuda"))]
    {
        false
    }
}

impl MistralRsBackend {
    /// Load a GGUF model from the specified path
    pub async fn load(model_path: &str) -> Result<Self> {
        use candle_core::Device;
        use mistralrs::{
            AutoDeviceMapParams, DefaultSchedulerMethod, DeviceMapSetting, LoaderBuilder,
            MistralRsBuilder, ModelDType, ModelSelected, SchedulerConfig, TokenSource,
        };

        let uses_gpu = is_cuda_available();

        tracing::info!(
            model_path = %model_path,
            uses_gpu = uses_gpu,
            "Loading GGUF model with mistral.rs backend"
        );

        // Check if model file exists
        let path = std::path::Path::new(model_path);
        if !path.exists() {
            anyhow::bail!("Model file not found: {}", model_path);
        }

        let start = std::time::Instant::now();

        // Select device based on CUDA availability
        let device = if uses_gpu {
            tracing::info!("CUDA detected, using GPU inference");
            #[cfg(feature = "cuda")]
            {
                Device::cuda_if_available(0).context("Failed to initialize CUDA device")?
            }
            #[cfg(not(feature = "cuda"))]
            {
                // Should not reach here since is_cuda_available() returns false
                // when cuda feature is not compiled in, but handle gracefully
                tracing::warn!("CUDA feature not compiled in, falling back to CPU");
                Device::Cpu
            }
        } else {
            tracing::info!("CUDA not available, using CPU inference");
            Device::Cpu
        };

        // Split model path into directory and filename
        let path_obj = std::path::Path::new(model_path);
        let model_dir = path_obj
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or(".")
            .to_string();
        let model_filename = path_obj
            .file_name()
            .and_then(|n| n.to_str())
            .context("Invalid model filename")?
            .to_string();

        // Adjust batch size based on device - CPU benefits from smaller batches
        let max_batch_size = if uses_gpu { 8 } else { 1 };

        // Create ModelSelected for GGUF
        let model = ModelSelected::GGUF {
            tok_model_id: None,
            quantized_model_id: model_dir,
            quantized_filename: model_filename,
            dtype: ModelDType::Auto,
            topology: None,
            max_seq_len: 4096,
            max_batch_size,
        };

        // Build loader
        let loader = LoaderBuilder::new(model)
            .build()
            .context("Failed to build loader")?;

        tracing::info!(
            "Loading model ({})",
            if uses_gpu { "GPU accelerated" } else { "CPU inference" }
        );

        // Load the pipeline (blocking operation)
        let pipeline = tokio::task::spawn_blocking(move || {
            loader.load_model_from_hf(
                None,                                                        // revision
                TokenSource::CacheToken,                                     // token_source
                &ModelDType::Auto,                                           // dtype
                &device,                                                     // device
                false,                                                       // silent
                DeviceMapSetting::Auto(AutoDeviceMapParams::default_text()), // mapper
                None,                                                        // in_situ_quant
                None,                                                        // paged_attn_config
            )
        })
        .await
        .context("Task join error")?
        .context("Failed to load model")?;

        // Create MistralRs instance with conservative scheduler settings
        // Use Fixed(1) to process one request at a time, preventing
        // accumulation of KV cache from concurrent requests
        let mistralrs = MistralRsBuilder::new(
            pipeline,
            SchedulerConfig::DefaultScheduler {
                method: DefaultSchedulerMethod::Fixed(std::num::NonZeroUsize::new(1).unwrap()),
            },
            false, // throughput_logging
            None,  // search_embedding_model
        )
        .build()
        .await;

        let load_time = start.elapsed();

        // Detect model family from path
        let model_family = Self::detect_model_family(model_path);

        tracing::info!(
            load_time_secs = load_time.as_secs_f32(),
            uses_gpu = uses_gpu,
            model_family = ?model_family,
            "Model loaded successfully ({})",
            if uses_gpu { "GPU" } else { "CPU" }
        );

        Ok(Self {
            mistralrs,
            model_path: model_path.to_string(),
            uses_gpu,
            model_family,
        })
    }

    /// Detect model family from model path
    fn detect_model_family(model_path: &str) -> ModelFamily {
        let path_lower = model_path.to_lowercase();
        if path_lower.contains("qwen") {
            ModelFamily::Qwen
        } else if path_lower.contains("mistral") {
            ModelFamily::Mistral
        } else {
            ModelFamily::Generic
        }
    }

    /// Build prompt with model-specific chat template
    fn build_instruct_prompt(&self, system: &str, user: &str) -> String {
        match self.model_family {
            ModelFamily::Qwen => {
                // Qwen 2.5 ChatML format
                format!(
                    "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
                    system, user
                )
            }
            ModelFamily::Mistral => {
                // Mistral Instruct format
                format!("<s>[INST] {}\n\n{} [/INST]", system, user)
            }
            ModelFamily::Generic => {
                // Simple format for unknown models
                format!("System: {}\n\nUser: {}\n\nAssistant:", system, user)
            }
        }
    }

    /// Get model-specific stop tokens
    fn get_stop_tokens(&self) -> Vec<String> {
        match self.model_family {
            ModelFamily::Qwen => {
                // Qwen uses <|im_end|> and <|endoftext|>
                vec!["<|im_end|>".to_string(), "<|endoftext|>".to_string()]
            }
            ModelFamily::Mistral => {
                // Mistral uses </s> and [/INST]
                vec!["</s>".to_string()]
            }
            ModelFamily::Generic => {
                // Generic stop tokens
                vec!["\n\n".to_string()]
            }
        }
    }
}

#[async_trait]
impl LlmBackend for MistralRsBackend {
    async fn generate(&self, prompt: &str) -> Result<String> {
        self.generate_with_params(prompt, 512, 0.7).await
    }

    fn backend_name(&self) -> &'static str {
        if self.uses_gpu {
            "Mistral.rs (CUDA)"
        } else {
            "Mistral.rs (CPU)"
        }
    }

    fn uses_gpu(&self) -> bool {
        self.uses_gpu
    }
}

impl MistralRsBackend {
    /// Generate with configurable parameters
    pub async fn generate_with_params(
        &self,
        prompt: &str,
        max_tokens: usize,
        temperature: f64,
    ) -> Result<String> {
        use mistralrs::{NormalRequest, Request, RequestMessage, Response, SamplingParams, StopTokens};

        let system_prompt =
            "You are a vibration analysis expert for industrial drilling equipment. Reply concisely.";
        let instruct_prompt = self.build_instruct_prompt(system_prompt, prompt);

        tracing::debug!(
            prompt_length = instruct_prompt.len(),
            max_tokens = max_tokens,
            temperature = temperature,
            "Sending request to mistral.rs"
        );

        // Create channel for response with sufficient capacity
        let (tx, mut rx) = tokio::sync::mpsc::channel(100);

        // Create a completion request
        let request = Request::Normal(Box::new(NormalRequest {
            messages: RequestMessage::Completion {
                text: instruct_prompt,
                echo_prompt: false,
                best_of: Some(1),
            },
            sampling_params: SamplingParams {
                temperature: Some(temperature),
                top_k: Some(50),
                top_p: Some(0.9),
                max_len: Some(max_tokens),
                stop_toks: Some(StopTokens::Seqs(self.get_stop_tokens())),
                logits_bias: None,
                n_choices: 1,
                top_n_logprobs: 0,
                frequency_penalty: None,
                presence_penalty: None,
                dry_params: None,
                min_p: None,
                repetition_penalty: None,
            },
            response: tx,
            return_raw_logits: false,
            return_logprobs: false,
            is_streaming: false,
            id: 0,
            constraint: mistralrs::Constraint::None,
            suffix: None,
            tool_choice: None,
            tools: None,
            logits_processors: None,
            web_search_options: None,
            model_id: None,
            truncate_sequence: false,
        }));

        // Send request in a blocking context to avoid blocking the tokio runtime
        let mistralrs_clone = self.mistralrs.clone();
        let send_result = tokio::task::spawn_blocking(move || {
            tracing::debug!("Sending request to mistral.rs in blocking context");
            mistralrs_clone
                .send_request(request)
                .map_err(|e| anyhow::anyhow!("Failed to send request: {:?}", e))
        })
        .await;

        match send_result {
            Ok(Ok(())) => {
                tracing::debug!("Request sent successfully, waiting for response");
            }
            Ok(Err(e)) => {
                tracing::error!("Failed to send request: {}", e);
                return Err(e);
            }
            Err(e) => {
                tracing::error!("Spawn blocking failed: {}", e);
                return Err(anyhow::anyhow!("Task join error: {}", e));
            }
        }

        // Wait for responses - use longer timeout for CPU inference
        let timeout_secs = if self.uses_gpu { 120 } else { 300 };
        tracing::debug!(timeout_secs = timeout_secs, "Waiting for response from mistral.rs");
        let text: String = loop {
            let response = tokio::time::timeout(
                std::time::Duration::from_secs(timeout_secs),
                rx.recv(),
            )
            .await
            .context(format!("Response timeout after {} seconds", timeout_secs))?
            .context("No response received from mistral.rs (channel closed)")?;

            tracing::debug!(
                "Received response from mistral.rs: {:?}",
                std::mem::discriminant(&response)
            );

            match response {
                Response::Chunk(_) | Response::CompletionChunk(_) => {
                    // Ignore streaming chunks when not streaming
                    tracing::debug!("Ignoring streaming chunk");
                    continue;
                }
                Response::Done(result) => {
                    tracing::debug!("Received Done response");
                    break result
                        .choices
                        .into_iter()
                        .next()
                        .and_then(|choice| choice.message.content)
                        .context("No text in Done response")?;
                }
                Response::CompletionDone(result) => {
                    tracing::debug!("Received CompletionDone response");
                    break result
                        .choices
                        .into_iter()
                        .next()
                        .map(|choice| choice.text.clone())
                        .context("No text in CompletionDone response")?;
                }
                Response::InternalError(e) => {
                    anyhow::bail!("Internal error from mistral.rs: {}", e)
                }
                Response::ValidationError(e) => {
                    anyhow::bail!("Validation error: {}", e)
                }
                Response::ModelError(e, resp) => {
                    anyhow::bail!("Model error: {} (response: {:?})", e, resp)
                }
                Response::CompletionModelError(e, resp) => {
                    anyhow::bail!("Completion model error: {} (response: {:?})", e, resp)
                }
                Response::ImageGeneration(_)
                | Response::Speech { .. }
                | Response::Raw { .. }
                | Response::Embeddings { .. } => {
                    anyhow::bail!("Unexpected response type (image/speech/raw/embeddings)")
                }
            }
        };

        tracing::debug!(
            response_length = text.len(),
            "Received response from mistral.rs"
        );

        // Drop channel receiver to free KV cache resources
        drop(rx);
        tracing::trace!("Channel receiver dropped, KV cache sequence should be freed");

        // Small delay to allow cache manager to process cleanup
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        Ok(text)
    }
}
