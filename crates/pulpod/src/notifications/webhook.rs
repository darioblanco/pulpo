use std::time::Duration;

use pulpo_common::event::Event;
use tracing::{error, info, warn};

use crate::config::{WebhookEndpointConfig, glob_match, severity_at_least};

/// Delays between retries, in order: the initial attempt plus these three
/// (~1s, 3s, 9s — roughly tripling backoff) before giving up. There is no
/// persistence: an event that exhausts every attempt is logged and dropped,
/// never retried again (this replaced the durable SQLite outbox).
pub const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(3),
    Duration::from_secs(9),
];

/// Connect timeout for outbound webhook requests: an endpoint that never
/// completes a TCP/TLS handshake must not hang a delivery task (and, since
/// deliveries share a bounded pool of concurrent slots, everything behind it)
/// forever.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Total per-attempt timeout (connect + send + response) for outbound webhook
/// requests. Bounds the same failure mode as [`CONNECT_TIMEOUT`] but for an
/// endpoint that accepts the connection and then never responds.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Build the `reqwest::Client` shared by every webhook delivery for the
/// dispatcher's lifetime. `reqwest::Client::new()` has no timeouts at all, so an
/// unresponsive endpoint would otherwise hang its delivery task (and hold its
/// concurrency permit — see `notifications::MAX_CONCURRENT_DELIVERIES`)
/// indefinitely; every request through this client gives up after
/// [`CONNECT_TIMEOUT`]/[`REQUEST_TIMEOUT`] and moves on to the retry schedule
/// (or drops the event) instead.
pub fn build_client() -> reqwest::Client {
    build_client_with_timeouts(CONNECT_TIMEOUT, REQUEST_TIMEOUT)
}

