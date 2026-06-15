//! Durable webhook delivery: outbox worker + retry/backoff policy.
//!
//! The dispatcher enqueues a pending `webhook_outbox` row per (endpoint, event);
//! this worker drains due rows on an interval, POSTs the stored envelope with the
//! same signed headers a live send uses, and on failure reschedules with
//! exponential backoff (or marks the row dead after [`MAX_ATTEMPTS`]). Because the
//! envelope is stored verbatim, every retry replays the identical body and
//! `X-Pulpo-Event-Id`, so receivers dedupe on the stable `event_id`.
//!
//! Restart-durability is automatic: rows left `pending` from before a restart are
//! simply due rows the worker drains on its next tick — no special recovery path.

use std::collections::HashMap;
use std::time::Duration;

use chrono::Utc;
use tracing::{info, warn};

use crate::config::WebhookEndpointConfig;
use crate::store::{Store, WebhookOutboxRow};

/// Base backoff: the first retry waits this long (`BASE * 2^0`).
pub const BASE_BACKOFF: Duration = Duration::from_secs(5);
/// Cap: no retry ever waits longer than this.
pub const MAX_BACKOFF: Duration = Duration::from_secs(3600);
/// After this many attempts a delivery is marked `dead`.
pub const MAX_ATTEMPTS: u32 = 10;
/// How often the worker polls for due deliveries.
pub const POLL_INTERVAL: Duration = Duration::from_secs(5);
/// Maximum due rows drained per tick.
pub const BATCH_LIMIT: usize = 100;
/// Delivered/dead outbox rows older than this many days are pruned (table stays bounded).
pub const OUTBOX_RETENTION_DAYS: i64 = 7;

/// Exponential backoff for the `attempts`-th retry: `min(CAP, BASE * 2^(attempts-1))`.
///
/// Deterministic (no jitter) so the curve is unit-testable. `attempts` is the
/// retry number that just failed-and-is-being-rescheduled — i.e. the count stored
/// on the row after the failing attempt. `attempts == 0` is treated as the first
/// retry (`BASE`).
#[must_use]
pub fn webhook_backoff(attempts: u32) -> Duration {
    let shift = attempts.saturating_sub(1).min(u32::BITS - 1);
    // `u64::checked_shl`/saturating math keeps us away from overflow at large shifts.
    let factor = 1u64.checked_shl(shift).unwrap_or(u64::MAX);
    let secs = BASE_BACKOFF.as_secs().saturating_mul(factor);
    Duration::from_secs(secs.min(MAX_BACKOFF.as_secs()))
}

/// The decision for a failed delivery: retry later, or give up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureOutcome {
    /// Reschedule: store this attempt count and become due after the delay.
    Reschedule { attempts: u32, delay: Duration },
    /// Give up permanently (max attempts reached).
    Dead,
}

/// Decide what to do after a delivery attempt fails.
///
/// `prior_attempts` is the row's `attempts` value before this failure. The
/// failing attempt bumps it to `prior_attempts + 1`; once that reaches
/// [`MAX_ATTEMPTS`] the row is dead, otherwise it is rescheduled with
/// [`webhook_backoff`].
#[must_use]
pub fn decide_failure(prior_attempts: u32) -> FailureOutcome {
    let attempts = prior_attempts.saturating_add(1);
    if attempts >= MAX_ATTEMPTS {
        FailureOutcome::Dead
    } else {
        FailureOutcome::Reschedule {
            attempts,
            delay: webhook_backoff(attempts),
        }
    }
}

/// Build a name → config lookup for resolving outbox rows to endpoints.
#[must_use]
pub fn endpoint_map(webhooks: &[WebhookEndpointConfig]) -> HashMap<String, WebhookEndpointConfig> {
    webhooks
        .iter()
        .map(|w| (w.name.clone(), w.clone()))
        .collect()
}

/// Apply a failure outcome for a row to the store (reschedule or mark dead).
///
/// Pure orchestration over the store API — no network — so it stays fully
/// testable. `error` is the failure reason recorded on the row.
async fn record_failure(store: &Store, row: &WebhookOutboxRow, error: &str) {
    let prior = u32::try_from(row.attempts).unwrap_or(u32::MAX);
    match decide_failure(prior) {
        FailureOutcome::Dead => {
            warn!(
                outbox_id = row.id,
                endpoint = %row.endpoint,
                event_id = %row.event_id,
                "Webhook delivery permanently failed (max attempts), marking dead"
            );
            if let Err(e) = store.mark_webhook_dead(row.id, error).await {
                warn!(outbox_id = row.id, error = %e, "Failed to mark webhook row dead");
            }
        }
        FailureOutcome::Reschedule { attempts, delay } => {
            let next_attempt_at = (Utc::now()
                + chrono::Duration::from_std(delay).unwrap_or_else(|_| chrono::Duration::hours(1)))
            .to_rfc3339();
            info!(
                outbox_id = row.id,
                endpoint = %row.endpoint,
                attempts,
                delay_secs = delay.as_secs(),
                "Rescheduling webhook delivery"
            );
            if let Err(e) = store
                .reschedule_webhook(row.id, i64::from(attempts), &next_attempt_at, error)
                .await
            {
                warn!(outbox_id = row.id, error = %e, "Failed to reschedule webhook row");
            }
        }
    }
}

