use std::sync::Arc;

use axum::{Json, extract::State};
use pulpo_common::api::{
    NotificationsConfigResponse, UpdateNotificationsRequest, WebhookEndpointConfigResponse,
};

use crate::api::error::{ApiError, internal_error};

fn to_response(config: &crate::config::Config) -> NotificationsConfigResponse {
    NotificationsConfigResponse {
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
                has_secret: w.secret.is_some(),
            })
            .collect(),
    }
}

pub async fn get_notifications(
    State(state): State<Arc<super::AppState>>,
) -> Result<Json<NotificationsConfigResponse>, ApiError> {
    let config = state.config.read().await;
    let resp = to_response(&config);
    drop(config);
    Ok(Json(resp))
}

pub async fn update_notifications(
    State(state): State<Arc<super::AppState>>,
    Json(req): Json<UpdateNotificationsRequest>,
) -> Result<Json<NotificationsConfigResponse>, ApiError> {
    let mut config = state.config.write().await;

    // Webhooks (full replace when provided). Writes the canonical top-level
    // `[[webhooks]]` list and clears the deprecated `[notifications.webhooks]`
    // form so the edited set is authoritative.
    if let Some(webhooks) = req.webhooks {
        config.webhooks = webhooks
            .into_iter()
            .map(|w| crate::config::WebhookEndpointConfig {
                name: w.name,
                url: w.url,
                events: w.events,
                min_severity: w.min_severity,
                secret: w.secret,
            })
            .collect();
        config.notifications.webhooks.clear();
    }

    // Save to disk
    if !state.config_path.as_os_str().is_empty() {
        crate::config::save(&config, &state.config_path)
            .map_err(|e| internal_error(&e.to_string()))?;
    }

    let resp = to_response(&config);
    drop(config);
    Ok(Json(resp))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::test_support::{test_state, test_state_with_config_path};
    use crate::config::{Config, NodeConfig};
    use axum::extract::State;
    use pulpo_common::api::WebhookEndpointUpdateRequest;

    #[tokio::test]
    async fn test_get_notifications_empty() {
        let state = test_state().await;
        let Json(resp) = get_notifications(State(state)).await.unwrap();
        assert!(resp.webhooks.is_empty());
    }

    #[tokio::test]
    async fn test_update_notifications_set_webhooks() {
        let state = test_state().await;
        let req = UpdateNotificationsRequest {
            webhooks: Some(vec![
                WebhookEndpointUpdateRequest {
                    name: "ci-hook".into(),
                    url: "https://example.com/hook".into(),
                    events: vec!["ready".into()],
                    min_severity: None,
                    secret: Some("s3cret".into()),
                },
                WebhookEndpointUpdateRequest {
                    name: "logs-hook".into(),
                    url: "https://logs.example.com".into(),
                    events: vec![],
                    min_severity: None,
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
            webhooks: Some(vec![WebhookEndpointUpdateRequest {
                name: "old".into(),
                url: "https://old.com".into(),
                events: vec![],
                min_severity: None,
                secret: None,
            }]),
        };
        let _ = update_notifications(State(state.clone()), Json(req))
            .await
            .unwrap();
        // Replace
        let req = UpdateNotificationsRequest {
            webhooks: Some(vec![WebhookEndpointUpdateRequest {
                name: "new".into(),
                url: "https://new.com".into(),
                events: vec!["killed".into()],
                min_severity: None,
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
            webhooks: Some(vec![WebhookEndpointUpdateRequest {
                name: "hook".into(),
                url: "https://a.com".into(),
                events: vec![],
                min_severity: None,
                secret: None,
            }]),
        };
        let _ = update_notifications(State(state.clone()), Json(req))
            .await
            .unwrap();
        // Clear
        let req = UpdateNotificationsRequest {
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
        assert!(resp.webhooks.is_empty());
    }

    #[tokio::test]
    async fn test_update_notifications_saves_to_disk() {
        let state = test_state_with_config_path().await;
        let req = UpdateNotificationsRequest {
            webhooks: Some(vec![WebhookEndpointUpdateRequest {
                name: "save-hook".into(),
                url: "https://example.com/save".into(),
                events: vec!["active".into()],
                min_severity: None,
                secret: None,
            }]),
        };
        let _ = update_notifications(State(state.clone()), Json(req))
            .await
            .unwrap();
        let loaded = crate::config::load(state.config_path.to_str().unwrap()).unwrap();
        // Updates write the canonical top-level `[[webhooks]]` list.
        assert_eq!(loaded.webhooks.len(), 1);
        assert_eq!(loaded.webhooks[0].url, "https://example.com/save");
    }

    #[test]
    fn test_to_response_with_all() {
        // Top-level canonical endpoint plus a legacy one — the response unions both.
        let config = Config {
            node: NodeConfig::default(),
            notifications: crate::config::NotificationsConfig {
                webhooks: vec![crate::config::WebhookEndpointConfig {
                    name: "legacy".into(),
                    url: "https://legacy.com".into(),
                    events: vec![],
                    min_severity: None,
                    secret: None,
                }],
                ..Default::default()
            },
            webhooks: vec![crate::config::WebhookEndpointConfig {
                name: "hook".into(),
                url: "https://hook.com".into(),
                events: vec![],
                min_severity: Some("warn".into()),
                secret: Some("key".into()),
            }],
            ..Default::default()
        };
        let resp = to_response(&config);
        assert_eq!(resp.webhooks.len(), 2);
        // Top-level endpoint comes first.
        assert_eq!(resp.webhooks[0].name, "hook");
        assert!(resp.webhooks[0].has_secret);
        assert_eq!(resp.webhooks[0].min_severity.as_deref(), Some("warn"));
        assert_eq!(resp.webhooks[1].name, "legacy");
    }
}
