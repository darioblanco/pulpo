use hmac::{Hmac, Mac};
use pulpo_common::event::{PulpoEvent, SessionEvent};
use sha2::Sha256;
use tracing::{error, info};

use crate::config::WebhookEndpointConfig;

/// Sends generic webhook notifications for session events.
pub struct WebhookNotifier {
    config: WebhookEndpointConfig,
    client: reqwest::Client,
}

/// Build the JSON payload for a generic webhook — just the `SessionEvent` as-is.
pub fn build_webhook_payload(event: &SessionEvent) -> serde_json::Value {
    serde_json::to_value(event).unwrap_or_default()
}

/// Compute HMAC-SHA256 signature for a payload body.
pub fn compute_signature(secret: &str, body: &[u8]) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(body);
    let result = mac.finalize();
    format!("sha256={}", hex::encode(result.into_bytes()))
}

/// Returns whether the notifier should send a notification for the given status.
pub fn should_notify(config: &WebhookEndpointConfig, status: &str) -> bool {
    config.events.is_empty() || config.events.iter().any(|e| e == status)
}

impl WebhookNotifier {
    /// Create a new `WebhookNotifier` from config.
    pub fn new(config: WebhookEndpointConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    /// Send a session event to the webhook endpoint.
    pub async fn send(&self, event: &SessionEvent) -> Result<(), reqwest::Error> {
        let payload = build_webhook_payload(event);
        let body = serde_json::to_vec(&payload).unwrap_or_default();

        info!(
            webhook = %self.config.name,
            session = %event.session_name,
            status = %event.status,
            "Sending webhook notification"
        );

        let mut req = self
            .client
            .post(&self.config.url)
            .header("Content-Type", "application/json")
            .header("X-Pulpo-Event", &event.status);

        if let Some(secret) = &self.config.secret {
            let sig = compute_signature(secret, &body);
            req = req.header("X-Pulpo-Signature", sig);
        }

        req.body(body).send().await?.error_for_status()?;
        Ok(())
    }
}

/// Run the notification loop for a generic webhook endpoint.
pub async fn run_notification_loop(
    notifier: WebhookNotifier,
    mut rx: tokio::sync::broadcast::Receiver<PulpoEvent>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event) => match event {
                        PulpoEvent::Session(ref se) => {
                            if should_notify(&notifier.config, &se.status)
                                && let Err(e) = notifier.send(se).await
                            {
                                error!(
                                    webhook = %notifier.config.name,
                                    error = %e,
                                    "Webhook notification failed"
                                );
                            }
                        }

                    },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(
                            webhook = %notifier.config.name,
                            missed = n,
                            "Webhook notifier lagged, skipping events"
                        );
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!(
                            webhook = %notifier.config.name,
                            "Event bus closed, stopping webhook notifier"
                        );
                        break;
                    }
                }
            }
            _ = shutdown.changed() => {
                info!(webhook = %notifier.config.name, "Webhook notifier shutting down");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_event(status: &str) -> SessionEvent {
        SessionEvent {
            session_id: "abc-123".into(),
            session_name: "my-session".into(),
            status: status.into(),
            previous_status: None,
            node_name: "node-1".into(),
            output_snippet: None,
            timestamp: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn test_config() -> WebhookEndpointConfig {
        WebhookEndpointConfig {
            name: "test-hook".into(),
            url: "https://example.com/hook".into(),
            events: vec![],
            secret: None,
        }
    }

    // --- build_webhook_payload tests ---

    #[test]
    fn test_build_payload_contains_all_fields() {
        let event = test_event("active");
        let payload = build_webhook_payload(&event);
        assert_eq!(payload["session_id"], "abc-123");
        assert_eq!(payload["session_name"], "my-session");
        assert_eq!(payload["status"], "active");
        assert_eq!(payload["node_name"], "node-1");
        assert_eq!(payload["timestamp"], "2026-01-01T00:00:00Z");
    }

    #[test]
    fn test_build_payload_with_optionals() {
        let event = SessionEvent {
            previous_status: Some("lost".into()),
            output_snippet: Some("hello".into()),
            ..test_event("active")
        };
        let payload = build_webhook_payload(&event);
        assert_eq!(payload["previous_status"], "lost");
        assert_eq!(payload["output_snippet"], "hello");
    }

    // --- compute_signature tests ---

    #[test]
    fn test_compute_signature_format() {
        let sig = compute_signature("my-secret", b"hello");
        assert!(sig.starts_with("sha256="));
        // Known HMAC-SHA256 of "hello" with key "my-secret"
        assert_eq!(sig.len(), 7 + 64); // "sha256=" + 64 hex chars
    }

    #[test]
    fn test_compute_signature_deterministic() {
        let sig1 = compute_signature("key", b"body");
        let sig2 = compute_signature("key", b"body");
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn test_compute_signature_different_keys() {
        let sig1 = compute_signature("key1", b"body");
        let sig2 = compute_signature("key2", b"body");
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn test_compute_signature_different_bodies() {
        let sig1 = compute_signature("key", b"body1");
        let sig2 = compute_signature("key", b"body2");
        assert_ne!(sig1, sig2);
    }

    // --- should_notify tests ---

    #[test]
    fn test_should_notify_empty_filter_allows_all() {
        let config = test_config();
        assert!(should_notify(&config, "active"));
        assert!(should_notify(&config, "stopped"));
    }

    #[test]
    fn test_should_notify_with_filter() {
        let config = WebhookEndpointConfig {
            events: vec!["ready".into(), "stopped".into()],
            ..test_config()
        };
        assert!(!should_notify(&config, "active"));
        assert!(should_notify(&config, "stopped"));
        assert!(should_notify(&config, "ready"));
    }

    // --- WebhookNotifier tests ---

    #[test]
    fn test_notifier_new() {
        let notifier = WebhookNotifier::new(test_config());
        assert_eq!(notifier.config.name, "test-hook");
        assert_eq!(notifier.config.url, "https://example.com/hook");
    }

    // --- send tests ---

    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_send_success_without_secret() {
        let captured_headers: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let headers_clone = captured_headers.clone();

        let app = axum::Router::new().route(
            "/hook",
            axum::routing::post(move |headers: axum::http::HeaderMap| async move {
                let mut captured = headers_clone.lock().await;
                for (k, v) in &headers {
                    captured.push((k.to_string(), v.to_str().unwrap_or("").to_string()));
                }
                axum::http::StatusCode::OK
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(axum::serve(listener, app).into_future());

        let config = WebhookEndpointConfig {
            url: format!("http://{addr}/hook"),
            ..test_config()
        };
        let notifier = WebhookNotifier::new(config);
        let result = notifier.send(&test_event("active")).await;
        assert!(result.is_ok());

        let headers = captured_headers.lock().await;
        let has_event = headers
            .iter()
            .any(|(k, v)| k == "x-pulpo-event" && v == "active");
        let has_sig = headers.iter().any(|(k, _)| k == "x-pulpo-signature");
        drop(headers);
        assert!(has_event, "Missing X-Pulpo-Event header");
        assert!(!has_sig, "Should not have signature without secret");
    }

    #[tokio::test]
    async fn test_send_success_with_secret() {
        let captured_headers: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let headers_clone = captured_headers.clone();

        let app = axum::Router::new().route(
            "/hook",
            axum::routing::post(move |headers: axum::http::HeaderMap| async move {
                let mut captured = headers_clone.lock().await;
                for (k, v) in &headers {
                    captured.push((k.to_string(), v.to_str().unwrap_or("").to_string()));
                }
                axum::http::StatusCode::OK
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(axum::serve(listener, app).into_future());

        let config = WebhookEndpointConfig {
            url: format!("http://{addr}/hook"),
            secret: Some("my-secret".into()),
            ..test_config()
        };
        let notifier = WebhookNotifier::new(config);
        let result = notifier.send(&test_event("active")).await;
        assert!(result.is_ok());

        let sig = captured_headers
            .lock()
            .await
            .iter()
            .find(|(k, _)| k == "x-pulpo-signature")
            .map(|(_, v)| v.clone());
        assert!(sig.is_some(), "Missing X-Pulpo-Signature header");
        assert!(
            sig.unwrap().starts_with("sha256="),
            "Signature should start with sha256="
        );
    }

    #[tokio::test]
    async fn test_send_error_for_status() {
        let app = axum::Router::new().route(
            "/hook",
            axum::routing::post(|| async { axum::http::StatusCode::BAD_REQUEST }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(axum::serve(listener, app).into_future());

        let config = WebhookEndpointConfig {
            url: format!("http://{addr}/hook"),
            ..test_config()
        };
        let notifier = WebhookNotifier::new(config);
        let result = notifier.send(&test_event("active")).await;
        assert!(result.is_err());
    }

    // --- run_notification_loop tests ---

    #[tokio::test]
    async fn test_notification_loop_shutdown() {
        let notifier = WebhookNotifier::new(test_config());
        let (event_tx, _) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let rx = event_tx.subscribe();
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        shutdown_tx.send(true).unwrap();

        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            run_notification_loop(notifier, rx, shutdown_rx),
        )
        .await
        .expect("should exit on shutdown");
    }

    #[tokio::test]
    async fn test_notification_loop_channel_closed() {
        let notifier = WebhookNotifier::new(test_config());
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (_shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        drop(event_tx);

        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            run_notification_loop(notifier, rx, shutdown_rx),
        )
        .await
        .expect("should exit when channel closes");
    }

    #[tokio::test]
    async fn test_notification_loop_filtered_event() {
        let config = WebhookEndpointConfig {
            events: vec!["stopped".into()],
            ..test_config()
        };
        let notifier = WebhookNotifier::new(config);
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        event_tx
            .send(PulpoEvent::Session(test_event("active")))
            .unwrap();

        let handle = tokio::spawn(run_notification_loop(notifier, rx, shutdown_rx));

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        shutdown_tx.send(true).unwrap();

        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("should finish")
            .expect("should not panic");
    }

    #[tokio::test]
    async fn test_notification_loop_send_error() {
        let config = WebhookEndpointConfig {
            url: "http://127.0.0.1:1/hook".into(),
            ..test_config()
        };
        let notifier = WebhookNotifier::new(config);
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        event_tx
            .send(PulpoEvent::Session(test_event("active")))
            .unwrap();

        let handle = tokio::spawn(run_notification_loop(notifier, rx, shutdown_rx));

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        shutdown_tx.send(true).unwrap();

        tokio::time::timeout(std::time::Duration::from_secs(5), handle)
            .await
            .expect("should finish")
            .expect("should not panic");
    }

    #[tokio::test]
    async fn test_notification_loop_lagged() {
        let config = WebhookEndpointConfig {
            events: vec!["stopped".into()],
            ..test_config()
        };
        let notifier = WebhookNotifier::new(config);
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(1);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        for i in 0..5 {
            let _ = event_tx.send(PulpoEvent::Session(SessionEvent {
                session_id: format!("id-{i}"),
                session_name: "s".into(),
                status: "active".into(),
                previous_status: None,
                node_name: "n".into(),
                output_snippet: None,
                timestamp: "t".into(),
            }));
        }

        let handle = tokio::spawn(run_notification_loop(notifier, rx, shutdown_rx));

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        shutdown_tx.send(true).unwrap();

        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("should finish")
            .expect("should not panic");
    }
}
