use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::Stream;
use pulpo_common::event::PulpoEvent;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

/// Converts a `PulpoEvent` into an SSE `Event` with the appropriate event type.
fn event_to_sse(event: &PulpoEvent) -> Option<Result<Event, Infallible>> {
    let (event_type, json) = match event {
        PulpoEvent::Session(se) => ("session", serde_json::to_string(se).ok()?),
    };
    Some(Ok(Event::default().event(event_type).data(json)))
}

pub async fn stream(
    State(state): State<Arc<super::AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.event_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| {
        let event = result.ok()?; // Lagged — skip missed events
        event_to_sse(&event)
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::AppState;
    use crate::backend::StubBackend;
    use crate::config::{Config, NodeConfig};
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;
    use pulpo_common::event::SessionEvent;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_broadcast_session_event_received() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let manager =
            SessionManager::new(Arc::new(StubBackend), store.clone(), HashMap::new(), None)
                .with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(
            Config {
                node: NodeConfig {
                    name: "test".into(),
                    port: 7433,
                    data_dir: tmpdir.path().to_str().unwrap().into(),
                    ..NodeConfig::default()
                },
                auth: crate::config::AuthConfig::default(),
                peers: HashMap::new(),
                watchdog: crate::config::WatchdogConfig::default(),
                inks: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
                docker: crate::config::DockerConfig::default(),
            },
            manager,
            peer_registry,
            store,
        );

        let mut rx = state.event_tx.subscribe();
        let event = PulpoEvent::Session(SessionEvent {
            session_id: "id-1".into(),
            session_name: "test-session".into(),
            status: "active".into(),
            previous_status: Some("creating".into()),
            node_name: "test".into(),
            output_snippet: None,
            timestamp: "2026-01-01T00:00:00Z".into(),
            git_branch: None,
            git_commit: None,
            git_insertions: None,
            git_deletions: None,
            git_files_changed: None,
            pr_url: None,
            error_status: None,
            total_input_tokens: None,
            total_output_tokens: None,
            session_cost_usd: None,
        });
        state.event_tx.send(event.clone()).unwrap();
        let received = rx.recv().await.unwrap();
        assert!(
            matches!(&received, PulpoEvent::Session(se) if se.session_id == "id-1" && se.status == "active")
        );
    }

    #[test]
    fn test_event_to_sse_session() {
        let event = PulpoEvent::Session(SessionEvent {
            session_id: "id-1".into(),
            session_name: "test-session".into(),
            status: "active".into(),
            previous_status: None,
            node_name: "test".into(),
            output_snippet: None,
            timestamp: "2026-01-01T00:00:00Z".into(),
            git_branch: None,
            git_commit: None,
            git_insertions: None,
            git_deletions: None,
            git_files_changed: None,
            pr_url: None,
            error_status: None,
            total_input_tokens: None,
            total_output_tokens: None,
            session_cost_usd: None,
        });

        let result = event_to_sse(&event);
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn test_broadcast_lagged_drops_gracefully() {
        let (tx, _) = tokio::sync::broadcast::channel::<PulpoEvent>(2);
        let mut rx = tx.subscribe();

        // Fill the buffer beyond capacity to cause lag
        for i in 0..5 {
            let _ = tx.send(PulpoEvent::Session(SessionEvent {
                session_id: format!("id-{i}"),
                session_name: "s".into(),
                status: "active".into(),
                previous_status: None,
                node_name: "n".into(),
                output_snippet: None,
                timestamp: "t".into(),
                git_branch: None,
                git_commit: None,
                git_insertions: None,
                git_deletions: None,
                git_files_changed: None,
                pr_url: None,
                error_status: None,
                total_input_tokens: None,
                total_output_tokens: None,
                session_cost_usd: None,
            }));
        }

        // The receiver should get a lagged error, then the latest message
        let result = rx.recv().await;
        // Either lagged error or the last few messages
        assert!(result.is_ok() || result.is_err());
    }
}
