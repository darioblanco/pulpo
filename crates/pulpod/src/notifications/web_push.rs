use std::fmt::Write;

use pulpo_common::event::Event;
#[cfg_attr(coverage, allow(unused_imports))]
use tracing::{error, info};

use crate::store::Store;

/// Builds and sends Web Push notifications for canonical events.
///
/// A sink in the event dispatcher: it `wants` session lifecycle events (phone
/// alerts on state changes) and renders the canonical [`Event`] into a push
/// title/body. Usage alerts stay on webhooks for now (no regression to the
/// previous session-only behavior).
pub struct WebPushSink {
    #[cfg_attr(coverage, allow(dead_code))]
    store: Store,
    #[cfg_attr(coverage, allow(dead_code))]
    vapid_private_key: String,
}

/// Builds a concise, enriched body line for a web push notification.
fn build_body(event: &Event) -> String {
    let session = event.session.as_ref();
    let name = session.map_or("session", |s| s.name.as_str());
    let mut body = format!("Session `{name}` is now {}", event.subtype);

    if let Some(session) = session {
        // Append PR info.
        if session.pr_url.is_some() {
            body.push_str(" — created PR");
        }
        // Append branch.
        if let Some(branch) = &session.git_branch {
            let _ = write!(body, " on branch {branch}");
        }
    }

    body
}

/// Builds the Web Push notification payload JSON for a canonical event.
pub fn build_payload(event: &Event) -> String {
    let session = event.session.as_ref();
    let name = session.map_or("session", |s| s.name.as_str());
    let id = session.map_or("", |s| s.id.as_str());
    let title = format!("Session: {name}");
    let body = build_body(event);
    serde_json::json!({
        "title": title,
        "body": body,
        "url": format!("/sessions/{id}"),
        "icon": "/icon-192.png",
        "status": event.subtype,
        "session_id": id,
        "session_name": name,
        "node_name": event.node,
    })
    .to_string()
}

impl WebPushSink {
    /// Create a new `WebPushSink` from store and VAPID private key.
    pub const fn new(store: Store, vapid_private_key: String) -> Self {
        Self {
            store,
            vapid_private_key,
        }
    }

