-- SAIREN Fleet Hub — Knowledge Graph (Phase 3)
--
-- PostgreSQL-backed graph for fleet-wide GraphRAG intelligence.
-- Nodes represent formations, rigs, and wells.
-- Edges capture drilling relationships with performance weights.
--
-- Node types : 'formation' | 'rig' | 'well'
-- Edge types : 'DRILLED_IN' | 'DRILLED_BY'

-- ─── Nodes ───────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS kg_nodes (
    id          TEXT        PRIMARY KEY,              -- UUID as text
    node_type   TEXT        NOT NULL,                 -- 'formation' | 'rig' | 'well'
    label       TEXT        NOT NULL,                 -- unique human-readable key
    properties  JSONB       NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Natural key: (type, label) must be unique — used by upsert ON CONFLICT
CREATE UNIQUE INDEX IF NOT EXISTS kg_nodes_natural_key ON kg_nodes (node_type, label);
CREATE INDEX        IF NOT EXISTS kg_nodes_type_idx    ON kg_nodes (node_type);
CREATE INDEX        IF NOT EXISTS kg_nodes_label_idx   ON kg_nodes (label);

-- ─── Edges ───────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS kg_edges (
    id          TEXT             PRIMARY KEY,         -- UUID as text
    from_node   TEXT             NOT NULL REFERENCES kg_nodes(id) ON DELETE CASCADE,
    to_node     TEXT             NOT NULL REFERENCES kg_nodes(id) ON DELETE CASCADE,
    edge_type   TEXT             NOT NULL,            -- 'DRILLED_IN' | 'DRILLED_BY'
    weight      DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    properties  JSONB            NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ      NOT NULL DEFAULT NOW()
);

-- Prevent duplicate (from, to, type) pairs — upsert target
CREATE UNIQUE INDEX IF NOT EXISTS kg_edges_unique   ON kg_edges (from_node, to_node, edge_type);
CREATE INDEX        IF NOT EXISTS kg_edges_from_idx ON kg_edges (from_node);
CREATE INDEX        IF NOT EXISTS kg_edges_to_idx   ON kg_edges (to_node);
CREATE INDEX        IF NOT EXISTS kg_edges_type_idx ON kg_edges (edge_type);
