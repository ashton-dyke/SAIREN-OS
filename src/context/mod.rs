//! Context module - Knowledge base and semantic search
//!
//! Provides contextual knowledge about TDS-11SA equipment specifications,
//! failure modes, and maintenance procedures.

pub mod vector_db;

pub use vector_db::{search, search_by_category, search_with_limit, DocumentCategory};
