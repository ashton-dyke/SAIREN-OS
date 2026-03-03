//! `SQLite` event store for gossip events.
//!
//! Stores `FleetEvent`s in an embedded `SQLite` database (WAL mode) with
//! indexed columns for fast structured queries. The full event is stored
//! as a zstd-compressed JSON blob in the `data` column.

use crate::fleet::types::FleetEvent;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use tracing::warn;

/// Maximum events to retain.
const RETENTION_CAP: usize = 50_000;
/// Age limit in seconds (12 months).
const RETENTION_AGE_SECS: u64 = 365 * 86400;
/// False positive cleanup age in seconds (3 months).
const FALSE_POSITIVE_AGE_SECS: u64 = 90 * 86400;

/// Embedded `SQLite` event store.
pub struct EventStore {
    conn: Connection,
}

#[allow(clippy::missing_errors_doc)]
impl EventStore {
    /// Open (or create) the event store at the given path.
    ///
    /// Runs schema migration and enables WAL mode.
    pub fn open(path: &Path) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS events (
                id            TEXT PRIMARY KEY,
                rig_id        TEXT NOT NULL,
                well_id       TEXT NOT NULL,
                timestamp     INTEGER NOT NULL,
                last_modified INTEGER NOT NULL,
                formation     TEXT,
                depth_ft      REAL,
                category      TEXT NOT NULL,
                severity      TEXT NOT NULL,
                risk_level    TEXT,
                outcome       TEXT DEFAULT 'Pending',
                action_taken  TEXT,
                data          BLOB NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_events_formation     ON events(formation, timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_events_category      ON events(category, timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_events_timestamp     ON events(timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_events_depth         ON events(depth_ft);
            CREATE INDEX IF NOT EXISTS idx_events_last_modified ON events(last_modified);",
        )?;
        Ok(Self { conn })
    }

    /// Open an in-memory store (for testing).
    #[allow(dead_code)]
    pub fn open_in_memory() -> Result<Self, rusqlite::Error> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
        store.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS events (
                id            TEXT PRIMARY KEY,
                rig_id        TEXT NOT NULL,
                well_id       TEXT NOT NULL,
                timestamp     INTEGER NOT NULL,
                last_modified INTEGER NOT NULL,
                formation     TEXT,
                depth_ft      REAL,
                category      TEXT NOT NULL,
                severity      TEXT NOT NULL,
                risk_level    TEXT,
                outcome       TEXT DEFAULT 'Pending',
                action_taken  TEXT,
                data          BLOB NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_events_formation     ON events(formation, timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_events_category      ON events(category, timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_events_timestamp     ON events(timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_events_depth         ON events(depth_ft);
            CREATE INDEX IF NOT EXISTS idx_events_last_modified ON events(last_modified);",
        )?;
        Ok(store)
    }

    /// Insert or update an event. On conflict (same UUID), replaces the row
    /// only if the incoming `last_modified` is newer.
    #[allow(clippy::too_many_lines)]
    pub fn upsert_event(
        &self,
        event: &FleetEvent,
        formation: Option<&str>,
    ) -> Result<(), rusqlite::Error> {
        let json = serde_json::to_vec(event).unwrap_or_default();
        let compressed = super::protocol::compress(&json).unwrap_or(json);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(event.timestamp);
        let last_modified = now;

        let category = format!("{:?}", event.advisory.category);
        let severity = format!("{:?}", event.advisory.severity);
        let risk_level = format!("{:?}", event.advisory.risk_level);
        let outcome = format!("{}", event.outcome);
        let action_taken = match &event.outcome {
            crate::fleet::types::EventOutcome::Resolved { action_taken } => {
                Some(action_taken.clone())
            }
            _ => None,
        };

        // Check if existing row has newer last_modified
        let existing_lm: Option<u64> = self
            .conn
            .query_row(
                "SELECT last_modified FROM events WHERE id = ?1",
                params![event.id],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(existing) = existing_lm {
            if existing >= last_modified {
                // Existing row is newer or same — skip
                return Ok(());
            }
        }

        self.conn.execute(
            "INSERT OR REPLACE INTO events
                (id, rig_id, well_id, timestamp, last_modified, formation,
                 depth_ft, category, severity, risk_level, outcome, action_taken, data)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                event.id,
                event.rig_id,
                event.well_id,
                event.timestamp,
                last_modified,
                formation,
                event.depth,
                category,
                severity,
                risk_level,
                outcome,
                action_taken,
                compressed,
            ],
        )?;
        Ok(())
    }

    /// Get events modified since a given cursor (unix seconds).
    ///
    /// Returns events ordered by `last_modified` ascending, limited to `limit`.
    pub fn events_modified_since(
        &self,
        cursor: u64,
        limit: usize,
    ) -> Result<Vec<FleetEvent>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT data FROM events
             WHERE last_modified > ?1
             ORDER BY last_modified ASC
             LIMIT ?2",
        )?;
        let events = stmt
            .query_map(params![cursor, limit], |row| {
                let blob: Vec<u8> = row.get(0)?;
                Ok(blob)
            })?
            .filter_map(std::result::Result::ok)
            .filter_map(|blob| {
                let json = super::protocol::decompress(&blob).unwrap_or(blob);
                serde_json::from_slice::<FleetEvent>(&json).ok()
            })
            .collect();
        Ok(events)
    }

    /// Query events by formation and depth range.
    #[cfg(test)]
    pub fn query_by_formation(
        &self,
        formation: &str,
        depth_min: f64,
        depth_max: f64,
        limit: usize,
    ) -> Result<Vec<FleetEvent>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT data FROM events
             WHERE formation = ?1
               AND depth_ft BETWEEN ?2 AND ?3
               AND outcome != 'FALSE_POSITIVE'
             ORDER BY timestamp DESC
             LIMIT ?4",
        )?;
        let events = stmt
            .query_map(params![formation, depth_min, depth_max, limit], |row| {
                let blob: Vec<u8> = row.get(0)?;
                Ok(blob)
            })?
            .filter_map(std::result::Result::ok)
            .filter_map(|blob| {
                let json = super::protocol::decompress(&blob).unwrap_or(blob);
                serde_json::from_slice::<FleetEvent>(&json).ok()
            })
            .collect();
        Ok(events)
    }

