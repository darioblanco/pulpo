use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use pulpo_common::api::{
    ErrorResponse, KnowledgeContextQuery, KnowledgeDeleteResponse, KnowledgeItemResponse,
    KnowledgePushResponse, KnowledgeResponse, ListKnowledgeQuery, UpdateKnowledgeRequest,
};

use super::AppState;

type ApiError = (StatusCode, Json<ErrorResponse>);

pub async fn list(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListKnowledgeQuery>,
) -> Result<Json<KnowledgeResponse>, Json<ErrorResponse>> {
    let Some(repo) = state.session_manager.knowledge_repo() else {
        return Ok(Json(KnowledgeResponse { knowledge: vec![] }));
    };
    let kind_str = query.kind.map(|k| k.to_string());
    let knowledge = repo
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

    Ok(Json(KnowledgeResponse { knowledge }))
}

pub async fn context(
    State(state): State<Arc<AppState>>,
    Query(query): Query<KnowledgeContextQuery>,
) -> Result<Json<KnowledgeResponse>, Json<ErrorResponse>> {
    let Some(repo) = state.session_manager.knowledge_repo() else {
        return Ok(Json(KnowledgeResponse { knowledge: vec![] }));
    };
    let limit = query.limit.unwrap_or(10);
    let knowledge = repo
        .query_context(query.workdir.as_deref(), query.ink.as_deref(), limit)
        .map_err(|e| {
            Json(ErrorResponse {
                error: e.to_string(),
            })
        })?;

    Ok(Json(KnowledgeResponse { knowledge }))
}

pub async fn get(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<KnowledgeItemResponse>, ApiError> {
    let Some(repo) = state.session_manager.knowledge_repo() else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "knowledge repo not configured".into(),
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
                    error: format!("knowledge item not found: {id}"),
                }),
            ))
        },
        |knowledge| Ok(Json(KnowledgeItemResponse { knowledge })),
    )
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<UpdateKnowledgeRequest>,
) -> Result<Json<KnowledgeItemResponse>, ApiError> {
    let Some(repo) = state.session_manager.knowledge_repo() else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "knowledge repo not configured".into(),
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
                error: format!("knowledge item not found: {id}"),
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
        |knowledge| Ok(Json(KnowledgeItemResponse { knowledge })),
    )
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<KnowledgeDeleteResponse>, ApiError> {
    let Some(repo) = state.session_manager.knowledge_repo() else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "knowledge repo not configured".into(),
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
    Ok(Json(KnowledgeDeleteResponse { deleted }))
}

