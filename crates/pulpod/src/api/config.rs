use std::sync::Arc;

use axum::{Json, extract::State};
use pulpo_common::api::{
    AuthConfigResponse, ConfigResponse, NodeConfigResponse, NotificationsConfigResponse,
    WatchdogConfigResponse, WebhookEndpointConfigResponse,
};

use crate::api::error::ApiError;

fn config_to_response(config: &crate::config::Config) -> ConfigResponse {
    ConfigResponse {
        node: NodeConfigResponse {
            name: config.node.name.clone(),
            port: config.node.port,
            data_dir: config.node.data_dir.clone(),
            bind: config.node.bind,
        },
        auth: AuthConfigResponse {},
        watchdog: WatchdogConfigResponse {
            enabled: config.watchdog.enabled,
            check_interval_secs: config.watchdog.check_interval_secs,
            idle_timeout_secs: config.watchdog.idle_timeout_secs,
            idle_action: config.watchdog.idle_action.clone(),
            idle_threshold_secs: config.watchdog.idle_threshold_secs,
            extra_waiting_patterns: config.watchdog.waiting_patterns.clone(),
        },
        notifications: NotificationsConfigResponse {
            // Surface the full set of endpoints (canonical top-level `[[webhooks]]`
            // unioned with the deprecated `[notifications.webhooks]` form).
            webhooks: config
                .webhook_endpoints()
                .iter()
                .map(|w| WebhookEndpointConfigResponse {
                    name: w.name.clone(),
                    url: w.url.clone(),
                    events: w.events.clone(),
                    min_severity: w.min_severity.clone(),
                })
                .collect(),
        },
    }
}

/// Read-only view of pulpod's effective configuration. The config file
/// (`~/.pulpo/config.toml`) is the source of truth — the web UI and API only read
/// it; editing happens by hand-editing the file and restarting pulpod.
pub async fn get_config(
    State(state): State<Arc<super::AppState>>,
) -> Result<Json<ConfigResponse>, ApiError> {
    let config = state.config.read().await;
    let response = config_to_response(&config);
    drop(config);
    Ok(Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::AppState;
    use crate::backend::StubBackend;

    use crate::config::{Config, NodeConfig};
    use crate::session::manager::SessionManager;
    use crate::store::Store;
    use axum::extract::State;

    async fn test_state() -> Arc<AppState> {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(backend, store.clone(), None).with_no_stale_grace();
        AppState::new(
            Config {
                node: NodeConfig {
                    name: "test-node".into(),
                    port: 7433,
                    data_dir: tmpdir.path().to_str().unwrap().into(),
                    ..NodeConfig::default()
                },
                ..Default::default()
            },
            manager,
            store,
        )
    }

    #[tokio::test]
    async fn test_get_config_returns_current() {
        let state = test_state().await;
        let Json(resp) = get_config(State(state)).await.unwrap();
        assert_eq!(resp.node.name, "test-node");
        assert_eq!(resp.node.port, 7433);
    }

    #[test]
    fn test_config_to_response() {
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp".into(),
                ..NodeConfig::default()
            },
            ..Default::default()
        };
        let resp = config_to_response(&config);
        assert_eq!(resp.node.name, "test");
        assert_eq!(resp.node.port, 7433);
        assert_eq!(resp.node.bind, pulpo_common::auth::BindMode::Local);
    }

    #[tokio::test]
    async fn test_get_config_returns_bind() {
        let state = test_state().await;
        let Json(resp) = get_config(State(state)).await.unwrap();
        assert_eq!(resp.node.bind, pulpo_common::auth::BindMode::Local);
    }

    #[tokio::test]
    async fn test_config_response_debug() {
        let state = test_state().await;
        let Json(resp) = get_config(State(state)).await.unwrap();
        let debug = format!("{resp:?}");
        assert!(debug.contains("test-node"));
    }

    #[test]
    fn test_config_to_response_with_notifications() {
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp".into(),
                ..NodeConfig::default()
            },
            watchdog: crate::config::WatchdogConfig {
                enabled: true,
                memory_threshold: None,
                check_interval_secs: 30,
                breach_count: None,
                idle_timeout_secs: 300,
                idle_action: "pause".into(),
                ready_ttl_secs: None,
                adopt_tmux: None,
                idle_threshold_secs: 60,
                waiting_patterns: Vec::new(),
                burn_ceiling_usd_per_hour: None,
                burn_ceiling_tokens_per_hour: None,
                burn_action: None,
            },
            notifications: crate::config::NotificationsConfig {
                webhooks: vec![crate::config::WebhookEndpointConfig {
                    name: "primary".into(),
                    url: "https://example.com/hook".into(),
                    events: vec!["session.created".into()],
                    min_severity: None,
                    secret: None,
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        let resp = config_to_response(&config);
        // Watchdog
        assert!(resp.watchdog.enabled);
        assert_eq!(resp.watchdog.check_interval_secs, 30);
        assert_eq!(resp.watchdog.idle_timeout_secs, 300);
        assert_eq!(resp.watchdog.idle_action, "pause");
        // Notifications
        assert_eq!(resp.notifications.webhooks.len(), 1);
        let w = &resp.notifications.webhooks[0];
        assert_eq!(w.url, "https://example.com/hook");
        assert_eq!(w.events, vec!["session.created"]);
    }

    #[test]
    fn test_config_to_response_with_webhooks() {
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp".into(),
                ..NodeConfig::default()
            },
            notifications: crate::config::NotificationsConfig {
                webhooks: vec![
                    crate::config::WebhookEndpointConfig {
                        name: "ci-hook".into(),
                        url: "https://example.com/hook".into(),
                        events: vec!["ready".into(), "killed".into()],
                        min_severity: None,
                        secret: Some("s3cret".into()),
                    },
                    crate::config::WebhookEndpointConfig {
                        name: "logs-hook".into(),
                        url: "https://logs.example.com".into(),
                        events: vec![],
                        min_severity: None,
                        secret: None,
                    },
                ],
                ..Default::default()
            },
            ..Default::default()
        };
        let resp = config_to_response(&config);
        assert_eq!(resp.notifications.webhooks.len(), 2);
        let w0 = &resp.notifications.webhooks[0];
        assert_eq!(w0.name, "ci-hook");
        assert_eq!(w0.url, "https://example.com/hook");
        assert_eq!(w0.events, vec!["ready", "killed"]);
        let w1 = &resp.notifications.webhooks[1];
        assert_eq!(w1.name, "logs-hook");
        assert!(w1.events.is_empty());
    }
}
