use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use std::collections::HashMap;

/// The runtime environment for a session.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Runtime {
    /// Native tmux session (default).
    #[default]
    Tmux,
    /// Docker container session (retired). The docker session runtime was
    /// removed — this variant is kept only so historical session rows stored
    /// with `runtime = "docker"` continue to deserialize and list. Spawning,
    /// resuming, or scheduling with it is rejected server-side.
    Docker,
}

impl fmt::Display for Runtime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tmux => write!(f, "tmux"),
            Self::Docker => write!(f, "docker"),
        }
    }
}

impl FromStr for Runtime {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "tmux" => Ok(Self::Tmux),
            "docker" => Ok(Self::Docker),
            other => Err(format!("unknown runtime: {other}")),
        }
    }
}

/// Machine-readable intervention reason codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterventionCode {
    /// Stopped due to system memory pressure exceeding threshold.
    MemoryPressure,
    /// Stopped due to session idle timeout.
    IdleTimeout,
    /// Manually stopped by user via API/CLI.
    #[serde(alias = "user_kill")]
    UserStop,
    /// Stopped because the session exceeded its configured cost budget.
    BudgetExceeded,
}

impl fmt::Display for InterventionCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MemoryPressure => write!(f, "memory_pressure"),
            Self::IdleTimeout => write!(f, "idle_timeout"),
            Self::UserStop => write!(f, "user_stop"),
            Self::BudgetExceeded => write!(f, "budget_exceeded"),
        }
    }
}

impl FromStr for InterventionCode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "memory_pressure" => Ok(Self::MemoryPressure),
            "idle_timeout" => Ok(Self::IdleTimeout),
            "user_stop" | "user_kill" => Ok(Self::UserStop),
            "budget_exceeded" => Ok(Self::BudgetExceeded),
            other => Err(format!("unknown intervention code: {other}")),
        }
    }
}

/// The five-state session model (ADR 0009): `starting` (spawn requested, backend
/// not yet confirmed), `working` (the agent process is running and busy), `waiting`
/// (the agent is at its prompt — `Session::status_reason` says why: `idle` or
/// `needs_input:<reason>`), `done` (the process has exited and the backend is gone —
/// `status_reason` says how: `exited`, `stopped`, or an intervention code — resumable),
/// and `lost` (the backend died with no evidence of a clean end — resumable).
///
/// Replaces the old six-state model (`creating`, `active`, `idle`, `ready`, `stopped`,
/// `lost`): `ready` and `stopped` both meant "not running, and resumable" and are
/// merged into `done`, with the distinction moved to `status_reason`. `#[serde(alias)]`
/// and the matching [`FromStr`] arms keep old JSON payloads and DB rows (pre-migration
/// text, or a stale client) parseable.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    #[default]
    #[serde(alias = "creating")]
    Starting,
    #[serde(alias = "active")]
    Working,
    #[serde(alias = "idle")]
    Waiting,
    #[serde(alias = "ready", alias = "stopped", alias = "killed")]
    Done,
    Lost,
}

impl fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Starting => write!(f, "starting"),
            Self::Working => write!(f, "working"),
            Self::Waiting => write!(f, "waiting"),
            Self::Done => write!(f, "done"),
            Self::Lost => write!(f, "lost"),
        }
    }
}

impl FromStr for SessionStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "starting" | "creating" => Ok(Self::Starting),
            "working" | "active" => Ok(Self::Working),
            "waiting" | "idle" => Ok(Self::Waiting),
            "done" | "ready" | "stopped" | "killed" => Ok(Self::Done),
            "lost" => Ok(Self::Lost),
            other => Err(format!("unknown session status: {other}")),
        }
    }
}

