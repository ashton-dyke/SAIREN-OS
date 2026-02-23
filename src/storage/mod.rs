//! Persistent Storage
//!
//! This module provides persistent storage for strategic reports and process locking.

mod strategic;
pub mod acks;
pub mod history;
pub mod lockfile;

pub use strategic::StrategicStorage;
pub use lockfile::ProcessLock;
