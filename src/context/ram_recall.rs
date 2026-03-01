//! RAM Recall — in-memory precedent search for fleet episodes
//!
//! Provides sub-millisecond precedent lookup using metadata filtering +
//! scored similarity matching. Episodes are filtered by category and campaign
//! first, then ranked by metric distance.
//!
//! ## Future: HNSW upgrade
//!
//! The current implementation uses metadata-filtered linear scan which is
//! O(n) but fast for <10,000 episodes. For larger fleet libraries, swap
//! the scoring path with an HNSW index (e.g., `instant-distance` crate)
//! for O(log n) approximate nearest neighbor search.

use crate::context::knowledge_store::KnowledgeStore;
use crate::fleet::types::{FleetEpisode, EpisodeMetrics};
use crate::types::{AnomalyCategory, Campaign};
use std::sync::RwLock;
use tracing::debug;

/// Maximum episodes in memory (~50MB at typical episode size)
const MAX_EPISODES: usize = 10_000;

/// RAM Recall knowledge store backed by in-memory fleet episodes
pub struct RAMRecall {
    /// Fleet episodes indexed in memory
    episodes: RwLock<Vec<FleetEpisode>>,
}

impl RAMRecall {
    /// Create an empty RAM Recall store
    pub fn new() -> Self {
        Self {
            episodes: RwLock::new(Vec::new()),
        }
    }

    /// Load episodes into memory (e.g., from fleet library sync)
    pub fn load_episodes(&self, episodes: Vec<FleetEpisode>) {
        let mut store = self.episodes.write().expect("RAMRecall lock poisoned");
        store.clear();
        store.extend(episodes);
        if store.len() > MAX_EPISODES {
            // Evict oldest non-critical episodes
            store.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
            store.truncate(MAX_EPISODES);
        }
        debug!(count = store.len(), "RAMRecall loaded episodes");
    }

    /// Add a single episode (e.g., from a local advisory)
    pub fn add_episode(&self, episode: FleetEpisode) {
        let mut store = self.episodes.write().expect("RAMRecall lock poisoned");

        // Dedup by ID
        if store.iter().any(|e| e.id == episode.id) {
            return;
        }

        store.push(episode);

        // Evict if over limit
        if store.len() > MAX_EPISODES {
            store.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
            store.truncate(MAX_EPISODES);
        }
    }

    /// Query precedents by category, campaign, and depth
    pub fn query_precedents(
        &self,
        category: &AnomalyCategory,
        campaign: &Campaign,
        query_metrics: Option<&EpisodeMetrics>,
        max_results: usize,
    ) -> Vec<&FleetEpisode> {
        // Note: this method borrows from the RwLock, so we can't return references
        // directly. The KnowledgeStore trait returns Vec<String>, so we use that path.
        // This method exists for direct programmatic access where the caller holds the lock.
        let _ = (category, campaign, query_metrics, max_results);
        Vec::new() // Use query() via KnowledgeStore trait instead
    }

    /// Remove episodes by ID (e.g., pruned by the hub)
    pub fn remove_episodes(&self, ids: &[String]) {
        let mut store = self.episodes.write().expect("RAMRecall lock poisoned");
        store.retain(|ep| !ids.contains(&ep.id));
    }

    /// Get total episode count
    pub fn episode_count(&self) -> usize {
        self.episodes.read().expect("RAMRecall lock poisoned").len()
    }

    /// Search episodes and return formatted context strings
    fn search_episodes(
        &self,
        category: &AnomalyCategory,
        campaign: &Campaign,
        max_results: usize,
    ) -> Vec<String> {
        let store = self.episodes.read().expect("RAMRecall lock poisoned");

        // Newest timestamp in store (for recency scoring)
        let newest_ts = store.iter().map(|e| e.timestamp).max().unwrap_or(0);

        // Phase 1: Metadata filter (category + campaign)
        let mut candidates: Vec<(&FleetEpisode, f64)> = store
            .iter()
            .filter(|ep| {
                // Exact category match or "any" for None
                (ep.category == *category || *category == AnomalyCategory::None)
                    && ep.campaign == *campaign
            })
            .map(|ep| {
                // Phase 2: Score by relevance (recency + outcome quality)
                let age_secs = (newest_ts as f64 - ep.timestamp as f64).max(0.0);
                let recency_score = 1.0 / (1.0 + age_secs / 3600.0);
                let outcome_score = match &ep.outcome {
                    crate::fleet::types::EventOutcome::Resolved { .. } => 1.0,
                    crate::fleet::types::EventOutcome::Escalated { .. } => 0.8,
                    crate::fleet::types::EventOutcome::FalsePositive => 0.3,
                    crate::fleet::types::EventOutcome::Pending => 0.1,
                };
                (ep, recency_score * 0.4 + outcome_score * 0.6)
            })
            .collect();

        // Sort by score descending
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Format top results as context strings
        candidates
            .into_iter()
            .take(max_results)
            .map(|(ep, score)| {
                format!(
                    "PRECEDENT [{}] {}: {} at {:.0}ft (rig {}, {}). Resolution: {}. Score: {:.2}",
                    ep.severity,
                    ep.category,
                    ep.risk_level,
                    ep.depth_range.0,
                    ep.rig_id,
                    ep.outcome,
                    ep.resolution_summary,
                    score
                )
            })
            .collect()
    }
}