/// Canonical `status_reason` values (ADR 0009). Stored as a plain TEXT column
/// (`sessions.status_reason`, migration `0010_five_state_status.sql`) rather than a
/// typed enum — the CLI/web/API format it for display, but nothing branches on it
/// beyond string matching/prefixing, so a free-form string keeps the wire format and
/// the DB schema simple. `None` on the [`Session`] for every status except `waiting`
/// and `done`.
pub mod status_reason {
    /// `waiting`: the turn finished (or there's simply no new output) — not
    /// specifically blocked on the user. Distinguished from [`NEEDS_INPUT_PREFIX`].
    pub const IDLE: &str = "idle";
    /// Prefix for a `waiting` session blocked on the user (a permission/question
    /// prompt, from a harness hook or a scrollback waiting-pattern match). The suffix
    /// is the harness's own reason label (`permission`, `question`, `idle`, or a
    /// harness-specific string) — see [`needs_input`]/[`needs_input_reason`].
    pub const NEEDS_INPUT_PREFIX: &str = "needs_input:";
    /// `done`: the agent process exited on its own, or the session's shell ended
    /// normally with no explicit `pulpo stop` — a clean end. `Session::exit_code`
    /// carries the code when one was recorded.
    pub const EXITED: &str = "exited";
    /// `done`: the user ran `pulpo stop` (or the API/CLI equivalent).
    pub const STOPPED: &str = "stopped";
    /// `done`: watchdog idle-timeout kill. Mirrors `InterventionCode::IdleTimeout`'s
    /// `Display` string.
    pub const IDLE_TIMEOUT: &str = "idle_timeout";
    /// `done`: budget breaker stop. Mirrors `InterventionCode::BudgetExceeded`'s
    /// `Display` string.
    pub const BUDGET_EXCEEDED: &str = "budget_exceeded";
    /// `done`: memory-pressure breaker stop. Mirrors `InterventionCode::MemoryPressure`'s
    /// `Display` string.
    pub const MEMORY_PRESSURE: &str = "memory_pressure";

    /// Build a `waiting` reason blocked on the user: `needs_input:<reason>` (e.g.
    /// `needs_input:permission`).
    #[must_use]
    pub fn needs_input(reason: &str) -> String {
        format!("{NEEDS_INPUT_PREFIX}{reason}")
    }

