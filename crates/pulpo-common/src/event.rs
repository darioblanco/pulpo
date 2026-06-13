use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionEvent {
    pub session_id: String,
    pub session_name: String,
    pub status: String,
    pub previous_status: Option<String>,
    pub node_name: String,
    pub output_snippet: Option<String>,
    pub timestamp: String,
    /// Enrichment fields for notifications (populated from session metadata/fields).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_insertions: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_deletions: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_files_changed: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_status: Option<String>,
    /// Token and cost enrichment fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionDeletedEvent {
    pub session_id: String,
    pub session_name: String,
    pub node_name: String,
    pub timestamp: String,
}

/// A usage/cost monitoring alert.
///
/// Sources: budget threshold, burn-rate ceiling, quota approaching, rate limit.
/// Non-destructive — informs; any auto-action is recorded separately as an intervention.
/// `alert_kind` distinguishes the source so clients can filter/route.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageAlertEvent {
    pub session_id: String,
    pub session_name: String,
    pub node_name: String,
    /// `budget_threshold` | `burn_ceiling` | `quota_threshold` | `rate_limit`.
    pub alert_kind: String,
    /// Human-readable summary, e.g. "Cost $0.85 reached 80% of $1.00 budget".
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_usd: Option<f64>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum PulpoEvent {
    Session(SessionEvent),
    SessionDeleted(SessionDeletedEvent),
    UsageAlert(UsageAlertEvent),
}

/// Session reference embedded in the canonical [`Event`] envelope.
///
/// Carries just enough session context for a receiver to route, display, and
/// correlate an event without a follow-up API call. Optional fields are omitted
/// from the wire payload when absent.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct EventSessionRef {
    pub id: String,
    pub name: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ink: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool: Option<String>,
}

/// The canonical, forward-facing event envelope.
///
/// One shape for *every* externally-forwarded event — session lifecycle changes,
/// interventions, usage alerts, and fleet events — serialized to the locked
/// webhook message contract (see ROADMAP "Webhook message contract"). Sinks
/// (webhooks, web-push, …) consume this rather than the internal [`PulpoEvent`].
///
/// `event_id` is a fresh UUID per event and doubles as the idempotency key for
/// at-least-once delivery (receivers dedupe on it).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    /// Envelope schema version. Currently always `1`.
    pub schema_version: u32,
    /// Unique id for this event; stable across delivery retries (idempotency key).
    pub event_id: String,
    /// `lifecycle` | `intervention` | `usage_alert` | `fleet`.
    #[serde(rename = "type")]
    pub event_type: String,
    /// The specific event within the type, e.g. `idle`, `budget_threshold`.
    pub subtype: String,
    /// `info` | `warn` | `critical` — the universal filter knob.
    pub severity: String,
    /// RFC 3339 timestamp of when the event occurred.
    pub occurred_at: String,
    /// Name of the node that emitted the event.
    pub node: String,
    /// Session context, present for session-scoped events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<EventSessionRef>,
    /// Type-specific extras (e.g. `budget_usd`, `intervention_reason`).
    #[serde(default)]
    pub payload: serde_json::Value,
}

/// Severity for a lifecycle subtype (session status).
fn lifecycle_severity(status: &str) -> &'static str {
    match status {
        "lost" => "critical",
        "stopped" | "idle" => "warn",
        _ => "info",
    }
}

/// Sum two optional token counts, yielding `None` only when both are absent.
fn sum_tokens(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (None, None) => None,
        (x, y) => Some(x.unwrap_or(0) + y.unwrap_or(0)),
    }
}

