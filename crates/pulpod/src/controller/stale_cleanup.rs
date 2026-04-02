use std::time::Duration;

use pulpo_common::event::{PulpoEvent, SessionEvent};
use tokio::sync::broadcast;
#[cfg(not(coverage))]
use tracing::{debug, info};

use crate::store::Store;

/// Run the stale cleanup loop on the controller node.
///
/// Every `check_interval` seconds, calls `mark_stale_workers` on the session index.
/// For each affected session entry, emits a `PulpoEvent::Session` with `status = "lost"`
/// so SSE subscribers and notification hooks see the change.
#[cfg(not(coverage))]
pub async fn run_stale_cleanup_loop(
    session_index: std::sync::Arc<super::SessionIndex>,
    store: Store,
    stale_timeout: Duration,
    check_interval: Duration,
    event_tx: broadcast::Sender<PulpoEvent>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    let mut interval = tokio::time::interval(check_interval);
    info!(
        timeout_secs = stale_timeout.as_secs(),
        "Controller stale cleanup loop started"
    );

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let affected = session_index.mark_stale_workers(stale_timeout).await;
                if !affected.is_empty() {
                    info!(count = affected.len(), "Marked stale node sessions as lost");
                }
                for entry in affected {
                    if let Err(e) = store.upsert_master_session_index_entry(&entry).await {
                        debug!(session_id = %entry.session_id, error = %e, "Failed to persist stale session index entry");
                    }
                    debug!(session_id = %entry.session_id, node = %entry.node_name, "Session marked lost (stale node)");
                    let event = PulpoEvent::Session(SessionEvent {
                        session_id: entry.session_id,
                        session_name: entry.session_name,
                        status: "lost".into(),
                        previous_status: Some("unknown".into()),
                        node_name: entry.node_name,
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        ..Default::default()
                    });
                    // Fire-and-forget: subscribers may have all dropped
                    let _ = event_tx.send(event);
                }
            }
            _ = shutdown_rx.changed() => {
                info!("Stale cleanup loop shutting down");
                break;
            }
        }
    }
}

/// Stub for coverage builds — the real loop performs time-based I/O.
#[cfg(coverage)]
pub async fn run_stale_cleanup_loop(
    _session_index: std::sync::Arc<super::SessionIndex>,
    _store: Store,
    _stale_timeout: Duration,
    _check_interval: Duration,
    _event_tx: broadcast::Sender<PulpoEvent>,
    _shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
}

