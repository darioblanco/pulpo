use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use norn_common::api::{CreateSessionRequest, ErrorResponse};
use norn_common::session::Session;

pub async fn list(State(_state): State<Arc<super::AppState>>) -> Json<Vec<Session>> {
    // TODO: query sessions from store
    Json(vec![])
}

pub async fn get(
    State(_state): State<Arc<super::AppState>>,
    Path(_id): Path<String>,
) -> Result<Json<Session>, (StatusCode, Json<ErrorResponse>)> {
    Err((
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "Not found".into(),
        }),
    ))
}

pub async fn create(
    State(_state): State<Arc<super::AppState>>,
    Json(_req): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<Session>), (StatusCode, Json<ErrorResponse>)> {
    Err((
        StatusCode::NOT_IMPLEMENTED,
        Json(ErrorResponse {
            error: "Not yet implemented".into(),
        }),
    ))
}

pub async fn kill(
    State(_state): State<Arc<super::AppState>>,
    Path(_id): Path<String>,
) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}
