//! Intelligence analysis workers
//!
//! Each worker takes a claimed `IntelligenceJob`, runs LLM inference via the
//! embedded mistralrs backend, and returns a `WorkerOutput`.
//!
//! ## Job types
//!
//! | Job type              | Description                                              |
//! |-----------------------|----------------------------------------------------------|
//! | `formation_benchmark` | Summarise optimal WOB/RPM/ROP for a formation across rigs|
//! | `anomaly_fingerprint` | Classify a recurring anomaly pattern into a fingerprint  |
//! | `post_well_report`    | Generate a comprehensive end-of-well intelligence report |
//! | `benchmark_gap`       | Identify where a rig underperforms vs fleet benchmarks   |
//!
//! Phase 2 ships the scaffolding and prompt skeletons. Full implementations
//! land in Phase 3.

use anyhow::{Context, Result};
use sqlx::PgPool;
use tracing::info;

use crate::hub::intelligence::job_queue::{IntelligenceJob, WorkerOutput, job_type};
use crate::hub::knowledge_graph::query as kg_query;
use crate::llm::MistralRsBackend;

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Attempt to extract a JSON object from a free-form LLM response string.
///
/// Searches for the first `{` and last `}` in the response and tries to parse
/// the substring as a `serde_json::Value`.  Returns `None` if no JSON is found
/// or if parsing fails — callers treat `None` as graceful degradation.
pub fn extract_json_from_llm_response(response: &str) -> Option<serde_json::Value> {
    let start = response.find('{')?;
    let end = response.rfind('}')?;
    if end < start {
        return None;
    }
    serde_json::from_str(&response[start..=end]).ok()
}

/// Derive a confidence score from data coverage (number of DB rows available).
///
/// More rows → higher confidence because the LLM had richer context to work
/// from.  Returns a value in [0.0, 0.9] — never 1.0 since LLM outputs always
/// carry irreducible uncertainty.
pub fn coverage_confidence(row_count: usize) -> f64 {
    match row_count {
        0 => 0.0,
        1..=2 => 0.4,
        3..=9 => 0.6,
        10..=29 => 0.75,
        _ => 0.9,
    }
}

/// Route an `IntelligenceJob` to the appropriate worker function.
pub async fn dispatch_job(
    backend: &MistralRsBackend,
    pool: &PgPool,
    job: &IntelligenceJob,
) -> Result<WorkerOutput> {
    info!(job_id = %job.id, job_type = %job.job_type, "Dispatching intelligence job");

    match job.job_type.as_str() {
        job_type::FORMATION_BENCHMARK => formation_benchmark(backend, pool, job).await,
        job_type::ANOMALY_FINGERPRINT => anomaly_fingerprint(backend, pool, job).await,
        job_type::POST_WELL_REPORT    => post_well_report(backend, pool, job).await,
        job_type::BENCHMARK_GAP       => benchmark_gap(backend, pool, job).await,
        other => anyhow::bail!("Unknown intelligence job type: {}", other),
    }
}

// ─── Worker: Formation Benchmark ──────────────────────────────────────────────
//
// Gathers fleet_performance rows for a formation, feeds them to the LLM, and
// produces a benchmark summary with recommended WOB/RPM/flow ranges.
//
// Input JSON shape:
//   { "formation_name": "Ekofisk", "field": "North Sea" }

