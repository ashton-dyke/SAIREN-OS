//! Authentication middleware for the Fleet Hub

pub mod api_key;

pub use api_key::{RigAuth, AdminAuth};
