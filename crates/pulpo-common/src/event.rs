use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::session::status_reason;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionEvent {
    pub session_id: String,
    pub session_name: String,
    pub status: String,
    pub previous_status: Option<String>,
    pub node_name: String,
    pub output_snippet: Option<String>,
    pub timestamp: String,
    /// Why the session is in `status` — see `pulpo_common::session::status_reason`.
    /// Only set for `waiting`/`done`, mirroring `Session::status_reason`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_reason: Option<String>,
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
    /// The needs-input sub-reason (e.g. "permission", "question"), when `status_reason`
    /// is `needs_input:<reason>` — kept for one release after ADR 0009's five-state
    /// model (which moved this from a `needs_input` metadata key to `status_reason`)
    /// so an older SSE consumer still sees it, populated from `status_reason` rather
    /// than written independently. Absent for a plain `waiting` (reason `idle`) session
    /// or any other status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub needs_input: Option<String>,
    /// Token and cost enrichment fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_cost_usd: Option<f64>,
    /// The agent's own process exit code, when known — set once a session
    /// resolves to `done` via an exit marker (`{id}.code`). `None` for every
    /// other status, and for a `done` session with no marker (e.g. an explicit
    /// `pulpo stop`/intervention, or a `lost` session with no clean end).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
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
/// Sources: budget threshold, quota approaching, rate limit.
/// Non-destructive — informs; any auto-action is recorded separately as an intervention.
/// `alert_kind` distinguishes the source so clients can filter/route.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageAlertEvent {
    pub session_id: String,
    pub session_name: String,
    pub node_name: String,
    /// `budget_threshold` | `quota_threshold` | `rate_limit`.
    pub alert_kind: String,
    /// Human-readable summary, e.g. "Cost $0.85 reached 80% of $1.00 budget".
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_usd: Option<f64>,
    pub timestamp: String,
}

/// A watchdog intervention: the daemon forcibly acted on a session.
///
/// Typically a forced stop. Unlike a [`UsageAlertEvent`] (which only informs), this records
/// a destructive action so receivers can alert on "Pulpo pulled the plug".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionInterventionEvent {
    pub session_id: String,
    pub session_name: String,
    pub node_name: String,
    /// `memory_pressure` | `idle_timeout` | `budget_exceeded` | `user_stop`
    /// (the `InterventionCode` `Display` form).
    pub code: String,
    pub reason: String,
    pub timestamp: String,
}

/// A daemon-level event not tied to any particular session — currently just
/// the database being found unusable at startup and quarantined (see
/// `pulpod::store::open_and_migrate`). Kept generic (a free-form `subtype` +
/// `message`) so future daemon-level notices don't each need a new
/// `PulpoEvent` variant.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DaemonEvent {
    pub node_name: String,
    /// `db_unusable` today; more subtypes may be added later.
    pub subtype: String,
    /// Human-readable summary, e.g. "database was unusable (...) — quarantined
    /// to state.db.unusable-<timestamp> and started fresh".
    pub message: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum PulpoEvent {
    Session(SessionEvent),
    SessionDeleted(SessionDeletedEvent),
    UsageAlert(UsageAlertEvent),
    Intervention(SessionInterventionEvent),
    Daemon(DaemonEvent),
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
    /// The agent's own process exit code, when known — see [`SessionEvent::exit_code`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

/// The canonical, forward-facing event envelope.
///
/// One shape for *every* externally-forwarded event — session lifecycle changes,
/// interventions, usage alerts, and fleet events — serialized to the locked
/// webhook message contract (see ROADMAP "Webhook message contract"). Sinks
/// (currently just webhooks) consume this rather than the internal [`PulpoEvent`].
///
/// `event_id` is a fresh UUID per event and doubles as the idempotency key for
/// at-least-once delivery (receivers dedupe on it).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    /// Envelope schema version. Currently always `1`.
    pub schema_version: u32,
    /// Unique id for this event; stable across delivery retries (idempotency key).
    pub event_id: String,
    /// `lifecycle` | `intervention` | `usage_alert` | `daemon` (`fleet` is
    /// reserved from an earlier multi-node design; nothing emits it today).
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

/// Severity for a lifecycle subtype (session status + its `status_reason`).
///
/// `done` no longer distinguishes "finished on its own" (the old `ready`) from
/// "forcibly/explicitly ended" (the old `stopped`) as separate top-level statuses —
/// that distinction now lives in `reason`, so severity has to look at it: a clean
/// `exited` end is informational, same as `ready` always was; anything else under
/// `done` (`stopped`, an intervention code, or an unrecognized/absent reason) keeps
/// `stopped`'s old `warn` severity.
fn lifecycle_severity(status: &str, reason: Option<&str>) -> &'static str {
    match status {
        "lost" => "critical",
        "waiting" => "warn",
        "done" if reason != Some(status_reason::EXITED) => "warn",
        _ => "info",
    }
}

