use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEvent {
    pub session_id: String,
    pub session_name: String,
    pub status: String,
    pub previous_status: Option<String>,
    pub node_name: String,
    pub output_snippet: Option<String>,
    pub waiting_for_input: Option<bool>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleEvent {
    pub schedule_id: String,
    pub schedule_name: String,
    pub event_type: String,
    pub session_id: Option<String>,
    pub error: Option<String>,
    pub node_name: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PulpoEvent {
    Session(SessionEvent),
    Schedule(ScheduleEvent),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_event_serialize_roundtrip() {
        let event = SessionEvent {
            session_id: "abc-123".into(),
            session_name: "my-session".into(),
            status: "running".into(),
            previous_status: Some("creating".into()),
            node_name: "node-1".into(),
            output_snippet: Some("Hello world".into()),
            waiting_for_input: None,
            timestamp: "2026-01-01T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: SessionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.session_id, "abc-123");
        assert_eq!(deserialized.session_name, "my-session");
        assert_eq!(deserialized.status, "running");
        assert_eq!(deserialized.previous_status, Some("creating".into()));
        assert_eq!(deserialized.node_name, "node-1");
        assert_eq!(deserialized.output_snippet, Some("Hello world".into()));
    }

    #[test]
    fn test_session_event_without_optionals() {
        let event = SessionEvent {
            session_id: "id".into(),
            session_name: "name".into(),
            status: "dead".into(),
            previous_status: None,
            node_name: "n".into(),
            output_snippet: None,
            waiting_for_input: None,
            timestamp: "2026-01-01T00:00:00Z".into(),
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
            status: "running".into(),
            previous_status: None,
            node_name: "n".into(),
            output_snippet: None,
            waiting_for_input: None,
            timestamp: "t".into(),
        };
        let cloned = event.clone();
        assert_eq!(format!("{event:?}"), format!("{cloned:?}"));
    }

    // --- ScheduleEvent tests ---

    #[test]
    fn test_schedule_event_serialize_roundtrip() {
        let event = ScheduleEvent {
            schedule_id: "sched-1".into(),
            schedule_name: "nightly-review".into(),
            event_type: "fired".into(),
            session_id: Some("sess-1".into()),
            error: None,
            node_name: "node-1".into(),
            timestamp: "2026-01-01T02:00:00Z".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: ScheduleEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.schedule_id, "sched-1");
        assert_eq!(deserialized.schedule_name, "nightly-review");
        assert_eq!(deserialized.event_type, "fired");
        assert_eq!(deserialized.session_id, Some("sess-1".into()));
        assert!(deserialized.error.is_none());
    }

    #[test]
    fn test_schedule_event_without_optionals() {
        let event = ScheduleEvent {
            schedule_id: "id".into(),
            schedule_name: "name".into(),
            event_type: "skipped".into(),
            session_id: None,
            error: None,
            node_name: "n".into(),
            timestamp: "t".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"session_id\":null"));
        assert!(json.contains("\"error\":null"));
    }

    #[test]
    fn test_schedule_event_with_error() {
        let event = ScheduleEvent {
            schedule_id: "id".into(),
            schedule_name: "name".into(),
            event_type: "failed".into(),
            session_id: None,
            error: Some("spawn failed".into()),
            node_name: "n".into(),
            timestamp: "t".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("spawn failed"));
    }

    #[test]
    fn test_schedule_event_debug_clone() {
        let event = ScheduleEvent {
            schedule_id: "id".into(),
            schedule_name: "name".into(),
            event_type: "fired".into(),
            session_id: None,
            error: None,
            node_name: "n".into(),
            timestamp: "t".into(),
        };
        let cloned = event.clone();
        assert_eq!(format!("{event:?}"), format!("{cloned:?}"));
    }

    // --- PulpoEvent tests ---

    #[test]
    fn test_pulpo_event_session_serialize() {
        let event = PulpoEvent::Session(SessionEvent {
            session_id: "s1".into(),
            session_name: "test".into(),
            status: "running".into(),
            previous_status: None,
            node_name: "n".into(),
            output_snippet: None,
            waiting_for_input: None,
            timestamp: "t".into(),
        });
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"kind\":\"session\""));
        assert!(json.contains("\"session_id\":\"s1\""));
    }

    #[test]
    fn test_pulpo_event_schedule_serialize() {
        let event = PulpoEvent::Schedule(ScheduleEvent {
            schedule_id: "sch1".into(),
            schedule_name: "nightly".into(),
            event_type: "fired".into(),
            session_id: None,
            error: None,
            node_name: "n".into(),
            timestamp: "t".into(),
        });
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"kind\":\"schedule\""));
        assert!(json.contains("\"schedule_id\":\"sch1\""));
    }

    #[test]
    fn test_pulpo_event_deserialize_session() {
        let json = r#"{"kind":"session","session_id":"s1","session_name":"test","status":"running","previous_status":null,"node_name":"n","output_snippet":null,"waiting_for_input":null,"timestamp":"t"}"#;
        let event: PulpoEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(&event, PulpoEvent::Session(se) if se.session_id == "s1"));
    }

    #[test]
    fn test_pulpo_event_deserialize_schedule() {
        let json = r#"{"kind":"schedule","schedule_id":"sch1","schedule_name":"nightly","event_type":"fired","session_id":null,"error":null,"node_name":"n","timestamp":"t"}"#;
        let event: PulpoEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(&event, PulpoEvent::Schedule(se) if se.schedule_id == "sch1"));
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
            status: "running".into(),
            previous_status: None,
            node_name: "n".into(),
            output_snippet: None,
            waiting_for_input: None,
            timestamp: "t".into(),
        });
        let cloned = event.clone();
        assert_eq!(format!("{event:?}"), format!("{cloned:?}"));
    }

    #[test]
    fn test_pulpo_event_roundtrip_session() {
        let original = PulpoEvent::Session(SessionEvent {
            session_id: "s1".into(),
            session_name: "test".into(),
            status: "completed".into(),
            previous_status: Some("running".into()),
            node_name: "n".into(),
            output_snippet: Some("done".into()),
            waiting_for_input: None,
            timestamp: "2026-01-01T00:00:00Z".into(),
        });
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: PulpoEvent = serde_json::from_str(&json).unwrap();
        assert!(
            matches!(&deserialized, PulpoEvent::Session(se) if se.session_id == "s1" && se.status == "completed")
        );
    }

    #[test]
    fn test_pulpo_event_roundtrip_schedule() {
        let original = PulpoEvent::Schedule(ScheduleEvent {
            schedule_id: "sch1".into(),
            schedule_name: "weekly".into(),
            event_type: "exhausted".into(),
            session_id: None,
            error: None,
            node_name: "n".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
        });
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: PulpoEvent = serde_json::from_str(&json).unwrap();
        assert!(
            matches!(&deserialized, PulpoEvent::Schedule(se) if se.schedule_name == "weekly" && se.event_type == "exhausted")
        );
    }
}
