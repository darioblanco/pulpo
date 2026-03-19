use pulpo_common::event::{PulpoEvent, SessionEvent};
#[cfg_attr(coverage, allow(unused_imports))]
use tracing::{error, info};

use crate::store::Store;

/// Builds and sends Web Push notifications for session events.
pub struct WebPushNotifier {
    #[cfg_attr(coverage, allow(dead_code))]
    store: Store,
    #[cfg_attr(coverage, allow(dead_code))]
    vapid_private_key: String,
}

/// Builds the Web Push notification payload JSON for a session event.
pub fn build_payload(event: &SessionEvent) -> String {
    let title = format!("Session: {}", event.session_name);
    let body = format!("Session `{}` is now {}", event.session_name, event.status);
    serde_json::json!({
        "title": title,
        "body": body,
        "url": format!("/sessions/{}", event.session_id),
        "icon": "/icon-192.png",
        "status": event.status,
        "session_id": event.session_id,
        "session_name": event.session_name,
        "node_name": event.node_name,
    })
    .to_string()
}

impl WebPushNotifier {
    /// Create a new `WebPushNotifier` from store and VAPID private key.
    pub const fn new(store: Store, vapid_private_key: String) -> Self {
        Self {
            store,
            vapid_private_key,
        }
    }

    /// Send a web push notification to all subscriptions.
    /// Gated with `#[cfg(not(coverage))]` because it requires real HTTP to push services.
    #[cfg(not(coverage))]
    pub async fn send(&self, event: &SessionEvent) {
        use web_push::WebPushClient;

        let payload = build_payload(event);
        let subs = match self.store.list_push_subscriptions().await {
            Ok(subs) => subs,
            Err(e) => {
                error!(error = %e, "Failed to list push subscriptions");
                return;
            }
        };

        if subs.is_empty() {
            return;
        }

        info!(
            session = %event.session_name,
            status = %event.status,
            subscribers = subs.len(),
            "Sending web push notifications"
        );

        let partial_builder =
            match web_push::VapidSignatureBuilder::from_base64_no_sub(&self.vapid_private_key) {
                Ok(b) => b,
                Err(e) => {
                    error!(error = %e, "Failed to create VAPID signature builder");
                    return;
                }
            };

        let client = match web_push::IsahcWebPushClient::new() {
            Ok(c) => c,
            Err(e) => {
                error!(error = %e, "Failed to create web push client");
                return;
            }
        };

        for sub in &subs {
            let subscription_info = web_push::SubscriptionInfo {
                endpoint: sub.endpoint.clone(),
                keys: web_push::SubscriptionKeys {
                    p256dh: sub.p256dh.clone(),
                    auth: sub.auth.clone(),
                },
            };

            let sig = match partial_builder
                .clone()
                .add_sub_info(&subscription_info)
                .build()
            {
                Ok(s) => s,
                Err(e) => {
                    error!(error = %e, endpoint = %sub.endpoint, "Failed to build VAPID signature");
                    continue;
                }
            };

            let mut builder = web_push::WebPushMessageBuilder::new(&subscription_info);
            builder.set_payload(web_push::ContentEncoding::Aes128Gcm, payload.as_bytes());
            builder.set_vapid_signature(sig);

            let message = match builder.build() {
                Ok(m) => m,
                Err(e) => {
                    error!(error = %e, endpoint = %sub.endpoint, "Failed to build web push message");
                    continue;
                }
            };

            if let Err(e) = client.send(message).await {
                tracing::warn!(
                    error = %e,
                    endpoint = %sub.endpoint,
                    "Web push send failed (removing stale subscription)"
                );
                // Remove stale subscriptions
                if let Err(del_err) = self.store.delete_push_subscription(&sub.endpoint).await {
                    error!(error = %del_err, "Failed to remove stale subscription");
                }
            }
        }
    }

    /// Stub for coverage builds — does nothing.
    #[cfg(coverage)]
    pub async fn send(&self, _event: &SessionEvent) {}
}

