//! ML Insights Storage (V2.1)
//!
//! Multi-well storage schema using Sled embedded database.
//! Key format: `{field_name}/{well_id}/{campaign}/{timestamp}`
//!
//! Enables:
//! - Per-well history queries
//! - Cross-well field-level queries (future)
//! - Campaign-specific filtering

use crate::types::{AnalysisResult, Campaign, MLInsightsReport};
use sled::Db;
use std::path::Path;
use tracing::debug;

/// Storage error types
#[derive(Debug)]
pub enum MLStorageError {
    /// Sled database error
    Database(sled::Error),
    /// Serialization error
    Serialization(serde_json::Error),
}

impl std::fmt::Display for MLStorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(e) => write!(f, "Database error: {}", e),
            Self::Serialization(e) => write!(f, "Serialization error: {}", e),
        }
    }
}

impl std::error::Error for MLStorageError {}

impl From<sled::Error> for MLStorageError {
    fn from(err: sled::Error) -> Self {
        MLStorageError::Database(err)
    }
}

impl From<serde_json::Error> for MLStorageError {
    fn from(err: serde_json::Error) -> Self {
        MLStorageError::Serialization(err)
    }
}

/// ML Insights persistent storage
pub struct MLInsightsStorage {
    db: Db,
}

#[allow(dead_code)]
impl MLInsightsStorage {
    /// Open or create the ML insights database
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, MLStorageError> {
        let db = sled::open(path)?;
        Ok(Self { db })
    }

    /// Open an in-memory database (for testing)
    #[cfg(test)]
    pub fn open_temp() -> Result<Self, MLStorageError> {
        let config = sled::Config::new().temporary(true);
        let db = config.open()?;
        Ok(Self { db })
    }

    /// Build storage key from report fields
    ///
    /// Key format: `{field_name}/{well_id}/{campaign}/{timestamp}`
    fn build_key(report: &MLInsightsReport) -> String {
        format!(
            "{}/{}/{:?}/{}",
            report.field_name, report.well_id, report.campaign, report.timestamp
        )
    }

    /// Store an ML insights report
    pub fn store_report(&self, report: &MLInsightsReport) -> Result<(), MLStorageError> {
        let key = Self::build_key(report);
        let value = serde_json::to_vec(report)?;
        self.db.insert(key.as_bytes(), value)?;

        debug!(
            key = %key,
            well_id = %report.well_id,
            "Stored ML insights report"
        );

        Ok(())
    }

    /// Get the latest report for a specific well
    pub fn get_latest(
        &self,
        well_id: &str,
        campaign: Option<Campaign>,
    ) -> Result<Option<MLInsightsReport>, MLStorageError> {
        let mut latest: Option<MLInsightsReport> = None;
        let mut latest_ts: u64 = 0;

        // Scan all entries (inefficient but simple for now)
        for result in self.db.iter() {
            let (key, value) = result?;
            if let Ok(key_str) = std::str::from_utf8(&key) {
                if key_str.contains(&format!("/{}/", well_id)) {
                    if let Ok(report) = serde_json::from_slice::<MLInsightsReport>(&value) {
                        // Filter by campaign if specified
                        if let Some(c) = campaign {
                            if report.campaign != c {
                                continue;
                            }
                        }
                        if report.timestamp > latest_ts {
                            latest_ts = report.timestamp;
                            latest = Some(report);
                        }
                    }
                }
            }
        }

        Ok(latest)
    }

    /// Get history for a specific well
    ///
    /// Returns reports in reverse chronological order (newest first)
    pub fn get_well_history(
        &self,
        well_id: &str,
        campaign: Option<Campaign>,
        limit: usize,
    ) -> Result<Vec<MLInsightsReport>, MLStorageError> {
        let mut reports: Vec<MLInsightsReport> = Vec::new();

        for result in self.db.iter() {
            let (key, value) = result?;
            if let Ok(key_str) = std::str::from_utf8(&key) {
                if key_str.contains(&format!("/{}/", well_id)) {
                    if let Ok(report) = serde_json::from_slice::<MLInsightsReport>(&value) {
                        // Filter by campaign if specified
                        if let Some(c) = campaign {
                            if report.campaign != c {
                                continue;
                            }
                        }
                        reports.push(report);
                    }
                }
            }
        }

        // Sort by timestamp descending (newest first)
        reports.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        // Limit results
        reports.truncate(limit);

        Ok(reports)
    }

