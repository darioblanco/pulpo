use std::fmt::Write;

use pulpo_common::event::Event;
#[cfg_attr(coverage, allow(unused_imports))]
use tracing::{error, info};

use super::action_token::{self, STOP_ACTION};
use crate::store::Store;

/// Builds and sends Web Push notifications for canonical events.
///
/// A sink in the event dispatcher: it `wants` session lifecycle changes,
/// usage/cost alerts, and interventions — every event type that's useful on a
/// phone lock screen — and renders the canonical [`Event`] into a push
/// title/body. `usage_alert` payloads additionally carry a short-lived "Stop
/// session" action token (see [`action_token`]); the daemon already stopped the
/// session by the time an `intervention` event fires, so there's nothing left
/// to action there.
pub struct WebPushSink {
    #[cfg_attr(coverage, allow(dead_code))]
    store: Store,
    #[cfg_attr(coverage, allow(dead_code))]
    vapid_private_key: String,
    #[cfg_attr(coverage, allow(dead_code))]
    action_secret: String,
}

/// Builds a concise, enriched body line for a lifecycle push notification.
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

/// Round a cost/budget ratio to a whole-number percentage (0 when `budget <= 0.0`
/// to avoid a division-by-zero/NaN percentage).
fn percent_of(cost: f64, budget: f64) -> u32 {
    if budget <= 0.0 {
        return 0;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let pct = (cost / budget * 100.0).round() as u32;
    pct
}

/// Title + body for a `usage_alert` event — the "intervention-imminent" alert
/// (fires *before* the watchdog would auto-stop the session), so this is the
/// notification that carries the actionable "Stop session" button.
fn build_usage_alert_title_body(event: &Event) -> (String, String) {
    let session = event.session.as_ref();
    let name = session.map_or("session", |s| s.name.as_str());
    let cost = event
        .payload
        .get("cost_usd")
        .and_then(serde_json::Value::as_f64);
    let budget = event
        .payload
        .get("budget_usd")
        .and_then(serde_json::Value::as_f64);

    match event.subtype.as_str() {
        "budget_threshold" => {
            let title = format!("Budget alert: {name}");
            let body = if let (Some(cost), Some(budget)) = (cost, budget) {
                let pct = percent_of(cost, budget);
                format!("{name} at {pct}% (${cost:.2}/${budget:.2})")
            } else {
                format!("Session `{name}` reached its cost budget threshold")
            };
            (title, body)
        }
        "burn_ceiling" => {
            let title = format!("Burn rate alert: {name}");
            let body = cost.map_or_else(
                || format!("Session `{name}` is burning above its rate ceiling"),
                |cost| format!("{name} is burning above its rate ceiling (cost so far ${cost:.2})"),
            );
            (title, body)
        }
        other => (
            format!("Usage alert: {name}"),
            format!("Session `{name}`: {other}"),
        ),
    }
}

/// Title + body for an `intervention` event — the daemon already stopped the
/// session by the time this fires, so it's informational only.
fn build_intervention_title_body(event: &Event) -> (String, String) {
    let session = event.session.as_ref();
    let name = session.map_or("session", |s| s.name.as_str());
    let reason = event
        .payload
        .get("intervention_reason")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("pulpo took action");
    let title = format!("Intervention: {name}");
    let body = format!("Session `{name}` stopped — {reason}");
    (title, body)
}

/// Dispatches to the right title/body builder for the event's type. Lifecycle
/// keeps its pre-existing "Session: {name}" title (unchanged from before
/// `usage_alert`/`intervention` push support was added).
fn build_title_body(event: &Event) -> (String, String) {
    match event.event_type.as_str() {
        "usage_alert" => build_usage_alert_title_body(event),
        "intervention" => build_intervention_title_body(event),
        _ => {
            let name = event
                .session
                .as_ref()
                .map_or("session", |s| s.name.as_str());
            (format!("Session: {name}"), build_body(event))
        }
    }
}

/// Builds the Web Push notification payload JSON for a canonical event.
///
/// `action_secret` signs the "Stop session" action token attached to
/// `usage_alert` payloads (see [`action_token::sign_action_token`]). When it's
/// empty (VAPID keys not yet provisioned) or the event has no session, the
/// payload is built without an `action` field — a push notification without a
/// stop button rather than a broken one.
pub fn build_payload(event: &Event, action_secret: &str) -> String {
    let session = event.session.as_ref();
    let name = session.map_or("session", |s| s.name.as_str());
    let id = session.map_or("", |s| s.id.as_str());
    let (title, body) = build_title_body(event);
    let mut payload = serde_json::json!({
        "title": title,
        "body": body,
        "url": format!("/sessions/{id}"),
        "icon": "/icon-192.png",
        "status": event.subtype,
        "session_id": id,
        "session_name": name,
        "node_name": event.node,
    });

    if event.event_type == "usage_alert" && !id.is_empty() && !action_secret.is_empty() {
        let token = action_token::sign_action_token(
            action_secret,
            id,
            STOP_ACTION,
            chrono::Utc::now(),
            action_token::DEFAULT_TTL_SECS,
        );
        payload["action"] = serde_json::json!({
            "token": token,
            "label": "Stop session",
        });
    }

    payload.to_string()
}

impl WebPushSink {
    /// Create a new `WebPushSink` from store, VAPID private key, and the push
    /// action-token HMAC secret (see [`action_token`]).
    pub const fn new(store: Store, vapid_private_key: String, action_secret: String) -> Self {
        Self {
            store,
            vapid_private_key,
            action_secret,
        }
    }

    /// Sink name.
    pub const fn name(&self) -> &'static str {
        "web-push"
    }

    /// Whether web push wants this event: lifecycle (session status changes),
    /// `usage_alert` (budget/burn alerts — carries the "Stop session" action),
    /// and `intervention` (informational — the daemon already acted).
    pub fn wants(&self, event: &Event) -> bool {
        matches!(
            event.event_type.as_str(),
            "lifecycle" | "usage_alert" | "intervention"
        )
    }

    /// Send a web push notification to all subscriptions. Best-effort; logs failures.
    /// Gated with `#[cfg(not(coverage))]` because it requires real HTTP to push services.
    #[cfg(not(coverage))]
    pub async fn deliver(&self, event: &Event) {
        use web_push::WebPushClient;

        let payload = build_payload(event, &self.action_secret);
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
        let payload_str = build_payload(&event, "");
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
        let payload_str = build_payload(&event, "");
        let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
        assert_eq!(payload["status"], "stopped");
        assert!(payload["body"].as_str().unwrap().contains("stopped"));
    }

    #[test]
    fn test_build_payload_no_session_falls_back() {
        let mut event = lifecycle_event("ready");
        event.session = None;
        let payload_str = build_payload(&event, "");
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
        let payload_str = build_payload(&event, "");
        // Should produce valid JSON even with special characters.
        let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
        assert!(
            payload["title"]
                .as_str()
                .unwrap()
                .contains("session with \"quotes\"")
        );
    }

    fn usage_alert_event(subtype: &str, cost_usd: Option<f64>, budget_usd: Option<f64>) -> Event {
        let mut payload = serde_json::Map::new();
        if let Some(cost) = cost_usd {
            payload.insert("cost_usd".into(), serde_json::json!(cost));
        }
        if let Some(budget) = budget_usd {
            payload.insert("budget_usd".into(), serde_json::json!(budget));
        }
        Event {
            schema_version: 1,
            event_id: "evt-2".into(),
            event_type: "usage_alert".into(),
            subtype: subtype.into(),
            severity: "warn".into(),
            occurred_at: "2026-06-13T12:00:00Z".into(),
            node: "node-1".into(),
            session: Some(EventSessionRef {
                id: "sess-42".into(),
                name: "fix-auth".into(),
                status: String::new(),
                ..Default::default()
            }),
            payload: serde_json::Value::Object(payload),
        }
    }

    fn intervention_event(code: &str, reason: &str) -> Event {
        Event {
            schema_version: 1,
            event_id: "evt-3".into(),
            event_type: "intervention".into(),
            subtype: code.into(),
            severity: "critical".into(),
            occurred_at: "2026-06-13T12:00:00Z".into(),
            node: "node-1".into(),
            session: Some(EventSessionRef {
                id: "sess-42".into(),
                name: "fix-auth".into(),
                status: "stopped".into(),
                ..Default::default()
            }),
            payload: serde_json::json!({ "intervention_reason": reason }),
        }
    }

    // --- percent_of ---

    #[test]
    fn test_percent_of_rounds() {
        assert_eq!(percent_of(8.2, 10.0), 82);
        assert_eq!(percent_of(0.85, 1.0), 85);
    }

    #[test]
    fn test_percent_of_zero_budget_is_zero() {
        assert_eq!(percent_of(5.0, 0.0), 0);
    }

    // --- usage_alert payload tests ---

    #[test]
    fn test_usage_alert_budget_threshold_title_body() {
        let event = usage_alert_event("budget_threshold", Some(8.2), Some(10.0));
        let payload_str = build_payload(&event, "");
        let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
        assert_eq!(payload["title"], "Budget alert: fix-auth");
        let body = payload["body"].as_str().unwrap();
        assert!(body.contains("82%"), "body was: {body}");
        assert!(body.contains("$8.20"));
        assert!(body.contains("$10.00"));
    }

    #[test]
    fn test_usage_alert_budget_threshold_missing_fields_falls_back() {
        let event = usage_alert_event("budget_threshold", None, None);
        let payload_str = build_payload(&event, "");
        let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
        assert_eq!(
            payload["body"],
            "Session `fix-auth` reached its cost budget threshold"
        );
    }

    #[test]
    fn test_usage_alert_burn_ceiling_title_body() {
        let event = usage_alert_event("burn_ceiling", Some(12.5), None);
        let payload_str = build_payload(&event, "");
        let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
        assert_eq!(payload["title"], "Burn rate alert: fix-auth");
        let body = payload["body"].as_str().unwrap();
        assert!(body.contains("$12.50"), "body was: {body}");
        assert!(body.contains("burning above its rate ceiling"));
    }

    #[test]
    fn test_usage_alert_burn_ceiling_no_cost_falls_back() {
        let event = usage_alert_event("burn_ceiling", None, None);
        let payload_str = build_payload(&event, "");
        let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
        assert_eq!(
            payload["body"],
            "Session `fix-auth` is burning above its rate ceiling"
        );
    }

    #[test]
    fn test_usage_alert_unknown_subtype_generic_fallback() {
        let event = usage_alert_event("quota_threshold", None, None);
        let payload_str = build_payload(&event, "");
        let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
        assert_eq!(payload["title"], "Usage alert: fix-auth");
        assert_eq!(payload["body"], "Session `fix-auth`: quota_threshold");
    }

    #[test]
    fn test_usage_alert_attaches_action_token_when_secret_present() {
        let event = usage_alert_event("budget_threshold", Some(8.2), Some(10.0));
        let payload_str = build_payload(&event, "test-action-secret");
        let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
        assert_eq!(payload["action"]["label"], "Stop session");
        let token = payload["action"]["token"].as_str().unwrap();

        // The attached token verifies and is bound to this session.
        let session_id = action_token::verify_action_token(
            "test-action-secret",
            token,
            STOP_ACTION,
            chrono::Utc::now(),
        )
        .unwrap();
        assert_eq!(session_id, "sess-42");
    }

    #[test]
    fn test_usage_alert_no_action_token_when_secret_empty() {
        let event = usage_alert_event("budget_threshold", Some(1.0), Some(2.0));
        let payload_str = build_payload(&event, "");
        let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
        assert!(payload.get("action").is_none());
    }

    #[test]
    fn test_usage_alert_no_action_token_when_no_session() {
        let mut event = usage_alert_event("budget_threshold", Some(1.0), Some(2.0));
        event.session = None;
        let payload_str = build_payload(&event, "test-action-secret");
        let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
        assert!(payload.get("action").is_none());
    }

    #[test]
    fn test_lifecycle_event_never_gets_action_token() {
        // Even with a secret present, lifecycle events don't carry a stop action.
        let event = lifecycle_event("active");
        let payload_str = build_payload(&event, "test-action-secret");
        let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
        assert!(payload.get("action").is_none());
    }

    // --- intervention payload tests ---

    #[test]
    fn test_intervention_title_body() {
        let event = intervention_event("budget_exceeded", "Cost $10.00 reached budget $10.00");
        let payload_str = build_payload(&event, "");
        let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
        assert_eq!(payload["title"], "Intervention: fix-auth");
        assert_eq!(
            payload["body"],
            "Session `fix-auth` stopped — Cost $10.00 reached budget $10.00"
        );
    }

    #[test]
    fn test_intervention_never_gets_action_token() {
        // The daemon already stopped the session by the time this fires — no
        // action token, even when a secret is configured.
        let event = intervention_event("burn_rate", "Burn rate exceeded ceiling");
        let payload_str = build_payload(&event, "test-action-secret");
        let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
        assert!(payload.get("action").is_none());
    }

    #[test]
    fn test_intervention_missing_reason_falls_back() {
        let mut event = intervention_event("user_stop", "irrelevant");
        event.payload = serde_json::json!({});
        let payload_str = build_payload(&event, "");
        let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap();
        assert_eq!(
            payload["body"],
            "Session `fix-auth` stopped — pulpo took action"
        );
    }

    // --- wants tests ---

    #[tokio::test]
    async fn test_wants_lifecycle_usage_alert_and_intervention() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let sink = WebPushSink::new(store, "priv".into(), "action-secret".into());
        assert_eq!(sink.name(), "web-push");
        assert!(sink.wants(&lifecycle_event("active")));

        let mut usage = lifecycle_event("budget_threshold");
        usage.event_type = "usage_alert".into();
        assert!(sink.wants(&usage));

        let mut intervention = lifecycle_event("budget_exceeded");
        intervention.event_type = "intervention".into();
        assert!(sink.wants(&intervention));

        let mut fleet = lifecycle_event("node_down");
        fleet.event_type = "fleet".into();
        assert!(!sink.wants(&fleet));
    }

    // --- deliver (coverage stub) ---

    #[tokio::test]
    async fn test_deliver_is_callable() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let sink = WebPushSink::new(store, "priv-key".into(), "action-secret".into());
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

        let sink = WebPushSink::new(store.clone(), vapid, "action-secret".into());
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