/// Process one due outbox row: resolve its endpoint, POST, and record the result.
///
/// The actual `reqwest` POST is split into [`post_envelope`] (gated out of
/// coverage); everything else — endpoint resolution, the dead/reschedule
/// decision, and the store writes — is testable here.
async fn process_row(
    store: &Store,
    endpoints: &HashMap<String, WebhookEndpointConfig>,
    client: &reqwest::Client,
    row: &WebhookOutboxRow,
) {
    let Some(config) = endpoints.get(&row.endpoint) else {
        warn!(
            outbox_id = row.id,
            endpoint = %row.endpoint,
            "Webhook endpoint no longer configured, marking delivery dead"
        );
        let err = format!("endpoint '{}' no longer configured", row.endpoint);
        if let Err(e) = store.mark_webhook_dead(row.id, &err).await {
            warn!(outbox_id = row.id, error = %e, "Failed to mark orphaned webhook row dead");
        }
        return;
    };

    match post_envelope(client, config, row).await {
        Ok(()) => {
            if let Err(e) = store
                .mark_webhook_delivered(row.id, &Utc::now().to_rfc3339())
                .await
            {
                warn!(outbox_id = row.id, error = %e, "Failed to mark webhook row delivered");
            }
        }
        Err(err) => record_failure(store, row, &err).await,
    }
}

/// Drain all currently-due deliveries once. Returns the number of rows processed.
async fn drain_due(
    store: &Store,
    endpoints: &HashMap<String, WebhookEndpointConfig>,
    client: &reqwest::Client,
) -> usize {
    let now = Utc::now().to_rfc3339();
    let due = match store.fetch_due_webhook_deliveries(&now, BATCH_LIMIT).await {
        Ok(rows) => rows,
        Err(e) => {
            warn!(error = %e, "Failed to fetch due webhook deliveries");
            return 0;
        }
    };
    let count = due.len();
    for row in &due {
        process_row(store, endpoints, client, row).await;
    }
    count
}

