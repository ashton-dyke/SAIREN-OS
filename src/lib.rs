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
// Expose storage sub-modules needed by both lib and bin crates.
// The full storage module (ProcessLock, persistence, etc.) is only
// available in the binary crate which declares its own `mod storage;`.
pub mod storage {
    pub mod acks;
    pub mod history;
    pub mod strategic;
    pub use strategic::StrategicStorage;
}
pub mod llm;
pub mod baseline;
pub mod aci;
pub mod cfc;
pub mod ml_engine;
pub mod strategic;
pub mod optimization;
pub mod causal;
pub mod background;
pub mod fleet;
pub mod knowledge_base;
pub mod volve;
#[cfg(feature = "fleet-hub")]
pub mod hub;
pub mod acquisition;
pub mod pipeline;
pub mod api;

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
pub use storage::history::StorageError;

// Re-export LLM components
pub use llm::TacticalLLM;

// Re-export baseline components
pub use baseline::{
    AnomalyCheckResult, AnomalyLevel, BaselineAccumulator, BaselineError, DynamicThresholds,
    LearningStatus, ThresholdManager, wits_metrics,
};
