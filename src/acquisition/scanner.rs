//! WITS Level 0 subnet scanner
//!
//! Actively scans the local /24 subnet for WITS TCP streams on configurable
//! port ranges. Used by the setup wizard to auto-discover WITS data servers.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

/// A discovered WITS TCP endpoint.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WitsDiscovery {
    /// IP address of the WITS server
    pub host: String,
    /// TCP port
    pub port: u16,
    /// True if a WITS Level 0 frame header (`&&\r\n`) was observed
    pub validated: bool,
}

/// Default port ranges to scan for WITS Level 0 streams.
pub const DEFAULT_PORT_RANGES: &[(u16, u16)] = &[(5000, 5010), (10001, 10010)];

/// Maximum concurrent TCP probe connections.
const MAX_CONCURRENT_PROBES: usize = 64;

/// Detect the local machine's IPv4 address by briefly connecting a UDP socket.
///
/// This does not send any data — the OS simply selects the appropriate
/// source address for the given destination.
fn detect_local_ip() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    // Connect to a public IP to determine which local interface would be used.
    // No data is actually sent over UDP.
    socket.connect("8.8.8.8:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) => Some(ip),
        IpAddr::V6(_) => None,
    }
}

/// Probe a single host:port for a WITS TCP stream.
///
/// Returns `Some(WitsDiscovery)` if TCP connects, `None` if unreachable.
async fn probe_endpoint(addr: SocketAddr, connect_timeout: Duration) -> Option<WitsDiscovery> {
    let stream = tokio::time::timeout(connect_timeout, TcpStream::connect(addr))
        .await
        .ok()?
        .ok()?;

    // TCP connected — try to read first bytes to validate WITS header
    let validated = match try_read_wits_header(stream).await {
        Ok(v) => v,
        Err(_) => false,
    };

    Some(WitsDiscovery {
        host: addr.ip().to_string(),
        port: addr.port(),
        validated,
    })
}

/// Attempt to read the first bytes from a connected TCP stream and check
/// for the WITS Level 0 frame start marker `&&\r\n`.
async fn try_read_wits_header(mut stream: TcpStream) -> std::io::Result<bool> {
    let mut buf = [0u8; 256];
    let read_timeout = Duration::from_secs(3);

    match tokio::time::timeout(read_timeout, stream.read(&mut buf)).await {
        Ok(Ok(n)) if n >= 4 => {
            // Look for && followed by \r\n anywhere in the first bytes
            let data = &buf[..n];
            Ok(data
                .windows(4)
                .any(|w| w == b"&&\r\n"))
        }
        Ok(Ok(n)) if n > 0 => {
            // Got some data but too short for a full WITS header
            let data = &buf[..n];
            Ok(data.windows(2).any(|w| w == b"&&"))
        }
        _ => Ok(false),
    }
}

/// Expand port ranges into a flat list of ports.
fn expand_port_ranges(ranges: &[(u16, u16)]) -> Vec<u16> {
    let mut ports = Vec::new();
    for &(start, end) in ranges {
        for port in start..=end {
            ports.push(port);
        }
    }
    ports
}

/// Scan the local /24 subnet on given port ranges for WITS TCP streams.
///
/// Returns discovered streams sorted by IP then port.
///
/// # Arguments
/// * `port_ranges` — Port ranges to scan, e.g. `&[(5000, 5010), (10001, 10010)]`
/// * `timeout_ms` — Per-connection timeout in milliseconds (default: 2000)
pub async fn scan_subnet(
    port_ranges: &[(u16, u16)],
    timeout_ms: u64,
) -> Vec<WitsDiscovery> {
    let local_ip = match detect_local_ip() {
        Some(ip) => ip,
        None => {
            warn!("Could not detect local IP address — subnet scan skipped");
            return Vec::new();
        }
    };

    let octets = local_ip.octets();
    info!(
        "Scanning subnet {}.{}.{}.0/24 for WITS streams",
        octets[0], octets[1], octets[2]
    );

    let ports = expand_port_ranges(port_ranges);
    let timeout = Duration::from_millis(timeout_ms);
    let semaphore = std::sync::Arc::new(Semaphore::new(MAX_CONCURRENT_PROBES));

    let mut handles = Vec::new();

    for host_octet in 1..=254u8 {
        let ip = Ipv4Addr::new(octets[0], octets[1], octets[2], host_octet);

        // Skip our own IP
        if ip == local_ip {
            continue;
        }

        for &port in &ports {
            let addr = SocketAddr::new(IpAddr::V4(ip), port);
            let sem = semaphore.clone();

            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await;
                debug!("Probing {}", addr);
                probe_endpoint(addr, timeout).await
            }));
        }
    }

    let mut discoveries = Vec::new();
    for handle in handles {
        if let Ok(Some(discovery)) = handle.await {
            info!(
                "Found WITS stream at {}:{} (validated: {})",
                discovery.host, discovery.port, discovery.validated
            );
            discoveries.push(discovery);
        }
    }

    // Sort by IP then port
    discoveries.sort_by(|a, b| a.host.cmp(&b.host).then(a.port.cmp(&b.port)));
    discoveries
}

/// Parse port ranges from a comma-separated string like "5000-5010,10001-10010".
pub fn parse_port_ranges(s: &str) -> Result<Vec<(u16, u16)>, String> {
    let mut ranges = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if let Some((start, end)) = part.split_once('-') {
            let start: u16 = start
                .trim()
                .parse()
                .map_err(|_| format!("Invalid port: {}", start))?;
            let end: u16 = end
                .trim()
                .parse()
                .map_err(|_| format!("Invalid port: {}", end))?;
            if start > end {
                return Err(format!("Invalid range: {}-{}", start, end));
            }
            ranges.push((start, end));
        } else {
            let port: u16 = part
                .parse()
                .map_err(|_| format!("Invalid port: {}", part))?;
            ranges.push((port, port));
        }
    }
    Ok(ranges)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_port_ranges() {
        let ranges = parse_port_ranges("5000-5010,10001-10010").expect("valid");
        assert_eq!(ranges, vec![(5000, 5010), (10001, 10010)]);
    }

    #[test]
    fn test_parse_single_port() {
        let ranges = parse_port_ranges("5000").expect("valid");
        assert_eq!(ranges, vec![(5000, 5000)]);
    }

    #[test]
    fn test_expand_port_ranges() {
        let ports = expand_port_ranges(&[(5000, 5002)]);
        assert_eq!(ports, vec![5000, 5001, 5002]);
    }

    #[test]
    fn test_detect_local_ip() {
        // Should succeed on any networked machine
        let ip = detect_local_ip();
        // Don't assert Some — CI may not have a route to 8.8.8.8
        if let Some(ip) = ip {
            assert!(!ip.is_loopback());
        }
    }
}
