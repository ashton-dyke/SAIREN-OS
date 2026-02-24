//! LLM Scheduler - Priority Queue for Dual-Model Inference
//!
//! Manages tactical (fast) and strategic (deep) models with strict priority:
//! - P0: Tactical (60-second loop, never delayed)
//! - P1: Strategic (hourly/daily, can be deferred)
//!
//! Single-threaded scheduler ensures no resource contention (GPU or CPU)
//! and predictable latency. Works identically on both GPU and CPU backends.

use anyhow::{Context, Result};
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, trace, warn};

use super::mistral_rs::MistralRsBackend;

// ============================================================================
// Request Types
// ============================================================================

/// Model selection for inference
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelId {
    /// Fast 1.5B tactical model (60-second loop)
    Tactical,
    /// Deeper 3.8B strategic model (hourly/daily)
    Strategic,
}

impl std::fmt::Display for ModelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelId::Tactical => write!(f, "Tactical"),
            ModelId::Strategic => write!(f, "Strategic"),
        }
    }
}

/// Request priority (lower number = higher priority)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// P0: Tactical analysis (60-second loop)
    Tactical = 0,
    /// P1: Strategic analysis (hourly/daily)
    Strategic = 1,
}

/// Inference request submitted to scheduler
#[derive(Debug)]
pub struct InferenceRequest {
    /// Model to use
    pub model_id: ModelId,
    /// Priority level
    pub priority: Priority,
    /// Prompt text
    pub prompt: String,
    /// Maximum tokens to generate
    pub max_tokens: usize,
    /// Temperature (0.0-1.0)
    pub temperature: f64,
    /// Response channel
    pub response_tx: oneshot::Sender<Result<String>>,
    /// Request enqueue time (for tie-breaking)
    pub enqueued_at: Instant,
}

/// Wrapper for priority queue ordering
struct PrioritizedRequest(InferenceRequest);

impl PartialEq for PrioritizedRequest {
    fn eq(&self, other: &Self) -> bool {
        self.0.priority == other.0.priority && self.0.enqueued_at == other.0.enqueued_at
    }
}

impl Eq for PrioritizedRequest {}

impl PartialOrd for PrioritizedRequest {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrioritizedRequest {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse ordering: lower priority value = higher priority
        // If equal priority, older request wins (earlier enqueued_at)
        match other.0.priority.cmp(&self.0.priority) {
            Ordering::Equal => other.0.enqueued_at.cmp(&self.0.enqueued_at),
            ord => ord,
        }
    }
}

// ============================================================================
// Scheduler Configuration
// ============================================================================

/// Scheduler configuration
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Deadline guard: don't start strategic if tactical due within this time
    pub tactical_deadline_guard_secs: u64,
    /// Expected tactical interval (60 seconds)
    pub tactical_interval_secs: u64,
    /// Channel buffer size
    pub channel_buffer_size: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            tactical_deadline_guard_secs: 10,
            tactical_interval_secs: 60,
            channel_buffer_size: 100,
        }
    }
}

// ============================================================================
// Scheduler Handle
// ============================================================================

/// Handle to submit requests to scheduler
#[derive(Clone)]
pub struct SchedulerHandle {
    tx: mpsc::Sender<InferenceRequest>,
    last_tactical: Arc<tokio::sync::Mutex<Option<Instant>>>,
}

impl SchedulerHandle {
    /// Submit tactical inference request (P0, max_tokens=40, temp=0.2)
    pub async fn infer_tactical(&self, prompt: String) -> Result<String> {
        let (response_tx, response_rx) = oneshot::channel();

        let request = InferenceRequest {
            model_id: ModelId::Tactical,
            priority: Priority::Tactical,
            prompt,
            max_tokens: 40,
            temperature: 0.2,
            response_tx,
            enqueued_at: Instant::now(),
        };

        // Update last tactical time
        {
            let mut last = self.last_tactical.lock().await;
            *last = Some(Instant::now());
        }

        self.tx
            .send(request)
            .await
            .context("Scheduler channel closed")?;

        response_rx.await.context("Response channel closed")?
    }

    /// Submit strategic inference request (P1, max_tokens configurable)
    pub async fn infer_strategic(
        &self,
        prompt: String,
        max_tokens: usize,
        temperature: f64,
    ) -> Result<String> {
        let (response_tx, response_rx) = oneshot::channel();

        let request = InferenceRequest {
            model_id: ModelId::Strategic,
            priority: Priority::Strategic,
            prompt,
            max_tokens,
            temperature,
            response_tx,
            enqueued_at: Instant::now(),
        };

        self.tx
            .send(request)
            .await
            .context("Scheduler channel closed")?;

        response_rx.await.context("Response channel closed")?
    }

