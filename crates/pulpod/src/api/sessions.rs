use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use pulpo_common::api::{
    CleanupResponse, CreateSessionRequest, CreateSessionResponse, HandoffSessionRequest,
    ListSessionsQuery, OutputQuery, SendInputRequest,
};
use pulpo_common::session::{Session, SessionStatus};
use serde::Deserialize;

use crate::api::error::{ApiError, internal_error, map_manager_err, not_found};

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
        Ok(None) => Err(not_found(&format!("session not found: {id}"))),
        Err(e) => Err(internal_error(&e.to_string())),
    }
}

pub async fn create(
    State(state): State<Arc<super::AppState>>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<CreateSessionResponse>), ApiError> {
    let session = state
        .session_manager
        .create_session(req)
        .await
        .map_err(|e| map_manager_err(&e))?;
    Ok((StatusCode::CREATED, Json(CreateSessionResponse { session })))
}

/// `POST /api/v1/sessions/{id}/handoff` — spawn a new session that inherits the
/// source session's working context (directory, and git worktree if it has one).
/// `id` resolves by ID or name, same as [`get`].
pub async fn handoff(
    State(state): State<Arc<super::AppState>>,
    Path(id): Path<String>,
    Json(req): Json<HandoffSessionRequest>,
) -> Result<(StatusCode, Json<CreateSessionResponse>), ApiError> {
    let session = state
        .session_manager
        .handoff_session(&id, req)
        .await
        .map_err(|e| map_manager_err(&e))?;
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
        Err(e) => Err(map_manager_err(&e)),
    }
}

pub async fn cleanup(
    State(state): State<Arc<super::AppState>>,
) -> Result<Json<CleanupResponse>, ApiError> {
    let response = state
        .session_manager
        .cleanup_dead_sessions()
        .await
        .map_err(|e| internal_error(&e.to_string()))?;
    Ok(Json(response))
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
        return Err(not_found(&format!("session not found: {id}")));
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
        Err(e) => Err(map_manager_err(&e)),
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
        return Err(not_found(&format!("session not found: {id}")));
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
        return Err(not_found(&format!("session not found: {id}")));
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
