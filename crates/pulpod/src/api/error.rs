//! Shared API handler error type and helpers.

use axum::Json;
use axum::http::StatusCode;
use pulpo_common::api::ErrorResponse;

pub(super) type ApiError = (StatusCode, Json<ErrorResponse>);

fn error_response(status: StatusCode, msg: &str) -> ApiError {
    (
        status,
        Json(ErrorResponse {
            error: msg.to_owned(),
        }),
    )
}

pub(super) fn internal_error(msg: &str) -> ApiError {
    error_response(StatusCode::INTERNAL_SERVER_ERROR, msg)
}

pub(super) fn bad_request(msg: &str) -> ApiError {
    error_response(StatusCode::BAD_REQUEST, msg)
}

pub(super) fn not_found(msg: &str) -> ApiError {
    error_response(StatusCode::NOT_FOUND, msg)
}

pub(super) fn conflict(msg: &str) -> ApiError {
    error_response(StatusCode::CONFLICT, msg)
}

/// Used by `POST /api/v1/push/action` when the caller's action token fails
/// signature/expiry/action verification. Deliberately generic — never echoes
/// *why* the token was rejected, to avoid giving an attacker an oracle.
pub(super) fn unauthorized(msg: &str) -> ApiError {
    error_response(StatusCode::UNAUTHORIZED, msg)
}

/// Used by `POST /api/v1/push/action` when the token itself is valid (and not
/// expired) but its target session no longer exists — distinct from a plain
/// 404 because the *token* was valid, only its target is gone (e.g. the
/// session was already purged after the daemon's own auto-stop fired).
pub(super) fn gone(msg: &str) -> ApiError {
    error_response(StatusCode::GONE, msg)
}

/// Map a `SessionManager` lifecycle error (create/stop/resume) to the right status
/// code by matching the well-known substrings baked into its `anyhow` error
/// messages. The message text is always passed through unchanged.
///
/// Substring table (checked in order):
/// - "not found" → 404
/// - "already active" → 409
/// - "cannot be resumed" → 400
/// - "docker runtime was removed" → 400
/// - "worktree no longer exists" (handoff, source worktree missing on disk) → 400
/// - anything else → 500
pub(super) fn map_manager_err(e: &anyhow::Error) -> ApiError {
    let msg = e.to_string();
    if msg.contains("not found") {
        not_found(&msg)
    } else if msg.contains("already active") {
        conflict(&msg)
    } else if msg.contains("cannot be resumed")
        || msg.contains("docker runtime was removed")
        || msg.contains("worktree no longer exists")
    {
        bad_request(&msg)
    } else {
        internal_error(&msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_internal_error() {
        let (status, Json(err)) = internal_error("boom");
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(err.error, "boom");
    }

    #[test]
    fn test_bad_request() {
        let (status, Json(err)) = bad_request("bad");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(err.error, "bad");
    }

    #[test]
    fn test_not_found() {
        let (status, Json(err)) = not_found("missing");
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(err.error, "missing");
    }

    #[test]
    fn test_conflict() {
        let (status, Json(err)) = conflict("taken");
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(err.error, "taken");
    }

    #[test]
    fn test_unauthorized() {
        let (status, Json(err)) = unauthorized("invalid or expired action token");
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(err.error, "invalid or expired action token");
    }

    #[test]
    fn test_gone() {
        let (status, Json(err)) = gone("session not found: abc");
        assert_eq!(status, StatusCode::GONE);
        assert_eq!(err.error, "session not found: abc");
    }

    #[test]
    fn test_map_manager_err_not_found() {
        let e = anyhow::anyhow!("session not found: abc");
        let (status, Json(err)) = map_manager_err(&e);
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(err.error, "session not found: abc");
    }

    #[test]
    fn test_map_manager_err_already_active() {
        let e = anyhow::anyhow!("a session named 'x' is already active — stop it first");
        let (status, _) = map_manager_err(&e);
        assert_eq!(status, StatusCode::CONFLICT);
    }

    #[test]
    fn test_map_manager_err_cannot_be_resumed() {
        let e = anyhow::anyhow!("session cannot be resumed (status: active)");
        let (status, _) = map_manager_err(&e);
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_map_manager_err_docker_runtime_removed() {
        let e = anyhow::anyhow!(crate::session::utils::DOCKER_RUNTIME_REMOVED);
        let (status, _) = map_manager_err(&e);
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_map_manager_err_worktree_missing() {
        let e = anyhow::anyhow!(
            "source session's worktree no longer exists on disk: /tmp/x — cannot hand off"
        );
        let (status, _) = map_manager_err(&e);
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_map_manager_err_fallback_internal_error() {
        let e = anyhow::anyhow!("something unexpected exploded");
        let (status, Json(err)) = map_manager_err(&e);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(err.error, "something unexpected exploded");
    }
}