fn build_client_with_timeouts(connect: Duration, total: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(connect)
        .timeout(total)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// Whether an endpoint config wants the given canonical event.
///
/// Applies the universal routing filter uniformly to **every** event type:
/// 1. the event's `severity` must clear the endpoint's `min_severity` floor
///    (`info` < `warn` < `critical`; absent ⇒ no floor), and
/// 2. its `"<type>.<subtype>"` key must match one of the endpoint's `events`
///    globs (an empty/absent `events` list matches all).
#[must_use]
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

/// Build the plain webhook `POST` request for a raw envelope body.
///
/// `body` is the exact bytes posted; `event_header` is the `X-Pulpo-Event`
/// value (`<type>.<subtype>`); `event_id` is the idempotency key a receiver
/// can dedupe on (stable across retries of the same event).
fn build_webhook_request(
    client: &reqwest::Client,
    config: &WebhookEndpointConfig,
    body: Vec<u8>,
    event_header: &str,
    event_id: &str,
) -> reqwest::RequestBuilder {
    client
        .post(&config.url)
        .header("Content-Type", "application/json")
        .header("User-Agent", concat!("pulpo/", env!("CARGO_PKG_VERSION")))
        .header("X-Pulpo-Event", event_header)
        .header("X-Pulpo-Event-Id", event_id)
        .body(body)
}

/// POST `event` to `config`'s URL, retrying per [`RETRY_DELAYS`] on failure.
///
/// Best-effort and in-memory only: never panics, and when every attempt
/// fails the event is logged and dropped rather than persisted for a later
/// retry.
pub async fn deliver(client: &reqwest::Client, config: &WebhookEndpointConfig, event: &Event) {
    deliver_with_delays(client, config, event, &RETRY_DELAYS).await;
}

/// Core of [`deliver`], parameterized by the retry delays so tests can use a
/// near-zero schedule instead of waiting on the real 1s/3s/9s timers.
async fn deliver_with_delays(
    client: &reqwest::Client,
    config: &WebhookEndpointConfig,
    event: &Event,
    delays: &[Duration],
) {
    let body = serde_json::to_vec(event).unwrap_or_default();
    let event_header = format!("{}.{}", event.event_type, event.subtype);
    let total_attempts = delays.len() + 1;

    for attempt in 1..=total_attempts {
        let result =
            build_webhook_request(client, config, body.clone(), &event_header, &event.event_id)
                .send()
                .await
                .and_then(reqwest::Response::error_for_status);

        match result {
            Ok(_) => {
                if attempt > 1 {
                    info!(
                        webhook = %config.name,
                        event = %event_header,
                        attempt,
                        "Webhook delivered after retry"
                    );
                }
                return;
            }
            // `reqwest::Error`'s `Display` appends the request URL, and a Slack/Discord
            // webhook URL embeds its secret in the path — `.without_url()` drops it
            // before the error reaches the log file.
            Err(e) if attempt == total_attempts => {
                error!(
                    webhook = %config.name,
                    event = %event_header,
                    attempts = attempt,
                    error = %e.without_url(),
                    "Webhook delivery failed after all retries, dropping event"
                );
                return;
            }
            Err(e) => {
                warn!(
                    webhook = %config.name,
                    event = %event_header,
                    attempt,
                    error = %e.without_url(),
                    "Webhook delivery attempt failed, retrying"
                );
                tokio::time::sleep(delays[attempt - 1]).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulpo_common::event::{EventSessionRef, PulpoEvent};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn test_config(url: &str) -> WebhookEndpointConfig {
        WebhookEndpointConfig {
            name: "test-hook".into(),
            url: url.into(),
            events: vec![],
            min_severity: None,
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

    fn event_with(event_type: &str, subtype: &str, severity: &str) -> Event {
        let mut e = lifecycle_event(subtype);
        e.event_type = event_type.into();
        e.severity = severity.into();
        e
    }

    // --- webhook_wants: glob matching ---

    #[test]
    fn test_wants_empty_events_matches_all() {
        let config = test_config("http://unused");
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
            ..test_config("http://unused")
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
            ..test_config("http://unused")
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
            ..test_config("http://unused")
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
            ..test_config("http://unused")
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
            ..test_config("http://unused")
        };
        assert!(webhook_wants(
            &config,
            &event_with("lifecycle", "lost", "critical")
        ));
        assert!(webhook_wants(
            &config,
            &event_with("usage_alert", "budget_threshold", "critical")
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
            ..test_config("http://unused")
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
            ..test_config("http://unused")
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
            ..test_config("http://unused")
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
        let config = WebhookEndpointConfig {
            events: vec!["lifecycle.*".into()],
            min_severity: Some("warn".into()),
            ..test_config("http://unused")
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

    #[test]
    fn test_wants_applies_to_all_types_uniformly() {
        let config = WebhookEndpointConfig {
            events: vec!["usage_alert.*".into()],
            ..test_config("http://unused")
        };
        assert!(webhook_wants(&config, &usage_alert_event()));
        assert!(!webhook_wants(&config, &lifecycle_event("active")));
    }

    // --- deliver / deliver_with_delays ---

    type CapturedRequest = (Vec<(String, String)>, String);

    /// A mock endpoint whose behavior is controlled by `fail_count`: the first
    /// `fail_count` requests get a 500, every request after that gets a 200.
    async fn capture_server(fail_count: usize) -> (String, Arc<Mutex<Vec<CapturedRequest>>>) {
        let captured: Arc<Mutex<Vec<CapturedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = captured.clone();
        let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let app = axum::Router::new().route(
            "/hook",
            axum::routing::post(move |headers: axum::http::HeaderMap, body: String| {
                let captured = captured_clone.clone();
                let seen = seen.clone();
                async move {
                    let mut hdrs = Vec::new();
                    for (k, v) in &headers {
                        hdrs.push((k.to_string(), v.to_str().unwrap_or("").to_string()));
                    }
                    captured.lock().await.push((hdrs, body));
                    let n = seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if n < fail_count {
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR
                    } else {
                        axum::http::StatusCode::OK
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(axum::serve(listener, app).into_future());
        (format!("http://{addr}/hook"), captured)
    }

    async fn captured_requests(
        captured: &Arc<Mutex<Vec<CapturedRequest>>>,
    ) -> Vec<CapturedRequest> {
        captured.lock().await.clone()
    }

    /// Delays used by tests: real logic, near-zero durations so tests run fast.
    const FAST_DELAYS: [Duration; 3] = [
        Duration::from_millis(1),
        Duration::from_millis(1),
        Duration::from_millis(1),
    ];

    #[tokio::test]
    async fn test_deliver_succeeds_first_try_posts_envelope() {
        let (url, captured) = capture_server(0).await;
        let config = test_config(&url);
        let client = reqwest::Client::new();

        deliver_with_delays(&client, &config, &lifecycle_event("active"), &FAST_DELAYS).await;

        let reqs = captured_requests(&captured).await;
        assert_eq!(reqs.len(), 1, "exactly one attempt when the first succeeds");
        let (headers, body) = &reqs[0];
        let json: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(json["type"], "lifecycle");
        assert_eq!(json["subtype"], "active");
        assert_eq!(json["session"]["name"], "my-session");
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
        // No signing header — HMAC signing was removed along with the outbox.
        assert!(!headers.iter().any(|(k, _)| k == "x-pulpo-signature"));
    }

    #[tokio::test]
    async fn test_deliver_retries_then_succeeds() {
        // Fails twice, then succeeds on the third (final) attempt.
        let (url, captured) = capture_server(2).await;
        let config = test_config(&url);
        let client = reqwest::Client::new();

        deliver_with_delays(&client, &config, &lifecycle_event("ready"), &FAST_DELAYS).await;

        assert_eq!(captured_requests(&captured).await.len(), 3);
    }

    #[tokio::test]
    async fn test_deliver_exhausts_retries_then_drops() {
        // Always fails: initial attempt + 3 retries = 4 total, then give up.
        let (url, captured) = capture_server(usize::MAX).await;
        let config = test_config(&url);
        let client = reqwest::Client::new();

        deliver_with_delays(&client, &config, &lifecycle_event("active"), &FAST_DELAYS).await;

        assert_eq!(
            captured_requests(&captured).await.len(),
            FAST_DELAYS.len() + 1
        );
    }

    #[tokio::test]
    async fn test_deliver_unreachable_endpoint_drops_without_panic() {
        // Connection-level failure (not just a non-2xx status) also retries then drops.
        let config = test_config("http://127.0.0.1:1/hook");
        let client = reqwest::Client::new();
        deliver_with_delays(&client, &config, &lifecycle_event("active"), &FAST_DELAYS).await;
    }

    #[tokio::test]
    async fn test_deliver_public_wrapper_uses_real_delays() {
        // Exercises the public `deliver` entry point (real RETRY_DELAYS) on the
        // fast, first-try-succeeds path so the test doesn't wait on real timers.
        let (url, captured) = capture_server(0).await;
        let config = test_config(&url);
        let client = reqwest::Client::new();

        deliver(&client, &config, &lifecycle_event("stopped")).await;

        assert_eq!(captured_requests(&captured).await.len(), 1);
    }

    #[test]
    fn test_retry_delays_are_one_three_nine_seconds() {
        assert_eq!(
            RETRY_DELAYS,
            [
                Duration::from_secs(1),
                Duration::from_secs(3),
                Duration::from_secs(9),
            ]
        );
    }

    // --- client timeouts (bounded delivery) ---

    #[test]
    fn test_documented_timeouts() {
        assert_eq!(CONNECT_TIMEOUT, Duration::from_secs(5));
        assert_eq!(REQUEST_TIMEOUT, Duration::from_secs(10));
    }

    #[test]
    fn test_build_client_constructs_successfully() {
        // Smoke test: `build_client` (the real entry point `notifications::mod` uses)
        // must actually succeed in building a client with the documented timeouts.
        let _ = build_client();
    }

    /// Accept TCP connections but never read/write/respond on them — simulates an
    /// endpoint that hangs instead of failing fast (a dropped/refused connection
    /// would already be handled by the existing retry-then-drop tests above).
    async fn hanging_server() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                if let Ok((stream, _)) = listener.accept().await {
                    tokio::spawn(async move {
                        let _stream = stream;
                        std::future::pending::<()>().await;
                    });
                }
            }
        });
        format!("http://{addr}/hook")
    }

    #[tokio::test]
    async fn test_deliver_gives_up_within_timeout_budget_against_hanging_endpoint() {
        // Before the fix, `reqwest::Client::new()` had no timeout at all, so a
        // request against an endpoint that accepts the connection and then never
        // responds would hang forever — `deliver_with_delays` would never return.
        // A client built with `build_client_with_timeouts` (what `build_client`
        // itself calls, just with short durations so the test doesn't wait on the
        // real 5s/10s production budget) must give up per attempt instead.
        let url = hanging_server().await;
        let config = test_config(&url);
        let client =
            build_client_with_timeouts(Duration::from_millis(100), Duration::from_millis(150));

        let result = tokio::time::timeout(
            Duration::from_secs(3),
            deliver_with_delays(&client, &config, &lifecycle_event("active"), &FAST_DELAYS),
        )
        .await;

        assert!(
            result.is_ok(),
            "delivery must give up within the client's own timeout budget, not hang forever"
        );
    }

    // --- URL redaction on failure (webhook URLs embed the secret) ---

    #[tokio::test]
    async fn test_deliver_failure_log_does_not_leak_url() {
        // The endpoint URL itself is the shared secret for Slack/Discord-style
        // webhooks (see docs/reference/config.md) — a failed delivery must not put
        // it in the log via `reqwest::Error`'s `Display` (which appends the URL).
        let secret_path = "T00000000/B00000000/super-secret-token-abc123";
        let config = WebhookEndpointConfig {
            events: vec![],
            min_severity: None,
            name: "leaky".into(),
            url: format!("http://127.0.0.1:1/{secret_path}"),
        };
        let client = reqwest::Client::new();

        // Connection refused on every attempt -> reaches the final error!() branch.
        deliver_with_delays(&client, &config, &lifecycle_event("active"), &FAST_DELAYS).await;

        // Build the same error `deliver_with_delays` would have logged and confirm
        // `without_url()` actually strips the secret from its `Display` output —
        // the behavior the fix relies on, checked directly against the redacted
        // error rather than by scraping log output.
        let err = client
            .get(&config.url)
            .send()
            .await
            .expect_err("connection to a closed port must fail");
        assert!(
            format!("{err}").contains(secret_path),
            "sanity check: the raw reqwest::Error must contain the URL"
        );
        assert!(
            !format!("{}", err.without_url()).contains(secret_path),
            "without_url() must strip the webhook URL/secret from the logged error"
        );
    }
}