    /// Get seconds until next tactical deadline (estimate)
    async fn seconds_until_tactical_deadline(&self, interval: u64) -> Option<u64> {
        let last = self.last_tactical.lock().await;
        last.map(|instant| {
            let elapsed = instant.elapsed().as_secs();
            interval.saturating_sub(elapsed)
        })
    }
}

// ============================================================================
// Scheduler Actor
// ============================================================================

/// LLM Scheduler - manages dual models with priority queue
pub struct LlmScheduler {
    /// Tactical model (fast)
    tactical_model: Arc<MistralRsBackend>,
    /// Strategic model (deep)
    strategic_model: Arc<MistralRsBackend>,
    /// Request receiver
    rx: mpsc::Receiver<InferenceRequest>,
    /// Priority queue
    queue: BinaryHeap<PrioritizedRequest>,
    /// Configuration
    config: SchedulerConfig,
    /// Last tactical request time
    last_tactical: Arc<tokio::sync::Mutex<Option<Instant>>>,
    /// Request counter for logging
    request_count: u64,
}

impl LlmScheduler {
    /// Create scheduler and return handle
    pub fn new(
        tactical_model: Arc<MistralRsBackend>,
        strategic_model: Arc<MistralRsBackend>,
        config: SchedulerConfig,
    ) -> (Self, SchedulerHandle) {
        let (tx, rx) = mpsc::channel(config.channel_buffer_size);
        let last_tactical = Arc::new(tokio::sync::Mutex::new(None));

        let scheduler = Self {
            tactical_model,
            strategic_model,
            rx,
            queue: BinaryHeap::new(),
            config: config.clone(),
            last_tactical: Arc::clone(&last_tactical),
            request_count: 0,
        };

        let handle = SchedulerHandle { tx, last_tactical };

        (scheduler, handle)
    }

    /// Run scheduler loop
    pub async fn run(mut self) {
        info!("LlmScheduler starting");

        loop {
            // Try to receive new requests (non-blocking check)
            match self.rx.try_recv() {
                Ok(request) => {
                    debug!(
                        model = %request.model_id,
                        priority = ?request.priority,
                        "Enqueued request"
                    );
                    self.queue.push(PrioritizedRequest(request));
                }
                Err(mpsc::error::TryRecvError::Empty) => {
                    // No new requests, process queue
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    info!("Scheduler channel closed, shutting down");
                    break;
                }
            }

            // Process highest priority request
            if let Some(request) = self.select_next_request().await {
                self.process_request(request).await;
            } else {
                // Queue empty, wait for next request
                match self.rx.recv().await {
                    Some(request) => {
                        debug!(
                            model = %request.model_id,
                            priority = ?request.priority,
                            "Enqueued request"
                        );
                        self.queue.push(PrioritizedRequest(request));
                    }
                    None => {
                        info!("Scheduler channel closed, shutting down");
                        break;
                    }
                }
            }
        }

        info!("LlmScheduler stopped");
    }

    /// Select next request with deadline guard
    async fn select_next_request(&mut self) -> Option<InferenceRequest> {
        if self.queue.is_empty() {
            return None;
        }

        let next = self.queue.peek()?;

        // If next is tactical, always process
        if next.0.priority == Priority::Tactical {
            return self.queue.pop().map(|p| p.0);
        }

        // Next is strategic - check deadline guard
        let last_tactical = self.last_tactical.lock().await;
        if let Some(last_instant) = *last_tactical {
            let elapsed = last_instant.elapsed().as_secs();
            let until_deadline = self
                .config
                .tactical_interval_secs
                .saturating_sub(elapsed);

            if until_deadline <= self.config.tactical_deadline_guard_secs {
                warn!(
                    queue_depth = self.queue.len(),
                    until_deadline_secs = until_deadline,
                    guard_secs = self.config.tactical_deadline_guard_secs,
                    "Strategic request deferred: tactical deadline imminent"
                );
                return None; // Don't process strategic, wait for tactical
            }
        }

        // Check if tactical is queued
        let has_tactical = self
            .queue
            .iter()
            .any(|p| p.0.priority == Priority::Tactical);
        if has_tactical {
            warn!(
                queue_depth = self.queue.len(),
                "Strategic request deferred: tactical request queued"
            );
            return None;
        }

        // Safe to process strategic
        self.queue.pop().map(|p| p.0)
    }

