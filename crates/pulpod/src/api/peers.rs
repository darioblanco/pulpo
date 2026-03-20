use std::sync::Arc;

use axum::http::StatusCode;
use axum::{
    Json,
    extract::{Path, State},
};
use pulpo_common::api::{AddPeerRequest, ErrorResponse, PeersResponse};
use pulpo_common::node::NodeInfo;
use pulpo_common::peer::PeerEntry;

use super::node::get_hostname;
use crate::watchdog::memory::{MemoryReader, SystemMemoryReader};

/// Best-effort GPU detection. Returns a label like "Apple Metal" or "NVIDIA" if
/// a GPU is likely present, `None` otherwise.
fn detect_gpu() -> Option<String> {
    detect_gpu_inner()
}

#[cfg(target_os = "macos")]
fn detect_gpu_inner() -> Option<String> {
    // All supported Macs (Apple Silicon) have Metal GPU
    if std::env::consts::ARCH == "aarch64" {
        Some("Apple Metal".into())
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn detect_gpu_inner() -> Option<String> {
    if std::path::Path::new("/dev/nvidia0").exists() {
        return Some("NVIDIA".into());
    }
    if std::path::Path::new("/dev/dri").exists() {
        return Some("GPU (DRI)".into());
    }
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn detect_gpu_inner() -> Option<String> {
    None
}

pub async fn list_peers(State(state): State<Arc<super::AppState>>) -> Json<PeersResponse> {
    let config = state.config.read().await;
    let memory_mb = SystemMemoryReader
        .read_memory()
        .map(|s| s.total_mb)
        .unwrap_or(0);
    let local = NodeInfo {
        name: config.node.name.clone(),
        hostname: get_hostname(),
        os: crate::platform::os_name().into(),
        arch: std::env::consts::ARCH.into(),
        cpus: num_cpus::get(),
        memory_mb,
        gpu: detect_gpu(),
    };
    drop(config);

    // Probe all peers on-demand (results are cached with a 60s TTL).
    // Gated behind cfg(not(coverage)) because CachedProber<HttpPeerProber> would
    // attempt real HTTP connections in tests, causing hangs/timeouts. The probing
    // logic itself is thoroughly tested in peers::health tests.
    #[cfg(not(coverage))]
    if let Some(prober) = &state.cached_prober {
        prober.probe_all(&state.peer_registry).await;
    }

    let peers = state.peer_registry.get_all().await;

    Json(PeersResponse { local, peers })
}

pub async fn add_peer(
    State(state): State<Arc<super::AppState>>,
    Json(req): Json<AddPeerRequest>,
) -> Result<(StatusCode, Json<PeersResponse>), (StatusCode, Json<ErrorResponse>)> {
    if req.name.is_empty() || req.address.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Name and address are required".into(),
            }),
        ));
    }

    let added = state
        .peer_registry
        .add_peer(&req.name, &req.address, None)
        .await;
    if !added {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: format!("Peer '{}' already exists", req.name),
            }),
        ));
    }

    // Update config and save to disk
    let mut config = state.config.write().await;
    config
        .peers
        .insert(req.name, PeerEntry::Simple(req.address));
    let _ = crate::config::save(&config, &state.config_path);
    drop(config);

    // Return updated peers list
    let resp = list_peers(State(state)).await;
    Ok((StatusCode::CREATED, resp))
}

