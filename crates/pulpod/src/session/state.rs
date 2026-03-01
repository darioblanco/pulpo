use pulpo_common::session::SessionStatus;

/// Check whether a transition from one session status to another is valid.
///
/// Valid transitions:
/// - Creating → Running (session started successfully)
/// - Creating → Dead (session failed to start)
/// - Running → Completed (session finished normally)
/// - Running → Dead (session killed or crashed)
/// - Running → Stale (session became unresponsive)
/// - Stale → Running (session resumed)
/// - Stale → Dead (session killed while stale)
pub const fn is_valid_transition(from: SessionStatus, to: SessionStatus) -> bool {
    matches!(
        (from, to),
        (
            SessionStatus::Creating | SessionStatus::Stale,
            SessionStatus::Running
        ) | (
            SessionStatus::Creating | SessionStatus::Running | SessionStatus::Stale,
            SessionStatus::Dead
        ) | (
            SessionStatus::Running,
            SessionStatus::Completed | SessionStatus::Stale
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_creating_to_running() {
        assert!(is_valid_transition(
            SessionStatus::Creating,
            SessionStatus::Running
        ));
    }

    #[test]
    fn test_creating_to_dead() {
        assert!(is_valid_transition(
            SessionStatus::Creating,
            SessionStatus::Dead
        ));
    }

    #[test]
    fn test_running_to_completed() {
        assert!(is_valid_transition(
            SessionStatus::Running,
            SessionStatus::Completed
        ));
    }

    #[test]
    fn test_running_to_dead() {
        assert!(is_valid_transition(
            SessionStatus::Running,
            SessionStatus::Dead
        ));
    }

    #[test]
    fn test_running_to_stale() {
        assert!(is_valid_transition(
            SessionStatus::Running,
            SessionStatus::Stale
        ));
    }

    #[test]
    fn test_stale_to_running() {
        assert!(is_valid_transition(
            SessionStatus::Stale,
            SessionStatus::Running
        ));
    }

    #[test]
    fn test_stale_to_dead() {
        assert!(is_valid_transition(
            SessionStatus::Stale,
            SessionStatus::Dead
        ));
    }

    // Invalid transitions
    #[test]
    fn test_creating_to_completed_invalid() {
        assert!(!is_valid_transition(
            SessionStatus::Creating,
            SessionStatus::Completed
        ));
    }

    #[test]
    fn test_creating_to_stale_invalid() {
        assert!(!is_valid_transition(
            SessionStatus::Creating,
            SessionStatus::Stale
        ));
    }

    #[test]
    fn test_creating_to_creating_invalid() {
        assert!(!is_valid_transition(
            SessionStatus::Creating,
            SessionStatus::Creating
        ));
    }

    #[test]
    fn test_running_to_creating_invalid() {
        assert!(!is_valid_transition(
            SessionStatus::Running,
            SessionStatus::Creating
        ));
    }

    #[test]
    fn test_running_to_running_invalid() {
        assert!(!is_valid_transition(
            SessionStatus::Running,
            SessionStatus::Running
        ));
    }

    #[test]
    fn test_completed_to_anything_invalid() {
        assert!(!is_valid_transition(
            SessionStatus::Completed,
            SessionStatus::Running
        ));
        assert!(!is_valid_transition(
            SessionStatus::Completed,
            SessionStatus::Dead
        ));
        assert!(!is_valid_transition(
            SessionStatus::Completed,
            SessionStatus::Creating
        ));
        assert!(!is_valid_transition(
            SessionStatus::Completed,
            SessionStatus::Stale
        ));
        assert!(!is_valid_transition(
            SessionStatus::Completed,
            SessionStatus::Completed
        ));
    }

    #[test]
    fn test_dead_to_anything_invalid() {
        assert!(!is_valid_transition(
            SessionStatus::Dead,
            SessionStatus::Running
        ));
        assert!(!is_valid_transition(
            SessionStatus::Dead,
            SessionStatus::Creating
        ));
        assert!(!is_valid_transition(
            SessionStatus::Dead,
            SessionStatus::Completed
        ));
        assert!(!is_valid_transition(
            SessionStatus::Dead,
            SessionStatus::Stale
        ));
        assert!(!is_valid_transition(
            SessionStatus::Dead,
            SessionStatus::Dead
        ));
    }

    #[test]
    fn test_stale_to_completed_invalid() {
        assert!(!is_valid_transition(
            SessionStatus::Stale,
            SessionStatus::Completed
        ));
    }

    #[test]
    fn test_stale_to_creating_invalid() {
        assert!(!is_valid_transition(
            SessionStatus::Stale,
            SessionStatus::Creating
        ));
    }

    #[test]
    fn test_stale_to_stale_invalid() {
        assert!(!is_valid_transition(
            SessionStatus::Stale,
            SessionStatus::Stale
        ));
    }
}
