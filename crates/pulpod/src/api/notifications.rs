use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode};
use pulpo_common::api::{
    DiscordWebhookConfigResponse, ErrorResponse, NotificationsConfigResponse,
    UpdateNotificationsRequest, WebhookEndpointConfigResponse,
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

fn to_response(config: &crate::config::NotificationsConfig) -> NotificationsConfigResponse {
    NotificationsConfigResponse {
        discord: config
            .discord
            .as_ref()
            .map(|d| DiscordWebhookConfigResponse {
                webhook_url: d.webhook_url.clone(),
                events: d.events.clone(),
            }),
        webhooks: config
            .webhooks
            .iter()
            .map(|w| WebhookEndpointConfigResponse {
                name: w.name.clone(),
                url: w.url.clone(),
                events: w.events.clone(),
                has_secret: w.secret.is_some(),
            })
            .collect(),
    }
}

pub async fn get_notifications(
    State(state): State<Arc<super::AppState>>,
) -> Result<Json<NotificationsConfigResponse>, ApiError> {
    let config = state.config.read().await;
    let resp = to_response(&config.notifications);
    drop(config);
    Ok(Json(resp))
}

pub async fn update_notifications(
    State(state): State<Arc<super::AppState>>,
    Json(req): Json<UpdateNotificationsRequest>,
) -> Result<Json<NotificationsConfigResponse>, ApiError> {
    let mut config = state.config.write().await;

    // Discord
    if let Some(discord) = req.discord {
        if discord.webhook_url.is_empty() {
            config.notifications.discord = None;
        } else {
            config.notifications.discord = Some(crate::config::DiscordWebhookConfig {
                webhook_url: discord.webhook_url,
                events: discord.events,
            });
        }
    }

    // Webhooks (full replace when provided)
    if let Some(webhooks) = req.webhooks {
        config.notifications.webhooks = webhooks
            .into_iter()
            .map(|w| crate::config::WebhookEndpointConfig {
                name: w.name,
                url: w.url,
                events: w.events,
                secret: w.secret,
            })
            .collect();
    }

    // Save to disk
    if !state.config_path.as_os_str().is_empty() {
        crate::config::save(&config, &state.config_path)
            .map_err(|e| internal_error(&e.to_string()))?;
    }

    let resp = to_response(&config.notifications);
    drop(config);
    Ok(Json(resp))
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
    use axum::extract::State;
    use pulpo_common::api::{DiscordWebhookUpdateRequest, WebhookEndpointUpdateRequest};
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
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
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
                watchdog: crate::config::WatchdogConfig::default(),
                inks: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
                sandbox: crate::config::SandboxConfig::default(),
            },
            manager,
            peer_registry,
            store,
        )
    }

    async fn test_state_with_config_path() -> Arc<AppState> {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
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
                watchdog: crate::config::WatchdogConfig::default(),
                inks: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
                sandbox: crate::config::SandboxConfig::default(),
            },
            config_path,
            manager,
            peer_registry,
            event_tx,
            store,
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
    async fn test_get_notifications_empty() {
        let state = test_state().await;
        let Json(resp) = get_notifications(State(state)).await.unwrap();
        assert!(resp.discord.is_none());
        assert!(resp.webhooks.is_empty());
    }

    #[tokio::test]
    async fn test_update_notifications_set_discord() {
        let state = test_state().await;
        let req = UpdateNotificationsRequest {
            discord: Some(DiscordWebhookUpdateRequest {
                webhook_url: "https://discord.com/api/webhooks/test".into(),
                events: vec!["active".into(), "killed".into()],
            }),
            webhooks: None,
        };
        let Json(resp) = update_notifications(State(state.clone()), Json(req))
            .await
            .unwrap();
        let discord = resp.discord.as_ref().unwrap();
        assert_eq!(discord.webhook_url, "https://discord.com/api/webhooks/test");
        assert_eq!(discord.events, vec!["active", "killed"]);

        // Verify persisted
        let Json(current) = get_notifications(State(state)).await.unwrap();
        assert!(current.discord.is_some());
    }

    #[tokio::test]
    async fn test_update_notifications_clear_discord() {
        let state = test_state().await;
        // Set discord first
        let req = UpdateNotificationsRequest {
            discord: Some(DiscordWebhookUpdateRequest {
                webhook_url: "https://discord.com/api/webhooks/test".into(),
                events: vec![],
            }),
            webhooks: None,
        };
        let _ = update_notifications(State(state.clone()), Json(req))
            .await
            .unwrap();
        // Clear with empty URL
        let req = UpdateNotificationsRequest {
            discord: Some(DiscordWebhookUpdateRequest {
                webhook_url: String::new(),
                events: vec![],
            }),
            webhooks: None,
        };
        let Json(resp) = update_notifications(State(state), Json(req)).await.unwrap();
        assert!(resp.discord.is_none());
    }

    #[tokio::test]
    async fn test_update_notifications_set_webhooks() {
        let state = test_state().await;
        let req = UpdateNotificationsRequest {
            discord: None,
            webhooks: Some(vec![
                WebhookEndpointUpdateRequest {
                    name: "ci-hook".into(),
                    url: "https://example.com/hook".into(),
                    events: vec!["ready".into()],
                    secret: Some("s3cret".into()),
                },
                WebhookEndpointUpdateRequest {
                    name: "logs-hook".into(),
                    url: "https://logs.example.com".into(),
                    events: vec![],
                    secret: None,
                },
            ]),
        };
        let Json(resp) = update_notifications(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.webhooks.len(), 2);
        assert_eq!(resp.webhooks[0].name, "ci-hook");
        assert!(resp.webhooks[0].has_secret);
        assert_eq!(resp.webhooks[1].name, "logs-hook");
        assert!(!resp.webhooks[1].has_secret);
    }

    #[tokio::test]
    async fn test_update_notifications_webhooks_replaces() {
        let state = test_state().await;
        // Set initial
        let req = UpdateNotificationsRequest {
            discord: None,
            webhooks: Some(vec![WebhookEndpointUpdateRequest {
                name: "old".into(),
                url: "https://old.com".into(),
                events: vec![],
                secret: None,
            }]),
        };
        let _ = update_notifications(State(state.clone()), Json(req))
            .await
            .unwrap();
        // Replace
        let req = UpdateNotificationsRequest {
            discord: None,
            webhooks: Some(vec![WebhookEndpointUpdateRequest {
                name: "new".into(),
                url: "https://new.com".into(),
                events: vec!["killed".into()],
                secret: None,
            }]),
        };
        let Json(resp) = update_notifications(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.webhooks.len(), 1);
        assert_eq!(resp.webhooks[0].name, "new");
    }

    #[tokio::test]
    async fn test_update_notifications_empty_webhooks_clears() {
        let state = test_state().await;
        // Set initial
        let req = UpdateNotificationsRequest {
            discord: None,
            webhooks: Some(vec![WebhookEndpointUpdateRequest {
                name: "hook".into(),
                url: "https://a.com".into(),
                events: vec![],
                secret: None,
            }]),
        };
        let _ = update_notifications(State(state.clone()), Json(req))
            .await
            .unwrap();
        // Clear
        let req = UpdateNotificationsRequest {
            discord: None,
            webhooks: Some(vec![]),
        };
        let Json(resp) = update_notifications(State(state), Json(req)).await.unwrap();
        assert!(resp.webhooks.is_empty());
    }

    #[tokio::test]
    async fn test_update_notifications_empty_request() {
        let state = test_state().await;
        let req = UpdateNotificationsRequest::default();
        let Json(resp) = update_notifications(State(state), Json(req)).await.unwrap();
        assert!(resp.discord.is_none());
        assert!(resp.webhooks.is_empty());
    }

    #[tokio::test]
    async fn test_update_notifications_saves_to_disk() {
        let state = test_state_with_config_path().await;
        let req = UpdateNotificationsRequest {
            discord: Some(DiscordWebhookUpdateRequest {
                webhook_url: "https://discord.com/api/webhooks/save-test".into(),
                events: vec!["active".into()],
            }),
            webhooks: None,
        };
        let _ = update_notifications(State(state.clone()), Json(req))
            .await
            .unwrap();
        let loaded = crate::config::load(state.config_path.to_str().unwrap()).unwrap();
        let discord = loaded.notifications.discord.unwrap();
        assert_eq!(
            discord.webhook_url,
            "https://discord.com/api/webhooks/save-test"
        );
    }

    #[test]
    fn test_internal_error() {
        let (status, Json(err)) = internal_error("boom");
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(err.error, "boom");
    }

    #[test]
    fn test_to_response_with_all() {
        let config = crate::config::NotificationsConfig {
            discord: Some(crate::config::DiscordWebhookConfig {
                webhook_url: "https://test.com".into(),
                events: vec!["killed".into()],
            }),
            webhooks: vec![crate::config::WebhookEndpointConfig {
                name: "hook".into(),
                url: "https://hook.com".into(),
                events: vec![],
                secret: Some("key".into()),
            }],
            ..Default::default()
        };
        let resp = to_response(&config);
        let d = resp.discord.unwrap();
        assert_eq!(d.webhook_url, "https://test.com");
        assert_eq!(d.events, vec!["killed"]);
        assert_eq!(resp.webhooks.len(), 1);
        assert!(resp.webhooks[0].has_secret);
    }
}
