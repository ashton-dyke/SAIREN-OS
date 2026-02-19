//! Background services — health checks and self-healing
//!
//! Runs as a background tokio task that monitors component health every 30 seconds
//! and performs automatic recovery where possible.

pub mod self_healer;

pub use self_healer::{HealthCheck, HealthStatus, HealAction, SelfHealer, ComponentHealth};