    /// Split a `needs_input:<reason>` status_reason into its inner reason. `None` for
    /// anything else, including plain `idle` — callers that want to render "blocked on
    /// me" vs. "just idle" branch on this.
    #[must_use]
    pub fn needs_input_reason(reason: &str) -> Option<&str> {
        reason.strip_prefix(NEEDS_INPUT_PREFIX)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub name: String,
    pub workdir: String,
    pub command: String,
    pub description: Option<String>,
    pub status: SessionStatus,
    /// Why the session is in its current status — see [`status_reason`]. Only ever
    /// set for `waiting` (`idle` / `needs_input:<reason>`) and `done` (`exited` /
    /// `stopped` / an intervention code); `None` for `starting`/`working`/`lost`.
    #[serde(default)]
    pub status_reason: Option<String>,
    pub exit_code: Option<i32>,
    pub backend_session_id: Option<String>,
    pub output_snapshot: Option<String>,
    pub metadata: Option<HashMap<String, String>>,
    /// Ink the session was spawned from (historical). The ink registry was
    /// removed — this is never set for new sessions but is preserved on the
    /// wire for sessions created before the removal.
    pub ink: Option<String>,
    pub intervention_code: Option<InterventionCode>,
    pub intervention_reason: Option<String>,
    pub intervention_at: Option<DateTime<Utc>>,
    pub last_output_at: Option<DateTime<Utc>>,
    pub idle_since: Option<DateTime<Utc>>,
    /// Per-session idle threshold override.
    /// `None` = use global, `Some(0)` = never idle, `Some(N)` = N seconds.
    pub idle_threshold_secs: Option<u32>,
    /// Path to the git worktree created for this session, if any.
    /// When set, the worktree is cleaned up when the session is stopped.
    pub worktree_path: Option<String>,
    /// Git branch name for the worktree (e.g. the session name or a custom name).
    pub worktree_branch: Option<String>,
    /// Current git branch detected by the watchdog (updated periodically).
    pub git_branch: Option<String>,
    /// Current git short commit hash detected by the watchdog (updated periodically).
    pub git_commit: Option<String>,
    /// Number of files changed in the working directory (tracked by watchdog).
    pub git_files_changed: Option<u32>,
    /// Lines added in the working directory (tracked by watchdog).
    pub git_insertions: Option<u32>,
    /// Lines deleted in the working directory (tracked by watchdog).
    pub git_deletions: Option<u32>,
    /// Commits ahead of remote tracking branch (tracked by watchdog).
    pub git_ahead: Option<u32>,
    /// The runtime environment for this session. Always tmux for new sessions;
    /// docker only appears on historical rows (the docker runtime was removed).
    #[serde(default)]
    pub runtime: Runtime,
    /// Harness adapter id (e.g. "claude"), set at spawn time when the launched
    /// command matched a known harness. `None` for sessions spawned before harness
    /// adapters existed, or whose command matched no adapter (generic).
    #[serde(default)]
    pub harness: Option<String>,
    /// The harness's own session/thread id (e.g. Claude Code's `--session-id`),
    /// used to resume the same conversation instead of starting a fresh one.
    #[serde(default)]
    pub harness_session_id: Option<String>,
    /// When the harness last reported a lifecycle event via `pulpo hook`. Presence
    /// tells the watchdog that event-driven state transitions own this session, so
    /// the scrollback heuristics (waiting-for-input, rate-limit, error detection,
    /// time-based Active→Idle) are skipped.
    #[serde(default)]
    pub harness_last_event_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Default for Session {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::nil(),
            name: String::new(),
            workdir: String::new(),
            command: String::new(),
            description: None,
            status: SessionStatus::default(),
            status_reason: None,
            exit_code: None,
            backend_session_id: None,
            output_snapshot: None,
            metadata: None,
            ink: None,
            intervention_code: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            idle_threshold_secs: None,
            worktree_path: None,
            worktree_branch: None,
            git_branch: None,
            git_commit: None,
            git_files_changed: None,
            git_insertions: None,
            git_deletions: None,
            git_ahead: None,
            runtime: Runtime::default(),
            harness: None,
            harness_session_id: None,
            harness_last_event_at: None,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Well-known metadata keys used across the codebase.
pub mod meta {
    // Token and cost tracking
    pub const TOTAL_INPUT_TOKENS: &str = "total_input_tokens";
    pub const TOTAL_OUTPUT_TOKENS: &str = "total_output_tokens";
    pub const CACHE_WRITE_TOKENS: &str = "cache_write_tokens";
    pub const CACHE_READ_TOKENS: &str = "cache_read_tokens";
    pub const SESSION_COST_USD: &str = "session_cost_usd";
    // Git and PR detection
    pub const PR_URL: &str = "pr_url";
    pub const BRANCH: &str = "branch";
    // Error and rate limit tracking
    pub const ERROR_STATUS: &str = "error_status";
    pub const ERROR_STATUS_AT: &str = "error_status_at";
    pub const RATE_LIMIT: &str = "rate_limit";
    pub const RATE_LIMIT_AT: &str = "rate_limit_at";
    // Structured usage tracking (read from the agent's own session files).
    // Absent USAGE_SOURCE means token/cost values were scraped from terminal output.
    pub const USAGE_SOURCE: &str = "usage_source";
    // Subscription quota snapshot (Codex writes exact rate-limit data to its session files).
    // "Primary" is the short rolling window (e.g. 5h), "secondary" the long one (e.g. weekly).
    pub const QUOTA_PRIMARY_USED_PERCENT: &str = "quota_primary_used_percent";
    pub const QUOTA_PRIMARY_WINDOW_MINUTES: &str = "quota_primary_window_minutes";
    pub const QUOTA_PRIMARY_RESETS_AT: &str = "quota_primary_resets_at";
    pub const QUOTA_SECONDARY_USED_PERCENT: &str = "quota_secondary_used_percent";
    pub const QUOTA_SECONDARY_WINDOW_MINUTES: &str = "quota_secondary_window_minutes";
    pub const QUOTA_SECONDARY_RESETS_AT: &str = "quota_secondary_resets_at";
    pub const QUOTA_PLAN: &str = "quota_plan";
    // Cost budget (resolved at spawn: explicit spawn/schedule flag). Watchdog alerts at 80%, stops at 100%.
    pub const BUDGET_COST_USD: &str = "budget_cost_usd";
    pub const BUDGET_ALERTED_AT: &str = "budget_alerted_at";
    // Harness adapter events (see `pulpod::harness`).
    // Set on a `NeedsInput` event; distinguishes "blocked on me" from plain idle.
    // Cleared on `SessionStarted`/`Working`.
    pub const NEEDS_INPUT: &str = "needs_input";
    // Set on `TurnFinished`, truncated to 200 chars — a short summary of what the
    // agent just did, for display alongside the idle status.
    pub const LAST_SUMMARY: &str = "last_summary";
}

impl Session {
    /// Get a metadata value as a string slice.
    pub fn meta_str(&self, key: &str) -> Option<&str> {
        self.metadata.as_ref()?.get(key).map(String::as_str)
    }

    /// Get a metadata value, parsed into a target type.
    pub fn meta_parsed<T: std::str::FromStr>(&self, key: &str) -> Option<T> {
        self.meta_str(key)?.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_session() -> Session {
        Session {
            id: Uuid::new_v4(),
            name: "test-session".into(),
            workdir: "/tmp/repo".into(),
            command: "claude -p 'Fix the bug'".into(),
            description: Some("Fix the bug".into()),
            status: SessionStatus::Working,
            backend_session_id: Some("test-session".into()),
            output_snapshot: Some("some output".into()),
            ..Default::default()
        }
    }

    #[test]
    fn test_session_status_serialize() {
        assert_eq!(
            serde_json::to_string(&SessionStatus::Starting).unwrap(),
            "\"starting\""
        );
        assert_eq!(
            serde_json::to_string(&SessionStatus::Working).unwrap(),
            "\"working\""
        );
        assert_eq!(
            serde_json::to_string(&SessionStatus::Waiting).unwrap(),
            "\"waiting\""
        );
        assert_eq!(
            serde_json::to_string(&SessionStatus::Done).unwrap(),
            "\"done\""
        );
        assert_eq!(
            serde_json::to_string(&SessionStatus::Lost).unwrap(),
            "\"lost\""
        );
    }

    #[test]
    fn test_session_status_deserialize() {
        assert_eq!(
            serde_json::from_str::<SessionStatus>("\"starting\"").unwrap(),
            SessionStatus::Starting
        );
        assert_eq!(
            serde_json::from_str::<SessionStatus>("\"working\"").unwrap(),
            SessionStatus::Working
        );
        assert_eq!(
            serde_json::from_str::<SessionStatus>("\"waiting\"").unwrap(),
            SessionStatus::Waiting
        );
        assert_eq!(
            serde_json::from_str::<SessionStatus>("\"done\"").unwrap(),
            SessionStatus::Done
        );
        assert_eq!(
            serde_json::from_str::<SessionStatus>("\"lost\"").unwrap(),
            SessionStatus::Lost
        );
    }

    #[test]
    fn test_session_status_deserialize_old_names_alias_to_new_states() {
        // Old JSON payloads / DB text (pre-ADR-0009) must keep deserializing.
        for (old, new) in [
            ("creating", SessionStatus::Starting),
            ("active", SessionStatus::Working),
            ("idle", SessionStatus::Waiting),
            ("ready", SessionStatus::Done),
            ("stopped", SessionStatus::Done),
            ("killed", SessionStatus::Done),
        ] {
            assert_eq!(
                serde_json::from_str::<SessionStatus>(&format!("\"{old}\"")).unwrap(),
                new,
                "alias {old}"
            );
        }
    }

    #[test]
    fn test_session_status_invalid_deserialize() {
        assert!(serde_json::from_str::<SessionStatus>("\"invalid\"").is_err());
    }

    #[test]
    fn test_session_status_display() {
        assert_eq!(SessionStatus::Starting.to_string(), "starting");
        assert_eq!(SessionStatus::Working.to_string(), "working");
        assert_eq!(SessionStatus::Waiting.to_string(), "waiting");
        assert_eq!(SessionStatus::Done.to_string(), "done");
        assert_eq!(SessionStatus::Lost.to_string(), "lost");
    }

    #[test]
    fn test_session_status_from_str() {
        assert_eq!(
            "starting".parse::<SessionStatus>().unwrap(),
            SessionStatus::Starting
        );
        assert_eq!(
            "working".parse::<SessionStatus>().unwrap(),
            SessionStatus::Working
        );
        assert_eq!(
            "waiting".parse::<SessionStatus>().unwrap(),
            SessionStatus::Waiting
        );
        assert_eq!(
            "done".parse::<SessionStatus>().unwrap(),
            SessionStatus::Done
        );
        assert_eq!(
            "lost".parse::<SessionStatus>().unwrap(),
            SessionStatus::Lost
        );
    }

    #[test]
    fn test_session_status_from_str_old_names_alias_to_new_states() {
        // Old DB text (pre-migration, or a downgrade) must keep parsing via `FromStr`
        // too — `store::rows::row_to_session` uses this, not serde.
        for (old, new) in [
            ("creating", SessionStatus::Starting),
            ("active", SessionStatus::Working),
            ("idle", SessionStatus::Waiting),
            ("ready", SessionStatus::Done),
            ("stopped", SessionStatus::Done),
            ("killed", SessionStatus::Done),
        ] {
            assert_eq!(old.parse::<SessionStatus>().unwrap(), new, "alias {old}");
        }
    }

    #[test]
    fn test_session_status_from_str_invalid() {
        let err = "invalid".parse::<SessionStatus>().unwrap_err();
        assert!(err.contains("unknown session status"));
    }

    #[test]
    fn test_session_status_default_is_starting() {
        assert_eq!(SessionStatus::default(), SessionStatus::Starting);
    }

    // -- status_reason helpers --

    #[test]
    fn test_status_reason_needs_input_builds_prefixed_string() {
        assert_eq!(
            status_reason::needs_input("permission"),
            "needs_input:permission"
        );
    }

    #[test]
    fn test_status_reason_needs_input_reason_splits_prefix() {
        assert_eq!(
            status_reason::needs_input_reason("needs_input:permission"),
            Some("permission")
        );
        assert_eq!(
            status_reason::needs_input_reason("needs_input:custom label"),
            Some("custom label")
        );
    }

    #[test]
    fn test_status_reason_needs_input_reason_none_for_non_needs_input() {
        assert_eq!(status_reason::needs_input_reason("idle"), None);
        assert_eq!(status_reason::needs_input_reason("exited"), None);
        assert_eq!(status_reason::needs_input_reason(""), None);
    }

    #[test]
    fn test_status_reason_constants() {
        assert_eq!(status_reason::IDLE, "idle");
        assert_eq!(status_reason::EXITED, "exited");
        assert_eq!(status_reason::STOPPED, "stopped");
        assert_eq!(status_reason::IDLE_TIMEOUT, "idle_timeout");
        assert_eq!(status_reason::BUDGET_EXCEEDED, "budget_exceeded");
        assert_eq!(status_reason::MEMORY_PRESSURE, "memory_pressure");
    }

    #[test]
    fn test_session_status_reason_roundtrip() {
        let mut session = make_session();
        session.status = SessionStatus::Done;
        session.status_reason = Some(status_reason::EXITED.to_owned());
        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"status_reason\":\"exited\""));
        let deserialized: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.status_reason.as_deref(), Some("exited"));
    }

    #[test]
    fn test_session_status_reason_defaults_to_none_when_absent() {
        let json = r#"{"id":"00000000-0000-0000-0000-000000000000","name":"test","workdir":"/tmp","command":"echo","status":"working","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}"#;
        let session: Session = serde_json::from_str(json).unwrap();
        assert_eq!(session.status_reason, None);
    }

    #[test]
    fn test_session_serialize_roundtrip() {
        let session = make_session();
        let json = serde_json::to_string(&session).unwrap();
        let deserialized: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, session.id);
        assert_eq!(deserialized.name, session.name);
        assert_eq!(deserialized.command, session.command);
        assert_eq!(deserialized.status, session.status);
        assert_eq!(deserialized.description, session.description);
    }

