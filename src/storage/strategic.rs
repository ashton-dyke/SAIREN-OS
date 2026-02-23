//! Strategic Report Storage
//!
//! Persistent storage for hourly and daily strategic reports using Sled DB.
//! Uses separate trees for hourly and daily reports with 7-day retention.

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

use crate::strategic::{DailyReport, HourlyReport};

// ============================================================================
// Storage Structure
// ============================================================================

/// Storage for strategic reports (hourly and daily)
#[derive(Clone)]
pub struct StrategicStorage {
    db: Arc<sled::Db>,
}

/// Stored report with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredReport<T> {
    /// The report data
    pub report: T,
    /// Storage timestamp (when it was saved)
    pub stored_at: DateTime<Utc>,
}

// ============================================================================
// Implementation
// ============================================================================

#[cfg_attr(not(feature = "llm"), allow(dead_code))]
impl StrategicStorage {
    /// Open or create the strategic storage database
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();
        let db = sled::open(path_ref).context("Failed to open strategic storage")?;

        tracing::info!("Strategic storage opened at {:?}", path_ref);

        Ok(Self { db: Arc::new(db) })
    }

    /// Store a new hourly report
    ///
    /// Note: Does not call flush() on each write for performance.
    /// Sled provides durability via background flushing.
    pub fn store_hourly(&self, report: &HourlyReport) -> Result<()> {
        let tree = self
            .db
            .open_tree("strategic_hourly")
            .context("Failed to open hourly tree")?;

        let stored = StoredReport {
            report: report.clone(),
            stored_at: Utc::now(),
        };

        // Use current timestamp as key (nanoseconds since epoch)
        let key = Utc::now()
            .timestamp_nanos_opt()
            .unwrap_or_else(|| Utc::now().timestamp() * 1_000_000_000)
            .to_be_bytes();

        let value = serde_json::to_vec(&stored).context("Failed to serialize hourly report")?;

        tree.insert(key, value)
            .context("Failed to insert hourly report")?;

        tracing::debug!(
            "Stored hourly report: score={}, severity={}",
            report.health_score,
            report.severity
        );

        Ok(())
    }

    /// Store a new daily report
    ///
    /// Note: Does not call flush() on each write for performance.
    /// Sled provides durability via background flushing.
    pub fn store_daily(&self, report: &DailyReport) -> Result<()> {
        let tree = self
            .db
            .open_tree("strategic_daily")
            .context("Failed to open daily tree")?;

        let stored = StoredReport {
            report: report.clone(),
            stored_at: Utc::now(),
        };

        // Use current timestamp as key
        let key = Utc::now()
            .timestamp_nanos_opt()
            .unwrap_or_else(|| Utc::now().timestamp() * 1_000_000_000)
            .to_be_bytes();

        let value = serde_json::to_vec(&stored).context("Failed to serialize daily report")?;

        tree.insert(key, value)
            .context("Failed to insert daily report")?;

        tracing::debug!(
            "Stored daily report: score={}, severity={}, has_details={}",
            report.health_score,
            report.severity,
            report.details.is_some()
        );

        Ok(())
    }

    /// Get the most recent N hourly reports
    pub fn get_hourly(&self, limit: usize) -> Result<Vec<HourlyReport>> {
        let tree = self
            .db
            .open_tree("strategic_hourly")
            .context("Failed to open hourly tree")?;

        let mut reports = Vec::new();

        // Iterate in reverse order (most recent first)
        for item in tree.iter().rev() {
            if reports.len() >= limit {
                break;
            }

            let (_key, value) = item.context("Failed to read from hourly tree")?;

            match serde_json::from_slice::<StoredReport<HourlyReport>>(&value) {
                Ok(stored) => reports.push(stored.report),
                Err(e) => {
                    tracing::warn!("Failed to deserialize hourly report: {}", e);
                    continue;
                }
            }
        }

        tracing::debug!("Retrieved {} hourly reports", reports.len());

        Ok(reports)
    }

    /// Get the most recent N daily reports
    pub fn get_daily(&self, limit: usize) -> Result<Vec<DailyReport>> {
        let tree = self
            .db
            .open_tree("strategic_daily")
            .context("Failed to open daily tree")?;

        let mut reports = Vec::new();

        // Iterate in reverse order (most recent first)
        for item in tree.iter().rev() {
            if reports.len() >= limit {
                break;
            }

            let (_key, value) = item.context("Failed to read from daily tree")?;

            match serde_json::from_slice::<StoredReport<DailyReport>>(&value) {
                Ok(stored) => reports.push(stored.report),
                Err(e) => {
                    tracing::warn!("Failed to deserialize daily report: {}", e);
                    continue;
                }
            }
        }

        tracing::debug!("Retrieved {} daily reports", reports.len());

        Ok(reports)
    }

    /// Clean up old hourly reports (keep only last N days)
    pub fn cleanup_hourly(&self, days_to_keep: i64) -> Result<usize> {
        let tree = self
            .db
            .open_tree("strategic_hourly")
            .context("Failed to open hourly tree")?;

        let cutoff = Utc::now() - Duration::days(days_to_keep);
        let cutoff_nanos = cutoff
            .timestamp_nanos_opt()
            .unwrap_or_else(|| cutoff.timestamp() * 1_000_000_000)
            .to_be_bytes();

        let mut deleted_count = 0;
        let mut keys_to_delete = Vec::new();

        for item in tree.iter() {
            let (key, _value) = item.context("Failed to read from hourly tree")?;

            if key.as_ref() < cutoff_nanos.as_slice() {
                keys_to_delete.push(key.to_vec());
            } else {
                break; // Keys are sorted
            }
        }

        for key in keys_to_delete {
            tree.remove(key).context("Failed to delete old hourly report")?;
            deleted_count += 1;
        }

        if deleted_count > 0 {
            tree.flush().context("Failed to flush hourly tree")?;
            tracing::info!(
                "Cleaned up {} old hourly reports (kept last {} days)",
                deleted_count,
                days_to_keep
            );
        }

        Ok(deleted_count)
    }

    /// Clean up old daily reports (keep only last N days)
    pub fn cleanup_daily(&self, days_to_keep: i64) -> Result<usize> {
        let tree = self
            .db
            .open_tree("strategic_daily")
            .context("Failed to open daily tree")?;

        let cutoff = Utc::now() - Duration::days(days_to_keep);
        let cutoff_nanos = cutoff
            .timestamp_nanos_opt()
            .unwrap_or_else(|| cutoff.timestamp() * 1_000_000_000)
            .to_be_bytes();

        let mut deleted_count = 0;
        let mut keys_to_delete = Vec::new();

        for item in tree.iter() {
            let (key, _value) = item.context("Failed to read from daily tree")?;

            if key.as_ref() < cutoff_nanos.as_slice() {
                keys_to_delete.push(key.to_vec());
            } else {
                break; // Keys are sorted
            }
        }

        for key in keys_to_delete {
            tree.remove(key).context("Failed to delete old daily report")?;
            deleted_count += 1;
        }

        if deleted_count > 0 {
            tree.flush().context("Failed to flush daily tree")?;
            tracing::info!(
                "Cleaned up {} old daily reports (kept last {} days)",
                deleted_count,
                days_to_keep
            );
        }

        Ok(deleted_count)
    }

    /// Get count of hourly reports
    pub fn count_hourly(&self) -> Result<usize> {
        let tree = self
            .db
            .open_tree("strategic_hourly")
            .context("Failed to open hourly tree")?;
        Ok(tree.len())
    }

    /// Get count of daily reports
    pub fn count_daily(&self) -> Result<usize> {
        let tree = self
            .db
            .open_tree("strategic_daily")
            .context("Failed to open daily tree")?;
        Ok(tree.len())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategic::{HourlyReport, DailyReport};
    use crate::strategic::parsing::DetailsSection;

    fn create_test_hourly() -> HourlyReport {
        HourlyReport {
            health_score: 75.0,
            severity: "Warning".to_string(),
            diagnosis: "Test diagnosis".to_string(),
            action: "Test action".to_string(),
            raw: "RAW".to_string(),
        }
    }

    fn create_test_daily() -> DailyReport {
        DailyReport {
            health_score: 82.0,
            severity: "Healthy".to_string(),
            diagnosis: "Test diagnosis".to_string(),
            action: "Test action".to_string(),
            details: Some(DetailsSection {
                trend: "Improving".to_string(),
                top_drivers: "Motor temp stable".to_string(),
                confidence: "High".to_string(),
                next_check: "24h".to_string(),
            }),
            raw: "RAW".to_string(),
        }
    }

    #[test]
    fn test_storage_open() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = StrategicStorage::open(temp_dir.path()).unwrap();
        assert_eq!(storage.count_hourly().unwrap(), 0);
        assert_eq!(storage.count_daily().unwrap(), 0);
    }

    #[test]
    fn test_store_and_retrieve_hourly() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = StrategicStorage::open(temp_dir.path()).unwrap();

        let report = create_test_hourly();
        storage.store_hourly(&report).unwrap();

        let reports = storage.get_hourly(10).unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].health_score, 75.0);
    }

    #[test]
    fn test_store_and_retrieve_daily() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = StrategicStorage::open(temp_dir.path()).unwrap();

        let report = create_test_daily();
        storage.store_daily(&report).unwrap();

        let reports = storage.get_daily(10).unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].health_score, 82.0);
        assert!(reports[0].details.is_some());
    }

    #[test]
    fn test_cleanup_hourly() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = StrategicStorage::open(temp_dir.path()).unwrap();

        // Store reports
        storage.store_hourly(&create_test_hourly()).unwrap();
        storage.store_hourly(&create_test_hourly()).unwrap();

        assert_eq!(storage.count_hourly().unwrap(), 2);

        // Clean up old reports (7 days)
        let deleted = storage.cleanup_hourly(7).unwrap();

        // In test, reports are recent, so nothing should be deleted
        assert_eq!(deleted, 0);
        assert_eq!(storage.count_hourly().unwrap(), 2);
    }
}
