use std::collections::HashMap;

use serde::Deserialize;

/// Default Tailscale port for pulpo probing.
pub const DEFAULT_PULPO_PORT: u16 = 7433;

/// Parsed output of `tailscale status --json`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TailscaleStatus {
    #[serde(rename = "Self")]
    pub self_node: TailscaleNode,
    #[serde(default)]
    pub peer: HashMap<String, TailscaleNode>,
}

/// A single node in the Tailscale network.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TailscaleNode {
    pub host_name: String,
    #[serde(rename = "DNSName")]
    pub dns_name: String,
    #[serde(default)]
    pub tailscale_i_ps: Vec<String>,
    #[serde(default)]
    pub online: bool,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

/// Candidate peer discovered via Tailscale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailscalePeer {
    pub name: String,
    pub address: String,
}

/// Parse `tailscale status --json` output into structured data.
pub fn parse_status(json: &str) -> anyhow::Result<TailscaleStatus> {
    serde_json::from_str(json).map_err(|e| anyhow::anyhow!("Failed to parse tailscale status: {e}"))
}

/// Filter peers by tag. If `tag` is `None`, all online peers are returned.
/// The tag is matched as `"tag:<value>"` in the node's tags list.
///
/// When `use_dns` is true (Tailscale bind mode), addresses use HTTPS DNS names
/// (e.g., `https://linux-server.tailnet.ts.net`) since `tailscale serve` proxies
/// over HTTPS on port 443. Otherwise, addresses use `{ip}:{port}`.
pub fn filter_peers(
    status: &TailscaleStatus,
    tag: Option<&str>,
    port: u16,
    use_dns: bool,
) -> Vec<TailscalePeer> {
    let own_hostname = &status.self_node.host_name;

    status
        .peer
        .values()
        .filter(|node| {
            if !node.online {
                return false;
            }
            if node.host_name == *own_hostname {
                return false;
            }
            tag.is_none_or(|required_tag| {
                let tag_value = format!("tag:{required_tag}");
                node.tags
                    .as_ref()
                    .is_some_and(|tags| tags.iter().any(|t| t == &tag_value))
            })
        })
        .filter_map(|node| {
            let address = if use_dns {
                let dns = node.dns_name.trim_end_matches('.');
                if dns.is_empty() {
                    return None;
                }
                format!("https://{dns}")
            } else {
                let ip = node.tailscale_i_ps.first()?;
                format!("{ip}:{port}")
            };
            Some(TailscalePeer {
                name: node.host_name.clone(),
                address,
            })
        })
        .collect()
}

/// Build the command to run `tailscale status --json`.
pub fn build_status_command() -> std::process::Command {
    let mut cmd = std::process::Command::new("tailscale");
    cmd.args(["status", "--json"]);
    cmd
}

/// Apply one discovery sweep to the peer registry.
///
/// Adds (or refreshes) every peer present in the sweep, then prunes
/// previously-discovered peers that are no longer present. Statically
/// configured peers are never removed — `remove_discovered_peer` protects
/// them by source.
pub async fn apply_discovery_sweep(
    registry: &crate::peers::PeerRegistry,
    peers: &[TailscalePeer],
    own_name: &str,
) {
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for peer in peers {
        if peer.name == own_name {
            continue;
        }
        seen.insert(peer.name.as_str());
        if registry
            .add_discovered_peer(&peer.name, &peer.address)
            .await
        {
            tracing::info!(
                "Tailscale: discovered peer {} at {}",
                peer.name,
                peer.address
            );
        }
    }

    // Prune discovered peers that disappeared from the tailnet.
    for existing in registry.get_all().await {
        if existing.source == pulpo_common::peer::PeerSource::Discovered
            && !seen.contains(existing.name.as_str())
            && registry.remove_discovered_peer(&existing.name).await
        {
            tracing::info!(
                "Tailscale: peer {} no longer present; pruned",
                existing.name
            );
        }
    }
}

