use hmac::{Hmac, Mac};
use pulpo_common::event::Event;
use sha2::Sha256;
use tracing::{error, info};

use crate::config::{WebhookEndpointConfig, glob_match, severity_at_least};

/// Emits the canonical [`Event`] envelope to a configured webhook endpoint.
///
/// Posts the locked webhook message contract (see ROADMAP "Webhook message
/// contract"): signed JSON body plus `X-Pulpo-*` routing headers. Every event
/// type (`lifecycle`, `intervention`, `usage_alert`, `fleet`) flows through the
/// same universal `<type>.<subtype>` glob + `min_severity` filter (see
/// [`webhook_wants`]).
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

/// Whether an endpoint config wants the given canonical event.
///
/// Applies the universal routing filter uniformly to **every** event type:
/// 1. the event's `severity` must clear the endpoint's `min_severity` floor
///    (`info` < `warn` < `critical`; absent ⇒ no floor), and
/// 2. its `"<type>.<subtype>"` key must match one of the endpoint's `events`
///    globs (an empty/absent `events` list matches all).
///
/// Free function so the dispatcher can filter by config without constructing a
/// [`WebhookSink`] (which holds a reqwest client). This is the single filtering
/// point — the outbox worker resolves stored rows by endpoint name and does not
/// re-filter.
pub fn webhook_wants(config: &WebhookEndpointConfig, event: &Event) -> bool {
    if !severity_at_least(&event.severity, config.min_severity.as_deref()) {
        return false;
    }
    if config.events.is_empty() {
        return true;
    }
    let event_key = format!("{}.{}", event.event_type, event.subtype);
    config
        .events
        .iter()
        .any(|pattern| glob_match(pattern, &event_key))
}

