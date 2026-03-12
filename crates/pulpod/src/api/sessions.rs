use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use pulpo_common::api::{
    CreateSessionRequest, CreateSessionResponse, ErrorResponse, ListSessionsQuery, OutputQuery,
    SendInputRequest,
};
use pulpo_common::session::{Session, SessionStatus};

type ApiError = (StatusCode, Json<ErrorResponse>);

fn internal_error(msg: &str) -> ApiError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: msg.to_owned(),
        }),
    )
}

pub async fn list(
    State(state): State<Arc<super::AppState>>,
    Query(query): Query<ListSessionsQuery>,
) -> Result<Json<Vec<Session>>, ApiError> {
    let has_filters = query.status.is_some()
        || query.provider.is_some()
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
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("session not found: {id}"),
            }),
        )),
        Err(e) => Err(internal_error(&e.to_string())),
    }
}

pub async fn create(
    State(state): State<Arc<super::AppState>>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<CreateSessionResponse>), ApiError> {
    let (session, warnings) = state
        .session_manager
        .create_session(req)
        .await
        .map_err(|e| internal_error(&e.to_string()))?;
    Ok((
        StatusCode::CREATED,
        Json(CreateSessionResponse { session, warnings }),
    ))
}

pub async fn kill(
    State(state): State<Arc<super::AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.session_manager.kill_session(&id).await.map_err(|e| {
        let msg = e.to_string();
        if msg.contains("not found") {
            (StatusCode::NOT_FOUND, Json(ErrorResponse { error: msg }))
        } else {
            internal_error(&msg)
        }
    })?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete(
    State(state): State<Arc<super::AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .session_manager
        .delete_session(&id)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("not found") {
                (StatusCode::NOT_FOUND, Json(ErrorResponse { error: msg }))
            } else if msg.contains("cannot delete") {
                (StatusCode::CONFLICT, Json(ErrorResponse { error: msg }))
            } else {
                internal_error(&msg)
            }
        })?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn output(
    State(state): State<Arc<super::AppState>>,
    Path(id): Path<String>,
    Query(query): Query<OutputQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = state
        .session_manager
        .get_session(&id)
        .await
        .map_err(|e| internal_error(&e.to_string()))?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("session not found: {id}"),
                }),
            )
        })?;

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
    state
        .session_manager
        .resume_session(&id)
        .await
        .map(Json)
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("not found") {
                (StatusCode::NOT_FOUND, Json(ErrorResponse { error: msg }))
            } else if msg.contains("not stale") {
                (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: msg }))
            } else {
                internal_error(&msg)
            }
        })
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
    let session = state
        .session_manager
        .get_session(&id)
        .await
        .map_err(|e| internal_error(&e.to_string()))?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("session not found: {id}"),
                }),
            )
        })?;

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
    let session = state
        .session_manager
        .get_session(&id)
        .await
        .map_err(|e| internal_error(&e.to_string()))?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("session not found: {id}"),
                }),
            )
        })?;

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
    use std::collections::HashMap;

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
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
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
                guards: crate::config::GuardDefaultConfig::default(),
                session_defaults: crate::config::SessionDefaultsConfig::default(),
                watchdog: crate::config::WatchdogConfig::default(),
                inks: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
                culture: crate::config::CultureConfig::default(),
            },
            manager,
            peer_registry,
        );
        (state, pool)
    }

    async fn test_state() -> Arc<AppState> {
        let (state, _) = test_state_with_pool().await;
        state
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
            name: Some("filter-test".into()),
            workdir: Some("/tmp".into()),
            provider: None,
            prompt: Some("test".into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
        };
        let _ = create(State(state.clone()), Json(req)).await.unwrap();

        let query = ListSessionsQuery {
            status: Some("active".into()),
            ..Default::default()
        };
        let Json(sessions) = list(State(state.clone()), Query(query)).await.unwrap();
        assert_eq!(sessions.len(), 1);

        let query = ListSessionsQuery {
            status: Some("finished".into()),
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
    async fn test_create_returns_created() {
        let state = test_state().await;
        let req = CreateSessionRequest {
            name: Some("test".into()),
            workdir: Some("/tmp".into()),
            provider: None,
            prompt: Some("Do something".into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
        };
        let result = create(State(state), Json(req)).await;
        assert!(result.is_ok());
        let (status, Json(resp)) = result.unwrap();
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(resp.session.name, "test");
    }

    #[tokio::test]
    async fn test_kill_not_found() {
        let state = test_state().await;
        let result = kill(State(state), Path("nonexistent".into())).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_kill_returns_no_content() {
        let state = test_state().await;
        let req = CreateSessionRequest {
            name: Some("kill-test".into()),
            workdir: Some("/tmp".into()),
            provider: None,
            prompt: Some("test".into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
        };
        let (_, Json(resp)) = create(State(state.clone()), Json(req)).await.unwrap();
        let session = resp.session;
        let result = kill(State(state), Path(session.id.to_string())).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_output_for_session() {
        let state = test_state().await;
        let req = CreateSessionRequest {
            name: Some("output-test".into()),
            workdir: Some("/tmp".into()),
            provider: None,
            prompt: Some("test".into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
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
    async fn test_input_for_session() {
        let state = test_state().await;
        let req = CreateSessionRequest {
            name: Some("input-test".into()),
            workdir: Some("/tmp".into()),
            provider: None,
            prompt: Some("test".into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
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
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
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
                guards: crate::config::GuardDefaultConfig::default(),
                session_defaults: crate::config::SessionDefaultsConfig::default(),
                watchdog: crate::config::WatchdogConfig::default(),
                inks: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
                culture: crate::config::CultureConfig::default(),
            },
            manager,
            peer_registry,
        )
    }

    #[tokio::test]
    async fn test_get_internal_error() {
        let state = failing_state().await;
        // Create a session first (create_session succeeds on FailingBackend)
        let req = CreateSessionRequest {
            name: Some("err-test".into()),
            workdir: Some("/tmp".into()),
            provider: None,
            prompt: Some("test".into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
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
    async fn test_kill_internal_error() {
        let state = failing_state().await;
        let req = CreateSessionRequest {
            name: Some("kill-err".into()),
            workdir: Some("/tmp".into()),
            provider: None,
            prompt: Some("test".into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
        };
        let (_, Json(resp)) = create(State(state.clone()), Json(req)).await.unwrap();
        let session = resp.session;

        // kill() finds session, calls backend.kill_session → Err("backend exploded")
        // Error message doesn't contain "not found" → 500
        let result = kill(State(state), Path(session.id.to_string())).await;
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
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
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
                guards: crate::config::GuardDefaultConfig::default(),
                session_defaults: crate::config::SessionDefaultsConfig::default(),
                watchdog: crate::config::WatchdogConfig::default(),
                inks: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
                culture: crate::config::CultureConfig::default(),
            },
            manager,
            peer_registry,
        );

        let req = CreateSessionRequest {
            name: Some("fail".into()),
            workdir: Some("/tmp".into()),
            provider: None,
            prompt: Some("test".into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
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
            name: Some("out-err".into()),
            workdir: Some("/tmp".into()),
            provider: None,
            prompt: Some("test".into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
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
            name: Some("in-err".into()),
            workdir: Some("/tmp".into()),
            provider: None,
            prompt: Some("test".into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
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
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
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
                guards: crate::config::GuardDefaultConfig::default(),
                session_defaults: crate::config::SessionDefaultsConfig::default(),
                watchdog: crate::config::WatchdogConfig::default(),
                inks: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
                culture: crate::config::CultureConfig::default(),
            },
            manager,
            peer_registry,
        )
    }

    #[tokio::test]
    async fn test_output_capture_fallback_to_log() {
        let state = capture_fail_state().await;
        let req = CreateSessionRequest {
            name: Some("cap-err".into()),
            workdir: Some("/tmp".into()),
            provider: None,
            prompt: Some("test".into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
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
            name: Some("send-err".into()),
            workdir: Some("/tmp".into()),
            provider: None,
            prompt: Some("test".into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
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
            name: Some("dl-test".into()),
            workdir: Some("/tmp".into()),
            provider: None,
            prompt: Some("test".into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
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
            provider: pulpo_common::session::Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Killed,
            mode: pulpo_common::session::SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            backend_session_id: None,
            output_snapshot: Some("saved output from snapshot".into()),
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_code: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: now,
            updated_at: now,
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
            name: Some("no-snap".into()),
            workdir: Some("/tmp".into()),
            provider: None,
            prompt: Some("test".into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
        };
        let (_, Json(resp)) = create(State(state.clone()), Json(req)).await.unwrap();
        let session = resp.session;

        // Kill the session so it becomes Dead
        let _ = kill(State(state.clone()), Path(session.id.to_string())).await;

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
    async fn test_download_output_internal_error() {
        let state = failing_state().await;
        let req = CreateSessionRequest {
            name: Some("dl-err".into()),
            workdir: Some("/tmp".into()),
            provider: None,
            prompt: Some("test".into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
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
            name: Some("int-empty".into()),
            workdir: Some("/tmp".into()),
            provider: None,
            prompt: Some("test".into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
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
            name: Some("int-events".into()),
            workdir: Some("/tmp".into()),
            provider: None,
            prompt: Some("test".into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
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
            name: Some("resume-test".into()),
            workdir: Some("/tmp".into()),
            provider: None,
            prompt: Some("test".into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
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
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
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
                guards: crate::config::GuardDefaultConfig::default(),
                session_defaults: crate::config::SessionDefaultsConfig::default(),
                watchdog: crate::config::WatchdogConfig::default(),
                inks: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
                culture: crate::config::CultureConfig::default(),
            },
            manager,
            peer_registry,
        );

        // Create a session (StaleBackend.is_alive returns false)
        let req = CreateSessionRequest {
            name: Some("stale-test".into()),
            workdir: Some("/tmp".into()),
            provider: None,
            prompt: Some("test".into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
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
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
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
                guards: crate::config::GuardDefaultConfig::default(),
                session_defaults: crate::config::SessionDefaultsConfig::default(),
                watchdog: crate::config::WatchdogConfig::default(),
                inks: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
                culture: crate::config::CultureConfig::default(),
            },
            manager,
            peer_registry,
        );

        // Create a session
        let req = CreateSessionRequest {
            name: Some("resume-fail".into()),
            workdir: Some("/tmp".into()),
            provider: None,
            prompt: Some("test".into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
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
    async fn test_delete_dead_session() {
        let state = test_state().await;
        let req = CreateSessionRequest {
            name: Some("del-test".into()),
            workdir: Some("/tmp".into()),
            provider: None,
            prompt: Some("test".into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
        };
        let (_, Json(resp)) = create(State(state.clone()), Json(req)).await.unwrap();
        let session = resp.session;

        // Kill first, then delete
        let _ = kill(State(state.clone()), Path(session.id.to_string())).await;
        let result = delete(State(state), Path(session.id.to_string())).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_delete_running_session_rejected() {
        let state = test_state().await;
        let req = CreateSessionRequest {
            name: Some("del-run".into()),
            workdir: Some("/tmp".into()),
            provider: None,
            prompt: Some("test".into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
        };
        let (_, Json(resp)) = create(State(state.clone()), Json(req)).await.unwrap();
        let session = resp.session;

        let result = delete(State(state), Path(session.id.to_string())).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_delete_not_found() {
        let state = test_state().await;
        let result = delete(State(state), Path("nonexistent".into())).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_delete_internal_error() {
        let (state, pool) = test_state_with_pool().await;
        sqlx::query("DROP TABLE sessions")
            .execute(&pool)
            .await
            .unwrap();
        let result = delete(State(state), Path("test".into())).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }
}
