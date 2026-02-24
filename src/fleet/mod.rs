//! Fleet Network — hub-and-spoke multi-rig learning
//!
//! Enables rigs to share anomaly events and learn from fleet-wide precedents.
//!
//! ## Architecture
//!
//! - **FleetEvent**: Confirmed AMBER/RED advisory with history window + outcome
//! - **FleetEpisode**: Compact precedent extracted from a FleetEvent (for library)
//! - **UploadQueue**: Disk-backed queue for reliable event upload to hub
//! - **FleetClient**: HTTP client for hub communication
//! - **LibrarySync**: Periodic precedent library synchronization
//! - **OutcomeForwarder**: Forwards driller acknowledgments to the hub
//!
//! ## Design Principles
//!
//! - Local autonomy: rig operates independently when hub is unreachable
//! - Event-only upload: only confirmed AMBER/RED events, not raw WITS data
//! - Idempotent: events keyed by advisory ID prevent duplicate uploads
//! - Bandwidth-conscious: zstd compression, delta sync, 6-hour cadence

pub mod types;
pub mod queue;
pub mod client;
pub mod uploader;
pub mod sync;

pub use types::{FleetEvent, FleetEpisode, EventOutcome, IntelligenceOutput, IntelligenceSyncResponse};
pub use queue::UploadQueue;
pub use client::FleetClient;

// Re-export fleet bridge types
pub use crate::knowledge_base::fleet_bridge::{FleetPerformanceUpload, FleetPerformanceResponse};
