# Fleet Hub Implementation Plan — SAIREN-OS

**Status:** Implementation Ready
**Date:** February 9, 2026
**Source:** FLEET_HUB_DESIGN.md

---

## Table of Contents

1. [Phase 1 — Project Scaffolding & Hub Binary](#phase-1--project-scaffolding--hub-binary)
2. [Phase 2 — PostgreSQL Schema & Migrations](#phase-2--postgresql-schema--migrations)
3. [Phase 3 — Hub API Skeleton](#phase-3--hub-api-skeleton)
4. [Phase 4 — Event Ingestion Pipeline](#phase-4--event-ingestion-pipeline)
5. [Phase 5 — Library Curator (Batch Processing)](#phase-5--library-curator-batch-processing)
6. [Phase 6 — Library Sync Endpoint (Hub → Spoke)](#phase-6--library-sync-endpoint-hub--spoke)
7. [Phase 7 — Rig Registry & API Key Authentication](#phase-7--rig-registry--api-key-authentication)
8. [Phase 8 — Spoke-Side Clients (FleetClient, LibrarySync, OutcomeForwarder)](#phase-8--spoke-side-clients-fleetclient-librarysync-outcomeforwarder)
9. [Phase 9 — Outcome Forwarding (Spoke → Hub PATCH)](#phase-9--outcome-forwarding-spoke--hub-patch)
10. [Phase 10 — Fleet Dashboard](#phase-10--fleet-dashboard)
11. [Phase 11 — Network & Security (WireGuard)](#phase-11--network--security-wireguard)
12. [Phase 12 — Integration Testing & End-to-End Validation](#phase-12--integration-testing--end-to-end-validation)
13. [Phase 13 — Deployment & Operations](#phase-13--deployment--operations)

---

## Pre-Implementation Checklist

Before starting any phase, confirm the following:

- [ ] The existing spoke-side types compile cleanly: `FleetEvent`, `FleetEpisode`, `EventOutcome`, `HistorySnapshot`, `EpisodeMetrics` in `src/fleet/types.rs`
- [ ] `UploadQueue` in `src/fleet/queue.rs` passes its existing tests
- [ ] `RAMRecall` in `src/context/ram_recall.rs` works with `load_episodes()` and `query()`
- [ ] The project builds with `cargo build` (default features, no `llm` feature required)
- [ ] PostgreSQL 15+ is available for development (local install or Docker)

---

## Phase 1 — Project Scaffolding & Hub Binary

### Goal
Create `src/bin/fleet_hub.rs` as a separate binary in the same workspace, sharing the `sairen_os` library crate types. Add all new dependencies needed for the hub.

### Steps

#### 1.1 Add new dependencies to `Cargo.toml`

Add the following crates to `[dependencies]`:

| Crate | Version | Purpose |
|-------|---------|---------|
| `sqlx` | `0.8` | PostgreSQL async driver (features: `runtime-tokio`, `tls-rustls`, `postgres`, `chrono`, `json`) |
| `reqwest` | `0.12` | HTTP client for spoke → hub uploads (features: `json`, `rustls-tls`) |
| `zstd` | `0.13` | Compression for event payloads and library sync |
| `bcrypt` | `0.15` | API key hashing for rig registry |
| `uuid` | `1.0` | Unique ID generation (features: `v4`) |
| `base64` | `0.22` | API key encoding |
| `dotenvy` | `0.15` | `.env` file loading for hub configuration |

These should be added as **optional** behind a `fleet-hub` feature flag to avoid bloating the rig binary:

```toml
[features]
fleet-hub = ["sqlx", "bcrypt", "uuid", "base64", "dotenvy"]
fleet-client = ["reqwest", "zstd"]
```

#### 1.2 Create the hub binary entry point

Create `src/bin/fleet_hub.rs` with:

- CLI argument parsing (clap) for: `--port`, `--database-url`, `--bind-address`
- Tokio runtime initialization
- Database connection pool setup (`sqlx::PgPool`)
- Axum router initialization (empty routes, placeholder)
- Graceful shutdown handler (`tokio::signal::ctrl_c`)
- Health endpoint at `GET /api/fleet/health`
- Startup logging

#### 1.3 Create hub module structure

```
src/
  hub/
    mod.rs              — Module root, re-exports
    config.rs           — Hub configuration (env vars, CLI args)
    db.rs               — Database connection pool, migration runner
    api/
      mod.rs            — Route registration
      events.rs         — Event ingestion handlers
      library.rs        — Library sync handlers
      registry.rs       — Rig registry handlers
      dashboard.rs      — Dashboard/admin handlers
      health.rs         — Health check handler
    curator/
      mod.rs            — Curation pipeline orchestrator
      scoring.rs        — Episode scoring algorithm
      dedup.rs          — Episode deduplication logic
      pruning.rs        — Episode pruning/archival
    auth/
      mod.rs            — Authentication middleware
      api_key.rs        — API key validation, bcrypt hashing
```

#### 1.4 Register the new binary in `Cargo.toml`

```toml
[[bin]]
name = "fleet-hub"
path = "src/bin/fleet_hub.rs"
required-features = ["fleet-hub"]
```

#### 1.5 Verify build

```bash
cargo build --bin fleet-hub --features fleet-hub
```

### Troubleshooting — Phase 1

| Problem | Likely Cause | Fix |
|---------|-------------|-----|
| `sqlx` fails to compile | Missing system SSL libraries | Install `libssl-dev` (Debian) or `openssl-devel` (RHEL). Or use `rustls` feature instead of `native-tls`. |
| Feature flag conflicts between `sairen-os` and `fleet-hub` binaries | Shared dependency version conflicts | Ensure `sqlx` and `reqwest` use the same TLS backend (`rustls`). Avoid mixing `native-tls` and `rustls`. |
| `cargo build` error: "can't find crate for `sairen_os`" | Library crate not properly exposed | Verify `src/lib.rs` exists and exports `fleet` module. Check `Cargo.toml` has `[lib]` section with `name = "sairen_os"`. |
| Binary not found by cargo | `required-features` not passed | Must build with `cargo build --features fleet-hub --bin fleet-hub`. |
| Dependency resolution conflict with existing `sled` or `tokio` versions | Major version mismatch | Pin compatible versions. `sqlx 0.8` requires `tokio 1.x` which is already in use. |
| `bcrypt` compile fails on musl target | C library linking issues | Consider switching to `argon2` (pure Rust) or use `bcrypt` with `--target x86_64-unknown-linux-gnu`. |
| Large binary size after adding `sqlx` | sqlx includes compile-time query checking | Use `sqlx::query!` only in development. For production builds, use `sqlx::query_as` with runtime checks or `SQLX_OFFLINE=true`. |

---

## Phase 2 — PostgreSQL Schema & Migrations

### Goal
Define the database schema, create migration files, and implement the migration runner.

### Steps

#### 2.1 Install sqlx-cli

```bash
cargo install sqlx-cli --no-default-features --features rustls,postgres
```

#### 2.2 Create migration directory

```bash
mkdir -p migrations
```

#### 2.3 Create initial migration: `001_initial_schema.sql`

Create `migrations/001_initial_schema.sql` with the full schema from the design doc:

**Tables to create:**

1. **`rigs`** — Rig registry
   - `rig_id TEXT PRIMARY KEY`
   - `api_key_hash TEXT NOT NULL`
   - `well_id TEXT`
   - `field TEXT`
   - `registered_at TIMESTAMPTZ NOT NULL DEFAULT NOW()`
   - `last_seen TIMESTAMPTZ`
   - `last_sync TIMESTAMPTZ`
   - `event_count INTEGER DEFAULT 0`
   - `status TEXT DEFAULT 'active'` (active, inactive, revoked)

2. **`events`** — Raw fleet events
   - `id TEXT PRIMARY KEY` (`{rig_id}-{timestamp}`)
   - `rig_id TEXT NOT NULL REFERENCES rigs(rig_id)`
   - `well_id TEXT NOT NULL`
   - `field TEXT`
   - `campaign TEXT NOT NULL`
   - `risk_level TEXT NOT NULL`
   - `category TEXT`
   - `depth DOUBLE PRECISION`
   - `timestamp TIMESTAMPTZ NOT NULL`
   - `outcome TEXT DEFAULT 'Pending'`
   - `action_taken TEXT`
   - `notes TEXT`
   - `payload JSONB NOT NULL` (full FleetEvent JSON)
   - `needs_curation BOOLEAN DEFAULT TRUE`
   - `created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()`
   - `updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()`

3. **`episodes`** — Curated episode library
   - `id TEXT PRIMARY KEY`
   - `source_event_id TEXT REFERENCES events(id)`
   - `rig_id TEXT NOT NULL`
   - `category TEXT NOT NULL`
   - `campaign TEXT NOT NULL`
   - `depth_min DOUBLE PRECISION`
   - `depth_max DOUBLE PRECISION`
   - `risk_level TEXT NOT NULL`
   - `severity TEXT NOT NULL`
   - `outcome TEXT NOT NULL`
   - `resolution TEXT`
   - `score DOUBLE PRECISION NOT NULL DEFAULT 0.0`
   - `key_metrics JSONB NOT NULL` (EpisodeMetrics)
   - `timestamp TIMESTAMPTZ NOT NULL`
   - `created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()`
   - `updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()`
   - `archived BOOLEAN DEFAULT FALSE`

4. **`sync_log`** — Tracks what each rig has received
   - `rig_id TEXT NOT NULL REFERENCES rigs(rig_id)`
   - `synced_at TIMESTAMPTZ NOT NULL DEFAULT NOW()`
   - `episodes_sent INTEGER NOT NULL`
   - `library_version INTEGER NOT NULL`
   - `PRIMARY KEY (rig_id, synced_at)`

**Indexes to create:**

```sql
-- Events
CREATE INDEX idx_events_rig ON events(rig_id);
CREATE INDEX idx_events_timestamp ON events(timestamp);
CREATE INDEX idx_events_needs_curation ON events(needs_curation) WHERE needs_curation = TRUE;

-- Episodes
CREATE INDEX idx_episodes_category ON episodes(category);
CREATE INDEX idx_episodes_campaign ON episodes(campaign);
CREATE INDEX idx_episodes_score ON episodes(score DESC);
CREATE INDEX idx_episodes_updated ON episodes(updated_at);
CREATE INDEX idx_episodes_active ON episodes(archived) WHERE archived = FALSE;
```

#### 2.4 Create `updated_at` trigger

```sql
CREATE OR REPLACE FUNCTION update_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER events_updated_at
    BEFORE UPDATE ON events
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();

CREATE TRIGGER episodes_updated_at
    BEFORE UPDATE ON episodes
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();
```

#### 2.5 Implement migration runner in `src/hub/db.rs`

```rust
pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}
```

#### 2.6 Create a `library_version` sequence

```sql
CREATE SEQUENCE library_version_seq START 1;
```

This sequence is incremented each time the curator modifies the episode library, providing a monotonic version number for sync.

#### 2.7 Test migration

```bash
DATABASE_URL=postgres://user:pass@localhost/sairen_fleet sqlx migrate run
```

### Troubleshooting — Phase 2

| Problem | Likely Cause | Fix |
|---------|-------------|-----|
| `sqlx migrate run` fails: "role does not exist" | PostgreSQL user not created | Run `createuser -P sairen` and `createdb -O sairen sairen_fleet` |
| Migration fails: "relation already exists" | Migration re-run without rollback | Drop the DB and recreate: `dropdb sairen_fleet && createdb sairen_fleet` for dev. In production, use version checks. |
| `JSONB` column inserts fail | Payload not valid JSON | Validate with `serde_json::to_value()` before INSERT. Add a CHECK constraint or validate in application layer. |
| Index creation is slow on large tables | Table has many rows from bulk import | Create indexes CONCURRENTLY: `CREATE INDEX CONCURRENTLY ...` (not inside a transaction). |
| `TIMESTAMPTZ` values incorrect | Timezone mismatch between Rust and PostgreSQL | Ensure `chrono::Utc` is used throughout. Set `SET timezone = 'UTC'` on the connection. sqlx with `chrono` feature handles this. |
| `sqlx::migrate!()` compile error | `DATABASE_URL` not set at compile time | Set `SQLX_OFFLINE=true` and run `cargo sqlx prepare` to generate offline query data, OR set `DATABASE_URL` env var during compilation. |
| Foreign key constraint violations during testing | Inserting events before rigs | Always insert the rig record first. Create a test helper that seeds a rig before each test. |
| Sequence `library_version_seq` not found | Migration order issue | Ensure the sequence CREATE is in the same migration file or an earlier one than any code that references it. |
| `DOUBLE PRECISION` precision loss for depth values | IEEE 754 floating point limits | Acceptable for depth (meter-level precision). If sub-millimeter precision needed, use `NUMERIC(10,4)` instead. |

---

## Phase 3 — Hub API Skeleton

### Goal
Stand up the Axum HTTP server with all route stubs, shared state, request/response types, and middleware.

### Steps

#### 3.1 Define hub application state

In `src/hub/config.rs`:

```rust
pub struct HubConfig {
    pub database_url: String,
    pub bind_address: String,     // "0.0.0.0:8080"
    pub max_payload_size: usize,  // 1 MB default
    pub curation_interval_secs: u64,  // 3600 default (hourly)
    pub library_max_episodes: usize,  // 50000
    pub pruning_max_age_days: u64,    // 365
}
```

In `src/hub/mod.rs`, define shared state:

```rust
pub struct HubState {
    pub db: PgPool,
    pub config: HubConfig,
    pub library_version: AtomicU64,  // Current library version from DB sequence
}
```

#### 3.2 Register all route stubs

In `src/hub/api/mod.rs`, create the router:

```
POST   /api/fleet/events                  → events::upload_event
GET    /api/fleet/events/{id}             → events::get_event
PATCH  /api/fleet/events/{id}/outcome     → events::update_outcome

GET    /api/fleet/library                 → library::get_library
GET    /api/fleet/library/stats           → library::get_library_stats

GET    /api/fleet/rigs                    → registry::list_rigs
GET    /api/fleet/rigs/{id}               → registry::get_rig
POST   /api/fleet/rigs/register           → registry::register_rig
POST   /api/fleet/rigs/{id}/revoke        → registry::revoke_rig

GET    /api/fleet/dashboard/summary       → dashboard::get_summary
GET    /api/fleet/dashboard/trends        → dashboard::get_trends
GET    /api/fleet/dashboard/outcomes      → dashboard::get_outcomes
GET    /api/fleet/health                  → health::get_health
```

#### 3.3 Define request/response structs

Create shared request/response types in `src/hub/api/`:

- `UploadEventRequest` — Wraps `FleetEvent` (reuse the existing type from `fleet::types`)
- `UploadEventResponse` — `{ id: String, status: String }` (`"accepted"` or `"already_exists"`)
- `UpdateOutcomeRequest` — `{ outcome: String, action_taken: Option<String>, notes: Option<String> }`
- `LibraryResponse` — `{ version: u64, episodes: Vec<FleetEpisode>, total_fleet_episodes: u64, pruned_ids: Vec<String> }`
- `RegisterRigRequest` — `{ rig_id: String, well_id: String, field: String }`
- `RegisterRigResponse` — `{ rig_id: String, api_key: String }` (plaintext key, returned once)
- `DashboardSummary` — `{ active_rigs: u32, events_today: u32, top_categories: Vec<...> }`
- `HealthResponse` — `{ status: String, db_connected: bool, library_version: u64, last_curation: Option<String> }`

#### 3.4 Add middleware layers

- **Request size limit**: 1 MB max body (`tower_http::limit::RequestBodyLimitLayer`)
- **Compression**: gzip/zstd response compression (`tower_http::compression::CompressionLayer`)
- **Tracing**: Request/response logging (`tower_http::trace::TraceLayer`)
- **CORS**: Restrictive CORS for dashboard only (`tower_http::cors::CorsLayer`)
- **Authentication**: Custom middleware layer (see Phase 7 for implementation)

#### 3.5 Implement health endpoint

The health endpoint is the first real handler:

```rust
async fn get_health(State(hub): State<Arc<HubState>>) -> Json<HealthResponse> {
    let db_ok = sqlx::query("SELECT 1").fetch_one(&hub.db).await.is_ok();
    Json(HealthResponse {
        status: if db_ok { "healthy" } else { "degraded" },
        db_connected: db_ok,
        library_version: hub.library_version.load(Ordering::Relaxed),
        last_curation: None, // Filled in Phase 5
    })
}
```

#### 3.6 Wire up the binary entry point

Update `src/bin/fleet_hub.rs` to:
1. Parse CLI args and load `.env`
2. Connect to PostgreSQL (`PgPool::connect`)
3. Run migrations
4. Build the Axum router with all routes
5. Bind and serve

#### 3.7 Verify

```bash
cargo build --bin fleet-hub --features fleet-hub
cargo run --bin fleet-hub --features fleet-hub -- --database-url postgres://... --port 8080
curl http://localhost:8080/api/fleet/health
```

### Troubleshooting — Phase 3

| Problem | Likely Cause | Fix |
|---------|-------------|-----|
| "Address already in use" on startup | Another process on port 8080 | Change port with `--port 8081` or kill the conflicting process. Check with `lsof -i :8080`. |
| Axum route mismatch / 404 on valid paths | Router nesting issue | Verify `Router::new().nest("/api/fleet", ...)` is correct. Check trailing slashes — Axum is strict about these. |
| State type mismatch: "the trait bound `HubState: Clone` is not satisfied" | Axum requires `Clone` for state | Wrap state in `Arc<HubState>` and use `with_state(arc_state)`. |
| Request body too large (413) for legitimate events | `RequestBodyLimitLayer` too restrictive | Increase from 1 MB to 2 MB. Compressed events should be well under 1 MB but raw JSON could be larger. |
| Compression middleware conflicts with zstd `Content-Encoding` from spoke | Double-compression | Disable tower-http compression on the `/api/fleet/events` route. Let the spoke handle zstd and the hub decompress manually. |
| CORS preflight fails from dashboard | CorsLayer not configured for dashboard origin | Add the dashboard URL to `allowed_origins`. For dev, use `CorsLayer::permissive()`. |
| Axum extractor ordering error | Body extractor must be last | In handler signatures, put `State(...)` before `Json(...)`. Axum consumes the body once. |
| `PgPool::connect` hangs indefinitely | Database not reachable | Set a connection timeout: `PgPoolOptions::new().acquire_timeout(Duration::from_secs(5))`. Check `DATABASE_URL` format. |

---

## Phase 4 — Event Ingestion Pipeline

### Goal
Implement the full `POST /api/fleet/events` handler: decompress, validate, dedup, authenticate, store, and queue for curation.

### Steps

#### 4.1 Implement zstd decompression middleware

Create a helper that checks `Content-Encoding: zstd` and decompresses the body:

```rust
async fn decompress_body(headers: &HeaderMap, body: Bytes) -> Result<Bytes, StatusCode> {
    if headers.get("content-encoding").map(|v| v == "zstd").unwrap_or(false) {
        zstd::decode_all(body.as_ref())
            .map(Bytes::from)
            .map_err(|_| StatusCode::BAD_REQUEST)
    } else {
        Ok(body)
    }
}
```

#### 4.2 Implement validation rules

Create `src/hub/api/events.rs` validation function:

```rust
fn validate_event(event: &FleetEvent) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();

    // Event ID must match pattern {rig_id}-{timestamp}
    let expected_prefix = format!("{}-", event.rig_id);
    if !event.id.starts_with(&expected_prefix) {
        errors.push("Event ID must start with rig_id".into());
    }

    // Risk level must be Elevated, High, or Critical
    if !should_upload(&event.advisory) {
        errors.push("Risk level must be Elevated, High, or Critical".into());
    }

    // Timestamp within reasonable range (not > 7 days old, not in future)
    let now = chrono::Utc::now().timestamp() as u64;
    let seven_days = 7 * 24 * 3600;
    if event.timestamp > now + 300 {  // 5 min grace for clock skew
        errors.push("Timestamp is in the future".into());
    }
    if event.timestamp + seven_days < now {
        errors.push("Timestamp is more than 7 days old".into());
    }

    // History window must have at least 1 snapshot
    if event.history_window.is_empty() {
        errors.push("History window must have at least 1 snapshot".into());
    }

    if errors.is_empty() { Ok(()) } else { Err(errors) }
}
```

#### 4.3 Implement deduplication check

```rust
async fn event_exists(pool: &PgPool, event_id: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM events WHERE id = $1)"
    )
    .bind(event_id)
    .fetch_one(pool)
    .await?;
    Ok(result)
}
```

Return `409 Conflict` with `{ "id": "...", "status": "already_exists" }` if duplicate.

#### 4.4 Implement event storage

```rust
async fn store_event(pool: &PgPool, event: &FleetEvent) -> Result<(), sqlx::Error> {
    let payload = serde_json::to_value(event)?;
    sqlx::query(
        r#"INSERT INTO events (id, rig_id, well_id, field, campaign, risk_level, category,
            depth, timestamp, outcome, payload, needs_curation)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, TRUE)"#
    )
    .bind(&event.id)
    .bind(&event.rig_id)
    // ... bind all fields
    .execute(pool)
    .await?;

    // Update rig last_seen and event_count
    sqlx::query(
        "UPDATE rigs SET last_seen = NOW(), event_count = event_count + 1 WHERE rig_id = $1"
    )
    .bind(&event.rig_id)
    .execute(pool)
    .await?;

    Ok(())
}
```

#### 4.5 Wire up the full handler

```rust
async fn upload_event(
    State(hub): State<Arc<HubState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<UploadEventResponse>), (StatusCode, Json<ErrorResponse>)> {
    // 1. Decompress
    // 2. Deserialize to FleetEvent
    // 3. Validate
    // 4. Check rig auth (API key matches rig_id) — Phase 7
    // 5. Dedup check
    // 6. Store
    // 7. Return 201 Created
}
```

#### 4.6 Implement `GET /api/fleet/events/{id}`

Simple fetch by primary key:

```rust
async fn get_event(
    State(hub): State<Arc<HubState>>,
    Path(event_id): Path<String>,
) -> Result<Json<FleetEvent>, StatusCode> {
    let row = sqlx::query_as("SELECT payload FROM events WHERE id = $1")
        .bind(&event_id)
        .fetch_optional(&hub.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    match row {
        Some(r) => Ok(Json(serde_json::from_value(r.payload)?)),
        None => Err(StatusCode::NOT_FOUND),
    }
}
```

#### 4.7 Implement payload size limit

Check compressed payload size before decompression:

```rust
if body.len() > 1_048_576 {  // 1 MB compressed
    return Err((StatusCode::PAYLOAD_TOO_LARGE, "Payload exceeds 1 MB".into()));
}
```

Also limit decompressed size to prevent zip bombs:

```rust
let decompressed = zstd::decode_all(body.as_ref())?;
if decompressed.len() > 10_485_760 {  // 10 MB decompressed limit
    return Err((StatusCode::PAYLOAD_TOO_LARGE, "Decompressed payload exceeds 10 MB".into()));
}
```

#### 4.8 Write integration tests

Test cases:
- Valid event → 201 Created
- Duplicate event → 409 Conflict
- Invalid risk level (Low) → 400 Bad Request
- Future timestamp → 400 Bad Request
- Missing history window → 400 Bad Request
- Oversized payload → 413 Payload Too Large
- Invalid JSON → 400 Bad Request
- Unknown rig_id → 403 Forbidden (after Phase 7)

### Troubleshooting — Phase 4

| Problem | Likely Cause | Fix |
|---------|-------------|-----|
| zstd decompression fails with "Unknown frame descriptor" | Client not sending valid zstd frames | Verify spoke uses `zstd::encode_all()` with default compression level. Check `Content-Encoding` header is set correctly. |
| Deserialization fails: "missing field `campaign`" | Spoke and hub have different `FleetEvent` struct versions | Both binaries share the same `fleet::types` module. Rebuild both after any type changes. Run `cargo test` to catch schema drift. |
| `INSERT` fails with foreign key violation on `rig_id` | Rig not registered before uploading events | Registration (Phase 7) must happen first. In testing, seed the `rigs` table. Return a clear 403 error: "Rig not registered". |
| `serde_json::to_value` fails for `FleetEvent` | A field type doesn't implement `Serialize` | Verify all nested types (`StrategicAdvisory`, `HistorySnapshot`, etc.) derive `Serialize`. They already do in the existing codebase. |
| Timestamp validation rejects valid events | Clock skew between rig and hub | Add 5-minute grace period for future timestamps. For past timestamps, use 7-day window. Log warnings for >1hr skew. |
| Database connection pool exhaustion under load | Too many concurrent uploads | Set `PgPoolOptions::max_connections(20)`. For 50 rigs uploading ~5 events/day, pool of 10 is sufficient. |
| Payload too large for PostgreSQL `JSONB` column | Event with unusually large history window | PostgreSQL JSONB supports up to 1 GB. The real constraint is the 1 MB HTTP limit. If history windows are huge, truncate to last 100 snapshots. |
| Zip bomb attack via zstd decompression | Malicious compressed payload | The 10 MB decompressed limit (step 4.7) prevents this. Also use `zstd::Decoder` with `window_log_max` set. |
| `409 Conflict` returned for events that should be new | Event ID collision from clock issues | Event IDs use `{rig_id}-{timestamp}`. If two events have the same second-resolution timestamp, append a counter or use millisecond resolution. |

---

## Phase 5 — Library Curator (Batch Processing)

### Goal
Build the background curator that transforms raw `FleetEvent` records into scored, deduplicated `FleetEpisode` entries in the `episodes` table.

### Steps

#### 5.1 Create the curator task

In `src/hub/curator/mod.rs`, create a background task that runs:

- **On demand**: When a new event is stored (triggered by the ingestion pipeline setting `needs_curation = TRUE`)
- **On schedule**: Hourly sweep for any missed events and pruning

```rust
pub async fn run_curator(pool: PgPool, config: HubConfig) {
    let mut interval = tokio::time::interval(Duration::from_secs(config.curation_interval_secs));
    loop {
        interval.tick().await;
        if let Err(e) = curate_pending_events(&pool).await {
            tracing::error!("Curation failed: {}", e);
        }
        if let Err(e) = prune_old_episodes(&pool, &config).await {
            tracing::error!("Pruning failed: {}", e);
        }
    }
}
```

#### 5.2 Implement `FleetEpisode::from_event()` for the hub

The spoke-side `FleetEpisode::from_event()` already exists in `src/fleet/types.rs`. The hub reuses it directly (same library crate). This step is about the database wrapper:

```rust
async fn curate_pending_events(pool: &PgPool) -> Result<u32, sqlx::Error> {
    // 1. SELECT events WHERE needs_curation = TRUE
    // 2. For each event:
    //    a. Deserialize FleetEvent from payload JSONB
    //    b. Call FleetEpisode::from_event(&event)
    //    c. Score the episode (step 5.3)
    //    d. Check for dedup (step 5.4)
    //    e. INSERT or UPDATE episodes table
    //    f. UPDATE events SET needs_curation = FALSE
    // 3. Increment library_version sequence
    // 4. Return count of curated events
}
```

#### 5.3 Implement scoring algorithm

In `src/hub/curator/scoring.rs`:

```rust
pub fn score_episode(episode: &FleetEpisode) -> f64 {
    let outcome_weight = match &episode.outcome {
        EventOutcome::Resolved { .. } => 1.0,
        EventOutcome::Escalated { .. } => 0.7,
        EventOutcome::Pending => 0.2,
        EventOutcome::FalsePositive => 0.1,
    };

    let age_days = (now_secs - episode.timestamp) as f64 / 86400.0;
    let recency_weight = (-age_days / 180.0_f64).exp();

    let detail_weight = {
        let has_notes = if episode.resolution_summary.is_some() { 0.3 } else { 0.0 };
        let has_action = match &episode.outcome {
            EventOutcome::Resolved { action_taken } if !action_taken.is_empty() => 0.4,
            _ => 0.0,
        };
        let has_metrics = 0.3; // Always true for FleetEpisode
        has_notes + has_action + has_metrics
    };

    let diversity_weight = 0.5; // Placeholder, computed across library (step 5.5)

    outcome_weight * 0.50
        + recency_weight * 0.25
        + detail_weight * 0.15
        + diversity_weight * 0.10
}
```

#### 5.4 Implement deduplication

In `src/hub/curator/dedup.rs`:

```rust
pub async fn find_duplicate(pool: &PgPool, episode: &FleetEpisode) -> Result<Option<String>, sqlx::Error> {
    // Same rig + same category + depth within 100 ft + timestamps within 10 minutes
    sqlx::query_scalar::<_, String>(
        r#"SELECT id FROM episodes
           WHERE rig_id = $1
             AND category = $2
             AND ABS(depth_min - $3) < 100.0
             AND ABS(EXTRACT(EPOCH FROM (timestamp - $4::timestamptz))) < 600
             AND archived = FALSE
           LIMIT 1"#
    )
    .bind(&episode.rig_id)
    .bind(&episode.category.to_string())
    .bind(episode.depth_range.0)
    .bind(episode.timestamp_as_datetime())
    .fetch_optional(pool)
    .await
}
```

When a duplicate is found:
- Keep the episode with the better outcome
- Merge notes from both
- Update the existing episode's score

#### 5.5 Implement diversity scoring

Diversity prevents one rig or category from dominating the library:

```rust
pub async fn compute_diversity(pool: &PgPool, episode: &FleetEpisode) -> Result<f64, sqlx::Error> {
    // Count episodes in same category
    let category_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM episodes WHERE category = $1 AND archived = FALSE"
    ).bind(&episode.category.to_string()).fetch_one(pool).await?;

    let total_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM episodes WHERE archived = FALSE"
    ).fetch_one(pool).await?;

    if total_count == 0 { return Ok(1.0); }

    let category_fraction = category_count as f64 / total_count as f64;
    // Underrepresented categories get higher diversity score
    Ok(1.0 - category_fraction)
}
```

#### 5.6 Implement pruning

In `src/hub/curator/pruning.rs`:

```rust
pub async fn prune_old_episodes(pool: &PgPool, config: &HubConfig) -> Result<u32, sqlx::Error> {
    let mut pruned = 0;

    // Rule 1: Episodes older than 12 months → archive
    pruned += sqlx::query(
        "UPDATE episodes SET archived = TRUE WHERE timestamp < NOW() - INTERVAL '12 months' AND archived = FALSE"
    ).execute(pool).await?.rows_affected() as u32;

    // Rule 2: FalsePositive + age > 3 months → archive
    pruned += sqlx::query(
        "UPDATE episodes SET archived = TRUE WHERE outcome = 'FalsePositive' AND timestamp < NOW() - INTERVAL '3 months' AND archived = FALSE"
    ).execute(pool).await?.rows_affected() as u32;

    // Rule 3: Pending + age > 30 days → downgrade score
    sqlx::query(
        "UPDATE episodes SET score = 0.05 WHERE outcome = 'Pending' AND timestamp < NOW() - INTERVAL '30 days' AND score > 0.05"
    ).execute(pool).await?;

    // Rule 4: Total episodes > 50,000 → prune lowest-scored
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM episodes WHERE archived = FALSE")
        .fetch_one(pool).await?;
    if total > 50_000 {
        let to_prune = total - 50_000;
        pruned += sqlx::query(
            "UPDATE episodes SET archived = TRUE WHERE id IN (
                SELECT id FROM episodes WHERE archived = FALSE ORDER BY score ASC LIMIT $1
            )"
        ).bind(to_prune).execute(pool).await?.rows_affected() as u32;
    }

    Ok(pruned)
}
```

#### 5.7 Spawn curator on hub startup

In `src/bin/fleet_hub.rs`:

```rust
tokio::spawn(curator::run_curator(pool.clone(), config.clone()));
```

### Troubleshooting — Phase 5

| Problem | Likely Cause | Fix |
|---------|-------------|-----|
| Curator runs but no episodes created | `needs_curation` flag never set to TRUE | Verify the event ingestion (Phase 4) sets `needs_curation = TRUE` on INSERT. Check with `SELECT COUNT(*) FROM events WHERE needs_curation = TRUE`. |
| `FleetEpisode::from_event()` panics | Unexpected field values in event data | Add error handling around `from_event()`. Log and skip problematic events rather than crashing the curator. |
| Scoring produces NaN | Division by zero in diversity calculation | Guard against `total_count == 0`. The `(-age_days / 180.0).exp()` is always valid for finite age. |
| Deduplication too aggressive — merging unrelated events | Depth window (100 ft) or time window (10 min) too wide | Tune the thresholds. Start with 50 ft and 5 minutes, then widen if under-deduplicating. Log every merge for audit. |
| Deduplication too lax — library fills with near-identical episodes | Thresholds too narrow | Widen to 200 ft and 15 minutes. Also add same `well_id` as a dedup criterion. |
| Pruning archives too aggressively | 12-month cutoff too short for sparse fleets | Make pruning thresholds configurable. For small fleets, set `pruning_max_age_days = 730` (2 years). |
| Curator blocks the event API under load | Long-running curation queries hold DB connections | Run curation in a separate connection pool with lower priority. Use `SET statement_timeout = '30s'` in curation queries. |
| `library_version` sequence gets out of sync | Multiple curator instances running | Ensure only one curator instance runs. Use `pg_advisory_lock` at the start of each curation cycle. |
| Episode scores don't update when outcome changes | Outcome update (Phase 9) doesn't trigger re-curation | When an outcome PATCH arrives, set `needs_curation = TRUE` on the source event. The curator re-processes it. |

---

## Phase 6 — Library Sync Endpoint (Hub → Spoke)

### Goal
Implement `GET /api/fleet/library` with delta sync, zstd compression, and rig-specific filtering.

### Steps

#### 6.1 Implement delta query

```rust
async fn get_episodes_since(
    pool: &PgPool,
    since: Option<chrono::DateTime<chrono::Utc>>,
    requesting_rig_id: &str,
) -> Result<(Vec<FleetEpisode>, Vec<String>, u64, i64), sqlx::Error> {
    // Get new/updated episodes (excluding the requesting rig's own)
    let episodes = if let Some(since_ts) = since {
        sqlx::query_as(
            r#"SELECT * FROM episodes
               WHERE updated_at > $1
                 AND archived = FALSE
                 AND rig_id != $2
               ORDER BY score DESC"#
        )
        .bind(since_ts)
        .bind(requesting_rig_id)
        .fetch_all(pool).await?
    } else {
        // Full sync — all active episodes except requesting rig's
        sqlx::query_as(
            r#"SELECT * FROM episodes
               WHERE archived = FALSE AND rig_id != $1
               ORDER BY score DESC"#
        )
        .bind(requesting_rig_id)
        .fetch_all(pool).await?
    };

    // Get pruned IDs since last sync (episodes that were archived)
    let pruned_ids = if let Some(since_ts) = since {
        sqlx::query_scalar(
            "SELECT id FROM episodes WHERE archived = TRUE AND updated_at > $1"
        ).bind(since_ts).fetch_all(pool).await?
    } else {
        vec![]
    };

    // Get current library version and total count
    let version: i64 = sqlx::query_scalar("SELECT last_value FROM library_version_seq")
        .fetch_one(pool).await?;
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM episodes WHERE archived = FALSE")
        .fetch_one(pool).await?;

    Ok((episodes, pruned_ids, version as u64, total))
}
```

#### 6.2 Implement the handler

```rust
async fn get_library(
    State(hub): State<Arc<HubState>>,
    headers: HeaderMap,
    rig_id: String,  // extracted from auth
) -> Result<Response, StatusCode> {
    // Parse If-Modified-Since header
    let since = headers.get("if-modified-since")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<i64>().ok())
        .map(|ts| chrono::DateTime::from_timestamp(ts, 0).unwrap());

    let (episodes, pruned_ids, version, total) = get_episodes_since(&hub.db, since, &rig_id).await?;

    // If no changes, return 304
    if episodes.is_empty() && pruned_ids.is_empty() {
        return Ok(StatusCode::NOT_MODIFIED.into_response());
    }

    let response = LibraryResponse {
        version,
        episodes,
        total_fleet_episodes: total as u64,
        pruned_ids,
    };

    let json = serde_json::to_vec(&response)?;

    // Compress with zstd if client accepts it
    let accepts_zstd = headers.get("accept-encoding")
        .map(|v| v.to_str().unwrap_or("").contains("zstd"))
        .unwrap_or(false);

    if accepts_zstd {
        let compressed = zstd::encode_all(json.as_slice(), 3)?;
        Ok(Response::builder()
            .header("Content-Encoding", "zstd")
            .header("X-Library-Version", version.to_string())
            .header("X-Total-Episodes", total.to_string())
            .header("X-Delta-Count", episodes.len().to_string())
            .body(Body::from(compressed))?)
    } else {
        Ok(Response::builder()
            .header("Content-Type", "application/json")
            .header("X-Library-Version", version.to_string())
            .body(Body::from(json))?)
    }
}
```

#### 6.3 Log sync events

After a successful library response, record in `sync_log`:

```rust
sqlx::query(
    "INSERT INTO sync_log (rig_id, episodes_sent, library_version) VALUES ($1, $2, $3)"
)
.bind(&rig_id)
.bind(episodes.len() as i32)
.bind(version as i32)
.execute(&hub.db).await?;

// Update rig last_sync
sqlx::query("UPDATE rigs SET last_sync = NOW() WHERE rig_id = $1")
    .bind(&rig_id)
    .execute(&hub.db).await?;
```

#### 6.4 Implement library stats endpoint

```rust
async fn get_library_stats(State(hub): State<Arc<HubState>>) -> Json<LibraryStats> {
    let total = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM episodes WHERE archived = FALSE")
        .fetch_one(&hub.db).await.unwrap_or(0);

    let category_breakdown = sqlx::query_as::<_, (String, i64)>(
        "SELECT category, COUNT(*) FROM episodes WHERE archived = FALSE GROUP BY category"
    ).fetch_all(&hub.db).await.unwrap_or_default();

    let outcome_breakdown = sqlx::query_as::<_, (String, i64)>(
        "SELECT outcome, COUNT(*) FROM episodes WHERE archived = FALSE GROUP BY outcome"
    ).fetch_all(&hub.db).await.unwrap_or_default();

    Json(LibraryStats { total, category_breakdown, outcome_breakdown })
}
```

### Troubleshooting — Phase 6

| Problem | Likely Cause | Fix |
|---------|-------------|-----|
| Delta sync returns empty but there are new episodes | `If-Modified-Since` timestamp parsing failed | Log the raw header value. Ensure both sides use Unix timestamps (seconds since epoch), not RFC 2822 date strings. |
| 304 returned when there ARE changes | `updated_at` not being set on episode updates | Verify the `update_updated_at` trigger (Phase 2) fires on UPDATE. Test with `UPDATE episodes SET score = score WHERE id = 'test'; SELECT updated_at FROM episodes WHERE id = 'test';`. |
| Full sync is too large (> 50 MB) | Too many episodes in the library | The 50,000 episode limit (pruning) keeps library under ~250 MB. For 2,000 episodes, expect ~10 MB uncompressed, ~2 MB compressed. |
| zstd compression error: "frame content size is too large" | Attempting to compress an extremely large response | Set zstd compression level to 3 (fast). For very large payloads, use streaming compression. |
| Spoke receives episodes from its own rig | `rig_id != $2` filter not working | Verify the rig_id is extracted correctly from the auth layer. Log both the filter value and query results. |
| Pruned IDs list grows unboundedly | Archived episodes accumulate indefinitely | Periodically DELETE archived episodes older than 30 days (not just archive them). Or: only send pruned IDs from the last 7 days. |
| `library_version_seq` returns 0 | Sequence never incremented | The curator must call `SELECT nextval('library_version_seq')` after each successful curation run. |
| Concurrent sync requests cause DB contention | Many rigs syncing simultaneously at the 6-hour mark | Add jitter to spoke sync interval (±30 minutes). Use read replicas if contention is severe (unlikely for < 100 rigs). |

---

## Phase 7 — Rig Registry & API Key Authentication

### Goal
Implement rig registration, API key generation/validation, and authentication middleware.

### Steps

#### 7.1 Implement API key generation

In `src/hub/auth/api_key.rs`:

```rust
pub fn generate_api_key() -> String {
    let random_bytes: [u8; 32] = rand::random();
    format!("sk-fleet-{}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_bytes))
}

pub fn hash_api_key(key: &str) -> String {
    bcrypt::hash(key, bcrypt::DEFAULT_COST).expect("bcrypt hash failed")
}

pub fn verify_api_key(key: &str, hash: &str) -> bool {
    bcrypt::verify(key, hash).unwrap_or(false)
}
```

#### 7.2 Implement registration handler

```rust
async fn register_rig(
    State(hub): State<Arc<HubState>>,
    admin_auth: AdminAuth,  // Middleware checks admin credentials
    Json(req): Json<RegisterRigRequest>,
) -> Result<(StatusCode, Json<RegisterRigResponse>), (StatusCode, Json<ErrorResponse>)> {
    let api_key = generate_api_key();
    let key_hash = hash_api_key(&api_key);

    sqlx::query(
        "INSERT INTO rigs (rig_id, api_key_hash, well_id, field) VALUES ($1, $2, $3, $4)"
    )
    .bind(&req.rig_id)
    .bind(&key_hash)
    .bind(&req.well_id)
    .bind(&req.field)
    .execute(&hub.db).await
    .map_err(|e| {
        if e.to_string().contains("duplicate key") {
            (StatusCode::CONFLICT, Json(ErrorResponse { error: "Rig already registered".into() }))
        } else {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() }))
        }
    })?;

    Ok((StatusCode::CREATED, Json(RegisterRigResponse {
        rig_id: req.rig_id,
        api_key,  // Returned in plaintext ONCE
    })))
}
```

#### 7.3 Implement revocation handler

```rust
async fn revoke_rig(
    State(hub): State<Arc<HubState>>,
    admin_auth: AdminAuth,
    Path(rig_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let result = sqlx::query(
        "UPDATE rigs SET status = 'revoked' WHERE rig_id = $1 AND status = 'active'"
    )
    .bind(&rig_id)
    .execute(&hub.db).await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if result.rows_affected() == 0 {
        Err(StatusCode::NOT_FOUND)
    } else {
        Ok(StatusCode::OK)
    }
}
```

#### 7.4 Implement authentication middleware

Create an Axum extractor that validates the `Authorization: Bearer <key>` header:

```rust
pub struct RigAuth {
    pub rig_id: String,
}

#[async_trait]
impl FromRequestParts<Arc<HubState>> for RigAuth {
    type Rejection = (StatusCode, Json<ErrorResponse>);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<HubState>,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts.headers.get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or((StatusCode::UNAUTHORIZED, Json(ErrorResponse { error: "Missing Bearer token".into() })))?;

        // Look up all active rigs and check key (cache this in production)
        let rigs = sqlx::query_as::<_, (String, String)>(
            "SELECT rig_id, api_key_hash FROM rigs WHERE status = 'active'"
        )
        .fetch_all(&state.db).await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "DB error".into() })))?;

        for (rig_id, hash) in &rigs {
            if verify_api_key(auth_header, hash) {
                return Ok(RigAuth { rig_id: rig_id.clone() });
            }
        }

        Err((StatusCode::FORBIDDEN, Json(ErrorResponse { error: "Invalid API key".into() })))
    }
}
```

#### 7.5 Add rig_id cross-check on event uploads

In the event upload handler, verify the authenticated rig matches the event:

```rust
if auth.rig_id != event.rig_id {
    return Err((StatusCode::FORBIDDEN, "API key does not match rig_id in event"));
}
```

#### 7.6 Implement admin authentication

For dashboard and registry endpoints, use a separate admin key:

```rust
pub struct AdminAuth;

// Validates against FLEET_ADMIN_KEY environment variable
```

#### 7.7 Cache API key lookups

bcrypt verification is intentionally slow (~100ms). Cache verified keys:

```rust
// In HubState:
pub api_key_cache: RwLock<HashMap<String, (String, Instant)>>,  // key → (rig_id, expires_at)
```

TTL: 5 minutes. On revocation, clear cache.

### Troubleshooting — Phase 7

| Problem | Likely Cause | Fix |
|---------|-------------|-----|
| Registration returns 500: "bcrypt hash failed" | bcrypt cost factor too high for system | Default cost (12) is fine. If on extremely slow hardware, reduce to 10. Never below 10. |
| API key verification is slow (>200ms per request) | bcrypt is slow by design | Implement the key cache (step 7.7). After first successful verification, serve from cache for 5 minutes. |
| All requests return 401 after restart | API key cache was in-memory, cleared on restart | Cache is just an optimization. The DB lookup should work. Check DB connectivity. |
| Rig registration fails: "duplicate key" | Rig already registered with that ID | Return 409 Conflict with a clear message. Admin should revoke the old rig first if re-registering. |
| Admin key compromised | Admin key stored insecurely | Rotate the key via env var `FLEET_ADMIN_KEY`. Update all admin clients. No DB changes needed. |
| API key returned as plaintext in registration response | By design — key is only shown once | Warn admin to save the key immediately. If lost, revoke and re-register the rig. |
| `Authorization` header stripped by reverse proxy | Proxy not forwarding auth headers | Configure nginx/Apache to pass `Authorization` header. Use `proxy_set_header Authorization $http_authorization;`. |
| Cache poisoning: wrong rig_id cached for a key | Bug in cache logic | Clear cache on any auth failure. Use key hash as cache key, not plaintext. |

---

## Phase 8 — Spoke-Side Clients (FleetClient, LibrarySync, OutcomeForwarder)

### Goal
Build the three spoke-side background services that connect to the hub.

### Steps

#### 8.1 Add fleet client configuration

Add to `well_config.toml` (or a new `[fleet]` section):

```toml
[fleet]
enabled = false
hub_url = "https://10.0.0.1:8080"
api_key = ""           # Set via FLEET_API_KEY env var (preferred)
rig_id = ""            # Auto-detected from well config or set explicitly
upload_interval_secs = 300    # 5 minutes
sync_interval_secs = 21600   # 6 hours
sync_jitter_secs = 1800      # ±30 minutes random jitter
```

#### 8.2 Implement `FleetClient` in `src/fleet/client.rs`

```rust
pub struct FleetClient {
    http: reqwest::Client,
    hub_url: String,
    api_key: String,
    rig_id: String,
}

impl FleetClient {
    pub fn new(hub_url: &str, api_key: &str, rig_id: &str) -> Self { ... }

    /// Upload a single event to the hub. Returns Ok(true) if accepted, Ok(false) if duplicate.
    pub async fn upload_event(&self, event: &FleetEvent) -> Result<bool, FleetClientError> {
        let json = serde_json::to_vec(event)?;
        let compressed = zstd::encode_all(json.as_slice(), 3)?;

        let resp = self.http.post(format!("{}/api/fleet/events", self.hub_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("Content-Encoding", "zstd")
            .header("X-Rig-ID", &self.rig_id)
            .body(compressed)
            .send()
            .await?;

        match resp.status() {
            StatusCode::CREATED => Ok(true),
            StatusCode::CONFLICT => Ok(false),  // Duplicate, safe to mark_uploaded
            status => Err(FleetClientError::ServerError(status)),
        }
    }

    /// Update event outcome on the hub
    pub async fn update_outcome(&self, event_id: &str, outcome: &EventOutcome) -> Result<(), FleetClientError> { ... }

    /// Pull library delta from hub
    pub async fn sync_library(&self, since: Option<u64>) -> Result<LibraryResponse, FleetClientError> { ... }
}
```

#### 8.3 Implement upload background task

In `src/fleet/client.rs` or a new `src/fleet/uploader.rs`:

```rust
pub async fn run_uploader(queue: Arc<UploadQueue>, client: FleetClient, interval_secs: u64) {
    let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
    loop {
        interval.tick().await;
        match queue.drain() {
            Ok(events) if events.is_empty() => continue,
            Ok(events) => {
                for event in &events {
                    match client.upload_event(event).await {
                        Ok(_) => {
                            let _ = queue.mark_uploaded(&event.id);
                            tracing::info!("Uploaded fleet event: {}", event.id);
                        }
                        Err(e) => {
                            tracing::warn!("Failed to upload {}: {}. Will retry.", event.id, e);
                            break;  // Stop on first failure, retry next cycle
                        }
                    }
                }
            }
            Err(e) => tracing::error!("Failed to drain upload queue: {}", e),
        }
    }
}
```

#### 8.4 Implement `LibrarySync` background task

In `src/fleet/sync.rs`:

```rust
pub async fn run_library_sync(
    client: FleetClient,
    ram_recall: Arc<RAMRecall>,
    interval_secs: u64,
    jitter_secs: u64,
) {
    let mut last_sync: Option<u64> = None;

    loop {
        // Sleep with jitter
        let jitter = rand::thread_rng().gen_range(0..jitter_secs);
        tokio::time::sleep(Duration::from_secs(interval_secs + jitter)).await;

        match client.sync_library(last_sync).await {
            Ok(library) => {
                tracing::info!(
                    "Library sync: {} new episodes, version {}, {} total fleet",
                    library.episodes.len(), library.version, library.total_fleet_episodes
                );

                // Load new episodes into RAMRecall
                // For delta sync, we need to ADD, not replace
                for episode in &library.episodes {
                    ram_recall.add_episode(episode.clone());
                }

                // Remove pruned episodes
                // (Requires adding a remove_episode method to RAMRecall)

                last_sync = Some(chrono::Utc::now().timestamp() as u64);
            }
            Err(FleetClientError::NotModified) => {
                tracing::debug!("Library sync: no changes");
            }
            Err(e) => {
                tracing::warn!("Library sync failed: {}. Will retry.", e);
            }
        }
    }
}
```

#### 8.5 Add `remove_episode()` to RAMRecall

In `src/context/ram_recall.rs`, add:

```rust
pub fn remove_episodes(&self, ids: &[String]) {
    let mut episodes = self.episodes.write().unwrap();
    episodes.retain(|ep| !ids.contains(&ep.id));
}
```

#### 8.6 Wire up fleet services in `main.rs`

In the main SAIREN-OS startup (not the hub binary):

```rust
if fleet_config.enabled {
    let client = FleetClient::new(&fleet_config.hub_url, &fleet_config.api_key, &fleet_config.rig_id);
    let queue = Arc::new(UploadQueue::open(&fleet_config.queue_dir)?);

    tokio::spawn(run_uploader(queue.clone(), client.clone(), fleet_config.upload_interval_secs));
    tokio::spawn(run_library_sync(client.clone(), ram_recall.clone(), fleet_config.sync_interval_secs, fleet_config.sync_jitter_secs));
}
```

### Troubleshooting — Phase 8

| Problem | Likely Cause | Fix |
|---------|-------------|-----|
| `reqwest` connection refused | Hub not running or wrong URL | Check `hub_url` config. Verify hub is listening: `curl http://<hub_url>/api/fleet/health`. |
| TLS handshake failure | Self-signed cert or missing CA | Use `reqwest::ClientBuilder::danger_accept_invalid_certs(true)` for dev. In production, install the WireGuard CA cert. Or use HTTP over WireGuard (already encrypted). |
| Uploads succeed but events don't appear on hub | Hub returns 201 but doesn't store | Check hub logs. Verify the event passes validation. Check DB for the event ID. |
| Upload queue grows indefinitely | Hub is unreachable | Expected behavior — queue is disk-backed and survives outages. Monitor `pending_count()`. Alert if > 100. Events auto-evict at 1000. |
| Library sync returns 304 but RAMRecall is empty | First sync sends wrong `If-Modified-Since` | On first sync, don't send `If-Modified-Since` (request full library). Set `last_sync = None` initially. |
| `add_episode()` causes duplicates in RAMRecall | Same episode received in multiple syncs | RAMRecall `add_episode()` already deduplicates by ID. Verify this works. |
| `remove_episodes()` deadlocks | Write lock held while reading | The `RwLock` pattern should prevent this. Don't hold the read lock while calling `remove_episodes()`. |
| Sync jitter causes all rigs to sync at the same offset | Jitter seeded with same value | Use `rand::thread_rng()` which is seeded from system entropy. Each rig gets different jitter. |
| Memory growth from accumulated episodes | Episodes never removed, only added | `load_episodes()` has a MAX_EPISODES (10,000) limit. Delta syncs add but pruned IDs remove. Monitor `episode_count()`. |
| Fleet config not found / not loaded | Missing `[fleet]` section in config | Default `fleet.enabled = false`. Only activate when hub is deployed. Log a startup message indicating fleet status. |

---

## Phase 9 — Outcome Forwarding (Spoke → Hub PATCH)

### Goal
When a driller acknowledges an advisory on the rig dashboard, forward the outcome to the hub so the episode can be re-scored.

### Steps

#### 9.1 Implement the hub PATCH handler

In `src/hub/api/events.rs`:

```rust
async fn update_outcome(
    State(hub): State<Arc<HubState>>,
    auth: RigAuth,
    Path(event_id): Path<String>,
    Json(req): Json<UpdateOutcomeRequest>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    // Verify the event belongs to the authenticated rig
    let event_rig: Option<String> = sqlx::query_scalar(
        "SELECT rig_id FROM events WHERE id = $1"
    ).bind(&event_id).fetch_optional(&hub.db).await?;

    match event_rig {
        None => return Err((StatusCode::NOT_FOUND, "Event not found")),
        Some(rig) if rig != auth.rig_id => return Err((StatusCode::FORBIDDEN, "Not your event")),
        _ => {}
    }

    // Update event outcome
    sqlx::query(
        "UPDATE events SET outcome = $1, action_taken = $2, notes = $3, needs_curation = TRUE WHERE id = $4"
    )
    .bind(&req.outcome)
    .bind(&req.action_taken)
    .bind(&req.notes)
    .bind(&event_id)
    .execute(&hub.db).await?;

    // needs_curation = TRUE triggers the curator to re-score the episode

    Ok(StatusCode::OK)
}
```

#### 9.2 Implement `OutcomeForwarder` on the spoke

Hook into the existing `POST /api/v1/advisory/acknowledge` handler:

```rust
// In src/api/handlers.rs, after processing the acknowledgment locally:
if let Some(fleet_client) = &state.fleet_client {
    let event_id = format!("{}-{}", rig_id, advisory_timestamp);
    let outcome = EventOutcome::Resolved {
        action_taken: req.action_taken.unwrap_or_default(),
    };
    // Fire-and-forget — don't block the driller's response
    let client = fleet_client.clone();
    tokio::spawn(async move {
        if let Err(e) = client.update_outcome(&event_id, &outcome).await {
            tracing::warn!("Failed to forward outcome to hub: {}", e);
            // Could queue for retry, but outcomes are best-effort
        }
    });
}
```

#### 9.3 Handle outcome types

Map the driller's acknowledgment to `EventOutcome`:

| Driller Action | EventOutcome |
|---------------|-------------|
| "Acknowledged" with action description | `Resolved { action_taken: "..." }` |
| "Escalated" with reason | `Escalated { reason: "..." }` |
| "Dismissed" / "False alarm" | `FalsePositive` |
| No acknowledgment after 48h | Stays `Pending` (curator downgrades score after 30 days) |

#### 9.4 Test outcome flow end-to-end

1. Upload event from spoke → hub (201 Created, outcome = Pending)
2. Episode created by curator (score reflects Pending: 0.2 outcome weight)
3. Driller acknowledges on rig dashboard
4. PATCH sent to hub (outcome = Resolved)
5. Curator re-processes → episode score increases (1.0 outcome weight)
6. Next library sync → other rigs receive the improved episode

### Troubleshooting — Phase 9

| Problem | Likely Cause | Fix |
|---------|-------------|-----|
| PATCH returns 404 for a valid event | Event not yet uploaded (driller acknowledged before upload cycle) | Queue the outcome locally. When the event is uploaded, send the outcome PATCH afterward. |
| PATCH returns 403 "Not your event" | Rig ID mismatch between event and API key | Verify `event_id` format matches `{rig_id}-{timestamp}`. Check that the same API key is used for upload and outcome. |
| Outcome forwarding fails silently | `tokio::spawn` swallows errors | Log errors inside the spawned task. Consider a retry queue for important outcomes (Resolved especially). |
| Curator doesn't re-score after outcome update | `needs_curation` not set to TRUE on PATCH | Verify the UPDATE query sets `needs_curation = TRUE`. The curator picks this up on its next cycle. |
| Multiple PATCH requests for the same event | Driller clicks acknowledge multiple times | Make PATCH idempotent — last write wins. The latest outcome overwrites the previous. |
| Outcome update arrives before the event | Race condition: acknowledge is fast, upload waits for cycle | Return 404 from hub. Spoke should retry on next upload cycle. Or: queue outcome updates in `UploadQueue` alongside events. |

---

## Phase 10 — Fleet Dashboard

### Goal
Build a static HTML + JS dashboard for fleet-wide visibility, served by the hub.

### Steps

#### 10.1 Create dashboard API handlers

In `src/hub/api/dashboard.rs`:

**Summary endpoint:**
```rust
async fn get_summary(State(hub): State<Arc<HubState>>) -> Json<DashboardSummary> {
    // Active rigs (contacted in last 48h)
    // Events today (count since midnight UTC)
    // Top 5 categories by event count
    // Library version and total episodes
}
```

**Trends endpoint:**
```rust
async fn get_trends(State(hub): State<Arc<HubState>>, Query(params): Query<TrendParams>) -> Json<Vec<TrendPoint>> {
    // Time series: events per day, grouped by category
    // Configurable date range (default: last 30 days)
    // Returns: [{ date, category, count }]
}
```

**Outcomes endpoint:**
```rust
async fn get_outcomes(State(hub): State<Arc<HubState>>) -> Json<OutcomeAnalytics> {
    // Resolution rate: (Resolved + Escalated) / Total
    // Most common actions taken (grouped by action_taken text)
    // Average time from event to resolution (where available)
    // Breakdown by category
}
```

#### 10.2 Create static HTML dashboard

Create `static/fleet_dashboard.html`:

**Views to implement:**
1. **Fleet Overview** — Active rigs table, events/day chart (Chart.js), category pie chart, outcome distribution
2. **Rig Detail** — Click a rig to see its event history, advisory timeline, outcome rate, sync status
3. **Library Health** — Episode count, score histogram, category coverage, rig contribution balance
4. **Trend Analysis** — Fleet anomaly trends over time, depth-correlated density, campaign comparison

#### 10.3 Use Chart.js for visualization

Embed Chart.js via CDN (or bundle locally for air-gapped environments):
- Line chart: Events per day over time
- Pie chart: Category breakdown
- Bar chart: Outcome distribution
- Histogram: Episode score distribution
- Scatter plot: Depth vs anomaly density

#### 10.4 Serve the dashboard from the hub

```rust
// In the hub router:
.route("/", get(serve_dashboard))
.route("/fleet_dashboard.html", get(serve_dashboard))
.nest_service("/static", ServeDir::new("static"))
```

#### 10.5 Auto-refresh

Dashboard should poll the summary endpoint every 60 seconds. Use `setInterval` + `fetch()`.

### Troubleshooting — Phase 10

| Problem | Likely Cause | Fix |
|---------|-------------|-----|
| Dashboard shows "0 active rigs" when rigs are connected | `last_seen` not updated on event upload or sync | Verify event ingestion and sync handlers update `rigs.last_seen`. Check: `SELECT rig_id, last_seen FROM rigs`. |
| Charts don't render | Chart.js CDN blocked in air-gapped environment | Bundle Chart.js locally in `static/js/chart.min.js`. |
| Trends query is slow (>5 seconds) | Missing index on `events.timestamp` | Verify `idx_events_timestamp` index exists. For date-range queries, consider a materialized view refreshed hourly. |
| Dashboard CORS error | Hub CORS not configured for dashboard origin | If dashboard is served from the hub itself (same origin), CORS is not needed. If separate, add the origin to CorsLayer. |
| Outcome analytics show 0% resolution | No outcomes have been forwarded yet | Expected during early deployment. Display "No outcome data yet" rather than 0%. |
| Dashboard fails to load in older browsers | Modern JS features (async/await, fetch) not supported | Use ES5-compatible code or add polyfills. Target IE11+ if required by operator IT policy. |
| Auto-refresh causes memory leak | `setInterval` + growing DOM elements | Clear previous chart data before re-rendering. Use `chart.destroy()` before creating a new chart instance. |

---

## Phase 11 — Network & Security (WireGuard)

### Goal
Set up WireGuard VPN for secure hub-spoke communication.

### Steps

#### 11.1 Generate WireGuard keys

On the hub:
```bash
wg genkey | tee hub_private.key | wg pubkey > hub_public.key
```

On each rig:
```bash
wg genkey | tee rig_private.key | wg pubkey > rig_public.key
```

#### 11.2 Configure hub WireGuard

Create `/etc/wireguard/wg0.conf` on the hub:
```ini
[Interface]
Address = 10.0.0.1/24
ListenPort = 51820
PrivateKey = <hub_private_key>

# Rig A
[Peer]
PublicKey = <rig_a_public_key>
AllowedIPs = 10.0.1.1/32

# Rig B
[Peer]
PublicKey = <rig_b_public_key>
AllowedIPs = 10.0.1.2/32
```

#### 11.3 Configure rig WireGuard

Create `/etc/wireguard/wg0.conf` on each rig:
```ini
[Interface]
Address = 10.0.1.X/32
PrivateKey = <rig_private_key>

[Peer]
PublicKey = <hub_public_key>
Endpoint = <hub_public_ip>:51820
AllowedIPs = 10.0.0.1/32
PersistentKeepalive = 25
```

#### 11.4 Start and enable

```bash
wg-quick up wg0
systemctl enable wg-quick@wg0
```

#### 11.5 Firewall rules

On the hub:
```bash
# Allow WireGuard
ufw allow 51820/udp

# Allow Fleet Hub API only from WireGuard subnet
ufw allow from 10.0.0.0/16 to any port 8080

# Deny Fleet Hub API from public internet
ufw deny 8080
```

#### 11.6 Bind hub to WireGuard interface

```bash
fleet-hub --bind-address 10.0.0.1:8080
```

#### 11.7 Test connectivity

```bash
# From rig:
ping 10.0.0.1
curl http://10.0.0.1:8080/api/fleet/health
```

### Troubleshooting — Phase 11

| Problem | Likely Cause | Fix |
|---------|-------------|-----|
| WireGuard handshake fails | Keys not matching, wrong endpoint | Verify public keys are correctly exchanged. Check `wg show` for handshake status. Ensure UDP 51820 is reachable. |
| Tunnel up but no traffic flows | `AllowedIPs` misconfigured | Hub must have each rig's IP in `AllowedIPs`. Rig must have hub's IP. Check with `wg show wg0`. |
| Connection drops on satellite links | Keepalive not configured | Set `PersistentKeepalive = 25` on the rig side. This sends a packet every 25 seconds to keep NAT mappings alive. |
| High latency (>500ms) on satellite | Expected for VSAT links | WireGuard adds minimal overhead (~60 bytes/packet). The 500ms is the satellite latency itself. Upload retries and sync intervals account for this. |
| Hub unreachable after IP change | Hub endpoint changed | Update `Endpoint` on all rig configs. Consider using a DNS name instead of IP for the hub. |
| "RTNETLINK answers: Operation not permitted" | Missing kernel module or permissions | Install `wireguard-tools` and ensure `wireguard` kernel module is loaded: `modprobe wireguard`. |
| Multiple rigs get same IP | IP assignment collision | Maintain a central IP registry. Use the rig_id to deterministically assign IPs (e.g., hash rig_id → IP offset). |
| WireGuard doesn't survive reboot | systemd service not enabled | Run `systemctl enable wg-quick@wg0`. Verify with `systemctl is-enabled wg-quick@wg0`. |

---

## Phase 12 — Integration Testing & End-to-End Validation

### Goal
Validate the complete flow: event creation → upload → curation → library sync → RAMRecall query.

### Steps

#### 12.1 Set up test environment

```bash
# Start PostgreSQL (Docker)
docker run -d --name fleet-pg -e POSTGRES_DB=sairen_fleet_test -e POSTGRES_PASSWORD=test -p 5433:5432 postgres:16

# Run migrations
DATABASE_URL=postgres://postgres:test@localhost:5433/sairen_fleet_test sqlx migrate run

# Start hub
cargo run --bin fleet-hub --features fleet-hub -- --database-url postgres://postgres:test@localhost:5433/sairen_fleet_test --port 8090
```

#### 12.2 Register a test rig

```bash
curl -X POST http://localhost:8090/api/fleet/rigs/register \
  -H "Authorization: Bearer <admin-key>" \
  -H "Content-Type: application/json" \
  -d '{"rig_id": "RIG-TEST-1", "well_id": "WELL-001", "field": "Test Basin"}'
```

Save the returned API key.

#### 12.3 Upload a test event

```bash
curl -X POST http://localhost:8090/api/fleet/events \
  -H "Authorization: Bearer <rig-api-key>" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "RIG-TEST-1-1707400000",
    "rig_id": "RIG-TEST-1",
    "well_id": "WELL-001",
    "field": "Test Basin",
    "campaign": "Production",
    "advisory": { ... },
    "history_window": [{ ... }],
    "outcome": "Pending",
    "depth": 12450.0,
    "timestamp": 1707400000
  }'
```

Expected: 201 Created

#### 12.4 Verify curation

Wait for the curator cycle (or trigger manually), then:

```bash
curl http://localhost:8090/api/fleet/library/stats \
  -H "Authorization: Bearer <rig-api-key>"
```

Expected: `{ "total": 1, ... }`

#### 12.5 Sync library from a second rig

Register RIG-TEST-2, then:

```bash
curl http://localhost:8090/api/fleet/library \
  -H "Authorization: Bearer <rig-2-api-key>" \
  -H "Accept-Encoding: zstd"
```

Expected: Response contains the episode from RIG-TEST-1.

#### 12.6 Update outcome

```bash
curl -X PATCH http://localhost:8090/api/fleet/events/RIG-TEST-1-1707400000/outcome \
  -H "Authorization: Bearer <rig-1-api-key>" \
  -H "Content-Type: application/json" \
  -d '{"outcome": "Resolved", "action_taken": "Reduced WOB by 5 klbs", "notes": "Pack-off cleared"}'
```

Expected: 200 OK. After curator re-runs, episode score increases.

#### 12.7 Write automated integration tests

Create `tests/fleet_integration.rs`:

```rust
#[tokio::test]
async fn test_full_fleet_cycle() {
    // 1. Start hub with test DB
    // 2. Register two rigs
    // 3. Upload event from rig 1
    // 4. Wait for curation
    // 5. Sync library from rig 2 → verify episode received
    // 6. Update outcome from rig 1
    // 7. Wait for re-curation
    // 8. Sync library from rig 2 → verify score increased
}

#[tokio::test]
async fn test_dedup_duplicate_event() { ... }

#[tokio::test]
async fn test_revoked_rig_rejected() { ... }

#[tokio::test]
async fn test_delta_sync_excludes_own_events() { ... }

#[tokio::test]
async fn test_pruning_archives_old_episodes() { ... }
```

### Troubleshooting — Phase 12

| Problem | Likely Cause | Fix |
|---------|-------------|-----|
| Integration tests flaky due to curation timing | Curator runs on interval, may not have processed yet | In tests, trigger curation manually after each event insert. Or: poll the episodes table until the episode appears (with timeout). |
| Test DB state leaks between tests | Tests share the same database | Use a fresh DB per test (create/drop), or wrap each test in a transaction that rolls back. |
| Docker PostgreSQL connection refused | Container not ready yet | Add a health check: `pg_isready -h localhost -p 5433`. Wait 5 seconds after `docker run`. |
| Serialization mismatch between test payload and real FleetEvent | Test uses hand-crafted JSON that doesn't match the struct | Use `serde_json::to_value(&real_fleet_event)` to generate test payloads from actual structs. |
| Library sync test fails: "no episodes" | Curator hasn't run yet OR episode from same rig (excluded) | Verify the sync is from a DIFFERENT rig. Verify curation has completed. |
| Tests pass locally, fail in CI | Missing PostgreSQL in CI environment | Add PostgreSQL service to CI config. Or: use `sqlx::testing` with a test database pool. |

---

## Phase 13 — Deployment & Operations

### Goal
Deploy the hub for production use.

### Steps

#### 13.1 Create systemd service for the hub

Create `deploy/fleet-hub.service`:

```ini
[Unit]
Description=SAIREN Fleet Hub
After=network.target postgresql.service
Requires=postgresql.service

[Service]
Type=simple
User=sairen
Group=sairen
ExecStart=/usr/local/bin/fleet-hub --database-url postgres:///sairen_fleet --port 8080 --bind-address 10.0.0.1
Restart=always
RestartSec=5
Environment=FLEET_ADMIN_KEY=<admin-key>
Environment=RUST_LOG=info,fleet_hub=debug

[Install]
WantedBy=multi-user.target
```

#### 13.2 Set up PostgreSQL backups

```bash
# Daily backup cron job
0 2 * * * pg_dump sairen_fleet | gzip > /backups/sairen_fleet_$(date +\%Y\%m\%d).sql.gz

# Keep 30 days of backups
find /backups -name "sairen_fleet_*.sql.gz" -mtime +30 -delete
```

#### 13.3 Set up monitoring

Key metrics to monitor:
- Hub process health (systemd status)
- PostgreSQL connectivity (health endpoint)
- Events received per day (dashboard summary)
- Active vs inactive rigs
- Library version (should increment)
- Disk usage
- Curator run time and success/failure

#### 13.4 Create install script

Create `deploy/install_hub.sh`:
1. Install PostgreSQL
2. Create database and user
3. Run migrations
4. Install fleet-hub binary
5. Generate admin key
6. Create systemd service
7. Start services

#### 13.5 Spoke deployment updates

Update `deploy/install.sh` to:
1. Add `[fleet]` section to well_config.toml (disabled by default)
2. Document how to enable fleet mode
3. Provide instructions for getting an API key from the hub admin

#### 13.6 Document operational runbooks

Create operational guides for:
- Adding a new rig to the fleet
- Revoking a compromised rig
- Restoring from backup
- Migrating hub to a new server
- Upgrading hub software

### Troubleshooting — Phase 13

| Problem | Likely Cause | Fix |
|---------|-------------|-----|
| Hub crashes on startup in production | Database URL wrong or DB not running | Check `journalctl -u fleet-hub`. Verify PostgreSQL is running: `systemctl status postgresql`. Test connection: `psql postgres:///sairen_fleet`. |
| Hub runs but rigs can't connect | Firewall blocking port 8080 | Check `ufw status`. Ensure port 8080 is allowed from the WireGuard subnet. |
| Disk fills up over time | Event payload storage growing | Monitor with `du -sh /var/lib/postgresql/`. Archive old events. The estimates show ~11 GB/year for 50 rigs, well within a 100 GB SSD. |
| PostgreSQL out of memory | Too many concurrent connections | Set `max_connections = 50` in `postgresql.conf`. The hub pool should be 10-20 connections. |
| Hub performance degrades over months | Table bloat from frequent updates | Run `VACUUM ANALYZE events; VACUUM ANALYZE episodes;` weekly. Add to cron. |
| Backup restore fails | Version mismatch between backup and current schema | Always run migrations after restoring: `sqlx migrate run`. Migrations are idempotent. |
| Systemd restarts hub in a loop | Configuration error causing immediate crash | Check `journalctl -u fleet-hub -n 50`. Fix the config issue. Use `RestartSec=5` to avoid tight restart loops. |
| Admin key lost | Not stored securely | Regenerate by setting a new `FLEET_ADMIN_KEY` env var and restarting the hub. Old admin key becomes invalid. |

---

## Open Questions — Decision Log

Track decisions on the open questions from the design document here as they are resolved:

| # | Question | Decision | Rationale | Date |
|---|----------|----------|-----------|------|
| Q1 | Hub: same repo or separate? | **Same repo** (`src/bin/fleet_hub.rs`) | Shares `FleetEvent`/`FleetEpisode` types, prevents schema drift. Acceptable dependency overhead. | — |
| Q2 | Require outcomes before library entry? | **No — create immediately** | Faster learning loop. Pending episodes have low score (0.2) anyway. Outcomes improve score when they arrive. | — |
| Q3 | Rig precedent rejection? | **Defer to v2** | Adds complexity. For v1, trust the curator's scoring. Revisit if operators request it. | — |
| Q4 | Formation-specific precedents? | **Defer — add `formation_type` field later** | Requires formation data not always available in WITS. Add as optional field in a future migration. | — |
| Q5 | Multi-tenant hub? | **Separate hubs per operator** | Data sovereignty and competitive concerns. Simpler deployment. Multi-tenant can be revisited for managed service model. | — |
| Q6 | Episode versioning? | **No — last write wins** | Simpler. The curator re-scores on outcome update. Audit trail exists in the `events` table (raw data preserved). | — |

---

## Phase Dependency Graph

```
Phase 1 (Scaffolding)
  │
  ├──► Phase 2 (Schema)
  │       │
  │       ├──► Phase 3 (API Skeleton)
  │       │       │
  │       │       ├──► Phase 4 (Event Ingestion) ◄── Phase 7 (Auth)
  │       │       │       │
  │       │       │       └──► Phase 5 (Curator)
  │       │       │               │
  │       │       │               └──► Phase 6 (Library Sync)
  │       │       │                       │
  │       │       │                       └──► Phase 8 (Spoke Clients)
  │       │       │                               │
  │       │       │                               └──► Phase 9 (Outcome Forwarding)
  │       │       │
  │       │       └──► Phase 10 (Dashboard)
  │       │
  │       └──► Phase 7 (Auth) ── can start in parallel with Phase 3
  │
  └──► Phase 11 (WireGuard) ── independent, can start anytime

  Phase 12 (Integration Testing) ── after Phases 4-9
  Phase 13 (Deployment) ── after Phase 12
```

---

## Estimated Effort Summary

| Phase | Component | Estimate | Priority |
|-------|-----------|----------|----------|
| 1 | Project Scaffolding | 1 day | Must have |
| 2 | PostgreSQL Schema | 1 day | Must have |
| 3 | Hub API Skeleton | 1-2 days | Must have |
| 4 | Event Ingestion | 1-2 days | Must have |
| 5 | Library Curator | 2 days | Must have |
| 6 | Library Sync | 1-2 days | Must have |
| 7 | Auth & Registry | 1 day | Must have |
| 8 | Spoke Clients | 2 days | Must have |
| 9 | Outcome Forwarding | 1 day | Should have |
| 10 | Dashboard | 3-5 days | Nice to have |
| 11 | WireGuard | 1-2 days | Environment dependent |
| 12 | Integration Testing | 2-3 days | Must have |
| 13 | Deployment | 1-2 days | Must have |
| **Total** | | **18-28 days** | |

---

*End of Fleet Hub Implementation Plan.*
