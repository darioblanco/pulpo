use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use pulpo_common::api::{EnrollWorkerRequest, EnrollWorkerResponse, ErrorResponse};
use sha2::{Digest, Sha256};

use super::AppState;
use crate::config;

type ApiError = (StatusCode, Json<ErrorResponse>);

#[derive(Debug, Clone)]
pub struct AuthenticatedWorker {
    pub node_name: String,
}

fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
}

pub fn hash_worker_token(token: &str) -> String {
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

pub async fn authenticate_worker(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<AuthenticatedWorker, ApiError> {
    let token = extract_bearer_token(headers)
        .ok_or_else(|| unauthorized("worker bearer token required"))?;
    let token_hash = hash_worker_token(token);
    let worker = state
        .store
        .get_enrolled_master_worker_by_token_hash(&token_hash)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("failed to look up enrolled worker: {e}"),
                }),
            )
        })?
        .ok_or_else(|| unauthorized("worker is not enrolled on this master"))?;
    Ok(AuthenticatedWorker {
        node_name: worker.node_name,
    })
}

/// Explicitly enroll a worker on the master and mint its bearer token.
pub async fn enroll_worker(
    State(state): State<Arc<AppState>>,
    Json(req): Json<EnrollWorkerRequest>,
) -> impl IntoResponse {
    if state.command_queue.is_none() || state.session_index.is_none() {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "This node is not in master mode".into(),
            }),
        )
            .into_response();
    }

    let token = config::generate_token();
    let token_hash = hash_worker_token(&token);

    match state
        .store
        .get_enrolled_master_worker_by_name(&req.node_name)
        .await
    {
        Ok(Some(_)) => {
            return (
                StatusCode::CONFLICT,
                Json(ErrorResponse {
                    error: "worker is already enrolled on this master".into(),
                }),
            )
                .into_response();
        }
        Ok(None) => {}
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("failed to load enrolled worker: {e}"),
                }),
            )
                .into_response();
        }
    }

    if let Err(e) = state
        .store
        .enroll_master_worker(&req.node_name, &token_hash, None, None)
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("failed to enroll worker: {e}"),
            }),
        )
            .into_response();
    }

    (
        StatusCode::CREATED,
        Json(EnrollWorkerResponse {
            node_name: req.node_name,
            token,
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;

    use axum_test::TestServer;
    use pulpo_common::api::{EnrollWorkerRequest, EnrollWorkerResponse};

    use crate::api::routes;
    use crate::backend::StubBackend;
    use crate::config::{Config, NodeConfig};
    use crate::master::{CommandQueue, SessionIndex};
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;

    #[test]
    fn test_hash_worker_token_deterministic() {
        assert_eq!(hash_worker_token("abc"), hash_worker_token("abc"));
        assert_ne!(hash_worker_token("abc"), hash_worker_token("abcd"));
    }

    async fn master_test_server() -> (TestServer, Arc<AppState>) {
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
    async fn test_enroll_worker_creates_token_and_store_record() {
        let (server, state) = master_test_server().await;
        let resp = server
            .post("/api/v1/master/workers")
            .json(&EnrollWorkerRequest {
                node_name: "worker-1".into(),
            })
            .await;
        resp.assert_status(StatusCode::CREATED);
        let body: EnrollWorkerResponse = resp.json();
        assert_eq!(body.node_name, "worker-1");
        assert!(!body.token.is_empty());

        let enrolled = state
            .store
            .get_enrolled_master_worker_by_name("worker-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(enrolled.token_hash, hash_worker_token(&body.token));
    }

    #[tokio::test]
    async fn test_enroll_worker_conflict_returns_conflict() {
        let (server, _) = master_test_server().await;
        server
            .post("/api/v1/master/workers")
            .json(&EnrollWorkerRequest {
                node_name: "worker-1".into(),
            })
            .await
            .assert_status(StatusCode::CREATED);

        let resp = server
            .post("/api/v1/master/workers")
            .json(&EnrollWorkerRequest {
                node_name: "worker-1".into(),
            })
            .await;
        resp.assert_status(StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_enroll_worker_forbidden_on_standalone() {
        let server = standalone_test_server().await;
        let resp = server
            .post("/api/v1/master/workers")
            .json(&EnrollWorkerRequest {
                node_name: "worker-1".into(),
            })
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }
}
