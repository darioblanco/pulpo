pub mod web_push;
pub mod webhook;

use pulpo_common::event::{Event, PulpoEvent};
use tracing::{info, warn};

use self::web_push::WebPushSink;
use self::webhook::WebhookSink;

/// An event sink: a destination that receives canonical [`Event`]s.
///
/// Modeled as an enum rather than a `dyn` trait object to avoid pulling in
/// `async-trait` (async methods aren't object-safe on stable). Each variant
/// answers the same questions: its `name`, whether it `wants` an event
/// (filtering), and how to `deliver` it (best-effort, logs on failure).
pub enum EventSink {
    Webhook(WebhookSink),
    WebPush(WebPushSink),
}

impl EventSink {
    /// Human-readable sink name, for logging.
    pub fn name(&self) -> &str {
        match self {
            Self::Webhook(s) => s.name(),
            Self::WebPush(s) => s.name(),
        }
    }

    /// Whether this sink wants the given event (its filter admits it).
    pub fn wants(&self, event: &Event) -> bool {
        match self {
            Self::Webhook(s) => s.wants(event),
            Self::WebPush(s) => s.wants(event),
        }
    }

    /// Deliver the event to this sink. Best-effort: never panics, logs on failure.
    pub async fn deliver(&self, event: &Event) {
        match self {
            Self::Webhook(s) => s.deliver(event).await,
            Self::WebPush(s) => s.deliver(event).await,
        }
    }
}

