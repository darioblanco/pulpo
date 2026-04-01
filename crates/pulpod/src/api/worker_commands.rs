use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use pulpo_common::api::{ErrorResponse, WorkerCommand, WorkerCommandsResponse};

use super::AppState;

/// Workers poll this endpoint to receive pending commands from the master.
pub async fn get_commands(
    State(state): State<Arc<AppState>>,
    Path(node_name): Path<String>,
) -> impl IntoResponse {
    let Some(command_queue) = &state.command_queue else {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "This node is not in master mode".into(),
            }),
        )
            .into_response();
    };

    let commands = command_queue.drain(&node_name).await;
    Json(WorkerCommandsResponse { commands }).into_response()
}

/// Enqueue a command for a specific worker node.
pub async fn enqueue_command(
    State(state): State<Arc<AppState>>,
    Path(node_name): Path<String>,
    Json(command): Json<WorkerCommand>,
) -> impl IntoResponse {
    let Some(command_queue) = &state.command_queue else {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "This node is not in master mode".into(),
            }),
        )
            .into_response();
    };

    command_queue.enqueue(&node_name, command).await;
    StatusCode::CREATED.into_response()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use axum_test::TestServer;
    use pulpo_common::api::{WorkerCommand, WorkerCommandsResponse};

    use crate::api::AppState;
    use crate::api::routes;
    use crate::backend::StubBackend;
    use crate::config::{Config, NodeConfig};
    use crate::master::{CommandQueue, SessionIndex};
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;

    async fn master_test_server() -> TestServer {
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
            master: crate::config::MasterConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let session_index = Arc::new(SessionIndex::new());
        let command_queue = Arc::new(CommandQueue::new());
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
        TestServer::new(app).unwrap()
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
            master: crate::config::MasterConfig::default(),
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
    async fn test_enqueue_and_poll_commands() {
        let server = master_test_server().await;

        // Enqueue two commands
        let cmd1 = WorkerCommand::CreateSession {
            command_id: "c1".into(),
            name: "task-1".into(),
            workdir: None,
            command: None,
            ink: None,
            description: None,
        };
        let resp = server
            .post("/api/v1/workers/worker-1/commands")
            .json(&cmd1)
            .await;
        resp.assert_status(axum::http::StatusCode::CREATED);

        let cmd2 = WorkerCommand::StopSession {
            command_id: "c2".into(),
            session_id: "s1".into(),
        };
        let resp = server
            .post("/api/v1/workers/worker-1/commands")
            .json(&cmd2)
            .await;
        resp.assert_status(axum::http::StatusCode::CREATED);

        // Poll for commands
        let resp = server.get("/api/v1/workers/worker-1/commands").await;
        resp.assert_status_ok();
        let body: WorkerCommandsResponse = resp.json();
        assert_eq!(body.commands.len(), 2);

        // Verify FIFO order
        match &body.commands[0] {
            WorkerCommand::CreateSession { command_id, .. } => assert_eq!(command_id, "c1"),
            WorkerCommand::StopSession { .. } => panic!("expected CreateSession"),
        }
        match &body.commands[1] {
            WorkerCommand::StopSession { command_id, .. } => assert_eq!(command_id, "c2"),
            WorkerCommand::CreateSession { .. } => panic!("expected StopSession"),
        }

        // Second poll should return empty
        let resp = server.get("/api/v1/workers/worker-1/commands").await;
        resp.assert_status_ok();
        let body: WorkerCommandsResponse = resp.json();
        assert!(body.commands.is_empty());
    }

    #[tokio::test]
    async fn test_poll_empty_commands() {
        let server = master_test_server().await;
        let resp = server.get("/api/v1/workers/worker-1/commands").await;
        resp.assert_status_ok();
        let body: WorkerCommandsResponse = resp.json();
        assert!(body.commands.is_empty());
    }

    #[tokio::test]
    async fn test_get_commands_forbidden_on_standalone() {
        let server = standalone_test_server().await;
        let resp = server.get("/api/v1/workers/worker-1/commands").await;
        resp.assert_status(axum::http::StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_enqueue_command_forbidden_on_standalone() {
        let server = standalone_test_server().await;
        let cmd = WorkerCommand::StopSession {
            command_id: "c1".into(),
            session_id: "s1".into(),
        };
        let resp = server
            .post("/api/v1/workers/worker-1/commands")
            .json(&cmd)
            .await;
        resp.assert_status(axum::http::StatusCode::FORBIDDEN);
    }
}
