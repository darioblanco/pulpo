use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode};
use pulpo_common::api::{ErrorResponse, UpdateWatchdogRequest, WatchdogConfigResponse};

type ApiError = (StatusCode, Json<ErrorResponse>);

fn internal_error(msg: &str) -> ApiError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: msg.to_owned(),
        }),
    )
}

fn bad_request(msg: &str) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: msg.to_owned(),
        }),
    )
}

pub async fn get_watchdog(
    State(state): State<Arc<super::AppState>>,
) -> Result<Json<WatchdogConfigResponse>, ApiError> {
    let config = state.config.read().await;
    let resp = WatchdogConfigResponse {
        enabled: config.watchdog.enabled,
        memory_threshold: config.watchdog.memory_threshold,
        check_interval_secs: config.watchdog.check_interval_secs,
        breach_count: config.watchdog.breach_count,
        idle_timeout_secs: config.watchdog.idle_timeout_secs,
        idle_action: config.watchdog.idle_action.clone(),
        finished_ttl_secs: config.watchdog.finished_ttl_secs,
    };
    drop(config);
    Ok(Json(resp))
}

pub async fn update_watchdog(
    State(state): State<Arc<super::AppState>>,
    Json(req): Json<UpdateWatchdogRequest>,
) -> Result<Json<WatchdogConfigResponse>, ApiError> {
    let mut config = state.config.write().await;

    if let Some(enabled) = req.enabled {
        config.watchdog.enabled = enabled;
    }
    if let Some(threshold) = req.memory_threshold {
        config.watchdog.memory_threshold = threshold;
    }
    if let Some(interval) = req.check_interval_secs {
        config.watchdog.check_interval_secs = interval;
    }
    if let Some(count) = req.breach_count {
        config.watchdog.breach_count = count;
    }
    if let Some(timeout) = req.idle_timeout_secs {
        config.watchdog.idle_timeout_secs = timeout;
    }
    if let Some(action) = req.idle_action {
        config.watchdog.idle_action = action;
    }
    if let Some(ttl) = req.finished_ttl_secs {
        config.watchdog.finished_ttl_secs = ttl;
    }

    // Validate the updated config
    config
        .watchdog
        .validate()
        .map_err(|e| bad_request(&e.to_string()))?;

    // Save to disk
    if !state.config_path.as_os_str().is_empty() {
        crate::config::save(&config, &state.config_path)
            .map_err(|e| internal_error(&e.to_string()))?;
    }

    // Push updated config to the running watchdog loop
    if let Some(tx) = &state.watchdog_config_tx {
        let runtime_cfg = crate::watchdog::WatchdogRuntimeConfig {
            threshold: config.watchdog.memory_threshold,
            interval: std::time::Duration::from_secs(config.watchdog.check_interval_secs),
            breach_count: config.watchdog.breach_count,
            idle: crate::watchdog::IdleConfig {
                enabled: config.watchdog.idle_timeout_secs > 0,
                timeout_secs: config.watchdog.idle_timeout_secs,
                action: if config.watchdog.idle_action == "kill" {
                    crate::watchdog::IdleAction::Kill
                } else {
                    crate::watchdog::IdleAction::Alert
                },
            },
            finished_ttl_secs: config.watchdog.finished_ttl_secs,
        };
        // Ignore send error — watchdog may have shut down
        let _ = tx.send(runtime_cfg);
    }

    let resp = WatchdogConfigResponse {
        enabled: config.watchdog.enabled,
        memory_threshold: config.watchdog.memory_threshold,
        check_interval_secs: config.watchdog.check_interval_secs,
        breach_count: config.watchdog.breach_count,
        idle_timeout_secs: config.watchdog.idle_timeout_secs,
        idle_action: config.watchdog.idle_action.clone(),
        finished_ttl_secs: config.watchdog.finished_ttl_secs,
    };
    drop(config);
    Ok(Json(resp))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::AppState;
    use crate::backend::Backend;
    use crate::config::{Config, GuardDefaultConfig, NodeConfig};
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;
    use anyhow::Result;
    use axum::extract::State;
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

    async fn test_state() -> Arc<AppState> {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        )
        .with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        AppState::new(
            Config {
                node: NodeConfig {
                    name: "test-node".into(),
                    port: 7433,
                    data_dir: tmpdir.path().to_str().unwrap().into(),
                    ..NodeConfig::default()
                },
                auth: crate::config::AuthConfig::default(),
                peers: HashMap::new(),
                guards: GuardDefaultConfig::default(),
                session_defaults: crate::config::SessionDefaultsConfig::default(),
                watchdog: crate::config::WatchdogConfig::default(),
                inks: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
            },
            manager,
            peer_registry,
        )
    }

    async fn test_state_with_config_path() -> Arc<AppState> {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        )
        .with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let config_path = tmpdir.path().join("config.toml");
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        AppState::with_event_tx(
            Config {
                node: NodeConfig {
                    name: "test-node".into(),
                    port: 7433,
                    data_dir: tmpdir.path().to_str().unwrap().into(),
                    ..NodeConfig::default()
                },
                auth: crate::config::AuthConfig::default(),
                peers: HashMap::new(),
                guards: GuardDefaultConfig::default(),
                session_defaults: crate::config::SessionDefaultsConfig::default(),
                watchdog: crate::config::WatchdogConfig::default(),
                inks: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
            },
            config_path,
            manager,
            peer_registry,
            event_tx,
        )
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

    #[tokio::test]
    async fn test_get_watchdog_returns_defaults() {
        let state = test_state().await;
        let Json(resp) = get_watchdog(State(state)).await.unwrap();
        assert!(resp.enabled);
        assert_eq!(resp.memory_threshold, 90);
        assert_eq!(resp.check_interval_secs, 10);
        assert_eq!(resp.breach_count, 3);
        assert_eq!(resp.idle_timeout_secs, 600);
        assert_eq!(resp.idle_action, "alert");
    }

    #[tokio::test]
    async fn test_update_watchdog_all_fields() {
        let state = test_state().await;
        let req = UpdateWatchdogRequest {
            enabled: Some(false),
            memory_threshold: Some(80),
            check_interval_secs: Some(30),
            breach_count: Some(5),
            idle_timeout_secs: Some(300),
            idle_action: Some("kill".into()),
            finished_ttl_secs: None,
        };
        let Json(resp) = update_watchdog(State(state.clone()), Json(req))
            .await
            .unwrap();
        assert!(!resp.enabled);
        assert_eq!(resp.memory_threshold, 80);
        assert_eq!(resp.check_interval_secs, 30);
        assert_eq!(resp.breach_count, 5);
        assert_eq!(resp.idle_timeout_secs, 300);
        assert_eq!(resp.idle_action, "kill");

        // Verify persisted in memory
        let Json(current) = get_watchdog(State(state)).await.unwrap();
        assert!(!current.enabled);
        assert_eq!(current.memory_threshold, 80);
    }

    #[tokio::test]
    async fn test_update_watchdog_partial() {
        let state = test_state().await;
        let req = UpdateWatchdogRequest {
            enabled: Some(false),
            ..Default::default()
        };
        let Json(resp) = update_watchdog(State(state), Json(req)).await.unwrap();
        assert!(!resp.enabled);
        // Others unchanged from defaults
        assert_eq!(resp.memory_threshold, 90);
        assert_eq!(resp.check_interval_secs, 10);
    }

    #[tokio::test]
    async fn test_update_watchdog_invalid_threshold() {
        let state = test_state().await;
        let req = UpdateWatchdogRequest {
            memory_threshold: Some(0),
            ..Default::default()
        };
        let result = update_watchdog(State(state), Json(req)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_update_watchdog_invalid_interval() {
        let state = test_state().await;
        let req = UpdateWatchdogRequest {
            check_interval_secs: Some(0),
            ..Default::default()
        };
        let result = update_watchdog(State(state), Json(req)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_update_watchdog_invalid_breach_count() {
        let state = test_state().await;
        let req = UpdateWatchdogRequest {
            breach_count: Some(0),
            ..Default::default()
        };
        let result = update_watchdog(State(state), Json(req)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_update_watchdog_invalid_idle_action() {
        let state = test_state().await;
        let req = UpdateWatchdogRequest {
            idle_action: Some("explode".into()),
            ..Default::default()
        };
        let result = update_watchdog(State(state), Json(req)).await;
        assert!(result.is_err());
        let (status, Json(err)) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(err.error.contains("idle_action"));
    }

    #[tokio::test]
    async fn test_update_watchdog_saves_to_disk() {
        let state = test_state_with_config_path().await;
        let req = UpdateWatchdogRequest {
            enabled: Some(false),
            memory_threshold: Some(75),
            ..Default::default()
        };
        let _ = update_watchdog(State(state.clone()), Json(req))
            .await
            .unwrap();
        let loaded = crate::config::load(state.config_path.to_str().unwrap()).unwrap();
        assert!(!loaded.watchdog.enabled);
        assert_eq!(loaded.watchdog.memory_threshold, 75);
    }

    #[tokio::test]
    async fn test_update_watchdog_empty_request() {
        let state = test_state().await;
        let req = UpdateWatchdogRequest::default();
        let Json(resp) = update_watchdog(State(state), Json(req)).await.unwrap();
        // No changes, all defaults
        assert!(resp.enabled);
        assert_eq!(resp.memory_threshold, 90);
    }

    #[test]
    fn test_internal_error() {
        let (status, Json(err)) = internal_error("boom");
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(err.error, "boom");
    }

    #[test]
    fn test_bad_request() {
        let (status, Json(err)) = bad_request("nope");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(err.error, "nope");
    }

    #[tokio::test]
    async fn test_update_watchdog_pushes_config_to_channel() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        )
        .with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let initial = crate::watchdog::WatchdogRuntimeConfig {
            threshold: 90,
            interval: std::time::Duration::from_secs(10),
            breach_count: 3,
            idle: crate::watchdog::IdleConfig::default(),
            finished_ttl_secs: 0,
        };
        let (config_tx, config_rx) = tokio::sync::watch::channel(initial);
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let state = AppState::with_watchdog_tx(
            Config {
                node: NodeConfig {
                    name: "test-node".into(),
                    port: 7433,
                    data_dir: tmpdir.path().to_str().unwrap().into(),
                    ..NodeConfig::default()
                },
                auth: crate::config::AuthConfig::default(),
                peers: HashMap::new(),
                guards: GuardDefaultConfig::default(),
                session_defaults: crate::config::SessionDefaultsConfig::default(),
                watchdog: crate::config::WatchdogConfig::default(),
                inks: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
            },
            std::path::PathBuf::new(),
            manager,
            peer_registry,
            event_tx,
            Some(config_tx),
        );

        // Update threshold via API
        let req = UpdateWatchdogRequest {
            memory_threshold: Some(75),
            check_interval_secs: Some(30),
            idle_action: Some("kill".into()),
            ..Default::default()
        };
        let Json(resp) = update_watchdog(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.memory_threshold, 75);

        // Verify the watch channel received the update
        let received = config_rx.borrow().clone();
        assert_eq!(received.threshold, 75);
        assert_eq!(received.interval, std::time::Duration::from_secs(30));
        assert_eq!(received.idle.action, crate::watchdog::IdleAction::Kill);
    }

    #[tokio::test]
    async fn test_update_watchdog_no_channel_still_works() {
        // When watchdog_config_tx is None, update should still succeed
        let state = test_state().await;
        assert!(state.watchdog_config_tx.is_none());
        let req = UpdateWatchdogRequest {
            memory_threshold: Some(80),
            ..Default::default()
        };
        let Json(resp) = update_watchdog(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.memory_threshold, 80);
    }
}
