use std::sync::Arc;

use axum::{Json, extract::State};
use pulpo_common::api::{ErrorResponse, FleetSession, FleetSessionsResponse};
use pulpo_common::peer::PeerStatus;
use pulpo_common::session::Session;

use super::AppState;

type ApiError = (axum::http::StatusCode, Json<ErrorResponse>);

/// Aggregate sessions from the local node and all online peers.
/// Queries peers in parallel with a 2-second timeout per peer.
pub async fn fleet_sessions(
    State(state): State<Arc<AppState>>,
) -> Result<Json<FleetSessionsResponse>, ApiError> {
    let mut all_sessions: Vec<FleetSession> = Vec::new();

    // Local sessions
    let local_name = state.config.read().await.node.name.clone();
    let local_sessions = state
        .session_manager
        .list_sessions()
        .await
        .unwrap_or_default();
    for session in local_sessions {
        all_sessions.push(FleetSession {
            node_name: local_name.clone(),
            node_address: String::new(),
            session,
        });
    }

    // Remote peer sessions (parallel, with timeout)
    #[cfg(not(coverage))]
    {
        let peers = state.peer_registry.get_all().await;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap_or_default();

        let mut handles = Vec::new();
        for peer in peers {
            if peer.status != PeerStatus::Online {
                continue;
            }
            let peer_name = peer.name.clone();
            let peer_address = peer.address.clone();
            let client = client.clone();
            let token = state.peer_registry.get_token(&peer_name).await;

            handles.push(tokio::spawn(async move {
                let url = format!("http://{peer_address}/api/v1/sessions");
                let mut req = client.get(&url);
                if let Some(tok) = &token {
                    req = req.bearer_auth(tok);
                }
                match req.send().await {
                    Ok(resp) if resp.status().is_success() => {
                        let sessions: Vec<Session> = resp.json().await.unwrap_or_default();
                        sessions
                            .into_iter()
                            .map(|s| FleetSession {
                                node_name: peer_name.clone(),
                                node_address: peer_address.clone(),
                                session: s,
                            })
                            .collect::<Vec<_>>()
                    }
                    _ => Vec::new(),
                }
            }));
        }

        for handle in handles {
            if let Ok(sessions) = handle.await {
                all_sessions.extend(sessions);
            }
        }
    }

    Ok(Json(FleetSessionsResponse {
        sessions: all_sessions,
    }))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use axum_test::TestServer;
    use pulpo_common::api::FleetSessionsResponse;

    use crate::api::AppState;
    use crate::api::routes;
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

    async fn test_server() -> TestServer {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "fleet-test-node".into(),
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
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(config, manager, peer_registry, store);
        let app = routes::build(state);
        TestServer::new(app).unwrap()
    }

    #[tokio::test]
    async fn test_fleet_sessions_empty() {
        let server = test_server().await;
        let resp = server.get("/api/v1/fleet/sessions").await;
        resp.assert_status_ok();
        let body: FleetSessionsResponse = resp.json();
        assert!(body.sessions.is_empty());
    }

    #[tokio::test]
    async fn test_fleet_sessions_with_local_session() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "my-node".into(),
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
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(config, manager, peer_registry, store);

        // Create a session via the API
        let app = routes::build(state);
        let server = TestServer::new(app).unwrap();

        let create_body = serde_json::json!({
            "name": "fleet-test",
            "workdir": "/tmp",
            "command": "echo hello"
        });
        let create_resp = server.post("/api/v1/sessions").json(&create_body).await;
        create_resp.assert_status(axum::http::StatusCode::CREATED);

        // Now query fleet sessions
        let resp = server.get("/api/v1/fleet/sessions").await;
        resp.assert_status_ok();
        let body: FleetSessionsResponse = resp.json();
        assert_eq!(body.sessions.len(), 1);
        assert_eq!(body.sessions[0].node_name, "my-node");
        assert!(body.sessions[0].node_address.is_empty());
        assert_eq!(body.sessions[0].session.name, "fleet-test");
    }

    #[tokio::test]
    async fn test_fleet_sessions_skips_offline_peers() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let mut configured_peers = HashMap::new();
        configured_peers.insert(
            "offline-peer".into(),
            pulpo_common::peer::PeerEntry::Simple("10.0.0.99:7433".into()),
        );
        let config = Config {
            node: NodeConfig {
                name: "local-node".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig::default(),
            peers: configured_peers.clone(),
            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
            sandbox: crate::config::SandboxConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&configured_peers);
        // Peer status is Unknown by default — should be skipped (not Online)
        let state = AppState::new(config, manager, peer_registry, store);
        let app = routes::build(state);
        let server = TestServer::new(app).unwrap();

        let resp = server.get("/api/v1/fleet/sessions").await;
        resp.assert_status_ok();
        let body: FleetSessionsResponse = resp.json();
        // Only local sessions (none), offline peer is skipped
        assert!(body.sessions.is_empty());
    }
}