/// Run the notification loop — subscribes to the event bus and sends web push notifications.
pub async fn run_notification_loop(
    notifier: WebPushNotifier,
    mut rx: tokio::sync::broadcast::Receiver<PulpoEvent>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event) => match event {
                        PulpoEvent::Session(ref se) => {
                            notifier.send(se).await;
                        }
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(missed = n, "Web Push notifier lagged, skipping events");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("Event bus closed, stopping Web Push notifier");
                        break;
                    }
                }
            }
            _ = shutdown.changed() => {
                info!("Web Push notifier shutting down");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_event(status: &str) -> SessionEvent {
        SessionEvent {
            session_id: "abc-123".into(),
            session_name: "my-session".into(),
            status: status.into(),
            previous_status: None,
            node_name: "node-1".into(),
            output_snippet: None,
            timestamp: "2026-01-01T00:00:00Z".into(),
        }
    }

    // --- build_payload tests ---

    #[test]
    fn test_build_payload_basic() {
        let event = test_event("active");
        let payload_str = build_payload(&event);
        let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
        assert_eq!(payload["title"], "Session: my-session");
        assert!(payload["body"].as_str().unwrap().contains("my-session"));
        assert!(payload["body"].as_str().unwrap().contains("active"));
        assert_eq!(payload["url"], "/sessions/abc-123");
        assert_eq!(payload["icon"], "/icon-192.png");
        assert_eq!(payload["status"], "active");
        assert_eq!(payload["session_id"], "abc-123");
        assert_eq!(payload["session_name"], "my-session");
        assert_eq!(payload["node_name"], "node-1");
    }

    #[test]
    fn test_build_payload_killed() {
        let event = test_event("killed");
        let payload_str = build_payload(&event);
        let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
        assert_eq!(payload["status"], "killed");
        assert!(payload["body"].as_str().unwrap().contains("killed"));
    }

    #[test]
    fn test_build_payload_ready() {
        let event = test_event("ready");
        let payload_str = build_payload(&event);
        let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
        assert_eq!(payload["status"], "ready");
    }

    #[test]
    fn test_build_payload_with_special_chars() {
        let event = SessionEvent {
            session_id: "id-1".into(),
            session_name: "session with \"quotes\"".into(),
            status: "active".into(),
            previous_status: None,
            node_name: "node".into(),
            output_snippet: None,
            timestamp: "t".into(),
        };
        let payload_str = build_payload(&event);
        // Should produce valid JSON even with special characters
        let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
        assert!(
            payload["title"]
                .as_str()
                .unwrap()
                .contains("session with \"quotes\"")
        );
    }

    // --- WebPushNotifier tests ---

    #[tokio::test]
    async fn test_notifier_new() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let notifier = WebPushNotifier::new(store, "priv-key".into());
        // Under coverage, send is a no-op stub
        notifier.send(&test_event("active")).await;
    }

    // --- run_notification_loop tests ---

    #[tokio::test]
    async fn test_notification_loop_shutdown() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let notifier = WebPushNotifier::new(store, "priv".into());
        let (event_tx, _) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let rx = event_tx.subscribe();
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        shutdown_tx.send(true).unwrap();

        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            run_notification_loop(notifier, rx, shutdown_rx),
        )
        .await
        .expect("notification loop should exit on shutdown");
    }

    #[tokio::test]
    async fn test_notification_loop_channel_closed() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let notifier = WebPushNotifier::new(store, "priv".into());
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (_shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        drop(event_tx);

        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            run_notification_loop(notifier, rx, shutdown_rx),
        )
        .await
        .expect("notification loop should exit when channel closes");
    }

    #[tokio::test]
    async fn test_notification_loop_processes_event() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let notifier = WebPushNotifier::new(store, "priv".into());
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        event_tx
            .send(PulpoEvent::Session(test_event("active")))
            .unwrap();

        let handle = tokio::spawn(run_notification_loop(notifier, rx, shutdown_rx));

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        shutdown_tx.send(true).unwrap();

        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("should finish")
            .expect("should not panic");
    }

    #[tokio::test]
    async fn test_notification_loop_lagged() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let notifier = WebPushNotifier::new(store, "priv".into());
        // Tiny buffer to force lag
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(1);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        // Overflow the buffer before the loop starts
        for i in 0..5 {
            let _ = event_tx.send(PulpoEvent::Session(SessionEvent {
                session_id: format!("id-{i}"),
                session_name: "s".into(),
                status: "active".into(),
                previous_status: None,
                node_name: "n".into(),
                output_snippet: None,
                timestamp: "t".into(),
            }));
        }

        let handle = tokio::spawn(run_notification_loop(notifier, rx, shutdown_rx));

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        shutdown_tx.send(true).unwrap();

        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("should finish")
            .expect("should not panic");
    }
}
