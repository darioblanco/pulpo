use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::config::PersonaConfig;

#[derive(Serialize)]
pub struct PersonasResponse {
    pub personas: HashMap<String, PersonaConfig>,
}

pub async fn list(State(state): State<Arc<super::AppState>>) -> Json<PersonasResponse> {
    let personas = state.session_manager.personas().clone();
    Json(PersonasResponse { personas })
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
    async fn test_list_personas_empty() {
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
                },
                auth: crate::config::AuthConfig::default(),
                peers: HashMap::new(),
                guards: crate::config::GuardDefaultConfig::default(),
                watchdog: crate::config::WatchdogConfig::default(),
                personas: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
                discovery: crate::config::DiscoveryConfig::default(),
            },
            manager,
            peer_registry,
        );

        let Json(response) = list(State(state)).await;
        assert!(response.personas.is_empty());
    }

    #[tokio::test]
    async fn test_list_personas_with_entries() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let mut personas = HashMap::new();
        personas.insert(
            "reviewer".into(),
            PersonaConfig {
                provider: Some("claude".into()),
                model: Some("sonnet".into()),
                mode: Some("autonomous".into()),
                guard_preset: Some("strict".into()),
                allowed_tools: Some(vec!["Read".into(), "Glob".into()]),
                system_prompt: Some("Review code".into()),
                max_turns: None,
                max_budget_usd: None,
                output_format: None,
            },
        );
        let manager = SessionManager::new(
            Arc::new(StubBackend),
            store,
            pulpo_common::guard::GuardConfig::default(),
            personas.clone(),
        );
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(
            Config {
                node: NodeConfig {
                    name: "test".into(),
                    port: 7433,
                    data_dir: tmpdir.path().to_str().unwrap().into(),
                },
                auth: crate::config::AuthConfig::default(),
                peers: HashMap::new(),
                guards: crate::config::GuardDefaultConfig::default(),
                watchdog: crate::config::WatchdogConfig::default(),
                personas: personas.clone(),
                notifications: crate::config::NotificationsConfig::default(),
                discovery: crate::config::DiscoveryConfig::default(),
            },
            manager,
            peer_registry,
        );

        let Json(response) = list(State(state)).await;
        assert_eq!(response.personas.len(), 1);
        let reviewer = &response.personas["reviewer"];
        assert_eq!(reviewer.model, Some("sonnet".into()));
        assert_eq!(reviewer.system_prompt, Some("Review code".into()));
    }

    #[test]
    fn test_personas_response_serialize() {
        let mut personas = HashMap::new();
        personas.insert(
            "coder".into(),
            PersonaConfig {
                provider: None,
                model: Some("opus".into()),
                mode: None,
                guard_preset: None,
                allowed_tools: None,
                system_prompt: None,
                max_turns: None,
                max_budget_usd: None,
                output_format: None,
            },
        );
        let resp = PersonasResponse { personas };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("coder"));
        assert!(json.contains("opus"));
    }
}
