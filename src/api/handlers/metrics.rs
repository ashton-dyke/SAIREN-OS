//! Observability: Prometheus metrics and fleet intelligence

use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::Json;

use super::DashboardState;

// ============================================================================
// Prometheus Metrics Endpoint (Item 4.1)
// ============================================================================

/// GET /api/v1/metrics
///
/// Returns runtime counters in Prometheus text format (version 0.0.4).
/// No external crate required — gauges and counters are hand-formatted from
/// `AppState` fields that are already maintained by the processing loop.
///
/// Exposed metrics:
/// - `sairen_packets_total`       — cumulative WITS packets processed
/// - `sairen_tickets_created_total` — advisory tickets generated
/// - `sairen_tickets_verified_total` — tickets confirmed by strategic agent
/// - `sairen_tickets_rejected_total` — tickets rejected as transient
/// - `sairen_uptime_seconds`       — process uptime in seconds
/// - `sairen_avg_mse_efficiency`   — current rolling MSE efficiency (gauge)
pub async fn get_metrics(State(state): State<DashboardState>) -> impl IntoResponse {
    let app_state = state.app_state.read().await;

    let mut body = String::with_capacity(1024);

    body.push_str("# HELP sairen_packets_total Total WITS packets processed\n");
    body.push_str("# TYPE sairen_packets_total counter\n");
    body.push_str(&format!("sairen_packets_total {}\n", app_state.packets_processed));

    body.push_str("# HELP sairen_tickets_created_total Advisory tickets generated\n");
    body.push_str("# TYPE sairen_tickets_created_total counter\n");
    body.push_str(&format!("sairen_tickets_created_total {}\n", app_state.tickets_created));

    body.push_str("# HELP sairen_tickets_verified_total Tickets confirmed by strategic agent\n");
    body.push_str("# TYPE sairen_tickets_verified_total counter\n");
    body.push_str(&format!("sairen_tickets_verified_total {}\n", app_state.tickets_verified));

    body.push_str("# HELP sairen_tickets_rejected_total Tickets rejected as transient\n");
    body.push_str("# TYPE sairen_tickets_rejected_total counter\n");
    body.push_str(&format!("sairen_tickets_rejected_total {}\n", app_state.tickets_rejected));

    body.push_str("# HELP sairen_uptime_seconds Process uptime in seconds\n");
    body.push_str("# TYPE sairen_uptime_seconds gauge\n");
    body.push_str(&format!("sairen_uptime_seconds {}\n", app_state.uptime_secs()));

    if let Some(eff) = app_state.avg_mse_efficiency {
        body.push_str("# HELP sairen_avg_mse_efficiency Rolling average MSE efficiency (0-100)\n");
        body.push_str("# TYPE sairen_avg_mse_efficiency gauge\n");
        body.push_str(&format!("sairen_avg_mse_efficiency {eff:.2}\n"));
    }

    (
        axum::http::StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        body,
    )
}

// ─── Fleet Intelligence ───────────────────────────────────────────────────────

/// GET /api/v1/fleet/intelligence
///
/// Returns locally cached hub intelligence outputs.  The cache is populated by
/// `run_intelligence_sync` from `fleet/sync.rs` and written to
/// `./data/fleet_intelligence.json`.
///
/// Query params:
/// - `?type=benchmark` — filter by output_type
/// - `?formation=Ekofisk` — filter by formation name

pub async fn get_fleet_intelligence(
    Query(params): Query<FleetIntelligenceQuery>,
) -> Json<Vec<crate::fleet::types::IntelligenceOutput>> {
    let path = std::path::Path::new("./data/fleet_intelligence.json");

    let outputs: Vec<crate::fleet::types::IntelligenceOutput> = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let filtered: Vec<_> = outputs
        .into_iter()
        .filter(|o| {
            if let Some(ref t) = params.r#type {
                if o.output_type != *t {
                    return false;
                }
            }
            if let Some(ref f) = params.formation {
                if o.formation_name.as_deref() != Some(f.as_str()) {
                    return false;
                }
            }
            true
        })
        .collect();

    Json(filtered)
}


#[derive(serde::Deserialize)]
pub struct FleetIntelligenceQuery {
    /// Filter by output_type: `benchmark`, `fingerprint`, `report`, `advisory`
    pub r#type: Option<String>,
    /// Filter by formation name
    pub formation: Option<String>,
}