    /// Get field-level history (cross-well)
    ///
    /// Returns reports from all wells in a field, newest first
    pub fn get_field_history(
        &self,
        field_name: &str,
        campaign: Option<Campaign>,
        limit: usize,
    ) -> Result<Vec<MLInsightsReport>, MLStorageError> {
        let prefix = format!("{}/", field_name);
        let mut reports: Vec<MLInsightsReport> = Vec::new();

        for result in self.db.scan_prefix(prefix.as_bytes()) {
            let (_, value) = result?;
            if let Ok(report) = serde_json::from_slice::<MLInsightsReport>(&value) {
                // Filter by campaign if specified
                if let Some(c) = campaign {
                    if report.campaign != c {
                        continue;
                    }
                }
                reports.push(report);
            }
        }

        // Sort by timestamp descending
        reports.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        reports.truncate(limit);

        Ok(reports)
    }

    /// Find reports near a specific depth
    ///
    /// Returns reports where the depth range overlaps with the query depth
    pub fn find_by_depth(
        &self,
        well_id: &str,
        depth: f64,
        tolerance: f64,
        limit: usize,
    ) -> Result<Vec<MLInsightsReport>, MLStorageError> {
        let mut reports: Vec<MLInsightsReport> = Vec::new();

        for result in self.db.iter() {
            let (key, value) = result?;
            if let Ok(key_str) = std::str::from_utf8(&key) {
                if key_str.contains(&format!("/{}/", well_id)) {
                    if let Ok(report) = serde_json::from_slice::<MLInsightsReport>(&value) {
                        // Check if depth is within range
                        let (min_depth, max_depth) = report.depth_range;
                        if depth >= min_depth - tolerance && depth <= max_depth + tolerance {
                            reports.push(report);
                        }
                    }
                }
            }
        }

        // Sort by how close the depth range is to query depth
        reports.sort_by(|a, b| {
            let a_dist = ((a.depth_range.0 + a.depth_range.1) / 2.0 - depth).abs();
            let b_dist = ((b.depth_range.0 + b.depth_range.1) / 2.0 - depth).abs();
            a_dist.partial_cmp(&b_dist).unwrap_or(std::cmp::Ordering::Equal)
        });

        reports.truncate(limit);

        Ok(reports)
    }

    /// Get count of stored reports
    pub fn count(&self) -> usize {
        self.db.len()
    }

    /// Get count of successful analyses
    pub fn count_successful(&self) -> Result<usize, MLStorageError> {
        let mut count = 0;
        for result in self.db.iter() {
            let (_, value) = result?;
            if let Ok(report) = serde_json::from_slice::<MLInsightsReport>(&value) {
                if matches!(report.result, AnalysisResult::Success(_)) {
                    count += 1;
                }
            }
        }
        Ok(count)
    }

    /// Flush any pending writes to disk
    pub fn flush(&self) -> Result<(), MLStorageError> {
        self.db.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        AnalysisInsights, ConfidenceLevel, OptimalParams, SignificantCorrelation,
    };

    fn make_report(
        well_id: &str,
        field_name: &str,
        campaign: Campaign,
        timestamp: u64,
        depth: f64,
    ) -> MLInsightsReport {
        MLInsightsReport {
            timestamp,
            campaign,
            depth_range: (depth - 50.0, depth + 50.0),
            well_id: well_id.to_string(),
            field_name: field_name.to_string(),
            bit_hours: 24.0,
            bit_depth: 500.0,
            formation_type: "Test Formation".to_string(),
            result: AnalysisResult::Success(AnalysisInsights {
                optimal_params: OptimalParams {
                    best_wob: 20.0,
                    best_rpm: 100.0,
                    best_flow: 500.0,
                    // Safe operating ranges (V2.2)
                    wob_min: 15.0,
                    wob_max: 25.0,
                    rpm_min: 80.0,
                    rpm_max: 120.0,
                    flow_min: 450.0,
                    flow_max: 550.0,
                    // Performance metrics
                    achieved_rop: 60.0,
                    achieved_mse: 18000.0,
                    mse_efficiency: 80.0,
                    composite_score: 0.75,
                    confidence: ConfidenceLevel::High,
                    // Stability metrics (V2.2)
                    stability_score: 0.85,
                    bin_sample_count: 50,
                    bins_evaluated: 48,
                    dysfunction_filtered: false,
                    regime_id: None,
                },
                correlations: vec![SignificantCorrelation {
                    x_param: "WOB".to_string(),
                    y_param: "ROP".to_string(),
                    r_value: 0.85,
                    r_squared: 0.72,
                    p_value: 0.001,
                    sample_count: 500,
                }],
                summary_text: "Test summary".to_string(),
                confidence: ConfidenceLevel::High,
                sample_count: 500,
            }),
        }
    }