    /// Enforce retention limits: cap, age, and false positive cleanup.
    pub fn prune(&self) -> Result<usize, rusqlite::Error> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let mut total_pruned = 0usize;

        // Remove events older than retention age
        let age_cutoff = now.saturating_sub(RETENTION_AGE_SECS);
        total_pruned += self.conn.execute(
            "DELETE FROM events WHERE timestamp < ?1",
            params![age_cutoff],
        )?;

        // Remove false positives older than 3 months
        let fp_cutoff = now.saturating_sub(FALSE_POSITIVE_AGE_SECS);
        total_pruned += self.conn.execute(
            "DELETE FROM events WHERE outcome = 'FALSE_POSITIVE' AND timestamp < ?1",
            params![fp_cutoff],
        )?;

        // Cap at RETENTION_CAP — remove oldest
        let count: usize = self
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?;
        if count > RETENTION_CAP {
            let excess = count - RETENTION_CAP;
            total_pruned += self.conn.execute(
                "DELETE FROM events WHERE id IN (
                    SELECT id FROM events ORDER BY timestamp ASC LIMIT ?1
                )",
                params![excess],
            )?;
        }

        if total_pruned > 0 {
            warn!(pruned = total_pruned, "EventStore: pruned stale events");
        }
        Ok(total_pruned)
    }

    /// Get the total event count.
    pub fn count(&self) -> Result<usize, rusqlite::Error> {
        self.conn
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
    }

    /// Update the outcome of an event, bumping `last_modified` so the
    /// change propagates to peers via the gossip sync cursor.
    ///
    /// Returns `true` if a row was updated, `false` if the event ID was not found.
    pub fn update_outcome(
        &self,
        event_id: &str,
        outcome: &str,
        notes: Option<&str>,
    ) -> Result<bool, rusqlite::Error> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let rows = self.conn.execute(
            "UPDATE events SET outcome = ?1, action_taken = ?2, last_modified = ?3
             WHERE id = ?4",
            params![outcome, notes, now, event_id],
        )?;
        Ok(rows > 0)
    }

    /// Count events per category where outcome matches the given value.
    ///
    /// Returns a list of (category, count) pairs, useful for computing
    /// per-category false positive rates.
    #[cfg(test)]
    pub fn outcome_counts_by_category(
        &self,
        outcome: &str,
    ) -> Result<Vec<(String, u64)>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT category, COUNT(*) FROM events
             WHERE outcome = ?1
             GROUP BY category
             ORDER BY COUNT(*) DESC",
        )?;
        let results = stmt
            .query_map(params![outcome], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
            })?
            .filter_map(std::result::Result::ok)
            .collect();
        Ok(results)
    }

    /// Get the max `last_modified` value in the store (for sync cursors).
    pub fn max_last_modified(&self) -> Result<u64, rusqlite::Error> {
        self.conn.query_row(
            "SELECT COALESCE(MAX(last_modified), 0) FROM events",
            [],
            |row| row.get(0),
        )
    }
}

