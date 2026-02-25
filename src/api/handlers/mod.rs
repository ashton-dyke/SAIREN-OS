//! API route handlers
//!
//! Request handling logic for all API endpoints including:
//! - Drilling health status and advisory data
//! - System status with WITS drilling parameters
//! - Baseline learning status and dynamic thresholds

mod status;
mod drilling;
mod reports;
mod ml;
mod config;
mod metrics;

pub use status::*;
pub use drilling::*;
pub use reports::*;
pub use ml::*;
pub use config::*;
pub use metrics::*;

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::baseline::ThresholdManager;
use crate::ml_engine::MLInsightsStorage;
use crate::pipeline::AppState;

// ============================================================================
// API State
// ============================================================================

/// Shared state for API handlers
#[derive(Clone)]
pub struct DashboardState {
    /// Application state from the pipeline
    pub app_state: Arc<RwLock<AppState>>,
    /// Strategic report storage
    pub strategic_storage: Option<crate::storage::StrategicStorage>,
    /// Optional threshold manager for baseline status
    pub threshold_manager: Option<Arc<std::sync::RwLock<ThresholdManager>>>,
    /// Equipment ID for baseline lookups
    pub equipment_id: String,
    /// ML insights storage (V2.1)
    pub ml_storage: Option<Arc<MLInsightsStorage>>,
}

impl DashboardState {
    /// Create a new DashboardState with storage
    /// Create a new DashboardState with thresholds
    pub fn new_with_storage_and_thresholds(
        app_state: Arc<RwLock<AppState>>,
        threshold_manager: Arc<std::sync::RwLock<ThresholdManager>>,
        equipment_id: &str,
    ) -> Self {
        Self {
            app_state,
            strategic_storage: None,
            threshold_manager: Some(threshold_manager),
            equipment_id: equipment_id.to_string(),
            ml_storage: None,
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_state() -> DashboardState {
        DashboardState {
            app_state: Arc::new(RwLock::new(AppState::default())),
            strategic_storage: None,
            threshold_manager: None,
            equipment_id: "RIG".to_string(),
            ml_storage: None,
        }
    }

    #[tokio::test]
    async fn test_health_check() {
        let state = create_test_state();
        let response = legacy_health_check(axum::extract::State(state)).await;
        assert!(response.uptime_seconds >= 0);
    }

    #[tokio::test]
    async fn test_get_status() {
        let state = create_test_state();
        let response = get_status(axum::extract::State(state)).await;
        assert_eq!(response.total_analyses, 0);
    }

    #[tokio::test]
    async fn test_get_health_no_analysis() {
        let state = create_test_state();
        let response = get_health(axum::extract::State(state)).await;
        assert_eq!(response.health_score, 100.0);
        assert_eq!(response.confidence, 0.0);
    }

    #[tokio::test]
    async fn test_get_baseline_not_configured() {
        let state = create_test_state();
        let response = get_baseline_status(axum::extract::State(state)).await;
        assert_eq!(response.overall_status, "Not configured");
        assert_eq!(response.locked_count, 0);
        assert_eq!(response.learning_count, 0);
    }
}
