//! Strategic Report History Storage
//!
//! Persists StrategicReports to Sled DB for historical analysis and dashboard display.
//! Uses timestamp-based keys for natural chronological ordering.

use crate::types::{FinalSeverity, StrategicReport};
use std::path::Path;
use std::sync::{Arc, OnceLock};

/// Global database instance for the history storage
static HISTORY_DB: OnceLock<Arc<sled::Db>> = OnceLock::new();


/// Error type for storage operations
#[derive(Debug)]
pub enum StorageError {
    DatabaseError(String),
    SerializationError(String),
    NotInitialized,
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::DatabaseError(msg) => write!(f, "Database error: {}", msg),
            StorageError::SerializationError(msg) => write!(f, "Serialization error: {}", msg),
            StorageError::NotInitialized => write!(f, "Storage not initialized"),
        }
    }
}

impl std::error::Error for StorageError {}

impl From<sled::Error> for StorageError {
    fn from(err: sled::Error) -> Self {
        StorageError::DatabaseError(err.to_string())
    }
}

impl From<serde_json::Error> for StorageError {
    fn from(err: serde_json::Error) -> Self {
        StorageError::SerializationError(err.to_string())
    }
}

/// History storage for StrategicReports
#[derive(Clone)]
#[allow(dead_code)]
pub struct HistoryStorage {
    db: Arc<sled::Db>,
}

#[allow(dead_code)]
impl HistoryStorage {
    /// Open or create the history storage at the specified path
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        let db = sled::open(path)?;
        Ok(Self { db: Arc::new(db) })
    }

    /// Store a strategic report
    ///
    /// Key: timestamp as u64 big-endian bytes (sorts chronologically)
    /// Value: JSON-serialized StrategicReport
    ///
    /// Note: Does not call flush() on each write for performance.
    /// Sled provides durability via background flushing. On crash,
    /// at most the last few writes may be lost (acceptable for this
    /// monitoring system since data is regenerated each cycle).
    pub fn store_report(&self, report: &StrategicReport) -> Result<(), StorageError> {
        // Use timestamp as key (big-endian for natural sorting)
        let key = report.timestamp.to_be_bytes();

        // Serialize report to JSON
        let value = serde_json::to_vec(report)?;

        // Insert into database
        self.db.insert(key, value)?;

        Ok(())
    }

    /// Get the most recent N reports (newest first)
    pub fn get_recent_history(&self, limit: usize) -> Vec<StrategicReport> {
        let mut reports = Vec::with_capacity(limit);

        // Iterate in reverse order (newest first due to big-endian timestamp keys)
        for item in self.db.iter().rev() {
            if reports.len() >= limit {
                break;
            }

            if let Ok((_key, value)) = item {
                if let Ok(report) = serde_json::from_slice::<StrategicReport>(&value) {
                    reports.push(report);
                }
            }
        }

        reports
    }

    /// Get all reports within a time range
    pub fn get_range(&self, start_ts: u64, end_ts: u64) -> Vec<StrategicReport> {
        let start_key = start_ts.to_be_bytes();
        let end_key = end_ts.to_be_bytes();

        let mut reports = Vec::new();

        for item in self.db.range(start_key..=end_key) {
            if let Ok((_key, value)) = item {
                if let Ok(report) = serde_json::from_slice::<StrategicReport>(&value) {
                    reports.push(report);
                }
            }
        }

        reports
    }

    /// Get total number of stored reports
    pub fn count(&self) -> usize {
        self.db.len()
    }

    /// Get database size in bytes
    pub fn size_bytes(&self) -> u64 {
        self.db.size_on_disk().unwrap_or(0)
    }

    /// Flush pending writes to disk
    pub fn flush(&self) -> Result<(), StorageError> {
        self.db.flush()?;
        Ok(())
    }

    /// Clear all reports (use with caution!)
    pub fn clear(&self) -> Result<(), StorageError> {
        self.db.clear()?;
        self.db.flush()?;
        Ok(())
    }

    /// Delete reports older than specified timestamp
    pub fn cleanup_before(&self, cutoff_ts: u64) -> Result<usize, StorageError> {
        let cutoff_key = cutoff_ts.to_be_bytes();
        let mut deleted = 0;

        let keys_to_delete: Vec<_> = self
            .db
            .iter()
            .filter_map(|item| {
                if let Ok((key, _)) = item {
                    if key.as_ref() < cutoff_key.as_slice() {
                        return Some(key.to_vec());
                    }
                }
                None
            })
            .collect();

        for key in keys_to_delete {
            self.db.remove(key)?;
            deleted += 1;
        }

        if deleted > 0 {
            self.db.flush()?;
        }

        Ok(deleted)
    }

    /// Get storage statistics
    pub fn stats(&self) -> StorageStats {
        let count = self.count();
        let size_bytes = self.size_bytes();

        let (oldest_ts, newest_ts) = if count > 0 {
            let oldest = self.db.iter().next().and_then(|r| {
                r.ok().map(|(k, _)| {
                    let mut bytes = [0u8; 8];
                    bytes.copy_from_slice(&k);
                    u64::from_be_bytes(bytes)
                })
            });
            let newest = self.db.iter().rev().next().and_then(|r| {
                r.ok().map(|(k, _)| {
                    let mut bytes = [0u8; 8];
                    bytes.copy_from_slice(&k);
                    u64::from_be_bytes(bytes)
                })
            });
            (oldest, newest)
        } else {
            (None, None)
        };

        StorageStats {
            report_count: count,
            size_bytes,
            oldest_timestamp: oldest_ts,
            newest_timestamp: newest_ts,
        }
    }
}

