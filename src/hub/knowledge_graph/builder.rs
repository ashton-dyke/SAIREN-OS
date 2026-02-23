//! Knowledge graph builder — syncs operational tables into kg_nodes / kg_edges.
//!
//! Call `rebuild_graph(pool)` from the curator background task (after each
//! successful curation cycle) to keep the graph up to date.  All operations
//! are idempotent upserts so the function is safe to call repeatedly.
//!
//! ## Sync order
//!
//! 1. **Rig nodes** from the `rigs` table.
//! 2. **Formation + Well nodes** from `fleet_performance`, plus `DRILLED_IN`
//!    and `DRILLED_BY` edges.
//!
//! Formation nodes carry the average ROP in their properties; the raw value
//! (ft/hr) is stored as-is for display, and a 0–1 normalised weight (capped
//! at 200 ft/hr) is stored on the edge for graph algorithms.

use std::collections::HashMap;

use anyhow::Result;
use serde_json::json;
use sqlx::{PgPool, Transaction, Postgres};
use tracing::{info, warn};

use super::{edge_type, node_type, upsert_edge, upsert_node};

/// Rebuild the full knowledge graph from the operational tables.
///
/// All upserts run inside a single database transaction so that the graph is
/// never left in a partially rebuilt state if an error occurs mid-way.
pub async fn rebuild_graph(pool: &PgPool) -> Result<()> {
    let mut tx = pool.begin().await?;

    let rig_count = sync_rigs(&mut tx).await?;
    let (formation_count, well_count) = sync_formations_and_wells(&mut tx).await?;

    tx.commit().await?;

    info!(
        rigs = rig_count,
        formations = formation_count,
        wells = well_count,
        "Knowledge graph rebuilt"
    );

    Ok(())
}

// ─── Rig nodes ────────────────────────────────────────────────────────────────

/// Sync one node per rig from the `rigs` registry table.
async fn sync_rigs(tx: &mut Transaction<'_, Postgres>) -> Result<usize> {
    let rows: Vec<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT rig_id, status, well_id, field FROM rigs",
    )
    .fetch_all(&mut **tx)
    .await?;

    let count = rows.len();

    for (rig_id, status, well_id, field) in rows {
        upsert_node(
            &mut **tx,
            node_type::RIG,
            &rig_id,
            json!({
                "status": status,
                "well_id": well_id,
                "field":   field,
            }),
        )
        .await?;
    }

    Ok(count)
}

// ─── Formation + Well nodes ───────────────────────────────────────────────────

/// Sync formation and well nodes from `fleet_performance`, creating edges.
///
/// Returns `(formation_count, well_count)` — counts of unique entities.
async fn sync_formations_and_wells(tx: &mut Transaction<'_, Postgres>) -> Result<(usize, usize)> {
    // Preload rig node IDs (inserted in the same transaction by sync_rigs) to
    // avoid per-row selects.
    let rig_node_rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT label, id FROM kg_nodes WHERE node_type = 'rig'",
    )
    .fetch_all(&mut **tx)
    .await?;

    let rig_node_ids: HashMap<String, String> = rig_node_rows.into_iter().collect();

    // Fetch all performance records, extracting avg_rop from JSONB
    let rows: Vec<(String, String, String, String, f64)> = sqlx::query_as(
        r#"SELECT rig_id, well_id, field, formation_name,
                  COALESCE((performance->>'avg_rop_ft_hr')::double precision, 0.0)
           FROM fleet_performance"#,
    )
    .fetch_all(&mut **tx)
    .await?;

    let mut formation_labels: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut well_labels: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    for (rig_id, well_id, field, formation_name, avg_rop) in rows {
        let formation_label = format!("{}::{}", field, formation_name);
        let well_label = format!("{}::{}", rig_id, well_id);

        // ── Formation node ────────────────────────────────────────────────────
        let formation_id = upsert_node(
            &mut **tx,
            node_type::FORMATION,
            &formation_label,
            json!({
                "name":          formation_name,
                "field":         field,
                "avg_rop_ft_hr": avg_rop,
            }),
        )
        .await?;

        formation_labels.insert(formation_label);

        // ── Well node ─────────────────────────────────────────────────────────
        let well_node_id = upsert_node(
            &mut **tx,
            node_type::WELL,
            &well_label,
            json!({
                "rig_id":  rig_id,
                "well_id": well_id,
                "field":   field,
            }),
        )
        .await?;

        well_labels.insert(well_label);

        // ── Edges ─────────────────────────────────────────────────────────────
        match rig_node_ids.get(&rig_id) {
            Some(rig_node_id) => {
                // DRILLED_IN: rig → formation  (weight = normalised avg ROP)
                let weight = (avg_rop / 200.0_f64).clamp(0.0, 1.0);

                upsert_edge(
                    &mut **tx,
                    rig_node_id,
                    &formation_id,
                    edge_type::DRILLED_IN,
                    weight,
                    json!({ "avg_rop_ft_hr": avg_rop }),
                )
                .await?;

                // DRILLED_BY: well → rig
                upsert_edge(
                    &mut **tx,
                    &well_node_id,
                    rig_node_id,
                    edge_type::DRILLED_BY,
                    1.0,
                    serde_json::Value::Object(serde_json::Map::new()),
                )
                .await?;
            }
            None => {
                warn!(
                    rig_id = %rig_id,
                    "Rig has performance data but no graph node — skipping edges"
                );
            }
        }
    }

    Ok((formation_labels.len(), well_labels.len()))
}
