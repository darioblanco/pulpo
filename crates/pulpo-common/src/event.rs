use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEvent {
    pub session_id: String,
    pub session_name: String,
    pub status: String,
    pub previous_status: Option<String>,
    pub node_name: String,
    pub output_snippet: Option<String>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CultureEvent {
    pub action: String,
    pub count: usize,
    pub node_name: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PulpoEvent {
    Session(SessionEvent),
    Culture(CultureEvent),
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
            status: "killed".into(),
            previous_status: None,
            node_name: "n".into(),
            output_snippet: None,
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
            status: "active".into(),
            previous_status: None,
            node_name: "n".into(),
            output_snippet: None,
            timestamp: "t".into(),
        };
        let cloned = event.clone();
        assert_eq!(format!("{event:?}"), format!("{cloned:?}"));
    }

    // --- CultureEvent tests ---

    #[test]
    fn test_culture_event_serialize_roundtrip() {
        let event = CultureEvent {
            action: "synced".into(),
            count: 3,
            node_name: "node-1".into(),
            timestamp: "2026-03-12T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: CultureEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.action, "synced");
        assert_eq!(deserialized.count, 3);
        assert_eq!(deserialized.node_name, "node-1");
        assert_eq!(deserialized.timestamp, "2026-03-12T00:00:00Z");
    }

    #[test]
    fn test_culture_event_debug_clone() {
        let event = CultureEvent {
            action: "saved".into(),
            count: 1,
            node_name: "n".into(),
            timestamp: "t".into(),
        };
        let cloned = event.clone();
        assert_eq!(format!("{event:?}"), format!("{cloned:?}"));
    }

    // --- PulpoEvent tests ---

    #[test]
    fn test_pulpo_event_culture_serialize() {
        let event = PulpoEvent::Culture(CultureEvent {
            action: "synced".into(),
            count: 2,
            node_name: "node-1".into(),
            timestamp: "2026-03-12T00:00:00Z".into(),
        });
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"kind\":\"culture\""));
        assert!(json.contains("\"action\":\"synced\""));
    }

    #[test]
    fn test_pulpo_event_deserialize_culture() {
        let json =
            r#"{"kind":"culture","action":"synced","count":2,"node_name":"n","timestamp":"t"}"#;
        let event: PulpoEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(&event, PulpoEvent::Culture(ce) if ce.action == "synced"));
    }

    #[test]
    fn test_pulpo_event_culture_debug_clone() {
        let event = PulpoEvent::Culture(CultureEvent {
            action: "harvested".into(),
            count: 1,
            node_name: "n".into(),
            timestamp: "t".into(),
        });
        let cloned = event.clone();
        assert_eq!(format!("{event:?}"), format!("{cloned:?}"));
    }

    #[test]
    fn test_pulpo_event_session_serialize() {
        let event = PulpoEvent::Session(SessionEvent {
            session_id: "s1".into(),
            session_name: "test".into(),
            status: "active".into(),
            previous_status: None,
            node_name: "n".into(),
            output_snippet: None,
            timestamp: "t".into(),
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
            previous_status: None,
            node_name: "n".into(),
            output_snippet: None,
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
            status: "finished".into(),
            previous_status: Some("active".into()),
            node_name: "n".into(),
            output_snippet: Some("done".into()),
            timestamp: "2026-01-01T00:00:00Z".into(),
        });
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: PulpoEvent = serde_json::from_str(&json).unwrap();
        assert!(
            matches!(&deserialized, PulpoEvent::Session(se) if se.session_id == "s1" && se.status == "finished")
        );
    }
}
