//! Vector Database for Contextual Knowledge
//!
//! Provides semantic search over equipment specifications and domain knowledge.
//! MVP implementation uses keyword matching; production would use embeddings.

use std::sync::OnceLock;

/// Knowledge document with metadata
#[derive(Debug, Clone)]
pub struct Document {
    pub id: usize,
    pub content: String,
    pub keywords: Vec<String>,
    pub category: DocumentCategory,
}

/// Categories of knowledge documents
#[derive(Debug, Clone, PartialEq)]
pub enum DocumentCategory {
    BearingSpec,
    VibrationThreshold,
    FailureMode,
    MaintenanceProcedure,
    OperationalLimit,
}

/// Static knowledge base - loaded once at startup
static KNOWLEDGE_BASE: OnceLock<Vec<Document>> = OnceLock::new();

/// Initialize the knowledge base with TDS-11SA specifications
fn get_knowledge_base() -> &'static Vec<Document> {
    KNOWLEDGE_BASE.get_or_init(|| {
        vec![
            // Bearing specifications
            Document {
                id: 1,
                content: "TDS-11SA Input Shaft Bearing BPFO is 7.2Hz at 60 RPM.".to_string(),
                keywords: vec![
                    "bearing".to_string(),
                    "bpfo".to_string(),
                    "input".to_string(),
                    "shaft".to_string(),
                    "frequency".to_string(),
                    "7.2".to_string(),
                    "outer".to_string(),
                    "race".to_string(),
                ],
                category: DocumentCategory::BearingSpec,
            },
            Document {
                id: 2,
                content: "TDS-11SA Input Shaft Bearing BPFI is 10.8Hz at 60 RPM.".to_string(),
                keywords: vec![
                    "bearing".to_string(),
                    "bpfi".to_string(),
                    "input".to_string(),
                    "shaft".to_string(),
                    "frequency".to_string(),
                    "10.8".to_string(),
                    "inner".to_string(),
                    "race".to_string(),
                ],
                category: DocumentCategory::BearingSpec,
            },
            Document {
                id: 3,
                content: "TDS-11SA Main Bearing BSF is 4.2Hz at 60 RPM.".to_string(),
                keywords: vec![
                    "bearing".to_string(),
                    "bsf".to_string(),
                    "main".to_string(),
                    "ball".to_string(),
                    "spin".to_string(),
                    "frequency".to_string(),
                ],
                category: DocumentCategory::BearingSpec,
            },
            // Vibration thresholds
            Document {
                id: 4,
                content: "Critical Vibration Threshold is 0.5g for continuous operation.".to_string(),
                keywords: vec![
                    "vibration".to_string(),
                    "threshold".to_string(),
                    "critical".to_string(),
                    "0.5g".to_string(),
                    "limit".to_string(),
                    "alarm".to_string(),
                ],
                category: DocumentCategory::VibrationThreshold,
            },
            Document {
                id: 5,
                content: "Warning Vibration Threshold is 0.3g - schedule inspection within 48 hours.".to_string(),
                keywords: vec![
                    "vibration".to_string(),
                    "threshold".to_string(),
                    "warning".to_string(),
                    "0.3g".to_string(),
                    "inspection".to_string(),
                ],
                category: DocumentCategory::VibrationThreshold,
            },
            // Failure modes
            Document {
                id: 6,
                content: "Outer race spalling typically manifests as BPFO harmonics with increasing amplitude over 2-4 weeks.".to_string(),
                keywords: vec![
                    "outer".to_string(),
                    "race".to_string(),
                    "spalling".to_string(),
                    "bpfo".to_string(),
                    "failure".to_string(),
                    "defect".to_string(),
                ],
                category: DocumentCategory::FailureMode,
            },
            Document {
                id: 7,
                content: "Inner race defects show BPFI sidebands modulated by shaft speed.".to_string(),
                keywords: vec![
                    "inner".to_string(),
                    "race".to_string(),
                    "defect".to_string(),
                    "bpfi".to_string(),
                    "sideband".to_string(),
                ],
                category: DocumentCategory::FailureMode,
            },
            Document {
                id: 8,
                content: "Bearing lubrication failure causes rapid temperature rise and broadband vibration increase.".to_string(),
                keywords: vec![
                    "lubrication".to_string(),
                    "failure".to_string(),
                    "temperature".to_string(),
                    "vibration".to_string(),
                    "bearing".to_string(),
                ],
                category: DocumentCategory::FailureMode,
            },
            // Operational limits
            Document {
                id: 9,
                content: "TDS-11SA maximum continuous RPM is 250. Gearbox oil temperature should not exceed 85C.".to_string(),
                keywords: vec![
                    "rpm".to_string(),
                    "maximum".to_string(),
                    "limit".to_string(),
                    "temperature".to_string(),
                    "gearbox".to_string(),
                    "oil".to_string(),
                ],
                category: DocumentCategory::OperationalLimit,
            },
            Document {
                id: 10,
                content: "Motor winding temperature alarm threshold is 120C. Shutdown at 140C.".to_string(),
                keywords: vec![
                    "motor".to_string(),
                    "temperature".to_string(),
                    "winding".to_string(),
                    "alarm".to_string(),
                    "shutdown".to_string(),
                ],
                category: DocumentCategory::OperationalLimit,
            },
            // Maintenance procedures
            Document {
                id: 11,
                content: "For elevated kurtosis readings, check for bearing cage damage or contamination.".to_string(),
                keywords: vec![
                    "kurtosis".to_string(),
                    "bearing".to_string(),
                    "cage".to_string(),
                    "contamination".to_string(),
                    "maintenance".to_string(),
                ],
                category: DocumentCategory::MaintenanceProcedure,
            },
            Document {
                id: 12,
                content: "Vibration trending showing exponential increase indicates imminent failure - plan replacement within 72 hours.".to_string(),
                keywords: vec![
                    "vibration".to_string(),
                    "trend".to_string(),
                    "exponential".to_string(),
                    "failure".to_string(),
                    "replacement".to_string(),
                    "urgent".to_string(),
                ],
                category: DocumentCategory::MaintenanceProcedure,
            },
        ]
    })
}

