use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use pulpo_common::api::{
    DetectionEventResponse, DetectionEventsQuery, DetectorStatsResponse, ErrorResponse,
};

type ApiError = (StatusCode, Json<ErrorResponse>);

fn internal_error(msg: &str) -> ApiError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: msg.to_owned(),
        }),
    )
}

fn not_found(msg: &str) -> ApiError {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: msg.to_owned(),
        }),
    )
}

pub async fn list(
    State(state): State<Arc<super::AppState>>,
    Query(query): Query<DetectionEventsQuery>,
) -> Result<Json<Vec<DetectionEventResponse>>, ApiError> {
    let events = state
        .session_manager
        .store()
        .list_detection_events(query.detector.as_deref())
        .await
        .map_err(|e| internal_error(&e.to_string()))?;
    let response: Vec<_> = events
        .into_iter()
        .map(|e| DetectionEventResponse {
            id: e.id,
            session_id: e.session_id,
            detector: e.detector,
            action: e.action,
            was_false_positive: e.was_false_positive,
            created_at: e.created_at.to_rfc3339(),
        })
        .collect();
    Ok(Json(response))
}

pub async fn mark_false_positive(
    State(state): State<Arc<super::AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let found = state
        .session_manager
        .store()
        .mark_detection_false_positive(id)
        .await
        .map_err(|e| internal_error(&e.to_string()))?;
    if found {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found(&format!("Detection event {id} not found")))
    }
}

pub async fn stats(
    State(state): State<Arc<super::AppState>>,
) -> Result<Json<HashMap<String, DetectorStatsResponse>>, ApiError> {
    let detector_stats = state
        .session_manager
        .store()
        .detection_event_stats()
        .await
        .map_err(|e| internal_error(&e.to_string()))?;
    let response: HashMap<_, _> = detector_stats
        .into_iter()
        .map(|(k, v)| {
            (
                k,
                DetectorStatsResponse {
                    total: v.total,
                    false_positives: v.false_positives,
                    rate: v.rate,
                },
            )
        })
        .collect();
    Ok(Json(response))
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
    use std::collections::HashMap as StdHashMap;

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

    async fn test_state() -> Arc<AppState> {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
            },
            auth: crate::config::AuthConfig::default(),
            peers: StdHashMap::new(),
            guards: crate::config::GuardDefaultConfig::default(),
            watchdog: crate::config::WatchdogConfig::default(),
            personas: StdHashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            StdHashMap::new(),
        );
        let peer_registry = PeerRegistry::new(&StdHashMap::new());
        AppState::new(config, manager, peer_registry)
    }

    #[test]
    fn test_stub_backend_methods() {
        let b = StubBackend;
        assert!(b.create_session("n", "d", "c").is_ok());
        assert!(b.kill_session("n").is_ok());
        assert!(b.is_alive("n").unwrap());
        assert!(b.capture_output("n", 10).unwrap().is_empty());
        assert!(b.send_input("n", "t").is_ok());
        assert!(b.setup_logging("n", "p").is_ok());
    }

    #[test]
    fn test_internal_error() {
        let (status, Json(body)) = internal_error("boom");
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body.error, "boom");
    }

    #[test]
    fn test_not_found() {
        let (status, Json(body)) = not_found("missing");
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body.error, "missing");
    }

    #[tokio::test]
    async fn test_list_handler() {
        let state = test_state().await;
        state
            .session_manager
            .store()
            .insert_detection_event("s1", "memory", "kill")
            .await
            .unwrap();

        let query = Query(DetectionEventsQuery { detector: None });
        let result = list(State(state), query).await.unwrap();
        assert_eq!(result.0.len(), 1);
        assert_eq!(result.0[0].detector, "memory");
    }

    #[tokio::test]
    async fn test_list_handler_filtered() {
        let state = test_state().await;
        state
            .session_manager
            .store()
            .insert_detection_event("s1", "memory", "kill")
            .await
            .unwrap();
        state
            .session_manager
            .store()
            .insert_detection_event("s2", "idle", "alert")
            .await
            .unwrap();

        let query = Query(DetectionEventsQuery {
            detector: Some("idle".into()),
        });
        let result = list(State(state), query).await.unwrap();
        assert_eq!(result.0.len(), 1);
        assert_eq!(result.0[0].detector, "idle");
    }

    #[tokio::test]
    async fn test_mark_handler_found() {
        let state = test_state().await;
        let id = state
            .session_manager
            .store()
            .insert_detection_event("s1", "memory", "kill")
            .await
            .unwrap();

        let result = mark_false_positive(State(state), Path(id)).await.unwrap();
        assert_eq!(result, StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_mark_handler_not_found() {
        let state = test_state().await;
        let err = mark_false_positive(State(state), Path(999))
            .await
            .unwrap_err();
        assert_eq!(err.0, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_stats_handler() {
        let state = test_state().await;
        state
            .session_manager
            .store()
            .insert_detection_event("s1", "memory", "kill")
            .await
            .unwrap();

        let result = stats(State(state.clone())).await.unwrap();
        assert!(result.0.contains_key("memory"));
        assert_eq!(result.0["memory"].total, 1);
    }

    #[tokio::test]
    async fn test_list_handler_store_error() {
        let state = test_state().await;
        sqlx::query("DROP TABLE detection_events")
            .execute(state.session_manager.store().pool())
            .await
            .unwrap();

        let query = Query(DetectionEventsQuery { detector: None });
        let err = list(State(state), query).await.unwrap_err();
        assert_eq!(err.0, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_mark_handler_store_error() {
        let state = test_state().await;
        sqlx::query("DROP TABLE detection_events")
            .execute(state.session_manager.store().pool())
            .await
            .unwrap();

        let err = mark_false_positive(State(state), Path(1))
            .await
            .unwrap_err();
        assert_eq!(err.0, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_stats_handler_store_error() {
        let state = test_state().await;
        sqlx::query("DROP TABLE detection_events")
            .execute(state.session_manager.store().pool())
            .await
            .unwrap();

        let err = stats(State(state)).await.unwrap_err();
        assert_eq!(err.0, StatusCode::INTERNAL_SERVER_ERROR);
    }
}
