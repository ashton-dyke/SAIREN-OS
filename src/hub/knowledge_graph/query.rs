//! Multi-hop graph queries for intelligence worker context enrichment.
//!
//! These functions provide the **Retrieval** part of GraphRAG: given a
//! formation or rig identifier, walk the graph to collect structured context
//! that can be serialised into an LLM prompt, giving the model relational
//! knowledge it would not otherwise have.

use anyhow::Result;
use serde::Serialize;
use sqlx::PgPool;

// ─── Returned context types ───────────────────────────────────────────────────

/// Enriched formation context assembled from graph traversal + direct queries.
///
/// Serialise with `to_prompt_string()` to get a compact block suitable for
/// injection into an LLM prompt.
#[derive(Debug, Serialize)]
pub struct FormationContext {
    pub formation_name: String,
    pub field: String,
    /// Rigs that have drilled this formation, ordered by performance
    pub rigs: Vec<RigSummary>,
    /// Fleet average ROP across all rigs (0 when unknown)
    pub avg_rop_ft_hr: f64,
    /// Top anomaly categories reported in this field
    pub top_anomaly_categories: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct RigSummary {
    pub rig_id: String,
    /// Average ROP de-normalised from graph edge weight
    pub avg_rop_ft_hr: f64,
}

/// Rig-centric context: formations drilled by a specific rig.
#[derive(Debug, Serialize)]
pub struct RigContext {
    pub rig_id: String,
    pub formations: Vec<FormationSummary>,
}

#[derive(Debug, Serialize)]
pub struct FormationSummary {
    pub formation_name: String,
    pub field: String,
    pub avg_rop_ft_hr: f64,
}

// ─── Queries ─────────────────────────────────────────────────────────────────

/// Build enriched context for a formation.
///
/// Traverses:
/// - Inbound `DRILLED_IN` edges → which rigs drilled this formation + performance
/// - `events` table directly → top anomaly categories seen in this field
///
/// Returns `None` when the formation node does not yet exist in the graph.
pub async fn formation_context(
    pool: &PgPool,
    formation_name: &str,
    field: &str,
) -> Result<Option<FormationContext>> {
    let label = format!("{}::{}", field, formation_name);

    // ── Find formation node ───────────────────────────────────────────────────
    let node: Option<(String, serde_json::Value)> = sqlx::query_as(
        "SELECT id, properties FROM kg_nodes WHERE node_type = 'formation' AND label = $1",
    )
    .bind(&label)
    .fetch_optional(pool)
    .await?;

    let (formation_id, props) = match node {
        Some(n) => n,
        None => return Ok(None),
    };

    let avg_rop_ft_hr = props["avg_rop_ft_hr"].as_f64().unwrap_or(0.0);

    // ── Rigs via inbound DRILLED_IN edges ─────────────────────────────────────
    let rig_rows: Vec<(String, f64)> = sqlx::query_as(
        r#"SELECT n.label, e.weight
           FROM kg_edges e
           JOIN kg_nodes n ON n.id = e.from_node
           WHERE e.to_node    = $1
             AND e.edge_type  = 'DRILLED_IN'
             AND n.node_type  = 'rig'
           ORDER BY e.weight DESC
           LIMIT 15"#,
    )
    .bind(&formation_id)
    .fetch_all(pool)
    .await?;

    // ── Top anomaly categories in this field (direct query) ───────────────────
    let anomaly_rows: Vec<(String,)> = sqlx::query_as(
        r#"SELECT category
           FROM events
           WHERE field = $1
             AND category IS NOT NULL
           GROUP BY category
           ORDER BY COUNT(*) DESC
           LIMIT 5"#,
    )
    .bind(field)
    .fetch_all(pool)
    .await?;

    Ok(Some(FormationContext {
        formation_name: formation_name.to_string(),
        field: field.to_string(),
        rigs: rig_rows
            .into_iter()
            .map(|(rig_id, weight)| RigSummary {
                rig_id,
                // De-normalise: weight = avg_rop / 200.0
                avg_rop_ft_hr: (weight * 200.0).round(),
            })
            .collect(),
        avg_rop_ft_hr,
        top_anomaly_categories: anomaly_rows.into_iter().map(|(c,)| c).collect(),
    }))
}

/// Build context for a specific rig — formations it has drilled + performance.
///
/// Returns `None` when the rig has no graph node.
pub async fn rig_context(pool: &PgPool, rig_id: &str) -> Result<Option<RigContext>> {
    // ── Find rig node ─────────────────────────────────────────────────────────
    let node: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM kg_nodes WHERE node_type = 'rig' AND label = $1",
    )
    .bind(rig_id)
    .fetch_optional(pool)
    .await?;

    let (rig_node_id,) = match node {
        Some(n) => n,
        None => return Ok(None),
    };

    // ── Outbound DRILLED_IN edges → formation nodes ───────────────────────────
    let formation_rows: Vec<(String, serde_json::Value, f64)> = sqlx::query_as(
        r#"SELECT n.label, n.properties, e.weight
           FROM kg_edges e
           JOIN kg_nodes n ON n.id = e.to_node
           WHERE e.from_node  = $1
             AND e.edge_type  = 'DRILLED_IN'
             AND n.node_type  = 'formation'
           ORDER BY e.weight DESC
           LIMIT 20"#,
    )
    .bind(&rig_node_id)
    .fetch_all(pool)
    .await?;

    Ok(Some(RigContext {
        rig_id: rig_id.to_string(),
        formations: formation_rows
            .into_iter()
            .map(|(label, props, weight)| {
                // label format: "{field}::{formation_name}"
                let parts: Vec<&str> = label.splitn(2, "::").collect();
                let field = parts.first().copied().unwrap_or("").to_string();
                let formation_name = parts.get(1).copied().unwrap_or(&label).to_string();
                FormationSummary {
                    formation_name,
                    field,
                    avg_rop_ft_hr: props["avg_rop_ft_hr"]
                        .as_f64()
                        .unwrap_or_else(|| (weight * 200.0).round()),
                }
            })
            .collect(),
    }))
}

// ─── Prompt serialisation helpers ─────────────────────────────────────────────

impl FormationContext {
    /// Serialise to a compact multi-line string for LLM prompt injection.
    pub fn to_prompt_string(&self) -> String {
        let mut lines = vec![
            format!(
                "FORMATION_CONTEXT: {} ({})",
                self.formation_name, self.field
            ),
            format!(
                "  Fleet avg ROP: {:.0} ft/hr across {} rigs",
                self.avg_rop_ft_hr,
                self.rigs.len()
            ),
        ];

        if !self.rigs.is_empty() {
            let rig_list = self
                .rigs
                .iter()
                .map(|r| format!("{}={:.0}ft/hr", r.rig_id, r.avg_rop_ft_hr))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("  Rigs drilled: {}", rig_list));
        }

        if !self.top_anomaly_categories.is_empty() {
            lines.push(format!(
                "  Common anomalies in field: {}",
                self.top_anomaly_categories.join(", ")
            ));
        }

        lines.join("\n")
    }
}

impl RigContext {
    /// Serialise to a compact multi-line string for LLM prompt injection.
    pub fn to_prompt_string(&self) -> String {
        let mut lines = vec![format!(
            "RIG_CONTEXT: {} ({} formations drilled)",
            self.rig_id,
            self.formations.len()
        )];

        for f in &self.formations {
            lines.push(format!(
                "  {} [{}]: avg {:.0} ft/hr",
                f.formation_name, f.field, f.avg_rop_ft_hr
            ));
        }

        lines.join("\n")
    }
}
