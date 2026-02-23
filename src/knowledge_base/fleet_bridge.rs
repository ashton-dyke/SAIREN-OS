//! Fleet bridge: upload post-well performance to hub, download offset data

#[cfg(feature = "fleet-client")]
use crate::fleet::client::{FleetClient, FleetClientError};
#[cfg(feature = "fleet-client")]
use crate::types::KnowledgeBaseConfig;
use crate::types::PostWellFormationPerformance;
use serde::{Deserialize, Serialize};
#[cfg(feature = "fleet-client")]
use tracing::{info, warn};

/// Fleet payload for post-well performance sharing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetPerformanceUpload {
    pub rig_id: String,
    pub well_id: String,
    pub field: String,
    pub formation_name: String,
    pub performance: PostWellFormationPerformance,
}

/// Response from the hub for performance queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetPerformanceResponse {
    pub records: Vec<FleetPerformanceUpload>,
    pub total: usize,
}

/// Upload all post-well performance files for the current well to the fleet hub.
///
/// Returns the number of files successfully uploaded.
#[cfg(feature = "fleet-client")]
pub async fn upload_post_well(
    client: &FleetClient,
    config: &KnowledgeBaseConfig,
) -> Result<usize, FleetClientError> {
    let perf_files = config
        .list_post_well_performance(&config.well)
        .map_err(|e| FleetClientError::Compression(format!("IO error listing files: {}", e)))?;

    let mut uploaded = 0;
    for path in &perf_files {
        let perf: PostWellFormationPerformance = match crate::knowledge_base::compressor::read_toml(path) {
            Ok(p) => p,
            Err(e) => {
                warn!(path = %path.display(), error = %e, "Failed to read performance file for upload");
                continue;
            }
        };

        let upload = FleetPerformanceUpload {
            rig_id: client.rig_id().to_string(),
            well_id: config.well.clone(),
            field: config.field.clone(),
            formation_name: perf.formation_name.clone(),
            performance: perf,
        };

        let json = serde_json::to_vec(&upload)?;
        let compressed = zstd::encode_all(json.as_slice(), 3)
            .map_err(|e| FleetClientError::Compression(e.to_string()))?;

        let resp = client
            .http_client()
            .post(format!("{}/api/fleet/performance", client.hub_url()))
            .header("Authorization", format!("Bearer {}", client.passphrase()))
            .header("Content-Type", "application/json")
            .header("Content-Encoding", "zstd")
            .header("X-Rig-ID", client.rig_id())
            .body(compressed)
            .send()
            .await?;

        if resp.status().is_success() {
            uploaded += 1;
            info!(formation = &upload.formation_name, "Uploaded post-well performance to hub");
        } else {
            warn!(
                status = %resp.status(),
                formation = &upload.formation_name,
                "Failed to upload performance to hub"
            );
        }
    }

    info!(uploaded = uploaded, total = perf_files.len(), "Post-well performance upload complete");
    Ok(uploaded)
}

/// Sync offset well performance data from the fleet hub.
///
/// Downloads performance records for the field and writes them to the
/// appropriate sibling well directories.
#[cfg(feature = "fleet-client")]
pub async fn sync_performance(
    client: &FleetClient,
    config: &KnowledgeBaseConfig,
    since: Option<u64>,
) -> Result<usize, FleetClientError> {
    let mut url = format!(
        "{}/api/fleet/performance?field={}",
        client.hub_url(),
        urlencoding_field(&config.field)
    );
    if let Some(ts) = since {
        url.push_str(&format!("&since={}", ts));
    }
    // Exclude our own rig's data
    url.push_str(&format!("&exclude_rig={}", urlencoding_field(client.rig_id())));

    let resp = client
        .http_client()
        .get(&url)
        .header("Authorization", format!("Bearer {}", client.passphrase()))
        .header("Accept-Encoding", "zstd")
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(FleetClientError::ServerError(resp.status()));
    }

    let body = resp.bytes().await?;
    let response: FleetPerformanceResponse = serde_json::from_slice(&body)?;

    let mut written = 0;
    for record in &response.records {
        let post_dir = config.post_well_dir(&record.well_id);
        if let Err(e) = std::fs::create_dir_all(&post_dir) {
            warn!(well = &record.well_id, error = %e, "Failed to create post-well dir");
            continue;
        }

        let safe_name = record.formation_name.replace(' ', "_").replace(['/', '\\', '(', ')'], "");
        let filename = format!("performance_{}.toml", safe_name);
        let path = post_dir.join(&filename);

        if let Err(e) = crate::knowledge_base::compressor::write_toml(&path, &record.performance) {
            warn!(path = %path.display(), error = %e, "Failed to write synced performance file");
            continue;
        }

        written += 1;
    }

    if written > 0 {
        info!(
            written = written,
            total = response.records.len(),
            "Synced performance data from fleet hub"
        );
    }

    Ok(written)
}

/// Simple URL-safe encoding for field/rig names
#[cfg(feature = "fleet-client")]
fn urlencoding_field(s: &str) -> String {
    s.replace(' ', "%20")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BestParams, ParameterRange};

    #[test]
    fn test_fleet_performance_upload_serde() {
        let upload = FleetPerformanceUpload {
            rig_id: "RIG-1".to_string(),
            well_id: "Well-A".to_string(),
            field: "TestField".to_string(),
            formation_name: "Shallow".to_string(),
            performance: PostWellFormationPerformance {
                well_id: "Well-A".to_string(),
                field: "TestField".to_string(),
                formation_name: "Shallow".to_string(),
                depth_top_ft: 0.0,
                depth_base_ft: 3000.0,
                avg_rop_ft_hr: 100.0,
                best_rop_ft_hr: 150.0,
                avg_mse_psi: 10000.0,
                best_params: BestParams { wob_klbs: 12.0, rpm: 130.0 },
                avg_wob_range: ParameterRange { min: 5.0, optimal: 12.0, max: 15.0 },
                avg_rpm_range: ParameterRange { min: 80.0, optimal: 130.0, max: 160.0 },
                avg_flow_range: ParameterRange { min: 400.0, optimal: 500.0, max: 600.0 },
                total_snapshots: 50,
                avg_confidence: 0.8,
                avg_stability: 0.9,
                notes: String::new(),
                completed_timestamp: 1700000000,
                sustained_only: None,
            },
        };

        let json = serde_json::to_string(&upload).expect("serialize");
        let deserialized: FleetPerformanceUpload = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.rig_id, "RIG-1");
        assert_eq!(deserialized.formation_name, "Shallow");
        assert!((deserialized.performance.avg_rop_ft_hr - 100.0).abs() < 0.01);
    }
}
