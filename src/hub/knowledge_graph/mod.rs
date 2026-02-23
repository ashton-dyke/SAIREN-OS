//! PostgreSQL-backed knowledge graph for fleet-wide GraphRAG intelligence.
//!
//! Stores drilling entities (formations, rigs, wells) as nodes and their
//! relationships as typed, weighted edges.  Intelligence workers query the
//! graph to enrich LLM prompts with multi-hop relational context before
//! inference — this is the "Retrieval" step in GraphRAG.
//!
//! ## Node types
//!
//! | Type        | Label format              | Description                   |
//! |-------------|---------------------------|-------------------------------|
//! | `formation` | `{field}::{name}`         | Geological formation          |
//! | `rig`       | `{rig_id}`                | Drilling rig                  |
//! | `well`      | `{rig_id}::{well_id}`     | Individual well bore          |
//!
//! ## Edge types
//!
//! | Type         | Direction          | Weight meaning              |
//! |--------------|--------------------|------------------------------|
//! | `DRILLED_IN` | rig → formation    | avg ROP normalised 0–1       |
//! | `DRILLED_BY` | well → rig         | always 1.0                  |

pub mod builder;
pub mod query;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

// ─── Node type constants ──────────────────────────────────────────────────────

pub mod node_type {
    pub const FORMATION: &str = "formation";
    pub const RIG:       &str = "rig";
    pub const WELL:      &str = "well";
}

// ─── Edge type constants ──────────────────────────────────────────────────────

pub mod edge_type {
    pub const DRILLED_IN: &str = "DRILLED_IN";
    pub const DRILLED_BY: &str = "DRILLED_BY";
}

// ─── Domain types ─────────────────────────────────────────────────────────────

/// A node in the knowledge graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KgNode {
    pub id: String,
    pub node_type: String,
    pub label: String,
    pub properties: serde_json::Value,
}

/// An edge in the knowledge graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KgEdge {
    pub id: String,
    pub from_node: String,
    pub to_node: String,
    pub edge_type: String,
    pub weight: f64,
}

/// Graph statistics for monitoring and dashboard display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStats {
    pub total_nodes: i64,
    pub total_edges: i64,
    pub nodes_by_type: Vec<NodeTypeCount>,
    pub edges_by_type: Vec<EdgeTypeCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeTypeCount {
    pub node_type: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeTypeCount {
    pub edge_type: String,
    pub count: i64,
}

// ─── Core graph operations ────────────────────────────────────────────────────

/// Upsert a node by (node_type, label).
///
/// Returns the canonical node ID (existing on conflict, new on insert).
/// On conflict the properties are replaced with the supplied value so that
/// a rebuild always reflects the latest data.
///
/// Accepts any sqlx executor — pass `&pool` for ad-hoc use or `&mut *tx`
/// inside a transaction.
pub async fn upsert_node<'a>(
    executor: impl sqlx::PgExecutor<'a>,
    node_type: &str,
    label: &str,
    properties: serde_json::Value,
) -> Result<String> {
    let new_id = Uuid::new_v4().to_string();

    let (id,): (String,) = sqlx::query_as(
        r#"INSERT INTO kg_nodes (id, node_type, label, properties)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (node_type, label)
           DO UPDATE SET properties = $4, updated_at = NOW()
           RETURNING id"#,
    )
    .bind(&new_id)
    .bind(node_type)
    .bind(label)
    .bind(&properties)
    .fetch_one(executor)
    .await?;

    Ok(id)
}

/// Upsert a directed edge between two nodes.
///
/// On conflict (same from/to/type) the weight and properties are updated.
/// Accepts any sqlx executor — pass `&pool` for ad-hoc use or `&mut *tx`
/// inside a transaction.
pub async fn upsert_edge<'a>(
    executor: impl sqlx::PgExecutor<'a>,
    from_node: &str,
    to_node: &str,
    edge_type: &str,
    weight: f64,
    properties: serde_json::Value,
) -> Result<()> {
    sqlx::query(
        r#"INSERT INTO kg_edges (id, from_node, to_node, edge_type, weight, properties)
           VALUES ($1, $2, $3, $4, $5, $6)
           ON CONFLICT (from_node, to_node, edge_type)
           DO UPDATE SET weight = $5, properties = $6"#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(from_node)
    .bind(to_node)
    .bind(edge_type)
    .bind(weight)
    .bind(&properties)
    .execute(executor)
    .await?;

    Ok(())
}

/// Return aggregate statistics about the knowledge graph.
pub async fn get_stats(pool: &PgPool) -> Result<GraphStats> {
    let (total_nodes,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM kg_nodes")
        .fetch_one(pool)
        .await?;

    let (total_edges,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM kg_edges")
        .fetch_one(pool)
        .await?;

    let node_rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT node_type, COUNT(*) FROM kg_nodes GROUP BY node_type ORDER BY COUNT(*) DESC",
    )
    .fetch_all(pool)
    .await?;

    let edge_rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT edge_type, COUNT(*) FROM kg_edges GROUP BY edge_type ORDER BY COUNT(*) DESC",
    )
    .fetch_all(pool)
    .await?;

    Ok(GraphStats {
        total_nodes,
        total_edges,
        nodes_by_type: node_rows
            .into_iter()
            .map(|(node_type, count)| NodeTypeCount { node_type, count })
            .collect(),
        edges_by_type: edge_rows
            .into_iter()
            .map(|(edge_type, count)| EdgeTypeCount { edge_type, count })
            .collect(),
    })
}
