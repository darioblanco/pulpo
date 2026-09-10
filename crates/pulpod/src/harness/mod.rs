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
//! - [`codex::CodexAdapter`] — the Codex CLI adapter (isolated `CODEX_HOME` + hooks).
//! - [`pi::PiAdapter`] — pi (`@earendil-works/pi-coding-agent`) adapter (extension
//!   events, `--session-id`).

pub mod claude;
pub mod codex;
pub mod generic;
pub mod pi;
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

/// Resolve the absolute path of the running `pulpo` binary — the sibling of the
/// running `pulpod` binary — falling back to the plain `pulpo` (resolved via `$PATH`
/// at hook-invocation time) when that sibling doesn't exist.
///
/// Shared by every adapter that needs to embed pulpo's own binary path into a
/// generated hook/extension file (`claude.rs`'s `--settings` JSON, `pi.rs`'s
/// `pulpo.ts`).
pub(crate) fn resolve_pulpo_bin() -> String {
    resolve_pulpo_bin_from(std::env::current_exe().ok().as_deref())
}

/// Testable core of [`resolve_pulpo_bin`]: given a (possibly absent) current-exe
/// path, resolve its `pulpo` sibling if it exists on disk.
pub(crate) fn resolve_pulpo_bin_from(current_exe: Option<&Path>) -> String {
    current_exe
        .and_then(Path::parent)
        .map(|dir| dir.join("pulpo"))
        .filter(|candidate| candidate.is_file())
        .map_or_else(
            || "pulpo".to_owned(),
            |candidate| candidate.to_string_lossy().into_owned(),
        )
}

/// Which of the watchdog's scrollback-heuristic signals an adapter's own lifecycle
/// events replace (spec: harness-adapter watchdog bypass, made granular per adapter).
///
/// Once a session's `harness_last_event_at` is set (see `watchdog::harness_owns_state`),
/// the watchdog stops applying a signal's heuristic only when the adapter says it
/// owns that signal. An adapter that never fires an error/rate-limit-shaped event
/// (Codex today has no such hook) must leave those fields `false` so
/// `detect_rate_limit`/`detect_error` keep running from scrollback even while its
/// other events flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarnessSignals {
    /// Waiting-for-input pattern matching and the time-based Active→Idle transition.
    pub lifecycle: bool,
    /// Rate-limit scrollback scraping (`detect_rate_limit`).
    pub rate_limit: bool,
    /// Error-status scrollback scraping (`detect_error`).
    pub error: bool,
}

impl HarnessSignals {
    /// Every heuristic is replaced by the adapter's own events. The default for any
    /// adapter that doesn't override [`HarnessAdapter::owned_signals`] — correct for
    /// an adapter (like Claude's) whose hooks cover lifecycle, errors, and rate
    /// limits alike.
    #[must_use]
    pub const fn all() -> Self {
        Self {
            lifecycle: true,
            rate_limit: true,
            error: true,
        }
    }

    /// Only lifecycle (turn/session boundaries, permission/idle prompts) is covered
    /// by the adapter's own events; error and rate-limit detection stay heuristic.
    /// What Codex uses today — it has no error/rate-limit hook.
    #[must_use]
    pub const fn lifecycle_only() -> Self {
        Self {
            lifecycle: true,
            rate_limit: false,
            error: false,
        }
    }

    /// Nothing is owned — every heuristic stays active. What the watchdog treats a
    /// session as when its harness isn't actually emitting events yet.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            lifecycle: false,
            rate_limit: false,
            error: false,
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