pub async fn push(
    State(state): State<Arc<AppState>>,
) -> Result<Json<KnowledgePushResponse>, ApiError> {
    let Some(repo) = state.session_manager.knowledge_repo() else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "knowledge repo not configured".into(),
            }),
        ));
    };
    match repo.push().await {
        Ok(()) => Ok(Json(KnowledgePushResponse {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::AppState;
    use crate::backend::Backend;
    use crate::config::{Config, NodeConfig};
    use crate::knowledge::repo::KnowledgeRepo;
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;
    use anyhow::Result;
    use axum_test::TestServer;
    use pulpo_common::knowledge::{Knowledge, KnowledgeKind};
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

    async fn test_state() -> (Arc<AppState>, KnowledgeRepo) {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let knowledge_repo = KnowledgeRepo::init(tmpdir.path().to_str().unwrap(), None)
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
            knowledge: crate::config::KnowledgeConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        )
        .with_knowledge_repo(knowledge_repo.clone(), true);
        let peer_registry = PeerRegistry::new(&HashMap::new());
        (
            AppState::new(config, manager, peer_registry),
            knowledge_repo,
        )
    }

    fn test_router(state: Arc<AppState>) -> TestServer {
        use axum::Router;
        use axum::routing::{self, post};
        let app = Router::new()
            .route("/api/v1/knowledge", routing::get(list))
            .route("/api/v1/knowledge/context", routing::get(context))
            .route("/api/v1/knowledge/push", post(push))
            .route(
                "/api/v1/knowledge/{id}",
                routing::get(get).put(update).delete(delete),
            )
            .with_state(state);
        TestServer::new(app).unwrap()
    }

    fn make_knowledge(title: &str, repo: Option<&str>, ink: Option<&str>) -> Knowledge {
        Knowledge {
            id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            kind: KnowledgeKind::Summary,
            scope_repo: repo.map(Into::into),
            scope_ink: ink.map(Into::into),
            title: title.into(),
            body: "Body text".into(),
            tags: vec!["claude".into()],
            relevance: 0.5,
            created_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_list_empty() {
        let (state, _repo) = test_state().await;
        let server = test_router(state);
        let resp = server.get("/api/v1/knowledge").await;
        resp.assert_status_ok();
        let body: KnowledgeResponse = resp.json();
        assert!(body.knowledge.is_empty());
    }

    #[tokio::test]
    async fn test_list_returns_knowledge() {
        let (state, repo) = test_state().await;
        repo.save(&make_knowledge("finding-1", Some("/repo"), Some("coder")))
            .await
            .unwrap();

        let server = test_router(state);
        let resp = server.get("/api/v1/knowledge").await;
        resp.assert_status_ok();
        let body: KnowledgeResponse = resp.json();
        assert_eq!(body.knowledge.len(), 1);
        assert_eq!(body.knowledge[0].title, "finding-1");
    }

    #[tokio::test]
    async fn test_list_filtered_by_kind() {
        let (state, repo) = test_state().await;
        repo.save(&make_knowledge("sum", Some("/repo"), None))
            .await
            .unwrap();
        repo.save(&Knowledge {
            kind: KnowledgeKind::Failure,
            ..make_knowledge("fail", Some("/repo"), None)
        })
        .await
        .unwrap();

        let server = test_router(state);
        let resp = server.get("/api/v1/knowledge?kind=failure").await;
        resp.assert_status_ok();
        let body: KnowledgeResponse = resp.json();
        assert_eq!(body.knowledge.len(), 1);
        assert_eq!(body.knowledge[0].title, "fail");
    }

    #[tokio::test]
    async fn test_list_with_limit() {
        let (state, repo) = test_state().await;
        for i in 0..5 {
            repo.save(&make_knowledge(&format!("item-{i}"), None, None))
                .await
                .unwrap();
        }

        let server = test_router(state);
        let resp = server.get("/api/v1/knowledge?limit=2").await;
        resp.assert_status_ok();
        let body: KnowledgeResponse = resp.json();
        assert_eq!(body.knowledge.len(), 2);
    }

    #[tokio::test]
    async fn test_context_empty() {
        let (state, _repo) = test_state().await;
        let server = test_router(state);
        let resp = server.get("/api/v1/knowledge/context").await;
        resp.assert_status_ok();
        let body: KnowledgeResponse = resp.json();
        assert!(body.knowledge.is_empty());
    }

    #[tokio::test]
    async fn test_context_returns_relevant() {
        let (state, repo) = test_state().await;
        repo.save(&make_knowledge("global", None, None))
            .await
            .unwrap();
        repo.save(&make_knowledge("scoped", Some("/my/repo"), Some("coder")))
            .await
            .unwrap();
        repo.save(&make_knowledge("other", Some("/other/repo"), None))
            .await
            .unwrap();

        let server = test_router(state);
        let resp = server
            .get("/api/v1/knowledge/context?workdir=/my/repo&ink=coder")
            .await;
        resp.assert_status_ok();
        let body: KnowledgeResponse = resp.json();
        // Should get global + scoped, not other
        assert_eq!(body.knowledge.len(), 2);
    }

    #[tokio::test]
    async fn test_context_with_limit() {
        let (state, repo) = test_state().await;
        for i in 0..10 {
            repo.save(&Knowledge {
                scope_repo: None,
                scope_ink: None,
                ..make_knowledge(&format!("g-{i}"), None, None)
            })
            .await
            .unwrap();
        }

        let server = test_router(state);
        let resp = server.get("/api/v1/knowledge/context?limit=3").await;
        resp.assert_status_ok();
        let body: KnowledgeResponse = resp.json();
        assert_eq!(body.knowledge.len(), 3);
    }

    #[tokio::test]
    async fn test_list_filtered_by_session() {
        let (state, repo) = test_state().await;
        let k = make_knowledge("target", None, None);
        let session_id = k.session_id.to_string();
        repo.save(&k).await.unwrap();
        repo.save(&make_knowledge("other", None, None))
            .await
            .unwrap();

        let server = test_router(state);
        let resp = server
            .get(&format!("/api/v1/knowledge?session_id={session_id}"))
            .await;
        resp.assert_status_ok();
        let body: KnowledgeResponse = resp.json();
        assert_eq!(body.knowledge.len(), 1);
        assert_eq!(body.knowledge[0].title, "target");
    }

    #[tokio::test]
    async fn test_list_filtered_by_repo() {
        let (state, repo) = test_state().await;
        repo.save(&make_knowledge("r1", Some("/repo/a"), None))
            .await
            .unwrap();
        repo.save(&make_knowledge("r2", Some("/repo/b"), None))
            .await
            .unwrap();

        let server = test_router(state);
        let resp = server.get("/api/v1/knowledge?repo=/repo/a").await;
        resp.assert_status_ok();
        let body: KnowledgeResponse = resp.json();
        assert_eq!(body.knowledge.len(), 1);
        assert_eq!(body.knowledge[0].title, "r1");
    }

    #[tokio::test]
    async fn test_list_filtered_by_ink() {
        let (state, repo) = test_state().await;
        repo.save(&make_knowledge("c", None, Some("coder")))
            .await
            .unwrap();
        repo.save(&make_knowledge("r", None, Some("reviewer")))
            .await
            .unwrap();

        let server = test_router(state);
        let resp = server.get("/api/v1/knowledge?ink=coder").await;
        resp.assert_status_ok();
        let body: KnowledgeResponse = resp.json();
        assert_eq!(body.knowledge.len(), 1);
        assert_eq!(body.knowledge[0].title, "c");
    }

    #[tokio::test]
    async fn test_list_no_knowledge_repo() {
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
            knowledge: crate::config::KnowledgeConfig::default(),
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
        let resp = server.get("/api/v1/knowledge").await;
        resp.assert_status_ok();
        let body: KnowledgeResponse = resp.json();
        assert!(body.knowledge.is_empty());
    }

    #[tokio::test]
    async fn test_get_item() {
        let (state, repo) = test_state().await;
        let k = make_knowledge("target", Some("/repo"), None);
        let id = k.id.to_string();
        repo.save(&k).await.unwrap();

        let server = test_router(state);
        let resp = server.get(&format!("/api/v1/knowledge/{id}")).await;
        resp.assert_status_ok();
        let body: KnowledgeItemResponse = resp.json();
        assert_eq!(body.knowledge.title, "target");
    }

    #[tokio::test]
    async fn test_get_item_not_found() {
        let (state, _repo) = test_state().await;
        let server = test_router(state);
        let resp = server.get("/api/v1/knowledge/nonexistent-id").await;
        assert_ne!(resp.status_code(), 200);
    }

    #[tokio::test]
    async fn test_update_item() {
        let (state, repo) = test_state().await;
        let k = make_knowledge("original", Some("/repo"), None);
        let id = k.id.to_string();
        repo.save(&k).await.unwrap();

        let server = test_router(state);
        let body = UpdateKnowledgeRequest {
            title: Some("updated".into()),
            body: None,
            tags: None,
            relevance: Some(0.9),
        };
        let resp = server
            .put(&format!("/api/v1/knowledge/{id}"))
            .json(&body)
            .await;
        resp.assert_status_ok();
        let result: KnowledgeItemResponse = resp.json();
        assert_eq!(result.knowledge.title, "updated");
        assert!((result.knowledge.relevance - 0.9).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_update_item_not_found() {
        let (state, _repo) = test_state().await;
        let server = test_router(state);
        let body = UpdateKnowledgeRequest {
            title: Some("updated".into()),
            body: None,
            tags: None,
            relevance: None,
        };
        let resp = server
            .put("/api/v1/knowledge/nonexistent-id")
            .json(&body)
            .await;
        assert_ne!(resp.status_code(), 200);
    }

    #[tokio::test]
    async fn test_delete_item() {
        let (state, repo) = test_state().await;
        let k = make_knowledge("to-delete", Some("/repo"), None);
        let id = k.id.to_string();
        repo.save(&k).await.unwrap();

        let server = test_router(state);
        let resp = server.delete(&format!("/api/v1/knowledge/{id}")).await;
        resp.assert_status_ok();
        let body: KnowledgeDeleteResponse = resp.json();
        assert!(body.deleted);

        // Verify it's gone
        let resp = server.get(&format!("/api/v1/knowledge/{id}")).await;
        assert_ne!(resp.status_code(), 200);
    }

    #[tokio::test]
    async fn test_delete_item_not_found() {
        let (state, _repo) = test_state().await;
        let server = test_router(state);
        let resp = server.delete("/api/v1/knowledge/nonexistent-id").await;
        resp.assert_status_ok();
        let body: KnowledgeDeleteResponse = resp.json();
        assert!(!body.deleted);
    }

    #[tokio::test]
    async fn test_push_no_remote() {
        let (state, _repo) = test_state().await;
        let server = test_router(state);
        let resp = server.post("/api/v1/knowledge/push").await;
        // Should error — no remote configured
        assert_ne!(resp.status_code(), 200);
    }

    #[tokio::test]
    async fn test_get_no_knowledge_repo() {
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
            knowledge: crate::config::KnowledgeConfig::default(),
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
        let resp = server.get("/api/v1/knowledge/some-id").await;
        assert_ne!(resp.status_code(), 200);
    }

    #[tokio::test]
    async fn test_update_no_knowledge_repo() {
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
            knowledge: crate::config::KnowledgeConfig::default(),
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
        let body = UpdateKnowledgeRequest {
            title: Some("x".into()),
            body: None,
            tags: None,
            relevance: None,
        };
        let resp = server.put("/api/v1/knowledge/some-id").json(&body).await;
        assert_ne!(resp.status_code(), 200);
    }

    #[tokio::test]
    async fn test_delete_no_knowledge_repo() {
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
            knowledge: crate::config::KnowledgeConfig::default(),
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
        let resp = server.delete("/api/v1/knowledge/some-id").await;
        assert_ne!(resp.status_code(), 200);
    }

    #[tokio::test]
    async fn test_push_no_knowledge_repo() {
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
            knowledge: crate::config::KnowledgeConfig::default(),
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
        let resp = server.post("/api/v1/knowledge/push").await;
        assert_ne!(resp.status_code(), 200);
    }

    #[tokio::test]
    async fn test_context_no_knowledge_repo() {
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
            knowledge: crate::config::KnowledgeConfig::default(),
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
        let resp = server.get("/api/v1/knowledge/context").await;
        resp.assert_status_ok();
        let body: KnowledgeResponse = resp.json();
        assert!(body.knowledge.is_empty());
    }
}
