//! Fleet data types for event sharing.
//!
//! Contains the `FleetEvent`, `FleetEpisode`, and related types used by
//! the P2P gossip protocol. The hub-and-spoke client/uploader/sync code
//! has been removed in favor of decentralized gossip.

pub mod types;

pub use types::{EventOutcome, FleetEpisode, FleetEvent};
