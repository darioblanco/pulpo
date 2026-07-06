pub mod outbox;
pub mod web_push;
pub mod webhook;

use chrono::Utc;
use pulpo_common::event::{Event, PulpoEvent};
use tracing::{info, warn};

use self::web_push::WebPushSink;
use self::webhook::webhook_wants;
use crate::config::WebhookEndpointConfig;
use crate::store::Store;

/// Enqueue a canonical event into the durable webhook outbox for every endpoint
/// whose filter admits it. Each row becomes due immediately (`next_attempt_at =
/// now`); the [`outbox::run_outbox_worker`] drains and delivers it with retry +
/// backoff. The serialized envelope is stored verbatim so retries replay the
/// identical body and `X-Pulpo-Event-Id`.
///
/// Returns the number of rows enqueued. Best-effort: a per-endpoint enqueue
/// failure is logged and skipped (never panics).
async fn enqueue_event_webhooks(
    store: &Store,
    webhooks: &[WebhookEndpointConfig],
    event: &Event,
) -> usize {
    let admitting: Vec<&WebhookEndpointConfig> = webhooks
        .iter()
        .filter(|w| webhook_wants(w, event))
        .collect();
    if admitting.is_empty() {
        return 0;
    }

    let envelope_json = match serde_json::to_string(event) {
        Ok(json) => json,
        Err(e) => {
            warn!(error = %e, "Failed to serialize event for webhook outbox");
            return 0;
        }
    };
    let now = Utc::now().to_rfc3339();

    let mut enqueued = 0;
    for w in admitting {
        match store
            .enqueue_webhook(&w.name, &event.event_id, &envelope_json, &now)
            .await
        {
            Ok(()) => enqueued += 1,
            Err(e) => warn!(
                webhook = %w.name,
                error = %e,
                "Failed to enqueue webhook delivery"
            ),
        }
    }
    enqueued
}

