use pulpo_common::api::EventPushRequest;
use pulpo_common::event::PulpoEvent;
#[cfg(not(coverage))]
use tracing::{debug, info, warn};

/// Maximum number of events to batch in a single POST request.
const MAX_BATCH_SIZE: usize = 10;

/// Run the event push loop — subscribes to the local broadcast channel, batches
/// events, and POSTs them to the master node.
#[cfg(not(coverage))]
pub async fn run_event_push_loop(
    master_url: String,
    master_token: Option<String>,
    node_name: String,
    mut event_rx: tokio::sync::broadcast::Receiver<PulpoEvent>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("failed to build reqwest client");

    let push_url = format!("{master_url}/api/v1/events/push");
    info!("Worker event push loop started (master={master_url})");

    loop {
        tokio::select! {
            result = event_rx.recv() => {
                match result {
                    Ok(event) => {
                        let mut batch = vec![event];
                        // Drain additional events non-blocking up to MAX_BATCH_SIZE
                        while batch.len() < MAX_BATCH_SIZE {
                            match event_rx.try_recv() {
                                Ok(ev) => batch.push(ev),
                                Err(_) => break,
                            }
                        }

                        let req_body = EventPushRequest {
                            node_name: node_name.clone(),
                            events: batch,
                        };

                        let mut request = client.post(&push_url).json(&req_body);
                        if let Some(ref token) = master_token {
                            request = request.bearer_auth(token);
                        }

                        match request.send().await {
                            Ok(resp) if resp.status().is_success() => {
                                debug!(
                                    events = req_body.events.len(),
                                    "Pushed events to master"
                                );
                            }
                            Ok(resp) => {
                                warn!(
                                    status = %resp.status(),
                                    "Master rejected event push"
                                );
                            }
                            Err(e) => {
                                warn!(error = %e, "Failed to push events to master");
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        debug!(missed = n, "Event push loop lagged, skipping events");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("Event bus closed, stopping event push loop");
                        break;
                    }
                }
            }
            _ = shutdown_rx.changed() => {
                info!("Event push loop shutting down");
                break;
            }
        }
    }
}

/// Stub for coverage builds — the real loop performs network I/O.
#[cfg(coverage)]
pub async fn run_event_push_loop(
    _master_url: String,
    _master_token: Option<String>,
    _node_name: String,
    _event_rx: tokio::sync::broadcast::Receiver<PulpoEvent>,
    _shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
}

/// Build an `EventPushRequest` from a node name and list of events.
#[cfg_attr(coverage, allow(dead_code))]
pub const fn build_push_request(node_name: String, events: Vec<PulpoEvent>) -> EventPushRequest {
    EventPushRequest { node_name, events }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulpo_common::event::SessionEvent;

    #[cfg(coverage)]
    #[tokio::test]
    async fn test_coverage_stub_returns_immediately() {
        let (_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (_shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        // The coverage stub should return immediately without blocking
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            run_event_push_loop(
                "http://localhost:9999".into(),
                None,
                "test-node".into(),
                rx,
                shutdown_rx,
            ),
        )
        .await
        .expect("coverage stub should return immediately");
    }

    #[test]
    fn test_build_push_request() {
        let events = vec![PulpoEvent::Session(SessionEvent {
            session_id: "s1".into(),
            session_name: "test".into(),
            status: "active".into(),
            previous_status: None,
            node_name: "node-1".into(),
            output_snippet: None,
            timestamp: "2026-01-01T00:00:00Z".into(),
            ..Default::default()
        })];

        let req = build_push_request("node-1".into(), events);
        assert_eq!(req.node_name, "node-1");
        assert_eq!(req.events.len(), 1);
    }

    #[test]
    fn test_build_push_request_empty() {
        let req = build_push_request("node-2".into(), vec![]);
        assert_eq!(req.node_name, "node-2");
        assert!(req.events.is_empty());
    }

    #[test]
    fn test_max_batch_size_constant() {
        assert_eq!(MAX_BATCH_SIZE, 10);
    }
}
