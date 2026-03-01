//! Shared data structures for WITS-based drilling operational intelligence
//!
//! This module defines the core types for the drilling advisory pipeline:
//! - Phase 1: WitsPacket (WITS Level 0 data)
//! - Phase 2-3: DrillingMetrics, AdvisoryTicket (tactical agent outputs)
//! - Phase 4: HistoryBuffer (packet circular buffer)
//! - Phase 5: DrillingPhysicsReport (drilling physics calculations)
//! - Phase 6: Context snippets from vector DB
//! - Phase 7: LLM advisory (RECOMMENDATION + REASONING)
//! - Phase 8: StrategicAdvisory (orchestrator output with weighted voting)

mod state;
mod wits;
mod tactical;
// Public because it contains the legacy `thresholds` sub-module
// which must remain accessible as `types::thresholds`.
pub mod thresholds;
mod advisory;
mod ticket;
mod ml;
mod formation;
mod optimization;
mod knowledge_base;
mod debrief;

pub use state::*;
pub use wits::*;
pub use tactical::*;
pub use thresholds::*;
pub use advisory::*;
pub use ticket::*;
pub use ml::*;
pub use formation::*;
pub use optimization::*;
pub use knowledge_base::*;
pub use debrief::*;