/// Build a `PulpoEvent::Session` marking a session as lost.
pub fn build_lost_event(
    session_id: &str,
    session_name: &str,
    node_name: &str,
    timestamp: &str,
) -> PulpoEvent {
    PulpoEvent::Session(SessionEvent {
        session_id: session_id.to_owned(),
        session_name: session_name.to_owned(),
        status: "lost".into(),
        previous_status: Some("unknown".into()),
        node_name: node_name.to_owned(),
        timestamp: timestamp.to_owned(),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::SessionIndex;
    use crate::store::Store;
    use chrono::{DateTime, Utc};
    use pulpo_common::api::SessionIndexEntry;

    fn ts(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn make_entry(session_id: &str, node_name: &str) -> SessionIndexEntry {
        SessionIndexEntry {
            session_id: session_id.into(),
            node_name: node_name.into(),
            node_address: None,
            session_name: format!("task-{session_id}"),
            status: "active".into(),
            command: None,
            updated_at: "2026-03-30T12:00:00Z".into(),
        }
    }

    #[cfg(coverage)]
    #[tokio::test]
    async fn test_coverage_stub_returns_immediately() {
        let index = std::sync::Arc::new(SessionIndex::new());
        let (tx, _rx) = broadcast::channel(16);
        let (_shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            run_stale_cleanup_loop(
                index,
                crate::store::Store::new(tempfile::tempdir().unwrap().path().to_str().unwrap())
                    .await
                    .unwrap(),
                Duration::from_secs(300),
                Duration::from_secs(60),
                tx,
                shutdown_rx,
            ),
        )
        .await
        .expect("coverage stub should return immediately");
    }

    #[test]
    fn test_build_lost_event() {
        let event = build_lost_event("s1", "task-s1", "worker-1", "2026-03-30T12:00:00Z");
        let PulpoEvent::Session(se) = event else {
            panic!("expected session event");
        };
        assert_eq!(se.session_id, "s1");
        assert_eq!(se.session_name, "task-s1");
        assert_eq!(se.status, "lost");
        assert_eq!(se.previous_status, Some("unknown".into()));
        assert_eq!(se.node_name, "worker-1");
        assert_eq!(se.timestamp, "2026-03-30T12:00:00Z");
    }

    #[test]
    fn test_build_lost_event_fields() {
        let event = build_lost_event("abc", "my-task", "node-2", "2026-01-15T08:00:00Z");
        let PulpoEvent::Session(se) = event else {
            panic!("expected session event");
        };
        assert_eq!(se.session_id, "abc");
        assert_eq!(se.session_name, "my-task");
        assert_eq!(se.node_name, "node-2");
        assert_eq!(se.timestamp, "2026-01-15T08:00:00Z");
        // All enrichment fields should be None
        assert!(se.output_snippet.is_none());
        assert!(se.git_branch.is_none());
        assert!(se.error_status.is_none());
    }

    /// Verify that stale workers get their entries marked lost and events are emitted.
    /// This tests the core logic used by `run_stale_cleanup_loop` without running the loop.
    #[tokio::test]
    async fn test_stale_entries_produce_lost_events() {
        let index = SessionIndex::new();
        index
            .touch_worker_at("worker-1", ts("2026-03-30T12:00:00Z"))
            .await;
        index.upsert(make_entry("s1", "worker-1")).await;
        index.upsert(make_entry("s2", "worker-1")).await;

        let (event_tx, mut event_rx) = broadcast::channel(16);
        let affected = index
            .mark_stale_workers_at(ts("2026-03-30T12:06:40Z"), Duration::from_secs(300))
            .await;
        assert_eq!(affected.len(), 2);

        // Simulate what the loop does: emit an event for each affected entry
        for entry in &affected {
            let event = build_lost_event(
                &entry.session_id,
                &entry.session_name,
                &entry.node_name,
                "2026-03-30T12:00:00Z",
            );
            event_tx.send(event).unwrap();
        }

        // Both events should be received
        let ev1 = event_rx.try_recv().unwrap();
        let ev2 = event_rx.try_recv().unwrap();
        let PulpoEvent::Session(se1) = ev1 else {
            panic!("expected session event");
        };
        let PulpoEvent::Session(se2) = ev2 else {
            panic!("expected session event");
        };
        assert_eq!(se1.status, "lost");
        assert_eq!(se2.status, "lost");
        assert_eq!(se1.node_name, "worker-1");
    }

    #[tokio::test]
    async fn test_no_events_when_no_stale_workers() {
        let index = SessionIndex::new();
        index
            .touch_worker_at("worker-1", ts("2026-03-30T12:00:00Z"))
            .await;
        index.upsert(make_entry("s1", "worker-1")).await;

        let affected = index
            .mark_stale_workers_at(ts("2026-03-30T12:00:30Z"), Duration::from_secs(300))
            .await;
        assert!(affected.is_empty());

        // Session should still be active
        let s1 = index.get("s1").await.unwrap();
        assert_eq!(s1.status, "active");
    }

    #[tokio::test]
    async fn test_recovered_worker_is_not_re_marked_lost_after_stale_cleanup() {
        let tmpdir = tempfile::tempdir().unwrap();
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let index = SessionIndex::new();

        index
            .touch_worker_at("worker-1", ts("2026-03-30T12:00:00Z"))
            .await;
        let active_entry = make_entry("s1", "worker-1");
        index.upsert(active_entry.clone()).await;
        store
            .upsert_master_session_index_entry(&active_entry)
            .await
            .unwrap();

        let affected = index
            .mark_stale_workers_at(ts("2026-03-30T12:06:40Z"), Duration::from_secs(300))
            .await;
        assert_eq!(affected.len(), 1);
        assert_eq!(affected[0].status, "lost");
        store
            .upsert_master_session_index_entry(&affected[0])
            .await
            .unwrap();

        index
            .touch_worker_at("worker-1", ts("2026-03-30T12:07:00Z"))
            .await;
        let recovered_entry = SessionIndexEntry {
            status: "active".into(),
            updated_at: "2026-03-30T12:07:00Z".into(),
            ..active_entry
        };
        index.upsert(recovered_entry.clone()).await;
        store
            .upsert_master_session_index_entry(&recovered_entry)
            .await
            .unwrap();

        let affected = index
            .mark_stale_workers_at(ts("2026-03-30T12:08:00Z"), Duration::from_secs(300))
            .await;
        assert!(
            affected.is_empty(),
            "freshly recovered worker should not be re-marked lost"
        );

        let entry = index.get("s1").await.unwrap();
        assert_eq!(entry.status, "active");
        let persisted = store.list_master_session_index_entries().await.unwrap();
        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0].status, "active");
    }

    #[tokio::test]
    async fn test_stale_cleanup_only_marks_workers_with_expired_heartbeat() {
        let index = SessionIndex::new();
        index
            .touch_worker_at("worker-1", ts("2026-03-30T12:00:00Z"))
            .await;
        index
            .touch_worker_at("worker-2", ts("2026-03-30T12:06:30Z"))
            .await;
        index.upsert(make_entry("s1", "worker-1")).await;
        index.upsert(make_entry("s2", "worker-2")).await;

        let affected = index
            .mark_stale_workers_at(ts("2026-03-30T12:06:40Z"), Duration::from_secs(300))
            .await;
        assert_eq!(affected.len(), 1);
        assert_eq!(affected[0].session_id, "s1");
        assert_eq!(affected[0].status, "lost");

        let worker_1 = index.get("s1").await.unwrap();
        let worker_2 = index.get("s2").await.unwrap();
        assert_eq!(worker_1.status, "lost");
        assert_eq!(worker_2.status, "active");
    }
}
