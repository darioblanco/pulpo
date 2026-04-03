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
use pulpo_common::session::{Session, SessionStatus};
use serde::Deserialize;
use uuid::Uuid;

use crate::api::session_remote::{
    ApiError, expect_remote_no_content, internal_error, parse_remote_json, remote_json_request,
    reqwest_error_response, resolve_remote_node_target, resolve_remote_worker_target,
    send_remote_request,
};
use crate::remote::{apply_remote_auth, remote_client};

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
#[path = "sessions_tests.rs"]
mod sessions_tests;
