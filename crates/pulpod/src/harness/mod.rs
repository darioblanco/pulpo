//! Harness adapters: normalize agent lifecycle events instead of scraping tmux
//! scrollback.
//!
//! Pulpo stays harness-agnostic: nothing outside this module (and the small
//! integration points in `session::manager` and `watchdog`) may assume a specific
//! agent CLI. A [`HarnessAdapter`] translates between "the shape of one specific
//! agent CLI" and the normalized [`HarnessEvent`] the rest of the daemon understands.
//!
//! - [`HarnessAdapter`] — the trait every adapter implements.
//! - [`HarnessRegistry`] — resolves a command line (or an explicit harness id) to an
//!   adapter, `generic` last.
//! - [`generic::GenericAdapter`] — the harness-agnostic fallback: no rewrite, no
//!   resume id, no events. Always matches, always last in the registry.
//! - [`claude::ClaudeAdapter`] — the first concrete adapter (Claude Code hooks).

pub mod claude;
pub mod generic;
pub mod registry;

use std::path::{Path, PathBuf};

use anyhow::Result;
use pulpo_common::session::{SessionStatus, meta};
use serde::{Deserialize, Serialize};

pub use registry::HarnessRegistry;

/// Why the agent is blocked on the human, normalized across harnesses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NeedsInputReason {
    /// A permission/tool-approval prompt is showing.
    Permission,
    /// The agent asked the user a question.
    Question,
    /// The harness is idle-prompting the user (e.g. "still there?").
    Idle,
    /// A harness-specific reason that doesn't map to one of the above; the inner
    /// string is the harness's own label, shown as-is.
    Other(String),
}

impl std::fmt::Display for NeedsInputReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Permission => write!(f, "permission"),
            Self::Question => write!(f, "question"),
            Self::Idle => write!(f, "idle"),
            Self::Other(label) => write!(f, "{label}"),
        }
    }
}

/// Normalized lifecycle event, harness-independent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HarnessEvent {
    /// The harness reported its own session/thread id (needed for resume).
    SessionStarted {
        harness_session_id: Option<String>,
        resumed: bool,
    },
    /// A user prompt was submitted / the agent started working.
    Working,
    /// The agent finished a turn and is waiting at its prompt.
    TurnFinished { summary: Option<String> },
    /// The agent is blocked on the human (permission, question, elicitation, idle prompt).
    NeedsInput { reason: NeedsInputReason },
    /// The turn ended with an error (API error, rate limit, crash).
    Failed { error: String, rate_limited: bool },
    /// The harness process is exiting.
    SessionEnded { reason: Option<String> },
}

/// Everything an adapter needs to rewrite a spawn so the harness reports events back
/// to pulpo.
pub struct SpawnContext<'a> {
    /// The pulpo session uuid (as a string).
    pub session_id: &'a str,
    pub session_name: &'a str,
    pub workdir: &'a str,
    /// The user's original command line (unwrapped — before `wrap_command`).
    pub command: &'a str,
    /// Pulpo's data dir; adapters may write files under
    /// `<data_dir>/harness/<session_id>/`.
    pub data_dir: &'a Path,
}

/// The result of [`HarnessAdapter::prepare_spawn`].
pub struct SpawnPlan {
    /// Possibly rewritten command line (unchanged when the adapter declines to rewrite).
    pub command: String,
    /// Extra env vars to export before the command. Most adapters don't need this —
    /// the injected settings/flags are usually enough — but it's here for adapters
    /// that need to pass something through the environment instead.
    pub env: Vec<(String, String)>,
    /// Files written under `<data_dir>/harness/<session_id>/` (for cleanup on session
    /// removal).
    pub files: Vec<PathBuf>,
    /// The harness's own session/thread id, when the adapter already knows it up
    /// front (e.g. Claude's pre-generated `--session-id`). `None` when the adapter
    /// only learns it later via an event, or never (e.g. `GenericAdapter`).
    pub harness_session_id: Option<String>,
}

impl SpawnPlan {
    /// The no-op plan: command unchanged, nothing written, nothing known up front.
    /// What every adapter returns when it declines to rewrite the spawn.
    #[must_use]
    pub fn unchanged(command: &str) -> Self {
        Self {
            command: command.to_owned(),
            env: Vec::new(),
            files: Vec::new(),
            harness_session_id: None,
        }
    }
}

/// A harness adapter: translates between one agent CLI's conventions and pulpo's
/// normalized lifecycle events.
pub trait HarnessAdapter: Send + Sync {
    /// Stable id: "claude", "codex", "pi", "gemini", "generic".
    fn id(&self) -> &'static str;

    /// True if this adapter owns the command (match on basename of argv[0]).
    fn matches(&self, argv0: &str) -> bool;

