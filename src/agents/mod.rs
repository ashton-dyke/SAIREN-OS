//! Multi-agent system for drilling operational intelligence
//!
//! ## Processing Pipeline Agents
//!
//! - **Tactical Agent** (Phase 2-3): Fast real-time drilling anomaly detection, < 15ms
//! - **Orchestrator** (Phase 8): Ensemble voting from 4 drilling specialists
//! - **Strategic Agent**: Deep analysis and advisory generation
//!
//! ## Specialists (Phase 8 voters)
//!
//! 1. MSE Specialist (25% weight) - drilling efficiency
//! 2. Hydraulic Specialist (25% weight) - SPP, flow, ECD margin
//! 3. WellControl Specialist (30% weight) - kick/loss, gas, pit volume (safety-critical)
//! 4. Formation Specialist (20% weight) - d-exponent, torque trends

pub mod tactical;
pub mod orchestrator;
pub mod strategic;
pub mod specialists;

pub use tactical::{TacticalAgent, TacticalMode, AgentStats, DrillingBaseline};
pub use orchestrator::Orchestrator;
pub use strategic::StrategicAgent;
pub use specialists::Specialist;
