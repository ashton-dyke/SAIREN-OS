//! Dashboard API handlers and static file serving

use crate::hub::HubState;
use crate::hub::auth::api_key::AdminAuth;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Serialize)]
pub struct DashboardSummary {
    pub active_rigs: i64,
    pub total_rigs: i64,
    pub events_today: i64,
    pub total_events: i64,
    pub library_version: i64,
    pub total_episodes: i64,
    pub top_categories: Vec<TopCategory>,
}

#[derive(Serialize)]
pub struct TopCategory {
    pub category: String,
    pub count: i64,
}

#[derive(Serialize)]
pub struct TrendPoint {
    pub date: String,
    pub category: String,
    pub count: i64,
}

#[derive(Deserialize)]
pub struct TrendParams {
    pub days: Option<i32>,
}

#[derive(Serialize)]
pub struct OutcomeAnalytics {
    pub total_events: i64,
    pub resolved_count: i64,
    pub escalated_count: i64,
    pub false_positive_count: i64,
    pub pending_count: i64,
    pub resolution_rate: f64,
    pub by_category: Vec<CategoryOutcome>,
}

#[derive(Serialize)]
pub struct CategoryOutcome {
    pub category: String,
    pub total: i64,
    pub resolved: i64,
    pub resolution_rate: f64,
}

/// GET /api/fleet/dashboard/summary
pub async fn get_summary(
    State(hub): State<Arc<HubState>>,
    _admin: AdminAuth,
) -> Json<DashboardSummary> {
    let active_rigs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM rigs WHERE status = 'active' AND last_seen > NOW() - INTERVAL '48 hours'",
    )
    .fetch_one(&hub.db)
    .await
    .unwrap_or(0);

    let total_rigs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM rigs")
        .fetch_one(&hub.db)
        .await
        .unwrap_or(0);

    let events_today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM events WHERE created_at >= CURRENT_DATE",
    )
    .fetch_one(&hub.db)
    .await
    .unwrap_or(0);

    let total_events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
        .fetch_one(&hub.db)
        .await
        .unwrap_or(0);

    let library_version: i64 =
        sqlx::query_scalar("SELECT last_value FROM library_version_seq")
            .fetch_one(&hub.db)
            .await
            .unwrap_or(0);

    let total_episodes: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM episodes WHERE archived = FALSE")
            .fetch_one(&hub.db)
            .await
            .unwrap_or(0);

    let top_cats: Vec<(String, i64)> = sqlx::query_as(
        "SELECT category, COUNT(*) as cnt FROM events GROUP BY category ORDER BY cnt DESC LIMIT 5",
    )
    .fetch_all(&hub.db)
    .await
    .unwrap_or_default();

    Json(DashboardSummary {
        active_rigs,
        total_rigs,
        events_today,
        total_events,
        library_version,
        total_episodes,
        top_categories: top_cats
            .into_iter()
            .map(|(category, count)| TopCategory { category, count })
            .collect(),
    })
}

/// GET /api/fleet/dashboard/trends
pub async fn get_trends(
    State(hub): State<Arc<HubState>>,
    _admin: AdminAuth,
    Query(params): Query<TrendParams>,
) -> Json<Vec<TrendPoint>> {
    let days = params.days.unwrap_or(30);

    let rows: Vec<(chrono::NaiveDate, String, i64)> = sqlx::query_as(
        r#"SELECT DATE(timestamp) as day, category, COUNT(*)
           FROM events
           WHERE timestamp > NOW() - ($1 || ' days')::interval
           GROUP BY day, category
           ORDER BY day"#,
    )
    .bind(days.to_string())
    .fetch_all(&hub.db)
    .await
    .unwrap_or_default();

    Json(
        rows.into_iter()
            .map(|(date, category, count)| TrendPoint {
                date: date.to_string(),
                category,
                count,
            })
            .collect(),
    )
}

/// GET /api/fleet/dashboard/outcomes
pub async fn get_outcomes(
    State(hub): State<Arc<HubState>>,
    _admin: AdminAuth,
) -> Json<OutcomeAnalytics> {
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
        .fetch_one(&hub.db)
        .await
        .unwrap_or(0);

    let resolved: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE outcome LIKE 'RESOLVED%'")
            .fetch_one(&hub.db)
            .await
            .unwrap_or(0);

    let escalated: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE outcome LIKE 'ESCALATED%'")
            .fetch_one(&hub.db)
            .await
            .unwrap_or(0);

    let false_positive: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE outcome = 'FALSE_POSITIVE'")
            .fetch_one(&hub.db)
            .await
            .unwrap_or(0);

    let pending: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE outcome = 'PENDING'")
            .fetch_one(&hub.db)
            .await
            .unwrap_or(0);

    let resolution_rate = if total > 0 {
        (resolved + escalated) as f64 / total as f64
    } else {
        0.0
    };

    let cat_rows: Vec<(String, i64, i64)> = sqlx::query_as(
        r#"SELECT category, COUNT(*) as total,
              SUM(CASE WHEN outcome LIKE 'RESOLVED%' THEN 1 ELSE 0 END) as resolved
           FROM events GROUP BY category"#,
    )
    .fetch_all(&hub.db)
    .await
    .unwrap_or_default();

    Json(OutcomeAnalytics {
        total_events: total,
        resolved_count: resolved,
        escalated_count: escalated,
        false_positive_count: false_positive,
        pending_count: pending,
        resolution_rate,
        by_category: cat_rows
            .into_iter()
            .map(|(category, total, resolved)| CategoryOutcome {
                category,
                total,
                resolved,
                resolution_rate: if total > 0 {
                    resolved as f64 / total as f64
                } else {
                    0.0
                },
            })
            .collect(),
    })
}

/// Serve the fleet dashboard HTML
pub async fn serve_dashboard() -> Response {
    Html(include_str!("../../../static/fleet_dashboard.html")).into_response()
}
