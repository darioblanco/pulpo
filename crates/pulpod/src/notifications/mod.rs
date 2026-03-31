pub mod discord;
pub mod web_push;
pub mod webhook;

#[cfg(test)]
pub(crate) fn test_event(status: &str) -> pulpo_common::event::SessionEvent {
    pulpo_common::event::SessionEvent {
        session_id: "abc-123".into(),
        session_name: "my-session".into(),
        status: status.into(),
        node_name: "node-1".into(),
        timestamp: "2026-01-01T00:00:00Z".into(),
        ..Default::default()
    }
}