/// Run the single event dispatcher loop.
///
/// Subscribes to the broadcast bus once, converts each [`PulpoEvent`] into the
/// canonical [`Event`] envelope (skipping events that aren't externally
/// forwarded), and fans it out to every sink whose `wants` admits it. Replaces
/// the former per-notifier loops (webhook + web-push). Delivery is best-effort;
/// the durable outbox + retries arrive in a later step.
pub async fn run_dispatcher_loop(
    sinks: Vec<EventSink>,
    node_name: String,
    mut rx: tokio::sync::broadcast::Receiver<PulpoEvent>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(pulpo_event) => {
                        let Some(event) = Event::from_pulpo_event(&pulpo_event, &node_name) else {
                            continue;
                        };
                        for sink in &sinks {
                            if sink.wants(&event) {
                                sink.deliver(&event).await;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(missed = n, "Event dispatcher lagged, skipping events");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("Event bus closed, stopping event dispatcher");
                        break;
                    }
                }
            }
            _ = shutdown.changed() => {
                info!("Event dispatcher shutting down");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WebhookEndpointConfig;
    use crate::store::Store;
    use pulpo_common::event::SessionEvent;
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

    async fn test_store() -> Store {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        store
    }

    fn session_pulpo_event(status: &str) -> PulpoEvent {
        PulpoEvent::Session(SessionEvent {
            session_id: "id-1".into(),
            session_name: "s".into(),
            status: status.into(),
            node_name: "n".into(),
            timestamp: "2026-06-13T12:00:00Z".into(),
            ..Default::default()
        })
    }

    fn webhook_sink(url: String, events: Vec<String>) -> EventSink {
        EventSink::Webhook(WebhookSink::new(WebhookEndpointConfig {
            name: "hook".into(),
            url,
            events,
            secret: None,
        }))
    }

    // --- EventSink enum delegation ---

    #[tokio::test]
    async fn test_sink_name_delegation() {
        let webhook = webhook_sink("http://x/hook".into(), vec![]);
        assert_eq!(webhook.name(), "hook");
        let push = EventSink::WebPush(WebPushSink::new(test_store().await, "k".into()));
        assert_eq!(push.name(), "web-push");
    }

    #[tokio::test]
    async fn test_sink_wants_delegation() {
        let event = Event::from_pulpo_event(&session_pulpo_event("active"), "n").unwrap();
        let webhook = webhook_sink("http://x/hook".into(), vec!["stopped".into()]);
        assert!(!webhook.wants(&event)); // filtered out
        let push = EventSink::WebPush(WebPushSink::new(test_store().await, "k".into()));
        assert!(push.wants(&event)); // lifecycle -> wanted
    }

    // --- dispatcher loop ---

    #[tokio::test]
    async fn test_dispatcher_delivers_to_matching_sink() {
        let (url, captured) = capture_server().await;
        let sinks = vec![webhook_sink(url, vec![])];
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        event_tx.send(session_pulpo_event("active")).unwrap();
        let handle = tokio::spawn(run_dispatcher_loop(
            sinks,
            "mac-mini".into(),
            rx,
            shutdown_rx,
        ));

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        shutdown_tx.send(true).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("finish")
            .expect("no panic");

        let reqs = captured_requests(&captured).await;
        assert_eq!(reqs.len(), 1);
        let json: serde_json::Value = serde_json::from_str(&reqs[0].1).unwrap();
        assert_eq!(json["node"], "mac-mini");
        assert_eq!(json["type"], "lifecycle");
    }

    #[tokio::test]
    async fn test_dispatcher_skips_filtered_event() {
        let (url, captured) = capture_server().await;
        // Endpoint only wants "stopped"; we send "active".
        let sinks = vec![webhook_sink(url, vec!["stopped".into()])];
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        event_tx.send(session_pulpo_event("active")).unwrap();
        let handle = tokio::spawn(run_dispatcher_loop(sinks, "n".into(), rx, shutdown_rx));

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        shutdown_tx.send(true).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("finish")
            .expect("no panic");

        assert!(captured_requests(&captured).await.is_empty());
    }

    #[tokio::test]
    async fn test_dispatcher_skips_session_deleted() {
        let (url, captured) = capture_server().await;
        let sinks = vec![webhook_sink(url, vec![])];
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        event_tx
            .send(PulpoEvent::SessionDeleted(
                pulpo_common::event::SessionDeletedEvent {
                    session_id: "s".into(),
                    session_name: "n".into(),
                    node_name: "x".into(),
                    timestamp: "t".into(),
                },
            ))
            .unwrap();
        let handle = tokio::spawn(run_dispatcher_loop(sinks, "n".into(), rx, shutdown_rx));

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        shutdown_tx.send(true).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("finish")
            .expect("no panic");

        // SessionDeleted is housekeeping -> nothing delivered.
        assert!(captured_requests(&captured).await.is_empty());
    }

    #[tokio::test]
    async fn test_dispatcher_usage_alert_reaches_webhook() {
        let (url, captured) = capture_server().await;
        // Status filter would block lifecycle, but usage alerts must still flow.
        let sinks = vec![webhook_sink(url, vec!["stopped".into()])];
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        event_tx
            .send(PulpoEvent::UsageAlert(
                pulpo_common::event::UsageAlertEvent {
                    session_id: "s".into(),
                    session_name: "n".into(),
                    node_name: "x".into(),
                    alert_kind: "budget_threshold".into(),
                    message: "m".into(),
                    cost_usd: Some(0.85),
                    budget_usd: Some(1.0),
                    timestamp: "2026-06-13T12:00:00Z".into(),
                },
            ))
            .unwrap();
        let handle = tokio::spawn(run_dispatcher_loop(sinks, "n".into(), rx, shutdown_rx));

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        shutdown_tx.send(true).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("finish")
            .expect("no panic");

        let reqs = captured_requests(&captured).await;
        assert_eq!(reqs.len(), 1);
        let json: serde_json::Value = serde_json::from_str(&reqs[0].1).unwrap();
        assert_eq!(json["type"], "usage_alert");
    }

    #[tokio::test]
    async fn test_dispatcher_fans_out_to_multiple_sinks() {
        let (url, captured) = capture_server().await;
        let sinks = vec![
            webhook_sink(url, vec![]),
            EventSink::WebPush(WebPushSink::new(test_store().await, "k".into())),
        ];
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        event_tx.send(session_pulpo_event("ready")).unwrap();
        let handle = tokio::spawn(run_dispatcher_loop(sinks, "n".into(), rx, shutdown_rx));

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        shutdown_tx.send(true).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("finish")
            .expect("no panic");

        // Webhook captured it; web-push is a no-op (no subscriptions) but didn't panic.
        assert_eq!(captured_requests(&captured).await.len(), 1);
    }

    #[tokio::test]
    async fn test_dispatcher_shutdown() {
        let (event_tx, _) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let rx = event_tx.subscribe();
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        shutdown_tx.send(true).unwrap();
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            run_dispatcher_loop(vec![], "n".into(), rx, shutdown_rx),
        )
        .await
        .expect("should exit on shutdown");
    }

    #[tokio::test]
    async fn test_dispatcher_channel_closed() {
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (_shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        drop(event_tx);
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            run_dispatcher_loop(vec![], "n".into(), rx, shutdown_rx),
        )
        .await
        .expect("should exit when channel closes");
    }

    #[tokio::test]
    async fn test_dispatcher_lagged() {
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(1);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        // Overflow before the loop starts to force a Lagged error.
        for _ in 0..5 {
            let _ = event_tx.send(session_pulpo_event("active"));
        }
        let handle = tokio::spawn(run_dispatcher_loop(vec![], "n".into(), rx, shutdown_rx));
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        shutdown_tx.send(true).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("finish")
            .expect("no panic");
    }
}
