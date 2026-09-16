pub mod webhook;

use std::collections::HashMap;
use std::sync::Arc;

use pulpo_common::event::{Event, PulpoEvent};
use tokio::sync::Semaphore;
use tracing::{info, warn};

use crate::config::WebhookEndpointConfig;

/// Total delivery-concurrency budget, split evenly across every configured
/// endpoint (see [`build_endpoint_semaphores`]) rather than pooled as one
/// endpoint-wide semaphore. A single shared pool meant one endpoint that's down
/// — each delivery attempt against it occupies a permit for the whole
/// connect/request timeout plus [`webhook::RETRY_DELAYS`] backoff before giving
/// up — could exhaust the *entire* budget and starve every other, healthy
/// endpoint's deliveries too. Each endpoint still gets at least 1 permit even
/// when there are more endpoints configured than this budget, so the actual
/// combined capacity can exceed this nominal total once endpoint count is high
/// — a looser bound than the exact global cap this replaces, traded for
/// isolating one endpoint's failures from every other's.
pub(crate) const MAX_CONCURRENT_DELIVERIES: usize = 16;

/// Build one independent semaphore per configured endpoint (keyed by name),
/// each capped at an equal share of [`MAX_CONCURRENT_DELIVERIES`] — at least 1,
/// even when there are more endpoints than the budget allows. Rebuilt once per
/// dispatcher-loop run; a webhook config change needs a daemon restart like
/// every other watchdog/notification setting.
fn build_endpoint_semaphores(webhooks: &[WebhookEndpointConfig]) -> HashMap<String, Arc<Semaphore>> {
    let per_endpoint = (MAX_CONCURRENT_DELIVERIES / webhooks.len().max(1)).max(1);
    webhooks
        .iter()
        .map(|w| (w.name.clone(), Arc::new(Semaphore::new(per_endpoint))))
        .collect()
}