/// Severity for an intervention subtype (`InterventionCode` `Display` form).
fn intervention_severity(code: &str) -> &'static str {
    match code {
        // Forced stops that cost money or lose work.
        "budget_exceeded" | "memory_pressure" => "critical",
        // Routine reclamation / user-initiated.
        _ => "warn",
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
                severity: lifecycle_severity(&se.status, se.status_reason.as_deref()).into(),
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
                    exit_code: se.exit_code,
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
            PulpoEvent::Intervention(iv) => {
                let mut payload = serde_json::Map::new();
                payload.insert("intervention_reason".into(), serde_json::json!(iv.reason));
                Some(Self {
                    schema_version: 1,
                    event_id: Uuid::new_v4().to_string(),
                    event_type: "intervention".into(),
                    subtype: iv.code.clone(),
                    severity: intervention_severity(&iv.code).into(),
                    occurred_at: rfc3339_or_now(&iv.timestamp),
                    node: node.to_string(),
                    session: Some(EventSessionRef {
                        id: iv.session_id.clone(),
                        name: iv.session_name.clone(),
                        status: "done".into(),
                        ..Default::default()
                    }),
                    payload: serde_json::Value::Object(payload),
                })
            }
            PulpoEvent::Daemon(d) => Some(Self {
                schema_version: 1,
                event_id: Uuid::new_v4().to_string(),
                event_type: "daemon".into(),
                subtype: d.subtype.clone(),
                severity: "critical".into(),
                occurred_at: rfc3339_or_now(&d.timestamp),
                node: node.to_string(),
                session: None,
                payload: serde_json::json!({ "message": d.message }),
            }),
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
            status: "working".into(),
            previous_status: Some("starting".into()),
            node_name: "node-1".into(),
            output_snippet: Some("Hello world".into()),
            timestamp: "2026-01-01T00:00:00Z".into(),
            ..Default::default()
        };
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: SessionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.session_id, "abc-123");
        assert_eq!(deserialized.session_name, "my-session");
        assert_eq!(deserialized.status, "working");
        assert_eq!(deserialized.previous_status, Some("starting".into()));
        assert_eq!(deserialized.node_name, "node-1");
        assert_eq!(deserialized.output_snippet, Some("Hello world".into()));
    }

    #[test]
    fn test_session_event_needs_input_roundtrip() {
        let event = SessionEvent {
            session_id: "id".into(),
            session_name: "name".into(),
            status: "waiting".into(),
            node_name: "n".into(),
            timestamp: "t".into(),
            needs_input: Some("permission".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"needs_input\":\"permission\""));
        let deserialized: SessionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.needs_input.as_deref(), Some("permission"));
    }

    #[test]
    fn test_session_event_needs_input_omitted_when_none() {
        let event = SessionEvent {
            session_id: "id".into(),
            session_name: "name".into(),
            status: "working".into(),
            node_name: "n".into(),
            timestamp: "t".into(),
            ..Default::default()
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("needs_input"));
    }

    #[test]
    fn test_session_event_without_optionals() {
        let event = SessionEvent {
            session_id: "id".into(),
            session_name: "name".into(),
            status: "done".into(),
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
            status: "working".into(),
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
            status: "working".into(),
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
        let json = r#"{"kind":"session","session_id":"s1","session_name":"test","status":"working","previous_status":null,"node_name":"n","output_snippet":null,"timestamp":"t"}"#;
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

    fn intervention(code: &str) -> PulpoEvent {
        PulpoEvent::Intervention(SessionInterventionEvent {
            session_id: "s1".into(),
            session_name: "fix-auth".into(),
            node_name: "ignored".into(),
            code: code.into(),
            reason: "Cost $10.00 reached budget $10.00".into(),
            timestamp: "2026-06-15T12:00:00Z".into(),
        })
    }

    #[test]
    fn test_from_pulpo_event_intervention_critical() {
        let ev = Event::from_pulpo_event(&intervention("budget_exceeded"), "mac-mini").unwrap();
        assert_eq!(ev.event_type, "intervention");
        assert_eq!(ev.subtype, "budget_exceeded");
        assert_eq!(ev.severity, "critical");
        assert_eq!(ev.node, "mac-mini");
        assert_eq!(ev.session.as_ref().unwrap().id, "s1");
        assert_eq!(ev.session.as_ref().unwrap().status, "done");
        assert_eq!(
            ev.payload.get("intervention_reason").unwrap(),
            "Cost $10.00 reached budget $10.00"
        );
    }

    #[test]
    fn test_from_pulpo_event_intervention_severity_split() {
        // budget_exceeded + memory_pressure are critical; idle_timeout/user_stop are warn.
        assert_eq!(
            Event::from_pulpo_event(&intervention("memory_pressure"), "n")
                .unwrap()
                .severity,
            "critical"
        );
        assert_eq!(
            Event::from_pulpo_event(&intervention("idle_timeout"), "n")
                .unwrap()
                .severity,
            "warn"
        );
        assert_eq!(
            Event::from_pulpo_event(&intervention("user_stop"), "n")
                .unwrap()
                .severity,
            "warn"
        );
    }

    #[test]
    fn test_pulpo_event_debug_clone() {
        let event = PulpoEvent::Session(SessionEvent {
            session_id: "id".into(),
            session_name: "name".into(),
            status: "working".into(),
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
            status: "done".into(),
            status_reason: Some("exited".into()),
            previous_status: Some("working".into()),
            node_name: "n".into(),
            output_snippet: Some("done".into()),
            timestamp: "2026-01-01T00:00:00Z".into(),
            ..Default::default()
        });
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: PulpoEvent = serde_json::from_str(&json).unwrap();
        assert!(
            matches!(&deserialized, PulpoEvent::Session(se) if se.session_id == "s1" && se.status == "done" && se.status_reason.as_deref() == Some("exited"))
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
        assert_eq!(lifecycle_severity("lost", None), "critical");
        assert_eq!(lifecycle_severity("waiting", Some("idle")), "warn");
        assert_eq!(
            lifecycle_severity("waiting", Some("needs_input:permission")),
            "warn"
        );
        assert_eq!(lifecycle_severity("working", None), "info");
        assert_eq!(lifecycle_severity("starting", None), "info");
        // `done` with a clean-exit reason is informational, same as the old `ready`.
        assert_eq!(lifecycle_severity("done", Some("exited")), "info");
        // Any other `done` reason (explicit stop, an intervention code, or an
        // absent/unrecognized reason) keeps the old `stopped` severity.
        assert_eq!(lifecycle_severity("done", Some("stopped")), "warn");
        assert_eq!(lifecycle_severity("done", Some("idle_timeout")), "warn");
        assert_eq!(lifecycle_severity("done", Some("budget_exceeded")), "warn");
        assert_eq!(lifecycle_severity("done", None), "warn");
        assert_eq!(lifecycle_severity("error", None), "info");
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
    fn test_from_pulpo_event_session_working() {
        let ev = PulpoEvent::Session(sample_session_event("working"));
        let event = Event::from_pulpo_event(&ev, "mac-mini").unwrap();
        assert_eq!(event.schema_version, 1);
        assert!(!event.event_id.is_empty());
        assert_eq!(event.event_type, "lifecycle");
        assert_eq!(event.subtype, "working");
        assert_eq!(event.severity, "info");
        assert_eq!(event.occurred_at, "2026-06-13T12:00:00Z");
        assert_eq!(event.node, "mac-mini");
        let session = event.session.unwrap();
        assert_eq!(session.id, "sess-1");
        assert_eq!(session.name, "fix-auth");
        assert_eq!(session.status, "working");
        assert_eq!(event.payload, serde_json::json!({}));
    }

    #[test]
    fn test_from_pulpo_event_session_severities() {
        for (status, reason, expected) in [
            ("lost", None, "critical"),
            ("waiting", Some("idle"), "warn"),
            ("waiting", Some("needs_input:permission"), "warn"),
            ("done", Some("exited"), "info"),
            ("done", Some("stopped"), "warn"),
            ("working", None, "info"),
            ("starting", None, "info"),
        ] {
            let mut se = sample_session_event(status);
            se.status_reason = reason.map(str::to_owned);
            let ev = PulpoEvent::Session(se);
            let event = Event::from_pulpo_event(&ev, "n").unwrap();
            assert_eq!(
                event.severity, expected,
                "status {status} reason {reason:?}"
            );
            assert_eq!(event.subtype, status);
        }
    }

    #[test]
    fn test_from_pulpo_event_session_enrichment() {
        let mut se = sample_session_event("done");
        se.status_reason = Some("exited".into());
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
        let mut se = sample_session_event("done");
        se.total_output_tokens = Some(500);
        let ev = PulpoEvent::Session(se);
        let event = Event::from_pulpo_event(&ev, "n").unwrap();
        assert_eq!(event.session.unwrap().total_tokens, Some(500));
    }

    #[test]
    fn test_from_pulpo_event_session_no_tokens() {
        let ev = PulpoEvent::Session(sample_session_event("working"));
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
            subtype: "waiting".into(),
            severity: "warn".into(),
            occurred_at: "2026-06-13T12:00:00Z".into(),
            node: "mac-mini".into(),
            session: Some(EventSessionRef {
                id: "sid".into(),
                name: "fix-auth".into(),
                status: "waiting".into(),
                ..Default::default()
            }),
            payload: serde_json::json!({}),
        };
        let json = serde_json::to_value(&event).unwrap();
        // `type` is the wire key, not `event_type`.
        assert_eq!(json["type"], "lifecycle");
        assert!(json.get("event_type").is_none());
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["subtype"], "waiting");
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
            subtype: "budget_threshold".into(),
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

    fn daemon_event() -> PulpoEvent {
        PulpoEvent::Daemon(DaemonEvent {
            node_name: "mac-mini".into(),
            subtype: "db_unusable".into(),
            message: "database was unusable — quarantined and started fresh".into(),
            timestamp: "2026-06-15T12:00:00Z".into(),
        })
    }

    #[test]
    fn test_daemon_event_serialize_roundtrip() {
        let event = DaemonEvent {
            node_name: "n".into(),
            subtype: "db_unusable".into(),
            message: "m".into(),
            timestamp: "t".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: DaemonEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.node_name, "n");
        assert_eq!(deserialized.subtype, "db_unusable");
        assert_eq!(deserialized.message, "m");
    }

    #[test]
    fn test_daemon_event_debug_clone_default() {
        let event = DaemonEvent::default();
        let cloned = event.clone();
        assert_eq!(format!("{event:?}"), format!("{cloned:?}"));
    }

    #[test]
    fn test_pulpo_event_daemon_serialize() {
        let json = serde_json::to_string(&daemon_event()).unwrap();
        assert!(json.contains("\"kind\":\"daemon\""));
        assert!(json.contains("\"subtype\":\"db_unusable\""));
    }

    #[test]
    fn test_pulpo_event_roundtrip_daemon() {
        let json = serde_json::to_string(&daemon_event()).unwrap();
        let deserialized: PulpoEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            &deserialized,
            PulpoEvent::Daemon(d) if d.subtype == "db_unusable" && d.node_name == "mac-mini"
        ));
    }

    #[test]
    fn test_from_pulpo_event_daemon() {
        let event = Event::from_pulpo_event(&daemon_event(), "mac-mini").unwrap();
        assert_eq!(event.event_type, "daemon");
        assert_eq!(event.subtype, "db_unusable");
        assert_eq!(event.severity, "critical");
        assert_eq!(event.node, "mac-mini");
        assert!(event.session.is_none());
        assert_eq!(
            event.payload["message"],
            "database was unusable — quarantined and started fresh"
        );
    }

    #[test]
    fn test_event_session_ref_clone_debug_default() {
        let r = EventSessionRef::default();
        let cloned = r.clone();
        assert_eq!(r, cloned);
        assert_eq!(format!("{r:?}"), format!("{cloned:?}"));
    }
}
