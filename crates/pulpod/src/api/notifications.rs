use std::sync::Arc;

use axum::{Json, extract::State};
use pulpo_common::api::{NotificationsConfigResponse, WebhookEndpointConfigResponse};

use crate::api::error::ApiError;

/// Mask the secret-bearing tail of a webhook URL for display: keep the
/// scheme, host (authority), and first path segment (usually the provider
/// name, e.g. Slack's `/services` or Discord's `/api`), replace everything
/// after that with `***`. Slack/Discord/etc. webhook URLs embed their secret
/// directly in the path; outbound delivery failures already drop the URL
/// entirely from logs (`reqwest::Error::without_url`) — this endpoint is
/// reachable by anyone with API access, so it gets the same protection while
/// staying useful enough to tell *which* configured webhook is which.
fn mask_webhook_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        // Not a recognizable absolute URL — mask it all rather than risk
        // leaking a secret we can't safely parse around.
        return "***".to_owned();
    };

    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);

    let Some(path) = tail.strip_prefix('/') else {
        // `tail` is either empty (bare `scheme://host`, nothing to mask) or a
        // query/fragment directly on the authority with no path at all.
        return if tail.is_empty() {
            format!("{scheme}://{authority}")
        } else {
            format!("{scheme}://{authority}/***")
        };
    };

    let seg_end = path.find(['/', '?', '#']).unwrap_or(path.len());
    let (first_segment, remainder) = path.split_at(seg_end);
    if remainder.is_empty() {
        format!("{scheme}://{authority}/{first_segment}")
    } else {
        format!("{scheme}://{authority}/{first_segment}/***")
    }
}

fn to_response(config: &crate::config::Config) -> NotificationsConfigResponse {
    NotificationsConfigResponse {
        // Surface the full set of endpoints (canonical top-level `[[webhooks]]`
        // unioned with the deprecated `[notifications.webhooks]` form).
        webhooks: config
            .webhook_endpoints()
            .iter()
            .map(|w| WebhookEndpointConfigResponse {
                name: w.name.clone(),
                url: mask_webhook_url(&w.url),
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
                }],
            },
            webhooks: vec![crate::config::WebhookEndpointConfig {
                name: "hook".into(),
                url: "https://hook.com".into(),
                events: vec![],
                min_severity: Some("warn".into()),
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

    #[test]
    fn test_to_response_masks_secret_bearing_url() {
        let config = Config {
            node: NodeConfig::default(),
            webhooks: vec![crate::config::WebhookEndpointConfig {
                name: "slack".into(),
                url: "https://hooks.slack.com/services/T000/B000/XXXXXXXXXXXXXXXXXXXXXXXX".into(),
                events: vec![],
                min_severity: None,
            }],
            ..Default::default()
        };
        let resp = to_response(&config);
        assert_eq!(resp.webhooks[0].url, "https://hooks.slack.com/services/***");
        assert!(!resp.webhooks[0].url.contains("XXXXXXXXXXXXXXXXXXXXXXXX"));
    }

    #[test]
    fn test_mask_webhook_url_slack_style() {
        assert_eq!(
            mask_webhook_url("https://hooks.slack.com/services/T000/B000/SECRET"),
            "https://hooks.slack.com/services/***"
        );
    }

    #[test]
    fn test_mask_webhook_url_discord_style() {
        assert_eq!(
            mask_webhook_url("https://discord.com/api/webhooks/12345/secrettoken"),
            "https://discord.com/api/***"
        );
    }

    #[test]
    fn test_mask_webhook_url_single_segment_unchanged() {
        // Nothing after the first (only) path segment — nothing to hide.
        assert_eq!(
            mask_webhook_url("https://example.com/hook"),
            "https://example.com/hook"
        );
    }

    #[test]
    fn test_mask_webhook_url_no_path_unchanged() {
        assert_eq!(
            mask_webhook_url("https://logs.example.com"),
            "https://logs.example.com"
        );
    }

    #[test]
    fn test_mask_webhook_url_query_on_bare_authority_masked() {
        assert_eq!(
            mask_webhook_url("https://example.com?token=secret"),
            "https://example.com/***"
        );
    }

    #[test]
    fn test_mask_webhook_url_query_after_segment_masked() {
        assert_eq!(
            mask_webhook_url("https://example.com/hook?token=secret"),
            "https://example.com/hook/***"
        );
    }

    #[test]
    fn test_mask_webhook_url_not_a_url_masks_everything() {
        assert_eq!(mask_webhook_url("not-a-url"), "***");
    }

    #[test]
    fn test_mask_webhook_url_preserves_port() {
        assert_eq!(
            mask_webhook_url("http://127.0.0.1:9000/secret-path/abc"),
            "http://127.0.0.1:9000/secret-path/***"
        );
    }
}