/// Search the knowledge base for relevant documents
///
/// Uses keyword matching for MVP. Production would use vector embeddings.
/// Returns up to `max_results` matching documents sorted by relevance.
pub fn search(query: &str) -> Vec<String> {
    search_with_limit(query, 3)
}

/// Search with configurable result limit
pub fn search_with_limit(query: &str, max_results: usize) -> Vec<String> {
    let kb = get_knowledge_base();
    let query_lower = query.to_lowercase();
    let query_terms: Vec<&str> = query_lower.split_whitespace().collect();

    // Score each document by keyword matches
    let mut scored: Vec<(usize, &Document)> = kb
        .iter()
        .map(|doc| {
            let score = calculate_relevance_score(&query_terms, doc);
            (score, doc)
        })
        .filter(|(score, _)| *score > 0)
        .collect();

    // Sort by score descending
    scored.sort_by(|a, b| b.0.cmp(&a.0));

    // Return top results
    scored
        .into_iter()
        .take(max_results)
        .map(|(_, doc)| doc.content.clone())
        .collect()
}

/// Search by category
pub fn search_by_category(category: DocumentCategory) -> Vec<String> {
    let kb = get_knowledge_base();
    kb.iter()
        .filter(|doc| doc.category == category)
        .map(|doc| doc.content.clone())
        .collect()
}

/// Calculate relevance score based on keyword matching
fn calculate_relevance_score(query_terms: &[&str], doc: &Document) -> usize {
    let mut score = 0;

    for term in query_terms {
        // Check keywords (higher weight)
        for keyword in &doc.keywords {
            if keyword.contains(term) || term.contains(keyword.as_str()) {
                score += 2;
            }
        }

        // Check content (lower weight)
        if doc.content.to_lowercase().contains(term) {
            score += 1;
        }
    }

    score
}

/// Get all documents (for debugging/testing)
pub fn get_all_documents() -> Vec<String> {
    get_knowledge_base()
        .iter()
        .map(|doc| doc.content.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_bearing() {
        let results = search("bearing vibration");
        assert!(!results.is_empty(), "Should find bearing-related docs");
        println!("Bearing search results: {:?}", results);
    }

    #[test]
    fn test_search_bpfo() {
        let results = search("BPFO outer race");
        assert!(!results.is_empty(), "Should find BPFO docs");
        assert!(
            results.iter().any(|r| r.contains("BPFO")),
            "Results should contain BPFO info"
        );
    }

    #[test]
    fn test_search_threshold() {
        let results = search("vibration threshold critical");
        assert!(!results.is_empty(), "Should find threshold docs");
        assert!(
            results.iter().any(|r| r.contains("0.5g") || r.contains("0.3g")),
            "Results should contain threshold values"
        );
    }

    #[test]
    fn test_search_kurtosis() {
        let results = search("kurtosis elevated");
        assert!(!results.is_empty(), "Should find kurtosis-related docs");
    }

    #[test]
    fn test_search_by_category() {
        let specs = search_by_category(DocumentCategory::BearingSpec);
        assert!(specs.len() >= 2, "Should have multiple bearing specs");

        let thresholds = search_by_category(DocumentCategory::VibrationThreshold);
        assert!(!thresholds.is_empty(), "Should have vibration thresholds");
    }

    #[test]
    fn test_empty_query() {
        let results = search("");
        assert!(results.is_empty(), "Empty query should return no results");
    }
}