impl Event {
    /// Build a canonical [`Event`] from an internal [`PulpoEvent`].
    ///
    /// Returns `None` for events that are not externally forwarded (currently
    /// [`PulpoEvent::SessionDeleted`], which is housekeeping). A fresh `event_id`
    /// is generated for each event.
    #[must_use]
    pub fn from_pulpo_event(ev: &PulpoEvent, node: &str) -> Option<Self> {
        match ev {
            PulpoEvent::Session(se) => Some(Self {
                schema_version: 1,
                event_id: Uuid::new_v4().to_string(),
                event_type: "lifecycle".into(),
                subtype: se.status.clone(),
                severity: lifecycle_severity(&se.status).into(),
                occurred_at: rfc3339_or_now(&se.timestamp),
                node: node.to_string(),
                session: Some(EventSessionRef {
                    id: se.session_id.clone(),
                    name: se.session_name.clone(),
                    status: se.status.clone(),
                    ink: None,
                    git_branch: se.git_branch.clone(),
                    pr_url: se.pr_url.clone(),
                    cost_usd: se.session_cost_usd,
                    total_tokens: sum_tokens(se.total_input_tokens, se.total_output_tokens),
                    pool: None,
                }),
                payload: serde_json::json!({}),
            }),
            PulpoEvent::UsageAlert(a) => {
                let mut payload = serde_json::Map::new();
                if let Some(cost) = a.cost_usd {
                    payload.insert("cost_usd".into(), serde_json::json!(cost));
                }
                if let Some(budget) = a.budget_usd {
                    payload.insert("budget_usd".into(), serde_json::json!(budget));
                }
                Some(Self {
                    schema_version: 1,
                    event_id: Uuid::new_v4().to_string(),
                    event_type: "usage_alert".into(),
                    subtype: a.alert_kind.clone(),
                    severity: "warn".into(),
                    occurred_at: rfc3339_or_now(&a.timestamp),
                    node: node.to_string(),
                    session: Some(EventSessionRef {
                        id: a.session_id.clone(),
                        name: a.session_name.clone(),
                        status: String::new(),
                        ..Default::default()
                    }),
                    payload: serde_json::Value::Object(payload),
                })
            }
            // Housekeeping — not an externally-forwarded "important event".
            PulpoEvent::SessionDeleted(_) => None,
        }
    }
}

