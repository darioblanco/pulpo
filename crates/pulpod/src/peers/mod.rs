pub mod health;

use std::collections::HashMap;
use std::sync::Arc;

use pulpo_common::node::NodeInfo;
use pulpo_common::peer::{PeerEntry, PeerInfo, PeerSource, PeerStatus};
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
struct CachedPeer {
    name: String,
    address: String,
    token: Option<String>,
    status: PeerStatus,
    node_info: Option<NodeInfo>,
    session_count: Option<usize>,
    source: PeerSource,
}

impl CachedPeer {
    fn to_peer_info(&self) -> PeerInfo {
        PeerInfo {
            name: self.name.clone(),
            address: self.address.clone(),
            status: self.status,
            node_info: self.node_info.clone(),
            session_count: self.session_count,
            source: self.source,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PeerRegistry {
    peers: Arc<RwLock<HashMap<String, CachedPeer>>>,
}

impl PeerRegistry {
    pub fn new(configured_peers: &HashMap<String, PeerEntry>) -> Self {
        let mut peers = HashMap::new();
        for (name, entry) in configured_peers {
            peers.insert(
                name.clone(),
                CachedPeer {
                    name: name.clone(),
                    address: entry.address().to_owned(),
                    token: entry.token().map(String::from),
                    status: PeerStatus::Unknown,
                    node_info: None,
                    session_count: None,
                    source: PeerSource::Configured,
                },
            );
        }
        Self {
            peers: Arc::new(RwLock::new(peers)),
        }
    }

    pub async fn get_all(&self) -> Vec<PeerInfo> {
        let peers = self.peers.read().await;
        peers.values().map(CachedPeer::to_peer_info).collect()
    }

    pub async fn get(&self, name: &str) -> Option<PeerInfo> {
        let peers = self.peers.read().await;
        peers.get(name).map(CachedPeer::to_peer_info)
    }

    pub async fn update_status(
        &self,
        name: &str,
        status: PeerStatus,
        node_info: Option<NodeInfo>,
        session_count: Option<usize>,
    ) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(name) {
            peer.status = status;
            peer.node_info = node_info;
            peer.session_count = session_count;
        }
    }

    pub async fn peer_count(&self) -> usize {
        self.peers.read().await.len()
    }

    /// Return the token for a peer (if configured).
    pub async fn get_token(&self, name: &str) -> Option<String> {
        let peers = self.peers.read().await;
        peers.get(name).and_then(|p| p.token.clone())
    }

    /// Add a new configured peer. Returns `false` if a peer with the same name already exists.
    pub async fn add_peer(&self, name: &str, address: &str, token: Option<&str>) -> bool {
        let mut peers = self.peers.write().await;
        if peers.contains_key(name) {
            return false;
        }
        peers.insert(
            name.to_owned(),
            CachedPeer {
                name: name.to_owned(),
                address: address.to_owned(),
                token: token.map(String::from),
                status: PeerStatus::Unknown,
                node_info: None,
                session_count: None,
                source: PeerSource::Configured,
            },
        );
        true
    }

    /// Add or update a discovered peer. Will not overwrite configured peers.
    pub async fn add_discovered_peer(&self, name: &str, address: &str) -> bool {
        let mut peers = self.peers.write().await;
        if let Some(existing) = peers.get(name)
            && existing.source == PeerSource::Configured
        {
            return false;
        }
        peers.insert(
            name.to_owned(),
            CachedPeer {
                name: name.to_owned(),
                address: address.to_owned(),
                token: None,
                status: PeerStatus::Unknown,
                node_info: None,
                session_count: None,
                source: PeerSource::Discovered,
            },
        );
        true
    }

    /// Remove a discovered peer. Configured peers are protected.
    pub async fn remove_discovered_peer(&self, name: &str) -> bool {
        let mut peers = self.peers.write().await;
        let is_configured = peers
            .get(name)
            .is_some_and(|p| p.source == PeerSource::Configured);
        if is_configured {
            return false;
        }
        peers.remove(name).is_some()
    }

    /// Remove a peer by name. Returns `false` if the peer was not found.
    pub async fn remove_peer(&self, name: &str) -> bool {
        let mut peers = self.peers.write().await;
        peers.remove(name).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_new_empty() {
        let registry = PeerRegistry::new(&HashMap::new());
        let peers = registry.get_all().await;
        assert!(peers.is_empty());
    }

    #[tokio::test]
    async fn test_new_with_peers() {
        let mut configured = HashMap::new();
        configured.insert("node-a".into(), PeerEntry::Simple("10.0.0.1:7433".into()));
        configured.insert("node-b".into(), PeerEntry::Simple("10.0.0.2:7433".into()));
        let registry = PeerRegistry::new(&configured);
        let peers = registry.get_all().await;
        assert_eq!(peers.len(), 2);
        for peer in &peers {
            assert_eq!(peer.status, PeerStatus::Unknown);
            assert!(peer.node_info.is_none());
            assert!(peer.session_count.is_none());
        }
    }

    #[tokio::test]
    async fn test_get_existing_peer() {
        let mut configured = HashMap::new();
        configured.insert("node-a".into(), PeerEntry::Simple("10.0.0.1:7433".into()));
        let registry = PeerRegistry::new(&configured);
        let peer = registry.get("node-a").await;
        assert!(peer.is_some());
        let peer = peer.unwrap();
        assert_eq!(peer.name, "node-a");
        assert_eq!(peer.address, "10.0.0.1:7433");
    }

    #[tokio::test]
    async fn test_get_nonexistent_peer() {
        let registry = PeerRegistry::new(&HashMap::new());
        assert!(registry.get("missing").await.is_none());
    }

    #[tokio::test]
    async fn test_update_status_online() {
        let mut configured = HashMap::new();
        configured.insert("node-a".into(), PeerEntry::Simple("10.0.0.1:7433".into()));
        let registry = PeerRegistry::new(&configured);

        let node_info = NodeInfo {
            name: "node-a".into(),
            hostname: "host-a".into(),
            os: "macos".into(),
            arch: "aarch64".into(),
            cpus: 8,
            memory_mb: 16384,
            gpu: None,
        };
        registry
            .update_status("node-a", PeerStatus::Online, Some(node_info), Some(5))
            .await;

        let peer = registry.get("node-a").await.unwrap();
        assert_eq!(peer.status, PeerStatus::Online);
        assert!(peer.node_info.is_some());
        assert_eq!(peer.node_info.unwrap().cpus, 8);
        assert_eq!(peer.session_count, Some(5));
    }

    #[tokio::test]
    async fn test_update_status_offline() {
        let mut configured = HashMap::new();
        configured.insert("node-a".into(), PeerEntry::Simple("10.0.0.1:7433".into()));
        let registry = PeerRegistry::new(&configured);

        registry
            .update_status("node-a", PeerStatus::Offline, None, None)
            .await;

        let peer = registry.get("node-a").await.unwrap();
        assert_eq!(peer.status, PeerStatus::Offline);
        assert!(peer.node_info.is_none());
        assert!(peer.session_count.is_none());
    }

    #[tokio::test]
    async fn test_update_status_nonexistent_peer() {
        let registry = PeerRegistry::new(&HashMap::new());
        // Should not panic — silently ignores unknown peers
        registry
            .update_status("missing", PeerStatus::Online, None, None)
            .await;
        assert!(registry.get("missing").await.is_none());
    }

    #[tokio::test]
    async fn test_registry_clone() {
        let mut configured = HashMap::new();
        configured.insert("node-a".into(), PeerEntry::Simple("10.0.0.1:7433".into()));
        let registry = PeerRegistry::new(&configured);
        let cloned = registry.clone();

        registry
            .update_status("node-a", PeerStatus::Online, None, Some(2))
            .await;

        // Clone shares the same Arc, so update visible from both
        let peer = cloned.get("node-a").await.unwrap();
        assert_eq!(peer.status, PeerStatus::Online);
        assert_eq!(peer.session_count, Some(2));
    }

    #[tokio::test]
    async fn test_registry_debug() {
        let registry = PeerRegistry::new(&HashMap::new());
        let debug = format!("{registry:?}");
        assert!(debug.contains("PeerRegistry"));
    }

    #[test]
    fn test_cached_peer_debug() {
        let peer = CachedPeer {
            name: "test".into(),
            address: "host:7433".into(),
            token: None,
            status: PeerStatus::Online,
            node_info: None,
            session_count: None,
            source: PeerSource::Configured,
        };
        let debug = format!("{peer:?}");
        assert!(debug.contains("test"));
    }

    #[test]
    fn test_cached_peer_clone() {
        let peer = CachedPeer {
            name: "test".into(),
            address: "host:7433".into(),
            token: None,
            status: PeerStatus::Online,
            node_info: None,
            session_count: None,
            source: PeerSource::Configured,
        };
        let cloned = peer.clone();
        assert_eq!(cloned.name, peer.name);
    }

    #[test]
    fn test_cached_peer_to_peer_info() {
        let peer = CachedPeer {
            name: "node-a".into(),
            address: "10.0.0.1:7433".into(),
            token: None,
            status: PeerStatus::Offline,
            node_info: None,
            session_count: Some(3),
            source: PeerSource::Configured,
        };
        let info = peer.to_peer_info();
        assert_eq!(info.name, "node-a");
        assert_eq!(info.address, "10.0.0.1:7433");
        assert_eq!(info.status, PeerStatus::Offline);
        assert_eq!(info.session_count, Some(3));
    }

    #[tokio::test]
    async fn test_add_peer() {
        let registry = PeerRegistry::new(&HashMap::new());
        assert!(registry.add_peer("new-node", "10.0.0.5:7433", None).await);
        assert_eq!(registry.peer_count().await, 1);
        let peer = registry.get("new-node").await.unwrap();
        assert_eq!(peer.address, "10.0.0.5:7433");
        assert_eq!(peer.status, PeerStatus::Unknown);
    }

    #[tokio::test]
    async fn test_add_peer_duplicate() {
        let mut configured = HashMap::new();
        configured.insert("existing".into(), PeerEntry::Simple("10.0.0.1:7433".into()));
        let registry = PeerRegistry::new(&configured);
        assert!(!registry.add_peer("existing", "10.0.0.2:7433", None).await);
        // Address should not have changed
        let peer = registry.get("existing").await.unwrap();
        assert_eq!(peer.address, "10.0.0.1:7433");
    }

    #[tokio::test]
    async fn test_remove_peer() {
        let mut configured = HashMap::new();
        configured.insert(
            "to-remove".into(),
            PeerEntry::Simple("10.0.0.1:7433".into()),
        );
        let registry = PeerRegistry::new(&configured);
        assert!(registry.remove_peer("to-remove").await);
        assert_eq!(registry.peer_count().await, 0);
        assert!(registry.get("to-remove").await.is_none());
    }

    #[tokio::test]
    async fn test_remove_peer_nonexistent() {
        let registry = PeerRegistry::new(&HashMap::new());
        assert!(!registry.remove_peer("missing").await);
    }

    #[tokio::test]
    async fn test_peer_count() {
        let mut configured = HashMap::new();
        configured.insert("node-a".into(), PeerEntry::Simple("10.0.0.1:7433".into()));
        let registry = PeerRegistry::new(&configured);
        assert_eq!(registry.peer_count().await, 1);
    }

    #[tokio::test]
    async fn test_new_with_full_entry() {
        let mut configured = HashMap::new();
        configured.insert(
            "authed".into(),
            PeerEntry::Full {
                address: "10.0.0.1:7433".into(),
                token: Some("secret".into()),
            },
        );
        let registry = PeerRegistry::new(&configured);
        let peer = registry.get("authed").await.unwrap();
        assert_eq!(peer.address, "10.0.0.1:7433");
        assert_eq!(registry.get_token("authed").await, Some("secret".into()));
    }

    #[tokio::test]
    async fn test_get_token_simple_entry() {
        let mut configured = HashMap::new();
        configured.insert("simple".into(), PeerEntry::Simple("h:1".into()));
        let registry = PeerRegistry::new(&configured);
        assert_eq!(registry.get_token("simple").await, None);
    }

    #[tokio::test]
    async fn test_get_token_nonexistent() {
        let registry = PeerRegistry::new(&HashMap::new());
        assert_eq!(registry.get_token("missing").await, None);
    }

    #[tokio::test]
    async fn test_add_peer_with_token() {
        let registry = PeerRegistry::new(&HashMap::new());
        assert!(registry.add_peer("node", "h:1", Some("tok")).await);
        assert_eq!(registry.get_token("node").await, Some("tok".into()));
    }

    #[tokio::test]
    async fn test_add_discovered_peer() {
        let registry = PeerRegistry::new(&HashMap::new());
        assert!(
            registry
                .add_discovered_peer("disc-node", "10.0.0.5:7433")
                .await
        );
        assert_eq!(registry.peer_count().await, 1);
        let peer = registry.get("disc-node").await.unwrap();
        assert_eq!(peer.address, "10.0.0.5:7433");
        assert_eq!(peer.source, PeerSource::Discovered);
        assert_eq!(peer.status, PeerStatus::Unknown);
    }

    #[tokio::test]
    async fn test_add_discovered_peer_protected_from_configured() {
        let mut configured = HashMap::new();
        configured.insert("existing".into(), PeerEntry::Simple("10.0.0.1:7433".into()));
        let registry = PeerRegistry::new(&configured);
        // Should not overwrite configured peer
        assert!(
            !registry
                .add_discovered_peer("existing", "10.0.0.2:7433")
                .await
        );
        let peer = registry.get("existing").await.unwrap();
        assert_eq!(peer.address, "10.0.0.1:7433");
        assert_eq!(peer.source, PeerSource::Configured);
    }

    #[tokio::test]
    async fn test_add_discovered_peer_overwrites_discovered() {
        let registry = PeerRegistry::new(&HashMap::new());
        assert!(registry.add_discovered_peer("disc", "10.0.0.1:7433").await);
        // Re-discovering with new address should overwrite
        assert!(registry.add_discovered_peer("disc", "10.0.0.2:7433").await);
        let peer = registry.get("disc").await.unwrap();
        assert_eq!(peer.address, "10.0.0.2:7433");
    }

    #[tokio::test]
    async fn test_remove_discovered_peer() {
        let registry = PeerRegistry::new(&HashMap::new());
        registry.add_discovered_peer("disc", "10.0.0.1:7433").await;
        assert!(registry.remove_discovered_peer("disc").await);
        assert_eq!(registry.peer_count().await, 0);
        assert!(registry.get("disc").await.is_none());
    }

    #[tokio::test]
    async fn test_remove_discovered_peer_protects_configured() {
        let mut configured = HashMap::new();
        configured.insert("conf".into(), PeerEntry::Simple("10.0.0.1:7433".into()));
        let registry = PeerRegistry::new(&configured);
        assert!(!registry.remove_discovered_peer("conf").await);
        assert_eq!(registry.peer_count().await, 1);
    }

    #[tokio::test]
    async fn test_remove_discovered_peer_nonexistent() {
        let registry = PeerRegistry::new(&HashMap::new());
        assert!(!registry.remove_discovered_peer("missing").await);
    }

    #[tokio::test]
    async fn test_to_peer_info_includes_source() {
        let registry = PeerRegistry::new(&HashMap::new());
        registry.add_discovered_peer("disc", "h:1").await;
        registry.add_peer("conf", "h:2", None).await;
        let disc = registry.get("disc").await.unwrap();
        let conf = registry.get("conf").await.unwrap();
        assert_eq!(disc.source, PeerSource::Discovered);
        assert_eq!(conf.source, PeerSource::Configured);
    }

    #[tokio::test]
    async fn test_get_all_includes_both_sources() {
        let registry = PeerRegistry::new(&HashMap::new());
        registry.add_peer("conf", "h:1", None).await;
        registry.add_discovered_peer("disc", "h:2").await;
        let all = registry.get_all().await;
        assert_eq!(all.len(), 2);
        let sources: Vec<PeerSource> = all.iter().map(|p| p.source).collect();
        assert!(sources.contains(&PeerSource::Configured));
        assert!(sources.contains(&PeerSource::Discovered));
    }
}
