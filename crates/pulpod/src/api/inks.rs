use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::config::InkConfig;

#[derive(Serialize)]
pub struct InksResponse {
    pub inks: HashMap<String, InkConfig>,
}

pub async fn list(State(state): State<Arc<super::AppState>>) -> Json<InksResponse> {
    let inks = state.session_manager.inks().clone();
    Json(InksResponse { inks })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::AppState;
    use crate::backend::StubBackend;
    use crate::config::{Config, NodeConfig};
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;

    #[tokio::test]
    async fn test_list_inks_empty() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let manager =
            SessionManager::new(Arc::new(StubBackend), store.clone(), HashMap::new(), None)
                .with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(
            Config {
                node: NodeConfig {
                    name: "test".into(),
                    port: 7433,
                    data_dir: tmpdir.path().to_str().unwrap().into(),
                    ..NodeConfig::default()
                },
                auth: crate::config::AuthConfig::default(),
                peers: HashMap::new(),
                watchdog: crate::config::WatchdogConfig::default(),
                inks: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
                sandbox: crate::config::SandboxConfig::default(),
            },
            manager,
            peer_registry,
            store,
        );

        let Json(response) = list(State(state)).await;
        assert!(response.inks.is_empty());
    }

    #[tokio::test]
    async fn test_list_inks_with_entries() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let mut inks = HashMap::new();
        inks.insert(
            "reviewer".into(),
            InkConfig {
                description: None,
                command: Some("Review code".into()),
            },
        );
        let manager = SessionManager::new(Arc::new(StubBackend), store.clone(), inks.clone(), None)
            .with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(
            Config {
                node: NodeConfig {
                    name: "test".into(),
                    port: 7433,
                    data_dir: tmpdir.path().to_str().unwrap().into(),
                    ..NodeConfig::default()
                },
                auth: crate::config::AuthConfig::default(),
                peers: HashMap::new(),
                watchdog: crate::config::WatchdogConfig::default(),
                inks: inks.clone(),
                notifications: crate::config::NotificationsConfig::default(),
                sandbox: crate::config::SandboxConfig::default(),
            },
            manager,
            peer_registry,
            store,
        );

        let Json(response) = list(State(state)).await;
        assert_eq!(response.inks.len(), 1);
        let reviewer = &response.inks["reviewer"];
        assert_eq!(reviewer.command, Some("Review code".into()));
    }

    #[test]
    fn test_inks_response_serialize() {
        let mut inks = HashMap::new();
        inks.insert(
            "coder".into(),
            InkConfig {
                description: None,
                command: None,
            },
        );
        let resp = InksResponse { inks };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("coder"));
    }
}
