use std::sync::Arc;

use axum::{
    Router, middleware,
    routing::{delete, get, post, put},
};
use pulpo_common::auth::BindMode;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};

use super::AppState;
use super::auth;
use super::config;
use super::event_push;
use super::events;
use super::fleet;
use super::health;
use super::inks;
use super::node;
use super::notifications;
use super::peers;

use super::node_auth;
use super::node_commands;
use super::push;
use super::schedules;
use super::secrets;
use super::sessions;
use super::static_files;
use super::watchdog;
use super::ws;

#[allow(clippy::too_many_lines)]
pub fn build(state: Arc<AppState>) -> Router {
    // In Public mode, restrict CORS to same-origin only (the embedded web UI
    // is served from the same origin and doesn't need permissive CORS).
    // For Local/Tailscale/Container, allow Any for cross-node convenience.
    let bind_mode = state
        .config
        .try_read()
        .map_or(BindMode::Local, |c| c.node.bind);
    let cors = if bind_mode == BindMode::Public {
        CorsLayer::new()
            .allow_origin(AllowOrigin::predicate(|origin, _parts| {
                // Deny cross-origin requests in Public mode.
                // Same-origin requests don't trigger CORS at all, so this
                // effectively blocks only third-party origins.
                let _ = origin;
                false
            }))
            .allow_methods(Any)
            .allow_headers(Any)
    } else {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
    };

    Router::new()
        .route("/api/v1/health", get(health::check))
        .route("/api/v1/auth/token", get(auth::get_token))
        .route("/api/v1/auth/pairing-url", get(auth::get_pairing_url))
        .route("/api/v1/node", get(node::get_info))
        .route(
            "/api/v1/config",
            get(config::get_config).put(config::update_config),
        )
        .route(
            "/api/v1/watchdog",
            get(watchdog::get_watchdog).put(watchdog::update_watchdog),
        )
        .route(
            "/api/v1/notifications",
            get(notifications::get_notifications).put(notifications::update_notifications),
        )
        .route(
            "/api/v1/peers",
            get(peers::list_peers).post(peers::add_peer),
        )
        .route("/api/v1/peers/{name}", delete(peers::remove_peer))
        .route(
            "/api/v1/sessions",
            get(sessions::list).post(sessions::create),
        )
        .route("/api/v1/sessions/{id}", get(sessions::get))
        .route("/api/v1/sessions/cleanup", post(sessions::cleanup))
        .route("/api/v1/sessions/{id}/stop", post(sessions::stop))
        .route("/api/v1/sessions/{id}/output", get(sessions::output))
        .route(
            "/api/v1/sessions/{id}/output/download",
            get(sessions::download_output),
        )
        .route("/api/v1/sessions/{id}/input", post(sessions::input))
        .route(
            "/api/v1/sessions/{id}/interventions",
            get(sessions::list_interventions),
        )
        .route("/api/v1/sessions/{id}/stream", get(ws::stream))
        .route("/api/v1/sessions/{id}/resume", post(sessions::resume))
        .route("/api/v1/fleet/sessions", get(fleet::fleet_sessions))
        .route("/api/v1/inks", get(inks::list))
        .route(
            "/api/v1/inks/{name}",
            get(inks::get)
                .post(inks::create)
                .put(inks::update)
                .delete(inks::delete),
        )
        .route("/api/v1/push/vapid-key", get(push::get_vapid_key))
        .route("/api/v1/push/subscribe", post(push::subscribe_push))
        .route("/api/v1/push/unsubscribe", post(push::unsubscribe_push))
        .route("/api/v1/events", get(events::stream))
        .route("/api/v1/events/push", post(event_push::push_events))
        .route(
            "/api/v1/controller/nodes",
            get(node_auth::list_enrolled_nodes).post(node_auth::enroll_node),
        )
        .route("/api/v1/node/commands", get(node_commands::get_commands))
        .route(
            "/api/v1/schedules",
            get(schedules::list).post(schedules::create),
        )
        .route(
            "/api/v1/schedules/{id}",
            get(schedules::get)
                .put(schedules::update)
                .delete(schedules::delete),
        )
        .route("/api/v1/schedules/{id}/runs", get(schedules::list_runs))
        .route("/api/v1/secrets", get(secrets::list_secrets))
        .route(
            "/api/v1/secrets/{name}",
            put(secrets::set_secret).delete(secrets::delete_secret),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ))
        .layer(cors)
        .with_state(state)
        .fallback(static_files::serve)
}

#[cfg(all(test, not(coverage)))]
mod tests {
    use super::*;
    use crate::backend::Backend;
    use std::collections::HashMap;

