use std::sync::Arc;

use axum::{Json, extract::State};
use pulpo_common::api::{NotificationsConfigResponse, WebhookEndpointConfigResponse};

use crate::api::error::ApiError;

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
            })
            .collect(),
    }
}

/// Read-only view of pulpod's effective notification configuration. The config file
/// (`~/.pulpo/config.toml`) is the source of truth — editing happens by hand-editing
/// the file and restarting pulpod.
pub async fn get_notifications(
    State(state): State<Arc<super::AppState>>,
) -> Result<Json<NotificationsConfigResponse>, ApiError> {
    let config = state.config.read().await;
    let resp = to_response(&config);
    drop(config);
    Ok(Json(resp))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::test_support::test_state;
    use crate::config::{Config, NodeConfig};

    #[tokio::test]
    async fn test_get_notifications_empty() {
        let state = test_state().await;
        let Json(resp) = get_notifications(State(state)).await.unwrap();
        assert!(resp.webhooks.is_empty());
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
        assert_eq!(resp.webhooks[0].min_severity.as_deref(), Some("warn"));
        assert_eq!(resp.webhooks[1].name, "legacy");
    }
}
