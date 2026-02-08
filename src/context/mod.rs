//! Context module - Knowledge base and semantic search
//!
//! Provides contextual knowledge about equipment specifications,
//! failure modes, and drilling domain knowledge.
//!
//! ## KnowledgeStore trait
//!
//! The `KnowledgeStore` trait abstracts the knowledge query interface so
//! different backends can be swapped (static keyword DB, NoOp, future HNSW).

pub mod vector_db;
pub mod knowledge_store;

pub use vector_db::{search, search_by_category, search_with_limit, DocumentCategory};
pub use knowledge_store::{KnowledgeStore, NoOpStore, StaticKnowledgeBase};
