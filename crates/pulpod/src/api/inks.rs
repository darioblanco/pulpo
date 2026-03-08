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
    use crate::backend::Backend;
    use crate::config::{Config, NodeConfig};
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;
    use anyhow::Result;

    struct StubBackend;

    impl Backend for StubBackend {
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

    #[test]
    fn test_stub_backend_methods() {
        let b = StubBackend;
        assert!(b.create_session("n", "d", "c").is_ok());
        assert!(b.kill_session("n").is_ok());
        assert!(b.is_alive("n").unwrap());
        assert!(b.capture_output("n", 10).unwrap().is_empty());
        assert!(b.send_input("n", "t").is_ok());
        assert!(b.setup_logging("n", "p").is_ok());
    }

    #[tokio::test]
    async fn test_list_inks_empty() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let manager = SessionManager::new(
            Arc::new(StubBackend),
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
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
                guards: crate::config::GuardDefaultConfig::default(),
                watchdog: crate::config::WatchdogConfig::default(),
                inks: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
            },
            manager,
            peer_registry,
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
                provider: Some("claude".into()),
                model: Some("sonnet".into()),
                mode: Some("autonomous".into()),
                guard_preset: Some("strict".into()),
                instructions: Some("Review code".into()),
            },
        );
        let manager = SessionManager::new(
            Arc::new(StubBackend),
            store,
            pulpo_common::guard::GuardConfig::default(),
            inks.clone(),
        );
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
                guards: crate::config::GuardDefaultConfig::default(),
                watchdog: crate::config::WatchdogConfig::default(),
                inks: inks.clone(),
                notifications: crate::config::NotificationsConfig::default(),
            },
            manager,
            peer_registry,
        );

        let Json(response) = list(State(state)).await;
        assert_eq!(response.inks.len(), 1);
        let reviewer = &response.inks["reviewer"];
        assert_eq!(reviewer.model, Some("sonnet".into()));
        assert_eq!(reviewer.instructions, Some("Review code".into()));
    }

    #[test]
    fn test_inks_response_serialize() {
        let mut inks = HashMap::new();
        inks.insert(
            "coder".into(),
            InkConfig {
                description: None,
                provider: None,
                model: Some("opus".into()),
                mode: None,
                guard_preset: None,
                instructions: None,
            },
        );
        let resp = InksResponse { inks };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("coder"));
        assert!(json.contains("opus"));
    }
}
