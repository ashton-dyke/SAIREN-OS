//! Gossip wire protocol types and compression helpers.

use crate::config::PeerInfo;
use crate::fleet::types::FleetEvent;
use serde::{Deserialize, Serialize};

/// Current gossip protocol version.
pub const PROTOCOL_VERSION: u32 = 1;

/// A gossip exchange envelope — sent and received during each peer exchange.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipEnvelope {
    /// Sender node ID.
    pub sender_id: String,
    /// Protocol version for forward compatibility.
    pub version: u32,
    /// Unix timestamp of this envelope.
    pub timestamp: u64,
    /// Recent events (created or updated since last sync with this peer).
    pub recent_events: Vec<FleetEvent>,
    /// Known peers (for optional dynamic peer discovery).
    pub known_peers: Vec<PeerInfo>,
}

/// Compress a JSON-serialized envelope with zstd.
///
/// # Errors
///
/// Returns an I/O error if zstd compression fails.
pub fn compress(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    zstd::encode_all(data, 3)
}

/// Decompress a zstd-compressed envelope.
///
/// # Errors
///
/// Returns an I/O error if zstd decompression fails.
pub fn decompress(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    zstd::decode_all(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gossip_envelope_serde_round_trip() {
        let envelope = GossipEnvelope {
            sender_id: "rig-001".to_string(),
            version: PROTOCOL_VERSION,
            timestamp: 1_700_000_000,
            recent_events: Vec::new(),
            known_peers: vec![PeerInfo {
                id: "rig-002".to_string(),
                address: "10.0.0.2:8080".to_string(),
            }],
        };

        let json = serde_json::to_vec(&envelope).expect("serialize");
        let roundtripped: GossipEnvelope = serde_json::from_slice(&json).expect("deserialize");

        assert_eq!(roundtripped.sender_id, "rig-001");
        assert_eq!(roundtripped.version, PROTOCOL_VERSION);
        assert_eq!(roundtripped.known_peers.len(), 1);
        assert_eq!(roundtripped.known_peers[0].id, "rig-002");
    }

    #[test]
    fn test_zstd_round_trip() {
        let envelope = GossipEnvelope {
            sender_id: "rig-001".to_string(),
            version: PROTOCOL_VERSION,
            timestamp: 1_700_000_000,
            recent_events: Vec::new(),
            known_peers: Vec::new(),
        };

        let json = serde_json::to_vec(&envelope).expect("serialize");
        let compressed = compress(&json).expect("compress");
        let decompressed = decompress(&compressed).expect("decompress");
        let roundtripped: GossipEnvelope =
            serde_json::from_slice(&decompressed).expect("deserialize");

        assert_eq!(roundtripped.sender_id, "rig-001");
        assert!(compressed.len() < json.len(), "zstd should reduce size");
    }
}
