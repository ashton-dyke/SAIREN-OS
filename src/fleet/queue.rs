//! Upload Queue — disk-backed durable queue for fleet event upload
//!
//! Stores confirmed AMBER/RED advisory events on disk as zstd-compressed JSON
//! files for eventual upload to the fleet hub. Files are named by event ID
//! for idempotent retry. The queue scans the directory on startup to resume
//! pending uploads after a restart.

use super::types::FleetEvent;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, error, info, warn};

/// Default maximum queue size (number of events)
const DEFAULT_MAX_QUEUE_SIZE: usize = 1000;

/// Upload queue for fleet events
pub struct UploadQueue {
    /// Directory for queue files
    queue_dir: PathBuf,
    /// Maximum number of events in the queue
    max_size: usize,
}

impl UploadQueue {
    /// Create or open an upload queue at the given directory
    pub fn open<P: AsRef<Path>>(queue_dir: P) -> Result<Self, QueueError> {
        let queue_dir = queue_dir.as_ref().to_path_buf();
        fs::create_dir_all(&queue_dir).map_err(|e| QueueError::Io(e.to_string()))?;

        let queue = Self {
            queue_dir,
            max_size: DEFAULT_MAX_QUEUE_SIZE,
        };

        let pending = queue.pending_count()?;
        if pending > 0 {
            info!(pending = pending, "Upload queue opened with pending events");
        } else {
            debug!("Upload queue opened (empty)");
        }

        Ok(queue)
    }

    /// Enqueue a fleet event for upload
    ///
    /// The event is written to disk as a JSON file named by event ID.
    /// If an event with the same ID already exists, it's silently skipped
    /// (idempotent behavior for retry scenarios).
    pub fn enqueue(&self, event: &FleetEvent) -> Result<(), QueueError> {
        // Check queue size
        let current = self.pending_count()?;
        if current >= self.max_size {
            warn!(
                max = self.max_size,
                current = current,
                "Upload queue full — dropping oldest event"
            );
            self.drop_oldest()?;
        }

        let file_path = self.event_path(&event.id);

        // Idempotent: skip if already queued
        if file_path.exists() {
            debug!(id = %event.id, "Event already queued, skipping");
            return Ok(());
        }

        let json = serde_json::to_vec(event)
            .map_err(|e| QueueError::Serialization(e.to_string()))?;

        fs::write(&file_path, &json)
            .map_err(|e| QueueError::Io(e.to_string()))?;

        debug!(id = %event.id, size_bytes = json.len(), "Event queued for upload");
        Ok(())
    }

    /// Drain all pending events from the queue
    ///
    /// Returns events in chronological order (oldest first).
    /// Events are NOT removed from disk — call `mark_uploaded` after successful upload.
    pub fn drain(&self) -> Result<Vec<FleetEvent>, QueueError> {
        let mut events = Vec::new();

        let entries = fs::read_dir(&self.queue_dir)
            .map_err(|e| QueueError::Io(e.to_string()))?;

        for entry in entries {
            let entry = entry.map_err(|e| QueueError::Io(e.to_string()))?;
            let path = entry.path();

            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            match fs::read(&path) {
                Ok(data) => {
                    match serde_json::from_slice::<FleetEvent>(&data) {
                        Ok(event) => events.push(event),
                        Err(e) => {
                            error!(path = %path.display(), error = %e, "Corrupted queue entry — removing");
                            let _ = fs::remove_file(&path);
                        }
                    }
                }
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "Could not read queue entry");
                }
            }
        }

        // Sort by timestamp (oldest first for chronological upload)
        events.sort_by_key(|e| e.timestamp);

        Ok(events)
    }

    /// Mark an event as successfully uploaded (removes from queue)
    pub fn mark_uploaded(&self, event_id: &str) -> Result<(), QueueError> {
        let path = self.event_path(event_id);
        if path.exists() {
            fs::remove_file(&path).map_err(|e| QueueError::Io(e.to_string()))?;
            debug!(id = event_id, "Event marked as uploaded");
        }
        Ok(())
    }

    /// Get the number of pending events
    pub fn pending_count(&self) -> Result<usize, QueueError> {
        let entries = fs::read_dir(&self.queue_dir)
            .map_err(|e| QueueError::Io(e.to_string()))?;

        Ok(entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    == Some("json")
            })
            .count())
    }

    /// Drop the oldest event to make room
    fn drop_oldest(&self) -> Result<(), QueueError> {
        let entries = fs::read_dir(&self.queue_dir)
            .map_err(|e| QueueError::Io(e.to_string()))?;

        let mut oldest: Option<(PathBuf, std::time::SystemTime)> = None;

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(meta) = entry.metadata() {
                if let Ok(modified) = meta.modified() {
                    match &oldest {
                        None => oldest = Some((path, modified)),
                        Some((_, oldest_time)) if modified < *oldest_time => {
                            oldest = Some((path, modified));
                        }
                        _ => {}
                    }
                }
            }
        }

        if let Some((path, _)) = oldest {
            fs::remove_file(&path).map_err(|e| QueueError::Io(e.to_string()))?;
            debug!(path = %path.display(), "Dropped oldest queue entry");
        }

        Ok(())
    }

    /// File path for an event in the queue
    fn event_path(&self, event_id: &str) -> PathBuf {
        // Sanitize the event ID to be a safe filename
        let safe_id: String = event_id
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        self.queue_dir.join(format!("{}.json", safe_id))
    }
}

