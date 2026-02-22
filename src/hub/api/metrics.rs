//! Hub Prometheus metrics endpoint (item 4.1)
//!
//! Returns a small set of hub-side counters in Prometheus text format
//! (version 0.0.4) so external monitoring tools (Prometheus, Grafana) can
//! scrape hub health without parsing JSON dashboards.
//!
//! Exposed metrics:
//! - `hub_intelligence_jobs_pending`   — jobs queued but not yet claimed
//! - `hub_intelligence_jobs_in_flight` — jobs currently being processed
//! - `hub_intelligence_jobs_completed` — cumulative completed jobs
//! - `hub_intelligence_jobs_failed`    — cumulative failed jobs
//! - `hub_registered_rigs_total`       — total rigs in the registry
//! - `hub_active_rigs_total`           — active rigs seen in last 48 hours

use crate::hub::HubState;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use std::sync::Arc;

/// GET /api/fleet/metrics
///
/// No authentication required — metrics are not sensitive and must be
/// scraped by Prometheus without per-request credentials.
pub async fn get_metrics(State(hub): State<Arc<HubState>>) -> impl IntoResponse {
    // Job counts by status
    let counts: Vec<(String, i64)> = sqlx::query_as(
        "SELECT status, COUNT(*) FROM intelligence_jobs GROUP BY status",
    )
    .fetch_all(&hub.db)
    .await
    .unwrap_or_default();

    let mut pending: i64 = 0;
    let mut in_flight: i64 = 0;
    let mut completed: i64 = 0;
    let mut failed: i64 = 0;

    for (status, count) in counts {
        match status.as_str() {
            "pending"   => pending   = count,
            "in_flight" => in_flight = count,
            "completed" => completed = count,
            "failed"    => failed    = count,
            _ => {}
        }
    }

    // Rig counts
    let total_rigs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM rigs")
        .fetch_one(&hub.db)
        .await
        .unwrap_or(0);

    let active_rigs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM rigs WHERE status = 'active' AND last_seen > NOW() - INTERVAL '48 hours'",
    )
    .fetch_one(&hub.db)
    .await
    .unwrap_or(0);

    let mut body = String::with_capacity(1024);

    body.push_str("# HELP hub_intelligence_jobs_pending Jobs queued but not yet claimed\n");
    body.push_str("# TYPE hub_intelligence_jobs_pending gauge\n");
    body.push_str(&format!("hub_intelligence_jobs_pending {pending}\n"));

    body.push_str("# HELP hub_intelligence_jobs_in_flight Jobs currently being processed\n");
    body.push_str("# TYPE hub_intelligence_jobs_in_flight gauge\n");
    body.push_str(&format!("hub_intelligence_jobs_in_flight {in_flight}\n"));

    body.push_str("# HELP hub_intelligence_jobs_completed_total Cumulative completed jobs\n");
    body.push_str("# TYPE hub_intelligence_jobs_completed_total counter\n");
    body.push_str(&format!("hub_intelligence_jobs_completed_total {completed}\n"));

    body.push_str("# HELP hub_intelligence_jobs_failed_total Cumulative failed jobs\n");
    body.push_str("# TYPE hub_intelligence_jobs_failed_total counter\n");
    body.push_str(&format!("hub_intelligence_jobs_failed_total {failed}\n"));

    body.push_str("# HELP hub_registered_rigs_total Total rigs in the registry\n");
    body.push_str("# TYPE hub_registered_rigs_total gauge\n");
    body.push_str(&format!("hub_registered_rigs_total {total_rigs}\n"));

    body.push_str("# HELP hub_active_rigs_total Active rigs seen in last 48 hours\n");
    body.push_str("# TYPE hub_active_rigs_total gauge\n");
    body.push_str(&format!("hub_active_rigs_total {active_rigs}\n"));

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        body,
    )
}
