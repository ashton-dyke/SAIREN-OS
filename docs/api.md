# SAIREN-OS API Reference

All routes return JSON unless noted. Timestamps are Unix seconds (u64). Auth
headers are required on routes marked **Bearer** or **Admin**.

---

## Edge API  (`/api/v1/…`)

The edge process binds on port **8080** by default.

### Authentication

Most read endpoints are unauthenticated (internal rig dashboard use). The
fleet-sync endpoints require a bearer token set via the `FLEET_CLIENT_KEY`
environment variable:

```
Authorization: Bearer <FLEET_CLIENT_KEY>
```

---

### Health & Status

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `GET` | `/health` | None | Legacy health check — `{"status","version","uptime_seconds"}` |
| `GET` | `/api/v1/health` | None | Drilling health assessment from latest advisory |
| `GET` | `/api/v1/status` | None | Full WITS parameter snapshot + system status |
| `GET` | `/api/v1/drilling` | None | MSE efficiency, formation analysis, specialist votes |
| `GET` | `/api/v1/verification` | None | Latest fault verification result |
| `GET` | `/api/v1/diagnosis` | None | Current strategic advisory — 204 if no advisory yet |
| `GET` | `/api/v1/baseline` | None | Baseline learning status and learned thresholds |

#### `GET /api/v1/health`

```json
{
  "health_score": 87.3,
  "severity": "Low",
  "diagnosis": "MSE efficiency within normal range",
  "recommended_action": "Continue current parameters",
  "timestamp": "2024-01-15T10:30:00Z",
  "confidence": 0.9,
  "rpm": 120.0,
  "mse_efficiency": 87.3,
  "risk_level": "Low"
}
```

#### `GET /api/v1/status`

Returns all live WITS parameters including `bit_depth`, `rop`, `wob`, `rpm`,
`torque`, `spp`, `hook_load`, `flow_in`, `flow_out`, `pit_volume`,
`mud_weight`, `ecd`, `gas_units`, `ecd_margin`, plus `rig_state`,
`operation`, `total_analyses`, `uptime_secs`.

#### `GET /api/v1/drilling`

```json
{
  "mse": 28500.0,
  "mse_efficiency": 73.2,
  "mse_baseline": 21000.0,
  "mse_deviation": 8.5,
  "d_exponent": 1.42,
  "dxc": 1.38,
  "formation_type": "Normal",
  "formation_change": false,
  "trend": "Stable",
  "votes": {
    "mse": "Medium",
    "hydraulic": "Low",
    "well_control": "Low",
    "formation": "Low"
  },
  "cfc_formation_transition": null
}
```

---

### Strategic Reports

| Method | Path | Auth | Query params | Description |
|--------|------|------|-------------|-------------|
| `GET` | `/api/v1/strategic/hourly` | None | `?limit=24` | Up to 24 recent hourly reports |
| `GET` | `/api/v1/strategic/daily` | None | `?limit=7` | Up to 7 recent daily reports |

---

### Campaign Management

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `GET` | `/api/v1/campaign` | None | Current campaign type and thresholds |
| `POST` | `/api/v1/campaign` | None | Set campaign (`"production"` or `"p&a"`) |

#### `POST /api/v1/campaign` request body

```json
{ "campaign": "production" }
```

Valid values: `"production"`, `"prod"`, `"p&a"`, `"pa"`, `"plug_abandonment"`.

---

### ML Engine

| Method | Path | Auth | Query params | Description |
|--------|------|------|-------------|-------------|
| `GET` | `/api/v1/ml/latest` | None | — | Latest ML insights report |
| `GET` | `/api/v1/ml/history` | None | `?limit=24&campaign=production` | Historical ML reports |
| `GET` | `/api/v1/ml/optimal` | None | `?depth=3250.0` | Optimal parameters near a depth |

---

### Configuration

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `GET` | `/api/v1/config` | None | Active `WellConfig` as JSON |
| `POST` | `/api/v1/config` | None | Validate and save new config to `well_config.toml` |
| `POST` | `/api/v1/config/validate` | None | Validate config without saving |

Config changes take effect on restart.

---

### Advisory Acknowledgments

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `POST` | `/api/v1/advisory/acknowledge` | None | Acknowledge an advisory ticket (persisted to sled) |
| `GET` | `/api/v1/advisory/acknowledgments` | None | List all in-memory acknowledgments |

#### `POST /api/v1/advisory/acknowledge` request body

```json
{
  "ticket_timestamp": 1705314600,
  "acknowledged_by": "Driller",
  "notes": "Adjusted WOB",
  "action_taken": "adjusted_parameters"
}
```

---

### Critical Reports

| Method | Path | Auth | Query params | Description |
|--------|------|------|-------------|-------------|
| `GET` | `/api/v1/reports/critical` | None | `?limit=50` | Last N critical severity reports |
| `POST` | `/api/v1/reports/test` | None | — | Create synthetic critical report (UI testing) |

---

### Shift Summary

| Method | Path | Auth | Query params | Description |
|--------|------|------|-------------|-------------|
| `GET` | `/api/v1/shift/summary` | None | `?hours=12` or `?from=<ts>&to=<ts>` | Shift KPIs for a time range |

