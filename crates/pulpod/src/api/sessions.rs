use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use pulpo_common::api::{
    CreateSessionRequest, CreateSessionResponse, ErrorResponse, ListSessionsQuery, NodeCommand,
    OutputQuery, SendInputRequest, SessionIndexEntry,
};
use pulpo_common::peer::PeerInfo;
use pulpo_common::session::{Session, SessionStatus};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::remote::{
    RemoteNodeTarget, apply_remote_auth, normalize_http_base, remote_client, resolve_peer_target,
};

type ApiError = (StatusCode, Json<ErrorResponse>);

fn internal_error(msg: &str) -> ApiError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: msg.to_owned(),
        }),
    )
}

fn bad_gateway(msg: &str) -> ApiError {
    (
        StatusCode::BAD_GATEWAY,
        Json(ErrorResponse {
            error: msg.to_owned(),
        }),
    )
}

fn session_from_index_entry(entry: SessionIndexEntry) -> Session {
    let status = entry
        .status
        .parse::<SessionStatus>()
        .unwrap_or(SessionStatus::Lost);

    Session {
        id: Uuid::parse_str(&entry.session_id).unwrap_or_else(|_| Uuid::nil()),
        name: entry.session_name,
        command: entry.command.unwrap_or_default(),
        status,
        updated_at: chrono::DateTime::parse_from_rfc3339(&entry.updated_at)
            .map_or_else(|_| chrono::Utc::now(), |dt| dt.with_timezone(&chrono::Utc)),
        ..Session::default()
    }
}

#[derive(Debug, Clone)]
struct RemoteWorkerTarget {
    session_id: String,
    node_name: String,
    base_url: String,
    token: Option<String>,
}

fn reqwest_status_to_axum(status: reqwest::StatusCode) -> StatusCode {
    StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY)
}

async fn reqwest_error_response(resp: reqwest::Response, fallback: &str) -> ApiError {
    let status = reqwest_status_to_axum(resp.status());
    let error = match resp.json::<ErrorResponse>().await {
        Ok(body) => body.error,
        Err(_) => fallback.to_owned(),
    };
    (status, Json(ErrorResponse { error }))
}

async fn send_remote_request(
    request: reqwest::RequestBuilder,
    failure: String,
) -> Result<reqwest::Response, ApiError> {
    request
        .send()
        .await
        .map_err(|e| bad_gateway(&format!("{failure}: {e}")))
}

fn remote_json_request(
    target: &RemoteWorkerTarget,
    request: reqwest::RequestBuilder,
) -> reqwest::RequestBuilder {
    apply_remote_auth(request, target.token.as_deref())
}

async fn parse_remote_json<T: DeserializeOwned>(
    resp: reqwest::Response,
    fallback: &str,
    parse_error: &str,
) -> Result<T, ApiError> {
    if !resp.status().is_success() {
        return Err(reqwest_error_response(resp, fallback).await);
    }

    resp.json::<T>()
        .await
        .map_err(|e| internal_error(&format!("{parse_error}: {e}")))
}

async fn expect_remote_no_content(
    resp: reqwest::Response,
    fallback: &str,
) -> Result<StatusCode, ApiError> {
    if !resp.status().is_success() {
        return Err(reqwest_error_response(resp, fallback).await);
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn resolve_remote_worker_target(
    state: &Arc<super::AppState>,
    id: &str,
) -> Result<Option<RemoteWorkerTarget>, ApiError> {
    let Some(session_index) = &state.session_index else {
        return Ok(None);
    };
    let Some(entry) = session_index.get(id).await else {
        return Ok(None);
    };

    let peer: Option<PeerInfo> = state.peer_registry.get(&entry.node_name).await;
    let address = peer
        .as_ref()
        .map(|p| p.address.clone())
        .or_else(|| entry.node_address.clone());

    let Some(address) = address else {
        return Err(bad_gateway(&format!(
            "node address unknown for remote session {id} on node {}",
            entry.node_name
        )));
    };

    let token = state.peer_registry.get_token(&entry.node_name).await;
    Ok(Some(RemoteWorkerTarget {
        session_id: entry.session_id,
        node_name: entry.node_name,
        base_url: normalize_http_base(&address),
        token,
    }))
}

async fn resolve_remote_node_target(
    state: &Arc<super::AppState>,
    target_node: &str,
) -> Result<Option<RemoteNodeTarget>, ApiError> {
    let local_name = state.config.read().await.node.name.clone();
    if target_node == local_name {
        return Ok(None);
    }

    let Some(target) = resolve_peer_target(&state.peer_registry, target_node).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("target node not found: {target_node}"),
            }),
        ));
    };
    Ok(Some(target))
}

pub async fn list(
    State(state): State<Arc<super::AppState>>,
    Query(query): Query<ListSessionsQuery>,
) -> Result<Json<Vec<Session>>, ApiError> {
    let has_filters = query.status.is_some()
        || query.search.is_some()
        || query.sort.is_some()
        || query.order.is_some();

    let sessions = if has_filters {
        state
            .session_manager
            .list_sessions_filtered(&query)
            .await
            .map_err(|e| internal_error(&e.to_string()))?
    } else {
        state
            .session_manager
            .list_sessions()
            .await
            .map_err(|e| internal_error(&e.to_string()))?
    };
    Ok(Json(sessions))
}

pub async fn get(
    State(state): State<Arc<super::AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Session>, ApiError> {
    match state.session_manager.get_session(&id).await {
        Ok(Some(session)) => Ok(Json(session)),
        Ok(None) => {
            if let Some(session_index) = &state.session_index
                && let Some(entry) = session_index.get(&id).await
            {
                return Ok(Json(session_from_index_entry(entry)));
            }
            Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("session not found: {id}"),
                }),
            ))
        }
        Err(e) => Err(internal_error(&e.to_string())),
    }
}

