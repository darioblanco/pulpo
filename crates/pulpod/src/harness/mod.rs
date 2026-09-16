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
use pulpo_common::session::{SessionStatus, meta, status_reason};
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

    /// A resume command that continues the harness's own most-recently-active
    /// conversation, without needing to know its `harness_session_id` at all.
    ///
    /// Used by `session::manager::resolve_resume_command` when a session has an
    /// adapter but no known harness session id — a legacy row from before this
    /// session learned its id, a hook that never fired, or a user-supplied
    /// `--session-id`/identity flag that made `prepare_spawn` no-op at spawn time.
    /// Re-running the *original* command as a fresh conversation would silently
    /// lose history in that case; a "most recent conversation here" flag
    /// (`claude --continue`, `codex resume --last`, `pi -c`) preserves it instead.
    ///
    /// Distinct from [`resume_command`](HarnessAdapter::resume_command), which
    /// resumes an exact, previously-learned id. Returns `None` when the adapter has
    /// no such fallback, or the command conflicts with it — the caller then falls
    /// back to the plain original command, same as it always has.
    fn fallback_resume_command(&self, _original_command: &str) -> Option<String> {
        None
    }

    /// Whether [`fallback_resume_command`](HarnessAdapter::fallback_resume_command)'s
    /// output means "the most recent conversation in the *current working
    /// directory*" — true for Claude's `--continue` and pi's `-c`/`--continue`
    /// (pi's own docs describe it as scoped to `(cwd, sessionDir)`), false for
    /// Codex's `resume --last`, which is scoped to this session's own isolated
    /// `CODEX_HOME` (keyed by session id, not by cwd — see `codex::rewrite_spawn`).
    ///
    /// `session::manager::resolve_resume_command` uses this to refuse a fallback
    /// resume rather than silently run it from the wrong directory: when a
    /// session's worktree has been removed, `effective_resume_workdir` falls back
    /// to the original (non-worktree) `workdir`, so a cwd-scoped fallback command
    /// run there could continue a completely unrelated conversation that happens
    /// to be the most recent one in that directory. Defaults to `true` — the safer
    /// default is to assume a fallback is cwd-scoped and refuse rather than guess;
    /// an adapter overrides it to `false` only once it's confirmed its fallback
    /// doesn't depend on cwd at all.
    fn fallback_resume_is_cwd_scoped(&self) -> bool {
        true
    }

    /// Whether this adapter's own exact
    /// [`resume_command`](HarnessAdapter::resume_command) — resuming a
    /// previously-learned `harness_session_id`, not the fallback — is itself
    /// scoped to the *directory it runs from*, the same shape
    /// [`fallback_resume_is_cwd_scoped`](HarnessAdapter::fallback_resume_is_cwd_scoped)
    /// describes for the fallback path.
    ///
    /// `false` for every adapter by default: Claude's `--resume <id>` and Codex's
    /// `resume <id>` are both keyed globally by id regardless of the current
    /// working directory, so an exact resume is safe to run from a substituted
    /// workdir even when the session's original worktree is gone. pi overrides
    /// this to `true` — pi's own `--session-id <id>` is documented (see `pi.rs`'s
    /// module doc) as idempotent create-or-open *scoped to `(cwd, sessionDir)`*:
    /// running it from a different directory than the harness's original
    /// conversation silently opens (or creates) a *different*, unrelated session
    /// at that id instead of erroring.
    ///
    /// `session::manager::resolve_resume_command` applies the same worktree-gone
    /// refusal to the exact-resume path this describes as
    /// [`fallback_resume_is_cwd_scoped`](HarnessAdapter::fallback_resume_is_cwd_scoped)
    /// already applies to the fallback path.
    fn resume_is_cwd_scoped(&self) -> bool {
        false
    }

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
    /// `Session::status_reason` to pair with `status` — see
    /// `pulpo_common::session::status_reason`. `None` when `status` is `None`, or
    /// when the target status is one that never carries a reason.
    pub status_reason: Option<String>,
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
/// `SessionEnded` deliberately returns `status: None` — the harness process is in
/// the middle of exiting, but the wrapper's own exit marker (the deterministic
/// signal `wrap_command` writes) hasn't necessarily landed yet, and the tmux backend
/// hasn't necessarily died yet either. `session::manager::apply_harness_event`
/// leaves the status alone in that case and lets the ordinary dead-backend
/// classification (`SessionManager::resolve_dead_backend_session`, driven by the
/// next `get_session`/`list_sessions`/`resume_lost_sessions` check) resolve it to
/// `Done`/`Lost` once the backend is actually confirmed dead — except when the exit
/// marker has *already* landed by the time the event is processed, in which case the
/// caller transitions straight to `Done` itself rather than waiting.
#[must_use]
pub fn transition_for_event(event: &HarnessEvent) -> StateUpdate {
    match event {
        HarnessEvent::SessionStarted {
            harness_session_id,
            resumed: _,
        } => StateUpdate {
            status: Some(SessionStatus::Working),
            // Opportunistic cleanup of the pre-ADR-0009 `needs_input` metadata key —
            // new code never writes it (the reason lives in `status_reason` now), but
            // a legacy row not yet touched by migration 0010 may still have it.
            metadata_clear: vec![meta::NEEDS_INPUT],
            harness_session_id: harness_session_id.clone(),
            ..Default::default()
        },
        HarnessEvent::Working => StateUpdate {
            status: Some(SessionStatus::Working),
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
                status: Some(SessionStatus::Waiting),
                status_reason: Some(status_reason::IDLE.to_owned()),
                metadata_set,
                // A turn finishing (e.g. `Stop` after the user approved a permission
                // prompt in the terminal, with no hook in between) always clears a
                // stale `needs_input` key — otherwise a legacy row could keep it
                // forever now that new code never overwrites it.
                metadata_clear: vec![meta::NEEDS_INPUT],
                set_idle_since: true,
                ..Default::default()
            }
        }
        HarnessEvent::NeedsInput { reason } => StateUpdate {
            status: Some(SessionStatus::Waiting),
            status_reason: Some(status_reason::needs_input(&truncate_chars(
                &reason.to_string(),
                500,
            ))),
            metadata_clear: vec![meta::NEEDS_INPUT],
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
                status: Some(SessionStatus::Waiting),
                status_reason: Some(status_reason::IDLE.to_owned()),
                metadata_set,
                // A failed turn also clears any stale `needs_input` key — the harness
                // moved past whatever it was blocked on (or crashed out of it).
                metadata_clear: vec![meta::NEEDS_INPUT],
                notify: true,
                ..Default::default()
            }
        }
        HarnessEvent::SessionEnded { reason: _ } => StateUpdate::default(),
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
    fn test_fallback_resume_command_default_is_none() {
        // GenericAdapter doesn't override it — the trait default applies, and a
        // legacy/harness-less session keeps falling back to the plain original
        // command exactly as it did before this method existed.
        assert!(
            generic::GenericAdapter
                .fallback_resume_command("bash")
                .is_none()
        );
    }

    #[test]
    fn test_fallback_resume_is_cwd_scoped_default_is_true() {
        // The trait default is the conservative one: assume cwd-scoped (and thus
        // refuse a fallback resume from a different directory) unless an adapter
        // has confirmed otherwise. GenericAdapter, ClaudeAdapter, and PiAdapter
        // don't override it — pi's own `-c`/`--continue` is documented (see
        // `pi.rs`'s module doc) as "most recent session in this cwd", same as
        // Claude's `--continue`. Only Codex overrides it (see codex.rs's own
        // test) — its fallback is keyed by an isolated `CODEX_HOME`, not cwd.
        assert!(generic::GenericAdapter.fallback_resume_is_cwd_scoped());
        assert!(claude::ClaudeAdapter.fallback_resume_is_cwd_scoped());
        assert!(pi::PiAdapter.fallback_resume_is_cwd_scoped());
    }

    #[test]
    fn test_resume_is_cwd_scoped_false_by_default_except_pi() {
        // Claude's `--resume <id>` and Codex's `resume <id>` are both keyed
        // globally by id, so the conservative "assume cwd-scoped" stance that
        // fallback resume takes does NOT apply here — only pi's exact
        // `--session-id <id>` is itself scoped to `(cwd, sessionDir)`.
        assert!(!generic::GenericAdapter.resume_is_cwd_scoped());
        assert!(!claude::ClaudeAdapter.resume_is_cwd_scoped());
        assert!(!codex::CodexAdapter.resume_is_cwd_scoped());
        assert!(pi::PiAdapter.resume_is_cwd_scoped());
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
    fn test_transition_session_started_sets_working_and_id() {
        let update = transition_for_event(&HarnessEvent::SessionStarted {
            harness_session_id: Some("sid-1".into()),
            resumed: false,
        });
        assert_eq!(update.status, Some(SessionStatus::Working));
        assert_eq!(update.status_reason, None);
        assert_eq!(update.harness_session_id.as_deref(), Some("sid-1"));
        assert_eq!(update.metadata_clear, vec![meta::NEEDS_INPUT]);
        assert!(!update.notify);
    }

    #[test]
    fn test_transition_session_started_resumed_still_working() {
        let update = transition_for_event(&HarnessEvent::SessionStarted {
            harness_session_id: None,
            resumed: true,
        });
        assert_eq!(update.status, Some(SessionStatus::Working));
        assert!(update.harness_session_id.is_none());
    }

    #[test]
    fn test_transition_working_clears_needs_input_and_error() {
        let update = transition_for_event(&HarnessEvent::Working);
        assert_eq!(update.status, Some(SessionStatus::Working));
        assert_eq!(update.status_reason, None);
        assert_eq!(
            update.metadata_clear,
            vec![meta::NEEDS_INPUT, meta::ERROR_STATUS, meta::ERROR_STATUS_AT]
        );
        assert!(!update.notify);
    }

    #[test]
    fn test_transition_turn_finished_sets_waiting_idle_and_summary() {
        let update = transition_for_event(&HarnessEvent::TurnFinished {
            summary: Some("Fixed the bug".into()),
        });
        assert_eq!(update.status, Some(SessionStatus::Waiting));
        assert_eq!(update.status_reason.as_deref(), Some(status_reason::IDLE));
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
        let update = transition_for_event(&HarnessEvent::TurnFinished {
            summary: Some(long),
        });
        assert_eq!(update.metadata_set[0].1.chars().count(), 200);
    }

    #[test]
    fn test_transition_turn_finished_no_summary_sets_no_metadata() {
        let update = transition_for_event(&HarnessEvent::TurnFinished { summary: None });
        assert!(update.metadata_set.is_empty());
        assert!(update.set_idle_since);
    }

    #[test]
    fn test_transition_turn_finished_clears_needs_input() {
        // Scenario: permission_prompt sets needs_input, the user approves in the
        // terminal (no hook fires for the approval itself), the turn then finishes
        // via `Stop` with no further hook to clear it — a stale legacy `needs_input`
        // metadata key (kept for cleanup only; new code never writes it) must not
        // survive a completed turn.
        let update = transition_for_event(&HarnessEvent::TurnFinished { summary: None });
        assert_eq!(update.metadata_clear, vec![meta::NEEDS_INPUT]);
    }

    #[test]
    fn test_transition_needs_input_sets_waiting_and_reason_and_notifies() {
        let update = transition_for_event(&HarnessEvent::NeedsInput {
            reason: NeedsInputReason::Permission,
        });
        assert_eq!(update.status, Some(SessionStatus::Waiting));
        assert_eq!(
            update.status_reason.as_deref(),
            Some("needs_input:permission")
        );
        assert!(update.metadata_set.is_empty());
        assert!(update.notify);
    }

    #[test]
    fn test_transition_needs_input_idle_reason_is_distinct_from_plain_idle() {
        // `NeedsInputReason::Idle` (the harness idle-prompting the user, e.g. "still
        // there?") is a *needs_input* sub-reason, distinct from the plain top-level
        // `waiting` reason `idle` (turn finished / no output) that `TurnFinished`/
        // `Failed` set.
        let update = transition_for_event(&HarnessEvent::NeedsInput {
            reason: NeedsInputReason::Idle,
        });
        assert_eq!(update.status_reason.as_deref(), Some("needs_input:idle"));
        assert_ne!(update.status_reason.as_deref(), Some(status_reason::IDLE));
    }

    #[test]
    fn test_transition_failed_sets_waiting_idle_error_and_notifies() {
        let update = transition_for_event(&HarnessEvent::Failed {
            error: "API error".into(),
            rate_limited: false,
        });
        assert_eq!(update.status, Some(SessionStatus::Waiting));
        assert_eq!(update.status_reason.as_deref(), Some(status_reason::IDLE));
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
        let update = transition_for_event(&HarnessEvent::Failed {
            error: "429 rate limited".into(),
            rate_limited: true,
        });
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
        let update = transition_for_event(&HarnessEvent::Failed {
            error: "API error".into(),
            rate_limited: false,
        });
        assert_eq!(update.metadata_clear, vec![meta::NEEDS_INPUT]);
    }

    #[test]
    fn test_transition_failed_truncates_error_to_500_chars() {
        let long = "x".repeat(2000);
        let update = transition_for_event(&HarnessEvent::Failed {
            error: long,
            rate_limited: true,
        });
        for (key, value) in &update.metadata_set {
            if *key == meta::ERROR_STATUS || *key == meta::RATE_LIMIT {
                assert_eq!(value.chars().count(), 500, "key {key} not truncated");
            }
        }
    }

    #[test]
    fn test_transition_needs_input_truncates_other_reason_to_500_chars() {
        let long = "y".repeat(2000);
        let update = transition_for_event(&HarnessEvent::NeedsInput {
            reason: NeedsInputReason::Other(long),
        });
        // "needs_input:" (12 chars) + 500 chars of the (truncated) inner reason.
        assert_eq!(
            update.status_reason.as_deref().unwrap().len()
                - status_reason::NEEDS_INPUT_PREFIX.len(),
            500
        );
    }

    #[test]
    fn test_transition_session_ended_leaves_status_alone() {
        // The harness process is only just starting to exit — the caller
        // (`session::manager::apply_harness_event`) decides whether to transition to
        // `Done` immediately (exit marker already landed) or leave the status as-is
        // and let the ordinary dead-backend classification catch it later.
        let update = transition_for_event(&HarnessEvent::SessionEnded { reason: None });
        assert_eq!(update.status, None);
        assert_eq!(update.status_reason, None);

        let update = transition_for_event(&HarnessEvent::SessionEnded {
            reason: Some("exit".into()),
        });
        assert_eq!(update.status, None);
    }
}