/// Look up the current formation from a sorted formation tops table.
///
/// Returns the formation whose top depth is <= `current_depth`, or `None`
/// if the depth is above all formation tops.
#[cfg(test)]
#[must_use]
pub fn current_formation(current_depth: f64, tops: &[FormationTop]) -> Option<&str> {
    // Walk in reverse to find the deepest top that is <= current_depth
    tops.iter()
        .rev()
        .find(|t| current_depth >= t.depth_ft)
        .map(|t| t.formation.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::types::{EventOutcome, FleetEvent};
    use crate::types::{
        AnomalyCategory, Campaign, DrillingPhysicsReport, FinalSeverity, RiskLevel,
        StrategicAdvisory,
    };

    fn make_test_event(id: &str, depth: f64, timestamp: u64) -> FleetEvent {
        FleetEvent {
            id: id.to_string(),
            rig_id: "rig-001".to_string(),
            well_id: "well-alpha".to_string(),
            field: "test-field".to_string(),
            campaign: Campaign::Production,
            advisory: StrategicAdvisory {
                timestamp,
                efficiency_score: 70,
                risk_level: RiskLevel::Elevated,
                severity: FinalSeverity::Medium,
                recommendation: "test".to_string(),
                expected_benefit: "test".to_string(),
                reasoning: "test".to_string(),
                votes: Vec::new(),
                physics_report: DrillingPhysicsReport::default(),
                context_used: Vec::new(),
                trace_log: Vec::new(),
                category: AnomalyCategory::Mechanical,
                trigger_parameter: "torque_cv".to_string(),
                trigger_value: 0.25,
                threshold_value: 0.15,
            },
            history_window: Vec::new(),
            outcome: EventOutcome::Pending,
            notes: None,
            depth,
            timestamp,
        }
    }

    #[test]
    fn test_event_insert_and_query() {
        let store = EventStore::open_in_memory().expect("open in-memory store");
        let event = make_test_event("evt-1", 8000.0, 1_700_000_000);
        store.upsert_event(&event, Some("shale")).expect("insert");

        let results = store
            .query_by_formation("shale", 6000.0, 10000.0, 10)
            .expect("query");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "evt-1");
    }

    #[test]
    fn test_event_deduplication() {
        let store = EventStore::open_in_memory().expect("open");
        let event = make_test_event("evt-dup", 8000.0, 1_700_000_000);
        store.upsert_event(&event, Some("shale")).expect("insert 1");
        store.upsert_event(&event, Some("shale")).expect("insert 2");
        assert_eq!(store.count().expect("count"), 1);
    }

    #[test]
    fn test_events_modified_since() {
        let store = EventStore::open_in_memory().expect("open");
        let e1 = make_test_event("evt-a", 8000.0, 1_700_000_000);
        let e2 = make_test_event("evt-b", 9000.0, 1_700_001_000);
        store.upsert_event(&e1, Some("shale")).expect("insert");
        store.upsert_event(&e2, Some("limestone")).expect("insert");

        // Query all events (cursor = 0)
        let all = store.events_modified_since(0, 100).expect("query");
        assert_eq!(all.len(), 2);

        // Query with a cursor that should exclude earlier events
        // (both events have last_modified ~ now, so cursor=0 gets both)
        let count = store.count().expect("count");
        assert_eq!(count, 2);
    }

    #[test]
    fn test_prune_retention() {
        let store = EventStore::open_in_memory().expect("open");
        // Insert an event with a very old timestamp
        let mut old_event = make_test_event("evt-old", 5000.0, 100);
        old_event.timestamp = 100; // Very old

        // We need to force the last_modified to be old too
        let json = serde_json::to_vec(&old_event).unwrap();
        let compressed = super::super::protocol::compress(&json).unwrap();
        store
            .conn
            .execute(
                "INSERT INTO events
                (id, rig_id, well_id, timestamp, last_modified, formation,
                 depth_ft, category, severity, risk_level, outcome, data)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    "evt-old",
                    "rig-001",
                    "well-alpha",
                    100i64,
                    100i64,
                    "shale",
                    5000.0,
                    "StickSlip",
                    "Medium",
                    "Elevated",
                    "PENDING",
                    compressed,
                ],
            )
            .expect("raw insert");

        assert_eq!(store.count().expect("count"), 1);
        let pruned = store.prune().expect("prune");
        assert!(pruned >= 1, "should have pruned the old event");
        assert_eq!(store.count().expect("count after prune"), 0);
    }

    #[test]
    fn test_current_formation_lookup() {
        let tops = vec![
            FormationTop {
                depth_ft: 0.0,
                formation: "surface".to_string(),
            },
            FormationTop {
                depth_ft: 2000.0,
                formation: "shale".to_string(),
            },
            FormationTop {
                depth_ft: 5000.0,
                formation: "limestone".to_string(),
            },
            FormationTop {
                depth_ft: 8000.0,
                formation: "sandstone".to_string(),
            },
        ];

        assert_eq!(current_formation(1000.0, &tops), Some("surface"));
        assert_eq!(current_formation(2000.0, &tops), Some("shale"));
        assert_eq!(current_formation(3500.0, &tops), Some("shale"));
        assert_eq!(current_formation(5000.0, &tops), Some("limestone"));
        assert_eq!(current_formation(9999.0, &tops), Some("sandstone"));

        // Empty tops table
        assert_eq!(current_formation(5000.0, &[]), None);
    }

    #[test]
    fn test_update_outcome_sets_fields() {
        let store = EventStore::open_in_memory().expect("open");
        let event = make_test_event("evt-outcome", 8000.0, 1_700_000_000);
        store.upsert_event(&event, Some("shale")).expect("insert");

        let updated = store
            .update_outcome(
                "evt-outcome",
                "true_positive",
                Some("confirmed by operator"),
            )
            .expect("update");
        assert!(updated, "should return true for existing event");

        // Verify the outcome and notes were set
        let (outcome, notes): (String, Option<String>) = store
            .conn
            .query_row(
                "SELECT outcome, action_taken FROM events WHERE id = 'evt-outcome'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("query outcome");
        assert_eq!(outcome, "true_positive");
        assert_eq!(notes.as_deref(), Some("confirmed by operator"));
    }

    #[test]
    fn test_update_outcome_bumps_last_modified() {
        let store = EventStore::open_in_memory().expect("open");
        let event = make_test_event("evt-lm", 8000.0, 1_700_000_000);
        store.upsert_event(&event, Some("shale")).expect("insert");

        let lm_before: u64 = store
            .conn
            .query_row(
                "SELECT last_modified FROM events WHERE id = 'evt-lm'",
                [],
                |row| row.get(0),
            )
            .expect("query lm");

        // Sleep to ensure timestamp advances
        std::thread::sleep(std::time::Duration::from_millis(1100));

        store
            .update_outcome("evt-lm", "false_positive", None)
            .expect("update");

        let lm_after: u64 = store
            .conn
            .query_row(
                "SELECT last_modified FROM events WHERE id = 'evt-lm'",
                [],
                |row| row.get(0),
            )
            .expect("query lm after");
        assert!(
            lm_after > lm_before,
            "last_modified should be bumped: before={lm_before}, after={lm_after}"
        );
    }

    #[test]
    fn test_update_outcome_nonexistent_event() {
        let store = EventStore::open_in_memory().expect("open");
        let updated = store
            .update_outcome("does-not-exist", "false_positive", None)
            .expect("update");
        assert!(!updated, "should return false for missing event");
    }

    #[test]
    fn test_outcome_counts_by_category() {
        let store = EventStore::open_in_memory().expect("open");

        let e1 = make_test_event("evt-c1", 8000.0, 1_700_000_000);
        let mut e2 = make_test_event("evt-c2", 8500.0, 1_700_001_000);
        e2.advisory.category = AnomalyCategory::DrillingEfficiency;
        let mut e3 = make_test_event("evt-c3", 9000.0, 1_700_002_000);
        e3.advisory.category = AnomalyCategory::DrillingEfficiency;

        store.upsert_event(&e1, Some("shale")).expect("insert");
        store.upsert_event(&e2, Some("shale")).expect("insert");
        store.upsert_event(&e3, Some("shale")).expect("insert");

        // Set outcomes
        store
            .update_outcome("evt-c1", "false_positive", None)
            .expect("update");
        store
            .update_outcome("evt-c2", "false_positive", None)
            .expect("update");
        store
            .update_outcome("evt-c3", "true_positive", None)
            .expect("update");

        let fp_counts = store
            .outcome_counts_by_category("false_positive")
            .expect("query");
        assert_eq!(fp_counts.len(), 2);

        let mech = fp_counts.iter().find(|(c, _)| c == "Mechanical");
        let eff = fp_counts.iter().find(|(c, _)| c == "DrillingEfficiency");
        assert_eq!(mech.map(|m| m.1), Some(1));
        assert_eq!(eff.map(|e| e.1), Some(1));

        // true_positive should have 1 entry (DrillingEfficiency)
        let tp_counts = store
            .outcome_counts_by_category("true_positive")
            .expect("query");
        assert_eq!(tp_counts.len(), 1);
        assert_eq!(tp_counts[0].0, "DrillingEfficiency");
        assert_eq!(tp_counts[0].1, 1);
    }
}
