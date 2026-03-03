//! P2P mesh gossip module.
//!
//! Implements decentralized event sharing between peer nodes.
//! Every node periodically broadcasts its recent events to all peers
//! and receives theirs. No central server, no special roles.
//!
//! ## Submodules
//!
//! - [`protocol`]: Wire types (`GossipEnvelope`) and zstd compression helpers
//! - [`store`]: `SQLite` event store with formation-based queries
//! - [`client`]: Gossip broadcast loop (periodic outbound exchanges)
//! - [`server`]: Axum handlers for incoming gossip and mesh status
//! - [`state`]: Per-peer sync cursor tracking (sled-backed)

pub mod client;
pub mod protocol;
pub mod server;
pub mod state;
pub mod store;
