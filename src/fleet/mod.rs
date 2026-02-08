//! Fleet Network — hub-and-spoke multi-rig learning
//!
//! Enables rigs to share anomaly events and learn from fleet-wide precedents.
//!
//! ## Architecture
//!
//! - **FleetEvent**: Confirmed AMBER/RED advisory with history window + outcome
//! - **FleetEpisode**: Compact precedent extracted from a FleetEvent (for library)
//! - **UploadQueue**: Disk-backed queue for reliable event upload to hub
//! - **FleetClient** (future): HTTP client for hub communication
//! - **LibrarySync** (future): Periodic precedent library synchronization
//!
//! ## Design Principles
//!
//! - Local autonomy: rig operates independently when hub is unreachable
//! - Event-only upload: only confirmed AMBER/RED events, not raw WITS data
//! - Idempotent: events keyed by advisory ID prevent duplicate uploads
//! - Bandwidth-conscious: zstd compression, delta sync, 6-hour cadence

pub mod types;
pub mod queue;

pub use types::{FleetEvent, FleetEpisode, EventOutcome};
pub use queue::UploadQueue;
