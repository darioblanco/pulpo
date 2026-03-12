use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use pulpo_common::api::{
    CultureContextQuery, CultureDeleteResponse, CultureFileContentResponse, CultureFileEntry,
    CultureFilesResponse, CultureItemResponse, CulturePushResponse, CultureResponse, ErrorResponse,
    ListCultureQuery, SyncStatusResponse, UpdateCultureRequest,
};

use super::AppState;

type ApiError = (StatusCode, Json<ErrorResponse>);

pub async fn list(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListCultureQuery>,
) -> Result<Json<CultureResponse>, Json<ErrorResponse>> {
    let Some(repo) = state.session_manager.culture_repo() else {
        return Ok(Json(CultureResponse { culture: vec![] }));
    };
    let kind_str = query.kind.map(|k| k.to_string());
    let culture = repo
        .list(
            query.session_id.as_deref(),
            kind_str.as_deref(),
            query.repo.as_deref(),
            query.ink.as_deref(),
            query.limit,
        )
        .map_err(|e| {
            Json(ErrorResponse {
                error: e.to_string(),
            })
        })?;

    Ok(Json(CultureResponse { culture }))
}

pub async fn context(
    State(state): State<Arc<AppState>>,
    Query(query): Query<CultureContextQuery>,
) -> Result<Json<CultureResponse>, Json<ErrorResponse>> {
    let Some(repo) = state.session_manager.culture_repo() else {
        return Ok(Json(CultureResponse { culture: vec![] }));
    };
    let limit = query.limit.unwrap_or(10);
    let culture = repo
        .query_context(query.workdir.as_deref(), query.ink.as_deref(), limit)
        .map_err(|e| {
            Json(ErrorResponse {
                error: e.to_string(),
            })
        })?;

    Ok(Json(CultureResponse { culture }))
}

pub async fn get(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<CultureItemResponse>, ApiError> {
    let Some(repo) = state.session_manager.culture_repo() else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "culture repo not configured".into(),
            }),
        ));
    };
    let item = repo.get_by_id(&id).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
    })?;
    item.map_or_else(
        || {
            Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("culture item not found: {id}"),
                }),
            ))
        },
        |culture| Ok(Json(CultureItemResponse { culture })),
    )
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<UpdateCultureRequest>,
) -> Result<Json<CultureItemResponse>, ApiError> {
    let Some(repo) = state.session_manager.culture_repo() else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "culture repo not configured".into(),
            }),
        ));
    };
    let updated = repo
        .update(
            &id,
            body.title.as_deref(),
            body.body.as_deref(),
            body.tags.as_deref(),
            body.relevance,
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;
    if !updated {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("culture item not found: {id}"),
            }),
        ));
    }
    // Re-read the updated item
    let item = repo.get_by_id(&id).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
    })?;
    item.map_or_else(
        || {
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "item deleted after update".into(),
                }),
            ))
        },
        |culture| Ok(Json(CultureItemResponse { culture })),
    )
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<CultureDeleteResponse>, ApiError> {
    let Some(repo) = state.session_manager.culture_repo() else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "culture repo not configured".into(),
            }),
        ));
    };
    let deleted = repo.delete(&id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
    })?;
    Ok(Json(CultureDeleteResponse { deleted }))
}

pub async fn push(
    State(state): State<Arc<AppState>>,
) -> Result<Json<CulturePushResponse>, ApiError> {
    let Some(repo) = state.session_manager.culture_repo() else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "culture repo not configured".into(),
            }),
        ));
    };
    match repo.push().await {
        Ok(()) => Ok(Json(CulturePushResponse {
            pushed: true,
            message: "pushed to remote".into(),
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

pub async fn approve(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<CultureItemResponse>, ApiError> {
    let Some(repo) = state.session_manager.culture_repo() else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "culture repo not configured".into(),
            }),
        ));
    };
    let approved = repo.approve(&id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
    })?;
    if !approved {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("culture item not found: {id}"),
            }),
        ));
    }
    // Re-read the approved item
    let item = repo.get_by_id(&id).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
    })?;
    item.map_or_else(
        || {
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "item deleted after approve".into(),
                }),
            ))
        },
        |culture| Ok(Json(CultureItemResponse { culture })),
    )
}