/// Storage statistics
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct StorageStats {
    pub report_count: usize,
    pub size_bytes: u64,
    pub oldest_timestamp: Option<u64>,
    pub newest_timestamp: Option<u64>,
}

#[allow(dead_code)]
impl StorageStats {
    /// Get size in megabytes
    pub fn size_mb(&self) -> f64 {
        self.size_bytes as f64 / (1024.0 * 1024.0)
    }
}

// ============================================================================
// Global/Static API for convenience (module-level functions)
// ============================================================================

/// Initialize the global history storage
pub fn init(path: &str) -> Result<(), StorageError> {
    let db = sled::open(path)?;
    HISTORY_DB
        .set(Arc::new(db))
        .map_err(|_| StorageError::DatabaseError("Already initialized".to_string()))?;
    Ok(())
}

/// Get the global database (initializes with default if not yet initialized)
pub(super) fn get_db() -> Result<&'static Arc<sled::Db>, StorageError> {
    HISTORY_DB.get().ok_or(StorageError::NotInitialized)
}

/// Store a report using the global database
///
/// Note: Does not call flush() on each write for performance.
/// Sled provides durability via background flushing.
pub fn store_report(report: &StrategicReport) -> Result<(), StorageError> {
    let db = get_db()?;

    let key = report.timestamp.to_be_bytes();
    let value = serde_json::to_vec(report)?;

    db.insert(key, value)?;

    Ok(())
}

/// Delete reports older than `max_age_days` days from the global database.
///
/// Returns the number of deleted records, or an error if the database is not
/// initialized or the deletion fails. Call once at startup after `init()` to
/// keep the on-disk history bounded.
pub fn prune_old_reports(max_age_days: u64) -> Result<usize, StorageError> {
    let db = get_db()?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let cutoff = now.saturating_sub(max_age_days * 86_400);

    // Reuse cleanup_before via a temporary wrapper that shares the global db handle.
    let storage = HistoryStorage { db: Arc::clone(db) };
    storage.cleanup_before(cutoff)
}

