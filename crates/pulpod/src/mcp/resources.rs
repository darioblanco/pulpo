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
            RawResource {
                uri: "pulpo://cluster/status".into(),
                name: "cluster_status".into(),
                description: Some(
                    "Curated, agent-readable cluster status: session counts, idle/waiting sessions, node info, and peers"
                        .into(),
                ),
                mime_type: Some("application/json".into()),
                ..RawResource::new("pulpo://cluster/status", "cluster_status")
            }
            .no_annotation(),
        ],
        ..Default::default()
    }
}

async fn read_cluster_status(mcp: &PulpoMcp, uri: &str) -> Result<ReadResourceResult, ErrorData> {
    let sessions = mcp
        .session_manager
        .list_sessions()
        .await
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    let total = sessions.len();
    let mut by_status = std::collections::HashMap::new();
    let mut idle = Vec::new();
    let mut waiting_for_input = Vec::new();

    for s in &sessions {
        *by_status.entry(s.status.to_string()).or_insert(0usize) += 1;

        if let Some(idle_since) = s.idle_since {
            let idle_minutes =
                u64::try_from((chrono::Utc::now() - idle_since).num_minutes().max(0)).unwrap_or(0);
            idle.push(serde_json::json!({
                "id": s.id.to_string(),
                "name": s.name,
                "idle_minutes": idle_minutes,
            }));
        }

        if s.waiting_for_input {
            waiting_for_input.push(serde_json::json!({
                "id": s.id.to_string(),
                "name": s.name,
            }));
        }
    }

    let local = mcp.build_node_info();
    let peers = mcp.peer_registry.get_all().await;
    let available_count = 1 + peers
        .iter()
        .filter(|p| p.status == pulpo_common::peer::PeerStatus::Online)
        .count();

    let running_count = by_status.get("running").copied().unwrap_or(0);
    let completed_count = by_status.get("completed").copied().unwrap_or(0);
    let summary = format!(
        "{total} sessions ({running_count} running, {completed_count} completed). {available_count} nodes online."
    );

    let peer_list: Vec<_> = peers
        .iter()
        .map(|p| {
            serde_json::json!({
                "name": p.name,
                "status": format!("{:?}", p.status).to_lowercase(),
                "session_count": p.session_count,
            })
        })
        .collect();

    let result = serde_json::json!({
        "summary": summary,
        "sessions": {
            "total": total,
            "by_status": by_status,
            "idle": idle,
            "waiting_for_input": waiting_for_input,
        },
        "nodes": {
            "local": {
                "name": local.name,
                "cpus": local.cpus,
                "memory_mb": local.memory_mb,
            },
            "peers": peer_list,
            "available_count": available_count,
        },
    });
    let json = serde_json::to_string_pretty(&result).unwrap_or_default();
    Ok(ReadResourceResult {
        contents: vec![ResourceContents::text(json, uri)],
    })
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
        "pulpo://cluster/status" => read_cluster_status(mcp, uri).await,
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
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            guards: GuardDefaultConfig::default(),
            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
            knowledge: crate::config::KnowledgeConfig::default(),
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
        assert_eq!(result.resources.len(), 3);
        let uris: Vec<&str> = result.resources.iter().map(|r| r.uri.as_str()).collect();
        assert!(uris.contains(&"pulpo://sessions"));
        assert!(uris.contains(&"pulpo://nodes"));
        assert!(uris.contains(&"pulpo://cluster/status"));
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
            workdir: Some("/tmp".into()),
            prompt: Some("test".into()),
            provider: None,
            mode: None,
            unrestricted: None,
            name: None,
            ink: None,
            model: None,
            worktree: None,
            node: None,
        };
        mcp.spawn_session(Parameters(params)).await;

        let result = read_resource(&mcp, "pulpo://sessions").await.unwrap();
        let json = serde_json::to_string(&result.contents[0]).unwrap();
        assert!(json.contains("tmp"));
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
            workdir: Some("/tmp".into()),
            prompt: Some("test".into()),
            provider: None,
            mode: None,
            unrestricted: None,
            name: None,
            ink: None,
            model: None,
            worktree: None,
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

    // -- cluster/status resource tests --

    #[tokio::test]
    async fn test_read_resource_cluster_status_empty() {
        let mcp = test_mcp().await;
        let result = read_resource(&mcp, "pulpo://cluster/status").await.unwrap();
        assert_eq!(result.contents.len(), 1);
        let json = serde_json::to_string(&result.contents[0]).unwrap();
        assert!(json.contains("0 sessions"));
        assert!(json.contains("total"));
        assert!(json.contains("test-node"));
    }

    #[tokio::test]
    async fn test_read_resource_cluster_status_with_sessions() {
        let mcp = test_mcp().await;
        // Create a session
        let params = SpawnSessionParams {
            workdir: Some("/tmp".into()),
            prompt: Some("test".into()),
            provider: None,
            mode: None,
            unrestricted: None,
            name: None,
            ink: None,
            model: None,
            worktree: None,
            node: None,
        };
        mcp.spawn_session(Parameters(params)).await;

        let result = read_resource(&mcp, "pulpo://cluster/status").await.unwrap();
        let json = serde_json::to_string(&result.contents[0]).unwrap();
        assert!(json.contains("1 sessions"));
        assert!(json.contains("running"));
    }

    #[tokio::test]
    async fn test_read_resource_cluster_status_with_waiting_session() {
        let (mcp, pool) = test_mcp_with_pool().await;
        // Create a session
        let params = SpawnSessionParams {
            workdir: Some("/tmp".into()),
            prompt: Some("test".into()),
            provider: None,
            mode: None,
            unrestricted: None,
            name: None,
            ink: None,
            model: None,
            worktree: None,
            node: None,
        };
        let spawn_result = mcp.spawn_session(Parameters(params)).await;
        let session: pulpo_common::session::Session = serde_json::from_str(&spawn_result).unwrap();

        // Mark waiting_for_input
        sqlx::query("UPDATE sessions SET waiting_for_input = 1 WHERE id = ?")
            .bind(session.id.to_string())
            .execute(&pool)
            .await
            .unwrap();

        let result = read_resource(&mcp, "pulpo://cluster/status").await.unwrap();
        let json = serde_json::to_string(&result.contents[0]).unwrap();
        assert!(json.contains("waiting_for_input"));
        assert!(json.contains(&session.id.to_string()));
    }

    #[tokio::test]
    async fn test_read_resource_cluster_status_with_idle_session() {
        let (mcp, pool) = test_mcp_with_pool().await;
        // Create a session
        let params = SpawnSessionParams {
            workdir: Some("/tmp".into()),
            prompt: Some("test".into()),
            provider: None,
            mode: None,
            unrestricted: None,
            name: None,
            ink: None,
            model: None,
            worktree: None,
            node: None,
        };
        let spawn_result = mcp.spawn_session(Parameters(params)).await;
        let session: pulpo_common::session::Session = serde_json::from_str(&spawn_result).unwrap();

        // Set idle_since to 15 minutes ago
        let idle_since = (chrono::Utc::now() - chrono::Duration::minutes(15)).to_rfc3339();
        sqlx::query("UPDATE sessions SET idle_since = ? WHERE id = ?")
            .bind(&idle_since)
            .bind(session.id.to_string())
            .execute(&pool)
            .await
            .unwrap();

        let result = read_resource(&mcp, "pulpo://cluster/status").await.unwrap();
        let json = serde_json::to_string(&result.contents[0]).unwrap();
        assert!(json.contains("idle"));
        assert!(json.contains("idle_minutes"));
    }

    #[tokio::test]
    async fn test_read_resource_cluster_status_store_error() {
        let (mcp, pool) = test_mcp_with_pool().await;
        // Drop the sessions table to trigger a store error
        sqlx::query("DROP TABLE sessions")
            .execute(&pool)
            .await
            .unwrap();
        let result = read_resource(&mcp, "pulpo://cluster/status").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_resource_cluster_status_nodes_info() {
        let mcp = test_mcp().await;
        let result = read_resource(&mcp, "pulpo://cluster/status").await.unwrap();
        let json = serde_json::to_string(&result.contents[0]).unwrap();
        // Should have nodes section with local info
        assert!(json.contains("available_count"));
        assert!(json.contains("test-node"));
    }

    #[tokio::test]
    async fn test_read_resource_cluster_status_with_peers() {
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
        let mut peers = HashMap::new();
        peers.insert(
            "remote".into(),
            pulpo_common::peer::PeerEntry::Simple("10.0.0.1:7433".into()),
        );
        let peer_registry = PeerRegistry::new(&peers);
        let mcp = PulpoMcp::new(manager, peer_registry, test_config());

        let result = read_resource(&mcp, "pulpo://cluster/status").await.unwrap();
        let json = serde_json::to_string(&result.contents[0]).unwrap();
        assert!(json.contains("remote"));
        assert!(json.contains("session_count"));
    }
}
