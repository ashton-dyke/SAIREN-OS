//! PersistenceLayer trait — pluggable storage backend
//!
//! Abstracts advisory and ML report persistence so different backends can be
//! swapped without touching pipeline code:
//! - `InMemoryDAL`: In-memory store for testing and minimal deployments
//! - Current sled backend can implement this trait
//! - Future: PostgreSQL for production fleet deployments

use crate::types::{MLInsightsReport, StrategicAdvisory};

/// Trait for pluggable persistence backends
///
/// Implementations must be thread-safe (Send + Sync) for shared access
/// across async tasks.
pub trait PersistenceLayer: Send + Sync {
    /// Store a strategic advisory
    fn store_advisory(&self, advisory: &StrategicAdvisory) -> Result<(), PersistenceError>;

    /// Get advisory by ID (timestamp-based)
    fn get_advisory(&self, timestamp: u64) -> Result<Option<StrategicAdvisory>, PersistenceError>;

    /// List recent advisories (most recent first)
    fn list_advisories(&self, limit: usize) -> Result<Vec<StrategicAdvisory>, PersistenceError>;

    /// Store an ML insights report
    fn store_ml_report(&self, report: &MLInsightsReport) -> Result<(), PersistenceError>;

    /// Get the latest ML insights report
    fn get_latest_ml_report(&self) -> Result<Option<MLInsightsReport>, PersistenceError>;

    /// Backend name for logging
    fn backend_name(&self) -> &'static str;
}

/// Persistence errors
#[derive(Debug, thiserror::Error)]
pub enum PersistenceError {
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("not found")]
    NotFound,
}

/// In-memory persistence for testing and minimal deployments
///
/// Thread-safe via `RwLock`. Not durable — data lost on restart.
pub struct InMemoryDAL {
    advisories: std::sync::RwLock<Vec<StrategicAdvisory>>,
    ml_reports: std::sync::RwLock<Vec<MLInsightsReport>>,
    max_advisories: usize,
    max_ml_reports: usize,
}

impl InMemoryDAL {
    /// Create a new in-memory store with default limits
    pub fn new() -> Self {
        Self {
            advisories: std::sync::RwLock::new(Vec::new()),
            ml_reports: std::sync::RwLock::new(Vec::new()),
            max_advisories: 1000,
            max_ml_reports: 100,
        }
    }
}

impl Default for InMemoryDAL {
    fn default() -> Self {
        Self::new()
    }
}

impl PersistenceLayer for InMemoryDAL {
    fn store_advisory(&self, advisory: &StrategicAdvisory) -> Result<(), PersistenceError> {
        let mut store = self
            .advisories
            .write()
            .map_err(|e| PersistenceError::Storage(e.to_string()))?;

        store.push(advisory.clone());

        // Evict oldest if over limit
        if store.len() > self.max_advisories {
            store.remove(0);
        }

        Ok(())
    }

    fn get_advisory(&self, timestamp: u64) -> Result<Option<StrategicAdvisory>, PersistenceError> {
        let store = self
            .advisories
            .read()
            .map_err(|e| PersistenceError::Storage(e.to_string()))?;

        Ok(store.iter().find(|a| a.timestamp == timestamp).cloned())
    }

    fn list_advisories(&self, limit: usize) -> Result<Vec<StrategicAdvisory>, PersistenceError> {
        let store = self
            .advisories
            .read()
            .map_err(|e| PersistenceError::Storage(e.to_string()))?;

        Ok(store.iter().rev().take(limit).cloned().collect())
    }

    fn store_ml_report(&self, report: &MLInsightsReport) -> Result<(), PersistenceError> {
        let mut store = self
            .ml_reports
            .write()
            .map_err(|e| PersistenceError::Storage(e.to_string()))?;

        store.push(report.clone());

        if store.len() > self.max_ml_reports {
            store.remove(0);
        }

        Ok(())
    }

    fn get_latest_ml_report(&self) -> Result<Option<MLInsightsReport>, PersistenceError> {
        let store = self
            .ml_reports
            .read()
            .map_err(|e| PersistenceError::Storage(e.to_string()))?;

        Ok(store.last().cloned())
    }

    fn backend_name(&self) -> &'static str {
        "InMemory"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        DrillingPhysicsReport, FinalSeverity, RiskLevel, StrategicAdvisory,
    };

    fn make_advisory(ts: u64) -> StrategicAdvisory {
        StrategicAdvisory {
            timestamp: ts,
            efficiency_score: 80,
            risk_level: RiskLevel::Low,
            severity: FinalSeverity::Low,
            recommendation: "test".to_string(),
            expected_benefit: "test".to_string(),
            reasoning: "test".to_string(),
            votes: Vec::new(),
            physics_report: DrillingPhysicsReport::default(),
            context_used: Vec::new(),
            trace_log: Vec::new(),
        }
    }

    #[test]
    fn test_in_memory_store_and_retrieve() {
        let dal = InMemoryDAL::new();
        let advisory = make_advisory(1000);
        dal.store_advisory(&advisory).unwrap();

        let retrieved = dal.get_advisory(1000).unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().timestamp, 1000);
    }

    #[test]
    fn test_in_memory_list_order() {
        let dal = InMemoryDAL::new();
        dal.store_advisory(&make_advisory(100)).unwrap();
        dal.store_advisory(&make_advisory(200)).unwrap();
        dal.store_advisory(&make_advisory(300)).unwrap();

        let list = dal.list_advisories(2).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].timestamp, 300); // most recent first
        assert_eq!(list[1].timestamp, 200);
    }

    #[test]
    fn test_trait_object() {
        let dal: Box<dyn PersistenceLayer> = Box::new(InMemoryDAL::new());
        assert_eq!(dal.backend_name(), "InMemory");
        dal.store_advisory(&make_advisory(42)).unwrap();
        assert_eq!(dal.list_advisories(10).unwrap().len(), 1);
    }
}