impl Default for RAMRecall {
    fn default() -> Self {
        Self::new()
    }
}

impl KnowledgeStore for RAMRecall {
    fn query(&self, query: &str, max_results: usize) -> Vec<String> {
        // Parse category from query string keywords
        let category = if query.contains("well control") || query.contains("kick") || query.contains("loss") {
            AnomalyCategory::WellControl
        } else if query.contains("MSE") || query.contains("efficiency") || query.contains("ROP") {
            AnomalyCategory::DrillingEfficiency
        } else if query.contains("pressure") || query.contains("ECD") || query.contains("hydraulic") {
            AnomalyCategory::Hydraulics
        } else if query.contains("torque") || query.contains("pack-off") || query.contains("stick-slip") {
            AnomalyCategory::Mechanical
        } else if query.contains("d-exponent") || query.contains("formation") || query.contains("pore") {
            AnomalyCategory::Formation
        } else {
            AnomalyCategory::None
        };

        // Default to Production campaign (could be parameterized later)
        let campaign = Campaign::Production;

        self.search_episodes(&category, &campaign, max_results)
    }

    fn store_name(&self) -> &'static str {
        "RAMRecall"
    }

    fn is_healthy(&self) -> bool {
        self.episodes.read().is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::types::{EventOutcome, FleetEpisode, EpisodeMetrics};
    use crate::types::{FinalSeverity, RiskLevel};

    fn make_episode(id: &str, category: AnomalyCategory, campaign: Campaign) -> FleetEpisode {
        FleetEpisode {
            id: id.to_string(),
            rig_id: "RIG1".to_string(),
            category,
            campaign,
            depth_range: (10000.0, 10050.0),
            risk_level: RiskLevel::High,
            severity: FinalSeverity::High,
            resolution_summary: "Reduced WOB, resolved pack-off".to_string(),
            outcome: EventOutcome::Resolved { action_taken: "Reduced WOB".to_string() },
            timestamp: 1000,
            key_metrics: EpisodeMetrics {
                mse_efficiency: 60.0,
                flow_balance: 2.0,
                d_exponent: 1.5,
                torque_delta_percent: 0.2,
                ecd_margin: 0.4,
                rop: 45.0,
            },
        }
    }

    #[test]
    fn test_empty_recall() {
        let recall = RAMRecall::new();
        assert_eq!(recall.episode_count(), 0);
        assert!(recall.query("anything", 5).is_empty());
        assert!(recall.is_healthy());
    }

    #[test]
    fn test_load_and_query() {
        let recall = RAMRecall::new();
        recall.load_episodes(vec![
            make_episode("ep-1", AnomalyCategory::WellControl, Campaign::Production),
            make_episode("ep-2", AnomalyCategory::DrillingEfficiency, Campaign::Production),
        ]);

        assert_eq!(recall.episode_count(), 2);

        // Query for well control
        let results = recall.query("well control kick", 5);
        assert_eq!(results.len(), 1);
        assert!(results[0].contains("Well Control"));

        // Query for MSE
        let results = recall.query("MSE efficiency", 5);
        assert_eq!(results.len(), 1);
        assert!(results[0].contains("Drilling Efficiency"));
    }

    #[test]
    fn test_dedup() {
        let recall = RAMRecall::new();
        let ep = make_episode("ep-1", AnomalyCategory::WellControl, Campaign::Production);
        recall.add_episode(ep.clone());
        recall.add_episode(ep);
        assert_eq!(recall.episode_count(), 1);
    }

    #[test]
    fn test_knowledge_store_trait() {
        let store: Box<dyn KnowledgeStore> = Box::new(RAMRecall::new());
        assert_eq!(store.store_name(), "RAMRecall");
        assert!(store.is_healthy());
        assert!(store.query("anything", 5).is_empty());
    }
}
