use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use pulpo_common::api::{
    CleanupResponse, CreateSessionRequest, CreateSessionResponse, HandoffSessionRequest,
    HarnessEventRequest, ListSessionsQuery, OutputQuery, SendInputRequest,
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

/// `DELETE /api/v1/sessions/{id}` — remove a session outright (`pulpo rm`).
///
/// Only sessions not currently `Active`/`Idle` may be removed (409 otherwise);
/// removal purges the session row, its intervention events, exit markers, session
/// log, and harness dir — see `SessionManager::remove_session`.
pub async fn remove(
    State(state): State<Arc<super::AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    match state.session_manager.remove_session(&id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => Err(map_manager_err(&e)),
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

/// `POST /api/v1/sessions/{id}/stop`. `200 OK` means the session was already
/// `done`/`lost` (a no-op — see `SessionManager::stop_session`); `204 No Content`
/// means this call is what stopped it. Both are success; the CLI uses the
/// distinction only to word its own message ("already done" vs. "stopped").
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
        Ok(true) => Ok(StatusCode::OK),
        Ok(false) => Ok(StatusCode::NO_CONTENT),
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
    // A `done` session's pane is guaranteed gone by the time it gets here (ADR
    // 0009 — no more lingering fallback shell keeping it open), so a live tmux
    // capture always comes back empty; worse, the tmux backend's own
    // `capture_output` swallows `tmux capture-pane` failing (no such session) and
    // returns `Ok("")` rather than an `Err`, so `SessionManager::capture_output`'s
    // own log-tail fallback never even triggers. `resolve_dead_backend_session`
    // already computed and persisted the best available final output (a live
    // capture attempt made at resolution time, then the pipe-pane log) into
    // `output_snapshot` — use that directly instead of re-deriving it here.
    let output = if session.status == SessionStatus::Done {
        session.output_snapshot.clone().unwrap_or_default()
    } else {
        let backend_id = state.session_manager.resolve_backend_id(&session);
        // `session.id` (the UUID), not the path's `id` — a request addressed by
        // *name* (as every `pulpo logs <name>` call is) would otherwise look up
        // the pipe-pane log fallback under `logs/<name>.log`, which never exists
        // (the file is always named by UUID — see
        // `SessionManager::create_session`).
        state
            .session_manager
            .capture_output(&session.id.to_string(), &backend_id, lines)
    };

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

    let output =
        if session.status == SessionStatus::Working || session.status == SessionStatus::Lost {
            let backend_id = state.session_manager.resolve_backend_id(&session);
            // See the `output` handler above for why this must be `session.id`,
            // not the path's `id` (which may be a name).
            state
                .session_manager
                .capture_output(&session.id.to_string(), &backend_id, 10_000)
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

/// `POST /api/v1/sessions/{id}/harness-events` — ingest a harness lifecycle event.
///
/// Posted by `pulpo hook <harness>`. Resolves the session's adapter, applies the
/// resulting state transition, and emits the existing SSE `session` event (which
/// already carries notifications through the existing webhook path — no new
/// channel is added here).
pub async fn harness_events(
    State(state): State<Arc<super::AppState>>,
    Path(id): Path<String>,
    Json(req): Json<HarnessEventRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .session_manager
        .apply_harness_event(&id, &req.harness, &req.event)
        .await
        .map_err(|e| map_manager_err(&e))?;
    Ok(StatusCode::NO_CONTENT)
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
