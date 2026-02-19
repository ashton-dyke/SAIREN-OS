# Fleet Hub Design — SAIREN-OS Multi-Rig Learning

**Status:** Design Review
**Date:** February 8, 2026

---

## Table of Contents

1. [What the Hub Is](#1-what-the-hub-is)
2. [What the Hub Is Not](#2-what-the-hub-is-not)
3. [Architecture Overview](#3-architecture-overview)
4. [Data Flow: Spoke → Hub → Spoke](#4-data-flow-spoke--hub--spoke)
5. [Hub Components](#5-hub-components)
6. [Hub API](#6-hub-api)
7. [Event Ingestion Pipeline](#7-event-ingestion-pipeline)
8. [Library Curation](#8-library-curation)
9. [Library Sync (Hub → Spoke)](#9-library-sync-hub--spoke)
10. [Rig Registry](#10-rig-registry)
11. [Network & Security](#11-network--security)
12. [Storage & Schema](#12-storage--schema)
13. [Fleet Dashboard](#13-fleet-dashboard)
14. [Failure Modes](#14-failure-modes)
15. [Bandwidth Budget](#15-bandwidth-budget)
16. [Deployment Topology](#16-deployment-topology)
17. [What Already Exists (Spoke Side)](#17-what-already-exists-spoke-side)
18. [What Needs Building](#18-what-needs-building)
19. [Open Questions](#19-open-questions)

---

## 1. What the Hub Is

The hub is a central server that collects confirmed anomaly events from every rig running SAIREN-OS, curates those events into a precedent library, and distributes the library back to all rigs. Its purpose is **fleet-wide learning**: when Rig A encounters a pack-off at 12,000 ft in shale and the driller resolves it by reducing WOB, that resolution becomes a precedent that Rig B can reference when it encounters similar conditions.

The hub does three things:

1. **Receives events** — Rigs push confirmed AMBER/RED advisory events (not raw WITS data) to the hub when connectivity allows
2. **Curates a library** — The hub processes incoming events into compact `FleetEpisode` precedents, scores them by outcome quality, and builds a library optimized for similarity search
3. **Distributes the library** — Rigs pull the curated library on a 6-hour cadence and load it into RAM Recall for sub-millisecond precedent lookup during anomaly processing

The hub is a **learning aggregator**, not a control plane. It never sends commands to rigs. It never processes WITS data. It never makes drilling decisions.

---

## 2. What the Hub Is Not

- **Not a real-time monitoring system.** The hub does not receive live WITS streams. It sees events hours or days after they happen. Real-time monitoring is entirely local to each rig.
- **Not a command-and-control server.** Rigs are autonomous. If the hub disappears, every rig continues operating exactly as before — they just don't get new precedents.
- **Not a data lake.** The hub stores curated episodes (~5 KB each), not raw sensor data (~500 bytes/second/rig). A 50-rig fleet running for a year produces maybe 2 GB of hub data.
- **Not a single point of failure.** Loss of the hub degrades fleet learning. It does not degrade any rig's ability to detect anomalies, generate advisories, or protect the well.

---

## 3. Architecture Overview

```
   RIG A (Spoke)                    FLEET HUB                     RIG B (Spoke)
   ┌──────────────┐                ┌──────────────────┐          ┌──────────────┐
   │  SAIREN-OS   │                │                  │          │  SAIREN-OS   │
   │  Pipeline    │                │  Event Ingestion │          │  Pipeline    │
   │      │       │                │       │          │          │      │       │
   │      ▼       │   FleetEvent   │       ▼          │          │      ▼       │
   │ UploadQueue ─┼───(HTTPS)────► │  Dedup + Store   │          │ UploadQueue ─┼──►
   │              │                │       │          │          │              │
   │              │                │       ▼          │          │              │
   │              │                │  Library Curator │          │              │
   │              │                │       │          │          │              │
   │              │                │       ▼          │          │              │
   │  RAMRecall  ◄┼───(HTTPS)──── │  Episode Library │ ────────►┼  RAMRecall   │
   │              │   6h sync      │                  │  6h sync │              │
   │              │                │  Fleet Dashboard │          │              │
   └──────────────┘                └──────────────────┘          └──────────────┘

   ◄── WireGuard VPN tunnel ──────────────────────────────────────────────────►
```

**Key principle:** Data flows in two directions with very different cadences:

| Direction | What | When | Size |
|-----------|------|------|------|
| Spoke → Hub | `FleetEvent` (advisory + history window + outcome) | Per confirmed AMBER/RED event (~2-10/day/rig) | ~50-200 KB each (zstd compressed) |
| Hub → Spoke | `FleetLibrary` (curated episodes) | Every 6 hours | ~500 KB - 5 MB (delta sync) |

---

## 4. Data Flow: Spoke → Hub → Spoke

### 4.1 Event Creation (on the rig)

This already exists in the codebase. When the pipeline produces a confirmed AMBER/RED advisory:

1. `should_upload()` in `fleet/types.rs` checks if the advisory qualifies (Elevated, High, or Critical risk level)
2. A `FleetEvent` is constructed with the advisory, a compressed `HistorySnapshot` window, rig/well metadata, and an initial `EventOutcome::Pending`
3. The event is written to the `UploadQueue` (disk-backed, idempotent by event ID)

### 4.2 Event Upload (spoke → hub)

A background task on the rig (not yet built) periodically drains the `UploadQueue` and POSTs events to the hub:

```
Spoke UploadQueue                        Hub
      │                                   │
      │  POST /api/fleet/events           │
      │  Content-Encoding: zstd           │
      │  X-Rig-ID: RIG-ALPHA-7           │
      │  Body: FleetEvent (JSON+zstd)     │
      │ ─────────────────────────────────►│
      │                                   │  Decompress
      │                                   │  Validate schema
      │                                   │  Dedup by event ID
      │                                   │  Store in events table
      │                                   │  Queue for curation
      │  201 Created                      │
      │◄───────────────────────────────── │
      │                                   │
      │  mark_uploaded(event_id)          │
      │  (removes file from local queue)  │
```

If the upload fails (network error, hub down, 5xx), the event stays in the queue and retries on the next cycle. Events are never lost because the queue is disk-backed and survives restarts.

### 4.3 Outcome Update (spoke → hub)

When a driller acknowledges an advisory on the rig dashboard (already supported via `POST /api/v1/advisory/acknowledge`), the outcome should be forwarded to the hub:

```
Spoke                                    Hub
  │                                       │
  │  PATCH /api/fleet/events/{id}/outcome │
  │  Body: {                              │
  │    "outcome": "Resolved",             │
  │    "action_taken": "Reduced WOB",     │
  │    "notes": "Pack-off cleared in 5m"  │
  │  }                                    │
  │ ─────────────────────────────────────►│
  │                                       │  Update event record
  │                                       │  Re-curate episode
  │  200 OK                               │  (resolved events rank higher)
  │◄───────────────────────────────────── │
```

Outcome updates are critical for library quality. A `Resolved` event with a specific `action_taken` is worth 10x more than a `Pending` event because it tells other rigs what actually worked.

### 4.4 Library Curation (on the hub)

The hub processes raw `FleetEvent` records into compact `FleetEpisode` precedents. This is a batch process that runs whenever new events arrive or outcomes update:

1. **Extract** — `FleetEpisode::from_event()` strips the full history window down to key metrics + metadata
2. **Score** — Episodes scored by outcome quality (Resolved > Escalated > FalsePositive > Pending) and recency
3. **Deduplicate** — Multiple events from the same rig at the same depth for the same category within 10 minutes collapse into one episode
4. **Index** — Episodes indexed by (category, campaign, formation_type, depth_range) for efficient sync filtering
5. **Prune** — Episodes older than 12 months or with `FalsePositive` outcomes older than 3 months are archived

### 4.5 Library Sync (hub → spoke)

Every 6 hours, each rig pulls the latest library:

```
Spoke                                    Hub
  │                                       │
  │  GET /api/fleet/library               │
  │  X-Rig-ID: RIG-ALPHA-7               │
  │  If-Modified-Since: <last_sync_ts>    │
  │  Accept-Encoding: zstd                │
  │ ─────────────────────────────────────►│
  │                                       │  Build delta since last_sync_ts
  │                                       │  Filter out spoke's own events
  │                                       │  (rig already has its own data)
  │  200 OK                               │
  │  Content-Encoding: zstd               │
  │  Body: FleetLibrary {                 │
  │    episodes: [...],                   │
  │    version: 42,                       │
  │    total_fleet_episodes: 1847         │
  │  }                                    │
  │◄───────────────────────────────────── │
  │                                       │
  │  ram_recall.load_episodes(episodes)   │
  │  (replaces in-memory index)           │
```

Delta sync means the spoke only downloads episodes created or updated since its last sync. On first sync, the spoke gets the full library. The hub excludes the spoke's own events from the response because the spoke already has them locally.

---

## 5. Hub Components

```
┌─────────────────────────────────────────────────────────┐
│                      FLEET HUB                          │
│                                                         │
│  ┌─────────────┐   ┌──────────────┐   ┌─────────────┐  │
│  │  HTTP API   │   │   Curator    │   │  Dashboard   │  │
│  │  (actix-web)│   │  (batch job) │   │  (static UI) │  │
│  └──────┬──────┘   └──────┬───────┘   └──────┬───────┘  │
│         │                 │                   │          │
│  ┌──────▼─────────────────▼───────────────────▼───────┐  │
│  │                  PostgreSQL                        │  │
│  │                                                    │  │
│  │  ┌────────┐  ┌──────────┐  ┌──────┐  ┌─────────┐  │  │
│  │  │  rigs  │  │  events  │  │ epi- │  │  sync   │  │  │
│  │  │        │  │          │  │ sodes│  │  _log   │  │  │
│  │  └────────┘  └──────────┘  └──────┘  └─────────┘  │  │
│  └────────────────────────────────────────────────────┘  │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

| Component | Responsibility | Implementation |
|-----------|---------------|----------------|
| **HTTP API** | Event ingestion, outcome updates, library serving, rig registration, health | Actix-web (same framework as rig) or Axum |
| **Curator** | Event → Episode extraction, scoring, dedup, pruning | Background task on the hub, runs on event arrival + hourly sweep |
| **PostgreSQL** | Durable storage for events, episodes, rig registry, sync log | Single instance is fine for <100 rigs |
| **Dashboard** | Fleet-wide anomaly trends, rig health, library stats, outcome analytics | Static HTML + API calls (similar to rig dashboard) |

The hub is deliberately simple. It's a web server with a database and a batch job. No message queues, no stream processing, no Kubernetes. It runs on a single VM or small server.

---

## 6. Hub API

### Event Endpoints

| Endpoint | Method | Purpose | Auth |
|----------|--------|---------|------|
| `/api/fleet/events` | POST | Upload a FleetEvent from a rig | Rig API key |
| `/api/fleet/events/{id}` | GET | Get a specific event | Rig API key |
| `/api/fleet/events/{id}/outcome` | PATCH | Update event outcome | Rig API key (same rig only) |

### Library Endpoints

| Endpoint | Method | Purpose | Auth |
|----------|--------|---------|------|
| `/api/fleet/library` | GET | Download curated episode library (supports delta sync via `If-Modified-Since`) | Rig API key |
| `/api/fleet/library/stats` | GET | Library statistics (episode count, category breakdown, outcome distribution) | Rig API key |

### Registry Endpoints

| Endpoint | Method | Purpose | Auth |
|----------|--------|---------|------|
| `/api/fleet/rigs` | GET | List registered rigs | Admin |
| `/api/fleet/rigs/{id}` | GET | Rig details (last sync, event count, health) | Admin |
| `/api/fleet/rigs/register` | POST | Register a new rig (returns API key) | Admin |
| `/api/fleet/rigs/{id}/revoke` | POST | Revoke a rig's API key | Admin |

### Dashboard / Admin Endpoints

| Endpoint | Method | Purpose | Auth |
|----------|--------|---------|------|
| `/api/fleet/dashboard/summary` | GET | Fleet-wide summary (active rigs, events/day, top categories) | Admin |
| `/api/fleet/dashboard/trends` | GET | Anomaly trends across fleet (time series by category) | Admin |
| `/api/fleet/dashboard/outcomes` | GET | Outcome analytics (resolution rate, common actions) | Admin |
| `/api/fleet/health` | GET | Hub health (DB connectivity, queue depth, last curation run) | Public |

### Request/Response Examples

**Upload Event:**
```
POST /api/fleet/events
Authorization: Bearer <rig-api-key>
Content-Type: application/json
Content-Encoding: zstd
X-Rig-ID: RIG-ALPHA-7

{
  "id": "RIG-ALPHA-7-1707400000",
  "rig_id": "RIG-ALPHA-7",
  "well_id": "WELL-042",
  "field": "Permian Basin",
  "campaign": "Production",
  "advisory": { ... },           // StrategicAdvisory
  "history_window": [ ... ],     // Vec<HistorySnapshot>
  "outcome": "Pending",
  "depth": 12450.0,
  "timestamp": 1707400000
}

→ 201 Created
  { "id": "RIG-ALPHA-7-1707400000", "status": "accepted" }

→ 409 Conflict (duplicate)
  { "id": "RIG-ALPHA-7-1707400000", "status": "already_exists" }
```

**Delta Library Sync:**
```
GET /api/fleet/library
Authorization: Bearer <rig-api-key>
If-Modified-Since: 1707350000
Accept-Encoding: zstd

→ 200 OK
  Content-Encoding: zstd
  X-Library-Version: 42
  X-Total-Episodes: 1847
  X-Delta-Count: 23

  {
    "version": 42,
    "episodes": [ ... ],          // Only new/updated since 1707350000
    "total_fleet_episodes": 1847,
    "pruned_ids": ["ep-old-1"]    // Episodes removed since last sync
  }

→ 304 Not Modified (no changes since last sync)
```

---

## 7. Event Ingestion Pipeline

When the hub receives a `POST /api/fleet/events`:

```
                    ┌──────────────┐
                    │  HTTP POST   │
                    │  /events     │
                    └──────┬───────┘
                           │
                    ┌──────▼───────┐
                    │  Decompress  │  zstd → JSON
                    │  + Validate  │  schema check, field ranges
                    └──────┬───────┘
                           │
                    ┌──────▼───────┐
                    │   Dedup      │  Check events table by ID
                    │              │  → 409 if exists
                    └──────┬───────┘
                           │
                    ┌──────▼───────┐
                    │  Rig Auth    │  Verify API key matches rig_id
                    │  + Register  │  Update rig last_seen timestamp
                    └──────┬───────┘
                           │
                    ┌──────▼───────┐
                    │  Store Event │  INSERT into events table
                    │              │  (full FleetEvent JSON)
                    └──────┬───────┘
                           │
                    ┌──────▼───────┐
                    │  Queue for   │  Mark event as needs_curation
                    │  Curation    │
                    └──────┬───────┘
                           │
                    ┌──────▼───────┐
                    │  201 Created │
                    └──────────────┘
```

Validation rules:
- Event ID must match pattern `{rig_id}-{timestamp}`
- `rig_id` in body must match the API key's registered rig
- Risk level must be Elevated, High, or Critical (hub rejects Low events)
- Timestamp must be within reasonable range (not more than 7 days old, not in the future)
- History window must have at least 1 snapshot
- Total payload must be under 1 MB (compressed)

---

## 8. Library Curation

The curator runs as a background task on the hub. It transforms raw events into library episodes.

### Curation Pipeline

```
Events (raw)                          Episodes (curated)
┌───────────────┐                    ┌───────────────┐
│ Full advisory │                    │ Category      │
│ History window│  ──► from_event    │ Campaign      │
│ Outcome       │  ──► score         │ Depth range   │
│ Rig metadata  │  ──► dedup         │ Key metrics   │
│ ~100 KB       │  ──► index         │ Resolution    │
│               │                    │ Score         │
│               │                    │ ~5 KB         │
└───────────────┘                    └───────────────┘
```

### Scoring Algorithm

Episodes are scored for library ranking. Higher-scored episodes appear first in sync responses and get prioritized in RAMRecall similarity results.

```
score = outcome_weight × 0.50
      + recency_weight × 0.25
      + detail_weight  × 0.15
      + diversity_weight × 0.10
```

| Factor | Weight | What it measures |
|--------|--------|-----------------|
| **Outcome quality** | 50% | Resolved (1.0) > Escalated (0.7) > Pending (0.2) > FalsePositive (0.1) |
| **Recency** | 25% | Exponential decay: `e^(-age_days / 180)` — recent events rank higher |
| **Detail** | 15% | Has driller notes (0.3) + has specific action_taken (0.4) + has full history window (0.3) |
| **Diversity** | 10% | Bonus for underrepresented categories or rigs in the library (prevents one noisy rig from dominating) |

### Deduplication

Multiple events from the same rig for the same anomaly can cluster. The curator deduplicates:

- **Same rig + same category + depth within 100 ft + timestamps within 10 minutes** → collapse into one episode
- Keep the event with the best outcome (Resolved > Escalated > Pending)
- Merge notes from all collapsed events

### Pruning Schedule

| Rule | Action |
|------|--------|
| Episode age > 12 months | Archive (remove from active library, keep in DB) |
| FalsePositive outcome + age > 3 months | Archive |
| Pending outcome + age > 30 days | Downgrade score to 0.05 (likely abandoned) |
| Episode count > 50,000 | Prune lowest-scored episodes to stay under limit |

---

## 9. Library Sync (Hub → Spoke)

### Sync Protocol

Each rig runs a background `LibrarySync` task (not yet built). Every 6 hours:

1. **Request delta** — `GET /api/fleet/library` with `If-Modified-Since` set to last successful sync timestamp
2. **Receive response** — Hub returns only episodes created/updated since that timestamp, plus a list of pruned episode IDs to remove
3. **Merge locally** — New episodes are added to RAMRecall. Pruned IDs are removed. Existing episodes with updated outcomes are replaced.
4. **Record sync** — Store sync timestamp for next delta request

### Why 6 Hours?

- Anomaly events are rare (2-10 per rig per day). Library changes accumulate slowly.
- 6-hour cadence is frequent enough that a new rig in the fleet benefits from precedents within half a working shift.
- Low bandwidth: even a full library sync is < 5 MB. Delta syncs are typically < 100 KB.
- Avoids the complexity of real-time sync (WebSocket, SSE, push notifications) for data that doesn't need to be real-time.

The 6-hour cadence is configurable. Could be 1 hour for a tightly connected fleet or 24 hours for very remote operations.

### First Sync (Bootstrap)

When a new rig joins the fleet, its first sync downloads the entire library. For a fleet with 2,000 episodes, this is roughly 10 MB compressed. The rig loads all episodes into RAMRecall and immediately benefits from the fleet's entire learning history.

### What Gets Excluded

The hub excludes the requesting rig's own episodes from the sync response. The rig already has its own events locally and doesn't need them echoed back. This saves bandwidth and avoids duplicate precedents in RAMRecall.

---

## 10. Rig Registry

The hub maintains a registry of all rigs in the fleet.

### Rig Record

| Field | Type | Description |
|-------|------|-------------|
| `rig_id` | String | Unique identifier (e.g., "RIG-ALPHA-7") |
| `api_key_hash` | String | bcrypt hash of the rig's API key |
| `well_id` | String | Current well being drilled |
| `field` | String | Field/basin name |
| `registered_at` | Timestamp | When the rig was added to the fleet |
| `last_seen` | Timestamp | Last event upload or library sync |
| `last_sync` | Timestamp | Last successful library sync |
| `event_count` | u32 | Total events uploaded |
| `status` | Enum | Active, Inactive (no contact > 48h), Revoked |

### Registration Flow

Rig registration is an admin action, not self-service. This prevents unauthorized rigs from polluting the library:

1. **Admin** creates a rig entry via `POST /api/fleet/rigs/register` with rig_id and metadata
2. **Hub** generates a random API key, stores the bcrypt hash, returns the plaintext key once
3. **Admin** configures the API key on the rig's SAIREN-OS instance (environment variable or config file)
4. **Rig** authenticates all requests with `Authorization: Bearer <api-key>`

### Revocation

If a rig is decommissioned or compromised:

1. `POST /api/fleet/rigs/{id}/revoke` invalidates the API key
2. Hub stops accepting events from that rig
3. Existing episodes from that rig remain in the library (they're still valid learning data)
4. Optionally: admin can purge a rig's episodes if they're suspected of being bad data

---

## 11. Network & Security

### Transport

All hub communication runs over WireGuard VPN. The hub and all rigs are peers in a WireGuard mesh (or star topology with the hub as the central peer).

```
Rig A ──── WireGuard ──── Hub ──── WireGuard ──── Rig B
  10.0.1.1                10.0.0.1                10.0.1.2
```

- **Why WireGuard over TLS?** Rig-edge networks are often behind NAT, satellite links, or cellular modems. WireGuard handles these gracefully with its UDP-based protocol, automatic roaming, and minimal handshake overhead. It also provides mutual authentication via public keys, which is simpler to manage than TLS certificates in air-gapped environments.
- **Why not just HTTPS?** HTTPS is fine if the rigs have reliable internet access. WireGuard is the safer default for offshore/remote rigs where connectivity is unreliable.

### Authentication Layers

| Layer | Mechanism | Protects against |
|-------|-----------|-----------------|
| **Network** | WireGuard (peer public keys) | Unauthorized network access |
| **Application** | API key per rig (Bearer token) | Rig impersonation, data pollution |
| **Authorization** | Rig can only update its own events | Cross-rig tampering |

### Data Sovereignty Considerations

- All event data includes rig_id, well_id, and field. Operators can configure the hub to **reject or quarantine** events from specific wells/fields if data sovereignty policies require it.
- The library sync can be **filtered by field** so rigs only receive precedents from wells in the same operating area (if required by data sharing agreements between operators).
- The hub stores no raw WITS sensor data. Events contain only advisory-level summaries and a small metrics snapshot. This is significantly less sensitive than raw drilling data.

---

## 12. Storage & Schema

### PostgreSQL Tables

```sql
-- Registered rigs
CREATE TABLE rigs (
    rig_id          TEXT PRIMARY KEY,
    api_key_hash    TEXT NOT NULL,
    well_id         TEXT,
    field           TEXT,
    registered_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen       TIMESTAMPTZ,
    last_sync       TIMESTAMPTZ,
    event_count     INTEGER DEFAULT 0,
    status          TEXT DEFAULT 'active'  -- active, inactive, revoked
);

-- Raw fleet events (full fidelity)
CREATE TABLE events (
    id              TEXT PRIMARY KEY,       -- "{rig_id}-{timestamp}"
    rig_id          TEXT NOT NULL REFERENCES rigs(rig_id),
    well_id         TEXT NOT NULL,
    field           TEXT,
    campaign        TEXT NOT NULL,
    risk_level      TEXT NOT NULL,
    category        TEXT,
    depth           DOUBLE PRECISION,
    timestamp       TIMESTAMPTZ NOT NULL,
    outcome         TEXT DEFAULT 'Pending',
    action_taken    TEXT,
    notes           TEXT,
    payload         JSONB NOT NULL,         -- Full FleetEvent JSON
    needs_curation  BOOLEAN DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_events_rig ON events(rig_id);
CREATE INDEX idx_events_timestamp ON events(timestamp);
CREATE INDEX idx_events_needs_curation ON events(needs_curation) WHERE needs_curation = TRUE;

-- Curated episode library
CREATE TABLE episodes (
    id              TEXT PRIMARY KEY,
    source_event_id TEXT REFERENCES events(id),
    rig_id          TEXT NOT NULL,
    category        TEXT NOT NULL,
    campaign        TEXT NOT NULL,
    depth_min       DOUBLE PRECISION,
    depth_max       DOUBLE PRECISION,
    risk_level      TEXT NOT NULL,
    severity        TEXT NOT NULL,
    outcome         TEXT NOT NULL,
    resolution      TEXT,
    score           DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    key_metrics     JSONB NOT NULL,         -- EpisodeMetrics
    timestamp       TIMESTAMPTZ NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    archived        BOOLEAN DEFAULT FALSE
);

CREATE INDEX idx_episodes_category ON episodes(category);
CREATE INDEX idx_episodes_campaign ON episodes(campaign);
CREATE INDEX idx_episodes_score ON episodes(score DESC);
CREATE INDEX idx_episodes_updated ON episodes(updated_at);
CREATE INDEX idx_episodes_active ON episodes(archived) WHERE archived = FALSE;

-- Sync log (tracks what each rig has received)
CREATE TABLE sync_log (
    rig_id          TEXT NOT NULL REFERENCES rigs(rig_id),
    synced_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    episodes_sent   INTEGER NOT NULL,
    library_version INTEGER NOT NULL,
    PRIMARY KEY (rig_id, synced_at)
);
```

### Storage Estimates

| Entity | Size per record | Records per year (50-rig fleet) | Annual storage |
|--------|-----------------|-------------------------------|----------------|
| Events (full) | ~100 KB | ~100,000 (5/day × 50 rigs × 365 days) | ~10 GB |
| Episodes (curated) | ~5 KB | ~80,000 (after dedup) | ~400 MB |
| Sync log | ~100 bytes | ~150,000 (50 rigs × 8/day × 365) | ~15 MB |

Total: ~11 GB/year for a 50-rig fleet. A single 100 GB SSD handles years of data.

---

## 13. Fleet Dashboard

The hub serves a web dashboard for fleet-wide visibility. This is used by operations managers, not individual drillers.

### Views

**Fleet Overview**
- Active rigs (with last-seen status)
- Events per day (time series chart)
- Category breakdown (pie chart: Well Control, Efficiency, Mechanical, Formation, Hydraulics)
- Outcome distribution (Resolved vs Escalated vs Pending vs FalsePositive)

**Rig Detail**
- Event history for a specific rig
- Advisory timeline
- Outcome rate (what % of advisories were acted on)
- Sync status (last sync, library version)

**Library Health**
- Total episodes in active library
- Score distribution histogram
- Category coverage (are some categories underrepresented?)
- Rig contribution balance (is one rig generating 90% of events?)

**Trend Analysis**
- Fleet-wide anomaly trends over time
- Depth-correlated anomaly density (do pack-offs cluster at certain depths?)
- Campaign comparison (Production vs P&A event rates)
- Resolution effectiveness (which actions are most commonly taken for each category?)

---

## 14. Failure Modes

| Failure | Impact | Recovery |
|---------|--------|----------|
| **Hub is down** | Rigs can't upload or sync. No impact on rig operations. | Rigs queue events locally (disk-backed, survives restarts). Resume upload when hub returns. |
| **Hub DB is down** | Hub API returns 503. Same as hub down from rig perspective. | Fix DB. Events in rig queues are safe. No data loss. |
| **Rig can't reach hub** | Same as hub down. Rig operates autonomously. | Events queue locally. Sync resumes when connectivity restores. |
| **Rig uploads bad data** | Polluted precedents in library. | Admin revokes rig, purges its episodes. Other rigs get clean library on next sync. |
| **Hub disk full** | Can't accept new events. | Alert admin. Archive old events. Prune low-score episodes. |
| **Corrupt library sync** | Rig gets invalid episodes. | RAMRecall rejects unparseable episodes. Rig falls back to local knowledge. Full re-sync on next cycle. |
| **Clock skew between rig and hub** | Events may have confusing timestamps. Delta sync may miss events. | Hub uses server-side `created_at` for sync, not rig-reported timestamp. Rig timestamps used only for display. |
| **WireGuard tunnel drops** | Temporary loss of hub connectivity. | WireGuard auto-reconnects. Uploads retry on next cycle. No operator action needed. |

### The Critical Guarantee

**No failure mode causes a rig to stop protecting the well.** The entire 10-phase pipeline (WITS ingestion → physics → tactical → strategic → orchestrator → advisory) runs locally on the rig. The hub is purely additive — it makes advisories better by providing precedent, but the rig generates advisories without it.

---

## 15. Bandwidth Budget

For a typical offshore rig with satellite connectivity (256 Kbps - 2 Mbps):

| Activity | Frequency | Size (compressed) | Daily bandwidth |
|----------|-----------|-------------------|-----------------|
| Event upload | ~5 events/day | ~50 KB each | ~250 KB |
| Outcome update | ~3 updates/day | ~1 KB each | ~3 KB |
| Library sync | Every 6 hours | ~100 KB delta | ~400 KB |
| **Daily total** | | | **~650 KB** |

This is negligible even on the slowest satellite link. For comparison, a single email with an attachment is typically 1-5 MB.

### Why Event-Only (Not Raw WITS)?

A single rig produces ~500 bytes/second of WITS data. That's ~43 MB/day of raw sensor data. Uploading that would consume a significant fraction of offshore bandwidth and create massive storage on the hub — all for data that the hub doesn't need, because physics processing happens on the rig.

By uploading only confirmed AMBER/RED events (~5 per day), we reduce bandwidth by **99.99%** while capturing the learning-relevant signal.

---

## 16. Deployment Topology

### Minimal (Pilot: 1-5 Rigs)

```
┌──────────────────────────────────┐
│  Hub: Single VM or small server  │
│  - 2 CPU / 4 GB RAM             │
│  - 50 GB SSD                    │
│  - PostgreSQL (same machine)     │
│  - HTTPS (no WireGuard needed   │
│    if rigs have internet access) │
└──────────────────────────────────┘
```

For a pilot with a few rigs on the same network, the hub can be a single machine. WireGuard may not be needed if all rigs can reach the hub over a private network or the internet.

### Production (10-50 Rigs)

```
┌────────────────────────────────────────┐
│  Hub: Dedicated server or cloud VM     │
│  - 4 CPU / 8 GB RAM                   │
│  - 200 GB SSD                         │
│  - PostgreSQL (could be managed RDS)   │
│  - WireGuard endpoint                  │
│  - Automated backups (daily)           │
│  - Monitoring (Prometheus + Grafana    │
│    or similar)                         │
└────────────────────────────────────────┘
```

### Large Fleet (50+ Rigs)

Same as production but consider:
- **Read replicas** for PostgreSQL if library sync requests create read contention (unlikely under 100 rigs)
- **CDN or cache** for library endpoint if many rigs sync simultaneously
- **Separate curator worker** if curation batches take >1 minute (unlikely under 10,000 events)

The hub is not compute-intensive. It receives a few events per day per rig, runs a simple scoring algorithm, and serves a static-ish library. A $20/month cloud VM handles 50 rigs comfortably.

---

## 17. What Already Exists (Spoke Side)

The following spoke-side components are already implemented in the SAIREN-OS codebase:

| Component | Location | Status |
|-----------|----------|--------|
| `FleetEvent` struct | `src/fleet/types.rs` | Complete — includes advisory, history window, outcome, metadata |
| `FleetEpisode` struct | `src/fleet/types.rs` | Complete — compact precedent with `from_event()` constructor |
| `EventOutcome` enum | `src/fleet/types.rs` | Complete — Pending, Resolved, Escalated, FalsePositive |
| `HistorySnapshot` | `src/fleet/types.rs` | Complete — packet + metrics snapshot with `from_packet_and_metrics()` |
| `should_upload()` | `src/fleet/types.rs` | Complete — filters for Elevated/High/Critical risk levels |
| `UploadQueue` | `src/fleet/queue.rs` | Complete — disk-backed, idempotent, survives restarts, auto-evicts |
| `RAMRecall` | `src/context/ram_recall.rs` | Complete — metadata-filtered linear scan, KnowledgeStore trait impl |
| `KnowledgeStore` trait | `src/context/knowledge_store.rs` | Complete — `query()`, `store_name()`, `is_healthy()` |
| Advisory acknowledgment API | `src/api/handlers.rs` | Complete — `POST /api/v1/advisory/acknowledge` |

### What's Missing on the Spoke

| Component | What it does | Depends on |
|-----------|-------------|------------|
| `FleetClient` | HTTP client that uploads events from `UploadQueue` to hub | Hub API being defined |
| `LibrarySync` | Background task that pulls library from hub, loads into RAMRecall | Hub API being defined |
| `OutcomeForwarder` | Sends acknowledgment outcomes to hub as PATCH updates | Hub API being defined |
| Config for hub URL + API key | Environment variables or `well_config.toml` section | Hub deployment |

---

## 18. What Needs Building

### Hub (New Codebase or Separate Binary)

The hub is a **separate application** from SAIREN-OS. It doesn't process WITS data and doesn't run the 10-phase pipeline. It could be:

- **Option A: Separate Rust binary** in the same repo (e.g., `src/bin/fleet_hub.rs`) sharing types from the library crate. This reuses `FleetEvent`, `FleetEpisode`, and serialization code.
- **Option B: Separate repo/service** (e.g., Python/Go/Rust standalone). Simpler deployment but requires keeping types in sync.

**Recommendation: Option A.** The shared types (FleetEvent, FleetEpisode, EpisodeMetrics) are already defined in the library crate. A second binary in the same workspace avoids schema drift.

### Build Order

| Phase | Component | Effort | Priority |
|-------|-----------|--------|----------|
| 1 | Hub API skeleton (event POST, library GET, health) | 2-3 days | Must have |
| 2 | PostgreSQL schema + migrations | 1 day | Must have |
| 3 | Event ingestion pipeline (validate, dedup, store) | 1-2 days | Must have |
| 4 | Library curator (from_event, score, dedup, prune) | 2 days | Must have |
| 5 | Library sync endpoint (delta, compression, filtering) | 1-2 days | Must have |
| 6 | Spoke `FleetClient` + `LibrarySync` background tasks | 2 days | Must have |
| 7 | Rig registry + API key auth | 1 day | Must have |
| 8 | Fleet dashboard | 3-5 days | Nice to have for v1 |
| 9 | Outcome forwarding (spoke → hub PATCH) | 1 day | Should have |
| 10 | WireGuard setup automation | 1-2 days | Depends on deployment environment |

---

## 19. Open Questions

**Q1: Should the hub be a separate binary in this repo or a separate project?**
Recommend same repo (`src/bin/fleet_hub.rs`) to share FleetEvent/FleetEpisode types. But this means the hub binary depends on the SAIREN-OS library crate, which includes drilling-specific code the hub doesn't need. Acceptable trade-off for a small team.

**Q2: Should outcomes be required before episodes enter the library?**
Currently episodes are created immediately from events (even with `Pending` outcome). Alternative: wait 24-48 hours for an outcome before curating. Pro: higher quality library. Con: slower learning loop.

**Q3: Should rigs be able to reject specific precedents?**
If a rig's RAMRecall surfaces a precedent that the driller considers irrelevant, should there be a feedback mechanism? This would improve library quality but adds complexity.

**Q4: How to handle formation-specific precedents?**
A pack-off resolution in shale at 12,000 ft may not apply to sandstone at 8,000 ft. The current `FleetEpisode` struct has `depth_range` but no `formation_type`. Should formation be added? This requires rigs to report formation (which may come from mudlog or LWD data not in WITS).

**Q5: Multi-tenant hub?**
If SAIREN-OS is deployed across multiple operators, should one hub serve all operators with data isolation? Or should each operator run their own hub? Data sovereignty and competitive concerns strongly favor separate hubs per operator, but a multi-tenant option could simplify infrastructure for a managed service model.

**Q6: Episode versioning?**
When an outcome updates (Pending → Resolved), the episode changes. Should the hub track episode versions, or just overwrite? Versioning is useful for auditing but adds storage and complexity.

---

*End of Fleet Hub Design.*
