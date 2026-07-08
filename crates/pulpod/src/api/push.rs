use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode};
use chrono::Utc;
use pulpo_common::api::{
    PushActionRequest, PushActionResponse, PushSubscriptionRequest, PushUnsubscribeRequest,
    VapidPublicKeyResponse,
};

use super::AppState;
use crate::api::error::{ApiError, gone, internal_error, map_manager_err, unauthorized};
use crate::notifications::action_token::{self, STOP_ACTION};

pub async fn get_vapid_key(
    State(state): State<Arc<AppState>>,
) -> Result<Json<VapidPublicKeyResponse>, ApiError> {
    let public_key = state
        .config
        .read()
        .await
        .notifications
        .vapid
        .public_key
        .clone();
    if public_key.is_empty() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(pulpo_common::api::ErrorResponse {
                error: "VAPID keys not configured".into(),
            }),
        ));
    }
    Ok(Json(VapidPublicKeyResponse { public_key }))
}

pub async fn subscribe_push(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PushSubscriptionRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .store
        .save_push_subscription(&req.endpoint, &req.keys.p256dh, &req.keys.auth)
        .await
        .map_err(|e| internal_error(&e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn unsubscribe_push(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PushUnsubscribeRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .store
        .delete_push_subscription(&req.endpoint)
        .await
        .map_err(|e| internal_error(&e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/v1/push/action` — stop a session using the short-lived action
/// token carried in a `usage_alert` push notification's payload, in place of a
/// bearer token.
///
/// Deliberately unauthenticated at the HTTP layer (exempted in
/// [`crate::api::auth::require_auth`]): a service worker has no way to read the
/// app's auth token, so the capability lives in the token itself — HMAC-signed
/// server-side, bound to one session, expiring in
/// [`action_token::DEFAULT_TTL_SECS`].
///
/// Status codes:
/// - `200` — the token verified and the session was stopped (or was already
///   stopped — re-tapping a notification after the daemon's own auto-stop
///   already fired is a harmless no-op, not an error).
/// - `401` — the token is missing, malformed, tampered, expired, or signed for
///   a different action. One generic message for every case, so a bad token
///   can't be used as an oracle to distinguish *why* it's bad.
/// - `410` — the token itself is valid, but its target session no longer
///   exists (e.g. purged after stopping). Distinct from a plain 404: the
///   *token* was fine, only its target is gone.
pub async fn action(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PushActionRequest>,
) -> Result<Json<PushActionResponse>, ApiError> {
    let secret = state
        .config
        .read()
        .await
        .notifications
        .vapid
        .action_secret
        .clone();

    let session_id =
        action_token::verify_action_token(&secret, &req.token, STOP_ACTION, Utc::now())
            .map_err(|_| unauthorized("invalid or expired action token"))?;

    match state.session_manager.stop_session(&session_id, false).await {
        Ok(()) => {
            let session_name = state
                .session_manager
                .get_session(&session_id)
                .await
                .ok()
                .flatten()
                .map_or_else(|| session_id.clone(), |s| s.name);
            Ok(Json(PushActionResponse {
                session_id,
                session_name,
            }))
        }
        Err(e) if e.to_string().contains("not found") => Err(gone(&e.to_string())),
        Err(e) => Err(map_manager_err(&e)),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use axum::http::StatusCode;
    use axum_test::TestServer;
    use chrono::Utc;

    use crate::api::AppState;
    use crate::api::routes;
    use crate::backend::StubBackend;
    use crate::config::{Config, NodeConfig, NotificationsConfig, VapidConfig};
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;

    async fn test_server_with_vapid(private_key: &str, public_key: &str) -> (TestServer, Store) {
        test_server_with_vapid_and_secret(private_key, public_key, "").await
    }

    async fn test_server_with_vapid_and_secret(
        private_key: &str,
        public_key: &str,
        action_secret: &str,
    ) -> (TestServer, Store) {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "test-node".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                ..NodeConfig::default()
            },
            notifications: NotificationsConfig {
                vapid: VapidConfig {
                    private_key: private_key.into(),
                    public_key: public_key.into(),
                    action_secret: action_secret.into(),
                },
                ..Default::default()
            },
            ..Default::default()
        };
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(backend, store.clone(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(config, manager, peer_registry, store.clone());
        let app = routes::build(state);
        (TestServer::new(app).unwrap(), store)
    }

    #[tokio::test]
    async fn test_get_vapid_key_returns_public_key() {
        let (server, _store) = test_server_with_vapid("priv-key", "pub-key-123").await;
        let resp = server.get("/api/v1/push/vapid-key").await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("pub-key-123"));
    }

    #[tokio::test]
    async fn test_get_vapid_key_empty_returns_503() {
        let (server, _store) = test_server_with_vapid("", "").await;
        let resp = server.get("/api/v1/push/vapid-key").await;
        resp.assert_status(StatusCode::SERVICE_UNAVAILABLE);
        let body = resp.text();
        assert!(body.contains("VAPID keys not configured"));
    }

    #[tokio::test]
    async fn test_subscribe_push() {
        let (server, store) = test_server_with_vapid("priv", "pub").await;
        let resp = server
            .post("/api/v1/push/subscribe")
            .json(&serde_json::json!({
                "endpoint": "https://push.example.com/sub1",
                "keys": {
                    "p256dh": "p256dh-value",
                    "auth": "auth-value"
                }
            }))
            .await;
        resp.assert_status(StatusCode::NO_CONTENT);

        let subs = store.list_push_subscriptions().await.unwrap();
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].endpoint, "https://push.example.com/sub1");
        assert_eq!(subs[0].p256dh, "p256dh-value");
        assert_eq!(subs[0].auth, "auth-value");
    }

    #[tokio::test]
    async fn test_subscribe_push_replaces_existing() {
        let (server, store) = test_server_with_vapid("priv", "pub").await;
        // Subscribe twice with same endpoint
        for auth in ["auth1", "auth2"] {
            server
                .post("/api/v1/push/subscribe")
                .json(&serde_json::json!({
                    "endpoint": "https://push.example.com/sub1",
                    "keys": {
                        "p256dh": "p256dh-value",
                        "auth": auth
                    }
                }))
                .await;
        }

        let subs = store.list_push_subscriptions().await.unwrap();
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].auth, "auth2");
    }

    #[tokio::test]
    async fn test_unsubscribe_push() {
        let (server, store) = test_server_with_vapid("priv", "pub").await;
        // Subscribe first
        server
            .post("/api/v1/push/subscribe")
            .json(&serde_json::json!({
                "endpoint": "https://push.example.com/sub1",
                "keys": { "p256dh": "p", "auth": "a" }
            }))
            .await;

        // Unsubscribe
        let resp = server
            .post("/api/v1/push/unsubscribe")
            .json(&serde_json::json!({
                "endpoint": "https://push.example.com/sub1"
            }))
            .await;
        resp.assert_status(StatusCode::NO_CONTENT);

        let subs = store.list_push_subscriptions().await.unwrap();
        assert!(subs.is_empty());
    }

    #[tokio::test]
    async fn test_unsubscribe_push_nonexistent() {
        let (server, _store) = test_server_with_vapid("priv", "pub").await;
        let resp = server
            .post("/api/v1/push/unsubscribe")
            .json(&serde_json::json!({
                "endpoint": "https://push.example.com/nonexistent"
            }))
            .await;
        resp.assert_status(StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_subscribe_push_invalid_body() {
        let (server, _store) = test_server_with_vapid("priv", "pub").await;
        let resp = server
            .post("/api/v1/push/subscribe")
            .json(&serde_json::json!({"invalid": true}))
            .await;
        assert_ne!(resp.status_code(), StatusCode::NO_CONTENT);
    }

    // -- POST /api/v1/push/action --

    use crate::notifications::action_token::{DEFAULT_TTL_SECS, STOP_ACTION, sign_action_token};

    // `axum_test`'s response future isn't `Send`; this test-only helper is never
    // spawned across threads (`#[tokio::test]` defaults to a current-thread
    // runtime), so the lint doesn't apply here.
    #[allow(clippy::future_not_send)]
    async fn spawn_session(server: &TestServer, name: &str) -> String {
        let resp = server
            .post("/api/v1/sessions")
            .json(&serde_json::json!({
                "name": name,
                "workdir": "/tmp",
                "command": "test"
            }))
            .await;
        let created: serde_json::Value = serde_json::from_str(&resp.text()).unwrap();
        created["session"]["id"].as_str().unwrap().to_owned()
    }

    #[tokio::test]
    async fn test_push_action_stops_session() {
        let (server, _store) = test_server_with_vapid_and_secret("", "", "action-secret").await;
        let id = spawn_session(&server, "push-action-stop").await;

        let token = sign_action_token(
            "action-secret",
            &id,
            STOP_ACTION,
            Utc::now(),
            DEFAULT_TTL_SECS,
        );
        let resp = server
            .post("/api/v1/push/action")
            .json(&serde_json::json!({ "token": token }))
            .await;
        resp.assert_status(StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["session_id"], id);
        assert_eq!(body["session_name"], "push-action-stop");

        let get_resp = server.get(&format!("/api/v1/sessions/{id}")).await;
        let session: serde_json::Value = get_resp.json();
        assert_eq!(session["status"], "stopped");
    }

    #[tokio::test]
    async fn test_push_action_idempotent_on_already_stopped_session() {
        // Re-tapping the notification after the session was already stopped
        // (e.g. the watchdog's own 100% auto-stop beat the user to it) succeeds
        // rather than erroring — see the `action` handler doc comment.
        let (server, _store) = test_server_with_vapid_and_secret("", "", "action-secret").await;
        let id = spawn_session(&server, "push-action-double-stop").await;
        server.post(&format!("/api/v1/sessions/{id}/stop")).await;

        let token = sign_action_token(
            "action-secret",
            &id,
            STOP_ACTION,
            Utc::now(),
            DEFAULT_TTL_SECS,
        );
        let resp = server
            .post("/api/v1/push/action")
            .json(&serde_json::json!({ "token": token }))
            .await;
        resp.assert_status(StatusCode::OK);
    }

    #[tokio::test]
    async fn test_push_action_malformed_token_401() {
        let (server, _store) = test_server_with_vapid_and_secret("", "", "action-secret").await;
        let resp = server
            .post("/api/v1/push/action")
            .json(&serde_json::json!({ "token": "not-a-valid-token" }))
            .await;
        resp.assert_status(StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_push_action_expired_token_401() {
        let (server, _store) = test_server_with_vapid_and_secret("", "", "action-secret").await;
        let id = spawn_session(&server, "push-action-expired").await;
        let token = sign_action_token("action-secret", &id, STOP_ACTION, Utc::now(), -1);
        let resp = server
            .post("/api/v1/push/action")
            .json(&serde_json::json!({ "token": token }))
            .await;
        resp.assert_status(StatusCode::UNAUTHORIZED);

        // The session was never actually stopped.
        let get_resp = server.get(&format!("/api/v1/sessions/{id}")).await;
        let session: serde_json::Value = get_resp.json();
        assert_eq!(session["status"], "active");
    }

    #[tokio::test]
    async fn test_push_action_wrong_secret_401() {
        let (server, _store) = test_server_with_vapid_and_secret("", "", "action-secret").await;
        let id = spawn_session(&server, "push-action-wrong-secret").await;
        // Signed with a different secret than the one the server has configured.
        let token = sign_action_token(
            "some-other-secret",
            &id,
            STOP_ACTION,
            Utc::now(),
            DEFAULT_TTL_SECS,
        );
        let resp = server
            .post("/api/v1/push/action")
            .json(&serde_json::json!({ "token": token }))
            .await;
        resp.assert_status(StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_push_action_nonexistent_session_410() {
        let (server, _store) = test_server_with_vapid_and_secret("", "", "action-secret").await;
        let token = sign_action_token(
            "action-secret",
            "00000000-0000-0000-0000-000000000000",
            STOP_ACTION,
            Utc::now(),
            DEFAULT_TTL_SECS,
        );
        let resp = server
            .post("/api/v1/push/action")
            .json(&serde_json::json!({ "token": token }))
            .await;
        resp.assert_status(StatusCode::GONE);
    }

    #[tokio::test]
    async fn test_push_action_bypasses_auth_in_public_bind_mode() {
        // A separate check at the full-router integration level (auth.rs unit-tests
        // the middleware directly): even with `bind = "public"` and no bearer token
        // at all, the action endpoint is reachable — the token in the body is the
        // capability.
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "test-node".into(),
                port: 7433,
                bind: pulpo_common::auth::BindMode::Public,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig {
                token: "unrelated-bearer-token".into(),
            },
            notifications: NotificationsConfig {
                vapid: VapidConfig {
                    action_secret: "action-secret".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        };
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(backend, store.clone(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(config, manager, peer_registry, store);
        let server = TestServer::new(routes::build(state)).unwrap();

        // No Authorization header at all.
        let token = sign_action_token(
            "action-secret",
            "00000000-0000-0000-0000-000000000000",
            STOP_ACTION,
            Utc::now(),
            DEFAULT_TTL_SECS,
        );
        let resp = server
            .post("/api/v1/push/action")
            .json(&serde_json::json!({ "token": token }))
            .await;
        // Reaches the handler (410, not 401-from-the-auth-middleware) — proof
        // the auth middleware let it straight through.
        resp.assert_status(StatusCode::GONE);
    }
}
