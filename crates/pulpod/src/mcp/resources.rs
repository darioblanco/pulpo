use rmcp::model::{
    AnnotateAble, ErrorData, ListResourcesResult, RawResource, ReadResourceResult, ResourceContents,
};

use super::PulpoMcp;

pub fn list_resources() -> ListResourcesResult {
    ListResourcesResult {
        resources: vec![
            RawResource {
                uri: "pulpo://sessions".into(),
                name: "sessions".into(),
                description: Some("JSON list of all sessions".into()),
                mime_type: Some("application/json".into()),
                ..RawResource::new("pulpo://sessions", "sessions")
            }
            .no_annotation(),
            RawResource {
                uri: "pulpo://nodes".into(),
                name: "nodes".into(),
                description: Some("JSON with local node info and peers".into()),
                mime_type: Some("application/json".into()),
                ..RawResource::new("pulpo://nodes", "nodes")
            }
            .no_annotation(),
        ],
        ..Default::default()
    }
}

pub async fn read_resource(mcp: &PulpoMcp, uri: &str) -> Result<ReadResourceResult, ErrorData> {
    match uri {
        "pulpo://sessions" => {
            let sessions = mcp
                .session_manager
                .list_sessions()
                .await
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
            let json = serde_json::to_string_pretty(&sessions).unwrap_or_default();
            Ok(ReadResourceResult {
                contents: vec![ResourceContents::text(json, uri)],
            })
        }
        "pulpo://nodes" => {
            let local = mcp.build_node_info();
            let peers = mcp.peer_registry.get_all().await;
            let result = serde_json::json!({
                "local": local,
                "peers": peers,
            });
            let json = serde_json::to_string_pretty(&result).unwrap_or_default();
            Ok(ReadResourceResult {
                contents: vec![ResourceContents::text(json, uri)],
            })
        }
        _ if uri.starts_with("pulpo://sessions/") && uri.ends_with("/output") => {
            let id = uri
                .strip_prefix("pulpo://sessions/")
                .and_then(|s| s.strip_suffix("/output"))
                .unwrap_or_default();
            let session = mcp
                .session_manager
                .get_session(id)
                .await
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?
                .ok_or_else(|| {
                    ErrorData::resource_not_found(format!("session not found: {id}"), None)
                })?;
            let output = mcp.session_manager.capture_output(id, &session.name, 200);
            Ok(ReadResourceResult {
                contents: vec![ResourceContents::text(output, uri)],
            })
        }
        _ => Err(ErrorData::resource_not_found(
            format!("unknown resource: {uri}"),
            None,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Backend;
    use crate::config::{AuthConfig, Config, GuardDefaultConfig, NodeConfig};
    use crate::mcp::SpawnSessionParams;
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;
    use anyhow::Result;
    use rmcp::handler::server::wrapper::Parameters;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    struct MockBackend {
        alive: Mutex<bool>,
        captured_output: Mutex<String>,
    }

    impl MockBackend {
        fn new() -> Self {
            Self {
                alive: Mutex::new(true),
                captured_output: Mutex::new("output line 1\noutput line 2".into()),
            }
        }
    }

    impl Backend for MockBackend {
        fn create_session(&self, _: &str, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn kill_session(&self, _: &str) -> Result<()> {
            Ok(())
        }
        fn is_alive(&self, _: &str) -> Result<bool> {
            Ok(*self.alive.lock().unwrap())
        }
        fn capture_output(&self, _: &str, _: usize) -> Result<String> {
            Ok(self.captured_output.lock().unwrap().clone())
        }
        fn send_input(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn setup_logging(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
    }

    fn test_config() -> Config {
        Config {
            node: NodeConfig {
                name: "test-node".into(),
                port: 7433,
                data_dir: "/tmp/test".into(),
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            guards: GuardDefaultConfig::default(),
            watchdog: crate::config::WatchdogConfig::default(),
            personas: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
        }
    }

    async fn test_mcp() -> PulpoMcp {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(MockBackend::new());
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
        let peer_registry = PeerRegistry::new(&HashMap::new());
        PulpoMcp::new(manager, peer_registry, test_config())
    }

    async fn test_mcp_with_pool() -> (PulpoMcp, sqlx::SqlitePool) {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let pool = store.pool().clone();
        let backend = Arc::new(MockBackend::new());
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
        let peer_registry = PeerRegistry::new(&HashMap::new());
        (PulpoMcp::new(manager, peer_registry, test_config()), pool)
    }

    #[test]
    fn test_list_resources_returns_expected() {
        let result = list_resources();
        assert_eq!(result.resources.len(), 2);
        let uris: Vec<&str> = result.resources.iter().map(|r| r.uri.as_str()).collect();
        assert!(uris.contains(&"pulpo://sessions"));
        assert!(uris.contains(&"pulpo://nodes"));
    }

    #[tokio::test]
    async fn test_read_resource_sessions() {
        let mcp = test_mcp().await;
        let result = read_resource(&mcp, "pulpo://sessions").await.unwrap();
        assert_eq!(result.contents.len(), 1);
        // Serialize the content to verify it's valid text
        let json = serde_json::to_string(&result.contents[0]).unwrap();
        assert!(json.contains("text"));
    }

    #[tokio::test]
    async fn test_read_resource_sessions_with_data() {
        let mcp = test_mcp().await;
        // Create a session
        let params = SpawnSessionParams {
            workdir: "/tmp/repo".into(),
            prompt: "test".into(),
            provider: None,
            mode: None,
            guard_preset: None,
            name: None,
            node: None,
        };
        mcp.spawn_session(Parameters(params)).await;

        let result = read_resource(&mcp, "pulpo://sessions").await.unwrap();
        let json = serde_json::to_string(&result.contents[0]).unwrap();
        assert!(json.contains("repo"));
    }

    #[tokio::test]
    async fn test_read_resource_nodes() {
        let mcp = test_mcp().await;
        let result = read_resource(&mcp, "pulpo://nodes").await.unwrap();
        assert_eq!(result.contents.len(), 1);
        let json = serde_json::to_string(&result.contents[0]).unwrap();
        assert!(json.contains("test-node"));
        assert!(json.contains("peers"));
    }

    #[tokio::test]
    async fn test_read_resource_session_output() {
        let mcp = test_mcp().await;
        // Create a session first
        let params = SpawnSessionParams {
            workdir: "/tmp/repo".into(),
            prompt: "test".into(),
            provider: None,
            mode: None,
            guard_preset: None,
            name: None,
            node: None,
        };
        let spawn_result = mcp.spawn_session(Parameters(params)).await;
        let session: pulpo_common::session::Session = serde_json::from_str(&spawn_result).unwrap();

        let uri = format!("pulpo://sessions/{}/output", session.id);
        let result = read_resource(&mcp, &uri).await.unwrap();
        let json = serde_json::to_string(&result.contents[0]).unwrap();
        assert!(json.contains("output line"));
    }

    #[test]
    fn test_mock_backend_kill_session() {
        let backend = MockBackend::new();
        assert!(backend.kill_session("test-session").is_ok());
    }

    #[tokio::test]
    async fn test_read_resource_session_output_not_found() {
        let mcp = test_mcp().await;
        let result = read_resource(&mcp, "pulpo://sessions/nonexistent/output").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_resource_unknown() {
        let mcp = test_mcp().await;
        let result = read_resource(&mcp, "pulpo://unknown").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_resource_sessions_store_error() {
        let (mcp, pool) = test_mcp_with_pool().await;
        // Drop the sessions table to trigger a store error in list_sessions
        sqlx::query("DROP TABLE sessions")
            .execute(&pool)
            .await
            .unwrap();
        let result = read_resource(&mcp, "pulpo://sessions").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_resource_session_output_store_error() {
        let (mcp, pool) = test_mcp_with_pool().await;
        // Drop the sessions table to trigger a store error in get_session
        sqlx::query("DROP TABLE sessions")
            .execute(&pool)
            .await
            .unwrap();
        let result = read_resource(&mcp, "pulpo://sessions/some-id/output").await;
        assert!(result.is_err());
    }

    #[test]
    fn test_mock_backend_methods() {
        let b = MockBackend::new();
        assert!(b.create_session("n", "d", "c").is_ok());
        assert!(b.kill_session("n").is_ok());
        assert!(b.is_alive("n").unwrap());
        assert!(b.capture_output("n", 10).unwrap().contains("output line"));
        assert!(b.send_input("n", "t").is_ok());
        assert!(b.setup_logging("n", "p").is_ok());
    }
}