    use crate::config::{Config, NodeConfig};
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;
    use anyhow::Result;
    use axum::http::StatusCode;
    use axum_test::TestServer;

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
            Ok("captured output".into())
        }
        fn send_input(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn setup_logging(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
    }

    async fn test_server() -> TestServer {
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
            notifications: crate::config::NotificationsConfig::default(),
            docker: crate::config::DockerConfig::default(),
            controller: crate::config::ControllerConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(config, manager, peer_registry, store);
        let app = build(state);
        TestServer::new(app).unwrap()
    }

    #[tokio::test]
    async fn test_health() {
        let server = test_server().await;
        let resp = server.get("/api/v1/health").await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("\"status\":\"ok\""));
        assert!(body.contains("\"version\""));
    }

    #[tokio::test]
    async fn test_inks_empty() {
        let server = test_server().await;
        let resp = server.get("/api/v1/inks").await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("\"inks\":{}"));
    }

    #[tokio::test]
    async fn test_inks_with_entries() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let mut inks = HashMap::new();
        inks.insert(
            "reviewer".into(),
            crate::config::InkConfig {
                description: None,
                command: Some("Review code".into()),
                ..crate::config::InkConfig::default()
            },
        );
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
            inks: inks.clone(),
            notifications: crate::config::NotificationsConfig::default(),
            docker: crate::config::DockerConfig::default(),
            controller: crate::config::ControllerConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(backend, store.clone(), inks, None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(config, manager, peer_registry, store);
        let app = build(state);
        let server = TestServer::new(app).unwrap();
        let resp = server.get("/api/v1/inks").await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("reviewer"));
    }

    #[tokio::test]
    async fn test_get_node() {
        let server = test_server().await;
        let resp = server.get("/api/v1/node").await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("test-node"));
    }

    #[tokio::test]
    async fn test_get_peers() {
        let server = test_server().await;
        let resp = server.get("/api/v1/peers").await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("test-node")); // local node name
        assert!(body.contains("\"peers\"")); // peers array
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let server = test_server().await;
        let resp = server.get("/api/v1/sessions").await;
        resp.assert_status_ok();
        let body = resp.text();
        assert_eq!(body, "[]");
    }

    #[tokio::test]
    async fn test_get_session_not_found() {
        let server = test_server().await;
        let resp = server.get("/api/v1/sessions/nonexistent").await;
        resp.assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_create_session_returns_201() {
        let server = test_server().await;
        let resp = server
            .post("/api/v1/sessions")
            .json(&serde_json::json!({
                "name": "create-test",
                "workdir": "/tmp",
                "command": "Do something"
            }))
            .await;
        resp.assert_status(StatusCode::CREATED);
        let body = resp.text();
        assert!(body.contains("tmp"));
        assert!(body.contains("active"));
    }

    #[tokio::test]
    async fn test_create_and_list_session() {
        let server = test_server().await;
        server
            .post("/api/v1/sessions")
            .json(&serde_json::json!({
                "name": "list-test",
                "workdir": "/tmp",
                "command": "test"
            }))
            .await;

        let resp = server.get("/api/v1/sessions").await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("tmp"));
    }

    #[tokio::test]
    async fn test_create_and_get_session() {
        let server = test_server().await;
        let create_resp = server
            .post("/api/v1/sessions")
            .json(&serde_json::json!({
                "name": "get-test",
                "workdir": "/tmp",
                "command": "test"
            }))
            .await;
        let created: serde_json::Value = serde_json::from_str(&create_resp.text()).unwrap();
        let id = created["session"]["id"].as_str().unwrap();

        let resp = server.get(&format!("/api/v1/sessions/{id}")).await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains(id));
    }

    #[tokio::test]
    async fn test_stop_session_via_post() {
        let server = test_server().await;
        let create_resp = server
            .post("/api/v1/sessions")
            .json(&serde_json::json!({
                "name": "stop-test",
                "workdir": "/tmp",
                "command": "test"
            }))
            .await;
        let created: serde_json::Value = serde_json::from_str(&create_resp.text()).unwrap();
        let id = created["session"]["id"].as_str().unwrap();

        let resp = server.post(&format!("/api/v1/sessions/{id}/stop")).await;
        resp.assert_status(StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_stop_session_not_found() {
        let server = test_server().await;
        let resp = server.post("/api/v1/sessions/nonexistent/stop").await;
        resp.assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_stop_session_with_purge() {
        let server = test_server().await;
        let create_resp = server
            .post("/api/v1/sessions")
            .json(&serde_json::json!({
                "name": "stop-purge-test",
                "workdir": "/tmp",
                "command": "test"
            }))
            .await;
        let created: serde_json::Value = serde_json::from_str(&create_resp.text()).unwrap();
        let id = created["session"]["id"].as_str().unwrap();

        let resp = server
            .post(&format!("/api/v1/sessions/{id}/stop?purge=true"))
            .await;
        resp.assert_status(StatusCode::NO_CONTENT);

        // Verify it's gone
        let get_resp = server.get(&format!("/api/v1/sessions/{id}")).await;
        get_resp.assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_get_session_by_name() {
        let server = test_server().await;
        server
            .post("/api/v1/sessions")
            .json(&serde_json::json!({
                "name": "by-name-test",
                "workdir": "/tmp",
                "command": "test"
            }))
            .await;

        let resp = server.get("/api/v1/sessions/by-name-test").await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("by-name-test"));
    }

    #[tokio::test]
    async fn test_stop_session_by_name() {
        let server = test_server().await;
        server
            .post("/api/v1/sessions")
            .json(&serde_json::json!({
                "name": "stop-name-test",
                "workdir": "/tmp",
                "command": "test"
            }))
            .await;

        let resp = server.post("/api/v1/sessions/stop-name-test/stop").await;
        resp.assert_status(StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_stop_session_by_name_with_purge() {
        let server = test_server().await;
        server
            .post("/api/v1/sessions")
            .json(&serde_json::json!({
                "name": "stop-purge-name",
                "workdir": "/tmp",
                "command": "test"
            }))
            .await;

        let resp = server
            .post("/api/v1/sessions/stop-purge-name/stop?purge=true")
            .await;
        resp.assert_status(StatusCode::NO_CONTENT);

        // Verify it's gone
        let get_resp = server.get("/api/v1/sessions/stop-purge-name").await;
        get_resp.assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_session_output() {
        let server = test_server().await;
        let create_resp = server
            .post("/api/v1/sessions")
            .json(&serde_json::json!({
                "name": "out-test",
                "workdir": "/tmp",
                "command": "test"
            }))
            .await;
        let created: serde_json::Value = serde_json::from_str(&create_resp.text()).unwrap();
        let id = created["session"]["id"].as_str().unwrap();

        let resp = server
            .get(&format!("/api/v1/sessions/{id}/output?lines=50"))
            .await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("captured output"));
    }

    #[tokio::test]
    async fn test_session_output_not_found() {
        let server = test_server().await;
        let resp = server.get("/api/v1/sessions/nonexistent/output").await;
        resp.assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_session_input() {
        let server = test_server().await;
        let create_resp = server
            .post("/api/v1/sessions")
            .json(&serde_json::json!({
                "name": "inp-test",
                "workdir": "/tmp",
                "command": "test"
            }))
            .await;
        let created: serde_json::Value = serde_json::from_str(&create_resp.text()).unwrap();
        let id = created["session"]["id"].as_str().unwrap();

        let resp = server
            .post(&format!("/api/v1/sessions/{id}/input"))
            .json(&serde_json::json!({"text": "hello"}))
            .await;
        resp.assert_status(StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_session_input_not_found() {
        let server = test_server().await;
        let resp = server
            .post("/api/v1/sessions/nonexistent/input")
            .json(&serde_json::json!({"text": "hello"}))
            .await;
        resp.assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_list_interventions_empty() {
        let server = test_server().await;
        let create_resp = server
            .post("/api/v1/sessions")
            .json(&serde_json::json!({
                "name": "interv-test",
                "workdir": "/tmp",
                "command": "test"
            }))
            .await;
        let created: serde_json::Value = serde_json::from_str(&create_resp.text()).unwrap();
        let id = created["session"]["id"].as_str().unwrap();

        let resp = server
            .get(&format!("/api/v1/sessions/{id}/interventions"))
            .await;
        resp.assert_status_ok();
        assert_eq!(resp.text(), "[]");
    }

    #[tokio::test]
    async fn test_resume_not_found() {
        let server = test_server().await;
        let resp = server.post("/api/v1/sessions/nonexistent/resume").await;
        resp.assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_resume_not_stale() {
        let server = test_server().await;
        let create_resp = server
            .post("/api/v1/sessions")
            .json(&serde_json::json!({
                "name": "resume-test",
                "workdir": "/tmp",
                "command": "test"
            }))
            .await;
        let created: serde_json::Value = serde_json::from_str(&create_resp.text()).unwrap();
        let id = created["session"]["id"].as_str().unwrap();

        let resp = server.post(&format!("/api/v1/sessions/{id}/resume")).await;
        resp.assert_status(StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_cors_headers_present() {
        let server = test_server().await;
        let resp = server
            .get("/api/v1/node")
            .add_header(
                axum::http::header::ORIGIN,
                axum::http::HeaderValue::from_static("http://other-node:7433"),
            )
            .await;
        resp.assert_status_ok();
        let headers = resp.headers();
        assert_eq!(
            headers
                .get("access-control-allow-origin")
                .unwrap()
                .to_str()
                .unwrap(),
            "*"
        );
    }

    #[tokio::test]
    async fn test_cors_preflight_options() {
        let server = test_server().await;
        let resp = server
            .method(axum::http::Method::OPTIONS, "/api/v1/node")
            .add_header(
                axum::http::header::ORIGIN,
                axum::http::HeaderValue::from_static("http://other-node:7433"),
            )
            .add_header(
                axum::http::header::ACCESS_CONTROL_REQUEST_METHOD,
                axum::http::HeaderValue::from_static("GET"),
            )
            .await;
        resp.assert_status_ok();
        let headers = resp.headers();
        assert!(headers.get("access-control-allow-origin").is_some());
        assert!(headers.get("access-control-allow-methods").is_some());
    }

    #[tokio::test]
    async fn test_cors_on_peers_endpoint() {
        let server = test_server().await;
        let resp = server
            .get("/api/v1/peers")
            .add_header(
                axum::http::header::ORIGIN,
                axum::http::HeaderValue::from_static("http://remote:7433"),
            )
            .await;
        resp.assert_status_ok();
        assert_eq!(
            resp.headers()
                .get("access-control-allow-origin")
                .unwrap()
                .to_str()
                .unwrap(),
            "*"
        );
    }

    async fn test_server_with_bind(bind: pulpo_common::auth::BindMode) -> TestServer {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "test-node".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                bind,
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig {
                token: "test-token".into(),
            },
            peers: HashMap::new(),
            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
            docker: crate::config::DockerConfig::default(),
            controller: crate::config::ControllerConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(config, manager, peer_registry, store);
        let app = build(state);
        TestServer::new(app).unwrap()
    }

    #[tokio::test]
    async fn test_cors_public_bind_denies_cross_origin() {
        let server = test_server_with_bind(pulpo_common::auth::BindMode::Public).await;
        let resp = server
            .get("/api/v1/health")
            .add_header(
                axum::http::header::ORIGIN,
                axum::http::HeaderValue::from_static("http://evil.example.com"),
            )
            .await;
        resp.assert_status_ok();
        // In Public mode, cross-origin requests should NOT get access-control-allow-origin: *
        assert_ne!(
            resp.headers()
                .get("access-control-allow-origin")
                .map(|v| v.to_str().unwrap_or("")),
            Some("*")
        );
    }

    #[tokio::test]
    async fn test_get_config() {
        let server = test_server().await;
        let resp = server.get("/api/v1/config").await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("test-node"));
        assert!(body.contains("7433"));
    }

    #[tokio::test]
    async fn test_put_config() {
        let server = test_server().await;
        let resp = server
            .put("/api/v1/config")
            .json(&serde_json::json!({
                "node_name": "updated"
            }))
            .await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("updated"));
        assert!(body.contains("\"restart_required\":false"));
    }

    #[tokio::test]
    async fn test_put_config_port_change() {
        let server = test_server().await;
        let resp = server
            .put("/api/v1/config")
            .json(&serde_json::json!({
                "port": 9999
            }))
            .await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("9999"));
        assert!(body.contains("\"restart_required\":true"));
    }

    #[tokio::test]
    async fn test_add_peer() {
        let server = test_server().await;
        let resp = server
            .post("/api/v1/peers")
            .json(&serde_json::json!({
                "name": "new-node",
                "address": "10.0.0.5:7433"
            }))
            .await;
        resp.assert_status(StatusCode::CREATED);
        let body = resp.text();
        assert!(body.contains("new-node"));
    }

    #[tokio::test]
    async fn test_add_peer_duplicate() {
        let server = test_server().await;
        let payload = serde_json::json!({
            "name": "dup-node",
            "address": "10.0.0.1:7433"
        });
        server.post("/api/v1/peers").json(&payload).await;
        let resp = server.post("/api/v1/peers").json(&payload).await;
        resp.assert_status(StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_remove_peer() {
        let server = test_server().await;
        // Add then remove
        server
            .post("/api/v1/peers")
            .json(&serde_json::json!({
                "name": "temp-node",
                "address": "10.0.0.1:7433"
            }))
            .await;
        let resp = server.delete("/api/v1/peers/temp-node").await;
        resp.assert_status(StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_remove_peer_not_found() {
        let server = test_server().await;
        let resp = server.delete("/api/v1/peers/nonexistent").await;
        resp.assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_download_output() {
        let server = test_server().await;
        let create_resp = server
            .post("/api/v1/sessions")
            .json(&serde_json::json!({
                "name": "dl-test",
                "workdir": "/tmp",
                "command": "test"
            }))
            .await;
        let created: serde_json::Value = serde_json::from_str(&create_resp.text()).unwrap();
        let id = created["session"]["id"].as_str().unwrap();

        let resp = server
            .get(&format!("/api/v1/sessions/{id}/output/download"))
            .await;
        resp.assert_status_ok();
        let body = resp.text();
        assert_eq!(body, "captured output");
        let headers = resp.headers();
        assert_eq!(
            headers.get("content-type").unwrap().to_str().unwrap(),
            "text/plain; charset=utf-8"
        );
        let disposition = headers
            .get("content-disposition")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(disposition.contains(".log"));
    }

    #[tokio::test]
    async fn test_download_output_not_found() {
        let server = test_server().await;
        let resp = server
            .get("/api/v1/sessions/nonexistent/output/download")
            .await;
        resp.assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_stream_without_upgrade_returns_400() {
        let server = test_server().await;
        // Non-WebSocket GET to a WS endpoint → 400 (missing upgrade header)
        let resp = server.get("/api/v1/sessions/nonexistent/stream").await;
        resp.assert_status(StatusCode::BAD_REQUEST);
    }

    /// Spin up a real TCP server for WebSocket testing (axum-test doesn't support WS).
    async fn ws_test_server() -> (String, Arc<crate::api::AppState>) {
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
            notifications: crate::config::NotificationsConfig::default(),
            docker: crate::config::DockerConfig::default(),
            controller: crate::config::ControllerConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = crate::api::AppState::new(config, manager, peer_registry, store);
        let app = build(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        #[rustfmt::skip]
        tokio::spawn(async move { let _ = axum::serve(listener, app).await; });
        (format!("127.0.0.1:{}", addr.port()), state)
    }

    #[tokio::test]
    async fn test_ws_stream_not_found() {
        let (addr, _state) = ws_test_server().await;
        let result = tokio_tungstenite::connect_async(format!(
            "ws://{addr}/api/v1/sessions/nonexistent/stream"
        ))
        .await;
        // Server should reject with HTTP error before WS upgrade
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_ws_stream_not_running() {
        use pulpo_common::session::SessionStatus;

        let (addr, state) = ws_test_server().await;
        // Create a session, then stop it so it's not running
        let req = pulpo_common::api::CreateSessionRequest {
            name: "ws-dead".into(),
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
        let session = state.session_manager.create_session(req).await.unwrap();
        state
            .session_manager
            .stop_session(&session.id.to_string(), false)
            .await
            .unwrap();

        // Verify session is stopped
        let fetched = state
            .session_manager
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Stopped);

        // WebSocket should fail (session not running)
        let result = tokio_tungstenite::connect_async(format!(
            "ws://{addr}/api/v1/sessions/{}/stream",
            session.id
        ))
        .await;
        assert!(result.is_err());
    }

    /// Backend where `is_alive` fails — causes `get_session` to return an error.
    struct FailIsAliveBackend;

    impl Backend for FailIsAliveBackend {
        fn create_session(&self, _: &str, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn kill_session(&self, _: &str) -> Result<()> {
            Ok(())
        }
        fn is_alive(&self, _: &str) -> Result<bool> {
            Err(anyhow::anyhow!("backend error"))
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
    fn test_fail_is_alive_backend_methods() {
        let b = FailIsAliveBackend;
        assert!(b.create_session("n", "d", "c").is_ok());
        assert!(b.kill_session("n").is_ok());
        assert!(b.is_alive("n").is_err());
        assert!(b.capture_output("n", 10).unwrap().is_empty());
        assert!(b.send_input("n", "t").is_ok());
        assert!(b.setup_logging("n", "p").is_ok());
    }

    #[cfg(coverage)]
    #[tokio::test]
    async fn test_ws_stream_internal_error() {
        // Use a backend where is_alive fails, causing get_session to error
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
            notifications: crate::config::NotificationsConfig::default(),
            docker: crate::config::DockerConfig::default(),
            controller: crate::config::ControllerConfig::default(),
        };
        let backend = Arc::new(FailIsAliveBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = crate::api::AppState::new(config, manager, peer_registry, store);

        // Create a session (create works, but later get_session will call is_alive → error)
        let req = pulpo_common::api::CreateSessionRequest {
            name: "ws-err".into(),
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
        let session = state.session_manager.create_session(req).await.unwrap();

        let app = build(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        #[rustfmt::skip]
        tokio::spawn(async move { let _ = axum::serve(listener, app).await; });

        // WS connect should fail because get_session returns an error (500)
        let result = tokio_tungstenite::connect_async(format!(
            "ws://127.0.0.1:{}/api/v1/sessions/{}/stream",
            addr.port(),
            session.id
        ))
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_ws_stream_upgrade_succeeds() {
        let (addr, state) = ws_test_server().await;
        let req = pulpo_common::api::CreateSessionRequest {
            name: "ws-upgrade".into(),
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
        let session = state.session_manager.create_session(req).await.unwrap();

        // WebSocket upgrade should succeed for a running session
        let result = tokio_tungstenite::connect_async(format!(
            "ws://{addr}/api/v1/sessions/{}/stream",
            session.id
        ))
        .await;
        assert!(result.is_ok());
    }

    /// Echo tests only work in coverage builds where the echo mock is active.
    /// In non-coverage builds, the PTY spawn fails and the connection closes.
    #[cfg(coverage)]
    #[tokio::test]
    async fn test_ws_stream_echo_binary() {
        use futures::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message as TMsg;

        let (addr, state) = ws_test_server().await;
        let req = pulpo_common::api::CreateSessionRequest {
            name: "ws-echo-bin".into(),
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
        let session = state.session_manager.create_session(req).await.unwrap();

        let (mut ws, _) = tokio_tungstenite::connect_async(format!(
            "ws://{addr}/api/v1/sessions/{}/stream",
            session.id
        ))
        .await
        .unwrap();

        ws.send(TMsg::Binary(b"hello".to_vec().into()))
            .await
            .unwrap();

        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), ws.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(msg, TMsg::Binary(ref data) if data[..] == b"hello"[..]));

        let _ = ws.close(None).await;
    }

    #[cfg(coverage)]
    #[tokio::test]
    async fn test_ws_stream_echo_text() {
        use futures::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message as TMsg;

        let (addr, state) = ws_test_server().await;
        let req = pulpo_common::api::CreateSessionRequest {
            name: "ws-echo-txt".into(),
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
        let session = state.session_manager.create_session(req).await.unwrap();

        let (mut ws, _) = tokio_tungstenite::connect_async(format!(
            "ws://{addr}/api/v1/sessions/{}/stream",
            session.id
        ))
        .await
        .unwrap();

        ws.send(TMsg::Text("test text".into())).await.unwrap();

        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), ws.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(msg, TMsg::Text(ref text) if text.contains("echo:test text")));

        let _ = ws.close(None).await;
    }

    #[cfg(coverage)]
    #[tokio::test]
    async fn test_ws_stream_echo_close() {
        use futures::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message as TMsg;

        let (addr, state) = ws_test_server().await;
        let req = pulpo_common::api::CreateSessionRequest {
            name: "ws-close".into(),
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
        let session = state.session_manager.create_session(req).await.unwrap();

        let (mut ws, _) = tokio_tungstenite::connect_async(format!(
            "ws://{addr}/api/v1/sessions/{}/stream",
            session.id
        ))
        .await
        .unwrap();

        // Send Close — server echo mock should break on Close
        ws.send(TMsg::Close(None)).await.unwrap();

        // Read until connection ends
        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), ws.next())
            .await
            .unwrap();
        // Either get Close frame, None, or connection error — all acceptable
        #[rustfmt::skip]
        assert!(matches!(msg, Some(Ok(TMsg::Close(_))) | None | Some(Err(_))));
    }

    // -- Auth middleware integration tests --

    const TEST_TOKEN: &str = "test-auth-token-value";

    async fn authed_test_server() -> TestServer {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "auth-node".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                bind: pulpo_common::auth::BindMode::Public,
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig {
                token: TEST_TOKEN.into(),
            },
            peers: HashMap::new(),
            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
            docker: crate::config::DockerConfig::default(),
            controller: crate::config::ControllerConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(config, manager, peer_registry, store);
        let app = build(state);
        TestServer::new(app).unwrap()
    }

    #[tokio::test]
    async fn test_auth_health_exempt() {
        let server = authed_test_server().await;
        // Health should be accessible without auth
        let resp = server.get("/api/v1/health").await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn test_auth_required_no_token() {
        let server = authed_test_server().await;
        // Protected endpoint without auth → 401
        let resp = server.get("/api/v1/sessions").await;
        resp.assert_status(StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_required_wrong_token() {
        let server = authed_test_server().await;
        let resp = server
            .get("/api/v1/sessions")
            .add_header(
                axum::http::header::AUTHORIZATION,
                axum::http::HeaderValue::from_static("Bearer wrong-token"),
            )
            .await;
        resp.assert_status(StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_required_correct_token() {
        let server = authed_test_server().await;
        let resp = server
            .get("/api/v1/sessions")
            .add_header(
                axum::http::header::AUTHORIZATION,
                axum::http::HeaderValue::from_static("Bearer test-auth-token-value"),
            )
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn test_auth_query_param_token() {
        let server = authed_test_server().await;
        let resp = server
            .get(&format!("/api/v1/sessions?token={TEST_TOKEN}"))
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn test_auth_token_endpoint_exempt() {
        let server = authed_test_server().await;
        // No ConnectInfo in test → fail closed (treated as remote), so pass token
        let bearer = format!("Bearer {TEST_TOKEN}");
        let resp = server
            .get("/api/v1/auth/token")
            .add_header(
                axum::http::header::AUTHORIZATION,
                axum::http::HeaderValue::from_str(&bearer).unwrap(),
            )
            .await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains(TEST_TOKEN));
    }

    #[tokio::test]
    async fn test_auth_pairing_url_endpoint() {
        let server = authed_test_server().await;
        // No ConnectInfo in test → fail closed (treated as remote), so pass token
        let bearer = format!("Bearer {TEST_TOKEN}");
        let resp = server
            .get("/api/v1/auth/pairing-url")
            .add_header(
                axum::http::header::AUTHORIZATION,
                axum::http::HeaderValue::from_str(&bearer).unwrap(),
            )
            .await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains(TEST_TOKEN));
        assert!(body.contains("7433"));
    }

    #[tokio::test]
    async fn test_auth_node_requires_token() {
        let server = authed_test_server().await;
        let resp = server.get("/api/v1/node").await;
        resp.assert_status(StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_node_with_token() {
        let server = authed_test_server().await;
        let resp = server
            .get("/api/v1/node")
            .add_header(
                axum::http::header::AUTHORIZATION,
                axum::http::HeaderValue::from_static("Bearer test-auth-token-value"),
            )
            .await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn test_auth_local_bind_no_auth_needed() {
        // Default test server uses bind=local → no auth required
        let server = test_server().await;
        let resp = server.get("/api/v1/sessions").await;
        resp.assert_status_ok();
    }

    #[tokio::test]
    async fn test_auth_config_endpoint_requires_token() {
        let server = authed_test_server().await;
        let resp = server.get("/api/v1/config").await;
        resp.assert_status(StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_config_with_token() {
        let server = authed_test_server().await;
        let resp = server
            .get("/api/v1/config")
            .add_header(
                axum::http::header::AUTHORIZATION,
                axum::http::HeaderValue::from_static("Bearer test-auth-token-value"),
            )
            .await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("auth-node"));
    }

    #[tokio::test]
    async fn test_auth_static_file_exempt() {
        let server = authed_test_server().await;
        // Non-API paths (static files) should be exempt from auth even with bind=public
        let resp = server.get("/nonexistent.html").await;
        // Should NOT be 401 (the static file handler may return 200 for SPA fallback or 404)
        let status = resp.status_code();
        assert_ne!(status, StatusCode::UNAUTHORIZED);
    }

    /// Test auth via real TCP server (with `ConnectInfo` available).
    async fn real_authed_tcp_server() -> (String, Arc<crate::api::AppState>) {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "auth-tcp".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                bind: pulpo_common::auth::BindMode::Public,
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig {
                token: TEST_TOKEN.into(),
            },
            peers: HashMap::new(),
            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
            docker: crate::config::DockerConfig::default(),
            controller: crate::config::ControllerConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = crate::api::AppState::new(config, manager, peer_registry, store);
        let app = build(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        #[rustfmt::skip]
        tokio::spawn(async move { let _ = axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await; });
        (format!("127.0.0.1:{}", addr.port()), state)
    }

    #[tokio::test]
    async fn test_auth_token_endpoint_loopback_real_server() {
        let (addr, _state) = real_authed_tcp_server().await;
        let client = reqwest::Client::new();
        // From loopback → /api/v1/auth/token should work without auth
        let resp = client
            .get(format!("http://{addr}/api/v1/auth/token"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.text().await.unwrap();
        assert!(body.contains(TEST_TOKEN));
    }

    #[tokio::test]
    async fn test_auth_api_requires_token_real_server() {
        let (addr, _state) = real_authed_tcp_server().await;
        let client = reqwest::Client::new();
        // No token → 401
        let resp = client
            .get(format!("http://{addr}/api/v1/sessions"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401);

        // With token → 200
        let resp = client
            .get(format!("http://{addr}/api/v1/sessions"))
            .header("Authorization", format!("Bearer {TEST_TOKEN}"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
    }

    #[tokio::test]
    async fn test_auth_health_exempt_real_server() {
        let (addr, _state) = real_authed_tcp_server().await;
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("http://{addr}/api/v1/health"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
    }

    #[tokio::test]
    async fn test_auth_static_file_exempt_real_server() {
        let (addr, _state) = real_authed_tcp_server().await;
        let client = reqwest::Client::new();
        // Non-API paths should bypass auth (serve static files / SPA fallback)
        let resp = client
            .get(format!("http://{addr}/index.html"))
            .send()
            .await
            .unwrap();
        assert_ne!(resp.status(), 401);
    }

    #[tokio::test]
    async fn test_events_sse_stream() {
        use axum::body::Body;
        use axum::http::Request;
        use pulpo_common::event::{PulpoEvent, SessionEvent};
        use tower::ServiceExt;

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
            notifications: crate::config::NotificationsConfig::default(),
            docker: crate::config::DockerConfig::default(),
            controller: crate::config::ControllerConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(config, manager, peer_registry, store);

        // Subscribe to the event_tx before building the router so we can send events
        let event_tx = state.event_tx.clone();
        let app = build(state);

        // Send SSE request
        let req = Request::builder()
            .uri("/api/v1/events")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/event-stream"
        );

        // Send an event through the broadcast channel
        event_tx
            .send(PulpoEvent::Session(SessionEvent {
                session_id: "sse-test-id".into(),
                session_name: "sse-session".into(),
                status: "active".into(),
                previous_status: Some("creating".into()),
                node_name: "test-node".into(),
                output_snippet: None,
                timestamp: "2026-01-01T00:00:00Z".into(),
                ..Default::default()
            }))
            .unwrap();

        // Read the response body — collect with a timeout.
        // We drop the event_tx so the stream ends after sending.
        drop(event_tx);
        let body_bytes = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            axum::body::to_bytes(response.into_body(), 4096),
        )
        .await
        .unwrap()
        .unwrap();
        let text = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(text.contains("event: session"));
        assert!(text.contains("sse-test-id"));
        assert!(text.contains("sse-session"));
    }

    #[tokio::test]
    async fn test_get_watchdog() {
        let server = test_server().await;
        let resp = server.get("/api/v1/watchdog").await;
        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        assert_eq!(body["enabled"], true);
        assert_eq!(body["memory_threshold"], 90);
        assert_eq!(body["idle_action"], "alert");
    }

    #[tokio::test]
    async fn test_put_watchdog() {
        let server = test_server().await;
        let resp = server
            .put("/api/v1/watchdog")
            .json(&serde_json::json!({
                "enabled": false,
                "memory_threshold": 75,
                "idle_action": "kill"
            }))
            .await;
        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        assert_eq!(body["enabled"], false);
        assert_eq!(body["memory_threshold"], 75);
        assert_eq!(body["idle_action"], "kill");
    }

    #[tokio::test]
    async fn test_put_watchdog_validation_error() {
        let server = test_server().await;
        let resp = server
            .put("/api/v1/watchdog")
            .json(&serde_json::json!({
                "memory_threshold": 0
            }))
            .await;
        resp.assert_status(StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_get_notifications() {
        let server = test_server().await;
        let resp = server.get("/api/v1/notifications").await;
        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        assert!(body["discord"].is_null());
        assert_eq!(body["webhooks"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn test_put_notifications() {
        let server = test_server().await;
        let resp = server
            .put("/api/v1/notifications")
            .json(&serde_json::json!({
                "discord": {
                    "webhook_url": "https://discord.com/api/webhooks/test",
                    "events": ["active"]
                }
            }))
            .await;
        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        assert_eq!(
            body["discord"]["webhook_url"],
            "https://discord.com/api/webhooks/test"
        );
        assert_eq!(body["discord"]["events"], serde_json::json!(["active"]));
    }

    // -- Push endpoint integration tests --

    async fn test_server_with_vapid() -> TestServer {
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
            notifications: crate::config::NotificationsConfig {
                vapid: crate::config::VapidConfig {
                    private_key: "test-priv-key".into(),
                    public_key: "test-pub-key".into(),
                },
                ..Default::default()
            },
            docker: crate::config::DockerConfig::default(),
            controller: crate::config::ControllerConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(config, manager, peer_registry, store);
        let app = build(state);
        TestServer::new(app).unwrap()
    }

    #[tokio::test]
    async fn test_get_vapid_key() {
        let server = test_server_with_vapid().await;
        let resp = server.get("/api/v1/push/vapid-key").await;
        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        assert_eq!(body["public_key"], "test-pub-key");
    }

    #[tokio::test]
    async fn test_get_vapid_key_empty() {
        let server = test_server().await;
        let resp = server.get("/api/v1/push/vapid-key").await;
        resp.assert_status(StatusCode::SERVICE_UNAVAILABLE);
    }

    // -- Secrets integration tests --

    #[tokio::test]
    async fn test_list_secrets_empty() {
        let server = test_server().await;
        let resp = server.get("/api/v1/secrets").await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("\"secrets\":[]"));
    }

    #[tokio::test]
    async fn test_set_and_list_secret() {
        let server = test_server().await;
        let resp = server
            .put("/api/v1/secrets/MY_TOKEN")
            .json(&serde_json::json!({"value": "secret-value"}))
            .await;
        resp.assert_status(StatusCode::NO_CONTENT);

        let resp = server.get("/api/v1/secrets").await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("MY_TOKEN"));
        // Value should NEVER appear in list response
        assert!(!body.contains("secret-value"));
    }

    #[tokio::test]
    async fn test_set_secret_invalid_name() {
        let server = test_server().await;
        let resp = server
            .put("/api/v1/secrets/invalid-name")
            .json(&serde_json::json!({"value": "val"}))
            .await;
        resp.assert_status(StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_delete_secret() {
        let server = test_server().await;
        server
            .put("/api/v1/secrets/DEL_ME")
            .json(&serde_json::json!({"value": "val"}))
            .await;
        let resp = server.delete("/api/v1/secrets/DEL_ME").await;
        resp.assert_status(StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_delete_secret_not_found() {
        let server = test_server().await;
        let resp = server.delete("/api/v1/secrets/NONEXISTENT").await;
        resp.assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_secrets_require_auth() {
        let server = authed_test_server().await;
        let resp = server.get("/api/v1/secrets").await;
        resp.assert_status(StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_push_subscribe_and_unsubscribe() {
        let server = test_server_with_vapid().await;

        // Subscribe
        let resp = server
            .post("/api/v1/push/subscribe")
            .json(&serde_json::json!({
                "endpoint": "https://push.example.com/sub",
                "keys": { "p256dh": "p", "auth": "a" }
            }))
            .await;
        resp.assert_status(StatusCode::NO_CONTENT);

        // Unsubscribe
        let resp = server
            .post("/api/v1/push/unsubscribe")
            .json(&serde_json::json!({
                "endpoint": "https://push.example.com/sub"
            }))
            .await;
        resp.assert_status(StatusCode::NO_CONTENT);
    }
}