/// Run the Tailscale discovery loop — periodically scans the tailnet and updates
/// the peer registry.
///
/// Excluded from coverage builds because it executes real processes.
#[cfg(not(coverage))]
pub async fn run_tailscale_discovery(
    registry: crate::peers::PeerRegistry,
    own_name: String,
    tag: Option<String>,
    interval: std::time::Duration,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    tracing::info!(
        tag = tag.as_deref().unwrap_or("(none)"),
        interval_secs = interval.as_secs(),
        "Tailscale discovery: started"
    );

    loop {
        // Run tailscale status --json
        match tokio::task::spawn_blocking(|| {
            let output = build_status_command().output()?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("tailscale status failed: {stderr}");
            }
            let json = String::from_utf8(output.stdout)?;
            parse_status(&json)
        })
        .await
        {
            Ok(Ok(status)) => {
                let peers = filter_peers(&status, tag.as_deref(), DEFAULT_PULPO_PORT, true);
                apply_discovery_sweep(&registry, &peers, &own_name).await;
            }
            Ok(Err(e)) => {
                tracing::warn!("Tailscale discovery: {e}");
            }
            Err(e) => {
                tracing::error!("Tailscale discovery: task error: {e}");
            }
        }

        // Wait for the next scan or shutdown
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    tracing::info!("Tailscale discovery: shutting down");
                    break;
                }
            }
            () = tokio::time::sleep(interval) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_status_json() -> &'static str {
        r#"{
            "Self": {
                "HostName": "my-mac",
                "DNSName": "my-mac.tailnet-abc.ts.net.",
                "TailscaleIPs": ["100.64.0.1"],
                "Online": true,
                "Tags": ["tag:pulpo"]
            },
            "Peer": {
                "nodekey:abc123": {
                    "HostName": "linux-server",
                    "DNSName": "linux-server.tailnet-abc.ts.net.",
                    "TailscaleIPs": ["100.64.0.2"],
                    "Online": true,
                    "Tags": ["tag:pulpo"]
                },
                "nodekey:def456": {
                    "HostName": "win-pc",
                    "DNSName": "win-pc.tailnet-abc.ts.net.",
                    "TailscaleIPs": ["100.64.0.3"],
                    "Online": true,
                    "Tags": ["tag:other"]
                },
                "nodekey:ghi789": {
                    "HostName": "offline-node",
                    "DNSName": "offline-node.tailnet-abc.ts.net.",
                    "TailscaleIPs": ["100.64.0.4"],
                    "Online": false,
                    "Tags": ["tag:pulpo"]
                }
            }
        }"#
    }

    #[test]
    fn test_parse_status_valid() {
        let status = parse_status(sample_status_json()).unwrap();
        assert_eq!(status.self_node.host_name, "my-mac");
        assert_eq!(status.self_node.dns_name, "my-mac.tailnet-abc.ts.net.");
        assert_eq!(status.self_node.tailscale_i_ps, vec!["100.64.0.1"]);
        assert!(status.self_node.online);
        assert_eq!(status.peer.len(), 3);
    }

    #[test]
    fn test_parse_status_invalid_json() {
        let result = parse_status("not json");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("Failed to parse tailscale status"));
    }

    #[test]
    fn test_parse_status_empty_peers() {
        let json = r#"{
            "Self": {
                "HostName": "lonely-node",
                "DNSName": "lonely.ts.net.",
                "TailscaleIPs": ["100.64.0.1"],
                "Online": true
            },
            "Peer": {}
        }"#;
        let status = parse_status(json).unwrap();
        assert_eq!(status.self_node.host_name, "lonely-node");
        assert!(status.peer.is_empty());
    }

    #[test]
    fn test_parse_status_no_peers_key() {
        let json = r#"{
            "Self": {
                "HostName": "solo",
                "DNSName": "solo.ts.net.",
                "TailscaleIPs": ["100.64.0.1"],
                "Online": true
            }
        }"#;
        let status = parse_status(json).unwrap();
        assert!(status.peer.is_empty());
    }

    #[test]
    fn test_parse_status_node_no_tags() {
        let json = r#"{
            "Self": {
                "HostName": "no-tags",
                "DNSName": "no-tags.ts.net.",
                "TailscaleIPs": ["100.64.0.1"],
                "Online": true
            },
            "Peer": {
                "nodekey:abc": {
                    "HostName": "peer-no-tags",
                    "DNSName": "peer.ts.net.",
                    "TailscaleIPs": ["100.64.0.2"],
                    "Online": true
                }
            }
        }"#;
        let status = parse_status(json).unwrap();
        assert!(status.peer["nodekey:abc"].tags.is_none());
    }

    #[test]
    fn test_filter_peers_by_tag() {
        let status = parse_status(sample_status_json()).unwrap();
        let peers = filter_peers(&status, Some("pulpo"), DEFAULT_PULPO_PORT, false);
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].name, "linux-server");
        assert_eq!(peers[0].address, "100.64.0.2:7433");
    }

    #[test]
    fn test_filter_peers_no_tag_filter() {
        let status = parse_status(sample_status_json()).unwrap();
        let peers = filter_peers(&status, None, DEFAULT_PULPO_PORT, false);
        // Should include all online peers (linux-server + win-pc), excluding offline
        assert_eq!(peers.len(), 2);
        let names: Vec<&str> = peers.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"linux-server"));
        assert!(names.contains(&"win-pc"));
    }

    #[test]
    fn test_filter_peers_excludes_offline() {
        let status = parse_status(sample_status_json()).unwrap();
        let peers = filter_peers(&status, None, DEFAULT_PULPO_PORT, false);
        assert!(!peers.iter().any(|p| p.name == "offline-node"));
    }

    #[test]
    fn test_filter_peers_excludes_self() {
        // If own hostname appears as a peer, it should be skipped
        let json = r#"{
            "Self": {
                "HostName": "my-mac",
                "DNSName": "my-mac.ts.net.",
                "TailscaleIPs": ["100.64.0.1"],
                "Online": true
            },
            "Peer": {
                "nodekey:self": {
                    "HostName": "my-mac",
                    "DNSName": "my-mac.ts.net.",
                    "TailscaleIPs": ["100.64.0.1"],
                    "Online": true
                }
            }
        }"#;
        let status = parse_status(json).unwrap();
        let peers = filter_peers(&status, None, DEFAULT_PULPO_PORT, false);
        assert!(peers.is_empty());
    }

    #[test]
    fn test_filter_peers_no_matching_tag() {
        let status = parse_status(sample_status_json()).unwrap();
        let peers = filter_peers(&status, Some("nonexistent"), DEFAULT_PULPO_PORT, false);
        assert!(peers.is_empty());
    }

    #[test]
    fn test_filter_peers_custom_port() {
        let status = parse_status(sample_status_json()).unwrap();
        let peers = filter_peers(&status, Some("pulpo"), 9000, false);
        assert_eq!(peers[0].address, "100.64.0.2:9000");
    }

    #[test]
    fn test_filter_peers_node_without_ips() {
        let json = r#"{
            "Self": {
                "HostName": "self",
                "DNSName": "self.ts.net.",
                "TailscaleIPs": ["100.64.0.1"],
                "Online": true
            },
            "Peer": {
                "nodekey:no-ip": {
                    "HostName": "no-ip-node",
                    "DNSName": "no-ip.ts.net.",
                    "TailscaleIPs": [],
                    "Online": true
                }
            }
        }"#;
        let status = parse_status(json).unwrap();
        let peers = filter_peers(&status, None, DEFAULT_PULPO_PORT, false);
        assert!(peers.is_empty());
    }

    #[test]
    fn test_filter_peers_dns_mode() {
        let status = parse_status(sample_status_json()).unwrap();
        let peers = filter_peers(&status, Some("pulpo"), DEFAULT_PULPO_PORT, true);
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].name, "linux-server");
        assert_eq!(peers[0].address, "https://linux-server.tailnet-abc.ts.net");
    }

    #[test]
    fn test_filter_peers_dns_mode_no_tag() {
        let status = parse_status(sample_status_json()).unwrap();
        let peers = filter_peers(&status, None, DEFAULT_PULPO_PORT, true);
        assert_eq!(peers.len(), 2);
        let addrs: Vec<&str> = peers.iter().map(|p| p.address.as_str()).collect();
        assert!(addrs.contains(&"https://linux-server.tailnet-abc.ts.net"));
        assert!(addrs.contains(&"https://win-pc.tailnet-abc.ts.net"));
    }

    #[test]
    fn test_filter_peers_dns_mode_empty_dns_name() {
        let json = r#"{
            "Self": {
                "HostName": "self",
                "DNSName": "self.ts.net.",
                "TailscaleIPs": ["100.64.0.1"],
                "Online": true
            },
            "Peer": {
                "nodekey:empty-dns": {
                    "HostName": "no-dns",
                    "DNSName": "",
                    "TailscaleIPs": ["100.64.0.2"],
                    "Online": true
                }
            }
        }"#;
        let status = parse_status(json).unwrap();
        let peers = filter_peers(&status, None, DEFAULT_PULPO_PORT, true);
        assert!(peers.is_empty());
    }

    #[test]
    fn test_filter_peers_dns_mode_strips_trailing_dot() {
        let json = r#"{
            "Self": {
                "HostName": "self",
                "DNSName": "self.ts.net.",
                "TailscaleIPs": ["100.64.0.1"],
                "Online": true
            },
            "Peer": {
                "nodekey:dotted": {
                    "HostName": "dotted-node",
                    "DNSName": "dotted-node.tailnet.ts.net.",
                    "TailscaleIPs": ["100.64.0.2"],
                    "Online": true
                }
            }
        }"#;
        let status = parse_status(json).unwrap();
        let peers = filter_peers(&status, None, DEFAULT_PULPO_PORT, true);
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].address, "https://dotted-node.tailnet.ts.net");
    }

    #[test]
    fn test_filter_peers_dns_mode_node_without_ips_still_works() {
        // In DNS mode, IPs aren't needed — DNS name is sufficient
        let json = r#"{
            "Self": {
                "HostName": "self",
                "DNSName": "self.ts.net.",
                "TailscaleIPs": ["100.64.0.1"],
                "Online": true
            },
            "Peer": {
                "nodekey:no-ip": {
                    "HostName": "no-ip-node",
                    "DNSName": "no-ip.tailnet.ts.net.",
                    "TailscaleIPs": [],
                    "Online": true
                }
            }
        }"#;
        let status = parse_status(json).unwrap();
        let peers = filter_peers(&status, None, DEFAULT_PULPO_PORT, true);
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].address, "https://no-ip.tailnet.ts.net");
    }

    #[tokio::test]
    async fn test_apply_discovery_sweep_adds_and_prunes_discovered_peers() {
        let registry = crate::peers::PeerRegistry::new(&std::collections::HashMap::new());
        // Previously discovered peer that will vanish from the tailnet
        registry
            .add_discovered_peer("gone-node", "100.64.0.9:7433")
            .await;

        let peers = vec![TailscalePeer {
            name: "new-node".into(),
            address: "100.64.0.2:7433".into(),
        }];
        apply_discovery_sweep(&registry, &peers, "my-mac").await;

        assert!(registry.get("new-node").await.is_some());
        assert!(
            registry.get("gone-node").await.is_none(),
            "peer absent from the sweep must be pruned"
        );
    }

    #[tokio::test]
    async fn test_apply_discovery_sweep_preserves_configured_peers() {
        let mut configured = std::collections::HashMap::new();
        configured.insert(
            "static-node".into(),
            pulpo_common::peer::PeerEntry::Simple("10.0.0.1:7433".into()),
        );
        let registry = crate::peers::PeerRegistry::new(&configured);

        // Sweep result does not contain the configured peer
        apply_discovery_sweep(&registry, &[], "my-mac").await;

        let peer = registry.get("static-node").await;
        assert!(
            peer.is_some(),
            "configured peers must never be pruned by discovery"
        );
        assert_eq!(
            peer.unwrap().source,
            pulpo_common::peer::PeerSource::Configured
        );
    }

    #[tokio::test]
    async fn test_apply_discovery_sweep_skips_own_name() {
        let registry = crate::peers::PeerRegistry::new(&std::collections::HashMap::new());
        let peers = vec![TailscalePeer {
            name: "my-mac".into(),
            address: "100.64.0.1:7433".into(),
        }];
        apply_discovery_sweep(&registry, &peers, "my-mac").await;
        assert!(registry.get("my-mac").await.is_none());
    }

    #[tokio::test]
    async fn test_apply_discovery_sweep_keeps_still_present_discovered_peer() {
        let registry = crate::peers::PeerRegistry::new(&std::collections::HashMap::new());
        registry
            .add_discovered_peer("stable-node", "100.64.0.5:7433")
            .await;

        let peers = vec![TailscalePeer {
            name: "stable-node".into(),
            address: "100.64.0.5:7433".into(),
        }];
        apply_discovery_sweep(&registry, &peers, "my-mac").await;

        assert!(registry.get("stable-node").await.is_some());
    }

    #[test]
    fn test_build_status_command() {
        let cmd = build_status_command();
        assert_eq!(cmd.get_program(), "tailscale");
        let args: Vec<&std::ffi::OsStr> = cmd.get_args().collect();
        assert_eq!(args, vec!["status", "--json"]);
    }

    #[test]
    fn test_tailscale_peer_debug() {
        let peer = TailscalePeer {
            name: "test".into(),
            address: "100.64.0.1:7433".into(),
        };
        let debug = format!("{peer:?}");
        assert!(debug.contains("test"));
    }

    #[test]
    fn test_tailscale_peer_clone() {
        let peer = TailscalePeer {
            name: "test".into(),
            address: "100.64.0.1:7433".into(),
        };
        #[allow(clippy::redundant_clone)]
        let cloned = peer.clone();
        assert_eq!(peer, cloned);
    }

    #[test]
    fn test_tailscale_status_debug() {
        let status = parse_status(sample_status_json()).unwrap();
        let debug = format!("{status:?}");
        assert!(debug.contains("my-mac"));
    }

    #[test]
    fn test_tailscale_node_debug() {
        let status = parse_status(sample_status_json()).unwrap();
        let debug = format!("{:?}", status.self_node);
        assert!(debug.contains("my-mac"));
    }
}
