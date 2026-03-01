//! Operator feedback persistence
//!
//! Stores feedback records in a named sled tree ("feedback") within the global
//! history DB. Each record links an operator's assessment (confirmed / false
//! positive) to a specific advisory by timestamp. Category and trigger fields
//! are denormalized from the advisory for fast statistical queries.
//!
//! Call `init()` after `storage::history::init()`.

use super::history::{get_db, StorageError};
use crate::types::AnomalyCategory;
use serde::{Deserialize, Serialize};
use sled::Tree;
use std::sync::OnceLock;

static FEEDBACK_TREE: OnceLock<Tree> = OnceLock::new();

/// Operator assessment of an advisory.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackOutcome {
    Confirmed,
    FalsePositive,
    Unclear,
}

/// A single feedback record linking an operator assessment to an advisory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackRecord {
    /// Timestamp of the advisory being rated (also used as key).
    pub advisory_timestamp: u64,
    /// Operator's assessment.
    pub outcome: FeedbackOutcome,
    /// Anomaly category (denormalized from advisory).
    pub category: AnomalyCategory,
    /// Parameter that triggered the advisory (denormalized).
    pub trigger_parameter: String,
    /// Measured value at detection time (denormalized).
    pub trigger_value: f64,
    /// Threshold that was exceeded (denormalized).
    pub threshold_value: f64,
    /// Who submitted the feedback (e.g. "driller", "toolpusher").
    pub submitted_by: String,
    /// Unix timestamp when feedback was submitted.
    pub submitted_at: u64,
    /// Optional free-text notes from the operator.
    #[serde(default)]
    pub notes: String,
}

/// Initialise the feedback sled tree.
///
/// Must be called after `storage::history::init()`.
pub fn init() -> Result<(), StorageError> {
    if FEEDBACK_TREE.get().is_some() {
        return Ok(());
    }
    let db = get_db()?;
    let tree = db
        .open_tree("feedback")
        .map_err(|e: sled::Error| StorageError::DatabaseError(e.to_string()))?;
    let _ = FEEDBACK_TREE.set(tree);
    Ok(())
}

fn get_tree() -> Result<&'static Tree, StorageError> {
    FEEDBACK_TREE.get().ok_or(StorageError::NotInitialized)
}

/// Persist a feedback record keyed by advisory timestamp.
///
/// Last write wins if the same advisory is re-rated.
pub fn persist(record: &FeedbackRecord) -> Result<(), StorageError> {
    let tree = get_tree()?;
    let bytes = serde_json::to_vec(record)
        .map_err(|e| StorageError::SerializationError(e.to_string()))?;
    tree.insert(record.advisory_timestamp.to_be_bytes(), bytes)?;
    Ok(())
}

/// Load all feedback records (oldest first).
pub fn load_all() -> Vec<FeedbackRecord> {
    let tree = match get_tree() {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };

    tree.iter()
        .filter_map(|item| {
            item.ok()
                .and_then(|(_, v)| serde_json::from_slice(&v).ok())
        })
        .collect()
}

/// Look up feedback for a specific advisory timestamp.
pub fn get_by_advisory(timestamp: u64) -> Option<FeedbackRecord> {
    let tree = get_tree().ok()?;
    let bytes = tree.get(timestamp.to_be_bytes()).ok()??;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_record(ts: u64, outcome: FeedbackOutcome, category: AnomalyCategory) -> FeedbackRecord {
        FeedbackRecord {
            advisory_timestamp: ts,
            outcome,
            category,
            trigger_parameter: "test_param".to_string(),
            trigger_value: 42.0,
            threshold_value: 50.0,
            submitted_by: "driller".to_string(),
            submitted_at: ts + 100,
            notes: String::new(),
        }
    }

    #[test]
    fn test_feedback_serde_roundtrip() {
        let record = make_record(1000, FeedbackOutcome::Confirmed, AnomalyCategory::DrillingEfficiency);
        let json = serde_json::to_vec(&record).unwrap();
        let decoded: FeedbackRecord = serde_json::from_slice(&json).unwrap();
        assert_eq!(decoded.advisory_timestamp, 1000);
        assert_eq!(decoded.outcome, FeedbackOutcome::Confirmed);
    }
}
