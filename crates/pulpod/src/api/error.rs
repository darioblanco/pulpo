//! Shared API handler error type and helpers.

use axum::Json;
use axum::http::StatusCode;
use pulpo_common::api::ErrorResponse;

pub(super) type ApiError = (StatusCode, Json<ErrorResponse>);

pub(super) fn internal_error(msg: &str) -> ApiError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: msg.to_owned(),
        }),
    )
}