    /// Sink name.
    pub const fn name(&self) -> &'static str {
        "web-push"
    }

    /// Whether web push wants this event. Only lifecycle (session) events —
    /// matches the pre-dispatcher behavior of notifying on session changes.
    pub fn wants(&self, event: &Event) -> bool {
        event.event_type == "lifecycle"
    }

    /// Send a web push notification to all subscriptions. Best-effort; logs failures.
    /// Gated with `#[cfg(not(coverage))]` because it requires real HTTP to push services.
    #[cfg(not(coverage))]
    pub async fn deliver(&self, event: &Event) {
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
            event = %format!("{}.{}", event.event_type, event.subtype),
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
                // Remove stale subscriptions.
                if let Err(del_err) = self.store.delete_push_subscription(&sub.endpoint).await {
                    error!(error = %del_err, "Failed to remove stale subscription");
                }
            }
        }
    }

    /// Stub for coverage builds — does nothing.
    #[cfg(coverage)]
    pub async fn deliver(&self, _event: &Event) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulpo_common::event::EventSessionRef;

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

    // --- build_payload tests ---

    #[test]
    fn test_build_payload_basic() {
        let event = lifecycle_event("active");
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
    fn test_build_payload_stopped() {
        let event = lifecycle_event("stopped");
        let payload_str = build_payload(&event);
        let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
        assert_eq!(payload["status"], "stopped");
        assert!(payload["body"].as_str().unwrap().contains("stopped"));
    }

    #[test]
    fn test_build_payload_no_session_falls_back() {
        let mut event = lifecycle_event("ready");
        event.session = None;
        let payload_str = build_payload(&event);
        let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
        assert_eq!(payload["title"], "Session: session");
        assert_eq!(payload["url"], "/sessions/");
        assert_eq!(payload["session_id"], "");
    }

    #[test]
    fn test_build_body_with_pr_and_branch() {
        let mut event = lifecycle_event("ready");
        if let Some(s) = event.session.as_mut() {
            s.pr_url = Some("https://github.com/org/repo/pull/42".into());
            s.git_branch = Some("main".into());
        }
        let body = build_body(&event);
        assert_eq!(
            body,
            "Session `my-session` is now ready — created PR on branch main"
        );
    }

    #[test]
    fn test_build_body_with_branch_only() {
        let mut event = lifecycle_event("ready");
        if let Some(s) = event.session.as_mut() {
            s.git_branch = Some("fix-auth".into());
        }
        let body = build_body(&event);
        assert_eq!(body, "Session `my-session` is now ready on branch fix-auth");
    }

    #[test]
    fn test_build_body_with_pr_no_branch() {
        let mut event = lifecycle_event("ready");
        if let Some(s) = event.session.as_mut() {
            s.pr_url = Some("https://github.com/org/repo/pull/1".into());
        }
        let body = build_body(&event);
        assert_eq!(body, "Session `my-session` is now ready — created PR");
    }

    #[test]
    fn test_build_body_plain() {
        let event = lifecycle_event("active");
        let body = build_body(&event);
        assert_eq!(body, "Session `my-session` is now active");
    }

    #[test]
    fn test_build_body_no_session() {
        let mut event = lifecycle_event("active");
        event.session = None;
        let body = build_body(&event);
        assert_eq!(body, "Session `session` is now active");
    }

    #[test]
    fn test_build_payload_with_special_chars() {
        let mut event = lifecycle_event("active");
        if let Some(s) = event.session.as_mut() {
            s.name = "session with \"quotes\"".into();
        }
        let payload_str = build_payload(&event);
        // Should produce valid JSON even with special characters.
        let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
        assert!(
            payload["title"]
                .as_str()
                .unwrap()
                .contains("session with \"quotes\"")
        );
    }

    // --- wants tests ---

    #[tokio::test]
    async fn test_wants_lifecycle_only() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let sink = WebPushSink::new(store, "priv".into());
        assert_eq!(sink.name(), "web-push");
        assert!(sink.wants(&lifecycle_event("active")));

        let mut usage = lifecycle_event("budget_threshold");
        usage.event_type = "usage_alert".into();
        assert!(!sink.wants(&usage));
    }

    // --- deliver (coverage stub) ---

    #[tokio::test]
    async fn test_deliver_is_callable() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let sink = WebPushSink::new(store, "priv-key".into());
        // Under coverage, deliver is a no-op stub; otherwise no subscriptions -> no-op.
        sink.deliver(&lifecycle_event("active")).await;
    }

    /// End-to-end web-push delivery against a mock gateway: real VAPID signing + ECE
    /// encryption (valid keys) → HTTP → a 410 prunes the stale subscription while a 2xx
    /// keeps it. Gated `not(coverage)` (real HTTP via the web-push/isahc client).
    #[cfg(not(coverage))]
    #[tokio::test]
    async fn test_deliver_prunes_stale_subscription_on_410_keeps_delivered() {
        use base64::Engine;
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use p256::elliptic_curve::sec1::ToEncodedPoint;

        // Mock push gateway: /gone → 410 Gone (stale), /ok → 201 Created (delivered).
        let app = axum::Router::new()
            .route(
                "/gone",
                axum::routing::post(|| async { axum::http::StatusCode::GONE }),
            )
            .route(
                "/ok",
                axum::routing::post(|| async { axum::http::StatusCode::CREATED }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let tmpdir = Box::leak(Box::new(tempfile::tempdir().unwrap()));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();

        // A valid VAPID private key, in the exact format the daemon generates.
        let mut cfg = crate::config::Config::default();
        crate::config::ensure_vapid_keys(&mut cfg);
        let vapid = cfg.notifications.vapid.private_key.clone();

        // Two subscriptions backed by real P-256 recipient keypairs so ECE encryption
        // actually succeeds (the part that needs valid keys).
        let make_sub = |path: &str| {
            let recip = p256::SecretKey::random(&mut p256::elliptic_curve::rand_core::OsRng);
            let p256dh =
                URL_SAFE_NO_PAD.encode(recip.public_key().to_encoded_point(false).as_bytes());
            let auth = URL_SAFE_NO_PAD.encode([7u8; 16]); // 16-byte auth secret
            (format!("http://{addr}{path}"), p256dh, auth)
        };
        let (gone_ep, gp, ga) = make_sub("/gone");
        let (ok_ep, op, oa) = make_sub("/ok");
        store
            .save_push_subscription(&gone_ep, &gp, &ga)
            .await
            .unwrap();
        store
            .save_push_subscription(&ok_ep, &op, &oa)
            .await
            .unwrap();

        let sink = WebPushSink::new(store.clone(), vapid);
        sink.deliver(&lifecycle_event("active")).await;

        let remaining: Vec<String> = store
            .list_push_subscriptions()
            .await
            .unwrap()
            .into_iter()
            .map(|s| s.endpoint)
            .collect();
        assert!(
            !remaining.contains(&gone_ep),
            "410 (Gone) subscription should be pruned, remaining={remaining:?}"
        );
        assert!(
            remaining.contains(&ok_ep),
            "successfully-delivered subscription should be kept, remaining={remaining:?}"
        );
    }
}