async fn formation_benchmark(
    backend: &MistralRsBackend,
    pool: &PgPool,
    job: &IntelligenceJob,
) -> Result<WorkerOutput> {
    let formation = job.input_data["formation_name"]
        .as_str()
        .unwrap_or("Unknown");
    let field = job.input_data["field"].as_str().unwrap_or("Unknown");

    // Pull fleet performance data for this formation
    let rows: Vec<(String, String, serde_json::Value)> = sqlx::query_as(
        "SELECT rig_id, well_id, performance \
         FROM fleet_performance \
         WHERE formation_name = $1 \
         ORDER BY updated_at DESC \
         LIMIT 20",
    )
    .bind(formation)
    .fetch_all(pool)
    .await
    .context("Failed to fetch fleet performance for formation benchmark")?;

    if rows.is_empty() {
        return Ok(WorkerOutput {
            output_type: "benchmark".to_string(),
            content: format!(
                "No fleet performance data available for formation '{}' in field '{}'.",
                formation, field
            ),
            structured_data: None,
            confidence: Some(0.0),
            rig_id: None,
            well_id: None,
            formation_name: Some(formation.to_string()),
        });
    }

    let data_summary = rows
        .iter()
        .map(|(rig, well, perf)| {
            format!("Rig: {}, Well: {}, Metrics: {}", rig, well, perf)
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Enrich prompt with knowledge graph context (best-effort — failures are non-fatal)
    let graph_context = kg_query::formation_context(pool, formation, field)
        .await
        .ok()
        .flatten()
        .map(|c| c.to_prompt_string())
        .unwrap_or_default();

    let prompt = format!(
        "You are a drilling performance analyst for fleet-wide benchmarking.\n\
         \n\
         FORMATION: {} | FIELD: {}\n\
         {}\
         \n\
         FLEET PERFORMANCE DATA ({} wells):\n\
         {}\n\
         \n\
         Analyse this fleet data and produce a formation benchmark.\n\
         Output ONLY the following 5 lines:\n\
         FORMATION: [name]\n\
         WOB_RANGE: [min–max klbs]\n\
         RPM_RANGE: [min–max]\n\
         ROP_BENCHMARK: [expected ft/hr at optimal parameters]\n\
         SUMMARY: [2-3 sentence drilling recommendation for this formation]",
        formation,
        field,
        if graph_context.is_empty() {
            String::new()
        } else {
            format!("\n{}\n", graph_context)
        },
        rows.len(),
        data_summary
    );

    let response = backend
        .generate_with_params(&prompt, 250, 0.3)
        .await
        .context("LLM inference failed for formation_benchmark")?;

    info!(
        formation = %formation,
        rigs = rows.len(),
        "Formation benchmark complete"
    );

    Ok(WorkerOutput {
        output_type: "benchmark".to_string(),
        structured_data: extract_json_from_llm_response(&response),
        confidence: Some(coverage_confidence(rows.len())),
        content: response,
        rig_id: None,
        well_id: None,
        formation_name: Some(formation.to_string()),
    })
}

// ─── Worker: Anomaly Fingerprint ──────────────────────────────────────────────
//
// Analyses a cluster of similar events across rigs and produces a named
// anomaly fingerprint with root-cause hypothesis and recommended action.
//
// Input JSON shape:
//   { "category": "Hydraulics", "event_ids": ["uuid1", "uuid2", ...] }

async fn anomaly_fingerprint(
    backend: &MistralRsBackend,
    pool: &PgPool,
    job: &IntelligenceJob,
) -> Result<WorkerOutput> {
    let category = job.input_data["category"].as_str().unwrap_or("Unknown");

    let event_ids: Vec<String> = job.input_data["event_ids"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    if event_ids.is_empty() {
        anyhow::bail!("anomaly_fingerprint job has no event_ids");
    }

    // Fetch the events — use ANY($1) for the list
    let rows: Vec<(String, String, serde_json::Value)> = sqlx::query_as(
        "SELECT id, category, payload \
         FROM events \
         WHERE id = ANY($1) \
         LIMIT 30",
    )
    .bind(&event_ids)
    .fetch_all(pool)
    .await
    .context("Failed to fetch events for anomaly fingerprint")?;

    let event_summary = rows
        .iter()
        .map(|(id, cat, payload)| format!("ID: {}, Category: {}, Data: {}", id, cat, payload))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "You are a drilling anomaly analyst identifying patterns across multiple rigs.\n\
         \n\
         ANOMALY CATEGORY: {}\n\
         EVENTS ANALYSED: {}\n\
         \n\
         EVENT DATA:\n\
         {}\n\
         \n\
         Identify the common anomaly pattern. Output ONLY:\n\
         FINGERPRINT_NAME: [short name for this anomaly pattern]\n\
         ROOT_CAUSE: [most likely physical root cause]\n\
         COMMON_PARAMETERS: [drilling parameters consistently outside range]\n\
         RECOMMENDED_ACTION: [fleet-wide prevention recommendation]\n\
         CONFIDENCE: [0-100]%",
        category,
        rows.len(),
        event_summary
    );

    let response = backend
        .generate_with_params(&prompt, 300, 0.4)
        .await
        .context("LLM inference failed for anomaly_fingerprint")?;

    info!(
        category = %category,
        events = rows.len(),
        "Anomaly fingerprint complete"
    );

    Ok(WorkerOutput {
        output_type: "fingerprint".to_string(),
        structured_data: extract_json_from_llm_response(&response),
        confidence: Some(coverage_confidence(rows.len())),
        content: response,
        rig_id: None,
        well_id: None,
        formation_name: None,
    })
}

// ─── Worker: Post-Well Report ──────────────────────────────────────────────────
//
// Generates a comprehensive intelligence report for a completed well,
// summarising performance, anomalies, and lessons learned.
//
// Input JSON shape:
//   { "well_id": "15/9-19A", "rig_id": "RIG_01" }

async fn post_well_report(
    backend: &MistralRsBackend,
    pool: &PgPool,
    job: &IntelligenceJob,
) -> Result<WorkerOutput> {
    let well_id = job.input_data["well_id"].as_str().unwrap_or("Unknown");
    let rig_id = job.input_data["rig_id"].as_str().unwrap_or("Unknown");

    // Fetch all episodes for this well
    let episodes: Vec<(String, String, f64, String, serde_json::Value)> = sqlx::query_as(
        "SELECT category, risk_level, score, outcome, key_metrics \
         FROM episodes \
         WHERE rig_id = $1 \
         ORDER BY timestamp ASC",
    )
    .bind(rig_id)
    .fetch_all(pool)
    .await
    .context("Failed to fetch episodes for post-well report")?;

    let episode_summary = episodes
        .iter()
        .map(|(cat, risk, score, outcome, metrics)| {
            format!(
                "Category: {}, Risk: {}, Score: {:.2}, Outcome: {}, Metrics: {}",
                cat, risk, score, outcome, metrics
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "You are a drilling intelligence analyst preparing a post-well report.\n\
         \n\
         WELL: {} | RIG: {}\n\
         TOTAL ADVISORY EPISODES: {}\n\
         \n\
         EPISODE HISTORY:\n\
         {}\n\
         \n\
         Write a post-well intelligence report. Output ONLY:\n\
         WELL_SUMMARY: [2-3 sentences on overall well performance]\n\
         KEY_ACHIEVEMENTS: [top 2 positive outcomes]\n\
         KEY_LEARNINGS: [top 3 lessons for future wells]\n\
         FORMATION_INSIGHTS: [notable formation behaviour and response]\n\
         RECOMMENDATIONS: [top 3 recommendations for the next well in this field]",
        well_id,
        rig_id,
        episodes.len(),
        episode_summary
    );

    let response = backend
        .generate_with_params(&prompt, 400, 0.4)
        .await
        .context("LLM inference failed for post_well_report")?;

    info!(
        well_id = %well_id,
        rig_id = %rig_id,
        episodes = episodes.len(),
        "Post-well report complete"
    );

    Ok(WorkerOutput {
        output_type: "report".to_string(),
        structured_data: extract_json_from_llm_response(&response),
        confidence: Some(coverage_confidence(episodes.len())),
        content: response,
        rig_id: Some(rig_id.to_string()),
        well_id: Some(well_id.to_string()),
        formation_name: None,
    })
}

// ─── Worker: Benchmark Gap ────────────────────────────────────────────────────
//
// Compares a specific rig's performance against fleet benchmarks for the same
// formation and identifies where it falls behind.
//
// Input JSON shape:
//   { "rig_id": "RIG_01", "formation_name": "Ekofisk" }

async fn benchmark_gap(
    backend: &MistralRsBackend,
    pool: &PgPool,
    job: &IntelligenceJob,
) -> Result<WorkerOutput> {
    let rig_id = job.input_data["rig_id"].as_str().unwrap_or("Unknown");
    let formation = job.input_data["formation_name"].as_str().unwrap_or("Unknown");

    // Rig's own performance for this formation
    let rig_perf: Option<(serde_json::Value,)> = sqlx::query_as(
        "SELECT performance \
         FROM fleet_performance \
         WHERE rig_id = $1 AND formation_name = $2 \
         ORDER BY updated_at DESC LIMIT 1",
    )
    .bind(rig_id)
    .bind(formation)
    .fetch_optional(pool)
    .await
    .context("Failed to fetch rig performance")?;

    // Fleet average for this formation (excluding the target rig)
    let fleet_perf: Vec<(String, serde_json::Value)> = sqlx::query_as(
        "SELECT rig_id, performance \
         FROM fleet_performance \
         WHERE formation_name = $1 AND rig_id != $2 \
         LIMIT 10",
    )
    .bind(formation)
    .bind(rig_id)
    .fetch_all(pool)
    .await
    .context("Failed to fetch fleet performance")?;

    let rig_data = rig_perf
        .map(|(v,)| v.to_string())
        .unwrap_or_else(|| "No data available".to_string());

    let fleet_data = fleet_perf
        .iter()
        .map(|(rid, perf)| format!("Rig {}: {}", rid, perf))
        .collect::<Vec<_>>()
        .join("\n");

    // Enrich prompt with knowledge graph context for both formation and rig
    let formation_graph = kg_query::formation_context(pool, formation, "")
        .await
        .ok()
        .flatten()
        .map(|c| c.to_prompt_string())
        .unwrap_or_default();

    let rig_graph = kg_query::rig_context(pool, rig_id)
        .await
        .ok()
        .flatten()
        .map(|c| c.to_prompt_string())
        .unwrap_or_default();

    let graph_block = [formation_graph, rig_graph]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "You are a drilling performance analyst identifying improvement opportunities.\n\
         \n\
         FORMATION: {} | TARGET RIG: {}\n\
         {}\
         \n\
         TARGET RIG PERFORMANCE:\n\
         {}\n\
         \n\
         FLEET BENCHMARK ({} other rigs):\n\
         {}\n\
         \n\
         Identify performance gaps and recommend improvements. Output ONLY:\n\
         PERFORMANCE_GAP_SUMMARY: [where target rig underperforms vs fleet]\n\
         PRIMARY_GAP: [single most impactful gap with magnitude]\n\
         ROOT_CAUSE_HYPOTHESIS: [likely cause of the gap]\n\
         RECOMMENDED_ACTIONS: [top 3 specific parameter adjustments]\n\
         EXPECTED_IMPROVEMENT: [estimated ROP/efficiency gain if gaps are closed]",
        formation,
        rig_id,
        if graph_block.is_empty() {
            String::new()
        } else {
            format!("\n{}\n", graph_block)
        },
        rig_data,
        fleet_perf.len(),
        fleet_data
    );

    let response = backend
        .generate_with_params(&prompt, 350, 0.35)
        .await
        .context("LLM inference failed for benchmark_gap")?;

    info!(
        rig_id = %rig_id,
        formation = %formation,
        "Benchmark gap analysis complete"
    );

    Ok(WorkerOutput {
        output_type: "advisory".to_string(),
        structured_data: extract_json_from_llm_response(&response),
        confidence: Some(coverage_confidence(fleet_perf.len())),
        content: response,
        rig_id: Some(rig_id.to_string()),
        well_id: None,
        formation_name: Some(formation.to_string()),
    })
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── coverage_confidence ────────────────────────────────────────────────────

    #[test]
    fn coverage_confidence_zero_rows_is_zero() {
        assert_eq!(coverage_confidence(0), 0.0);
    }

    #[test]
    fn coverage_confidence_one_row_is_low() {
        assert_eq!(coverage_confidence(1), 0.4);
        assert_eq!(coverage_confidence(2), 0.4);
    }

    #[test]
    fn coverage_confidence_small_dataset() {
        assert_eq!(coverage_confidence(3), 0.6);
        assert_eq!(coverage_confidence(9), 0.6);
    }

    #[test]
    fn coverage_confidence_medium_dataset() {
        assert_eq!(coverage_confidence(10), 0.75);
        assert_eq!(coverage_confidence(29), 0.75);
    }

    #[test]
    fn coverage_confidence_large_dataset() {
        assert_eq!(coverage_confidence(30), 0.9);
        assert_eq!(coverage_confidence(1000), 0.9);
    }

    // ── extract_json_from_llm_response ────────────────────────────────────────

    #[test]
    fn extract_json_finds_embedded_object() {
        let response = r#"Analysis complete. {"key": "value", "score": 42}"#;
        let result = extract_json_from_llm_response(response);
        assert!(result.is_some(), "Should extract JSON from response");
        let val = result.unwrap();
        assert_eq!(val["key"], "value");
        assert_eq!(val["score"], 42);
    }

    #[test]
    fn extract_json_returns_none_for_plain_text() {
        let response = "No JSON here, just plain text analysis.";
        assert!(extract_json_from_llm_response(response).is_none());
    }

    #[test]
    fn extract_json_returns_none_for_empty_string() {
        assert!(extract_json_from_llm_response("").is_none());
    }

    #[test]
    fn extract_json_handles_nested_object() {
        let response = r#"FORMATION: Ekofisk
SUMMARY: Good formation. {"wob_range": "15-25", "nested": {"rpm": 120}}"#;
        let result = extract_json_from_llm_response(response);
        assert!(result.is_some());
        let val = result.unwrap();
        assert_eq!(val["wob_range"], "15-25");
    }

    #[test]
    fn extract_json_handles_malformed_json() {
        let response = "partial {key: value without quotes}";
        // Malformed JSON — should return None gracefully
        let result = extract_json_from_llm_response(response);
        assert!(result.is_none(), "Malformed JSON should return None");
    }

    #[test]
    fn coverage_confidence_monotonically_increases() {
        let counts = [0, 1, 3, 10, 30];
        let confidences: Vec<f64> = counts.iter().map(|&n| coverage_confidence(n)).collect();
        for window in confidences.windows(2) {
            assert!(
                window[1] >= window[0],
                "confidence should be non-decreasing: {:?}",
                confidences
            );
        }
    }
}
