use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use pulpo_common::api::{ErrorResponse, NodeCommandsResponse};

use super::AppState;
use super::node_auth::authenticate_node;

/// Nodes poll this endpoint to receive pending commands from the controller.
pub async fn get_commands(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let Some(command_queue) = &state.command_queue else {
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

    let commands = command_queue.drain(&node.node_name).await;
    Json(NodeCommandsResponse { commands }).into_response()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use axum_test::TestServer;
    use pulpo_common::api::{
        EnrollNodeRequest, EnrollNodeResponse, NodeCommand, NodeCommandsResponse,
    };

    use crate::api::AppState;
    use crate::api::node_auth::hash_node_token;
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
            controller: crate::config::ControllerConfig::default(),
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
            controller: crate::config::ControllerConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(config, manager, peer_registry, store);
        let app = routes::build(state);
        TestServer::new(app).unwrap()
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
    async fn test_enqueue_and_poll_commands() {
        let (server, state) = controller_test_server().await;
        let token = enroll_node(&server, "worker-1").await;
        // Enqueue two commands directly into the controller queue
        let queue = state.command_queue.as_ref().unwrap().clone();
        let cmd1 = NodeCommand::CreateSession {
            command_id: "c1".into(),
            name: "task-1".into(),
            workdir: None,
            command: None,
            ink: None,
            description: None,
        };
        queue.enqueue("worker-1", cmd1).await;

        let cmd2 = NodeCommand::StopSession {
            command_id: "c2".into(),
            session_id: "s1".into(),
        };
        queue.enqueue("worker-1", cmd2).await;

        // Poll for commands
        let resp = server
            .get("/api/v1/node/commands")
            .add_header("authorization", format!("Bearer {token}"))
            .await;
        resp.assert_status_ok();
        let body: NodeCommandsResponse = resp.json();
        assert_eq!(body.commands.len(), 2);

        // Verify FIFO order
        match &body.commands[0] {
            NodeCommand::CreateSession { command_id, .. } => assert_eq!(command_id, "c1"),
            NodeCommand::StopSession { .. } => panic!("expected CreateSession"),
        }
        match &body.commands[1] {
            NodeCommand::StopSession { command_id, .. } => assert_eq!(command_id, "c2"),
            NodeCommand::CreateSession { .. } => panic!("expected StopSession"),
        }

        // Second poll should return empty
        let resp = server
            .get("/api/v1/node/commands")
            .add_header("authorization", format!("Bearer {token}"))
            .await;
        resp.assert_status_ok();
        let body: NodeCommandsResponse = resp.json();
        assert!(body.commands.is_empty());
    }

    #[tokio::test]
    async fn test_poll_empty_commands() {
        let (server, _) = controller_test_server().await;
        let token = enroll_node(&server, "worker-1").await;
        let resp = server
            .get("/api/v1/node/commands")
            .add_header("authorization", format!("Bearer {token}"))
            .await;
        resp.assert_status_ok();
        let body: NodeCommandsResponse = resp.json();
        assert!(body.commands.is_empty());
    }

    #[tokio::test]
    async fn test_get_commands_forbidden_on_standalone() {
        let server = standalone_test_server().await;
        let resp = server.get("/api/v1/node/commands").await;
        resp.assert_status(axum::http::StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_get_commands_requires_registered_node() {
        let (server, _) = controller_test_server().await;
        let resp = server
            .get("/api/v1/node/commands")
            .add_header("authorization", "Bearer unknown-worker")
            .await;
        resp.assert_status(axum::http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_enroll_node_binds_token_to_name() {
        let (server, state) = controller_test_server().await;
        let token = enroll_node(&server, "worker-1").await;
        let enrolled = state
            .store
            .get_enrolled_controller_node_by_name("worker-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(enrolled.token_hash, hash_node_token(&token));
    }
}