    /// Which scrollback-heuristic signals this adapter's own events replace, once
    /// they're flowing (see [`HarnessSignals`]). Defaults to "all" — correct for an
    /// adapter whose hooks cover lifecycle, errors, and rate limits alike (Claude);
    /// an adapter missing some of those signals (Codex has no error/rate-limit hook)
    /// overrides this so the watchdog keeps covering them from scrollback.
    fn owned_signals(&self) -> HarnessSignals {
        HarnessSignals::all()
    }
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

/// Truncate `s` to at most `max_chars` `char`s (never splits a multi-byte codepoint).
/// Used to cap harness-supplied free text (a `Failed.error` message, or a
/// harness-specific `NeedsInputReason::Other` label) before it's stored as session
/// metadata — an unbounded harness payload must never bloat the sessions table.
fn truncate_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
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
                .map(|s| (meta::LAST_SUMMARY, truncate_chars(s, 200)))
                .into_iter()
                .collect();
            StateUpdate {
                status: Some(SessionStatus::Idle),
                metadata_set,
                // A turn finishing (e.g. `Stop` after the user approved a permission
                // prompt in the terminal, with no hook in between) always clears a
                // stale `needs_input` — otherwise `pulpo ls`/the web UI keep showing
                // "needs input (permission)" on a session that's simply done.
                metadata_clear: vec![meta::NEEDS_INPUT],
                set_idle_since: true,
                ..Default::default()
            }
        }
        HarnessEvent::NeedsInput { reason } => StateUpdate {
            status: Some(SessionStatus::Idle),
            metadata_set: vec![(meta::NEEDS_INPUT, truncate_chars(&reason.to_string(), 500))],
            notify: true,
            ..Default::default()
        },
        HarnessEvent::Failed {
            error,
            rate_limited,
        } => {
            let now = chrono::Utc::now().to_rfc3339();
            let error = truncate_chars(error, 500);
            let mut metadata_set = vec![
                (meta::ERROR_STATUS, error.clone()),
                (meta::ERROR_STATUS_AT, now.clone()),
            ];
            if *rate_limited {
                metadata_set.push((meta::RATE_LIMIT, error));
                metadata_set.push((meta::RATE_LIMIT_AT, now));
            }
            StateUpdate {
                status: Some(SessionStatus::Idle),
                metadata_set,
                // A failed turn also clears any stale `needs_input` — the harness
                // moved past whatever it was blocked on (or crashed out of it).
                metadata_clear: vec![meta::NEEDS_INPUT],
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
    fn test_harness_signals_all() {
        let signals = HarnessSignals::all();
        assert!(signals.lifecycle);
        assert!(signals.rate_limit);
        assert!(signals.error);
    }

    #[test]
    fn test_harness_signals_lifecycle_only() {
        let signals = HarnessSignals::lifecycle_only();
        assert!(signals.lifecycle);
        assert!(!signals.rate_limit);
        assert!(!signals.error);
    }

    #[test]
    fn test_harness_signals_none() {
        let signals = HarnessSignals::none();
        assert!(!signals.lifecycle);
        assert!(!signals.rate_limit);
        assert!(!signals.error);
    }

    #[test]
    fn test_owned_signals_default_is_all() {
        // GenericAdapter doesn't override it — the trait default applies.
        assert_eq!(
            generic::GenericAdapter.owned_signals(),
            HarnessSignals::all()
        );
        assert_eq!(claude::ClaudeAdapter.owned_signals(), HarnessSignals::all());
    }

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
    fn test_transition_turn_finished_clears_needs_input() {
        // Scenario: permission_prompt sets needs_input, the user approves in the
        // terminal (no hook fires for the approval itself), the turn then finishes
        // via `Stop` with no further hook to clear it — the stale "needs input
        // (permission)" badge must not survive a completed turn.
        let update = transition_for_event(&HarnessEvent::TurnFinished { summary: None }, true);
        assert_eq!(update.metadata_clear, vec![meta::NEEDS_INPUT]);
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
    fn test_transition_failed_clears_needs_input() {
        let update = transition_for_event(
            &HarnessEvent::Failed {
                error: "API error".into(),
                rate_limited: false,
            },
            true,
        );
        assert_eq!(update.metadata_clear, vec![meta::NEEDS_INPUT]);
    }

    #[test]
    fn test_transition_failed_truncates_error_to_500_chars() {
        let long = "x".repeat(2000);
        let update = transition_for_event(
            &HarnessEvent::Failed {
                error: long,
                rate_limited: true,
            },
            true,
        );
        for (key, value) in &update.metadata_set {
            if *key == meta::ERROR_STATUS || *key == meta::RATE_LIMIT {
                assert_eq!(value.chars().count(), 500, "key {key} not truncated");
            }
        }
    }

    #[test]
    fn test_transition_needs_input_truncates_other_reason_to_500_chars() {
        let long = "y".repeat(2000);
        let update = transition_for_event(
            &HarnessEvent::NeedsInput {
                reason: NeedsInputReason::Other(long),
            },
            true,
        );
        assert_eq!(update.metadata_set[0].1.chars().count(), 500);
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