pub async fn create(
    State(state): State<Arc<super::AppState>>,
    Json(mut req): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<CreateSessionResponse>), ApiError> {
    if let Some(target_node) = req.target_node.clone() {
        let role = state.config.read().await.role();
        if role != crate::config::NodeRole::Controller {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "target_node requires controller mode".into(),
                }),
            ));
        }

        if let Some(target) = resolve_remote_node_target(&state, &target_node).await? {
            req.target_node = None;
            let client = remote_client()
                .map_err(|e| internal_error(&format!("failed to build HTTP client: {e}")))?;
            let resp = send_remote_request(
                apply_remote_auth(
                    client
                        .post(format!("{}/api/v1/sessions", target.base_url))
                        .json(&req),
                    target.token.as_deref(),
                ),
                format!("failed to create session on node {}", target.node_name),
            )
            .await?;
            let body = parse_remote_json::<CreateSessionResponse>(
                resp,
                "failed to create remote session",
                "failed to parse remote create response",
            )
            .await?;
            return Ok((StatusCode::CREATED, Json(body)));
        }
    }

    let session = state
        .session_manager
        .create_session(req)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("already active") {
                (StatusCode::CONFLICT, Json(ErrorResponse { error: msg }))
            } else {
                internal_error(&msg)
            }
        })?;
    Ok((StatusCode::CREATED, Json(CreateSessionResponse { session })))
}

#[derive(Deserialize)]
pub struct StopQuery {
    pub purge: Option<bool>,
}

pub async fn stop(
    State(state): State<Arc<super::AppState>>,
    Path(id): Path<String>,
    Query(query): Query<StopQuery>,
) -> Result<StatusCode, ApiError> {
    match state
        .session_manager
        .stop_session(&id, query.purge.unwrap_or(false))
        .await
    {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("not found") {
                if let (Some(session_index), Some(command_queue)) =
                    (&state.session_index, &state.command_queue)
                    && let Some(entry) = session_index.get(&id).await
                {
                    command_queue
                        .enqueue(
                            &entry.node_name,
                            NodeCommand::StopSession {
                                command_id: Uuid::new_v4().to_string(),
                                session_id: id,
                            },
                        )
                        .await;
                    return Ok(StatusCode::ACCEPTED);
                }

                Err((StatusCode::NOT_FOUND, Json(ErrorResponse { error: msg })))
            } else {
                Err(internal_error(&msg))
            }
        }
    }
}

pub async fn cleanup(
    State(state): State<Arc<super::AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let count = state
        .store
        .cleanup_dead_sessions()
        .await
        .map_err(|e| internal_error(&e.to_string()))?;
    Ok(Json(serde_json::json!({ "deleted": count })))
}

pub async fn output(
    State(state): State<Arc<super::AppState>>,
    Path(id): Path<String>,
    Query(query): Query<OutputQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Some(session) = state
        .session_manager
        .get_session(&id)
        .await
        .map_err(|e| internal_error(&e.to_string()))?
    else {
        let Some(target) = resolve_remote_worker_target(&state, &id).await? else {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("session not found: {id}"),
                }),
            ));
        };

        let lines = query.lines.unwrap_or(100);
        let client = remote_client()
            .map_err(|e| internal_error(&format!("failed to build HTTP client: {e}")))?;
        let url = format!(
            "{}/api/v1/sessions/{}/output?lines={lines}",
            target.base_url, target.session_id
        );
        let resp = send_remote_request(
            remote_json_request(&target, client.get(url)),
            format!("failed to fetch output from node {}", target.node_name),
        )
        .await?;
        let body = parse_remote_json::<serde_json::Value>(
            resp,
            "failed to fetch remote output",
            "failed to parse remote output",
        )
        .await?;
        return Ok(Json(body));
    };

    let lines = query.lines.unwrap_or(100);
    let backend_id = state.session_manager.resolve_backend_id(&session);
    let output = state
        .session_manager
        .capture_output(&id, &backend_id, lines);

    Ok(Json(serde_json::json!({ "output": output })))
}

pub async fn resume(
    State(state): State<Arc<super::AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Session>, ApiError> {
    match state.session_manager.resume_session(&id).await {
        Ok(session) => Ok(Json(session)),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("not found") {
                if let Some(target) = resolve_remote_worker_target(&state, &id).await? {
                    let client = remote_client().map_err(|e| {
                        internal_error(&format!("failed to build HTTP client: {e}"))
                    })?;
                    let url = format!(
                        "{}/api/v1/sessions/{}/resume",
                        target.base_url, target.session_id
                    );
                    let resp = send_remote_request(
                        remote_json_request(&target, client.post(url)),
                        format!("failed to resume session on node {}", target.node_name),
                    )
                    .await?;
                    let session = parse_remote_json::<Session>(
                        resp,
                        "failed to resume remote session",
                        "failed to parse remote resume response",
                    )
                    .await?;
                    return Ok(Json(session));
                }
                Err((StatusCode::NOT_FOUND, Json(ErrorResponse { error: msg })))
            } else if msg.contains("cannot be resumed") {
                Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: msg })))
            } else if msg.contains("already active") {
                Err((StatusCode::CONFLICT, Json(ErrorResponse { error: msg })))
            } else {
                Err(internal_error(&msg))
            }
        }
    }
}

pub async fn download_output(
    State(state): State<Arc<super::AppState>>,
    Path(id): Path<String>,
) -> Result<
    (
        StatusCode,
        [(axum::http::header::HeaderName, String); 2],
        String,
    ),
    ApiError,