/// Build the signed webhook `POST` request for a raw envelope body.
///
/// Single source of truth for the on-the-wire contract: the same headers and
/// HMAC signing are used by the inline [`WebhookSink::send`] and the durable
/// outbox worker, so a stored envelope replays byte-for-byte identically to a
/// fresh send (and stays compatible with `contrib/examples/webhook-discord/`).
///
/// `body` is the exact bytes posted and signed; `event_header` is the
/// `X-Pulpo-Event` value (`<type>.<subtype>`); `event_id` is the idempotency key.
pub fn build_webhook_request(
    client: &reqwest::Client,
    config: &WebhookEndpointConfig,
    body: Vec<u8>,
    event_header: &str,
    event_id: &str,
) -> reqwest::RequestBuilder {
    let mut req = client
        .post(&config.url)
        .header("Content-Type", "application/json")
        .header("User-Agent", concat!("pulpo/", env!("CARGO_PKG_VERSION")))
        .header("X-Pulpo-Event", event_header)
        .header("X-Pulpo-Event-Id", event_id);

    if let Some(secret) = &config.secret {
        let sig = compute_signature(secret, &body);
        req = req.header("X-Pulpo-Signature", sig);
    }

    req.body(body)
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
    /// Delegates to [`webhook_wants`] so inline-send and dispatcher filtering
    /// share one rule (`<type>.<subtype>` globs + `min_severity`).
    pub fn wants(&self, event: &Event) -> bool {
        webhook_wants(&self.config, event)
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

        build_webhook_request(
            &self.client,
            &self.config,
            body,
            &event_header,
            &event.event_id,
        )
        .send()
        .await?
        .error_for_status()?;
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
            min_severity: None,
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

    fn event_with(event_type: &str, subtype: &str, severity: &str) -> Event {
        let mut e = lifecycle_event(subtype);
        e.event_type = event_type.into();
        e.severity = severity.into();
        e
    }

    // --- webhook_wants: glob matching (via the public filter) ---

    #[test]
    fn test_wants_empty_events_matches_all() {
        let config = test_config();
        assert!(webhook_wants(
            &config,
            &event_with("lifecycle", "idle", "info")
        ));
        assert!(webhook_wants(
            &config,
            &event_with("fleet", "node_down", "info")
        ));
    }

    #[test]
    fn test_wants_exact_match() {
        let config = WebhookEndpointConfig {
            events: vec!["lifecycle.idle".into()],
            ..test_config()
        };
        assert!(webhook_wants(
            &config,
            &event_with("lifecycle", "idle", "info")
        ));
        assert!(!webhook_wants(
            &config,
            &event_with("lifecycle", "active", "info")
        ));
    }

    #[test]
    fn test_wants_prefix_glob() {
        let config = WebhookEndpointConfig {
            events: vec!["usage_alert.*".into()],
            ..test_config()
        };
        assert!(webhook_wants(
            &config,
            &event_with("usage_alert", "budget_threshold", "warn")
        ));
        assert!(webhook_wants(
            &config,
            &event_with("usage_alert", "rate_limit", "warn")
        ));
        assert!(!webhook_wants(
            &config,
            &event_with("lifecycle", "idle", "warn")
        ));
    }

    #[test]
    fn test_wants_bare_type_matches_all_subtypes() {
        let config = WebhookEndpointConfig {
            events: vec!["intervention".into()],
            ..test_config()
        };
        assert!(webhook_wants(
            &config,
            &event_with("intervention", "idle_timeout", "warn")
        ));
        assert!(!webhook_wants(
            &config,
            &event_with("lifecycle", "idle", "warn")
        ));
    }

    #[test]
    fn test_wants_star_matches_everything() {
        let config = WebhookEndpointConfig {
            events: vec!["*".into()],
            ..test_config()
        };
        assert!(webhook_wants(
            &config,
            &event_with("lifecycle", "idle", "info")
        ));
        assert!(webhook_wants(
            &config,
            &event_with("fleet", "peer_unreachable", "critical")
        ));
    }

    #[test]
    fn test_wants_multiple_patterns_any_match() {
        let config = WebhookEndpointConfig {
            events: vec!["lifecycle.lost".into(), "usage_alert.*".into()],
            ..test_config()
        };
        assert!(webhook_wants(
            &config,
            &event_with("lifecycle", "lost", "critical")
        ));
        assert!(webhook_wants(
            &config,
            &event_with("usage_alert", "burn_ceiling", "critical")
        ));
        assert!(!webhook_wants(
            &config,
            &event_with("lifecycle", "idle", "warn")
        ));
    }

    #[test]
    fn test_wants_no_match_drops() {
        let config = WebhookEndpointConfig {
            events: vec!["lifecycle.idle".into()],
            ..test_config()
        };
        assert!(!webhook_wants(
            &config,
            &event_with("fleet", "node_down", "warn")
        ));
    }

    // --- webhook_wants: severity floor ---

    #[test]
    fn test_wants_min_severity_drops_below_floor() {
        let config = WebhookEndpointConfig {
            min_severity: Some("warn".into()),
            ..test_config()
        };
        assert!(!webhook_wants(
            &config,
            &event_with("lifecycle", "active", "info")
        ));
        assert!(webhook_wants(
            &config,
            &event_with("lifecycle", "idle", "warn")
        ));
        assert!(webhook_wants(
            &config,
            &event_with("lifecycle", "lost", "critical")
        ));
    }

    #[test]
    fn test_wants_min_severity_critical_only() {
        let config = WebhookEndpointConfig {
            min_severity: Some("critical".into()),
            ..test_config()
        };
        assert!(!webhook_wants(
            &config,
            &event_with("usage_alert", "budget_threshold", "warn")
        ));
        assert!(webhook_wants(
            &config,
            &event_with("lifecycle", "lost", "critical")
        ));
    }

    #[test]
    fn test_wants_severity_and_glob_combined() {
        // Both filters apply: pattern matches but severity below floor → dropped.
        let config = WebhookEndpointConfig {
            events: vec!["lifecycle.*".into()],
            min_severity: Some("warn".into()),
            ..test_config()
        };
        assert!(!webhook_wants(
            &config,
            &event_with("lifecycle", "active", "info")
        ));
        assert!(webhook_wants(
            &config,
            &event_with("lifecycle", "idle", "warn")
        ));
    }

    // --- wants (sink delegates to webhook_wants) ---

    #[test]
    fn test_sink_wants_applies_to_all_types_uniformly() {
        let sink = WebhookSink::new(WebhookEndpointConfig {
            events: vec!["usage_alert.*".into()],
            ..test_config()
        });
        // Usage alerts now obey the glob filter like every other type.
        assert!(sink.wants(&usage_alert_event()));
        assert!(!sink.wants(&lifecycle_event("active")));
    }

    #[test]
    fn test_sink_wants_empty_filter() {
        let sink = WebhookSink::new(test_config());
        assert!(sink.wants(&lifecycle_event("active")));
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
