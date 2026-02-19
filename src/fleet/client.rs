//! Fleet Client — HTTP client for spoke → hub communication
//!
//! Handles event uploads, outcome forwarding, and library sync.

#[cfg(feature = "fleet-client")]
use crate::fleet::types::{EventOutcome, FleetEpisode, FleetEvent};

/// Fleet client errors
#[cfg(feature = "fleet-client")]
#[derive(Debug, thiserror::Error)]
pub enum FleetClientError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Server returned status {0}")]
    ServerError(reqwest::StatusCode),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Compression error: {0}")]
    Compression(String),
    #[error("Not modified (304)")]
    NotModified,
}

/// Library sync response from the hub
#[cfg(feature = "fleet-client")]
#[derive(Debug, serde::Deserialize)]
pub struct LibraryResponse {
    pub version: i64,
    pub episodes: Vec<FleetEpisode>,
    pub total_fleet_episodes: i64,
    pub pruned_ids: Vec<String>,
}

/// HTTP client for hub communication
#[cfg(feature = "fleet-client")]
#[derive(Clone)]
pub struct FleetClient {
    http: reqwest::Client,
    hub_url: String,
    api_key: String,
    rig_id: String,
}

#[cfg(feature = "fleet-client")]
impl FleetClient {
    /// Create a new fleet client
    pub fn new(hub_url: &str, api_key: &str, rig_id: &str) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            http,
            hub_url: hub_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            rig_id: rig_id.to_string(),
        }
    }

    /// Upload a single event to the hub
    ///
    /// Returns Ok(true) if accepted, Ok(false) if duplicate (409).
    pub async fn upload_event(&self, event: &FleetEvent) -> Result<bool, FleetClientError> {
        let json = serde_json::to_vec(event)?;
        let compressed = zstd::encode_all(json.as_slice(), 3)
            .map_err(|e| FleetClientError::Compression(e.to_string()))?;

        let resp = self
            .http
            .post(format!("{}/api/fleet/events", self.hub_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("Content-Encoding", "zstd")
            .header("X-Rig-ID", &self.rig_id)
            .body(compressed)
            .send()
            .await?;

        match resp.status() {
            reqwest::StatusCode::CREATED => Ok(true),
            reqwest::StatusCode::CONFLICT => Ok(false),
            status => Err(FleetClientError::ServerError(status)),
        }
    }

    /// Update event outcome on the hub
    pub async fn update_outcome(
        &self,
        event_id: &str,
        outcome: &EventOutcome,
    ) -> Result<(), FleetClientError> {
        let (outcome_str, action, notes) = match outcome {
            EventOutcome::Resolved { action_taken } => {
                ("Resolved".to_string(), Some(action_taken.clone()), None)
            }
            EventOutcome::Escalated { reason } => {
                ("Escalated".to_string(), None, Some(reason.clone()))
            }
            EventOutcome::FalsePositive => ("FalsePositive".to_string(), None, None),
            EventOutcome::Pending => ("Pending".to_string(), None, None),
        };

        let body = serde_json::json!({
            "outcome": outcome_str,
            "action_taken": action,
            "notes": notes,
        });

        let resp = self
            .http
            .patch(format!(
                "{}/api/fleet/events/{}/outcome",
                self.hub_url, event_id
            ))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(FleetClientError::ServerError(resp.status()))
        }
    }

    /// Pull library delta from hub
    pub async fn sync_library(
        &self,
        since: Option<u64>,
    ) -> Result<LibraryResponse, FleetClientError> {
        let mut req = self
            .http
            .get(format!("{}/api/fleet/library", self.hub_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Accept-Encoding", "zstd");

        if let Some(ts) = since {
            req = req.header("If-Modified-Since", ts.to_string());
        }

        let resp = req.send().await?;

        match resp.status() {
            reqwest::StatusCode::NOT_MODIFIED => Err(FleetClientError::NotModified),
            reqwest::StatusCode::OK => {
                let body = resp.bytes().await?;
                let library: LibraryResponse = serde_json::from_slice(&body)?;
                Ok(library)
            }
            status => Err(FleetClientError::ServerError(status)),
        }
    }

    /// Get hub URL for logging
    pub fn hub_url(&self) -> &str {
        &self.hub_url
    }

    /// Get rig ID
    pub fn rig_id(&self) -> &str {
        &self.rig_id
    }
}
