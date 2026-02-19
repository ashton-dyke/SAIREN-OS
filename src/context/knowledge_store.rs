//! Knowledge Store trait and implementations
//!
//! Abstracts the knowledge query interface so different backends can be swapped:
//! - `StaticKnowledgeBase`: Keyword search over static documents (current)
//! - `NoOpStore`: Returns empty results (pilot mode without knowledge base)
//! - Future: `RAMRecall` for HNSW-based vector search (fleet mode)

/// Trait for knowledge store backends
///
/// Every implementation must be thread-safe (Send + Sync) since the pipeline
/// coordinator shares the store across async tasks.
pub trait KnowledgeStore: Send + Sync {
    /// Query the knowledge store for relevant context snippets
    fn query(&self, query: &str, max_results: usize) -> Vec<String>;

    /// Get the store name for logging and health checks
    fn store_name(&self) -> &'static str;

    /// Check if the store is healthy and available
    fn is_healthy(&self) -> bool;
}

/// NoOp knowledge store that returns empty results
///
/// Used in pilot mode when no fleet knowledge base is available.
/// Always reports healthy since "no knowledge" is a valid operational state.
pub struct NoOpStore;

impl KnowledgeStore for NoOpStore {
    fn query(&self, _query: &str, _max_results: usize) -> Vec<String> {
        Vec::new()
    }

    fn store_name(&self) -> &'static str {
        "NoOp"
    }

    fn is_healthy(&self) -> bool {
        true
    }
}

/// Wrapper around the existing static vector_db to implement KnowledgeStore
///
/// Delegates to the keyword-matching search in `vector_db::search_with_limit`.
pub struct StaticKnowledgeBase;

impl KnowledgeStore for StaticKnowledgeBase {
    fn query(&self, query: &str, max_results: usize) -> Vec<String> {
        super::vector_db::search_with_limit(query, max_results)
    }

    fn store_name(&self) -> &'static str {
        "StaticKB"
    }

    fn is_healthy(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noop_store() {
        let store = NoOpStore;
        assert!(store.query("anything", 5).is_empty());
        assert!(store.is_healthy());
        assert_eq!(store.store_name(), "NoOp");
    }

    #[test]
    fn test_static_kb() {
        let store = StaticKnowledgeBase;
        let results = store.query("bearing vibration", 3);
        assert!(!results.is_empty());
        assert!(store.is_healthy());
        assert_eq!(store.store_name(), "StaticKB");
    }

    #[test]
    fn test_trait_object() {
        let store: Box<dyn KnowledgeStore> = Box::new(NoOpStore);
        assert!(store.query("test", 3).is_empty());
        assert!(store.is_healthy());
    }
}
