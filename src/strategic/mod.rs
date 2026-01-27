//! Strategic Analysis Module
//!
//! Aggregates tactical analyses and generates hourly/daily strategic reports using
//! the strategic model via LlmScheduler.

#![allow(dead_code)]

#[cfg(feature = "llm")]
mod actor;
mod aggregation;
mod parsing;

#[cfg(feature = "llm")]
pub use actor::{StrategicActor, StrategicActorHandle};
#[cfg(feature = "llm")]
pub use aggregation::TacticalAnalysis;
pub use parsing::{DailyReport, DetailsSection, HourlyReport};