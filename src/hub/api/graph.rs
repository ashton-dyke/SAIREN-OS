//! Knowledge graph API endpoints
//!
//! | Method | Path                          | Auth  | Description                      |
//! |--------|-------------------------------|-------|----------------------------------|
//! | GET    | /api/fleet/graph/stats        | Admin | Node/edge counts by type         |
//! | GET    | /api/fleet/graph/formation    | Admin | Formation context (graph + events)|
//! | POST   | /api/fleet/graph/rebuild      | Admin | Trigger a full graph rebuild     |

use crate::hub::HubState;
use crate::hub::auth::api_key::AdminAuth;
use crate::hub::knowledge_graph::{builder, get_stats, query, GraphStats};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

// ─── GET /api/fleet/graph/stats ───────────────────────────────────────────────

/// GET /api/fleet/graph/stats — Return node and edge counts by type.
pub async fn graph_stats(
    State(hub): State<Arc<HubState>>,
    _admin: AdminAuth,
) -> Result<Json<GraphStats>, StatusCode> {
    get_stats(&hub.db)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

// ─── GET /api/fleet/graph/formation ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct FormationQuery {
    pub name: String,
    pub field: String,
}

#[derive(Serialize)]
pub struct FormationContextResponse {
    pub found: bool,
    pub formation_name: String,
    pub field: String,
    pub rig_count: usize,
    pub rigs: Vec<RigEntry>,
    pub avg_rop_ft_hr: f64,
    pub top_anomaly_categories: Vec<String>,
    /// Pre-formatted prompt block — ready to paste into an LLM prompt
    pub prompt_context: String,
}

#[derive(Serialize)]
pub struct RigEntry {
    pub rig_id: String,
    pub avg_rop_ft_hr: f64,
}

/// GET /api/fleet/graph/formation?name=Ekofisk&field=NorthSea
pub async fn formation_context(
    State(hub): State<Arc<HubState>>,
    _admin: AdminAuth,
    Query(params): Query<FormationQuery>,
) -> Result<Json<FormationContextResponse>, StatusCode> {
    let ctx = query::formation_context(&hub.db, &params.name, &params.field)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match ctx {
        None => Ok(Json(FormationContextResponse {
            found: false,
            formation_name: params.name,
            field: params.field,
            rig_count: 0,
            rigs: vec![],
            avg_rop_ft_hr: 0.0,
            top_anomaly_categories: vec![],
            prompt_context: String::new(),
        })),
        Some(c) => {
            let prompt_context = c.to_prompt_string();
            let rig_count = c.rigs.len();
            let rigs = c
                .rigs
                .iter()
                .map(|r| RigEntry {
                    rig_id: r.rig_id.clone(),
                    avg_rop_ft_hr: r.avg_rop_ft_hr,
                })
                .collect();

            Ok(Json(FormationContextResponse {
                found: true,
                formation_name: c.formation_name,
                field: c.field,
                rig_count,
                rigs,
                avg_rop_ft_hr: c.avg_rop_ft_hr,
                top_anomaly_categories: c.top_anomaly_categories,
                prompt_context,
            }))
        }
    }
}

// ─── POST /api/fleet/graph/rebuild ───────────────────────────────────────────

#[derive(Serialize)]
pub struct RebuildResponse {
    pub status: String,
}

/// POST /api/fleet/graph/rebuild — Trigger a full graph rebuild synchronously.
pub async fn rebuild_graph(
    State(hub): State<Arc<HubState>>,
    _admin: AdminAuth,
) -> Result<Json<RebuildResponse>, StatusCode> {
    info!("Manual knowledge graph rebuild triggered via API");

    builder::rebuild_graph(&hub.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(RebuildResponse {
        status: "rebuilt".to_string(),
    }))
}
