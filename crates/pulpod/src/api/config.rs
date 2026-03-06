use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode};
use pulpo_common::api::{
    AuthConfigResponse, ConfigResponse, ErrorResponse, GuardDefaultConfigResponse,
    NodeConfigResponse, UpdateConfigRequest, UpdateConfigResponse,
};

type ApiError = (StatusCode, Json<ErrorResponse>);

fn internal_error(msg: &str) -> ApiError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: msg.to_owned(),
        }),
    )
}

fn config_to_response(config: &crate::config::Config) -> ConfigResponse {
    ConfigResponse {
        node: NodeConfigResponse {
            name: config.node.name.clone(),
            port: config.node.port,
            data_dir: config.node.data_dir.clone(),
        },
        auth: AuthConfigResponse {
            bind: config.auth.bind,
        },
        peers: config.peers.clone(),
        guards: GuardDefaultConfigResponse {
            preset: config.guards.preset,
        },
    }
}

pub async fn get_config(
    State(state): State<Arc<super::AppState>>,
) -> Result<Json<ConfigResponse>, ApiError> {
    let config = state.config.read().await;
    let response = config_to_response(&config);
    drop(config);
    Ok(Json(response))
}

pub async fn update_config(
    State(state): State<Arc<super::AppState>>,
    Json(req): Json<UpdateConfigRequest>,
) -> Result<Json<UpdateConfigResponse>, ApiError> {
    let mut config = state.config.write().await;
    let original_port = config.node.port;
    let original_bind = config.auth.bind;

    if let Some(name) = &req.node_name {
        config.node.name.clone_from(name);
    }
    if let Some(port) = req.port {
        config.node.port = port;
    }
    if let Some(data_dir) = &req.data_dir {
        config.node.data_dir.clone_from(data_dir);
    }
    if let Some(bind) = req.bind {
        config.auth.bind = bind;
    }
    if let Some(preset) = req.guard_preset {
        config.guards.preset = preset;
    }
    if let Some(peers) = req.peers {
        config.peers = peers;
    }

    let restart_required = config.node.port != original_port || config.auth.bind != original_bind;

    // Save to disk if config_path is set
    if !state.config_path.as_os_str().is_empty() {
        crate::config::save(&config, &state.config_path)
            .map_err(|e| internal_error(&e.to_string()))?;
    }

    let response = config_to_response(&config);
    drop(config);
    Ok(Json(UpdateConfigResponse {
        config: response,
        restart_required,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::AppState;
    use crate::backend::Backend;
    use std::collections::HashMap;

    use crate::config::{Config, GuardDefaultConfig, NodeConfig};
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;
    use anyhow::Result;
    use axum::extract::State;
    use pulpo_common::peer::PeerEntry;

    struct StubBackend;

    impl Backend for StubBackend {
        fn session_id(&self, name: &str) -> String {
            name.to_owned()
        }
        fn spawn_attach(&self, _: &str) -> anyhow::Result<tokio::process::Child> {
            anyhow::bail!("not supported in mock")
        }
        fn create_session(&self, _: &str, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn kill_session(&self, _: &str) -> Result<()> {
            Ok(())
        }
        fn is_alive(&self, _: &str) -> Result<bool> {
            Ok(true)
        }
        fn capture_output(&self, _: &str, _: usize) -> Result<String> {
            Ok(String::new())
        }
        fn send_input(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn setup_logging(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
    }

    async fn test_state() -> Arc<AppState> {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
        let peer_registry = PeerRegistry::new(&HashMap::new());
        AppState::new(
            Config {
                node: NodeConfig {
                    name: "test-node".into(),
                    port: 7433,
                    data_dir: tmpdir.path().to_str().unwrap().into(),
                },
                auth: crate::config::AuthConfig::default(),
                peers: HashMap::new(),
                guards: GuardDefaultConfig::default(),
                watchdog: crate::config::WatchdogConfig::default(),
                personas: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
                discovery: crate::config::DiscoveryConfig::default(),
            },
            manager,
            peer_registry,
        )
    }

    async fn test_state_with_config_path() -> Arc<AppState> {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let config_path = tmpdir.path().join("config.toml");
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        AppState::with_event_tx(
            Config {
                node: NodeConfig {
                    name: "test-node".into(),
                    port: 7433,
                    data_dir: tmpdir.path().to_str().unwrap().into(),
                },
                auth: crate::config::AuthConfig::default(),
                peers: HashMap::new(),
                guards: GuardDefaultConfig::default(),
                watchdog: crate::config::WatchdogConfig::default(),
                personas: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
                discovery: crate::config::DiscoveryConfig::default(),
            },
            config_path,
            manager,
            peer_registry,
            event_tx,
        )
    }

    #[test]
    fn test_stub_backend_methods() {
        use crate::backend::Backend;
        let b = StubBackend;
        assert!(b.create_session("n", "d", "c").is_ok());
        assert!(b.kill_session("n").is_ok());
        assert!(b.is_alive("n").unwrap());
        assert!(b.capture_output("n", 10).unwrap().is_empty());
        assert!(b.send_input("n", "t").is_ok());
        assert!(b.setup_logging("n", "p").is_ok());
    }

    #[tokio::test]
    async fn test_get_config_returns_current() {
        let state = test_state().await;
        let Json(resp) = get_config(State(state)).await.unwrap();
        assert_eq!(resp.node.name, "test-node");
        assert_eq!(resp.node.port, 7433);
        assert_eq!(
            resp.guards.preset,
            pulpo_common::guard::GuardPreset::Standard
        );
        assert!(resp.peers.is_empty());
    }

    #[tokio::test]
    async fn test_update_config_node_name() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            node_name: Some("new-name".into()),
            port: None,
            data_dir: None,
            bind: None,
            guard_preset: None,

            peers: None,
        };
        let Json(resp) = update_config(State(state.clone()), Json(req))
            .await
            .unwrap();
        assert_eq!(resp.config.node.name, "new-name");
        assert!(!resp.restart_required);

        // Verify persisted
        let Json(current) = get_config(State(state)).await.unwrap();
        assert_eq!(current.node.name, "new-name");
    }

    #[tokio::test]
    async fn test_update_config_port_requires_restart() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            node_name: None,
            port: Some(9999),
            data_dir: None,
            bind: None,
            guard_preset: None,

            peers: None,
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.config.node.port, 9999);
        assert!(resp.restart_required);
    }

    #[tokio::test]
    async fn test_update_config_same_port_no_restart() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            node_name: None,
            port: Some(7433),
            data_dir: None,
            bind: None,
            guard_preset: None,

            peers: None,
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert!(!resp.restart_required);
    }

    #[tokio::test]
    async fn test_update_config_guard_preset() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            node_name: None,
            port: None,
            data_dir: None,
            bind: None,
            guard_preset: Some(pulpo_common::guard::GuardPreset::Strict),

            peers: None,
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert_eq!(
            resp.config.guards.preset,
            pulpo_common::guard::GuardPreset::Strict
        );
    }

    #[tokio::test]
    async fn test_update_config_peers() {
        let state = test_state().await;
        let mut peers = HashMap::new();
        peers.insert("remote".into(), PeerEntry::Simple("10.0.0.1:7433".into()));
        let req = UpdateConfigRequest {
            node_name: None,
            port: None,
            data_dir: None,
            bind: None,
            guard_preset: None,

            peers: Some(peers),
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.config.peers.len(), 1);
        assert_eq!(
            resp.config.peers["remote"],
            PeerEntry::Simple("10.0.0.1:7433".into())
        );
    }

    #[tokio::test]
    async fn test_update_config_data_dir() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            node_name: None,
            port: None,
            data_dir: Some("/new/data/dir".into()),
            bind: None,
            guard_preset: None,

            peers: None,
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.config.node.data_dir, "/new/data/dir");
    }

    #[tokio::test]
    async fn test_update_config_multiple_fields() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            node_name: Some("multi".into()),
            port: Some(8888),
            data_dir: None,
            bind: None,
            guard_preset: Some(pulpo_common::guard::GuardPreset::Unrestricted),

            peers: None,
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.config.node.name, "multi");
        assert_eq!(resp.config.node.port, 8888);
        assert_eq!(
            resp.config.guards.preset,
            pulpo_common::guard::GuardPreset::Unrestricted
        );
        assert!(resp.restart_required);
    }

    #[tokio::test]
    async fn test_update_config_saves_to_disk() {
        let state = test_state_with_config_path().await;
        let req = UpdateConfigRequest {
            node_name: Some("saved-node".into()),
            port: None,
            data_dir: None,
            bind: None,
            guard_preset: None,

            peers: None,
        };
        let Json(resp) = update_config(State(state.clone()), Json(req))
            .await
            .unwrap();
        assert_eq!(resp.config.node.name, "saved-node");

        // Verify file was written
        let content = std::fs::read_to_string(&state.config_path).unwrap();
        assert!(content.contains("saved-node"));
    }

    #[tokio::test]
    async fn test_update_config_save_roundtrip() {
        let state = test_state_with_config_path().await;
        let req = UpdateConfigRequest {
            node_name: Some("roundtrip".into()),
            port: Some(9000),
            data_dir: None,
            bind: None,
            guard_preset: Some(pulpo_common::guard::GuardPreset::Strict),

            peers: None,
        };
        let _ = update_config(State(state.clone()), Json(req))
            .await
            .unwrap();

        // Load back from disk
        let loaded = crate::config::load(state.config_path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.node.name, "roundtrip");
        assert_eq!(loaded.node.port, 9000);
        assert_eq!(
            loaded.guards.preset,
            pulpo_common::guard::GuardPreset::Strict
        );
    }

    #[tokio::test]
    async fn test_update_config_empty_request() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            node_name: None,
            port: None,
            data_dir: None,
            bind: None,
            guard_preset: None,

            peers: None,
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        // Nothing changed
        assert_eq!(resp.config.node.name, "test-node");
        assert_eq!(resp.config.node.port, 7433);
        assert!(!resp.restart_required);
    }

    #[test]
    fn test_config_to_response() {
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp".into(),
            },
            auth: crate::config::AuthConfig::default(),
            peers: HashMap::new(),
            guards: GuardDefaultConfig::default(),
            watchdog: crate::config::WatchdogConfig::default(),
            personas: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
            discovery: crate::config::DiscoveryConfig::default(),
        };
        let resp = config_to_response(&config);
        assert_eq!(resp.node.name, "test");
        assert_eq!(resp.node.port, 7433);
        assert_eq!(resp.auth.bind, pulpo_common::auth::BindMode::Local);
    }

    #[tokio::test]
    async fn test_update_config_bind_requires_restart() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            node_name: None,
            port: None,
            data_dir: None,
            bind: Some(pulpo_common::auth::BindMode::Public),
            guard_preset: None,

            peers: None,
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.config.auth.bind, pulpo_common::auth::BindMode::Public);
        assert!(resp.restart_required);
    }

    #[tokio::test]
    async fn test_update_config_same_bind_no_restart() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            node_name: None,
            port: None,
            data_dir: None,
            bind: Some(pulpo_common::auth::BindMode::Local),
            guard_preset: None,

            peers: None,
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert!(!resp.restart_required);
    }

    #[tokio::test]
    async fn test_get_config_returns_auth() {
        let state = test_state().await;
        let Json(resp) = get_config(State(state)).await.unwrap();
        assert_eq!(resp.auth.bind, pulpo_common::auth::BindMode::Local);
    }

    #[test]
    fn test_internal_error() {
        let (status, Json(err)) = internal_error("boom");
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(err.error, "boom");
    }

    #[tokio::test]
    async fn test_config_response_debug() {
        let state = test_state().await;
        let Json(resp) = get_config(State(state)).await.unwrap();
        let debug = format!("{resp:?}");
        assert!(debug.contains("test-node"));
    }

    #[tokio::test]
    async fn test_update_config_save_error() {
        // Use an invalid path that can't be written
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
        let peer_registry = PeerRegistry::new(&HashMap::new());

        // Use /dev/null/impossible as config path (can't create dirs under /dev/null)
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let state = AppState::with_event_tx(
            Config {
                node: NodeConfig {
                    name: "test".into(),
                    port: 7433,
                    data_dir: tmpdir.path().to_str().unwrap().into(),
                },
                auth: crate::config::AuthConfig::default(),
                peers: HashMap::new(),
                guards: GuardDefaultConfig::default(),
                watchdog: crate::config::WatchdogConfig::default(),
                personas: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
                discovery: crate::config::DiscoveryConfig::default(),
            },
            std::path::PathBuf::from("/dev/null/impossible/config.toml"),
            manager,
            peer_registry,
            event_tx,
        );

        let req = UpdateConfigRequest {
            node_name: Some("fail".into()),
            port: None,
            data_dir: None,
            bind: None,
            guard_preset: None,

            peers: None,
        };
        let result = update_config(State(state), Json(req)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }
}
