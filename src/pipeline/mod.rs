//! Processing Pipeline Module
//!
//! ## 10-Phase Pipeline Architecture
//!
//! ```text
//! PHASE 1: Sensor Ingestion (every 60 seconds)
//! PHASE 2: Basic Physics (inside Tactical Agent, < 15ms)
//! PHASE 3: Tactical Agent Decision (ticket or discard)
//! PHASE 4: History Buffer (continuous, parallel)
//! PHASE 5: Advanced Physics (ONLY if ticket created)
//! PHASE 6: Context Lookup (ONLY if ticket created)
//! PHASE 7: LLM Explainer (ONLY if ticket created)
//! PHASE 8: Orchestrator Voting (ONLY if ticket created)
//! PHASE 9: Storage (ONLY if ticket created)
//! PHASE 10: Dashboard API (continuous)
//! ```
//!
//! CRITICAL GUARANTEE: Phases 5-9 ONLY execute if Tactical Agent created a ticket.
//!
//! # Usage
//!
//! ```ignore
//! use tds_guardian::pipeline::{PipelineCoordinator, VibrationProcessor, AppState};
//!
//! let mut coordinator = PipelineCoordinator::new();
//! let report = coordinator.process_packet(&sensor_packet).await;
//! ```

mod processor;
mod coordinator;

pub use processor::*;
pub use coordinator::PipelineCoordinator;
