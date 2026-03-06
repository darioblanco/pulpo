use anyhow::Result;
use pulpo_common::api::{AddPeerRequest, PeersResponse};

/// Build the URL for fetching the peer list from a seed node.
pub fn peers_url(seed_address: &str) -> String {
    format!("http://{seed_address}/api/v1/peers")
}

/// Parse a `PeersResponse` from JSON.
pub fn parse_peers_response(json: &str) -> Result<PeersResponse> {
    serde_json::from_str(json)
        .map_err(|e| anyhow::anyhow!("Failed to parse seed peers response: {e}"))
}

/// Build the `AddPeerRequest` for announcing ourselves to a remote node.
pub fn build_announce_request(own_name: &str, own_address: &str) -> AddPeerRequest {
    AddPeerRequest {
        name: own_name.to_owned(),
        address: own_address.to_owned(),
    }
}

/// Extract peer addresses from a `PeersResponse`, excluding our own node.
/// Returns `(name, address)` pairs.
pub fn extract_peers(response: &PeersResponse, own_name: &str) -> Vec<(String, String)> {
    let mut peers = Vec::new();

    // The seed node itself (from the `local` field)
    if response.local.name != own_name {
        // We don't know the seed's listen address from the response,
        // so we skip the local node here — we already know it (it's the seed).
    }

    // All peers known by the seed
    for peer in &response.peers {
        if peer.name != own_name {
            peers.push((peer.name.clone(), peer.address.clone()));
        }
    }

    peers
}

/// Run the seed discovery loop — periodically fetches the peer list from the
/// seed node, registers discovered peers, and announces ourselves.
///
/// Excluded from coverage builds because it performs real HTTP I/O.
#[cfg(not(coverage))]
pub async fn run_seed_discovery(
    registry: crate::peers::PeerRegistry,
    own_name: String,
    own_port: u16,
    seed_address: String,
    interval: std::time::Duration,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    tracing::info!(
        seed = seed_address,
        interval_secs = interval.as_secs(),
        "Seed discovery: started"
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();

    // Register the seed itself as a discovered peer
    if registry.add_discovered_peer("seed", &seed_address).await {
        tracing::info!("Seed discovery: registered seed at {seed_address}");
    }

    loop {
        // Fetch peer list from seed
        let url = peers_url(&seed_address);
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(body) = resp.text().await
                    && let Ok(peers_resp) = parse_peers_response(&body)
                {
                    let discovered = extract_peers(&peers_resp, &own_name);
                    for (name, address) in &discovered {
                        if registry.add_discovered_peer(name, address).await {
                            tracing::info!("Seed discovery: discovered peer {name} at {address}");
                        }
                    }

                    // Also update the seed peer's name from the response
                    let seed_name = &peers_resp.local.name;
                    if seed_name != &own_name {
                        registry.remove_discovered_peer("seed").await;
                        registry.add_discovered_peer(seed_name, &seed_address).await;
                    }
                }
            }
            Ok(resp) => {
                tracing::warn!("Seed discovery: seed returned {}", resp.status());
            }
            Err(e) => {
                tracing::warn!("Seed discovery: failed to contact seed: {e}");
            }
        }

        // Announce ourselves to the seed
        let own_address = format!("{}:{own_port}", detect_own_ip().unwrap_or_default());
        if !own_address.starts_with(':') {
            let announce = build_announce_request(&own_name, &own_address);
            let announce_url = peers_url(&seed_address);
            let _ = client.post(&announce_url).json(&announce).send().await;
        }

        // Wait for the next scan or shutdown
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    tracing::info!("Seed discovery: shutting down");
                    break;
                }
            }
            () = tokio::time::sleep(interval) => {}
        }
    }
}