---

### Prometheus Metrics

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `GET` | `/api/v1/metrics` | None | Prometheus text format (version 0.0.4) |

Exposed counters/gauges:
- `sairen_packets_total` — cumulative WITS packets processed
- `sairen_tickets_created_total` — advisory tickets generated
- `sairen_tickets_verified_total` — tickets confirmed by strategic agent
- `sairen_tickets_rejected_total` — tickets rejected as transient
- `sairen_uptime_seconds` — process uptime
- `sairen_avg_mse_efficiency` — rolling MSE efficiency (0–100)

---

### Fleet Intelligence (requires `fleet-client` feature)

| Method | Path | Auth | Query params | Description |
|--------|------|------|-------------|-------------|
| `GET` | `/api/v1/fleet/intelligence` | None | `?type=benchmark&formation=Ekofisk` | Cached hub intelligence outputs |

---

## Hub API  (`/api/fleet/…`)

The fleet hub binds on port **8080** by default. Rate limiting: 20 req/s
sustained, burst of 50, per IP address (HTTP 429 on exhaustion).

### Authentication

Two key types:

| Key | Header | Used for |
|-----|--------|---------|
| `FLEET_ADMIN_KEY` | `Authorization: Bearer <key>` | Admin-only routes (dashboard, graph, registry) |
| Rig registration | `rig-id: <rig_id>` + `Authorization: Bearer <rig_key>` | Rig-facing routes (events, intelligence) |

---

### Health

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `GET` | `/api/fleet/health` | None | Hub health — `{"status","db","version"}` |
| `GET` | `/api/fleet/metrics` | None | Prometheus text format metrics |

**Prometheus metrics** (`/api/fleet/metrics`):
- `hub_intelligence_jobs_pending` — queued jobs
- `hub_intelligence_jobs_in_flight` — jobs in progress
- `hub_intelligence_jobs_completed_total` — cumulative completed
- `hub_intelligence_jobs_failed_total` — cumulative failed
- `hub_registered_rigs_total` — total registered rigs
- `hub_active_rigs_total` — rigs active in last 48 hours

---

### Event Ingestion

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `POST` | `/api/fleet/events` | Rig | Upload a new advisory event |
| `GET` | `/api/fleet/events/{id}` | Rig | Retrieve event by UUID |
| `PATCH` | `/api/fleet/events/{id}/outcome` | Rig | Update event outcome |

#### `POST /api/fleet/events` request body

```json
{
  "rig_id": "RIG-001",
  "category": "WellControl",
  "severity": "Critical",
  "payload": { ... },
  "timestamp": 1705314600
}
```

---

### Rig Registry

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `GET` | `/api/fleet/rigs` | Admin | List all registered rigs |
| `GET` | `/api/fleet/rigs/{id}` | Admin | Get rig details |
| `POST` | `/api/fleet/rigs/register` | Admin | Register a new rig (returns API key) |
| `POST` | `/api/fleet/rigs/{id}/revoke` | Admin | Revoke a rig's API key |

---

### Performance Data (Offset Well Sharing)

| Method | Path | Auth | Query params | Description |
|--------|------|------|-------------|-------------|
| `POST` | `/api/fleet/performance` | Rig | — | Upload post-well ML performance data |
| `GET` | `/api/fleet/performance` | Rig | `?field=NorthSea&since=<ts>&exclude_rig=RIG-001` | Query fleet performance for a field |

Supports `Content-Encoding: zstd` for compressed uploads.

---

### Library Sync

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `GET` | `/api/fleet/library` | Rig | Get advisory library (since last sync) |
| `GET` | `/api/fleet/library/stats` | Admin | Library version and record counts |

---

### Dashboard

| Method | Path | Auth | Query params | Description |
|--------|------|------|-------------|-------------|
| `GET` | `/api/fleet/dashboard/summary` | Admin | — | Fleet overview (rigs, events, top categories) |
| `GET` | `/api/fleet/dashboard/trends` | Admin | `?days=30` | Event trends by day and category |
| `GET` | `/api/fleet/dashboard/outcomes` | Admin | — | Resolution rate analytics by category |

---

### Intelligence Distribution

| Method | Path | Auth | Query params | Description |
|--------|------|------|-------------|-------------|
| `GET` | `/api/fleet/intelligence` | Rig | `?since=<ts>&formation=Ekofisk` | Pull intelligence outputs (cursor-based) |

Rigs store the returned `synced_at` timestamp and pass it as `since` on the
next poll to receive only new outputs. Fleet-wide outputs (`rig_id IS NULL`)
are returned to all rigs; rig-specific outputs only to the owning rig.

---

### Knowledge Graph

| Method | Path | Auth | Query params | Description |
|--------|------|------|-------------|-------------|
| `GET` | `/api/fleet/graph/stats` | Admin | — | Node and edge counts by type |
| `GET` | `/api/fleet/graph/formation` | Admin | `?name=Ekofisk&field=NorthSea` | Formation context from graph |
| `POST` | `/api/fleet/graph/rebuild` | Admin | — | Trigger full graph rebuild from fleet data |
