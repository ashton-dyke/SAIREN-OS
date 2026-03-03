//! SAIREN-OS: Drilling Operational Intelligence
//!
//! Multi-agent architecture for rig operational intelligence and drilling optimization.
//!
//! ## Architecture
//!
//! - **Tactical Agent**: Fast real-time drilling anomaly detection
//! - **Strategic Agent**: Deep analysis for advisory generation
//! - **Physics Engine**: Drilling calculations (MSE, d-exponent, kick/loss detection)
//! - **CfC Networks**: Continuous-time neural networks for pattern recognition

// Multi-agent architecture modules
pub mod agents;
pub mod config;
pub mod context;
pub mod physics_engine;
pub mod sensors;
pub mod types;
// Expose storage sub-modules needed by both lib and bin crates.
// The full storage module (ProcessLock, persistence, etc.) is only
// available in the binary crate which declares its own `mod storage;`.
pub mod storage {
    pub mod acks;
    pub mod damping_recipes;
    pub mod feedback;
    pub mod history;
    pub mod strategic;
    pub mod suggestions;
    pub use strategic::StrategicStorage;
}
pub mod aci;
pub mod acquisition;
pub mod api;
pub mod background;
pub mod baseline;
pub mod causal;
pub mod cfc;
pub mod debrief;
pub mod fleet;
pub mod gossip;
pub mod knowledge_base;
pub mod ml_engine;
pub mod optimization;
pub mod pipeline;
pub mod strategic;
pub mod volve;

// Re-export well configuration
pub use config::WellConfig;

// Re-export commonly used types
pub use types::{
    AdvisoryTicket, AnomalyCategory, DrillingMetrics, DrillingPhysicsReport, RigState, RiskLevel,
    StrategicAdvisory, TicketSeverity, TicketType, WitsPacket,
};

// Re-export ML Engine types
pub use types::{
    ml_quality_thresholds, AnalysisFailure, AnalysisInsights, AnalysisResult, ConfidenceLevel,
    FormationSegment, HourlyDataset, MLInsightsReport, OptimalParams, SignificantCorrelation,
};

// Re-export agents
pub use agents::{StrategicAgent, TacticalAgent};

// Re-export storage
pub use storage::history::StorageError;

// Re-export baseline components
pub use baseline::{
    wits_metrics, AnomalyCheckResult, AnomalyLevel, BaselineAccumulator, BaselineError,
    DynamicThresholds, LearningStatus, ThresholdManager,
};