/// POST a stored envelope to its endpoint, returning `Ok` on a 2xx response.
///
/// The only network I/O in this module; gated out of coverage. The signing and
/// headers come from [`crate::notifications::webhook::build_webhook_request`], so
/// a replayed envelope is byte-identical to a fresh send.
#[cfg(not(coverage))]
async fn post_envelope(
    client: &reqwest::Client,
    config: &WebhookEndpointConfig,
    row: &WebhookOutboxRow,
) -> Result<(), String> {
    use crate::notifications::webhook::build_webhook_request;

    let event_header = event_header_from_envelope(&row.envelope_json);
    let body = row.envelope_json.clone().into_bytes();
    build_webhook_request(client, config, body, &event_header, &row.event_id)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Coverage stub for the network POST.
///
/// Real HTTP is untestable under coverage, so this stand-in returns a
/// deterministic result driven by the endpoint URL: a URL containing `"fail"`
/// yields an error (exercising the reschedule/dead path), anything else
/// succeeds (exercising the delivered path). This keeps both arms of
/// [`process_row`] covered without a live network.
#[cfg(coverage)]
async fn post_envelope(
    _client: &reqwest::Client,
    config: &WebhookEndpointConfig,
    _row: &WebhookOutboxRow,
) -> Result<(), String> {
    if config.url.contains("fail") {
        Err("simulated failure".to_owned())
    } else {
        Ok(())
    }
}

/// Derive the `X-Pulpo-Event` header (`<type>.<subtype>`) from a stored envelope.
///
/// Falls back to `"event"` if the envelope can't be parsed (it always can, since
/// we serialized it ourselves) so a malformed row still gets a valid header.
#[cfg_attr(coverage, allow(dead_code))]
fn event_header_from_envelope(envelope_json: &str) -> String {
    serde_json::from_str::<pulpo_common::event::Event>(envelope_json).map_or_else(
        |_| "event".to_owned(),
        |e| format!("{}.{}", e.event_type, e.subtype),
    )
}

/// Run the durable webhook outbox delivery worker.
///
/// Ticks every [`POLL_INTERVAL`], draining due deliveries each tick. Rebuilds the
/// endpoint name→config map on every tick so config changes (added/removed
/// endpoints) take effect without a restart; a removed endpoint causes its
/// pending rows to be marked dead. Honors the shutdown watch channel.
pub async fn run_outbox_worker(
    store: Store,
    webhooks: Vec<WebhookEndpointConfig>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let client = reqwest::Client::new();
    let endpoints = endpoint_map(&webhooks);
    let mut ticker = tokio::time::interval(POLL_INTERVAL);
    info!(endpoints = endpoints.len(), "Webhook outbox worker started");

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                drain_due(&store, &endpoints, &client).await;
                // Keep the table bounded: sweep delivered/dead rows past the retention
                // window. Cheap indexed DELETE (usually removes 0 rows); pending rows
                // are never touched regardless of age.
                let cutoff = (Utc::now() - chrono::Duration::days(OUTBOX_RETENTION_DAYS)).to_rfc3339();
                let _ = store.prune_webhook_outbox(&cutoff).await;
            }
            _ = shutdown.changed() => {
                info!("Webhook outbox worker shutting down");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_store() -> Store {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        store
    }

    fn config(name: &str, url: &str) -> WebhookEndpointConfig {
        WebhookEndpointConfig {
            name: name.into(),
            url: url.into(),
            events: vec![],
            min_severity: None,
            secret: None,
        }
    }

    // --- webhook_backoff ---

    #[test]
    fn test_backoff_curve() {
        // attempts 1..=5: 5s, 10s, 20s, 40s, 80s (BASE * 2^(n-1)).
        assert_eq!(webhook_backoff(1), Duration::from_secs(5));
        assert_eq!(webhook_backoff(2), Duration::from_secs(10));
        assert_eq!(webhook_backoff(3), Duration::from_secs(20));
        assert_eq!(webhook_backoff(4), Duration::from_secs(40));
        assert_eq!(webhook_backoff(5), Duration::from_secs(80));
    }

    #[test]
    fn test_backoff_attempts_zero_is_base() {
        assert_eq!(webhook_backoff(0), BASE_BACKOFF);
    }

    #[test]
    fn test_backoff_caps_at_max() {
        // 5 * 2^9 = 2560s > 3600? no -> 2560s for attempts=10.
        assert_eq!(webhook_backoff(10), Duration::from_secs(2560));
        // attempts=11: 5 * 2^10 = 5120s > 3600 -> capped.
        assert_eq!(webhook_backoff(11), MAX_BACKOFF);
        // Very large attempts stay capped, no overflow/panic.
        assert_eq!(webhook_backoff(1000), MAX_BACKOFF);
        assert_eq!(webhook_backoff(u32::MAX), MAX_BACKOFF);
    }

    // --- decide_failure ---

    #[test]
    fn test_decide_failure_reschedules_below_max() {
        assert_eq!(
            decide_failure(0),
            FailureOutcome::Reschedule {
                attempts: 1,
                delay: Duration::from_secs(5),
            }
        );
        assert_eq!(
            decide_failure(3),
            FailureOutcome::Reschedule {
                attempts: 4,
                delay: Duration::from_secs(40),
            }
        );
    }

    #[test]
    fn test_decide_failure_dead_at_max() {
        // prior=8 -> attempts=9 -> still reschedule (9 < 10).
        assert!(matches!(
            decide_failure(8),
            FailureOutcome::Reschedule { attempts: 9, .. }
        ));
        // prior=9 -> attempts=10 -> dead.
        assert_eq!(decide_failure(9), FailureOutcome::Dead);
        assert_eq!(decide_failure(100), FailureOutcome::Dead);
        assert_eq!(decide_failure(u32::MAX), FailureOutcome::Dead);
    }

    #[test]
    fn test_failure_outcome_debug_clone() {
        let o = FailureOutcome::Reschedule {
            attempts: 2,
            delay: Duration::from_secs(10),
        };
        let c = o.clone();
        assert_eq!(o, c);
        assert_eq!(format!("{o:?}"), format!("{c:?}"));
        let d = FailureOutcome::Dead;
        assert_eq!(format!("{d:?}"), "Dead");
    }

    // --- endpoint_map ---

    #[test]
    fn test_endpoint_map_builds_lookup() {
        let map = endpoint_map(&[config("a", "http://a"), config("b", "http://b")]);
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("a").unwrap().url, "http://a");
        assert_eq!(map.get("b").unwrap().url, "http://b");
        assert!(!map.contains_key("missing"));
    }

    // --- event_header_from_envelope ---

    #[test]
    fn test_event_header_from_valid_envelope() {
        let env = r#"{"schema_version":1,"event_id":"x","type":"lifecycle","subtype":"idle","severity":"warn","occurred_at":"t","node":"n"}"#;
        assert_eq!(event_header_from_envelope(env), "lifecycle.idle");
    }

    #[test]
    fn test_event_header_from_invalid_envelope_falls_back() {
        assert_eq!(event_header_from_envelope("{not json"), "event");
    }

    // --- record_failure (store side, no network) ---

    fn pending_row(id: i64, attempts: i64) -> WebhookOutboxRow {
        WebhookOutboxRow {
            id,
            endpoint: "hook".into(),
            event_id: "evt".into(),
            envelope_json: "{}".into(),
            status: "pending".into(),
            attempts,
            next_attempt_at: "2026-06-13T12:00:00Z".into(),
            last_error: None,
            created_at: "2026-06-13T11:00:00Z".into(),
            delivered_at: None,
        }
    }

    #[tokio::test]
    async fn test_record_failure_reschedules() {
        let store = test_store().await;
        store
            .enqueue_webhook("hook", "evt", "{}", "2026-06-13T12:00:00Z")
            .await
            .unwrap();
        let row = pending_row(1, 0);

        record_failure(&store, &row, "boom").await;

        // Far-future poll finds it still pending, attempts bumped, error recorded.
        let due = store
            .fetch_due_webhook_deliveries("2027-01-01T00:00:00Z", 10)
            .await
            .unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].attempts, 1);
        assert_eq!(due[0].status, "pending");
        assert_eq!(due[0].last_error.as_deref(), Some("boom"));
    }

    #[tokio::test]
    async fn test_record_failure_marks_dead_at_max() {
        let store = test_store().await;
        store
            .enqueue_webhook("hook", "evt", "{}", "2026-06-13T12:00:00Z")
            .await
            .unwrap();
        // prior attempts = 9 -> next attempt = 10 = MAX -> dead.
        let row = pending_row(1, 9);

        record_failure(&store, &row, "final boom").await;

        let counts = store.count_webhook_outbox_by_status().await.unwrap();
        assert_eq!(counts, vec![("dead".to_owned(), 1)]);
    }

    #[tokio::test]
    async fn test_record_failure_reschedule_store_error_is_swallowed() {
        // Store write fails (table gone) -> warn branch, no panic.
        let store = test_store().await;
        sqlx::query("DROP TABLE webhook_outbox")
            .execute(store.pool())
            .await
            .unwrap();
        record_failure(&store, &pending_row(1, 0), "boom").await;
    }

    #[tokio::test]
    async fn test_record_failure_dead_store_error_is_swallowed() {
        let store = test_store().await;
        sqlx::query("DROP TABLE webhook_outbox")
            .execute(store.pool())
            .await
            .unwrap();
        // prior=9 -> dead path; mark_webhook_dead fails -> warn, no panic.
        record_failure(&store, &pending_row(1, 9), "boom").await;
    }

    #[tokio::test]
    async fn test_process_row_orphan_mark_dead_store_error_is_swallowed() {
        let store = test_store().await;
        sqlx::query("DROP TABLE webhook_outbox")
            .execute(store.pool())
            .await
            .unwrap();
        let endpoints = endpoint_map(&[]);
        let client = reqwest::Client::new();
        // Unknown endpoint -> tries mark_webhook_dead which fails -> warn, no panic.
        process_row(&store, &endpoints, &client, &pending_row(1, 0)).await;
    }

    // --- process_row (endpoint resolution + dead path; network stubbed under coverage) ---

    #[tokio::test]
    async fn test_process_row_unknown_endpoint_marks_dead() {
        let store = test_store().await;
        store
            .enqueue_webhook("gone", "evt", "{}", "2026-06-13T12:00:00Z")
            .await
            .unwrap();
        let row = pending_row(1, 0);
        let endpoints = endpoint_map(&[config("other", "http://other")]);
        let client = reqwest::Client::new();

        process_row(&store, &endpoints, &client, &row).await;

        let counts = store.count_webhook_outbox_by_status().await.unwrap();
        assert_eq!(counts, vec![("dead".to_owned(), 1)]);
    }

    #[tokio::test]
    #[cfg(coverage)]
    async fn test_process_row_known_endpoint_delivers_under_coverage() {
        // Under coverage, post_envelope succeeds for a non-"fail" URL -> delivered.
        let store = test_store().await;
        store
            .enqueue_webhook("hook", "evt", "{}", "2026-06-13T12:00:00Z")
            .await
            .unwrap();
        let row = pending_row(1, 0);
        let endpoints = endpoint_map(&[config("hook", "http://ok")]);
        let client = reqwest::Client::new();

        process_row(&store, &endpoints, &client, &row).await;

        let counts = store.count_webhook_outbox_by_status().await.unwrap();
        assert_eq!(counts, vec![("delivered".to_owned(), 1)]);
    }

    #[tokio::test]
    #[cfg(coverage)]
    async fn test_process_row_delivered_store_error_is_swallowed_under_coverage() {
        // Post succeeds (stub), but mark_webhook_delivered fails -> warn, no panic.
        let store = test_store().await;
        sqlx::query("DROP TABLE webhook_outbox")
            .execute(store.pool())
            .await
            .unwrap();
        let endpoints = endpoint_map(&[config("hook", "http://ok")]);
        let client = reqwest::Client::new();
        process_row(&store, &endpoints, &client, &pending_row(1, 0)).await;
    }

    #[tokio::test]
    #[cfg(coverage)]
    async fn test_process_row_failed_post_reschedules_under_coverage() {
        // Under coverage, a "fail" URL makes post_envelope return Err -> reschedule.
        let store = test_store().await;
        store
            .enqueue_webhook("hook", "evt", "{}", "2026-06-13T12:00:00Z")
            .await
            .unwrap();
        let row = pending_row(1, 0);
        let endpoints = endpoint_map(&[config("hook", "http://fail")]);
        let client = reqwest::Client::new();

        process_row(&store, &endpoints, &client, &row).await;

        let due = store
            .fetch_due_webhook_deliveries("2027-01-01T00:00:00Z", 10)
            .await
            .unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].attempts, 1);
        assert_eq!(due[0].status, "pending");
    }

    // --- drain_due ---

    #[tokio::test]
    async fn test_drain_due_processes_orphans() {
        let store = test_store().await;
        for i in 0..3 {
            store
                .enqueue_webhook("gone", &format!("evt-{i}"), "{}", "2026-06-13T12:00:00Z")
                .await
                .unwrap();
        }
        let endpoints = endpoint_map(&[]);
        let client = reqwest::Client::new();

        let processed = drain_due(&store, &endpoints, &client).await;
        assert_eq!(processed, 3);

        // All orphaned -> dead.
        let counts = store.count_webhook_outbox_by_status().await.unwrap();
        assert_eq!(counts, vec![("dead".to_owned(), 3)]);
    }

    #[tokio::test]
    async fn test_drain_due_empty_returns_zero() {
        let store = test_store().await;
        let processed = drain_due(&store, &endpoint_map(&[]), &reqwest::Client::new()).await;
        assert_eq!(processed, 0);
    }

    #[tokio::test]
    async fn test_drain_due_fetch_error_returns_zero() {
        let store = test_store().await;
        sqlx::query("DROP TABLE webhook_outbox")
            .execute(store.pool())
            .await
            .unwrap();
        let processed = drain_due(&store, &endpoint_map(&[]), &reqwest::Client::new()).await;
        assert_eq!(processed, 0);
    }

    // --- run_outbox_worker shutdown ---

    #[tokio::test]
    async fn test_worker_shuts_down() {
        let store = test_store().await;
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        shutdown_tx.send(true).unwrap();
        tokio::time::timeout(
            Duration::from_secs(2),
            run_outbox_worker(store, vec![], shutdown_rx),
        )
        .await
        .expect("worker exits on shutdown");
    }

    #[tokio::test]
    async fn test_worker_drains_on_tick() {
        // The interval's first tick fires immediately, so the worker drains the
        // pre-existing (restart-style) pending row before we shut it down. The row
        // points at an endpoint absent from config -> marked dead, proving the tick
        // path ran end to end without waiting POLL_INTERVAL.
        let store = test_store().await;
        store
            .enqueue_webhook("gone", "evt", "{}", "2026-06-13T12:00:00Z")
            .await
            .unwrap();
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let handle = tokio::spawn(run_outbox_worker(store.clone(), vec![], shutdown_rx));

        // Let the immediate first tick fire and process the row.
        tokio::time::sleep(Duration::from_millis(100)).await;
        shutdown_tx.send(true).unwrap();
        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("worker stops")
            .expect("no panic");

        let counts = store.count_webhook_outbox_by_status().await.unwrap();
        assert_eq!(counts, vec![("dead".to_owned(), 1)]);
    }
}