    #[test]
    fn test_session_with_all_none_optionals() {
        let session = Session {
            id: Uuid::new_v4(),
            name: "minimal".into(),
            workdir: "/tmp".into(),
            command: "echo hello".into(),
            description: None,
            status: SessionStatus::Starting,
            ..Default::default()
        };

        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"description\":null"));
        assert!(json.contains("\"echo hello\""));
    }

    #[test]
    fn test_session_status_clone_and_copy() {
        let s = SessionStatus::Working;
        let s2 = s;
        #[allow(clippy::clone_on_copy)]
        let s3 = s.clone();
        assert_eq!(s, s2);
        assert_eq!(s, s3);
    }

    #[test]
    fn test_session_status_debug() {
        assert_eq!(format!("{:?}", SessionStatus::Working), "Working");
    }

    #[test]
    fn test_session_debug() {
        let session = Session {
            id: Uuid::nil(),
            name: "test".into(),
            workdir: "/tmp".into(),
            command: "echo test".into(),
            description: None,
            status: SessionStatus::Working,
            ..Default::default()
        };
        let debug = format!("{session:?}");
        assert!(debug.contains("test"));
    }

    #[test]
    fn test_session_clone() {
        let session = Session {
            id: Uuid::new_v4(),
            name: "clone-test".into(),
            workdir: "/tmp".into(),
            command: "echo test".into(),
            status: SessionStatus::Done,
            exit_code: Some(0),
            ..Default::default()
        };
        let cloned = session.clone();
        assert_eq!(cloned.id, session.id);
        assert_eq!(cloned.exit_code, Some(0));
    }

    #[test]
    fn test_session_with_metadata() {
        let mut meta = HashMap::new();
        meta.insert("discord_channel".into(), "123".into());
        let session = Session {
            id: Uuid::new_v4(),
            name: "full".into(),
            workdir: "/tmp".into(),
            command: "claude -p 'test'".into(),
            description: Some("Testing".into()),
            status: SessionStatus::Working,
            metadata: Some(meta),
            ink: Some("coder".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&session).unwrap();
        let deserialized: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(
            deserialized
                .metadata
                .as_ref()
                .unwrap()
                .get("discord_channel"),
            Some(&"123".into())
        );
        assert_eq!(deserialized.ink, Some("coder".into()));
    }

    #[test]
    fn test_intervention_code_serialize() {
        assert_eq!(
            serde_json::to_string(&InterventionCode::MemoryPressure).unwrap(),
            "\"memory_pressure\""
        );
        assert_eq!(
            serde_json::to_string(&InterventionCode::IdleTimeout).unwrap(),
            "\"idle_timeout\""
        );
        assert_eq!(
            serde_json::to_string(&InterventionCode::UserStop).unwrap(),
            "\"user_stop\""
        );
        assert_eq!(
            serde_json::to_string(&InterventionCode::BudgetExceeded).unwrap(),
            "\"budget_exceeded\""
        );
    }

    #[test]
    fn test_intervention_code_deserialize() {
        assert_eq!(
            serde_json::from_str::<InterventionCode>("\"memory_pressure\"").unwrap(),
            InterventionCode::MemoryPressure
        );
        assert_eq!(
            serde_json::from_str::<InterventionCode>("\"idle_timeout\"").unwrap(),
            InterventionCode::IdleTimeout
        );
        assert_eq!(
            serde_json::from_str::<InterventionCode>("\"user_stop\"").unwrap(),
            InterventionCode::UserStop
        );
    }

    #[test]
    fn test_intervention_code_invalid_deserialize() {
        assert!(serde_json::from_str::<InterventionCode>("\"invalid\"").is_err());
    }

    #[test]
    fn test_intervention_code_display() {
        assert_eq!(
            InterventionCode::MemoryPressure.to_string(),
            "memory_pressure"
        );
        assert_eq!(InterventionCode::IdleTimeout.to_string(), "idle_timeout");
        assert_eq!(InterventionCode::UserStop.to_string(), "user_stop");
        assert_eq!(
            InterventionCode::BudgetExceeded.to_string(),
            "budget_exceeded"
        );
    }

    #[test]
    fn test_intervention_code_from_str() {
        assert_eq!(
            "memory_pressure".parse::<InterventionCode>().unwrap(),
            InterventionCode::MemoryPressure
        );
        assert_eq!(
            "idle_timeout".parse::<InterventionCode>().unwrap(),
            InterventionCode::IdleTimeout
        );
        assert_eq!(
            "user_stop".parse::<InterventionCode>().unwrap(),
            InterventionCode::UserStop
        );
        assert_eq!(
            "budget_exceeded".parse::<InterventionCode>().unwrap(),
            InterventionCode::BudgetExceeded
        );
    }

    #[test]
    fn test_intervention_code_from_str_retired_burn_rate_is_unknown() {
        // The burn-velocity governor (and its `BurnRate` intervention code) was
        // removed; the string is no longer recognized. Historical DB rows storing
        // this text are tolerated at the store layer (see `store::rows`), not here.
        let err = "burn_rate".parse::<InterventionCode>().unwrap_err();
        assert!(err.contains("unknown intervention code"));
    }

    #[test]
    fn test_intervention_code_from_str_invalid() {
        let err = "invalid".parse::<InterventionCode>().unwrap_err();
        assert!(err.contains("unknown intervention code"));
    }

    #[test]
    fn test_intervention_code_clone_and_copy() {
        let c = InterventionCode::MemoryPressure;
        let c2 = c;
        #[allow(clippy::clone_on_copy)]
        let c3 = c.clone();
        assert_eq!(c, c2);
        assert_eq!(c, c3);
    }

    #[test]
    fn test_intervention_code_debug() {
        assert_eq!(
            format!("{:?}", InterventionCode::MemoryPressure),
            "MemoryPressure"
        );
        assert_eq!(
            format!("{:?}", InterventionCode::IdleTimeout),
            "IdleTimeout"
        );
        assert_eq!(format!("{:?}", InterventionCode::UserStop), "UserStop");
    }

    #[test]
    fn test_session_with_intervention_code() {
        let mut session = make_session();
        session.intervention_code = Some(InterventionCode::MemoryPressure);
        session.intervention_reason = Some("Memory 95%".into());
        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"memory_pressure\""));
        let deserialized: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(
            deserialized.intervention_code,
            Some(InterventionCode::MemoryPressure)
        );
    }

    // -- Runtime enum tests --

    #[test]
    fn test_runtime_default() {
        assert_eq!(Runtime::default(), Runtime::Tmux);
    }

    #[test]
    fn test_runtime_display() {
        assert_eq!(Runtime::Tmux.to_string(), "tmux");
        assert_eq!(Runtime::Docker.to_string(), "docker");
    }

    #[test]
    fn test_runtime_from_str() {
        assert_eq!("tmux".parse::<Runtime>().unwrap(), Runtime::Tmux);
        assert_eq!("docker".parse::<Runtime>().unwrap(), Runtime::Docker);
    }

    #[test]
    fn test_runtime_from_str_invalid() {
        let err = "invalid".parse::<Runtime>().unwrap_err();
        assert!(err.contains("unknown runtime"));
    }

    #[test]
    fn test_runtime_serialize() {
        assert_eq!(serde_json::to_string(&Runtime::Tmux).unwrap(), "\"tmux\"");
        assert_eq!(
            serde_json::to_string(&Runtime::Docker).unwrap(),
            "\"docker\""
        );
    }

    #[test]
    fn test_runtime_deserialize() {
        assert_eq!(
            serde_json::from_str::<Runtime>("\"tmux\"").unwrap(),
            Runtime::Tmux
        );
        assert_eq!(
            serde_json::from_str::<Runtime>("\"docker\"").unwrap(),
            Runtime::Docker
        );
    }

    #[test]
    fn test_runtime_invalid_deserialize() {
        assert!(serde_json::from_str::<Runtime>("\"invalid\"").is_err());
    }

    #[test]
    fn test_runtime_clone_and_copy() {
        let r = Runtime::Tmux;
        let r2 = r;
        #[allow(clippy::clone_on_copy)]
        let r3 = r.clone();
        assert_eq!(r, r2);
        assert_eq!(r, r3);
    }

    #[test]
    fn test_runtime_debug() {
        assert_eq!(format!("{:?}", Runtime::Tmux), "Tmux");
        assert_eq!(format!("{:?}", Runtime::Docker), "Docker");
    }

    #[test]
    fn test_session_with_docker_runtime() {
        let mut session = make_session();
        session.runtime = Runtime::Docker;
        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"runtime\":\"docker\""));
        let deserialized: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.runtime, Runtime::Docker);
    }

    #[test]
    fn test_session_with_worktree_branch() {
        let mut session = make_session();
        session.worktree_path = Some("/home/user/.pulpo/worktrees/fix-auth".into());
        session.worktree_branch = Some("fix-auth".into());
        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"worktree_branch\":\"fix-auth\""));
        let deserialized: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.worktree_branch, Some("fix-auth".into()));
    }

    #[test]
    fn test_session_harness_fields_roundtrip() {
        let mut session = make_session();
        session.harness = Some("claude".into());
        session.harness_session_id = Some("sid-123".into());
        session.harness_last_event_at = Some(Utc::now());
        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"harness\":\"claude\""));
        assert!(json.contains("\"harness_session_id\":\"sid-123\""));
        let deserialized: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.harness, session.harness);
        assert_eq!(deserialized.harness_session_id, session.harness_session_id);
        assert_eq!(
            deserialized.harness_last_event_at,
            session.harness_last_event_at
        );
    }

    #[test]
    fn test_session_harness_fields_default_on_deserialize_when_missing() {
        // Older wire payloads (pre-harness-adapters) omit these fields entirely.
        let json = r#"{"id":"00000000-0000-0000-0000-000000000000","name":"test","workdir":"/tmp","command":"echo","status":"active","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}"#;
        let session: Session = serde_json::from_str(json).unwrap();
        assert_eq!(session.harness, None);
        assert_eq!(session.harness_session_id, None);
        assert_eq!(session.harness_last_event_at, None);
    }

    #[test]
    fn test_session_runtime_default_on_deserialize() {
        // When runtime field is missing, it should default to Tmux
        let json = r#"{"id":"00000000-0000-0000-0000-000000000000","name":"test","workdir":"/tmp","command":"echo","status":"active","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}"#;
        let session: Session = serde_json::from_str(json).unwrap();
        assert_eq!(session.runtime, Runtime::Tmux);
    }
}
