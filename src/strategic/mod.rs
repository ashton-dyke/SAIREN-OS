//! Strategic Analysis Module
//!
//! Aggregates tactical analyses and generates hourly/daily strategic reports using
//! the strategic model via LlmScheduler. Also provides advisory composition.

#![allow(dead_code)]

#[cfg(feature = "llm")]
mod actor;
mod aggregation;
pub mod advisory;
mod parsing;
pub mod templates;

#[cfg(feature = "llm")]
pub use actor::{StrategicActor, StrategicActorHandle};
#[cfg(feature = "llm")]
pub use aggregation::TacticalAnalysis;
pub use advisory::{AdvisoryComposer, VotingResult};
pub use parsing::{DailyReport, DetailsSection, HourlyReport};
pub use templates::{template_advisory, TemplateAdvisory};