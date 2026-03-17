use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode};
use pulpo_common::api::{
    ErrorResponse, PushSubscriptionRequest, PushUnsubscribeRequest, VapidPublicKeyResponse,
};

use super::AppState;

pub async fn get_vapid_key(
    State(state): State<Arc<AppState>>,
) -> Result<Json<VapidPublicKeyResponse>, (StatusCode, Json<ErrorResponse>)> {
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
            Json(ErrorResponse {
                error: "VAPID keys not configured".into(),
            }),
        ));
    }
    Ok(Json(VapidPublicKeyResponse { public_key }))
}

pub async fn subscribe_push(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PushSubscriptionRequest>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    state
        .store
        .save_push_subscription(&req.endpoint, &req.keys.p256dh, &req.keys.auth)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn unsubscribe_push(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PushUnsubscribeRequest>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    state
        .store
        .delete_push_subscription(&req.endpoint)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use axum::http::StatusCode;
    use axum_test::TestServer;

    use crate::api::AppState;
    use crate::api::routes;
    use crate::backend::Backend;
    use crate::config::{Config, NodeConfig, NotificationsConfig, VapidConfig};
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
            Ok(String::new())
        }
        fn send_input(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn setup_logging(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
    }

    async fn test_server_with_vapid(private_key: &str, public_key: &str) -> (TestServer, Store) {
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
            auth: crate::config::AuthConfig::default(),
            peers: HashMap::new(),
            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig {
                vapid: VapidConfig {
                    private_key: private_key.into(),
                    public_key: public_key.into(),
                },
                ..Default::default()
            },
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new()).with_no_stale_grace();
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
}