/// Look up a single report by its exact timestamp key.
pub fn get_by_timestamp(timestamp: u64) -> Result<Option<StrategicReport>, StorageError> {
    let db = get_db()?;
    let key = timestamp.to_be_bytes();
    match db.get(key)? {
        Some(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        None => Ok(None),
    }
}

/// Get all reports from the global database (oldest first).
pub fn get_all_reports() -> Vec<StrategicReport> {
    let db = match get_db() {
        Ok(db) => db,
        Err(_) => return Vec::new(),
    };

    db.iter()
        .filter_map(|item| {
            item.ok()
                .and_then(|(_, v)| serde_json::from_slice::<StrategicReport>(&v).ok())
        })
        .collect()
}

/// Get only Critical severity reports (newest first)
pub fn get_critical_reports(limit: usize) -> Vec<StrategicReport> {
    let db = match get_db() {
        Ok(db) => db,
        Err(_) => return Vec::new(),
    };

    let mut reports = Vec::with_capacity(limit);

    for item in db.iter().rev() {
        if reports.len() >= limit {
            break;
        }

        if let Ok((_key, value)) = item {
            if let Ok(report) = serde_json::from_slice::<StrategicReport>(&value) {
                // Only include Critical severity reports
                if report.severity == FinalSeverity::Critical {
                    reports.push(report);
                }
            }
        }
    }

    reports
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DrillingPhysicsReport, RiskLevel};

    fn create_test_report(ts: u64, score: f64) -> StrategicReport {
        StrategicReport {
            timestamp: ts,
            efficiency_score: score.clamp(0.0, 100.0) as u8,
            risk_level: RiskLevel::Low,
            severity: FinalSeverity::Medium,
            recommendation: "Test recommendation".to_string(),
            expected_benefit: "Test benefit".to_string(),
            reasoning: "Test reasoning".to_string(),
            votes: Vec::new(),
            physics_report: DrillingPhysicsReport::default(),
            context_used: Vec::new(),
            trace_log: Vec::new(),
            category: crate::types::AnomalyCategory::None,
            trigger_parameter: String::new(),
            trigger_value: 0.0,
            threshold_value: 0.0,
        }
    }

    #[test]
    fn test_storage_open() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("test.db");
        let storage = HistoryStorage::open(&path).unwrap();
        assert_eq!(storage.count(), 0);
    }

    #[test]
    fn test_store_and_retrieve() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("test.db");
        let storage = HistoryStorage::open(&path).unwrap();

        let report = create_test_report(1000, 75.0);
        storage.store_report(&report).unwrap();

        assert_eq!(storage.count(), 1);

        let history = storage.get_recent_history(10);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].efficiency_score, 75);
    }

    #[test]
    fn test_chronological_order() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("test.db");
        let storage = HistoryStorage::open(&path).unwrap();

        // Store reports out of order
        storage.store_report(&create_test_report(3000, 30.0)).unwrap();
        storage.store_report(&create_test_report(1000, 10.0)).unwrap();
        storage.store_report(&create_test_report(2000, 20.0)).unwrap();

        // Should come back newest first
        let history = storage.get_recent_history(10);
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].timestamp, 3000); // Newest
        assert_eq!(history[1].timestamp, 2000);
        assert_eq!(history[2].timestamp, 1000); // Oldest
    }

    #[test]
    fn test_limit() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("test.db");
        let storage = HistoryStorage::open(&path).unwrap();

        for i in 0..100 {
            storage
                .store_report(&create_test_report(i, i as f64))
                .unwrap();
        }

        assert_eq!(storage.count(), 100);

        let history = storage.get_recent_history(10);
        assert_eq!(history.len(), 10);
        assert_eq!(history[0].timestamp, 99); // Newest
    }

    #[test]
    fn test_cleanup() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("test.db");
        let storage = HistoryStorage::open(&path).unwrap();

        storage.store_report(&create_test_report(100, 10.0)).unwrap();
        storage.store_report(&create_test_report(200, 20.0)).unwrap();
        storage.store_report(&create_test_report(300, 30.0)).unwrap();

        assert_eq!(storage.count(), 3);

        // Delete before timestamp 250
        let deleted = storage.cleanup_before(250).unwrap();
        assert_eq!(deleted, 2);
        assert_eq!(storage.count(), 1);

        let history = storage.get_recent_history(10);
        assert_eq!(history[0].timestamp, 300);
    }

    #[test]
    fn test_range_query() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("test.db");
        let storage = HistoryStorage::open(&path).unwrap();

        for i in 0..10 {
            storage
                .store_report(&create_test_report(i * 100, i as f64 * 10.0))
                .unwrap();
        }

        let range = storage.get_range(200, 600);
        assert_eq!(range.len(), 5); // 200, 300, 400, 500, 600
    }

    #[test]
    fn test_stats() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("test.db");
        let storage = HistoryStorage::open(&path).unwrap();

        storage.store_report(&create_test_report(100, 10.0)).unwrap();
        storage.store_report(&create_test_report(500, 50.0)).unwrap();

        // Flush to ensure size_on_disk reflects written data
        storage.flush().unwrap();

        let stats = storage.stats();
        assert_eq!(stats.report_count, 2);
        assert_eq!(stats.oldest_timestamp, Some(100));
        assert_eq!(stats.newest_timestamp, Some(500));
        assert!(stats.size_bytes > 0);
    }
}