/// Run the single event dispatcher loop.
///
/// Subscribes to the broadcast bus once, converts each [`PulpoEvent`] into the
/// canonical [`Event`] envelope (skipping events that aren't externally
/// forwarded), and routes it:
/// - **webhooks** → enqueued into the durable outbox (delivery happens in
///   [`outbox::run_outbox_worker`] with retry + exponential backoff, surviving
///   restarts and transient endpoint failures);
/// - **web-push** → delivered inline, best-effort (browser push is inherently
///   best-effort, so it is intentionally *not* routed through the outbox).
pub async fn run_dispatcher_loop(
    store: Store,
    webhooks: Vec<WebhookEndpointConfig>,
    web_push: Option<WebPushSink>,
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
                        // Webhooks: durable enqueue (delivered by the outbox worker).
                        enqueue_event_webhooks(&store, &webhooks, &event).await;
                        // Web-push: inline best-effort.
                        if let Some(push) = &web_push
                            && push.wants(&event)
                        {
                            push.deliver(&event).await;
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
    use crate::store::test_store;
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

    fn webhook_config(name: &str, events: Vec<String>) -> WebhookEndpointConfig {
        WebhookEndpointConfig {
            name: name.into(),
            url: format!("http://example.com/{name}"),
            events,
            min_severity: None,
            secret: None,
        }
    }

    async fn pending_event_ids(store: &Store) -> Vec<String> {
        store
            .fetch_due_webhook_deliveries("2027-01-01T00:00:00Z", 100)
            .await
            .unwrap()
            .into_iter()
            .map(|r| format!("{}:{}", r.endpoint, r.event_id))
            .collect()
    }

    // --- enqueue_event_webhooks ---

    #[tokio::test]
    async fn test_enqueue_admitting_endpoints_only() {
        let store = test_store().await;
        let event = Event::from_pulpo_event(&session_pulpo_event("active"), "mac-mini").unwrap();
        let webhooks = vec![
            webhook_config("all", vec![]), // empty filter -> admits
            webhook_config("stopped-only", vec!["stopped".into()]), // filtered out
        ];

        let n = enqueue_event_webhooks(&store, &webhooks, &event).await;
        assert_eq!(n, 1);

        let rows = store
            .fetch_due_webhook_deliveries("2027-01-01T00:00:00Z", 10)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].endpoint, "all");
        assert_eq!(rows[0].event_id, event.event_id);
        // Stored envelope is the exact serialized event.
        assert_eq!(
            rows[0].envelope_json,
            serde_json::to_string(&event).unwrap()
        );
        assert_eq!(rows[0].status, "pending");
    }

    #[tokio::test]
    async fn test_enqueue_multiple_endpoints() {
        let store = test_store().await;
        let event = Event::from_pulpo_event(&session_pulpo_event("ready"), "n").unwrap();
        let webhooks = vec![webhook_config("a", vec![]), webhook_config("b", vec![])];

        let n = enqueue_event_webhooks(&store, &webhooks, &event).await;
        assert_eq!(n, 2);
        let mut ids = pending_event_ids(&store).await;
        ids.sort();
        assert_eq!(
            ids,
            vec![
                format!("a:{}", event.event_id),
                format!("b:{}", event.event_id),
            ]
        );
    }

    #[tokio::test]
    async fn test_enqueue_no_webhooks_is_noop() {
        let store = test_store().await;
        let event = Event::from_pulpo_event(&session_pulpo_event("active"), "n").unwrap();
        assert_eq!(enqueue_event_webhooks(&store, &[], &event).await, 0);
    }

    #[tokio::test]
    async fn test_enqueue_none_admit_is_noop() {
        let store = test_store().await;
        let event = Event::from_pulpo_event(&session_pulpo_event("active"), "n").unwrap();
        let webhooks = vec![webhook_config("stopped-only", vec!["stopped".into()])];
        assert_eq!(enqueue_event_webhooks(&store, &webhooks, &event).await, 0);
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
    async fn test_enqueue_usage_alert_routed_uniformly() {
        let store = test_store().await;
        let event = Event::from_pulpo_event(&usage_alert_pulpo_event(), "n").unwrap();
        // Universal routing: a `usage_alert.*` glob admits it, a lifecycle-only
        // glob does not (no more "usage alerts always flow" special case).
        let webhooks = vec![
            webhook_config("usage", vec!["usage_alert.*".into()]),
            webhook_config("lifecycle-only", vec!["lifecycle.*".into()]),
        ];
        let n = enqueue_event_webhooks(&store, &webhooks, &event).await;
        assert_eq!(n, 1);
        let rows = store
            .fetch_due_webhook_deliveries("2027-01-01T00:00:00Z", 10)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].endpoint, "usage");
    }

    #[tokio::test]
    async fn test_enqueue_store_error_is_swallowed() {
        let store = test_store().await;
        sqlx::query("DROP TABLE webhook_outbox")
            .execute(store.pool())
            .await
            .unwrap();
        let event = Event::from_pulpo_event(&session_pulpo_event("active"), "n").unwrap();
        let webhooks = vec![webhook_config("a", vec![])];
        // Enqueue fails (table gone) -> 0 enqueued, no panic.
        assert_eq!(enqueue_event_webhooks(&store, &webhooks, &event).await, 0);
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

    #[tokio::test]
    async fn test_dispatcher_enqueues_matching_webhook() {
        let store = test_store().await;
        let webhooks = vec![webhook_config("hook", vec![])];
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        event_tx.send(session_pulpo_event("active")).unwrap();
        let handle = tokio::spawn(run_dispatcher_loop(
            store.clone(),
            webhooks,
            None,
            "mac-mini".into(),
            rx,
            shutdown_rx,
        ));
        run_until_idle(handle, &shutdown_tx).await;

        let rows = store
            .fetch_due_webhook_deliveries("2027-01-01T00:00:00Z", 10)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].endpoint, "hook");
        let json: serde_json::Value = serde_json::from_str(&rows[0].envelope_json).unwrap();
        assert_eq!(json["node"], "mac-mini");
        assert_eq!(json["type"], "lifecycle");
    }

    #[tokio::test]
    async fn test_dispatcher_skips_filtered_event() {
        let store = test_store().await;
        // Endpoint only wants "stopped"; we send "active".
        let webhooks = vec![webhook_config("hook", vec!["stopped".into()])];
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        event_tx.send(session_pulpo_event("active")).unwrap();
        let handle = tokio::spawn(run_dispatcher_loop(
            store.clone(),
            webhooks,
            None,
            "n".into(),
            rx,
            shutdown_rx,
        ));
        run_until_idle(handle, &shutdown_tx).await;

        assert!(pending_event_ids(&store).await.is_empty());
    }

    #[tokio::test]
    async fn test_dispatcher_skips_session_deleted() {
        let store = test_store().await;
        let webhooks = vec![webhook_config("hook", vec![])];
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
        let handle = tokio::spawn(run_dispatcher_loop(
            store.clone(),
            webhooks,
            None,
            "n".into(),
            rx,
            shutdown_rx,
        ));
        run_until_idle(handle, &shutdown_tx).await;

        // SessionDeleted is housekeeping -> nothing enqueued.
        assert!(pending_event_ids(&store).await.is_empty());
    }

    #[tokio::test]
    async fn test_dispatcher_usage_alert_reaches_webhook() {
        let store = test_store().await;
        // Universal routing: a `usage_alert.*` glob admits the alert.
        let webhooks = vec![webhook_config("hook", vec!["usage_alert.*".into()])];
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        event_tx.send(usage_alert_pulpo_event()).unwrap();
        let handle = tokio::spawn(run_dispatcher_loop(
            store.clone(),
            webhooks,
            None,
            "n".into(),
            rx,
            shutdown_rx,
        ));
        run_until_idle(handle, &shutdown_tx).await;

        let rows = store
            .fetch_due_webhook_deliveries("2027-01-01T00:00:00Z", 10)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        let json: serde_json::Value = serde_json::from_str(&rows[0].envelope_json).unwrap();
        assert_eq!(json["type"], "usage_alert");
    }

    #[tokio::test]
    async fn test_dispatcher_usage_alert_dropped_by_lifecycle_filter() {
        let store = test_store().await;
        // A lifecycle-only filter now drops usage alerts (uniform routing).
        let webhooks = vec![webhook_config("hook", vec!["lifecycle.*".into()])];
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        event_tx.send(usage_alert_pulpo_event()).unwrap();
        let handle = tokio::spawn(run_dispatcher_loop(
            store.clone(),
            webhooks,
            None,
            "n".into(),
            rx,
            shutdown_rx,
        ));
        run_until_idle(handle, &shutdown_tx).await;

        assert!(pending_event_ids(&store).await.is_empty());
    }

    #[tokio::test]
    async fn test_dispatcher_with_web_push_sink_no_panic() {
        // Web-push delivered inline; no subscriptions -> no-op, and webhook still enqueued.
        let store = test_store().await;
        let webhooks = vec![webhook_config("hook", vec![])];
        let push = WebPushSink::new(store.clone(), "k".into());
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        event_tx.send(session_pulpo_event("ready")).unwrap();
        let handle = tokio::spawn(run_dispatcher_loop(
            store.clone(),
            webhooks,
            Some(push),
            "n".into(),
            rx,
            shutdown_rx,
        ));
        run_until_idle(handle, &shutdown_tx).await;

        assert_eq!(pending_event_ids(&store).await.len(), 1);
    }

    #[tokio::test]
    async fn test_dispatcher_shutdown() {
        let store = test_store().await;
        let (event_tx, _) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let rx = event_tx.subscribe();
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        shutdown_tx.send(true).unwrap();
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            run_dispatcher_loop(store, vec![], None, "n".into(), rx, shutdown_rx),
        )
        .await
        .expect("should exit on shutdown");
    }

    #[tokio::test]
    async fn test_dispatcher_channel_closed() {
        let store = test_store().await;
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (_shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        drop(event_tx);
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            run_dispatcher_loop(store, vec![], None, "n".into(), rx, shutdown_rx),
        )
        .await
        .expect("should exit when channel closes");
    }

    #[tokio::test]
    async fn test_dispatcher_lagged() {
        let store = test_store().await;
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(1);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        // Overflow before the loop starts to force a Lagged error.
        for _ in 0..5 {
            let _ = event_tx.send(session_pulpo_event("active"));
        }
        let handle = tokio::spawn(run_dispatcher_loop(
            store,
            vec![],
            None,
            "n".into(),
            rx,
            shutdown_rx,
        ));
        run_until_idle(handle, &shutdown_tx).await;
    }
}