> {
    let Some(session) = state
        .session_manager
        .get_session(&id)
        .await
        .map_err(|e| internal_error(&e.to_string()))?
    else {
        let Some(target) = resolve_remote_worker_target(&state, &id).await? else {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("session not found: {id}"),
                }),
            ));
        };

        let client = remote_client()
            .map_err(|e| internal_error(&format!("failed to build HTTP client: {e}")))?;
        let url = format!(
            "{}/api/v1/sessions/{}/output/download",
            target.base_url, target.session_id
        );
        let resp = send_remote_request(
            remote_json_request(&target, client.get(url)),
            format!("failed to download output from node {}", target.node_name),
        )
        .await?;
        if !resp.status().is_success() {
            return Err(
                reqwest_error_response(resp, "failed to download remote session output").await,
            );
        }

        let headers = resp.headers().clone();
        let content_type = headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/plain; charset=utf-8")
            .to_owned();
        let content_disposition = headers
            .get(reqwest::header::CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("attachment; filename=\"session.log\"")
            .to_owned();
        let body = resp
            .text()
            .await
            .map_err(|e| internal_error(&format!("failed to read remote output download: {e}")))?;

        return Ok((
            StatusCode::OK,
            [
                (axum::http::header::CONTENT_TYPE, content_type),
                (axum::http::header::CONTENT_DISPOSITION, content_disposition),
            ],
            body,
        ));
    };

    let output = if session.status == SessionStatus::Active || session.status == SessionStatus::Lost
    {
        let backend_id = state.session_manager.resolve_backend_id(&session);
        state
            .session_manager
            .capture_output(&id, &backend_id, 10_000)
    } else {
        session.output_snapshot.unwrap_or_default()
    };

    let filename = format!("{}.log", session.name);
    Ok((
        StatusCode::OK,
        [
            (
                axum::http::header::CONTENT_TYPE,
                "text/plain; charset=utf-8".to_owned(),
            ),
            (
                axum::http::header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        output,
    ))
}

pub async fn list_interventions(
    State(state): State<Arc<super::AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<pulpo_common::api::InterventionEventResponse>>, ApiError> {
    let events = state
        .session_manager
        .store()
        .list_intervention_events(&id)
        .await
        .map_err(|e| internal_error(&e.to_string()))?;
    let response: Vec<_> = events
        .into_iter()
        .map(|e| pulpo_common::api::InterventionEventResponse {
            id: e.id,
            session_id: e.session_id,
            code: e.code,
            reason: e.reason,
            created_at: e.created_at.to_rfc3339(),
        })
        .collect();
    Ok(Json(response))
}

pub async fn input(
    State(state): State<Arc<super::AppState>>,
    Path(id): Path<String>,
    Json(req): Json<SendInputRequest>,
) -> Result<StatusCode, ApiError> {
    let Some(session) = state
        .session_manager
        .get_session(&id)
        .await
        .map_err(|e| internal_error(&e.to_string()))?
    else {
        let Some(target) = resolve_remote_worker_target(&state, &id).await? else {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("session not found: {id}"),
                }),
            ));
        };
        let client = remote_client()
            .map_err(|e| internal_error(&format!("failed to build HTTP client: {e}")))?;
        let url = format!(
            "{}/api/v1/sessions/{}/input",
            target.base_url, target.session_id
        );
        let resp = send_remote_request(
            remote_json_request(
                &target,
                client
                    .post(url)
                    .json(&serde_json::json!({ "text": req.text })),
            ),
            format!("failed to send input to node {}", target.node_name),
        )
        .await?;
        return expect_remote_no_content(resp, "failed to send input to remote session").await;
    };

    let backend_id = state.session_manager.resolve_backend_id(&session);
    state
        .session_manager
        .send_input(&backend_id, &req.text)
        .map_err(|e| internal_error(&e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::AppState;
    use crate::backend::Backend;
    use crate::controller::{CommandQueue, SessionIndex};
    use std::collections::HashMap;
    use tokio::sync::broadcast;

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
            Ok("test output".into())
        }
        fn send_input(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn setup_logging(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
    }

    async fn test_state_with_pool() -> (Arc<AppState>, sqlx::SqlitePool) {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let pool = store.pool().clone();
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(
            Config {
                node: NodeConfig {
                    name: "test-node".into(),
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
            },
            manager,
            peer_registry,
            store,
        );
        (state, pool)
    }

    async fn test_state() -> Arc<AppState> {
        let (state, _) = test_state_with_pool().await;
        state
    }

    async fn controller_state_with_index(entry: SessionIndexEntry) -> Arc<AppState> {
        controller_state_with_index_and_peers(entry, HashMap::new()).await
    }

    async fn controller_state_with_index_and_peers(
        entry: SessionIndexEntry,
        peers: HashMap<String, pulpo_common::peer::PeerEntry>,
    ) -> Arc<AppState> {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&peers);
        let (event_tx, _) = broadcast::channel(16);
        let session_index = Arc::new(SessionIndex::new());
        session_index.upsert(entry).await;
        let command_queue = Arc::new(CommandQueue::new());

        AppState::with_event_tx_controller(
            Config {
                node: NodeConfig {
                    name: "master-node".into(),
                    port: 7433,
                    data_dir: tmpdir.path().to_str().unwrap().into(),
                    ..NodeConfig::default()
                },
                auth: crate::config::AuthConfig::default(),
                peers,
                watchdog: crate::config::WatchdogConfig::default(),
                inks: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
                docker: crate::config::DockerConfig::default(),
                controller: crate::config::ControllerConfig {
                    enabled: true,
                    ..crate::config::ControllerConfig::default()
                },
            },
            tmpdir.path().join("config.toml"),
            manager,
            peer_registry,
            event_tx,
            store,
            Some(session_index),
            Some(command_queue),
        )
    }

    #[tokio::test]
    async fn test_list_returns_empty_vec() {
        let state = test_state().await;
        let query = ListSessionsQuery::default();
        let Json(sessions) = list(State(state), Query(query)).await.unwrap();
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn test_list_with_status_filter() {
        let state = test_state().await;
        let req = CreateSessionRequest {
            name: "filter-test".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: None,
        };
        let _ = create(State(state.clone()), Json(req)).await.unwrap();

        let query = ListSessionsQuery {
            status: Some("active".into()),
            ..Default::default()
        };
        let Json(sessions) = list(State(state.clone()), Query(query)).await.unwrap();
        assert_eq!(sessions.len(), 1);

        let query = ListSessionsQuery {
            status: Some("ready".into()),
            ..Default::default()
        };
        let Json(sessions) = list(State(state), Query(query)).await.unwrap();
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn test_get_returns_not_found() {
        let state = test_state().await;
        let result = get(State(state), Path("some-id".into())).await;
        assert!(result.is_err());
        let (status, Json(err)) = result.unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(err.error.contains("not found"));
    }

    #[tokio::test]
    async fn test_get_returns_remote_session_from_controller_index() {
        let session_id = Uuid::new_v4().to_string();
        let state = controller_state_with_index(SessionIndexEntry {
            session_id: session_id.clone(),
            node_name: "worker-1".into(),
            node_address: Some("worker-1.tailnet:7433".into()),
            session_name: "remote-task".into(),
            status: "active".into(),
            command: Some("claude -p build".into()),
            updated_at: "2026-03-30T12:00:00Z".into(),
        })
        .await;

        let Json(session) = get(State(state), Path(session_id)).await.unwrap();
        assert_eq!(session.name, "remote-task");
        assert_eq!(session.status, SessionStatus::Active);
        assert_eq!(session.command, "claude -p build");
    }

    #[tokio::test]
    async fn test_create_returns_created() {
        let state = test_state().await;
        let req = CreateSessionRequest {
            name: "test".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: None,
        };
        let result = create(State(state), Json(req)).await;
        assert!(result.is_ok());
        let (status, Json(resp)) = result.unwrap();
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(resp.session.name, "test");
    }

    #[tokio::test]
    async fn test_create_target_node_requires_controller() {
        let state = test_state().await;
        let req = CreateSessionRequest {
            name: "remote-create".into(),
            workdir: Some("/repo".into()),
            metadata: None,
            command: Some("claude code".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: Some("worker-1".into()),
        };

        let result = create(State(state), Json(req)).await;
        assert!(result.is_err());
        let (status, Json(err)) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(err.error.contains("target_node requires controller mode"));
    }

    #[tokio::test]
    async fn test_create_target_node_matching_controller_name_creates_locally() {
        let state = controller_state_with_index(SessionIndexEntry {
            session_id: Uuid::new_v4().to_string(),
            node_name: "master-node".into(),
            node_address: None,
            session_name: "existing-local".into(),
            status: "active".into(),
            command: Some("echo".into()),
            updated_at: "2026-03-30T12:00:00Z".into(),
        })
        .await;
        let req = CreateSessionRequest {
            name: "master-local".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo local".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: Some("master-node".into()),
        };

        let (status, Json(resp)) = create(State(state), Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(resp.session.name, "master-local");
    }

    #[tokio::test]
    async fn test_create_target_node_offline_worker_returns_bad_gateway() {
        let peers = HashMap::from([(
            "worker-1".to_owned(),
            pulpo_common::peer::PeerEntry::Full {
                address: "127.0.0.1:9".into(),
                token: Some("secret-token".into()),
            },
        )]);
        let state = controller_state_with_index_and_peers(
            SessionIndexEntry {
                session_id: Uuid::new_v4().to_string(),
                node_name: "master-node".into(),
                node_address: None,
                session_name: "existing-local".into(),
                status: "active".into(),
                command: Some("echo".into()),
                updated_at: "2026-03-30T12:00:00Z".into(),
            },
            peers,
        )
        .await;
        let req = CreateSessionRequest {
            name: "remote-create".into(),
            workdir: Some("/repo".into()),
            metadata: None,
            command: Some("claude code".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: Some("worker-1".into()),
        };

        let result = create(State(state), Json(req)).await;
        assert!(result.is_err());
        let (status, Json(err)) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(
            err.error
                .contains("failed to create session on node worker-1")
        );
    }

    #[tokio::test]
    async fn test_create_duplicate_name_returns_conflict() {
        let state = test_state().await;
        let req = || CreateSessionRequest {
            name: "dupe".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: None,
        };
        let _ = create(State(state.clone()), Json(req())).await.unwrap();
        let result = create(State(state), Json(req())).await;
        assert!(result.is_err());
        let (status, Json(body)) = result.unwrap_err();
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(body.error.contains("already active"));
    }

    #[tokio::test]
    async fn test_stop_not_found() {
        let state = test_state().await;
        let query = StopQuery { purge: None };
        let result = stop(State(state), Path("nonexistent".into()), Query(query)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_stop_enqueues_remote_command_when_session_is_indexed_on_controller() {
        let session_id = Uuid::new_v4().to_string();
        let state = controller_state_with_index(SessionIndexEntry {
            session_id: session_id.clone(),
            node_name: "worker-1".into(),
            node_address: Some("worker-1.tailnet:7433".into()),
            session_name: "remote-task".into(),
            status: "active".into(),
            command: Some("claude -p build".into()),
            updated_at: "2026-03-30T12:00:00Z".into(),
        })
        .await;
        let query = StopQuery { purge: None };

        let status = stop(State(state.clone()), Path(session_id.clone()), Query(query))
            .await
            .unwrap();
        assert_eq!(status, StatusCode::ACCEPTED);

        let commands = state
            .command_queue
            .as_ref()
            .unwrap()
            .drain("worker-1")
            .await;
        assert_eq!(commands.len(), 1);
        match &commands[0] {
            NodeCommand::StopSession {
                session_id: queued_id,
                ..
            } => assert_eq!(queued_id, &session_id),
            NodeCommand::CreateSession { .. } => panic!("expected stop command"),
        }
    }

    #[tokio::test]
    async fn test_stop_returns_no_content() {
        let state = test_state().await;
        let req = CreateSessionRequest {
            name: "stop-test".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: None,
        };
        let (_, Json(resp)) = create(State(state.clone()), Json(req)).await.unwrap();
        let session = resp.session;
        let query = StopQuery { purge: None };
        let result = stop(State(state), Path(session.id.to_string()), Query(query)).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_stop_with_purge() {
        let state = test_state().await;
        let req = CreateSessionRequest {
            name: "stop-purge".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: None,
        };
        let (_, Json(resp)) = create(State(state.clone()), Json(req)).await.unwrap();
        let session = resp.session;
        let query = StopQuery { purge: Some(true) };
        let result = stop(
            State(state.clone()),
            Path(session.id.to_string()),
            Query(query),
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), StatusCode::NO_CONTENT);

        // Session should be purged (not found)
        let get_result = get(State(state), Path(session.id.to_string())).await;
        assert!(get_result.is_err());
        let (status, _) = get_result.unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_output_for_session() {
        let state = test_state().await;
        let req = CreateSessionRequest {
            name: "output-test".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: None,
        };
        let (_, Json(resp)) = create(State(state.clone()), Json(req)).await.unwrap();
        let session = resp.session;

        let query = OutputQuery { lines: Some(50) };
        let result = output(State(state), Path(session.id.to_string()), Query(query)).await;
        assert!(result.is_ok());
        let Json(val) = result.unwrap();
        assert_eq!(val["output"], "test output");
    }

    #[tokio::test]
    async fn test_output_not_found() {
        let state = test_state().await;
        let query = OutputQuery { lines: None };
        let result = output(State(state), Path("nonexistent".into()), Query(query)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_resolve_remote_worker_target_uses_peer_registry() {
        let session_id = Uuid::new_v4().to_string();
        let mut peers = HashMap::new();
        peers.insert(
            "worker-1".into(),
            pulpo_common::peer::PeerEntry::Full {
                address: "worker-1.tailnet:7433".into(),
                token: Some("secret-token".into()),
            },
        );
        let controller_state = controller_state_with_index_and_peers(
            SessionIndexEntry {
                session_id: session_id.clone(),
                node_name: "worker-1".into(),
                node_address: None,
                session_name: "remote-output".into(),
                status: "active".into(),
                command: Some("echo test".into()),
                updated_at: "2026-03-30T12:00:00Z".into(),
            },
            peers,
        )
        .await;

        let target = resolve_remote_worker_target(&controller_state, &session_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(target.node_name, "worker-1");
        assert_eq!(target.base_url, "http://worker-1.tailnet:7433");
        assert_eq!(target.token.as_deref(), Some("secret-token"));
    }

    #[tokio::test]
    async fn test_input_for_session() {
        let state = test_state().await;
        let req = CreateSessionRequest {
            name: "input-test".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: None,
        };
        let (_, Json(resp)) = create(State(state.clone()), Json(req)).await.unwrap();
        let session = resp.session;

        let input_req = SendInputRequest {
            text: "hello".into(),
        };
        let result = input(State(state), Path(session.id.to_string()), Json(input_req)).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_input_not_found() {
        let state = test_state().await;
        let input_req = SendInputRequest {
            text: "hello".into(),
        };
        let result = input(State(state), Path("nonexistent".into()), Json(input_req)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_resolve_remote_worker_target_falls_back_to_index_address() {
        let session_id = Uuid::new_v4().to_string();
        let controller_state = controller_state_with_index(SessionIndexEntry {
            session_id: session_id.clone(),
            node_name: "worker-1".into(),
            node_address: Some("https://worker-1.example.com".into()),
            session_name: "remote-input".into(),
            status: "active".into(),
            command: Some("echo test".into()),
            updated_at: "2026-03-30T12:00:00Z".into(),
        })
        .await;

        let target = resolve_remote_worker_target(&controller_state, &session_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(target.base_url, "https://worker-1.example.com");
        assert!(target.token.is_none());
    }

    /// Backend where all methods except `create_session` return errors.
    struct FailingBackend;

    impl Backend for FailingBackend {
        fn create_session(&self, _: &str, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn kill_session(&self, _: &str) -> Result<()> {
            Err(anyhow::anyhow!("backend exploded"))
        }
        fn is_alive(&self, _: &str) -> Result<bool> {
            Err(anyhow::anyhow!("backend exploded"))
        }
        fn capture_output(&self, _: &str, _: usize) -> Result<String> {
            Err(anyhow::anyhow!("backend exploded"))
        }
        fn send_input(&self, _: &str, _: &str) -> Result<()> {
            Err(anyhow::anyhow!("backend exploded"))
        }
        fn setup_logging(&self, _: &str, _: &str) -> Result<()> {
            Err(anyhow::anyhow!("backend exploded"))
        }
    }

    /// Backend where only `create_session` fails.
    struct FailCreateBackend;

    impl Backend for FailCreateBackend {
        fn create_session(&self, _: &str, _: &str, _: &str) -> Result<()> {
            Err(anyhow::anyhow!("create failed"))
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

    /// Backend where `capture_output` and `send_input` fail.
    struct CaptureFailBackend;

    impl Backend for CaptureFailBackend {
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
            Err(anyhow::anyhow!("capture failed"))
        }
        fn send_input(&self, _: &str, _: &str) -> Result<()> {
            Err(anyhow::anyhow!("send failed"))
        }
        fn setup_logging(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
    }

    async fn failing_state() -> Arc<AppState> {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(FailingBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        AppState::new(
            Config {
                node: NodeConfig {
                    name: "test-node".into(),
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
            },
            manager,
            peer_registry,
            store,
        )
    }

    #[tokio::test]
    async fn test_get_internal_error() {
        let state = failing_state().await;
        // Create a session first (create_session succeeds on FailingBackend)
        let req = CreateSessionRequest {
            name: "err-test".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: None,
        };
        let (_, Json(resp)) = create(State(state.clone()), Json(req)).await.unwrap();
        let session = resp.session;

        // get() will find the Running session, call is_alive → Err → propagates as 500
        let result = get(State(state), Path(session.id.to_string())).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_stop_internal_error() {
        let state = failing_state().await;
        let req = CreateSessionRequest {
            name: "stop-err".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: None,
        };
        let (_, Json(resp)) = create(State(state.clone()), Json(req)).await.unwrap();
        let session = resp.session;

        // stop() finds session, calls backend.kill_session → Err("backend exploded")
        // Error message doesn't contain "not found" → 500
        let query = StopQuery { purge: None };
        let result = stop(State(state), Path(session.id.to_string()), Query(query)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_create_internal_error() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(FailCreateBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(
            Config {
                node: NodeConfig {
                    name: "test-node".into(),
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
            },
            manager,
            peer_registry,
            store,
        );

        let req = CreateSessionRequest {
            name: "fail".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: None,
        };
        let result = create(State(state), Json(req)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_output_internal_error() {
        let state = failing_state().await;
        let req = CreateSessionRequest {
            name: "out-err".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: None,
        };
        let (_, Json(resp)) = create(State(state.clone()), Json(req)).await.unwrap();
        let session = resp.session;

        // output() calls get_session (is_alive fails → Err) → 500
        let query = OutputQuery { lines: Some(50) };
        let result = output(State(state), Path(session.id.to_string()), Query(query)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_input_internal_error() {
        let state = failing_state().await;
        let req = CreateSessionRequest {
            name: "in-err".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: None,
        };
        let (_, Json(resp)) = create(State(state.clone()), Json(req)).await.unwrap();
        let session = resp.session;

        // input() calls get_session (is_alive fails → Err) → 500
        let input_req = SendInputRequest {
            text: "hello".into(),
        };
        let result = input(State(state), Path(session.id.to_string()), Json(input_req)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    async fn capture_fail_state() -> Arc<AppState> {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(CaptureFailBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        AppState::new(
            Config {
                node: NodeConfig {
                    name: "test-node".into(),
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
            },
            manager,
            peer_registry,
            store,
        )
    }

    #[tokio::test]
    async fn test_output_capture_fallback_to_log() {
        let state = capture_fail_state().await;
        let req = CreateSessionRequest {
            name: "cap-err".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: None,
        };
        let (_, Json(resp)) = create(State(state.clone()), Json(req)).await.unwrap();
        let session = resp.session;

        let query = OutputQuery { lines: Some(50) };
        // When capture fails, it falls back to the log file (empty since no log exists)
        let result = output(State(state), Path(session.id.to_string()), Query(query)).await;
        assert!(result.is_ok());
        let Json(val) = result.unwrap();
        assert_eq!(val["output"], "");
    }

    #[tokio::test]
    async fn test_input_send_error() {
        let state = capture_fail_state().await;
        let req = CreateSessionRequest {
            name: "send-err".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: None,
        };
        let (_, Json(resp)) = create(State(state.clone()), Json(req)).await.unwrap();
        let session = resp.session;

        let input_req = SendInputRequest {
            text: "hello".into(),
        };
        let result = input(State(state), Path(session.id.to_string()), Json(input_req)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_failing_backend_methods() {
        let b = FailingBackend;
        assert!(b.create_session("n", "d", "c").is_ok());
        assert!(b.kill_session("n").is_err());
        assert!(b.is_alive("n").is_err());
        assert!(b.capture_output("n", 10).is_err());
        assert!(b.send_input("n", "t").is_err());
        assert!(b.setup_logging("n", "p").is_err());
    }

    #[test]
    fn test_fail_create_backend_methods() {
        let b = FailCreateBackend;
        assert!(b.create_session("n", "d", "c").is_err());
        assert!(b.kill_session("n").is_ok());
        assert!(b.is_alive("n").unwrap());
        assert!(b.capture_output("n", 10).unwrap().is_empty());
        assert!(b.send_input("n", "t").is_ok());
        assert!(b.setup_logging("n", "p").is_ok());
    }

    #[test]
    fn test_capture_fail_backend_methods() {
        let b = CaptureFailBackend;
        assert!(b.create_session("n", "d", "c").is_ok());
        assert!(b.kill_session("n").is_ok());
        assert!(b.is_alive("n").unwrap());
        assert!(b.capture_output("n", 10).is_err());
        assert!(b.send_input("n", "t").is_err());
        assert!(b.setup_logging("n", "p").is_ok());
    }

    #[tokio::test]
    async fn test_list_internal_error() {
        let (state, pool) = test_state_with_pool().await;
        sqlx::query("DROP TABLE sessions")
            .execute(&pool)
            .await
            .unwrap();
        let result = list(State(state), Query(ListSessionsQuery::default())).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_list_filtered_internal_error() {
        let (state, pool) = test_state_with_pool().await;
        sqlx::query("DROP TABLE sessions")
            .execute(&pool)
            .await
            .unwrap();
        let query = ListSessionsQuery {
            status: Some("active".into()),
            ..Default::default()
        };
        let result = list(State(state), Query(query)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_download_output_running_session() {
        let state = test_state().await;
        let req = CreateSessionRequest {
            name: "dl-test".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: None,
        };
        let (_, Json(resp)) = create(State(state.clone()), Json(req)).await.unwrap();
        let session = resp.session;

        let result = download_output(State(state), Path(session.id.to_string())).await;
        assert!(result.is_ok());
        let (status, headers, body) = result.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "test output");
        assert_eq!(headers[0].1, "text/plain; charset=utf-8");
        assert!(headers[1].1.contains("dl-test.log"));
    }

    #[tokio::test]
    async fn test_download_output_dead_session_with_snapshot() {
        let state = test_state().await;
        let id = uuid::Uuid::new_v4();
        let now = chrono::Utc::now();
        let session = Session {
            id,
            name: "snap-test".into(),
            workdir: "/tmp".into(),
            command: "echo test".into(),
            status: SessionStatus::Stopped,
            output_snapshot: Some("saved output from snapshot".into()),
            created_at: now,
            updated_at: now,
            ..Default::default()
        };
        state
            .session_manager
            .store()
            .insert_session(&session)
            .await
            .unwrap();

        let result = download_output(State(state), Path(id.to_string())).await;
        assert!(result.is_ok());
        let (_, headers, body) = result.unwrap();
        assert_eq!(body, "saved output from snapshot");
        assert!(headers[1].1.contains("snap-test.log"));
    }

    #[tokio::test]
    async fn test_download_output_dead_session_no_snapshot() {
        let state = test_state().await;
        let req = CreateSessionRequest {
            name: "no-snap".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: None,
        };
        let (_, Json(resp)) = create(State(state.clone()), Json(req)).await.unwrap();
        let session = resp.session;

        // Stop the session so it becomes Dead
        let query = StopQuery { purge: None };
        let _ = stop(
            State(state.clone()),
            Path(session.id.to_string()),
            Query(query),
        )
        .await;

        let result = download_output(State(state), Path(session.id.to_string())).await;
        assert!(result.is_ok());
        let (_, _, body) = result.unwrap();
        assert!(body.is_empty());
    }

    #[tokio::test]
    async fn test_download_output_not_found() {
        let state = test_state().await;
        let result = download_output(State(state), Path("nonexistent".into())).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_resolve_remote_worker_target_errors_without_any_address() {
        let session_id = Uuid::new_v4().to_string();
        let controller_state = controller_state_with_index(SessionIndexEntry {
            session_id: session_id.clone(),
            node_name: "worker-1".into(),
            node_address: None,
            session_name: "remote-download".into(),
            status: "active".into(),
            command: Some("echo test".into()),
            updated_at: "2026-03-30T12:00:00Z".into(),
        })
        .await;

        let result = resolve_remote_worker_target(&controller_state, &session_id).await;
        assert!(result.is_err());
        let (status, Json(err)) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(err.error.contains("node address unknown"));
    }

    #[tokio::test]
    async fn test_output_remote_worker_connection_failure_returns_bad_gateway() {
        let session_id = Uuid::new_v4().to_string();
        let state = controller_state_with_index(SessionIndexEntry {
            session_id: session_id.clone(),
            node_name: "worker-1".into(),
            node_address: Some("127.0.0.1:9".into()),
            session_name: "remote-output".into(),
            status: "active".into(),
            command: Some("echo test".into()),
            updated_at: "2026-03-30T12:00:00Z".into(),
        })
        .await;

        let result = output(
            State(state),
            Path(session_id),
            Query(OutputQuery { lines: Some(50) }),
        )
        .await;
        assert!(result.is_err());
        let (status, Json(err)) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(
            err.error
                .contains("failed to fetch output from node worker-1")
        );
    }

    #[tokio::test]
    async fn test_input_remote_worker_connection_failure_returns_bad_gateway() {
        let session_id = Uuid::new_v4().to_string();
        let state = controller_state_with_index(SessionIndexEntry {
            session_id: session_id.clone(),
            node_name: "worker-1".into(),
            node_address: Some("127.0.0.1:9".into()),
            session_name: "remote-input".into(),
            status: "active".into(),
            command: Some("echo test".into()),
            updated_at: "2026-03-30T12:00:00Z".into(),
        })
        .await;

        let result = input(
            State(state),
            Path(session_id),
            Json(SendInputRequest {
                text: "continue".into(),
            }),
        )
        .await;
        assert!(result.is_err());
        let (status, Json(err)) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(err.error.contains("failed to send input to node worker-1"));
    }

    #[tokio::test]
    async fn test_download_output_remote_worker_connection_failure_returns_bad_gateway() {
        let session_id = Uuid::new_v4().to_string();
        let state = controller_state_with_index(SessionIndexEntry {
            session_id: session_id.clone(),
            node_name: "worker-1".into(),
            node_address: Some("127.0.0.1:9".into()),
            session_name: "remote-download".into(),
            status: "active".into(),
            command: Some("echo test".into()),
            updated_at: "2026-03-30T12:00:00Z".into(),
        })
        .await;

        let result = download_output(State(state), Path(session_id)).await;
        assert!(result.is_err());
        let (status, Json(err)) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(
            err.error
                .contains("failed to download output from node worker-1")
        );
    }

    #[tokio::test]
    async fn test_resume_remote_worker_connection_failure_returns_bad_gateway() {
        let session_id = Uuid::new_v4().to_string();
        let state = controller_state_with_index(SessionIndexEntry {
            session_id: session_id.clone(),
            node_name: "worker-1".into(),
            node_address: Some("127.0.0.1:9".into()),
            session_name: "remote-resume".into(),
            status: "lost".into(),
            command: Some("echo test".into()),
            updated_at: "2026-03-30T12:00:00Z".into(),
        })
        .await;

        let result = resume(State(state), Path(session_id)).await;
        assert!(result.is_err());
        let (status, Json(err)) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(
            err.error
                .contains("failed to resume session on node worker-1")
        );
    }

    #[tokio::test]
    async fn test_download_output_internal_error() {
        let state = failing_state().await;
        let req = CreateSessionRequest {
            name: "dl-err".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: None,
        };
        let (_, Json(resp)) = create(State(state.clone()), Json(req)).await.unwrap();
        let session = resp.session;

        // download_output calls get_session → is_alive fails → 500
        let result = download_output(State(state), Path(session.id.to_string())).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_list_interventions_empty() {
        let state = test_state().await;
        let req = CreateSessionRequest {
            name: "int-empty".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: None,
        };
        let (_, Json(resp)) = create(State(state.clone()), Json(req)).await.unwrap();
        let session = resp.session;
        let Json(events) = list_interventions(State(state), Path(session.id.to_string()))
            .await
            .unwrap();
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn test_list_interventions_with_events() {
        let state = test_state().await;
        let req = CreateSessionRequest {
            name: "int-events".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: None,
        };
        let (_, Json(resp)) = create(State(state.clone()), Json(req)).await.unwrap();
        let session = resp.session;

        // Insert intervention events via store
        state
            .session_manager
            .store()
            .update_session_intervention(
                &session.id.to_string(),
                pulpo_common::session::InterventionCode::MemoryPressure,
                "Memory 95%",
            )
            .await
            .unwrap();

        let Json(events) = list_interventions(State(state), Path(session.id.to_string()))
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].reason, "Memory 95%");
        assert_eq!(events[0].session_id, session.id.to_string());
    }

    #[tokio::test]
    async fn test_list_interventions_store_error() {
        let (state, pool) = test_state_with_pool().await;
        sqlx::query("DROP TABLE intervention_events")
            .execute(&pool)
            .await
            .unwrap();
        let result = list_interventions(State(state), Path("some-id".into())).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_internal_error_helper() {
        let (status, Json(err)) = internal_error("boom");
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(err.error, "boom");
    }

    #[tokio::test]
    async fn test_resume_not_found() {
        let state = test_state().await;
        let result = resume(State(state), Path("nonexistent".into())).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_resume_not_stale() {
        let state = test_state().await;
        let req = CreateSessionRequest {
            name: "resume-test".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: None,
        };
        let (_, Json(resp)) = create(State(state.clone()), Json(req)).await.unwrap();
        let session = resp.session;

        // Session is Running, not Stale
        let result = resume(State(state), Path(session.id.to_string())).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    /// Backend that marks sessions as dead (`is_alive`=false) but allows create
    struct StaleBackend;

    impl Backend for StaleBackend {
        fn create_session(&self, _: &str, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn kill_session(&self, _: &str) -> Result<()> {
            Ok(())
        }
        fn is_alive(&self, _: &str) -> Result<bool> {
            Ok(false)
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
    fn test_stale_backend_methods() {
        let b = StaleBackend;
        assert!(b.create_session("n", "d", "c").is_ok());
        assert!(b.kill_session("n").is_ok());
        assert!(!b.is_alive("n").unwrap());
        assert!(b.capture_output("n", 10).unwrap().is_empty());
        assert!(b.send_input("n", "t").is_ok());
        assert!(b.setup_logging("n", "p").is_ok());
    }

    #[tokio::test]
    async fn test_resume_stale_session() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(StaleBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(
            Config {
                node: NodeConfig {
                    name: "test-node".into(),
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
            },
            manager,
            peer_registry,
            store,
        );

        // Create a session (StaleBackend.is_alive returns false)
        let req = CreateSessionRequest {
            name: "stale-test".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: None,
        };
        let (_, Json(resp)) = create(State(state.clone()), Json(req)).await.unwrap();
        let session = resp.session;

        // Get session to trigger stale detection
        let _ = get(State(state.clone()), Path(session.id.to_string())).await;

        // Now resume
        let result = resume(State(state), Path(session.id.to_string())).await;
        assert!(result.is_ok());
        let Json(resumed) = result.unwrap();
        assert_eq!(resumed.status, pulpo_common::session::SessionStatus::Active);
    }

    #[tokio::test]
    async fn test_resume_name_collision_returns_conflict() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let pool = store.pool().clone();
        let backend = Arc::new(StaleBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(
            Config {
                node: NodeConfig {
                    name: "test-node".into(),
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
            },
            manager,
            peer_registry,
            store,
        );

        // Create first session, mark it lost
        let req = CreateSessionRequest {
            name: "dup".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: None,
        };
        let (_, Json(resp)) = create(State(state.clone()), Json(req)).await.unwrap();
        let old_id = resp.session.id.to_string();
        sqlx::query("UPDATE sessions SET status = 'lost' WHERE id = ?")
            .bind(&old_id)
            .execute(&pool)
            .await
            .unwrap();

        // Create second active "dup"
        let req2 = CreateSessionRequest {
            name: "dup".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: None,
        };
        let _ = create(State(state.clone()), Json(req2)).await.unwrap();

        // Resume the lost one — should get 409
        let result = resume(State(state), Path(old_id)).await;
        assert!(result.is_err());
        let (status, Json(body)) = result.unwrap_err();
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(body.error.contains("already active"));
    }

    /// Backend that makes sessions stale and then fails on create (for resume internal error).
    struct ResumeFailBackend {
        created: std::sync::Mutex<bool>,
    }

    impl Backend for ResumeFailBackend {
        fn create_session(&self, _: &str, _: &str, _: &str) -> Result<()> {
            let mut created = self.created.lock().unwrap();
            if *created {
                // Second call (resume) fails
                return Err(anyhow::anyhow!("backend error"));
            }
            *created = true;
            drop(created);
            Ok(())
        }
        fn kill_session(&self, _: &str) -> Result<()> {
            Ok(())
        }
        fn is_alive(&self, _: &str) -> Result<bool> {
            Ok(false)
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
    fn test_resume_fail_backend_methods() {
        let b = ResumeFailBackend {
            created: std::sync::Mutex::new(false),
        };
        assert!(b.create_session("n", "d", "c").is_ok());
        assert!(b.create_session("n", "d", "c").is_err()); // second call fails
        assert!(b.kill_session("n").is_ok());
        assert!(!b.is_alive("n").unwrap());
        assert!(b.capture_output("n", 10).unwrap().is_empty());
        assert!(b.send_input("n", "t").is_ok());
        assert!(b.setup_logging("n", "p").is_ok());
    }

    #[tokio::test]
    async fn test_resume_internal_error() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(ResumeFailBackend {
            created: std::sync::Mutex::new(false),
        });
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(
            Config {
                node: NodeConfig {
                    name: "test-node".into(),
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
            },
            manager,
            peer_registry,
            store,
        );

        // Create a session
        let req = CreateSessionRequest {
            name: "resume-fail".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            target_node: None,
        };
        let (_, Json(resp)) = create(State(state.clone()), Json(req)).await.unwrap();
        let session = resp.session;

        // Mark as stale via get
        let _ = get(State(state.clone()), Path(session.id.to_string())).await;

        // Resume should fail with internal error (backend create fails)
        let result = resume(State(state), Path(session.id.to_string())).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_stop_purge_not_found() {
        let state = test_state().await;
        let query = StopQuery { purge: Some(true) };
        let result = stop(State(state), Path("nonexistent".into()), Query(query)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