/// Return the timestamp as-is if non-empty, otherwise the current time.
///
/// Internal events always carry an RFC 3339 timestamp; this guards against an
/// empty string slipping through so the envelope's `occurred_at` is never blank.
fn rfc3339_or_now(timestamp: &str) -> String {
    if timestamp.is_empty() {
        Utc::now().to_rfc3339()
    } else {
        timestamp.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_event_serialize_roundtrip() {
        let event = SessionEvent {
            session_id: "abc-123".into(),
            session_name: "my-session".into(),
            status: "active".into(),
            previous_status: Some("creating".into()),
            node_name: "node-1".into(),
            output_snippet: Some("Hello world".into()),
            timestamp: "2026-01-01T00:00:00Z".into(),
            ..Default::default()
        };
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: SessionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.session_id, "abc-123");
        assert_eq!(deserialized.session_name, "my-session");
        assert_eq!(deserialized.status, "active");
        assert_eq!(deserialized.previous_status, Some("creating".into()));
        assert_eq!(deserialized.node_name, "node-1");
        assert_eq!(deserialized.output_snippet, Some("Hello world".into()));
    }

    #[test]
    fn test_session_event_without_optionals() {
        let event = SessionEvent {
            session_id: "id".into(),
            session_name: "name".into(),
            status: "stopped".into(),
            node_name: "n".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
            ..Default::default()
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"previous_status\":null"));
        assert!(json.contains("\"output_snippet\":null"));
    }

    #[test]
    fn test_session_event_debug_clone() {
        let event = SessionEvent {
            session_id: "id".into(),
            session_name: "name".into(),
            status: "active".into(),
            node_name: "n".into(),
            timestamp: "t".into(),
            ..Default::default()
        };
        let cloned = event.clone();
        assert_eq!(format!("{event:?}"), format!("{cloned:?}"));
    }

    #[test]
    fn test_pulpo_event_session_serialize() {
        let event = PulpoEvent::Session(SessionEvent {
            session_id: "s1".into(),
            session_name: "test".into(),
            status: "active".into(),
            node_name: "n".into(),
            timestamp: "t".into(),
            ..Default::default()
        });
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"kind\":\"session\""));
        assert!(json.contains("\"session_id\":\"s1\""));
    }

    #[test]
    fn test_pulpo_event_deserialize_session() {
        let json = r#"{"kind":"session","session_id":"s1","session_name":"test","status":"active","previous_status":null,"node_name":"n","output_snippet":null,"timestamp":"t"}"#;
        let event: PulpoEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(&event, PulpoEvent::Session(se) if se.session_id == "s1"));
    }

    #[test]
    fn test_session_deleted_event_serialize_roundtrip() {
        let event = SessionDeletedEvent {
            session_id: "abc-123".into(),
            session_name: "my-session".into(),
            node_name: "node-1".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: SessionDeletedEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.session_id, "abc-123");
        assert_eq!(deserialized.session_name, "my-session");
        assert_eq!(deserialized.node_name, "node-1");
    }

    #[test]
    fn test_pulpo_event_serialize_session_deleted() {
        let event = PulpoEvent::SessionDeleted(SessionDeletedEvent {
            session_id: "s1".into(),
            session_name: "test".into(),
            node_name: "n".into(),
            timestamp: "t".into(),
        });
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"kind\":\"session_deleted\""));
        assert!(json.contains("\"session_id\":\"s1\""));
    }

    #[test]
    fn test_pulpo_event_invalid_kind() {
        let json = r#"{"kind":"unknown","data":"test"}"#;
        let result = serde_json::from_str::<PulpoEvent>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_pulpo_event_debug_clone() {
        let event = PulpoEvent::Session(SessionEvent {
            session_id: "id".into(),
            session_name: "name".into(),
            status: "active".into(),
            node_name: "n".into(),
            timestamp: "t".into(),
            ..Default::default()
        });
        let cloned = event.clone();
        assert_eq!(format!("{event:?}"), format!("{cloned:?}"));
    }

    #[test]
    fn test_pulpo_event_roundtrip_session() {
        let original = PulpoEvent::Session(SessionEvent {
            session_id: "s1".into(),
            session_name: "test".into(),
            status: "ready".into(),
            previous_status: Some("active".into()),
            node_name: "n".into(),
            output_snippet: Some("done".into()),
            timestamp: "2026-01-01T00:00:00Z".into(),
            ..Default::default()
        });
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: PulpoEvent = serde_json::from_str(&json).unwrap();
        assert!(
            matches!(&deserialized, PulpoEvent::Session(se) if se.session_id == "s1" && se.status == "ready")
        );
    }

    #[test]
    fn test_pulpo_event_roundtrip_session_deleted() {
        let original = PulpoEvent::SessionDeleted(SessionDeletedEvent {
            session_id: "s1".into(),
            session_name: "test".into(),
            node_name: "n".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
        });
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: PulpoEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            &deserialized,
            PulpoEvent::SessionDeleted(se) if se.session_id == "s1" && se.session_name == "test"
        ));
    }

    // --- Canonical Event envelope tests ---

    fn sample_session_event(status: &str) -> SessionEvent {
        SessionEvent {
            session_id: "sess-1".into(),
            session_name: "fix-auth".into(),
            status: status.into(),
            node_name: "ignored".into(),
            timestamp: "2026-06-13T12:00:00Z".into(),
            ..Default::default()
        }
    }

    #[test]
    fn test_lifecycle_severity_mapping() {
        assert_eq!(lifecycle_severity("lost"), "critical");
        assert_eq!(lifecycle_severity("stopped"), "warn");
        assert_eq!(lifecycle_severity("idle"), "warn");
        assert_eq!(lifecycle_severity("active"), "info");
        assert_eq!(lifecycle_severity("ready"), "info");
        assert_eq!(lifecycle_severity("creating"), "info");
        assert_eq!(lifecycle_severity("error"), "info");
    }

    #[test]
    fn test_sum_tokens() {
        assert_eq!(sum_tokens(None, None), None);
        assert_eq!(sum_tokens(Some(10), None), Some(10));
        assert_eq!(sum_tokens(None, Some(5)), Some(5));
        assert_eq!(sum_tokens(Some(10), Some(5)), Some(15));
        assert_eq!(sum_tokens(Some(0), Some(0)), Some(0));
    }

    #[test]
    fn test_rfc3339_or_now() {
        assert_eq!(
            rfc3339_or_now("2026-06-13T12:00:00Z"),
            "2026-06-13T12:00:00Z"
        );
        // Empty falls back to a non-empty current timestamp.
        assert!(!rfc3339_or_now("").is_empty());
    }

    #[test]
    fn test_from_pulpo_event_session_active() {
        let ev = PulpoEvent::Session(sample_session_event("active"));
        let event = Event::from_pulpo_event(&ev, "mac-mini").unwrap();
        assert_eq!(event.schema_version, 1);
        assert!(!event.event_id.is_empty());
        assert_eq!(event.event_type, "lifecycle");
        assert_eq!(event.subtype, "active");
        assert_eq!(event.severity, "info");
        assert_eq!(event.occurred_at, "2026-06-13T12:00:00Z");
        assert_eq!(event.node, "mac-mini");
        let session = event.session.unwrap();
        assert_eq!(session.id, "sess-1");
        assert_eq!(session.name, "fix-auth");
        assert_eq!(session.status, "active");
        assert_eq!(event.payload, serde_json::json!({}));
    }

    #[test]
    fn test_from_pulpo_event_session_severities() {
        for (status, expected) in [
            ("lost", "critical"),
            ("stopped", "warn"),
            ("idle", "warn"),
            ("ready", "info"),
            ("active", "info"),
        ] {
            let ev = PulpoEvent::Session(sample_session_event(status));
            let event = Event::from_pulpo_event(&ev, "n").unwrap();
            assert_eq!(event.severity, expected, "status {status}");
            assert_eq!(event.subtype, status);
        }
    }

    #[test]
    fn test_from_pulpo_event_session_enrichment() {
        let mut se = sample_session_event("ready");
        se.git_branch = Some("feat/x".into());
        se.pr_url = Some("https://github.com/org/repo/pull/9".into());
        se.session_cost_usd = Some(2.5);
        se.total_input_tokens = Some(1_000_000);
        se.total_output_tokens = Some(234_000);
        let ev = PulpoEvent::Session(se);
        let event = Event::from_pulpo_event(&ev, "n").unwrap();
        let session = event.session.unwrap();
        assert_eq!(session.git_branch.as_deref(), Some("feat/x"));
        assert_eq!(
            session.pr_url.as_deref(),
            Some("https://github.com/org/repo/pull/9")
        );
        assert_eq!(session.cost_usd, Some(2.5));
        assert_eq!(session.total_tokens, Some(1_234_000));
    }

    #[test]
    fn test_from_pulpo_event_session_tokens_partial() {
        let mut se = sample_session_event("ready");
        se.total_output_tokens = Some(500);
        let ev = PulpoEvent::Session(se);
        let event = Event::from_pulpo_event(&ev, "n").unwrap();
        assert_eq!(event.session.unwrap().total_tokens, Some(500));
    }

    #[test]
    fn test_from_pulpo_event_session_no_tokens() {
        let ev = PulpoEvent::Session(sample_session_event("active"));
        let event = Event::from_pulpo_event(&ev, "n").unwrap();
        assert_eq!(event.session.unwrap().total_tokens, None);
    }

    #[test]
    fn test_from_pulpo_event_usage_alert() {
        let ev = PulpoEvent::UsageAlert(UsageAlertEvent {
            session_id: "sess-2".into(),
            session_name: "burner".into(),
            node_name: "ignored".into(),
            alert_kind: "budget_threshold".into(),
            message: "Cost $0.85 reached 80% of $1.00 budget".into(),
            cost_usd: Some(0.85),
            budget_usd: Some(1.0),
            timestamp: "2026-06-13T12:00:00Z".into(),
        });
        let event = Event::from_pulpo_event(&ev, "mac-mini").unwrap();
        assert_eq!(event.event_type, "usage_alert");
        assert_eq!(event.subtype, "budget_threshold");
        assert_eq!(event.severity, "warn");
        assert_eq!(event.node, "mac-mini");
        let session = event.session.unwrap();
        assert_eq!(session.id, "sess-2");
        assert_eq!(session.name, "burner");
        assert_eq!(event.payload["cost_usd"], 0.85);
        assert_eq!(event.payload["budget_usd"], 1.0);
    }

    #[test]
    fn test_from_pulpo_event_usage_alert_omits_null_payload() {
        let ev = PulpoEvent::UsageAlert(UsageAlertEvent {
            session_id: "s".into(),
            session_name: "n".into(),
            node_name: "x".into(),
            alert_kind: "rate_limit".into(),
            message: "rate limited".into(),
            cost_usd: None,
            budget_usd: None,
            timestamp: "2026-06-13T12:00:00Z".into(),
        });
        let event = Event::from_pulpo_event(&ev, "n").unwrap();
        assert_eq!(event.payload, serde_json::json!({}));
        assert!(event.payload.get("cost_usd").is_none());
        assert!(event.payload.get("budget_usd").is_none());
    }

    #[test]
    fn test_from_pulpo_event_session_deleted_returns_none() {
        let ev = PulpoEvent::SessionDeleted(SessionDeletedEvent {
            session_id: "s".into(),
            session_name: "n".into(),
            node_name: "x".into(),
            timestamp: "t".into(),
        });
        assert!(Event::from_pulpo_event(&ev, "n").is_none());
    }

    #[test]
    fn test_event_serializes_to_contract() {
        let event = Event {
            schema_version: 1,
            event_id: "abc".into(),
            event_type: "lifecycle".into(),
            subtype: "idle".into(),
            severity: "warn".into(),
            occurred_at: "2026-06-13T12:00:00Z".into(),
            node: "mac-mini".into(),
            session: Some(EventSessionRef {
                id: "sid".into(),
                name: "fix-auth".into(),
                status: "idle".into(),
                ..Default::default()
            }),
            payload: serde_json::json!({}),
        };
        let json = serde_json::to_value(&event).unwrap();
        // `type` is the wire key, not `event_type`.
        assert_eq!(json["type"], "lifecycle");
        assert!(json.get("event_type").is_none());
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["subtype"], "idle");
        assert_eq!(json["severity"], "warn");
        assert_eq!(json["session"]["name"], "fix-auth");
        // Optional session fields omitted when None.
        assert!(json["session"].get("pr_url").is_none());
        assert!(json["session"].get("cost_usd").is_none());
    }

    #[test]
    fn test_event_session_omitted_when_none() {
        let event = Event {
            schema_version: 1,
            event_id: "abc".into(),
            event_type: "fleet".into(),
            subtype: "node_down".into(),
            severity: "critical".into(),
            occurred_at: "t".into(),
            node: "n".into(),
            session: None,
            payload: serde_json::json!({}),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert!(json.get("session").is_none());
    }

    #[test]
    fn test_event_roundtrip_and_clone_debug() {
        let event = Event {
            schema_version: 1,
            event_id: "id".into(),
            event_type: "usage_alert".into(),
            subtype: "burn_ceiling".into(),
            severity: "warn".into(),
            occurred_at: "t".into(),
            node: "n".into(),
            session: None,
            payload: serde_json::json!({"cost_usd": 1.0}),
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
        let cloned = event.clone();
        assert_eq!(format!("{event:?}"), format!("{cloned:?}"));
    }

    #[test]
    fn test_event_payload_defaults_when_absent() {
        let json = r#"{"schema_version":1,"event_id":"x","type":"fleet","subtype":"node_up","severity":"info","occurred_at":"t","node":"n"}"#;
        let event: Event = serde_json::from_str(json).unwrap();
        assert_eq!(event.payload, serde_json::Value::Null);
        assert!(event.session.is_none());
    }

    #[test]
    fn test_event_session_ref_clone_debug_default() {
        let r = EventSessionRef::default();
        let cloned = r.clone();
        assert_eq!(r, cloned);
        assert_eq!(format!("{r:?}"), format!("{cloned:?}"));
    }
}
