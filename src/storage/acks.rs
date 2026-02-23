//! Advisory acknowledgment persistence
//!
//! Stores acknowledgment records in a named sled tree ("acknowledgments")
//! within the global history DB.  The storage layer is type-agnostic —
//! records are serialized to JSON at the call site and stored as raw bytes
//! keyed by `acknowledged_at` timestamp (big-endian u64, so they sort
//! chronologically).
//!
//! Call `persist()` from the API handler and `load_all_raw()` at startup
//! to restore the in-memory acknowledgment list.

use super::history::get_db;
use super::history::StorageError;
use serde::Serialize;
use std::sync::OnceLock;
use sled::Tree;

static ACKS_TREE: OnceLock<Tree> = OnceLock::new();

/// Initialise the acknowledgments sled tree.
///
/// Must be called after `storage::history::init()` so the global DB is ready.
/// Calling this a second time is a no-op.
pub fn init() -> Result<(), StorageError> {
    if ACKS_TREE.get().is_some() {
        return Ok(());
    }
    let db = get_db()?;
    let tree = db
        .open_tree("acknowledgments")
        .map_err(|e: sled::Error| StorageError::DatabaseError(e.to_string()))?;
    // OnceLock::set returns Err if already set — race is benign, both threads
    // opened the same named tree which is idempotent in sled.
    let _ = ACKS_TREE.set(tree);
    Ok(())
}

fn get_tree() -> Result<&'static Tree, StorageError> {
    ACKS_TREE.get().ok_or(StorageError::NotInitialized)
}

/// Persist an acknowledgment record.
///
/// `key` should be the record's timestamp (seconds) so records are ordered
/// chronologically.  If two records share the same second, the later one
/// overwrites the earlier — acceptable for an audit trail.
pub fn persist<T: Serialize>(key: u64, record: &T) -> Result<(), StorageError> {
    let tree = get_tree()?;
    let bytes = serde_json::to_vec(record)
        .map_err(|e| StorageError::SerializationError(e.to_string()))?;
    tree.insert(key.to_be_bytes(), bytes)?;
    Ok(())
}

/// Return raw JSON bytes for all stored acknowledgments (oldest first).
///
/// The caller is responsible for deserializing each entry to the appropriate
/// concrete type.  Entries that cannot be parsed by the caller should be
/// silently skipped.
pub fn load_all_raw() -> Vec<Vec<u8>> {
    let tree = match get_tree() {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };

    tree.iter()
        .filter_map(|item| item.ok().map(|(_, v)| v.to_vec()))
        .collect()
}
