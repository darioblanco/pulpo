use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use pulpo_common::api::{
    ErrorResponse, KnowledgeContextQuery, KnowledgeResponse, ListKnowledgeQuery,
};

use super::AppState;

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
        .with_knowledge_repo(knowledge_repo.clone());
        let peer_registry = PeerRegistry::new(&HashMap::new());
        (
            AppState::new(config, manager, peer_registry),
            knowledge_repo,
        )
    }

    fn test_router(state: Arc<AppState>) -> TestServer {
        use axum::Router;
        use axum::routing::get;
        let app = Router::new()
            .route("/api/v1/knowledge", get(list))
            .route("/api/v1/knowledge/context", get(context))
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
