//! Strategic Analysis Module
//!
//! Aggregates tactical analyses and generates hourly/daily strategic reports using
//! the strategic model via LlmScheduler. Also provides advisory composition.

#[cfg(feature = "llm")]
mod actor;
mod aggregation;
pub mod advisory;
pub(crate) mod parsing;
pub mod templates;

#[cfg(feature = "llm")]
pub use actor::{StrategicActor, StrategicActorHandle};
#[cfg(feature = "llm")]
pub use aggregation::TacticalAnalysis;
pub use advisory::AdvisoryComposer;
pub use parsing::{DailyReport, HourlyReport};