    #[test]
    fn test_store_and_retrieve() {
        let storage = MLInsightsStorage::open_temp().unwrap();

        let report = make_report("WELL-001", "FIELD-A", Campaign::Production, 1000, 5000.0);
        storage.store_report(&report).unwrap();

        let retrieved = storage.get_latest("WELL-001", None).unwrap();
        assert!(retrieved.is_some());

        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.well_id, "WELL-001");
        assert_eq!(retrieved.timestamp, 1000);
    }

    #[test]
    fn test_get_latest_by_campaign() {
        let storage = MLInsightsStorage::open_temp().unwrap();

        // Store production and P&A reports
        let prod = make_report("WELL-001", "FIELD-A", Campaign::Production, 1000, 5000.0);
        let pa = make_report("WELL-001", "FIELD-A", Campaign::PlugAbandonment, 2000, 5000.0);

        storage.store_report(&prod).unwrap();
        storage.store_report(&pa).unwrap();

        // Get latest production
        let latest_prod = storage
            .get_latest("WELL-001", Some(Campaign::Production))
            .unwrap();
        assert!(latest_prod.is_some());
        assert_eq!(latest_prod.unwrap().campaign, Campaign::Production);

        // Get latest P&A
        let latest_pa = storage
            .get_latest("WELL-001", Some(Campaign::PlugAbandonment))
            .unwrap();
        assert!(latest_pa.is_some());
        assert_eq!(latest_pa.unwrap().campaign, Campaign::PlugAbandonment);
    }

    #[test]
    fn test_well_history() {
        let storage = MLInsightsStorage::open_temp().unwrap();

        // Store multiple reports for same well
        for i in 0..5 {
            let report = make_report(
                "WELL-001",
                "FIELD-A",
                Campaign::Production,
                1000 + i * 100,
                5000.0 + i as f64 * 100.0,
            );
            storage.store_report(&report).unwrap();
        }

        let history = storage.get_well_history("WELL-001", None, 10).unwrap();
        assert_eq!(history.len(), 5);

        // Should be in reverse chronological order
        assert!(history[0].timestamp > history[1].timestamp);
    }

    #[test]
    fn test_field_history() {
        let storage = MLInsightsStorage::open_temp().unwrap();

        // Store reports from different wells in same field
        let report1 = make_report("WELL-001", "FIELD-A", Campaign::Production, 1000, 5000.0);
        let report2 = make_report("WELL-002", "FIELD-A", Campaign::Production, 2000, 5500.0);
        let report3 = make_report("WELL-003", "FIELD-B", Campaign::Production, 3000, 6000.0);

        storage.store_report(&report1).unwrap();
        storage.store_report(&report2).unwrap();
        storage.store_report(&report3).unwrap();

        // Get field A history
        let field_a = storage.get_field_history("FIELD-A", None, 10).unwrap();
        assert_eq!(field_a.len(), 2);

        // Get field B history
        let field_b = storage.get_field_history("FIELD-B", None, 10).unwrap();
        assert_eq!(field_b.len(), 1);
    }

    #[test]
    fn test_find_by_depth() {
        let storage = MLInsightsStorage::open_temp().unwrap();

        // Store reports at different depths
        let report1 = make_report("WELL-001", "FIELD-A", Campaign::Production, 1000, 5000.0);
        let report2 = make_report("WELL-001", "FIELD-A", Campaign::Production, 2000, 6000.0);
        let report3 = make_report("WELL-001", "FIELD-A", Campaign::Production, 3000, 7000.0);

        storage.store_report(&report1).unwrap();
        storage.store_report(&report2).unwrap();
        storage.store_report(&report3).unwrap();

        // Search near 5000ft
        let near_5000 = storage.find_by_depth("WELL-001", 5000.0, 100.0, 10).unwrap();
        assert_eq!(near_5000.len(), 1);
        assert!((near_5000[0].depth_range.0 - 4950.0).abs() < 1.0);
    }

    #[test]
    fn test_count() {
        let storage = MLInsightsStorage::open_temp().unwrap();

        assert_eq!(storage.count(), 0);

        storage
            .store_report(&make_report("WELL-001", "FIELD-A", Campaign::Production, 1000, 5000.0))
            .unwrap();
        assert_eq!(storage.count(), 1);

        storage
            .store_report(&make_report("WELL-002", "FIELD-A", Campaign::Production, 2000, 5500.0))
            .unwrap();
        assert_eq!(storage.count(), 2);
    }
}