/// Spawn a detached, best-effort delivery task for every webhook endpoint
/// whose filter admits `event`, bounded by that endpoint's own semaphore in
/// `semaphores` (see [`build_endpoint_semaphores`]). Delivery (including
/// retries) happens concurrently with the caller — this is the "in-memory
/// queue": nothing is persisted, so a delivery that's still retrying when the
/// daemon exits is simply lost. Returns the number of deliveries actually
/// spawned (an admitting endpoint dropped for lack of a permit is not
/// counted).
fn dispatch_webhooks(
    client: &reqwest::Client,
    webhooks: &[WebhookEndpointConfig],
    event: &Event,
    semaphores: &HashMap<String, Arc<Semaphore>>,
) -> usize {
    let per_endpoint_cap = (MAX_CONCURRENT_DELIVERIES / webhooks.len().max(1)).max(1);
    let mut spawned = 0;
    for w in webhooks {
        if !webhook::webhook_wants(w, event) {
            continue;
        }
        let Some(semaphore) = semaphores.get(&w.name) else {
            continue;
        };
        let Ok(permit) = Arc::clone(semaphore).try_acquire_owned() else {
            warn!(
                webhook = %w.name,
                event = %format!("{}.{}", event.event_type, event.subtype),
                event_id = %event.event_id,
                max_concurrent = per_endpoint_cap,
                "Dropping webhook delivery: max concurrent deliveries reached for this endpoint"
            );
            continue;
        };
        let client = client.clone();
        let config = w.clone();
        let event = event.clone();
        tokio::spawn(async move {
            webhook::deliver(&client, &config, &event).await;
            drop(permit);
        });
        spawned += 1;
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
    let client = webhook::build_client();
    let semaphores = build_endpoint_semaphores(&webhooks);
    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(pulpo_event) => {
                        let Some(event) = Event::from_pulpo_event(&pulpo_event, &node_name) else {
                            continue;
                        };
                        dispatch_webhooks(&client, &webhooks, &event, &semaphores);
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
        }
    }

    // --- build_endpoint_semaphores ---

    #[test]
    fn test_build_endpoint_semaphores_splits_capacity_evenly() {
        let webhooks = vec![
            webhook_config("a", "http://127.0.0.1:1/a", vec![]),
            webhook_config("b", "http://127.0.0.1:1/b", vec![]),
        ];
        let semaphores = build_endpoint_semaphores(&webhooks);
        assert_eq!(semaphores.len(), 2);
        assert_eq!(
            semaphores["a"].available_permits(),
            MAX_CONCURRENT_DELIVERIES / 2
        );
        assert_eq!(
            semaphores["b"].available_permits(),
            MAX_CONCURRENT_DELIVERIES / 2
        );
    }

    #[test]
    fn test_build_endpoint_semaphores_at_least_one_permit_with_many_endpoints() {
        // More endpoints than the shared budget: each still gets at least 1
        // permit rather than being starved down to 0 by integer division.
        let webhooks: Vec<_> = (0..MAX_CONCURRENT_DELIVERIES * 4)
            .map(|i| webhook_config(&format!("hook-{i}"), "http://127.0.0.1:1/hook", vec![]))
            .collect();
        let semaphores = build_endpoint_semaphores(&webhooks);
        assert_eq!(semaphores.len(), webhooks.len());
        for semaphore in semaphores.values() {
            assert_eq!(semaphore.available_permits(), 1);
        }
    }

    #[test]
    fn test_build_endpoint_semaphores_empty_webhooks_is_empty_map() {
        assert!(build_endpoint_semaphores(&[]).is_empty());
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

        let semaphores = build_endpoint_semaphores(&webhooks);
        let n = dispatch_webhooks(&client, &webhooks, &event, &semaphores);
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

        let semaphores = build_endpoint_semaphores(&webhooks);
        assert_eq!(dispatch_webhooks(&client, &webhooks, &event, &semaphores), 2);
    }

    #[tokio::test]
    async fn test_dispatch_no_webhooks_is_noop() {
        let client = reqwest::Client::new();
        let event = Event::from_pulpo_event(&session_pulpo_event("active"), "n").unwrap();
        assert_eq!(
            dispatch_webhooks(&client, &[], &event, &build_endpoint_semaphores(&[])),
            0
        );
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
        let semaphores = build_endpoint_semaphores(&webhooks);
        assert_eq!(dispatch_webhooks(&client, &webhooks, &event, &semaphores), 0);
    }

    #[tokio::test]
    async fn test_dispatch_drops_when_no_permits_available_for_that_endpoint() {
        // Simulate one endpoint's concurrency slot already being in use (e.g. a
        // flood of prior events still delivering to it): a saturated per-endpoint
        // semaphore must make `dispatch_webhooks` drop the delivery for THAT
        // endpoint only — not spawn it anyway, and not block waiting for a permit.
        let client = reqwest::Client::new();
        let event = Event::from_pulpo_event(&session_pulpo_event("active"), "n").unwrap();
        let webhooks = vec![
            webhook_config("a", "http://127.0.0.1:1/a", vec![]),
            webhook_config("b", "http://127.0.0.1:1/b", vec![]),
        ];

        let semaphores = build_endpoint_semaphores(&webhooks);
        // Saturate every permit "a" owns ourselves.
        let a_semaphore = semaphores.get("a").unwrap().clone();
        let mut held = Vec::new();
        while let Ok(permit) = Arc::clone(&a_semaphore).try_acquire_owned() {
            held.push(permit);
        }
        assert_eq!(a_semaphore.available_permits(), 0);

        let spawned = dispatch_webhooks(&client, &webhooks, &event, &semaphores);
        assert_eq!(spawned, 1, "a's delivery is dropped, b's still spawns");
        assert_eq!(
            a_semaphore.available_permits(),
            0,
            "a dropped delivery must not touch the semaphore"
        );

        drop(held);
        assert!(a_semaphore.available_permits() > 0);
    }

    #[tokio::test]
    async fn test_dispatch_one_dead_endpoint_does_not_starve_others() {
        // Regression for the bug a single shared semaphore had: saturating one
        // endpoint's own budget must never reduce another, healthy endpoint's
        // ability to receive deliveries in the same dispatch call.
        let client = reqwest::Client::new();
        let event = Event::from_pulpo_event(&session_pulpo_event("active"), "n").unwrap();
        let webhooks = vec![
            webhook_config("dead", "http://127.0.0.1:1/dead", vec![]),
            webhook_config("healthy", "http://127.0.0.1:1/healthy", vec![]),
        ];

        let semaphores = build_endpoint_semaphores(&webhooks);
        let dead_semaphore = semaphores.get("dead").unwrap().clone();
        let mut held = Vec::new();
        while let Ok(permit) = Arc::clone(&dead_semaphore).try_acquire_owned() {
            held.push(permit);
        }

        for _ in 0..5 {
            let spawned = dispatch_webhooks(&client, &webhooks, &event, &semaphores);
            assert_eq!(spawned, 1, "the healthy endpoint keeps receiving deliveries");
        }
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
        let semaphores = build_endpoint_semaphores(&webhooks);
        assert_eq!(dispatch_webhooks(&client, &webhooks, &event, &semaphores), 1);
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