    /// Rewrite the spawn so the harness reports events back to pulpo.
    ///
    /// Must be a no-op (return the command unchanged) if the user already passed
    /// flags that conflict with pulpo's own injection. Never fails spawn because of
    /// an adapter problem: implementations should log a warning and fall back to the
    /// unchanged command rather than propagate an error that would abort the spawn —
    /// callers still treat `Err` as non-fatal and fall back themselves, but adapters
    /// should prefer not to rely on that.
    fn prepare_spawn(&self, ctx: &SpawnContext) -> Result<SpawnPlan>;

    /// Command line to resume an existing harness session, if the adapter knows how.
    ///
    /// `original_command` is the user's original command line; adapters should
    /// preserve the user's flags where the harness allows it. Returns `None` if no id
    /// is known / unsupported.
    fn resume_command(&self, original_command: &str, harness_session_id: &str) -> Option<String>;

    /// Translate a raw hook payload (as posted by `pulpo hook <harness>`) into a
    /// normalized event. Returns `Ok(None)` for events pulpo does not care about.
    fn parse_event(&self, raw: &serde_json::Value) -> Result<Option<HarnessEvent>>;

    /// Whether this adapter emits events at all (`generic` = false). Used by the
    /// watchdog to decide whether to keep using scrollback heuristics.
    fn emits_events(&self) -> bool;
}

/// The concrete session/metadata changes to apply for one [`HarnessEvent`].
///
/// Per the state-transition table, computed by [`transition_for_event`] — pure and
/// independently testable; `session::manager` applies it against the store.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StateUpdate {
    pub status: Option<SessionStatus>,
    pub metadata_set: Vec<(&'static str, String)>,
    pub metadata_clear: Vec<&'static str>,
    /// Set on `SessionStarted` when the harness reported its own session id.
    pub harness_session_id: Option<String>,
    /// Whether to stamp `idle_since = now` (set on `TurnFinished`).
    pub set_idle_since: bool,
    /// Whether this transition should trigger a notification (reuses the existing
    /// notification paths — no new channel).
    pub notify: bool,
}

