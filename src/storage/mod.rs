//! Historical Analysis Storage
//!
//! This module provides persistent storage for health assessments using Sled DB.
//! It stores all LLM analyses with timestamps and provides efficient querying.

#![allow(dead_code)]

mod strategic;
pub mod history;
pub mod lockfile;
pub mod persistence;

pub use strategic::StrategicStorage;
pub use lockfile::ProcessLock;
pub use persistence::{PersistenceLayer, PersistenceError, InMemoryDAL};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

use crate::director::HealthAssessment;

/// Storage for historical health assessments
#[derive(Clone)]
pub struct AnalysisStorage {
    db: Arc<sled::Db>,
}

/// Stored analysis with additional metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredAnalysis {
    /// The health assessment
    pub assessment: HealthAssessment,

    /// Storage timestamp (when it was saved)
    pub stored_at: DateTime<Utc>,
}

impl AnalysisStorage {
    /// Open or create the analysis storage database
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();
        let db = sled::open(path_ref).context("Failed to open sled database")?;

        tracing::info!("Analysis storage opened at {:?}", path_ref);

        Ok(Self { db: Arc::new(db) })
    }

    /// Store a new health assessment
    pub fn store(&self, assessment: &HealthAssessment) -> Result<()> {
        let stored = StoredAnalysis {
            assessment: assessment.clone(),
            stored_at: Utc::now(),
        };

        // Use timestamp as key (nanoseconds since epoch for uniqueness)
        let key = assessment
            .timestamp
            .timestamp_nanos_opt()
            .unwrap_or_else(|| assessment.timestamp.timestamp() * 1_000_000_000)
            .to_be_bytes();

        let value = serde_json::to_vec(&stored).context("Failed to serialize assessment")?;

        self.db
            .insert(key, value)
            .context("Failed to insert assessment into database")?;

        self.db.flush().context("Failed to flush database")?;

        tracing::debug!(
            "Stored assessment: timestamp={}, score={}, severity={:?}",
            assessment.timestamp,
            assessment.health_score,
            assessment.severity
        );

        Ok(())
    }

    /// Get the most recent N assessments
    pub fn get_recent_history(&self, limit: usize) -> Result<Vec<HealthAssessment>> {
        let mut assessments = Vec::new();

        // Iterate in reverse order (most recent first)
        for item in self.db.iter().rev() {
            if assessments.len() >= limit {
                break;
            }

            let (_key, value) = item.context("Failed to read from database")?;

            match serde_json::from_slice::<StoredAnalysis>(&value) {
                Ok(stored) => assessments.push(stored.assessment),
                Err(e) => {
                    tracing::warn!("Failed to deserialize stored assessment: {}", e);
                    continue;
                }
            }
        }

        tracing::debug!("Retrieved {} recent assessments", assessments.len());

        Ok(assessments)
    }

    /// Get assessment by exact timestamp
    pub fn get_by_timestamp(&self, timestamp: DateTime<Utc>) -> Result<Option<HealthAssessment>> {
        let key = timestamp
            .timestamp_nanos_opt()
            .unwrap_or_else(|| timestamp.timestamp() * 1_000_000_000)
            .to_be_bytes();

        match self.db.get(key).context("Failed to read from database")? {
            Some(value) => {
                let stored: StoredAnalysis =
                    serde_json::from_slice(&value).context("Failed to deserialize assessment")?;
                Ok(Some(stored.assessment))
            }
            None => Ok(None),
        }
    }

    /// Get assessment by timestamp (with tolerance for finding nearby timestamps)
    pub fn get_by_timestamp_fuzzy(
        &self,
        timestamp: DateTime<Utc>,
        tolerance_secs: i64,
    ) -> Result<Option<HealthAssessment>> {
        let start = timestamp - Duration::seconds(tolerance_secs);
        let end = timestamp + Duration::seconds(tolerance_secs);

        let start_key = start
            .timestamp_nanos_opt()
            .unwrap_or_else(|| start.timestamp() * 1_000_000_000)
            .to_be_bytes();

        let end_key = end
            .timestamp_nanos_opt()
            .unwrap_or_else(|| end.timestamp() * 1_000_000_000)
            .to_be_bytes();

        // Find the first assessment within tolerance range
        for item in self.db.range(start_key..=end_key) {
            let (_key, value) = item.context("Failed to read from database")?;

            if let Ok(stored) = serde_json::from_slice::<StoredAnalysis>(&value) {
                return Ok(Some(stored.assessment));
            }
        }

        Ok(None)
    }

    /// Clean up old assessments (keep only last N days)
    pub fn cleanup_old(&self, days_to_keep: i64) -> Result<usize> {
        let cutoff = Utc::now() - Duration::days(days_to_keep);
        let cutoff_nanos = cutoff
            .timestamp_nanos_opt()
            .unwrap_or_else(|| cutoff.timestamp() * 1_000_000_000)
            .to_be_bytes();

        let mut deleted_count = 0;

        // Collect keys to delete
        let mut keys_to_delete = Vec::new();
        for item in self.db.iter() {
            let (key, value) = item.context("Failed to read from database")?;

            if key.as_ref() < cutoff_nanos.as_slice() {
                keys_to_delete.push(key.to_vec());
            } else {
                // Keys are sorted, so we can stop once we hit newer entries
                break;
            }

            // Also check if deserialization fails and delete corrupted entries
            if serde_json::from_slice::<StoredAnalysis>(&value).is_err() {
                keys_to_delete.push(key.to_vec());
            }
        }

        // Delete collected keys
        for key in keys_to_delete {
            self.db.remove(key).context("Failed to delete old entry")?;
            deleted_count += 1;
        }

        if deleted_count > 0 {
            self.db.flush().context("Failed to flush database")?;
            tracing::info!(
                "Cleaned up {} old assessments (kept last {} days)",
                deleted_count,
                days_to_keep
            );
        }

        Ok(deleted_count)
    }

    /// Get total number of stored assessments
    pub fn count(&self) -> usize {
        self.db.len()
    }

    /// Get statistics about stored assessments
    pub fn get_stats(&self) -> Result<StorageStats> {
        let count = self.count();

        let mut oldest: Option<DateTime<Utc>> = None;
        let mut newest: Option<DateTime<Utc>> = None;

        if count > 0 {
            // Get oldest (first entry)
            if let Some(Ok((_key, value))) = self.db.iter().next() {
                if let Ok(stored) = serde_json::from_slice::<StoredAnalysis>(&value) {
                    oldest = Some(stored.assessment.timestamp);
                }
            }

            // Get newest (last entry)
            if let Some(Ok((_key, value))) = self.db.iter().rev().next() {
                if let Ok(stored) = serde_json::from_slice::<StoredAnalysis>(&value) {
                    newest = Some(stored.assessment.timestamp);
                }
            }
        }

        Ok(StorageStats {
            total_count: count,
            oldest_timestamp: oldest,
            newest_timestamp: newest,
        })
    }
}

