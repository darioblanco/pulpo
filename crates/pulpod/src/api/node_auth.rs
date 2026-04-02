use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use pulpo_common::api::{
    EnrollNodeRequest, EnrollNodeResponse, EnrolledNodeInfo, EnrolledNodesResponse, ErrorResponse,
};
use sha2::{Digest, Sha256};

use super::AppState;
use crate::config;

type ApiError = (StatusCode, Json<ErrorResponse>);

#[derive(Debug, Clone)]
pub struct AuthenticatedNode {
    pub node_name: String,
}

fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
}

pub fn hash_node_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

fn unauthorized(msg: &str) -> ApiError {
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorResponse {
            error: msg.to_owned(),
        }),
    )
}

pub async fn authenticate_node(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<AuthenticatedNode, ApiError> {
    let token =
        extract_bearer_token(headers).ok_or_else(|| unauthorized("node bearer token required"))?;
    let token_hash = hash_node_token(token);
    let enrolled_node = state
        .store
        .get_enrolled_controller_node_by_token_hash(&token_hash)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("failed to look up enrolled node: {e}"),
                }),
            )
        })?
        .ok_or_else(|| unauthorized("node is not enrolled on this controller"))?;
    Ok(AuthenticatedNode {
        node_name: enrolled_node.node_name,
    })
}

/// Explicitly enroll a node on the controller and mint its bearer token.
pub async fn enroll_node(
    State(state): State<Arc<AppState>>,
    Json(req): Json<EnrollNodeRequest>,
) -> impl IntoResponse {
    if state.command_queue.is_none() || state.session_index.is_none() {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "This node is not in controller mode".into(),
            }),
        )
            .into_response();
    }

    let token = config::generate_token();
    let token_hash = hash_node_token(&token);

    match state
        .store
        .get_enrolled_controller_node_by_name(&req.node_name)
        .await
    {
        Ok(Some(_)) => {
            return (
                StatusCode::CONFLICT,
                Json(ErrorResponse {
                    error: "node is already enrolled on this controller".into(),
                }),
            )
                .into_response();
        }
        Ok(None) => {}
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("failed to load enrolled node: {e}"),
                }),
            )
                .into_response();
        }
    }

    if let Err(e) = state
        .store
        .enroll_controller_node(&req.node_name, &token_hash, None, None)
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("failed to enroll node: {e}"),
            }),
        )
            .into_response();
    }

    (
        StatusCode::CREATED,
        Json(EnrollNodeResponse {
            node_name: req.node_name,
            token,
        }),
    )
        .into_response()
}

pub async fn list_enrolled_nodes(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if state.command_queue.is_none() || state.session_index.is_none() {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "This node is not in controller mode".into(),
            }),
        )
            .into_response();
    }

    let nodes = match state.store.list_enrolled_controller_nodes().await {
        Ok(nodes) => nodes,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("failed to list enrolled nodes: {e}"),
                }),
            )
                .into_response();
        }
    };

    Json(EnrolledNodesResponse {
        nodes: nodes
            .into_iter()
            .map(|node| EnrolledNodeInfo {
                node_name: node.node_name,
                last_seen_at: node.last_seen_at.map(|dt| dt.to_rfc3339()),
                last_seen_address: node.last_seen_address,
            })
            .collect(),
    })
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;

    use axum_test::TestServer;
    use pulpo_common::api::{EnrollNodeRequest, EnrollNodeResponse, EnrolledNodesResponse};

    use crate::api::routes;
    use crate::backend::StubBackend;
    use crate::config::{Config, NodeConfig};
    use crate::controller::{CommandQueue, SessionIndex};
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;

    #[test]
    fn test_hash_node_token_deterministic() {
        assert_eq!(hash_node_token("abc"), hash_node_token("abc"));
        assert_ne!(hash_node_token("abc"), hash_node_token("abcd"));
    }

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

    #[tokio::test]
    async fn test_enroll_node_creates_token_and_store_record() {
        let (server, state) = controller_test_server().await;
        let resp = server
            .post("/api/v1/controller/nodes")
            .json(&EnrollNodeRequest {
                node_name: "worker-1".into(),
            })
            .await;
        resp.assert_status(StatusCode::CREATED);
        let body: EnrollNodeResponse = resp.json();
        assert_eq!(body.node_name, "worker-1");
        assert!(!body.token.is_empty());

        let enrolled = state
            .store
            .get_enrolled_controller_node_by_name("worker-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(enrolled.token_hash, hash_node_token(&body.token));
    }

    #[tokio::test]
    async fn test_enroll_node_conflict_returns_conflict() {
        let (server, _) = controller_test_server().await;
        server
            .post("/api/v1/controller/nodes")
            .json(&EnrollNodeRequest {
                node_name: "worker-1".into(),
            })
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server
            .post("/api/v1/controller/nodes")
            .json(&EnrollNodeRequest {
                node_name: "worker-1".into(),
            })
            .await;
        resp.assert_status(StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_list_enrolled_nodes_returns_nodes() {
        let (server, _) = controller_test_server().await;
        let enroll_resp = server
            .post("/api/v1/controller/nodes")
            .json(&EnrollNodeRequest {
                node_name: "worker-1".into(),
            })
            .await;
        enroll_resp.assert_status(StatusCode::CREATED);
        let body: EnrollNodeResponse = enroll_resp.json();

        server
            .post("/api/v1/events/push")
            .add_header("authorization", format!("Bearer {}", body.token))
            .json(&pulpo_common::api::EventPushRequest { events: vec![] })
            .await
            .assert_status(StatusCode::NO_CONTENT);

        let resp = server.get("/api/v1/controller/nodes").await;
        resp.assert_status(StatusCode::OK);
        let list: EnrolledNodesResponse = resp.json();
        assert_eq!(list.nodes.len(), 1);
        assert_eq!(list.nodes[0].node_name, "worker-1");
        assert!(list.nodes[0].last_seen_at.is_some());
    }

    #[tokio::test]
    async fn test_enroll_node_forbidden_on_standalone() {
        let server = standalone_test_server().await;
        let resp = server
            .post("/api/v1/controller/nodes")
            .json(&EnrollNodeRequest {
                node_name: "worker-1".into(),
            })
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_list_enrolled_nodes_forbidden_on_standalone() {
        let server = standalone_test_server().await;
        let resp = server.get("/api/v1/controller/nodes").await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }
}