/// Map a normalized [`HarnessEvent`] to the session/metadata changes it implies.
///
/// `backend_alive` decides the `SessionEnded` branch (Ready if the backend — e.g. the
/// tmux session — is still alive, Stopped if it's already gone); the caller resolves
/// this via the `Backend` trait since it requires I/O this pure function can't do.
#[must_use]
pub fn transition_for_event(event: &HarnessEvent, backend_alive: bool) -> StateUpdate {
    match event {
        HarnessEvent::SessionStarted {
            harness_session_id,
            resumed: _,
        } => StateUpdate {
            status: Some(SessionStatus::Active),
            metadata_clear: vec![meta::NEEDS_INPUT],
            harness_session_id: harness_session_id.clone(),
            ..Default::default()
        },
        HarnessEvent::Working => StateUpdate {
            status: Some(SessionStatus::Active),
            metadata_clear: vec![meta::NEEDS_INPUT, meta::ERROR_STATUS, meta::ERROR_STATUS_AT],
            ..Default::default()
        },
        HarnessEvent::TurnFinished { summary } => {
            let metadata_set = summary
                .as_ref()
                .map(|s| (meta::LAST_SUMMARY, s.chars().take(200).collect()))
                .into_iter()
                .collect();
            StateUpdate {
                status: Some(SessionStatus::Idle),
                metadata_set,
                set_idle_since: true,
                ..Default::default()
            }
        }
        HarnessEvent::NeedsInput { reason } => StateUpdate {
            status: Some(SessionStatus::Idle),
            metadata_set: vec![(meta::NEEDS_INPUT, reason.to_string())],
            notify: true,
            ..Default::default()
        },
        HarnessEvent::Failed {
            error,
            rate_limited,
        } => {
            let now = chrono::Utc::now().to_rfc3339();
            let mut metadata_set = vec![
                (meta::ERROR_STATUS, error.clone()),
                (meta::ERROR_STATUS_AT, now.clone()),
            ];
            if *rate_limited {
                metadata_set.push((meta::RATE_LIMIT, error.clone()));
                metadata_set.push((meta::RATE_LIMIT_AT, now));
            }
            StateUpdate {
                status: Some(SessionStatus::Idle),
                metadata_set,
                notify: true,
                ..Default::default()
            }
        }
        HarnessEvent::SessionEnded { reason: _ } => StateUpdate {
            status: Some(if backend_alive {
                SessionStatus::Ready
            } else {
                SessionStatus::Stopped
            }),
            ..Default::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_needs_input_reason_display() {
        assert_eq!(NeedsInputReason::Permission.to_string(), "permission");
        assert_eq!(NeedsInputReason::Question.to_string(), "question");
        assert_eq!(NeedsInputReason::Idle.to_string(), "idle");
        assert_eq!(
            NeedsInputReason::Other("custom_thing".into()).to_string(),
            "custom_thing"
        );
    }

    #[test]
    fn test_needs_input_reason_serde_roundtrip() {
        let reason = NeedsInputReason::Other("weird".into());
        let json = serde_json::to_string(&reason).unwrap();
        let back: NeedsInputReason = serde_json::from_str(&json).unwrap();
        assert_eq!(back, reason);
    }

    #[test]
    fn test_spawn_plan_unchanged() {
        let plan = SpawnPlan::unchanged("claude -p hi");
        assert_eq!(plan.command, "claude -p hi");
        assert!(plan.env.is_empty());
        assert!(plan.files.is_empty());
        assert!(plan.harness_session_id.is_none());
    }

    #[test]
    fn test_transition_session_started_sets_active_and_id() {
        let update = transition_for_event(
            &HarnessEvent::SessionStarted {
                harness_session_id: Some("sid-1".into()),
                resumed: false,
            },
            true,
        );
        assert_eq!(update.status, Some(SessionStatus::Active));
        assert_eq!(update.harness_session_id.as_deref(), Some("sid-1"));
        assert_eq!(update.metadata_clear, vec![meta::NEEDS_INPUT]);
        assert!(!update.notify);
    }

    #[test]
    fn test_transition_session_started_resumed_still_active() {
        let update = transition_for_event(
            &HarnessEvent::SessionStarted {
                harness_session_id: None,
                resumed: true,
            },
            true,
        );
        assert_eq!(update.status, Some(SessionStatus::Active));
        assert!(update.harness_session_id.is_none());
    }

    #[test]
    fn test_transition_working_clears_needs_input_and_error() {
        let update = transition_for_event(&HarnessEvent::Working, true);
        assert_eq!(update.status, Some(SessionStatus::Active));
        assert_eq!(
            update.metadata_clear,
            vec![meta::NEEDS_INPUT, meta::ERROR_STATUS, meta::ERROR_STATUS_AT]
        );
        assert!(!update.notify);
    }

    #[test]
    fn test_transition_turn_finished_sets_idle_and_summary() {
        let update = transition_for_event(
            &HarnessEvent::TurnFinished {
                summary: Some("Fixed the bug".into()),
            },
            true,
        );
        assert_eq!(update.status, Some(SessionStatus::Idle));
        assert!(update.set_idle_since);
        assert_eq!(
            update.metadata_set,
            vec![(meta::LAST_SUMMARY, "Fixed the bug".to_owned())]
        );
        assert!(!update.notify);
    }

    #[test]
    fn test_transition_turn_finished_truncates_summary_to_200_chars() {
        let long = "x".repeat(500);
        let update = transition_for_event(
            &HarnessEvent::TurnFinished {
                summary: Some(long),
            },
            true,
        );
        assert_eq!(update.metadata_set[0].1.chars().count(), 200);
    }

    #[test]
    fn test_transition_turn_finished_no_summary_sets_no_metadata() {
        let update = transition_for_event(&HarnessEvent::TurnFinished { summary: None }, true);
        assert!(update.metadata_set.is_empty());
        assert!(update.set_idle_since);
    }

    #[test]
    fn test_transition_needs_input_sets_idle_and_reason_and_notifies() {
        let update = transition_for_event(
            &HarnessEvent::NeedsInput {
                reason: NeedsInputReason::Permission,
            },
            true,
        );
        assert_eq!(update.status, Some(SessionStatus::Idle));
        assert_eq!(
            update.metadata_set,
            vec![(meta::NEEDS_INPUT, "permission".to_owned())]
        );
        assert!(update.notify);
    }

    #[test]
    fn test_transition_failed_sets_idle_error_and_notifies() {
        let update = transition_for_event(
            &HarnessEvent::Failed {
                error: "API error".into(),
                rate_limited: false,
            },
            true,
        );
        assert_eq!(update.status, Some(SessionStatus::Idle));
        assert!(
            update
                .metadata_set
                .contains(&(meta::ERROR_STATUS, "API error".to_owned()))
        );
        assert!(
            !update
                .metadata_set
                .iter()
                .any(|(k, _)| *k == meta::RATE_LIMIT)
        );
        assert!(update.notify);
    }

    #[test]
    fn test_transition_failed_rate_limited_sets_rate_limit_metadata() {
        let update = transition_for_event(
            &HarnessEvent::Failed {
                error: "429 rate limited".into(),
                rate_limited: true,
            },
            true,
        );
        assert!(
            update
                .metadata_set
                .iter()
                .any(|(k, v)| *k == meta::RATE_LIMIT && v == "429 rate limited")
        );
        assert!(
            update
                .metadata_set
                .iter()
                .any(|(k, _)| *k == meta::RATE_LIMIT_AT)
        );
    }

    #[test]
    fn test_transition_session_ended_ready_when_backend_alive() {
        let update = transition_for_event(&HarnessEvent::SessionEnded { reason: None }, true);
        assert_eq!(update.status, Some(SessionStatus::Ready));
    }

    #[test]
    fn test_transition_session_ended_stopped_when_backend_gone() {
        let update = transition_for_event(
            &HarnessEvent::SessionEnded {
                reason: Some("exit".into()),
            },
            false,
        );
        assert_eq!(update.status, Some(SessionStatus::Stopped));
    }
}