pub async fn remove_peer(
    State(state): State<Arc<super::AppState>>,
    Path(name): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let removed = state.peer_registry.remove_peer(&name).await;
    if !removed {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Peer '{name}' not found"),
            }),
        ));
    }

    // Update config and save to disk
    let mut config = state.config.write().await;
    config.peers.remove(&name);
    let _ = crate::config::save(&config, &state.config_path);
    drop(config);

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use axum::extract::State;
    use pulpo_common::peer::{PeerEntry, PeerStatus};

    use crate::api::AppState;
    use crate::backend::StubBackend;
    use crate::config::{Config, NodeConfig};
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;

    async fn test_state_with_peers(peers_config: HashMap<String, PeerEntry>) -> Arc<AppState> {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&peers_config);
        AppState::new(
            Config {
                node: NodeConfig {
                    name: "local-node".into(),
                    port: 7433,
                    data_dir: tmpdir.path().to_str().unwrap().into(),
                    ..NodeConfig::default()
                },
                auth: crate::config::AuthConfig::default(),
                peers: peers_config,
                watchdog: crate::config::WatchdogConfig::default(),
                inks: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
                docker: crate::config::DockerConfig::default(),
            },
            manager,
            peer_registry,
            store,
        )
    }

    #[tokio::test]
    async fn test_list_peers_no_peers() {
        let state = test_state_with_peers(HashMap::new()).await;
        let Json(resp) = list_peers(State(state)).await;
        assert_eq!(resp.local.name, "local-node");
        assert!(resp.peers.is_empty());
    }

    #[tokio::test]
    async fn test_list_peers_with_peers() {
        let mut peers_config = HashMap::new();
        peers_config.insert("remote-a".into(), PeerEntry::Simple("10.0.0.1:7433".into()));
        peers_config.insert("remote-b".into(), PeerEntry::Simple("10.0.0.2:7433".into()));
        let state = test_state_with_peers(peers_config).await;
        let Json(resp) = list_peers(State(state)).await;
        assert_eq!(resp.local.name, "local-node");
        assert_eq!(resp.peers.len(), 2);
        // All peers should start as Unknown
        for peer in &resp.peers {
            assert_eq!(peer.status, PeerStatus::Unknown);
        }
    }

    #[tokio::test]
    async fn test_list_peers_with_updated_status() {
        let mut peers_config = HashMap::new();
        peers_config.insert("remote".into(), PeerEntry::Simple("10.0.0.1:7433".into()));
        let state = test_state_with_peers(peers_config).await;

        // Simulate health check updating status
        state
            .peer_registry
            .update_status(
                "remote",
                PeerStatus::Online,
                Some(NodeInfo {
                    name: "remote".into(),
                    hostname: "host".into(),
                    os: "linux".into(),
                    arch: "x86_64".into(),
                    cpus: 4,
                    memory_mb: 8192,
                    gpu: None,
                }),
                Some(3),
            )
            .await;

        let Json(resp) = list_peers(State(state)).await;
        assert_eq!(resp.peers.len(), 1);
        assert_eq!(resp.peers[0].status, PeerStatus::Online);
        assert_eq!(resp.peers[0].session_count, Some(3));
        assert!(resp.peers[0].node_info.is_some());
    }

    #[tokio::test]
    async fn test_list_peers_local_info() {
        let state = test_state_with_peers(HashMap::new()).await;
        let Json(resp) = list_peers(State(state)).await;
        // Local info should be populated
        assert!(!resp.local.hostname.is_empty());
        assert!(!resp.local.os.is_empty());
        assert!(!resp.local.arch.is_empty());
        assert!(resp.local.cpus > 0);
        assert!(resp.local.memory_mb > 0);
    }

    #[tokio::test]
    async fn test_add_peer_success() {
        let state = test_state_with_peers(HashMap::new()).await;
        let req = AddPeerRequest {
            name: "new-node".into(),
            address: "10.0.0.5:7433".into(),
        };
        let result = add_peer(State(state.clone()), Json(req)).await;
        assert!(result.is_ok());
        let (status, Json(resp)) = result.unwrap();
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(resp.peers.len(), 1);
        assert_eq!(resp.peers[0].name, "new-node");
        // Verify config was updated
        let has_peer = state.config.read().await.peers.contains_key("new-node");
        assert!(has_peer);
    }

    #[tokio::test]
    async fn test_add_peer_duplicate() {
        let mut peers = HashMap::new();
        peers.insert("existing".into(), PeerEntry::Simple("10.0.0.1:7433".into()));
        let state = test_state_with_peers(peers).await;
        let req = AddPeerRequest {
            name: "existing".into(),
            address: "10.0.0.2:7433".into(),
        };
        let result = add_peer(State(state), Json(req)).await;
        assert!(result.is_err());
        let (status, Json(err)) = result.unwrap_err();
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(err.error.contains("already exists"));
    }

    #[tokio::test]
    async fn test_add_peer_empty_name() {
        let state = test_state_with_peers(HashMap::new()).await;
        let req = AddPeerRequest {
            name: String::new(),
            address: "10.0.0.1:7433".into(),
        };
        let result = add_peer(State(state), Json(req)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_add_peer_empty_address() {
        let state = test_state_with_peers(HashMap::new()).await;
        let req = AddPeerRequest {
            name: "node".into(),
            address: String::new(),
        };
        let result = add_peer(State(state), Json(req)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_remove_peer_success() {
        let mut peers = HashMap::new();
        peers.insert(
            "to-remove".into(),
            PeerEntry::Simple("10.0.0.1:7433".into()),
        );
        let state = test_state_with_peers(peers).await;
        let result = remove_peer(State(state.clone()), Path("to-remove".into())).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), StatusCode::NO_CONTENT);
        // Verify config was updated
        let has_peer = state.config.read().await.peers.contains_key("to-remove");
        assert!(!has_peer);
    }

    #[tokio::test]
    async fn test_remove_peer_not_found() {
        let state = test_state_with_peers(HashMap::new()).await;
        let result = remove_peer(State(state), Path("missing".into())).await;
        assert!(result.is_err());
        let (status, Json(err)) = result.unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(err.error.contains("not found"));
    }

    #[test]
    fn test_detect_gpu_returns_value() {
        // On macOS aarch64 this should return Some("Apple Metal")
        // On Linux it depends on /dev paths
        // Just verify it doesn't panic
        let result = detect_gpu();
        if cfg!(target_os = "macos") && std::env::consts::ARCH == "aarch64" {
            assert_eq!(result, Some("Apple Metal".into()));
        }
    }
}