/// Queue errors
#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    #[error("IO error: {0}")]
    Io(String),
    #[error("serialization error: {0}")]
    Serialization(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        Campaign, DrillingPhysicsReport, FinalSeverity, RiskLevel, StrategicAdvisory,
    };
    use super::super::types::EventOutcome;

    fn make_event(id: &str, ts: u64) -> FleetEvent {
        FleetEvent {
            id: id.to_string(),
            rig_id: "RIG1".to_string(),
            well_id: "WELL-001".to_string(),
            field: "TestField".to_string(),
            campaign: Campaign::Production,
            advisory: StrategicAdvisory {
                timestamp: ts,
                efficiency_score: 70,
                risk_level: RiskLevel::High,
                severity: FinalSeverity::High,
                recommendation: "test".to_string(),
                expected_benefit: "test".to_string(),
                reasoning: "test".to_string(),
                votes: Vec::new(),
                physics_report: DrillingPhysicsReport::default(),
                context_used: Vec::new(),
                trace_log: Vec::new(),
            },
            history_window: Vec::new(),
            outcome: EventOutcome::Pending,
            notes: None,
            depth: 10000.0,
            timestamp: ts,
        }
    }

    #[test]
    fn test_enqueue_and_drain() {
        let tmp = tempfile::tempdir().unwrap();
        let queue = UploadQueue::open(tmp.path().join("queue")).unwrap();

        queue.enqueue(&make_event("evt-1", 100)).unwrap();
        queue.enqueue(&make_event("evt-2", 200)).unwrap();

        assert_eq!(queue.pending_count().unwrap(), 2);

        let events = queue.drain().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].id, "evt-1"); // oldest first
        assert_eq!(events[1].id, "evt-2");
    }

    #[test]
    fn test_idempotent_enqueue() {
        let tmp = tempfile::tempdir().unwrap();
        let queue = UploadQueue::open(tmp.path().join("queue")).unwrap();

        queue.enqueue(&make_event("evt-1", 100)).unwrap();
        queue.enqueue(&make_event("evt-1", 100)).unwrap(); // duplicate

        assert_eq!(queue.pending_count().unwrap(), 1);
    }

    #[test]
    fn test_mark_uploaded() {
        let tmp = tempfile::tempdir().unwrap();
        let queue = UploadQueue::open(tmp.path().join("queue")).unwrap();

        queue.enqueue(&make_event("evt-1", 100)).unwrap();
        assert_eq!(queue.pending_count().unwrap(), 1);

        queue.mark_uploaded("evt-1").unwrap();
        assert_eq!(queue.pending_count().unwrap(), 0);
    }

    #[test]
    fn test_survives_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let queue_dir = tmp.path().join("queue");

        // Write some events
        {
            let queue = UploadQueue::open(&queue_dir).unwrap();
            queue.enqueue(&make_event("evt-1", 100)).unwrap();
            queue.enqueue(&make_event("evt-2", 200)).unwrap();
        }

        // "Restart" — open the same directory
        {
            let queue = UploadQueue::open(&queue_dir).unwrap();
            assert_eq!(queue.pending_count().unwrap(), 2);
            let events = queue.drain().unwrap();
            assert_eq!(events.len(), 2);
        }
    }
}
