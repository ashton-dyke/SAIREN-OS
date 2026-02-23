//! Sensor data acquisition module
//!
//! Handles data ingestion from WITS data sources.

pub mod wits_parser;

pub use wits_parser::{WitsClient, WitsError};
