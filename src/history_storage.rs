//! History Storage Module
//!
//! Re-exports storage functionality for the multi-agent library.
//! This provides a clean interface for persisting StrategicReports.

use crate::types::StrategicReport;
use std::path::Path;
use std::sync::Arc;

/// Error type for storage operations
#[derive(Debug)]
pub enum StorageError {
    DatabaseError(String),
    SerializationError(String),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::DatabaseError(msg) => write!(f, "Database error: {}", msg),
            StorageError::SerializationError(msg) => write!(f, "Serialization error: {}", msg),
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
pub struct HistoryStorage {
    db: Arc<sled::Db>,
}

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
        let key = report.timestamp.to_be_bytes();
        let value = serde_json::to_vec(report)?;
        self.db.insert(key, value)?;
        Ok(())
    }

    /// Get the most recent N reports (newest first)
    pub fn get_recent_history(&self, limit: usize) -> Vec<StrategicReport> {
        let mut reports = Vec::with_capacity(limit);

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

    /// Clear all reports
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
pub struct StorageStats {
    pub report_count: usize,
    pub size_bytes: u64,
    pub oldest_timestamp: Option<u64>,
    pub newest_timestamp: Option<u64>,
}

impl StorageStats {
    /// Get size in megabytes
    pub fn size_mb(&self) -> f64 {
        self.size_bytes as f64 / (1024.0 * 1024.0)
    }
}