/// Detect our own IP address by connecting to a well-known address.
/// Falls back to `None` if detection fails.
#[cfg(not(coverage))]
fn detect_own_ip() -> Option<String> {
    use std::net::UdpSocket;
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let addr = socket.local_addr().ok()?;
    Some(addr.ip().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulpo_common::node::NodeInfo;
    use pulpo_common::peer::{PeerInfo, PeerSource, PeerStatus};

    fn sample_peers_response() -> PeersResponse {
        PeersResponse {
            local: NodeInfo {
                name: "seed-node".into(),
                hostname: "seed-host".into(),
                os: "linux".into(),
                arch: "x86_64".into(),
                cpus: 8,
                memory_mb: 16384,
                gpu: None,
            },
            peers: vec![
                PeerInfo {
                    name: "node-a".into(),
                    address: "10.0.0.1:7433".into(),
                    status: PeerStatus::Online,
                    node_info: None,
                    session_count: Some(2),
                    source: PeerSource::Configured,
                },
                PeerInfo {
                    name: "node-b".into(),
                    address: "10.0.0.2:7433".into(),
                    status: PeerStatus::Unknown,
                    node_info: None,
                    session_count: None,
                    source: PeerSource::Discovered,
                },
            ],
        }
    }

    #[test]
    fn test_peers_url() {
        assert_eq!(
            peers_url("10.0.0.5:7433"),
            "http://10.0.0.5:7433/api/v1/peers"
        );
    }

    #[test]
    fn test_peers_url_hostname() {
        assert_eq!(
            peers_url("seed-node:7433"),
            "http://seed-node:7433/api/v1/peers"
        );
    }

    #[test]
    fn test_parse_peers_response_valid() {
        let resp = sample_peers_response();
        let json = serde_json::to_string(&resp).unwrap();
        let parsed = parse_peers_response(&json).unwrap();
        assert_eq!(parsed.local.name, "seed-node");
        assert_eq!(parsed.peers.len(), 2);
    }

    #[test]
    fn test_parse_peers_response_invalid() {
        let result = parse_peers_response("not json");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("Failed to parse seed peers response"));
    }

    #[test]
    fn test_build_announce_request() {
        let req = build_announce_request("my-node", "10.0.0.10:7433");
        assert_eq!(req.name, "my-node");
        assert_eq!(req.address, "10.0.0.10:7433");
    }

    #[test]
    fn test_extract_peers_excludes_own() {
        let resp = sample_peers_response();
        let peers = extract_peers(&resp, "node-a");
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].0, "node-b");
        assert_eq!(peers[0].1, "10.0.0.2:7433");
    }

    #[test]
    fn test_extract_peers_includes_all_when_not_self() {
        let resp = sample_peers_response();
        let peers = extract_peers(&resp, "unrelated-node");
        assert_eq!(peers.len(), 2);
    }

    #[test]
    fn test_extract_peers_empty_response() {
        let resp = PeersResponse {
            local: NodeInfo {
                name: "seed".into(),
                hostname: "h".into(),
                os: "linux".into(),
                arch: "x86_64".into(),
                cpus: 4,
                memory_mb: 8192,
                gpu: None,
            },
            peers: vec![],
        };
        let peers = extract_peers(&resp, "my-node");
        assert!(peers.is_empty());
    }

    #[test]
    fn test_extract_peers_all_are_self() {
        let resp = PeersResponse {
            local: NodeInfo {
                name: "seed".into(),
                hostname: "h".into(),
                os: "linux".into(),
                arch: "x86_64".into(),
                cpus: 4,
                memory_mb: 8192,
                gpu: None,
            },
            peers: vec![PeerInfo {
                name: "my-node".into(),
                address: "10.0.0.1:7433".into(),
                status: PeerStatus::Online,
                node_info: None,
                session_count: None,
                source: PeerSource::Discovered,
            }],
        };
        let peers = extract_peers(&resp, "my-node");
        assert!(peers.is_empty());
    }

    #[test]
    fn test_parse_peers_response_roundtrip() {
        let resp = sample_peers_response();
        let json = serde_json::to_string(&resp).unwrap();
        let parsed = parse_peers_response(&json).unwrap();
        assert_eq!(parsed.peers.len(), resp.peers.len());
        assert_eq!(parsed.local.name, resp.local.name);
    }
}
