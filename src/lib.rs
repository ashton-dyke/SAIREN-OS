//! SAIREN-OS: Drilling Operational Intelligence
//!
//! Multi-agent architecture for rig operational intelligence and drilling optimization.
//!
//! ## Architecture
//!
//! - **Tactical Agent**: Fast real-time drilling anomaly detection
//! - **Strategic Agent**: Deep analysis for advisory generation
//! - **Physics Engine**: Drilling calculations (MSE, d-exponent, kick/loss detection)
//! - **LLM Module**: Dual-model inference for drilling advisories

// Multi-agent architecture modules
pub mod config;
pub mod types;
pub mod agents;
pub mod physics_engine;
pub mod context;
pub mod sensors;
// Only expose the history sub-module from the library crate.
// The full storage module (AnalysisStorage, ProcessLock, etc.) is only
// available in the binary crate which declares its own `mod storage;`.
pub mod storage {
    pub mod history;
}
pub mod llm;
pub mod baseline;
pub mod aci;
pub mod cfc;
pub mod ml_engine;
pub mod strategic;
pub mod background;
pub mod fleet;
pub mod volve;
#[cfg(feature = "fleet-hub")]
pub mod hub;

// Re-export well configuration
pub use config::WellConfig;

// Re-export commonly used types
pub use types::{
    AdvisoryTicket, AnomalyCategory, DrillingMetrics, DrillingPhysicsReport,
    RigState, RiskLevel, StrategicAdvisory, TicketSeverity, TicketType, WitsPacket,
};

// Re-export ML Engine types
pub use types::{
    AnalysisFailure, AnalysisInsights, AnalysisResult, ConfidenceLevel,
    FormationSegment, HourlyDataset, MLInsightsReport, OptimalParams,
    SignificantCorrelation, ml_quality_thresholds,
};

// Re-export agents
pub use agents::{TacticalAgent, StrategicAgent};

// Re-export storage
pub use storage::history::{HistoryStorage, StorageError, StorageStats};

// Re-export LLM components
pub use llm::StrategicLLM;
#[cfg(feature = "tactical_llm")]
pub use llm::TacticalLLM;

// Re-export baseline components
pub use baseline::{
    AnomalyCheckResult, AnomalyLevel, BaselineAccumulator, BaselineError, DynamicThresholds,
    LearningStatus, ThresholdManager, wits_metrics,
};