/// Statistics about the storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStats {
    pub total_count: usize,
    pub oldest_timestamp: Option<DateTime<Utc>>,
    pub newest_timestamp: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::director::Severity;

    fn create_test_assessment(timestamp: DateTime<Utc>) -> HealthAssessment {
        HealthAssessment {
            health_score: 75.0,
            severity: Severity::Watch,
            diagnosis: "Test diagnosis".to_string(),
            recommended_action: "Test action".to_string(),
            confidence: 0.95,
            raw_response: None,
            timestamp,
            rpm: 250.0,
        }
    }

    #[test]
    fn test_storage_open() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = AnalysisStorage::open(temp_dir.path()).unwrap();
        assert_eq!(storage.count(), 0);
    }

    #[test]
    fn test_store_and_retrieve() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = AnalysisStorage::open(temp_dir.path()).unwrap();

        let assessment = create_test_assessment(Utc::now());
        storage.store(&assessment).unwrap();

        let history = storage.get_recent_history(10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].health_score, 75.0);
    }

    #[test]
    fn test_cleanup_old() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = AnalysisStorage::open(temp_dir.path()).unwrap();

        // Store old assessment
        let old = create_test_assessment(Utc::now() - Duration::days(10));
        storage.store(&old).unwrap();

        // Store recent assessment
        let recent = create_test_assessment(Utc::now());
        storage.store(&recent).unwrap();

        assert_eq!(storage.count(), 2);

        // Clean up assessments older than 7 days
        let deleted = storage.cleanup_old(7).unwrap();
        assert_eq!(deleted, 1);
        assert_eq!(storage.count(), 1);
    }
}
