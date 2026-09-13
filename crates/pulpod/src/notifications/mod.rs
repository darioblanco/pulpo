pub mod webhook;

use pulpo_common::event::{Event, PulpoEvent};
use tracing::{info, warn};

use crate::config::WebhookEndpointConfig;

/// Spawn a detached, best-effort delivery task for every webhook endpoint
/// whose filter admits `event`. Delivery (including retries) happens
/// concurrently with the caller — this is the "in-memory queue": nothing is
/// persisted, so a delivery that's still retrying when the daemon exits is
/// simply lost. Returns the number of deliveries spawned.
fn dispatch_webhooks(
    client: &reqwest::Client,
    webhooks: &[WebhookEndpointConfig],
    event: &Event,
) -> usize {
    let mut spawned = 0;
    for w in webhooks {
        if webhook::webhook_wants(w, event) {
            let client = client.clone();
            let config = w.clone();
            let event = event.clone();
            tokio::spawn(async move {
                webhook::deliver(&client, &config, &event).await;
            });
            spawned += 1;
        }
    }
    spawned
}

/// Run the single event dispatcher loop.
///
/// Subscribes to the broadcast bus once, converts each [`PulpoEvent`] into the
/// canonical [`Event`] envelope (skipping events that aren't externally
/// forwarded), and fans it out to every admitting webhook endpoint via
/// [`dispatch_webhooks`] — a plain POST with a fixed retry schedule (see
/// [`webhook::deliver`]), best-effort, with no durable outbox.
pub async fn run_dispatcher_loop(
    webhooks: Vec<WebhookEndpointConfig>,
    node_name: String,
    mut rx: tokio::sync::broadcast::Receiver<PulpoEvent>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let client = reqwest::Client::new();
    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(pulpo_event) => {
                        let Some(event) = Event::from_pulpo_event(&pulpo_event, &node_name) else {
                            continue;
                        };
                        dispatch_webhooks(&client, &webhooks, &event);
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
    use pulpo_common::event::SessionEvent;

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

    fn webhook_config(name: &str, url: &str, events: Vec<String>) -> WebhookEndpointConfig {
        WebhookEndpointConfig {
            name: name.into(),
            url: url.into(),
            events,
            min_severity: None,
            secret: None,
        }
    }

    // --- dispatch_webhooks ---

    #[tokio::test]
    async fn test_dispatch_admitting_endpoints_only() {
        let client = reqwest::Client::new();
        let event = Event::from_pulpo_event(&session_pulpo_event("active"), "mac-mini").unwrap();
        let webhooks = vec![
            webhook_config("all", "http://127.0.0.1:1/a", vec![]), // empty filter -> admits
            webhook_config(
                "stopped-only",
                "http://127.0.0.1:1/b",
                vec!["stopped".into()],
            ), // filtered out
        ];

        let n = dispatch_webhooks(&client, &webhooks, &event);
        assert_eq!(n, 1);
    }

    #[tokio::test]
    async fn test_dispatch_multiple_endpoints() {
        let client = reqwest::Client::new();
        let event = Event::from_pulpo_event(&session_pulpo_event("ready"), "n").unwrap();
        let webhooks = vec![
            webhook_config("a", "http://127.0.0.1:1/a", vec![]),
            webhook_config("b", "http://127.0.0.1:1/b", vec![]),
        ];

        assert_eq!(dispatch_webhooks(&client, &webhooks, &event), 2);
    }

    #[tokio::test]
    async fn test_dispatch_no_webhooks_is_noop() {
        let client = reqwest::Client::new();
        let event = Event::from_pulpo_event(&session_pulpo_event("active"), "n").unwrap();
        assert_eq!(dispatch_webhooks(&client, &[], &event), 0);
    }

    #[tokio::test]
    async fn test_dispatch_none_admit_is_noop() {
        let client = reqwest::Client::new();
        let event = Event::from_pulpo_event(&session_pulpo_event("active"), "n").unwrap();
        let webhooks = vec![webhook_config(
            "stopped-only",
            "http://127.0.0.1:1/a",
            vec!["stopped".into()],
        )];
        assert_eq!(dispatch_webhooks(&client, &webhooks, &event), 0);
    }

    fn usage_alert_pulpo_event() -> PulpoEvent {
        PulpoEvent::UsageAlert(pulpo_common::event::UsageAlertEvent {
            session_id: "s".into(),
            session_name: "n".into(),
            node_name: "x".into(),
            alert_kind: "budget_threshold".into(),
            message: "m".into(),
            cost_usd: Some(0.85),
            budget_usd: Some(1.0),
            timestamp: "2026-06-13T12:00:00Z".into(),
        })
    }

    #[tokio::test]
    async fn test_dispatch_usage_alert_routed_uniformly() {
        let client = reqwest::Client::new();
        let event = Event::from_pulpo_event(&usage_alert_pulpo_event(), "n").unwrap();
        let webhooks = vec![
            webhook_config(
                "usage",
                "http://127.0.0.1:1/a",
                vec!["usage_alert.*".into()],
            ),
            webhook_config(
                "lifecycle-only",
                "http://127.0.0.1:1/b",
                vec!["lifecycle.*".into()],
            ),
        ];
        assert_eq!(dispatch_webhooks(&client, &webhooks, &event), 1);
    }

    // --- dispatcher loop ---

    async fn run_until_idle(
        handle: tokio::task::JoinHandle<()>,
        shutdown_tx: &tokio::sync::watch::Sender<bool>,
    ) {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        shutdown_tx.send(true).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("finish")
            .expect("no panic");
    }

    /// Minimal capture server, used to prove the dispatcher loop actually
    /// spawns and completes a real delivery (not just that it "would").
    async fn capture_server() -> (String, std::sync::Arc<tokio::sync::Mutex<Vec<String>>>) {
        let captured = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let captured_clone = captured.clone();
        let app = axum::Router::new().route(
            "/hook",
            axum::routing::post(move |body: String| {
                let captured = captured_clone.clone();
                async move {
                    captured.lock().await.push(body);
                    axum::http::StatusCode::OK
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(axum::serve(listener, app).into_future());
        (format!("http://{addr}/hook"), captured)
    }

    #[tokio::test]
    async fn test_dispatcher_delivers_matching_webhook() {
        let (url, captured) = capture_server().await;
        let webhooks = vec![webhook_config("hook", &url, vec![])];
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        event_tx.send(session_pulpo_event("active")).unwrap();
        let handle = tokio::spawn(run_dispatcher_loop(
            webhooks,
            "mac-mini".into(),
            rx,
            shutdown_rx,
        ));
        run_until_idle(handle, &shutdown_tx).await;

        let bodies = captured.lock().await;
        assert_eq!(bodies.len(), 1);
        let json: serde_json::Value = serde_json::from_str(&bodies[0]).unwrap();
        assert_eq!(json["node"], "mac-mini");
        assert_eq!(json["type"], "lifecycle");
    }

    #[tokio::test]
    async fn test_dispatcher_skips_filtered_event() {
        let webhooks = vec![webhook_config(
            "hook",
            "http://127.0.0.1:1/hook",
            vec!["stopped".into()],
        )];
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        event_tx.send(session_pulpo_event("active")).unwrap();
        let handle = tokio::spawn(run_dispatcher_loop(webhooks, "n".into(), rx, shutdown_rx));
        run_until_idle(handle, &shutdown_tx).await;
    }

    #[tokio::test]
    async fn test_dispatcher_skips_session_deleted() {
        let (url, captured) = capture_server().await;
        let webhooks = vec![webhook_config("hook", &url, vec![])];
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
        let handle = tokio::spawn(run_dispatcher_loop(webhooks, "n".into(), rx, shutdown_rx));
        run_until_idle(handle, &shutdown_tx).await;

        // SessionDeleted is housekeeping -> nothing delivered.
        assert!(captured.lock().await.is_empty());
    }

    #[tokio::test]
    async fn test_dispatcher_usage_alert_reaches_webhook() {
        let (url, captured) = capture_server().await;
        let webhooks = vec![webhook_config("hook", &url, vec!["usage_alert.*".into()])];
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        event_tx.send(usage_alert_pulpo_event()).unwrap();
        let handle = tokio::spawn(run_dispatcher_loop(webhooks, "n".into(), rx, shutdown_rx));
        run_until_idle(handle, &shutdown_tx).await;

        let bodies = captured.lock().await;
        assert_eq!(bodies.len(), 1);
        let json: serde_json::Value = serde_json::from_str(&bodies[0]).unwrap();
        assert_eq!(json["type"], "usage_alert");
    }

    #[tokio::test]
    async fn test_dispatcher_usage_alert_dropped_by_lifecycle_filter() {
        let webhooks = vec![webhook_config(
            "hook",
            "http://127.0.0.1:1/hook",
            vec!["lifecycle.*".into()],
        )];
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        event_tx.send(usage_alert_pulpo_event()).unwrap();
        let handle = tokio::spawn(run_dispatcher_loop(webhooks, "n".into(), rx, shutdown_rx));
        run_until_idle(handle, &shutdown_tx).await;
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
        run_until_idle(handle, &shutdown_tx).await;
    }
}
