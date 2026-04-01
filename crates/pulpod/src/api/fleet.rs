use std::sync::Arc;

use axum::{Json, extract::State};
use chrono::{DateTime, Utc};
use pulpo_common::api::{ErrorResponse, FleetSession, FleetSessionsResponse};
use pulpo_common::session::{Session, SessionStatus};
use uuid::Uuid;

use super::AppState;
use crate::config::NodeRole;

type ApiError = (axum::http::StatusCode, Json<ErrorResponse>);

/// Aggregate sessions for the current control-plane role.
///
/// Only the master node exposes canonical fleet-wide state. Worker and
/// standalone nodes return local sessions only.
pub async fn fleet_sessions(
    State(state): State<Arc<AppState>>,
) -> Result<Json<FleetSessionsResponse>, ApiError> {
    let mut all_sessions: Vec<FleetSession> = Vec::new();

    // Local sessions (always included)
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

    let role = state.config.read().await.role();
    if role == NodeRole::Master {
        let Some(session_index) = &state.session_index else {
            return Ok(Json(FleetSessionsResponse {
                sessions: all_sessions,
            }));
        };
        let entries = session_index.list_all().await;
        for entry in entries {
            let status = entry
                .status
                .parse::<SessionStatus>()
                .unwrap_or(SessionStatus::Lost);
            let session_id = entry
                .session_id
                .parse::<Uuid>()
                .unwrap_or_else(|_| Uuid::nil());
            let updated_at = entry
                .updated_at
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now());
            let session = Session {
                id: session_id,
                name: entry.session_name,
                command: entry.command.unwrap_or_default(),
                status,
                updated_at,
                ..Session::default()
            };
            all_sessions.push(FleetSession {
                node_name: entry.node_name,
                node_address: entry.node_address.unwrap_or_default(),
                session,
            });
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
    use crate::backend::StubBackend;
    use crate::config::{Config, NodeConfig};
    use crate::master::{CommandQueue, SessionIndex};
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;
    use pulpo_common::api::SessionIndexEntry;

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
            docker: crate::config::DockerConfig::default(),
            master: crate::config::MasterConfig {
                enabled: true,
                ..crate::config::MasterConfig::default()
            },
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
            docker: crate::config::DockerConfig::default(),
            master: crate::config::MasterConfig {
                enabled: true,
                ..crate::config::MasterConfig::default()
            },
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
            docker: crate::config::DockerConfig::default(),
            master: crate::config::MasterConfig {
                enabled: true,
                ..crate::config::MasterConfig::default()
            },
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

    #[tokio::test]
    async fn test_fleet_sessions_master_mode_returns_index_data() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "master-node".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig::default(),
            peers: HashMap::new(),
            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
            docker: crate::config::DockerConfig::default(),
            master: crate::config::MasterConfig {
                enabled: true,
                ..crate::config::MasterConfig::default()
            },
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let session_index = Arc::new(SessionIndex::new());
        let command_queue = Arc::new(CommandQueue::new());

        // Pre-populate the session index with worker data
        session_index
            .upsert(SessionIndexEntry {
                session_id: "s1".into(),
                node_name: "worker-1".into(),
                node_address: Some("10.0.0.1:7433".into()),
                session_name: "remote-task".into(),
                status: "active".into(),
                command: Some("claude -p build".into()),
                updated_at: "2026-03-30T12:00:00Z".into(),
            })
            .await;

        let state = AppState::with_event_tx_master(
            config,
            tmpdir.path().join("config.toml"),
            manager,
            peer_registry,
            event_tx,
            store,
            Some(session_index),
            Some(command_queue),
        );
        let app = routes::build(state);
        let server = TestServer::new(app).unwrap();

        let resp = server.get("/api/v1/fleet/sessions").await;
        resp.assert_status_ok();
        let body: FleetSessionsResponse = resp.json();
        // Should have the worker session from the index
        assert_eq!(body.sessions.len(), 1);
        assert_eq!(body.sessions[0].node_name, "worker-1");
        assert_eq!(body.sessions[0].node_address, "10.0.0.1:7433");
        assert_eq!(body.sessions[0].session.name, "remote-task");
        assert_eq!(
            body.sessions[0].session.status,
            pulpo_common::session::SessionStatus::Active
        );
    }
}
