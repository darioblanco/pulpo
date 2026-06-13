use hmac::{Hmac, Mac};
use pulpo_common::event::Event;
use sha2::Sha256;
use tracing::{error, info};

use crate::config::WebhookEndpointConfig;

/// Emits the canonical [`Event`] envelope to a configured webhook endpoint.
///
/// Posts the locked webhook message contract (see ROADMAP "Webhook message
/// contract"): signed JSON body plus `X-Pulpo-*` routing headers. Lifecycle and
/// usage-alert events both flow here; the universal `type.subtype`/severity
/// routing arrives in a later step.
pub struct WebhookSink {
    config: WebhookEndpointConfig,
    client: reqwest::Client,
}

/// Compute the `X-Pulpo-Signature` value: `sha256=<hex HMAC-SHA256(body, secret)>`.
///
/// Matches the contrib example consumer, which recomputes the HMAC over the raw
/// request body and compares constant-time.
pub fn compute_signature(secret: &str, body: &[u8]) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(body);
    let result = mac.finalize();
    format!("sha256={}", hex::encode(result.into_bytes()))
}

/// Returns whether the endpoint's status filter admits the given lifecycle status.
///
/// An empty filter admits everything. The filter only applies to `lifecycle`
/// events — usage alerts are always admitted (see [`WebhookSink::wants`]).
pub fn should_notify(config: &WebhookEndpointConfig, status: &str) -> bool {
    config.events.is_empty() || config.events.iter().any(|e| e == status)
}

