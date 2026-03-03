//! Strategic Analysis Module
//!
//! Aggregates tactical analyses and generates strategic reports.
//! Also provides advisory composition.

pub mod advisory;
mod aggregation;
pub(crate) mod parsing;
pub mod templates;

pub use advisory::AdvisoryComposer;
pub use parsing::{DailyReport, HourlyReport};
