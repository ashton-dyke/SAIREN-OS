//! Per-peer sync state tracking (sled-backed).
//!
//! Tracks the last sync cursor and failure count for each peer,
//! persisted across restarts via sled.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

/// Maximum backoff exponent (2^10 = 1024x base interval, capped at 3600s).
const MAX_BACKOFF_EXPONENT: u32 = 10;
/// Maximum backoff in seconds (1 hour).
const MAX_BACKOFF_SECS: u64 = 3600;

/// Sync state for a single peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerSyncState {
    pub peer_id: String,
    pub last_exchange_timestamp: u64,
    pub last_sync_cursor: u64,
    pub consecutive_failures: u32,
}

/// Mesh state tracker — stores per-peer sync cursors.
///
/// Backed by sled for persistence across restarts.
pub struct MeshState {
    tree: Option<sled::Tree>,
    // In-memory cache for fast access
    cache: RwLock<HashMap<String, PeerSyncState>>,
}

#[allow(clippy::missing_errors_doc)]
impl MeshState {
    /// Create a new `MeshState` backed by a sled tree.
    pub fn new(db: &sled::Db) -> Result<Self, sled::Error> {
        let tree = db.open_tree("mesh_peer_state")?;
        let mut cache = HashMap::new();

        // Load existing state from sled
        for (key, val) in (&tree).into_iter().flatten() {
            if let Ok(key_str) = std::str::from_utf8(&key) {
                if let Ok(state) = serde_json::from_slice::<PeerSyncState>(&val) {
                    cache.insert(key_str.to_string(), state);
                }
            }
        }

        Ok(Self {
            tree: Some(tree),
            cache: RwLock::new(cache),
        })
    }

    /// Create an in-memory-only `MeshState` (for testing).
    #[allow(dead_code)]
    #[must_use]
    pub fn in_memory() -> Self {
        Self {
            tree: None,
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// Get the sync cursor for a peer.
    pub fn get_cursor(&self, peer_id: &str) -> u64 {
        self.cache
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(peer_id)
            .map_or(0, |s| s.last_sync_cursor)
    }

    /// Update the sync cursor after a successful exchange.
    pub fn set_cursor(&self, peer_id: &str, cursor: u64) {
        let state = PeerSyncState {
            peer_id: peer_id.to_string(),
            last_exchange_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            last_sync_cursor: cursor,
            consecutive_failures: 0,
        };
        self.persist(peer_id, &state);
    }

    /// Record a successful exchange (resets failure counter).
    pub fn record_success(&self, peer_id: &str, cursor: u64) {
        self.set_cursor(peer_id, cursor);
    }

    /// Record a failed exchange (increments failure counter).
    pub fn record_failure(&self, peer_id: &str) {
        let mut cache = self
            .cache
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = cache
            .entry(peer_id.to_string())
            .or_insert_with(|| PeerSyncState {
                peer_id: peer_id.to_string(),
                last_exchange_timestamp: 0,
                last_sync_cursor: 0,
                consecutive_failures: 0,
            });
        state.consecutive_failures = state.consecutive_failures.saturating_add(1);
        let updated = state.clone();
        drop(cache);
        self.persist_to_sled(peer_id, &updated);
    }

    /// Get the backoff delay for a peer based on consecutive failures.
    ///
    /// Returns `base_interval * 2^min(failures, MAX_BACKOFF_EXPONENT)`,
    /// capped at `MAX_BACKOFF_SECS`.
    pub fn backoff_secs(&self, peer_id: &str, base_interval: u64) -> u64 {
        let failures = self
            .cache
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(peer_id)
            .map_or(0, |s| s.consecutive_failures);

        if failures == 0 {
            return base_interval;
        }

        let exponent = failures.min(MAX_BACKOFF_EXPONENT);
        let backoff = base_interval.saturating_mul(1u64 << exponent);
        let capped = backoff.min(MAX_BACKOFF_SECS);
        // Deterministic jitter from peer_id hash to avoid thundering herd
        let hash = peer_id
            .bytes()
            .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
        let jitter = hash % (capped / 4 + 1);
        capped.saturating_add(jitter).min(MAX_BACKOFF_SECS)
    }

    /// Get the consecutive failure count for a peer.
    pub fn failure_count(&self, peer_id: &str) -> u32 {
        self.cache
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(peer_id)
            .map_or(0, |s| s.consecutive_failures)
    }

    fn persist(&self, peer_id: &str, state: &PeerSyncState) {
        {
            let mut cache = self
                .cache
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            cache.insert(peer_id.to_string(), state.clone());
        }
        self.persist_to_sled(peer_id, state);
    }

    fn persist_to_sled(&self, peer_id: &str, state: &PeerSyncState) {
        if let Some(ref tree) = self.tree {
            if let Ok(bytes) = serde_json::to_vec(state) {
                if let Err(e) = tree.insert(peer_id, bytes) {
                    tracing::warn!(peer = peer_id, error = %e, "Failed to persist peer sync state");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_sync_state_in_memory() {
        let state = MeshState::in_memory();

        // Initial cursor should be 0
        assert_eq!(state.get_cursor("rig-002"), 0);

        // Set cursor
        state.set_cursor("rig-002", 1_700_000_000);
        assert_eq!(state.get_cursor("rig-002"), 1_700_000_000);

        // Record success with new cursor
        state.record_success("rig-002", 1_700_001_000);
        assert_eq!(state.get_cursor("rig-002"), 1_700_001_000);
        assert_eq!(state.failure_count("rig-002"), 0);
    }

    #[test]
    fn test_backoff_calculation() {
        let state = MeshState::in_memory();
        let base = 60u64;

        // No failures — base interval (no jitter when failures == 0)
        assert_eq!(state.backoff_secs("rig-002", base), 60);

        // 1 failure — 2x base + up to 25% jitter
        state.record_failure("rig-002");
        let b1 = state.backoff_secs("rig-002", base);
        assert!(b1 >= 120 && b1 <= 150, "expected 120..=150, got {b1}");

        // 2 failures — 4x base + up to 25% jitter
        state.record_failure("rig-002");
        let b2 = state.backoff_secs("rig-002", base);
        assert!(b2 >= 240 && b2 <= 300, "expected 240..=300, got {b2}");

        // 3 failures — 8x base + up to 25% jitter
        state.record_failure("rig-002");
        let b3 = state.backoff_secs("rig-002", base);
        assert!(b3 >= 480 && b3 <= 600, "expected 480..=600, got {b3}");

        // Many failures — capped at MAX_BACKOFF_SECS
        for _ in 0..20 {
            state.record_failure("rig-002");
        }
        assert!(state.backoff_secs("rig-002", base) <= MAX_BACKOFF_SECS);

        // Success resets
        state.record_success("rig-002", 100);
        assert_eq!(state.backoff_secs("rig-002", base), 60);
        assert_eq!(state.failure_count("rig-002"), 0);
    }

    #[test]
    fn test_peer_sync_state_persistence() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = sled::open(tmp.path().join("test_sled")).expect("open sled");

        {
            let state = MeshState::new(&db).expect("create MeshState");
            state.set_cursor("rig-002", 42);
            state.record_failure("rig-003");
            state.record_failure("rig-003");
        }

        // Re-open — state should be preserved
        {
            let state = MeshState::new(&db).expect("reopen MeshState");
            assert_eq!(state.get_cursor("rig-002"), 42);
            assert_eq!(state.failure_count("rig-003"), 2);
        }
    }
}
