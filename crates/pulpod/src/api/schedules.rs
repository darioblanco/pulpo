use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use pulpo_common::api::{CreateScheduleRequest, ErrorResponse, Schedule, UpdateScheduleRequest};
use uuid::Uuid;

use super::AppState;
use crate::scheduler;

type ApiError = (StatusCode, Json<ErrorResponse>);

fn bad_request(msg: &str) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: msg.to_owned(),
        }),
    )
}

#[cfg_attr(coverage, allow(dead_code))]
fn internal_error(msg: &str) -> ApiError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: msg.to_owned(),
        }),
    )
}

pub async fn list(State(state): State<Arc<AppState>>) -> Result<Json<Vec<Schedule>>, ApiError> {
    let schedules = state
        .store
        .list_schedules()
        .await
        .map_err(|e| internal_error(&e.to_string()))?;
    Ok(Json(schedules))
}

pub async fn get(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Schedule>, ApiError> {
    match state.store.get_schedule(&id).await {
        Ok(Some(s)) => Ok(Json(s)),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("schedule not found: {id}"),
            }),
        )),
        Err(e) => Err(internal_error(&e.to_string())),
    }
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateScheduleRequest>,
) -> Result<(StatusCode, Json<Schedule>), ApiError> {
    // Validate cron
    scheduler::validate_cron(&req.cron).map_err(|e| bad_request(&e))?;

    // Check for duplicate name
    let existing = state
        .store
        .list_schedules()
        .await
        .map_err(|e| internal_error(&e.to_string()))?;
    if existing.iter().any(|s| s.name == req.name) {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: format!("schedule '{}' already exists", req.name),
            }),
        ));
    }

    let schedule = Schedule {
        id: Uuid::new_v4().to_string(),
        name: req.name,
        cron: req.cron,
        command: req.command.unwrap_or_default(),
        workdir: req.workdir,
        target_node: req.target_node,
        ink: req.ink,
        description: req.description,
        enabled: true,
        last_run_at: None,
        last_session_id: None,
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    state
        .store
        .insert_schedule(&schedule)
        .await
        .map_err(|e| internal_error(&e.to_string()))?;
    Ok((StatusCode::CREATED, Json(schedule)))
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateScheduleRequest>,
) -> Result<Json<Schedule>, ApiError> {
    let mut schedule = state
        .store
        .get_schedule(&id)
        .await
        .map_err(|e| internal_error(&e.to_string()))?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("schedule not found: {id}"),
                }),
            )
        })?;

    if let Some(cron) = &req.cron {
        scheduler::validate_cron(cron).map_err(|e| bad_request(&e))?;
        schedule.cron.clone_from(cron);
    }
    if let Some(command) = &req.command {
        schedule.command.clone_from(command);
    }
    if let Some(workdir) = &req.workdir {
        schedule.workdir.clone_from(workdir);
    }
    if let Some(target_node) = &req.target_node {
        schedule.target_node.clone_from(target_node);
    }
    if let Some(ink) = &req.ink {
        schedule.ink.clone_from(ink);
    }
    if let Some(description) = &req.description {
        schedule.description.clone_from(description);
    }
    if let Some(enabled) = req.enabled {
        schedule.enabled = enabled;
    }

    // Delete and re-insert (simpler than UPDATE with all fields)
    state
        .store
        .delete_schedule(&schedule.id)
        .await
        .map_err(|e| internal_error(&e.to_string()))?;
    state
        .store
        .insert_schedule(&schedule)
        .await
        .map_err(|e| internal_error(&e.to_string()))?;

    Ok(Json(schedule))
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let exists = state
        .store
        .get_schedule(&id)
        .await
        .map_err(|e| internal_error(&e.to_string()))?;
    if exists.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("schedule not found: {id}"),
            }),
        ));
    }
    state
        .store
        .delete_schedule(&id)
        .await
        .map_err(|e| internal_error(&e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::AppState;
    use crate::backend::Backend;
    use crate::config::{Config, NodeConfig};
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;
    use anyhow::Result;
    use axum_test::TestServer;
    use std::collections::HashMap;

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
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(config, manager, peer_registry, store);
        let app = crate::api::routes::build(state);
        TestServer::new(app).unwrap()
    }

    #[tokio::test]
    async fn test_list_schedules_empty() {
        let server = test_server().await;
        let resp = server.get("/api/v1/schedules").await;
        resp.assert_status_ok();
        assert_eq!(resp.text(), "[]");
    }

    #[tokio::test]
    async fn test_create_schedule() {
        let server = test_server().await;
        let resp = server
            .post("/api/v1/schedules")
            .json(&serde_json::json!({
                "name": "nightly-review",
                "cron": "0 3 * * *",
                "command": "echo hello",
                "workdir": "/tmp"
            }))
            .await;
        resp.assert_status(StatusCode::CREATED);
        let body = resp.text();
        assert!(body.contains("nightly-review"));
        assert!(body.contains("0 3 * * *"));
    }

    #[tokio::test]
    async fn test_create_schedule_invalid_cron() {
        let server = test_server().await;
        let resp = server
            .post("/api/v1/schedules")
            .json(&serde_json::json!({
                "name": "bad-cron",
                "cron": "not valid",
                "command": "echo",
                "workdir": "/tmp"
            }))
            .await;
        resp.assert_status(StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_schedule_duplicate_name() {
        let server = test_server().await;
        server
            .post("/api/v1/schedules")
            .json(&serde_json::json!({
                "name": "dupe",
                "cron": "0 3 * * *",
                "command": "echo",
                "workdir": "/tmp"
            }))
            .await;

        let resp = server
            .post("/api/v1/schedules")
            .json(&serde_json::json!({
                "name": "dupe",
                "cron": "0 4 * * *",
                "command": "echo",
                "workdir": "/tmp"
            }))
            .await;
        resp.assert_status(StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_get_schedule() {
        let server = test_server().await;
        let create_resp = server
            .post("/api/v1/schedules")
            .json(&serde_json::json!({
                "name": "get-test",
                "cron": "0 3 * * *",
                "command": "echo",
                "workdir": "/tmp"
            }))
            .await;
        let created: serde_json::Value = serde_json::from_str(&create_resp.text()).unwrap();
        let id = created["id"].as_str().unwrap();

        let resp = server.get(&format!("/api/v1/schedules/{id}")).await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("get-test"));
    }

    #[tokio::test]
    async fn test_get_schedule_not_found() {
        let server = test_server().await;
        let resp = server.get("/api/v1/schedules/nonexistent").await;
        resp.assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_update_schedule() {
        let server = test_server().await;
        let create_resp = server
            .post("/api/v1/schedules")
            .json(&serde_json::json!({
                "name": "update-test",
                "cron": "0 3 * * *",
                "command": "echo",
                "workdir": "/tmp"
            }))
            .await;
        let created: serde_json::Value = serde_json::from_str(&create_resp.text()).unwrap();
        let id = created["id"].as_str().unwrap();

        let resp = server
            .put(&format!("/api/v1/schedules/{id}"))
            .json(&serde_json::json!({
                "cron": "0 4 * * *",
                "enabled": false
            }))
            .await;
        resp.assert_status_ok();
        let body: serde_json::Value = serde_json::from_str(&resp.text()).unwrap();
        assert_eq!(body["cron"], "0 4 * * *");
        assert_eq!(body["enabled"], false);
    }

    #[tokio::test]
    async fn test_update_schedule_not_found() {
        let server = test_server().await;
        let resp = server
            .put("/api/v1/schedules/nonexistent")
            .json(&serde_json::json!({"enabled": false}))
            .await;
        resp.assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_update_schedule_invalid_cron() {
        let server = test_server().await;
        let create_resp = server
            .post("/api/v1/schedules")
            .json(&serde_json::json!({
                "name": "bad-update",
                "cron": "0 3 * * *",
                "command": "echo",
                "workdir": "/tmp"
            }))
            .await;
        let created: serde_json::Value = serde_json::from_str(&create_resp.text()).unwrap();
        let id = created["id"].as_str().unwrap();

        let resp = server
            .put(&format!("/api/v1/schedules/{id}"))
            .json(&serde_json::json!({"cron": "invalid"}))
            .await;
        resp.assert_status(StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_delete_schedule() {
        let server = test_server().await;
        let create_resp = server
            .post("/api/v1/schedules")
            .json(&serde_json::json!({
                "name": "del-test",
                "cron": "0 3 * * *",
                "command": "echo",
                "workdir": "/tmp"
            }))
            .await;
        let created: serde_json::Value = serde_json::from_str(&create_resp.text()).unwrap();
        let id = created["id"].as_str().unwrap();

        let resp = server.delete(&format!("/api/v1/schedules/{id}")).await;
        resp.assert_status(StatusCode::NO_CONTENT);

        // Verify it's gone
        let get_resp = server.get(&format!("/api/v1/schedules/{id}")).await;
        get_resp.assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_delete_schedule_not_found() {
        let server = test_server().await;
        let resp = server.delete("/api/v1/schedules/nonexistent").await;
        resp.assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_create_schedule_without_command() {
        let server = test_server().await;
        let resp = server
            .post("/api/v1/schedules")
            .json(&serde_json::json!({
                "name": "no-cmd",
                "cron": "0 3 * * *",
                "workdir": "/tmp"
            }))
            .await;
        resp.assert_status(StatusCode::CREATED);
        let body: serde_json::Value = serde_json::from_str(&resp.text()).unwrap();
        assert_eq!(body["command"], "");
    }

    #[tokio::test]
    async fn test_create_schedule_with_all_fields() {
        let server = test_server().await;
        let resp = server
            .post("/api/v1/schedules")
            .json(&serde_json::json!({
                "name": "full-test",
                "cron": "0 3 * * *",
                "command": "echo hello",
                "workdir": "/tmp",
                "target_node": "raven",
                "ink": "reviewer",
                "description": "Nightly code review"
            }))
            .await;
        resp.assert_status(StatusCode::CREATED);
        let body: serde_json::Value = serde_json::from_str(&resp.text()).unwrap();
        assert_eq!(body["target_node"], "raven");
        assert_eq!(body["ink"], "reviewer");
        assert_eq!(body["description"], "Nightly code review");
    }

    #[tokio::test]
    async fn test_update_schedule_all_fields() {
        let server = test_server().await;
        let create_resp = server
            .post("/api/v1/schedules")
            .json(&serde_json::json!({
                "name": "update-all",
                "cron": "0 3 * * *",
                "command": "echo",
                "workdir": "/tmp"
            }))
            .await;
        let created: serde_json::Value = serde_json::from_str(&create_resp.text()).unwrap();
        let id = created["id"].as_str().unwrap();

        let resp = server
            .put(&format!("/api/v1/schedules/{id}"))
            .json(&serde_json::json!({
                "cron": "*/5 * * * *",
                "command": "echo updated",
                "workdir": "/home",
                "target_node": "raven",
                "ink": "reviewer",
                "description": "Updated desc"
            }))
            .await;
        resp.assert_status_ok();
        let body: serde_json::Value = serde_json::from_str(&resp.text()).unwrap();
        assert_eq!(body["cron"], "*/5 * * * *");
        assert_eq!(body["command"], "echo updated");
        assert_eq!(body["workdir"], "/home");
        assert_eq!(body["target_node"], "raven");
        assert_eq!(body["ink"], "reviewer");
        assert_eq!(body["description"], "Updated desc");
    }

    #[tokio::test]
    async fn test_list_schedules_after_create() {
        let server = test_server().await;
        server
            .post("/api/v1/schedules")
            .json(&serde_json::json!({
                "name": "list-test",
                "cron": "0 3 * * *",
                "command": "echo",
                "workdir": "/tmp"
            }))
            .await;

        let resp = server.get("/api/v1/schedules").await;
        resp.assert_status_ok();
        let body = resp.text();
        assert!(body.contains("list-test"));
    }
}
