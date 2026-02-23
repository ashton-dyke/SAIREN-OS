//! Packet source abstraction for WITS data ingestion.
//!
//! Provides a unified trait for reading WITS packets from different sources:
//! CSV files (replay), stdin (JSON), and TCP (WITS Level 0 protocol).

use crate::types::WitsPacket;
use anyhow::Result;
use async_trait::async_trait;

/// Events produced by a packet source.
pub enum PacketEvent {
    /// A valid WITS packet was read.
    Packet(WitsPacket),
    /// Source reached end of data (EOF for files/stdin, permanent disconnect for TCP).
    Eof,
}

/// Trait abstracting where WITS packets come from.
///
/// Implementations handle format parsing, reconnection, and pacing internally.
/// The processing loop calls [`next_packet`] in a select! with cancellation.
#[async_trait]
pub trait PacketSource: Send + 'static {
    /// Read the next packet from the source.
    ///
    /// Returns `PacketEvent::Eof` when no more data is available.
    /// Returns `Err` on unrecoverable errors (e.g. failed reconnection).
    async fn next_packet(&mut self) -> Result<PacketEvent>;

    /// Human-readable name for logging (e.g. "CSV", "stdin", "WITS-TCP").
    fn source_name(&self) -> &str;
}

// ============================================================================
// CSV Source (file / synthetic replay)
// ============================================================================

/// Replays pre-loaded WITS packets with optional inter-packet delay.
pub struct CsvSource {
    packets: std::vec::IntoIter<WitsPacket>,
    delay_ms: u64,
    yielded_first: bool,
}

impl CsvSource {
    pub fn new(packets: Vec<WitsPacket>, delay_ms: u64) -> Self {
        Self {
            packets: packets.into_iter(),
            delay_ms,
            yielded_first: false,
        }
    }
}

#[async_trait]
impl PacketSource for CsvSource {
    async fn next_packet(&mut self) -> Result<PacketEvent> {
        // Delay between packets (skip delay before the first packet
        // to match the original for-loop behaviour).
        if self.yielded_first && self.delay_ms > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(self.delay_ms)).await;
        }
        match self.packets.next() {
            Some(p) => {
                self.yielded_first = true;
                Ok(PacketEvent::Packet(p))
            }
            None => Ok(PacketEvent::Eof),
        }
    }

    fn source_name(&self) -> &str {
        "CSV"
    }
}

// ============================================================================
// Stdin Source (JSON WITS packets, one per line)
// ============================================================================

/// Reads JSON-formatted WITS packets from stdin.
///
/// Used with the simulation harness:
/// `python wits_simulator.py | ./sairen-os --stdin`
pub struct StdinSource {
    reader: tokio::io::BufReader<tokio::io::Stdin>,
    line_buffer: String,
}

impl StdinSource {
    pub fn new() -> Self {
        Self {
            reader: tokio::io::BufReader::new(tokio::io::stdin()),
            line_buffer: String::with_capacity(2048),
        }
    }
}

#[async_trait]
impl PacketSource for StdinSource {
    async fn next_packet(&mut self) -> Result<PacketEvent> {
        use tokio::io::AsyncBufReadExt;
        loop {
            self.line_buffer.clear();
            let bytes = self.reader.read_line(&mut self.line_buffer).await?;
            if bytes == 0 {
                return Ok(PacketEvent::Eof);
            }
            let line = self.line_buffer.trim();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<WitsPacket>(line) {
                Ok(packet) => return Ok(PacketEvent::Packet(packet)),
                Err(e) => {
                    tracing::warn!("[StdinSource] Failed to parse packet: {}", e);
                    // Skip malformed lines and keep reading
                }
            }
        }
    }

    fn source_name(&self) -> &str {
        "stdin"
    }
}

// ============================================================================
// TCP Source (WITS Level 0 protocol)
// ============================================================================

/// Reads WITS Level 0 packets from a TCP connection.
///
/// Wraps [`WitsClient`](crate::acquisition::WitsClient) which handles
/// reconnection and timeouts internally.
pub struct TcpSource {
    client: crate::acquisition::WitsClient,
}

impl TcpSource {
    /// Connect to a WITS server and return a ready source.
    pub async fn connect(host: &str, port: u16) -> Result<Self> {
        let mut client = crate::acquisition::WitsClient::new(host, port);
        client.connect().await.map_err(|e| anyhow::anyhow!("{}", e))?;
        Ok(Self { client })
    }
}

#[async_trait]
impl PacketSource for TcpSource {
    async fn next_packet(&mut self) -> Result<PacketEvent> {
        // WitsClient::read_packet() handles reconnection internally.
        // If it returns an error, reconnection has already been exhausted.
        match self.client.read_packet().await {
            Ok(packet) => Ok(PacketEvent::Packet(packet)),
            Err(crate::acquisition::WitsError::ConnectionClosed) => Ok(PacketEvent::Eof),
            Err(e) => Err(anyhow::anyhow!("WITS TCP error: {}", e)),
        }
    }

    fn source_name(&self) -> &str {
        "WITS-TCP"
    }
}