impl WebhookSink {
    /// Create a new `WebhookSink` from config.
    pub fn new(config: WebhookEndpointConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    /// Sink name (the endpoint's configured name).
    pub fn name(&self) -> &str {
        &self.config.name
    }

    /// Whether this endpoint wants the given canonical event.
    ///
    /// Lifecycle events respect the per-endpoint status filter (mapped onto the
    /// event subtype). Usage alerts are always delivered so cost/budget alerts
    /// reach webhooks. Other event types pass through (no filter built yet).
    pub fn wants(&self, event: &Event) -> bool {
        match event.event_type.as_str() {
            "lifecycle" => should_notify(&self.config, &event.subtype),
            // "usage_alert" and any future type pass through; per-endpoint
            // type.subtype/severity routing is a later step.
            _ => true,
        }
    }

    /// POST the canonical [`Event`] JSON to the endpoint. Best-effort; logs on failure.
    pub async fn deliver(&self, event: &Event) {
        if let Err(e) = self.send(event).await {
            error!(
                webhook = %self.config.name,
                error = %e,
                "Webhook delivery failed"
            );
        }
    }

    /// Send the canonical event to the webhook endpoint.
    async fn send(&self, event: &Event) -> Result<(), reqwest::Error> {
        let body = serde_json::to_vec(event).unwrap_or_default();
        let event_header = format!("{}.{}", event.event_type, event.subtype);

        info!(
            webhook = %self.config.name,
            event = %event_header,
            severity = %event.severity,
            "Sending webhook notification"
        );

        let mut req = self
            .client
            .post(&self.config.url)
            .header("Content-Type", "application/json")
            .header("User-Agent", concat!("pulpo/", env!("CARGO_PKG_VERSION")))
            .header("X-Pulpo-Event", &event_header)
            .header("X-Pulpo-Event-Id", &event.event_id);

        if let Some(secret) = &self.config.secret {
            let sig = compute_signature(secret, &body);
            req = req.header("X-Pulpo-Signature", sig);
        }

        req.body(body).send().await?.error_for_status()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulpo_common::event::{EventSessionRef, PulpoEvent};

    fn test_config() -> WebhookEndpointConfig {
        WebhookEndpointConfig {
            name: "test-hook".into(),
            url: "https://example.com/hook".into(),
            events: vec![],
            secret: None,
        }
    }

    fn lifecycle_event(subtype: &str) -> Event {
        Event {
            schema_version: 1,
            event_id: "evt-1".into(),
            event_type: "lifecycle".into(),
            subtype: subtype.into(),
            severity: "info".into(),
            occurred_at: "2026-06-13T12:00:00Z".into(),
            node: "node-1".into(),
            session: Some(EventSessionRef {
                id: "abc-123".into(),
                name: "my-session".into(),
                status: subtype.into(),
                ..Default::default()
            }),
            payload: serde_json::json!({}),
        }
    }

    fn usage_alert_event() -> Event {
        Event::from_pulpo_event(
            &PulpoEvent::UsageAlert(pulpo_common::event::UsageAlertEvent {
                session_id: "s".into(),
                session_name: "n".into(),
                node_name: "x".into(),
                alert_kind: "budget_threshold".into(),
                message: "m".into(),
                cost_usd: Some(0.85),
                budget_usd: Some(1.0),
                timestamp: "2026-06-13T12:00:00Z".into(),
            }),
            "node-1",
        )
        .unwrap()
    }

    // --- compute_signature tests ---

    #[test]
    fn test_compute_signature_format() {
        let sig = compute_signature("my-secret", b"hello");
        assert!(sig.starts_with("sha256="));
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

    #[test]
    fn test_compute_signature_known_vector() {
        // HMAC-SHA256("hello", key="key") — fixed reference value (lowercase hex).
        let sig = compute_signature("key", b"hello");
        assert_eq!(
            sig,
            "sha256=9307b3b915efb5171ff14d8cb55fbcc798c6c0ef1456d66ded1a6aa723a58b7b"
        );
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

    // --- wants tests ---

    #[test]
    fn test_wants_lifecycle_respects_filter() {
        let sink = WebhookSink::new(WebhookEndpointConfig {
            events: vec!["stopped".into()],
            ..test_config()
        });
        assert!(!sink.wants(&lifecycle_event("active")));
        assert!(sink.wants(&lifecycle_event("stopped")));
    }

    #[test]
    fn test_wants_lifecycle_empty_filter() {
        let sink = WebhookSink::new(test_config());
        assert!(sink.wants(&lifecycle_event("active")));
    }

    #[test]
    fn test_wants_usage_alert_always() {
        // Even with a status filter, usage alerts must reach the webhook.
        let sink = WebhookSink::new(WebhookEndpointConfig {
            events: vec!["stopped".into()],
            ..test_config()
        });
        assert!(sink.wants(&usage_alert_event()));
    }

    #[test]
    fn test_wants_unknown_type_passes() {
        let sink = WebhookSink::new(WebhookEndpointConfig {
            events: vec!["stopped".into()],
            ..test_config()
        });
        let mut event = lifecycle_event("node_down");
        event.event_type = "fleet".into();
        assert!(sink.wants(&event));
    }

    // --- WebhookSink basics ---

    #[test]
    fn test_sink_new_and_name() {
        let sink = WebhookSink::new(test_config());
        assert_eq!(sink.name(), "test-hook");
        assert_eq!(sink.config.url, "https://example.com/hook");
    }

    // --- send / deliver tests ---

    use std::sync::Arc;
    use tokio::sync::Mutex;

    type CapturedRequest = (Vec<(String, String)>, String);

    async fn capture_server() -> (String, Arc<Mutex<Vec<CapturedRequest>>>) {
        let captured: Arc<Mutex<Vec<CapturedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = captured.clone();

        let app = axum::Router::new().route(
            "/hook",
            axum::routing::post(
                move |headers: axum::http::HeaderMap, body: String| async move {
                    let mut hdrs = Vec::new();
                    for (k, v) in &headers {
                        hdrs.push((k.to_string(), v.to_str().unwrap_or("").to_string()));
                    }
                    captured_clone.lock().await.push((hdrs, body));
                    axum::http::StatusCode::OK
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(axum::serve(listener, app).into_future());
        (format!("http://{addr}/hook"), captured)
    }

    /// Snapshot the captured requests, releasing the guard before assertions.
    async fn captured_requests(
        captured: &Arc<Mutex<Vec<CapturedRequest>>>,
    ) -> Vec<CapturedRequest> {
        captured.lock().await.clone()
    }

    #[tokio::test]
    async fn test_send_success_without_secret_posts_envelope() {
        let (url, captured) = capture_server().await;
        let sink = WebhookSink::new(WebhookEndpointConfig {
            url,
            ..test_config()
        });
        sink.send(&lifecycle_event("active")).await.unwrap();

        let reqs = captured_requests(&captured).await;
        let (headers, body) = &reqs[0];
        // Canonical envelope body.
        let json: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(json["type"], "lifecycle");
        assert_eq!(json["subtype"], "active");
        assert_eq!(json["session"]["name"], "my-session");
        // Routing headers.
        assert!(
            headers
                .iter()
                .any(|(k, v)| k == "x-pulpo-event" && v == "lifecycle.active")
        );
        assert!(
            headers
                .iter()
                .any(|(k, v)| k == "x-pulpo-event-id" && v == "evt-1")
        );
        assert!(
            headers
                .iter()
                .any(|(k, v)| k == "user-agent" && v.starts_with("pulpo/"))
        );
        assert!(!headers.iter().any(|(k, _)| k == "x-pulpo-signature"));
    }

    #[tokio::test]
    async fn test_send_success_with_secret_signs_body() {
        let (url, captured) = capture_server().await;
        let sink = WebhookSink::new(WebhookEndpointConfig {
            url,
            secret: Some("my-secret".into()),
            ..test_config()
        });
        sink.send(&lifecycle_event("active")).await.unwrap();

        let reqs = captured_requests(&captured).await;
        let (headers, body) = &reqs[0];
        let sig = headers
            .iter()
            .find(|(k, _)| k == "x-pulpo-signature")
            .map(|(_, v)| v.clone())
            .expect("missing signature header");
        // Signature must verify against the exact raw body the server received.
        let expected = compute_signature("my-secret", body.as_bytes());
        assert_eq!(sig, expected);
        assert!(sig.starts_with("sha256="));
    }

    #[tokio::test]
    async fn test_send_usage_alert_envelope() {
        let (url, captured) = capture_server().await;
        let sink = WebhookSink::new(WebhookEndpointConfig {
            url,
            ..test_config()
        });
        sink.send(&usage_alert_event()).await.unwrap();

        let reqs = captured_requests(&captured).await;
        let (headers, body) = &reqs[0];
        let json: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(json["type"], "usage_alert");
        assert_eq!(json["subtype"], "budget_threshold");
        assert_eq!(json["payload"]["cost_usd"], 0.85);
        assert!(
            headers
                .iter()
                .any(|(k, v)| k == "x-pulpo-event" && v == "usage_alert.budget_threshold")
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

        let sink = WebhookSink::new(WebhookEndpointConfig {
            url: format!("http://{addr}/hook"),
            ..test_config()
        });
        assert!(sink.send(&lifecycle_event("active")).await.is_err());
    }

    #[tokio::test]
    async fn test_deliver_logs_on_failure() {
        // Unreachable endpoint — deliver swallows the error (best-effort).
        let sink = WebhookSink::new(WebhookEndpointConfig {
            url: "http://127.0.0.1:1/hook".into(),
            ..test_config()
        });
        sink.deliver(&lifecycle_event("active")).await;
    }

    #[tokio::test]
    async fn test_deliver_success() {
        let (url, captured) = capture_server().await;
        let sink = WebhookSink::new(WebhookEndpointConfig {
            url,
            ..test_config()
        });
        sink.deliver(&lifecycle_event("stopped")).await;
        assert_eq!(captured_requests(&captured).await.len(), 1);
    }
}
