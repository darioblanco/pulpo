use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use pulpo_common::api::{ErrorResponse, EventPushRequest, SessionIndexEntry};
use pulpo_common::event::PulpoEvent;

use super::AppState;
use super::node_auth::authenticate_node;

/// Nodes push batched events to the controller via this endpoint.
pub async fn push_events(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<EventPushRequest>,
) -> impl IntoResponse {
    let Some(session_index) = &state.session_index else {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "This node is not in controller mode".into(),
            }),
        )
            .into_response();
    };

    let node = match authenticate_node(&state, &headers).await {
        Ok(node) => node,
        Err(err) => return err.into_response(),
    };
    let node_name = node.node_name;

    for event in &req.events {
        match event {
            PulpoEvent::Session(se) => {
                let node_address = state
                    .peer_registry
                    .get(&node_name)
                    .await
                    .map(|peer| peer.address);
                let entry = SessionIndexEntry {
                    session_id: se.session_id.clone(),
                    node_name: node_name.clone(),
                    node_address,
                    session_name: se.session_name.clone(),
                    status: se.status.clone(),
                    command: None,
                    updated_at: se.timestamp.clone(),
                };
                if let Err(e) = state
                    .store
                    .upsert_controller_session_index_entry(&entry)
                    .await
                {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            error: format!("failed to persist controller session index entry: {e}"),
                        }),
                    )
                        .into_response();
                }
                session_index.upsert(entry).await;
            }
            PulpoEvent::SessionDeleted(se) => {
                if let Err(e) = state
                    .store
                    .delete_controller_session_index_entry(&se.session_id)
                    .await
                {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            error: format!("failed to delete controller session index entry: {e}"),
                        }),
                    )
                        .into_response();
                }
                session_index.remove(&se.session_id).await;
            }
        }
        let _ = state.event_tx.send(event.clone());
    }

    if let Err(e) = state
        .store
        .touch_controller_node(&node_name, &chrono::Utc::now().to_rfc3339())
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("failed to persist controller node heartbeat: {e}"),
            }),
        )
            .into_response();
    }
    if let Err(e) = state
        .store
        .touch_enrolled_controller_node(&node_name, &chrono::Utc::now().to_rfc3339(), None)
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("failed to persist enrolled node heartbeat: {e}"),
            }),
        )
            .into_response();
    }
    session_index.touch_node(&node_name).await;

    StatusCode::NO_CONTENT.into_response()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use axum_test::TestServer;
    use pulpo_common::api::{
        EnrollNodeRequest, EnrollNodeResponse, EventPushRequest, FleetSessionsResponse,
        SessionIndexEntry,
    };
    use pulpo_common::event::{PulpoEvent, SessionDeletedEvent, SessionEvent};

    use crate::api::AppState;
    use crate::api::routes;
    use crate::backend::StubBackend;
    use crate::config::{Config, NodeConfig};
    use crate::controller::{CommandQueue, SessionIndex};
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;

    async fn controller_test_server() -> (TestServer, Arc<AppState>) {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "controller-node".into(),
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
            controller: crate::config::ControllerConfig {
                enabled: true,
                ..crate::config::ControllerConfig::default()
            },
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let session_index = Arc::new(SessionIndex::new());
        let command_queue = Arc::new(CommandQueue::new());
        let state = AppState::with_event_tx_controller(
            config,
            tmpdir.path().join("config.toml"),
            manager,
            peer_registry,
            event_tx,
            store,
            Some(session_index),
            Some(command_queue),
        );
        let app = routes::build(state.clone());
        (TestServer::new(app).unwrap(), state)
    }

    async fn standalone_test_server() -> TestServer {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "standalone-node".into(),
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
            controller: crate::config::ControllerConfig {
                enabled: true,
                ..crate::config::ControllerConfig::default()
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

    fn make_session_event(session_id: &str, name: &str, status: &str) -> PulpoEvent {
        PulpoEvent::Session(SessionEvent {
            session_id: session_id.into(),
            session_name: name.into(),
            status: status.into(),
            previous_status: None,
            node_name: "node-1".into(),
            output_snippet: None,
            timestamp: "2026-03-30T12:00:00Z".into(),
            ..Default::default()
        })
    }

    fn make_deleted_event(session_id: &str, name: &str) -> PulpoEvent {
        PulpoEvent::SessionDeleted(SessionDeletedEvent {
            session_id: session_id.into(),
            session_name: name.into(),
            node_name: "node-1".into(),
            timestamp: "2026-03-30T12:00:00Z".into(),
        })
    }

    #[allow(clippy::future_not_send)]
    async fn enroll_node(server: &TestServer, node_name: &str) -> String {
        let resp = server
            .post("/api/v1/controller/nodes")
            .json(&EnrollNodeRequest {
                node_name: node_name.into(),
            })
            .await;
        resp.assert_status(axum::http::StatusCode::CREATED);
        let body: EnrollNodeResponse = resp.json();
        body.token
    }

    #[tokio::test]
    async fn test_push_events_updates_index() {
        let (server, _) = controller_test_server().await;
        let token = enroll_node(&server, "node-1").await;

        let req = EventPushRequest {
            events: vec![
                make_session_event("s1", "task-a", "active"),
                make_session_event("s2", "task-b", "idle"),
            ],
        };
        let resp = server
            .post("/api/v1/events/push")
            .add_header("authorization", format!("Bearer {token}"))
            .json(&req)
            .await;
        resp.assert_status(axum::http::StatusCode::NO_CONTENT);

        // Verify fleet endpoint returns the indexed sessions
        let fleet_resp = server.get("/api/v1/fleet/sessions").await;
        fleet_resp.assert_status_ok();
        let body: FleetSessionsResponse = fleet_resp.json();
        assert_eq!(body.sessions.len(), 2);

        let mut names: Vec<String> = body
            .sessions
            .iter()
            .map(|s| s.session.name.clone())
            .collect();
        names.sort();
        assert_eq!(names, vec!["task-a", "task-b"]);
    }

    #[tokio::test]
    async fn test_push_events_rebroadcasts_to_sse() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "controller-node".into(),
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
            controller: crate::config::ControllerConfig {
                enabled: true,
                ..crate::config::ControllerConfig::default()
            },
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let session_index = Arc::new(SessionIndex::new());
        let command_queue = Arc::new(CommandQueue::new());
        let state = AppState::with_event_tx_controller(
            config,
            tmpdir.path().join("config.toml"),
            manager,
            peer_registry,
            event_tx.clone(),
            store,
            Some(session_index),
            Some(command_queue),
        );

        // Subscribe to the broadcast channel before pushing
        let mut rx = event_tx.subscribe();

        let app = routes::build(state);
        let server = TestServer::new(app).unwrap();

        let token = enroll_node(&server, "node-1").await;
        let req = EventPushRequest {
            events: vec![make_session_event("s1", "task-a", "active")],
        };
        server
            .post("/api/v1/events/push")
            .add_header("authorization", format!("Bearer {token}"))
            .json(&req)
            .await;

        let received = rx.recv().await.unwrap();
        match received {
            PulpoEvent::Session(se) => {
                assert_eq!(se.session_id, "s1");
                assert_eq!(se.status, "active");
            }
            PulpoEvent::SessionDeleted(_) => panic!("expected session event"),
        }
    }

    #[tokio::test]
    async fn test_push_events_forbidden_on_standalone() {
        let server = standalone_test_server().await;
        let req = EventPushRequest {
            events: vec![make_session_event("s1", "task-a", "active")],
        };
        let resp = server.post("/api/v1/events/push").json(&req).await;
        resp.assert_status(axum::http::StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_push_events_empty_batch() {
        let (server, _) = controller_test_server().await;
        let token = enroll_node(&server, "node-1").await;
        let req = EventPushRequest { events: vec![] };
        let resp = server
            .post("/api/v1/events/push")
            .add_header("authorization", format!("Bearer {token}"))
            .json(&req)
            .await;
        resp.assert_status(axum::http::StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_push_deleted_event_removes_index_entry() {
        let (server, _) = controller_test_server().await;
        let token = enroll_node(&server, "node-1").await;

        let create_req = EventPushRequest {
            events: vec![make_session_event("s1", "task-a", "active")],
        };
        server
            .post("/api/v1/events/push")
            .add_header("authorization", format!("Bearer {token}"))
            .json(&create_req)
            .await;

        let delete_req = EventPushRequest {
            events: vec![make_deleted_event("s1", "task-a")],
        };
        let resp = server
            .post("/api/v1/events/push")
            .add_header("authorization", format!("Bearer {token}"))
            .json(&delete_req)
            .await;
        resp.assert_status(axum::http::StatusCode::NO_CONTENT);

        let fleet_resp = server.get("/api/v1/fleet/sessions").await;
        fleet_resp.assert_status_ok();
        let body: FleetSessionsResponse = fleet_resp.json();
        assert!(body.sessions.is_empty());
    }

    #[tokio::test]
    async fn test_push_event_recovers_lost_session_and_persists_update() {
        let recovered_id = "22222222-2222-2222-2222-222222222222";
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        store
            .upsert_controller_session_index_entry(&SessionIndexEntry {
                session_id: recovered_id.into(),
                node_name: "node-1".into(),
                node_address: Some("node-1.tail:7433".into()),
                session_name: "task-a".into(),
                status: "lost".into(),
                command: None,
                updated_at: "2026-03-30T11:59:00Z".into(),
            })
            .await
            .unwrap();
        let config = Config {
            node: NodeConfig {
                name: "controller-node".into(),
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
            controller: crate::config::ControllerConfig {
                enabled: true,
                ..crate::config::ControllerConfig::default()
            },
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let session_index = Arc::new(SessionIndex::new());
        session_index
            .upsert(SessionIndexEntry {
                session_id: recovered_id.into(),
                node_name: "node-1".into(),
                node_address: Some("node-1.tail:7433".into()),
                session_name: "task-a".into(),
                status: "lost".into(),
                command: None,
                updated_at: "2026-03-30T11:59:00Z".into(),
            })
            .await;
        let command_queue = Arc::new(CommandQueue::new());
        let state = AppState::with_event_tx_controller(
            config,
            tmpdir.path().join("config.toml"),
            manager,
            peer_registry,
            event_tx,
            store.clone(),
            Some(session_index),
            Some(command_queue),
        );
        let app = routes::build(state);
        let server = TestServer::new(app).unwrap();

        let token = enroll_node(&server, "node-1").await;
        let req = EventPushRequest {
            events: vec![make_session_event(recovered_id, "task-a", "active")],
        };
        let resp = server
            .post("/api/v1/events/push")
            .add_header("authorization", format!("Bearer {token}"))
            .json(&req)
            .await;
        resp.assert_status(axum::http::StatusCode::NO_CONTENT);

        let fleet_resp = server.get("/api/v1/fleet/sessions").await;
        fleet_resp.assert_status_ok();
        let body: FleetSessionsResponse = fleet_resp.json();
        let recovered = body
            .sessions
            .iter()
            .find(|entry| entry.session.id.to_string() == recovered_id)
            .unwrap();
        assert_eq!(recovered.session.status.to_string(), "active");

        let entries = store.list_controller_session_index_entries().await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].status, "active");
    }

    #[tokio::test]
    async fn test_push_deleted_event_is_idempotent() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "controller-node".into(),
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
            controller: crate::config::ControllerConfig {
                enabled: true,
                ..crate::config::ControllerConfig::default()
            },
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let session_index = Arc::new(SessionIndex::new());
        let command_queue = Arc::new(CommandQueue::new());
        let state = AppState::with_event_tx_controller(
            config,
            tmpdir.path().join("config.toml"),
            manager,
            peer_registry,
            event_tx,
            store.clone(),
            Some(session_index),
            Some(command_queue),
        );
        let app = routes::build(state);
        let server = TestServer::new(app).unwrap();

        let token = enroll_node(&server, "node-1").await;
        let create_req = EventPushRequest {
            events: vec![make_session_event("s1", "task-a", "active")],
        };
        server
            .post("/api/v1/events/push")
            .add_header("authorization", format!("Bearer {token}"))
            .json(&create_req)
            .await;

        let delete_req = EventPushRequest {
            events: vec![make_deleted_event("s1", "task-a")],
        };
        server
            .post("/api/v1/events/push")
            .add_header("authorization", format!("Bearer {token}"))
            .json(&delete_req)
            .await;
        let resp = server
            .post("/api/v1/events/push")
            .add_header("authorization", format!("Bearer {token}"))
            .json(&delete_req)
            .await;
        resp.assert_status(axum::http::StatusCode::NO_CONTENT);

        let entries = store.list_controller_session_index_entries().await.unwrap();
        assert!(entries.is_empty());
    }
}