pub async fn list_files(
    State(state): State<Arc<AppState>>,
) -> Result<Json<CultureFilesResponse>, ApiError> {
    let Some(repo) = state.session_manager.culture_repo() else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "culture repo not configured".into(),
            }),
        ));
    };
    let entries = repo.list_files().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
    })?;
    let files = entries
        .into_iter()
        .map(|(path, is_dir)| CultureFileEntry { path, is_dir })
        .collect();
    Ok(Json(CultureFilesResponse { files }))
}

pub async fn read_file(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
) -> Result<Json<CultureFileContentResponse>, ApiError> {
    let Some(repo) = state.session_manager.culture_repo() else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "culture repo not configured".into(),
            }),
        ));
    };
    let content = repo.read_file(&path).map_err(|e| {
        let status = if e.to_string().contains("not found") {
            StatusCode::NOT_FOUND
        } else if e.to_string().contains("traversal") {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        (
            status,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
    })?;
    Ok(Json(CultureFileContentResponse { path, content }))
}

pub async fn sync_status(State(state): State<Arc<AppState>>) -> Json<SyncStatusResponse> {
    let status = state.sync_status.read().await;
    Json(SyncStatusResponse {
        enabled: status.enabled,
        last_sync: status.last_sync.clone(),
        last_error: status.last_error.clone(),
        pending_commits: status.pending_commits,
        total_syncs: status.total_syncs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::AppState;
    use crate::backend::Backend;
    use crate::config::{Config, NodeConfig};
    use crate::culture::repo::CultureRepo;
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;
    use anyhow::Result;
    use axum_test::TestServer;
    use pulpo_common::culture::{Culture, CultureKind};
    use std::collections::HashMap;
    use uuid::Uuid;

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

    async fn test_state() -> (Arc<AppState>, CultureRepo) {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let culture_repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig::default(),
            peers: HashMap::new(),
            guards: crate::config::GuardDefaultConfig::default(),
            session_defaults: crate::config::SessionDefaultsConfig::default(),
            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
            culture: crate::config::CultureConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        )
        .with_culture_repo(culture_repo.clone(), true);
        let peer_registry = PeerRegistry::new(&HashMap::new());
        (AppState::new(config, manager, peer_registry), culture_repo)
    }

    fn test_router(state: Arc<AppState>) -> TestServer {
        use axum::Router;
        use axum::routing::{self, post};
        let app = Router::new()
            .route("/api/v1/culture", routing::get(list))
            .route("/api/v1/culture/context", routing::get(context))
            .route("/api/v1/culture/push", post(push))
            .route("/api/v1/culture/sync", routing::get(sync_status))
            .route("/api/v1/culture/files", routing::get(list_files))
            .route("/api/v1/culture/files/{*path}", routing::get(read_file))
            .route(
                "/api/v1/culture/{id}",
                routing::get(get).put(update).delete(delete),
            )
            .route("/api/v1/culture/{id}/approve", post(approve))
            .with_state(state);
        TestServer::new(app).unwrap()
    }

    fn make_culture(title: &str, repo: Option<&str>, ink: Option<&str>) -> Culture {
        Culture {
            id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            kind: CultureKind::Summary,
            scope_repo: repo.map(Into::into),
            scope_ink: ink.map(Into::into),
            title: title.into(),
            body: "Body text".into(),
            tags: vec!["claude".into()],
            relevance: 0.5,
            created_at: chrono::Utc::now(),
            last_referenced_at: None,
        }
    }

    #[tokio::test]
    async fn test_list_empty() {
        let (state, _repo) = test_state().await;
        let server = test_router(state);
        let resp = server.get("/api/v1/culture").await;
        resp.assert_status_ok();
        let body: CultureResponse = resp.json();
        assert!(body.culture.is_empty());
    }

    #[tokio::test]
    async fn test_list_returns_culture() {
        let (state, repo) = test_state().await;
        repo.save(&make_culture("finding-1", Some("/repo"), Some("coder")))
            .await
            .unwrap();

        let server = test_router(state);
        let resp = server.get("/api/v1/culture").await;
        resp.assert_status_ok();
        let body: CultureResponse = resp.json();
        assert_eq!(body.culture.len(), 1);
        assert_eq!(body.culture[0].title, "finding-1");
    }

    #[tokio::test]
    async fn test_list_filtered_by_kind() {
        let (state, repo) = test_state().await;
        repo.save(&make_culture("sum", Some("/repo"), None))
            .await
            .unwrap();
        repo.save(&Culture {
            kind: CultureKind::Failure,
            ..make_culture("fail", Some("/repo"), None)
        })
        .await
        .unwrap();

        let server = test_router(state);
        let resp = server.get("/api/v1/culture?kind=failure").await;
        resp.assert_status_ok();
        let body: CultureResponse = resp.json();
        assert_eq!(body.culture.len(), 1);
        assert_eq!(body.culture[0].title, "fail");
    }

    #[tokio::test]
    async fn test_list_with_limit() {
        let (state, repo) = test_state().await;
        for i in 0..5 {
            repo.save(&make_culture(&format!("item-{i}"), None, None))
                .await
                .unwrap();
        }

        let server = test_router(state);
        let resp = server.get("/api/v1/culture?limit=2").await;
        resp.assert_status_ok();
        let body: CultureResponse = resp.json();
        assert_eq!(body.culture.len(), 2);
    }

    #[tokio::test]
    async fn test_context_empty() {
        let (state, _repo) = test_state().await;
        let server = test_router(state);
        let resp = server.get("/api/v1/culture/context").await;
        resp.assert_status_ok();
        let body: CultureResponse = resp.json();
        assert!(body.culture.is_empty());
    }

    #[tokio::test]
    async fn test_context_returns_relevant() {
        let (state, repo) = test_state().await;
        repo.save(&make_culture("global", None, None))
            .await
            .unwrap();
        repo.save(&make_culture("scoped", Some("/my/repo"), Some("coder")))
            .await
            .unwrap();
        repo.save(&make_culture("other", Some("/other/repo"), None))
            .await
            .unwrap();

        let server = test_router(state);
        let resp = server
            .get("/api/v1/culture/context?workdir=/my/repo&ink=coder")
            .await;
        resp.assert_status_ok();
        let body: CultureResponse = resp.json();
        // Should get global + scoped, not other
        assert_eq!(body.culture.len(), 2);
    }

    #[tokio::test]
    async fn test_context_with_limit() {
        let (state, repo) = test_state().await;
        for i in 0..10 {
            repo.save(&Culture {
                scope_repo: None,
                scope_ink: None,
                ..make_culture(&format!("g-{i}"), None, None)
            })
            .await
            .unwrap();
        }

        let server = test_router(state);
        let resp = server.get("/api/v1/culture/context?limit=3").await;
        resp.assert_status_ok();
        let body: CultureResponse = resp.json();
        assert_eq!(body.culture.len(), 3);
    }

    #[tokio::test]
    async fn test_list_filtered_by_session() {
        let (state, repo) = test_state().await;
        let k = make_culture("target", None, None);
        let session_id = k.session_id.to_string();
        repo.save(&k).await.unwrap();
        repo.save(&make_culture("other", None, None)).await.unwrap();

        let server = test_router(state);
        let resp = server
            .get(&format!("/api/v1/culture?session_id={session_id}"))
            .await;
        resp.assert_status_ok();
        let body: CultureResponse = resp.json();
        assert_eq!(body.culture.len(), 1);
        assert_eq!(body.culture[0].title, "target");
    }

    #[tokio::test]
    async fn test_list_filtered_by_repo() {
        let (state, repo) = test_state().await;
        repo.save(&make_culture("r1", Some("/repo/a"), None))
            .await
            .unwrap();
        repo.save(&make_culture("r2", Some("/repo/b"), None))
            .await
            .unwrap();

        let server = test_router(state);
        let resp = server.get("/api/v1/culture?repo=/repo/a").await;
        resp.assert_status_ok();
        let body: CultureResponse = resp.json();
        assert_eq!(body.culture.len(), 1);
        assert_eq!(body.culture[0].title, "r1");
    }

    #[tokio::test]
    async fn test_list_filtered_by_ink() {
        let (state, repo) = test_state().await;
        repo.save(&make_culture("c", None, Some("coder")))
            .await
            .unwrap();
        repo.save(&make_culture("r", None, Some("reviewer")))
            .await
            .unwrap();

        let server = test_router(state);
        let resp = server.get("/api/v1/culture?ink=coder").await;
        resp.assert_status_ok();
        let body: CultureResponse = resp.json();
        assert_eq!(body.culture.len(), 1);
        assert_eq!(body.culture[0].title, "c");
    }

    #[tokio::test]
    async fn test_list_no_culture_repo() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig::default(),
            peers: HashMap::new(),
            guards: crate::config::GuardDefaultConfig::default(),
            session_defaults: crate::config::SessionDefaultsConfig::default(),
            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
            culture: crate::config::CultureConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(config, manager, peer_registry);

        let server = test_router(state);
        let resp = server.get("/api/v1/culture").await;
        resp.assert_status_ok();
        let body: CultureResponse = resp.json();
        assert!(body.culture.is_empty());
    }

    #[tokio::test]
    async fn test_get_item() {
        let (state, repo) = test_state().await;
        let k = make_culture("target", Some("/repo"), None);
        let id = k.id.to_string();
        repo.save(&k).await.unwrap();

        let server = test_router(state);
        let resp = server.get(&format!("/api/v1/culture/{id}")).await;
        resp.assert_status_ok();
        let body: CultureItemResponse = resp.json();
        assert_eq!(body.culture.title, "target");
    }

    #[tokio::test]
    async fn test_get_item_not_found() {
        let (state, _repo) = test_state().await;
        let server = test_router(state);
        let resp = server.get("/api/v1/culture/nonexistent-id").await;
        assert_ne!(resp.status_code(), 200);
    }

    #[tokio::test]
    async fn test_update_item() {
        let (state, repo) = test_state().await;
        let k = make_culture("original", Some("/repo"), None);
        let id = k.id.to_string();
        repo.save(&k).await.unwrap();

        let server = test_router(state);
        let body = UpdateCultureRequest {
            title: Some("updated".into()),
            body: None,
            tags: None,
            relevance: Some(0.9),
        };
        let resp = server
            .put(&format!("/api/v1/culture/{id}"))
            .json(&body)
            .await;
        resp.assert_status_ok();
        let result: CultureItemResponse = resp.json();
        assert_eq!(result.culture.title, "updated");
        assert!((result.culture.relevance - 0.9).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_update_item_not_found() {
        let (state, _repo) = test_state().await;
        let server = test_router(state);
        let body = UpdateCultureRequest {
            title: Some("updated".into()),
            body: None,
            tags: None,
            relevance: None,
        };
        let resp = server
            .put("/api/v1/culture/nonexistent-id")
            .json(&body)
            .await;
        assert_ne!(resp.status_code(), 200);
    }

    #[tokio::test]
    async fn test_delete_item() {
        let (state, repo) = test_state().await;
        let k = make_culture("to-delete", Some("/repo"), None);
        let id = k.id.to_string();
        repo.save(&k).await.unwrap();

        let server = test_router(state);
        let resp = server.delete(&format!("/api/v1/culture/{id}")).await;
        resp.assert_status_ok();
        let body: CultureDeleteResponse = resp.json();
        assert!(body.deleted);

        // Verify it's gone
        let resp = server.get(&format!("/api/v1/culture/{id}")).await;
        assert_ne!(resp.status_code(), 200);
    }

    #[tokio::test]
    async fn test_delete_item_not_found() {
        let (state, _repo) = test_state().await;
        let server = test_router(state);
        let resp = server.delete("/api/v1/culture/nonexistent-id").await;
        resp.assert_status_ok();
        let body: CultureDeleteResponse = resp.json();
        assert!(!body.deleted);
    }

    #[tokio::test]
    async fn test_push_no_remote() {
        let (state, _repo) = test_state().await;
        let server = test_router(state);
        let resp = server.post("/api/v1/culture/push").await;
        // Should error — no remote configured
        assert_ne!(resp.status_code(), 200);
    }

    #[tokio::test]
    async fn test_get_no_culture_repo() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig::default(),
            peers: HashMap::new(),
            guards: crate::config::GuardDefaultConfig::default(),
            session_defaults: crate::config::SessionDefaultsConfig::default(),
            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
            culture: crate::config::CultureConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(config, manager, peer_registry);

        let server = test_router(state);
        let resp = server.get("/api/v1/culture/some-id").await;
        assert_ne!(resp.status_code(), 200);
    }

    #[tokio::test]
    async fn test_update_no_culture_repo() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig::default(),
            peers: HashMap::new(),
            guards: crate::config::GuardDefaultConfig::default(),
            session_defaults: crate::config::SessionDefaultsConfig::default(),
            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
            culture: crate::config::CultureConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(config, manager, peer_registry);

        let server = test_router(state);
        let body = UpdateCultureRequest {
            title: Some("x".into()),
            body: None,
            tags: None,
            relevance: None,
        };
        let resp = server.put("/api/v1/culture/some-id").json(&body).await;
        assert_ne!(resp.status_code(), 200);
    }

    #[tokio::test]
    async fn test_delete_no_culture_repo() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig::default(),
            peers: HashMap::new(),
            guards: crate::config::GuardDefaultConfig::default(),
            session_defaults: crate::config::SessionDefaultsConfig::default(),
            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
            culture: crate::config::CultureConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(config, manager, peer_registry);

        let server = test_router(state);
        let resp = server.delete("/api/v1/culture/some-id").await;
        assert_ne!(resp.status_code(), 200);
    }

    #[tokio::test]
    async fn test_push_no_culture_repo() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig::default(),
            peers: HashMap::new(),
            guards: crate::config::GuardDefaultConfig::default(),
            session_defaults: crate::config::SessionDefaultsConfig::default(),
            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
            culture: crate::config::CultureConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(config, manager, peer_registry);

        let server = test_router(state);
        let resp = server.post("/api/v1/culture/push").await;
        assert_ne!(resp.status_code(), 200);
    }

    #[tokio::test]
    async fn test_context_no_culture_repo() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig::default(),
            peers: HashMap::new(),
            guards: crate::config::GuardDefaultConfig::default(),
            session_defaults: crate::config::SessionDefaultsConfig::default(),
            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
            culture: crate::config::CultureConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(config, manager, peer_registry);

        let server = test_router(state);
        let resp = server.get("/api/v1/culture/context").await;
        resp.assert_status_ok();
        let body: CultureResponse = resp.json();
        assert!(body.culture.is_empty());
    }

    #[tokio::test]
    async fn test_approve_removes_stale_tag() {
        let (state, repo) = test_state().await;
        let mut k = make_culture("stale-entry", Some("/repo"), None);
        k.tags.push("stale".into());
        let id = k.id.to_string();
        repo.save(&k).await.unwrap();

        let server = test_router(state);
        let resp = server.post(&format!("/api/v1/culture/{id}/approve")).await;
        resp.assert_status_ok();
        let body: CultureItemResponse = resp.json();
        assert_eq!(body.culture.title, "stale-entry");
        assert!(!body.culture.tags.contains(&"stale".into()));
        assert!(body.culture.last_referenced_at.is_some());
    }

    #[tokio::test]
    async fn test_approve_not_found() {
        let (state, _repo) = test_state().await;
        let server = test_router(state);
        let resp = server.post("/api/v1/culture/nonexistent-id/approve").await;
        assert_eq!(resp.status_code(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_approve_no_culture_repo() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig::default(),
            peers: HashMap::new(),
            guards: crate::config::GuardDefaultConfig::default(),
            session_defaults: crate::config::SessionDefaultsConfig::default(),
            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
            culture: crate::config::CultureConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(config, manager, peer_registry);

        let server = test_router(state);
        let resp = server.post("/api/v1/culture/some-id/approve").await;
        assert_ne!(resp.status_code(), 200);
    }

    #[tokio::test]
    async fn test_list_files_returns_tree() {
        let (state, _repo) = test_state().await;
        let server = test_router(state);
        let resp = server.get("/api/v1/culture/files").await;
        resp.assert_status_ok();
        let body: CultureFilesResponse = resp.json();
        // Should have at least culture/ dir and culture/AGENTS.md
        let paths: Vec<&str> = body.files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"culture"), "should list culture dir");
        assert!(
            paths.contains(&"culture/AGENTS.md"),
            "should list culture/AGENTS.md"
        );
        assert!(paths.contains(&"pending"), "should list pending dir");
        // .git directory should be excluded (but .gitkeep files are fine)
        assert!(!paths.iter().any(|p| *p == ".git" || p.starts_with(".git/")));
    }

    #[tokio::test]
    async fn test_list_files_no_culture_repo() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig::default(),
            peers: HashMap::new(),
            guards: crate::config::GuardDefaultConfig::default(),
            session_defaults: crate::config::SessionDefaultsConfig::default(),
            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
            culture: crate::config::CultureConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(config, manager, peer_registry);

        let server = test_router(state);
        let resp = server.get("/api/v1/culture/files").await;
        assert_ne!(resp.status_code(), 200);
    }

    #[tokio::test]
    async fn test_read_file_content() {
        let (state, _repo) = test_state().await;
        let server = test_router(state);
        let resp = server.get("/api/v1/culture/files/culture/AGENTS.md").await;
        resp.assert_status_ok();
        let body: CultureFileContentResponse = resp.json();
        assert_eq!(body.path, "culture/AGENTS.md");
        assert!(body.content.contains("# Culture"));
    }

    #[tokio::test]
    async fn test_read_file_not_found() {
        let (state, _repo) = test_state().await;
        let server = test_router(state);
        let resp = server.get("/api/v1/culture/files/nonexistent.md").await;
        assert_eq!(resp.status_code(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_read_file_no_culture_repo() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig::default(),
            peers: HashMap::new(),
            guards: crate::config::GuardDefaultConfig::default(),
            session_defaults: crate::config::SessionDefaultsConfig::default(),
            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
            culture: crate::config::CultureConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(config, manager, peer_registry);

        let server = test_router(state);
        let resp = server.get("/api/v1/culture/files/culture/AGENTS.md").await;
        assert_ne!(resp.status_code(), 200);
    }

    #[tokio::test]
    async fn test_sync_status_disabled() {
        let (state, _repo) = test_state().await;
        let server = test_router(state);
        let resp = server.get("/api/v1/culture/sync").await;
        resp.assert_status_ok();
        let body: SyncStatusResponse = resp.json();
        assert!(!body.enabled);
        assert!(body.last_sync.is_none());
        assert!(body.last_error.is_none());
        assert_eq!(body.pending_commits, 0);
        assert_eq!(body.total_syncs, 0);
    }

    #[tokio::test]
    async fn test_sync_status_enabled() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let culture_repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig::default(),
            peers: HashMap::new(),
            guards: crate::config::GuardDefaultConfig::default(),
            session_defaults: crate::config::SessionDefaultsConfig::default(),
            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
            culture: crate::config::CultureConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        )
        .with_culture_repo(culture_repo.clone(), true);
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let sync_status = std::sync::Arc::new(tokio::sync::RwLock::new(
            crate::culture::sync::SyncStatus::new(true),
        ));
        // Pre-populate sync status
        {
            let mut s = sync_status.write().await;
            s.last_sync = Some("2026-03-12T00:00:00Z".into());
            s.total_syncs = 5;
            s.pending_commits = 2;
        }
        let state = AppState::with_watchdog_tx(
            config,
            tmpdir.path().join("config.toml"),
            manager,
            peer_registry,
            event_tx,
            None,
            sync_status,
        );
        let server = test_router(state);
        let resp = server.get("/api/v1/culture/sync").await;
        resp.assert_status_ok();
        let body: SyncStatusResponse = resp.json();
        assert!(body.enabled);
        assert_eq!(body.last_sync.as_deref(), Some("2026-03-12T00:00:00Z"));
        assert_eq!(body.total_syncs, 5);
        assert_eq!(body.pending_commits, 2);
    }
}