    /// Process single inference request
    async fn process_request(&mut self, request: InferenceRequest) {
        self.request_count += 1;

        let model = match request.model_id {
            ModelId::Tactical => &self.tactical_model,
            ModelId::Strategic => &self.strategic_model,
        };

        let queue_time = request.enqueued_at.elapsed();
        let start = Instant::now();

        debug!(
            request_id = self.request_count,
            model = %request.model_id,
            priority = ?request.priority,
            queue_time_ms = queue_time.as_millis(),
            queue_depth = self.queue.len(),
            "Processing request"
        );

        // Run inference with explicit cleanup
        let result = self
            .generate_with_cleanup(model, &request.prompt, request.max_tokens, request.temperature)
            .await;

        let inference_time = start.elapsed();

        match &result {
            Ok(response) => {
                info!(
                    request_id = self.request_count,
                    model = %request.model_id,
                    queue_time_ms = queue_time.as_millis(),
                    inference_time_ms = inference_time.as_millis(),
                    response_len = response.len(),
                    queue_depth = self.queue.len(),
                    "Request completed"
                );
            }
            Err(e) => {
                warn!(
                    request_id = self.request_count,
                    model = %request.model_id,
                    error = %e,
                    "Request failed"
                );
            }
        }

        // Send response (ignore if receiver dropped)
        let _ = request.response_tx.send(result);
    }

    /// Generate with explicit cleanup (KV cache leak prevention)
    async fn generate_with_cleanup(
        &self,
        model: &MistralRsBackend,
        prompt: &str,
        max_tokens: usize,
        temperature: f64,
    ) -> Result<String> {
        // Generate response
        let response = model.generate_with_params(prompt, max_tokens, temperature).await?;

        // Explicit cleanup delay to allow cache manager to process
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        trace!("KV cache cleanup completed");

        Ok(response)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_ordering() {
        let mut heap = BinaryHeap::new();

        let t1 = Instant::now();
        let t2 = t1 + std::time::Duration::from_millis(100);

        // Create requests (strategic enqueued first, tactical later)
        let (strategic_tx, _) = oneshot::channel();
        let strategic = PrioritizedRequest(InferenceRequest {
            model_id: ModelId::Strategic,
            priority: Priority::Strategic,
            prompt: "strategic".to_string(),
            max_tokens: 160,
            temperature: 0.3,
            response_tx: strategic_tx,
            enqueued_at: t1,
        });

        let (tactical_tx, _) = oneshot::channel();
        let tactical = PrioritizedRequest(InferenceRequest {
            model_id: ModelId::Tactical,
            priority: Priority::Tactical,
            prompt: "tactical".to_string(),
            max_tokens: 80,
            temperature: 0.7,
            response_tx: tactical_tx,
            enqueued_at: t2,
        });

        heap.push(strategic);
        heap.push(tactical);

        // Tactical should come out first (higher priority)
        let first = heap.pop().unwrap();
        assert_eq!(first.0.priority, Priority::Tactical);

        let second = heap.pop().unwrap();
        assert_eq!(second.0.priority, Priority::Strategic);
    }

    #[test]
    fn test_same_priority_tie_breaker() {
        let mut heap = BinaryHeap::new();

        let t1 = Instant::now();
        let t2 = t1 + std::time::Duration::from_millis(100);

        let (tx1, _) = oneshot::channel();
        let req1 = PrioritizedRequest(InferenceRequest {
            model_id: ModelId::Tactical,
            priority: Priority::Tactical,
            prompt: "first".to_string(),
            max_tokens: 80,
            temperature: 0.7,
            response_tx: tx1,
            enqueued_at: t1,
        });

        let (tx2, _) = oneshot::channel();
        let req2 = PrioritizedRequest(InferenceRequest {
            model_id: ModelId::Tactical,
            priority: Priority::Tactical,
            prompt: "second".to_string(),
            max_tokens: 80,
            temperature: 0.7,
            response_tx: tx2,
            enqueued_at: t2,
        });

        heap.push(req2);
        heap.push(req1);

        // Older request (t1) should come out first
        let first = heap.pop().unwrap();
        assert_eq!(first.0.prompt, "first");
    }
}
