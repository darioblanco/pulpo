use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use std::collections::HashMap;

/// The runtime environment for a session.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Runtime {
    /// Native tmux session (default).
    #[default]
    Tmux,
    /// Docker container session.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InterventionCode {
    /// Stopped due to system memory pressure exceeding threshold.
    MemoryPressure,
    /// Stopped due to session idle timeout.
    IdleTimeout,
    /// Manually stopped by user via API/CLI.
    #[serde(alias = "user_kill")]
    UserStop,
}

impl fmt::Display for InterventionCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MemoryPressure => write!(f, "memory_pressure"),
            Self::IdleTimeout => write!(f, "idle_timeout"),
            Self::UserStop => write!(f, "user_stop"),
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
            other => Err(format!("unknown intervention code: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Creating,
    Active,
    Idle,
    Ready,
    #[serde(alias = "killed")]
    Stopped,
    Lost,
}

impl fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Creating => write!(f, "creating"),
            Self::Active => write!(f, "active"),
            Self::Idle => write!(f, "idle"),
            Self::Ready => write!(f, "ready"),
            Self::Stopped => write!(f, "stopped"),
            Self::Lost => write!(f, "lost"),
        }
    }
}

impl FromStr for SessionStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "creating" => Ok(Self::Creating),
            "active" => Ok(Self::Active),
            "idle" => Ok(Self::Idle),
            "ready" => Ok(Self::Ready),
            "stopped" | "killed" => Ok(Self::Stopped),
            "lost" => Ok(Self::Lost),
            other => Err(format!("unknown session status: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Session {
    pub id: Uuid,
    pub name: String,
    pub workdir: String,
    pub command: String,
    pub description: Option<String>,
    pub status: SessionStatus,
    pub exit_code: Option<i32>,
    pub backend_session_id: Option<String>,
    pub output_snapshot: Option<String>,
    pub metadata: Option<HashMap<String, String>>,
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
    /// The runtime environment for this session (tmux or docker).
    #[serde(default)]
    pub runtime: Runtime,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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
    use chrono::Utc;

    fn make_session() -> Session {
        Session {
            id: Uuid::new_v4(),
            name: "test-session".into(),
            workdir: "/tmp/repo".into(),
            command: "claude -p 'Fix the bug'".into(),
            description: Some("Fix the bug".into()),
            status: SessionStatus::Active,
            exit_code: None,
            backend_session_id: Some("test-session".into()),
            output_snapshot: Some("some output".into()),
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
            runtime: Runtime::Tmux,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_session_status_serialize() {
        assert_eq!(
            serde_json::to_string(&SessionStatus::Creating).unwrap(),
            "\"creating\""
        );
        assert_eq!(
            serde_json::to_string(&SessionStatus::Active).unwrap(),
            "\"active\""
        );
        assert_eq!(
            serde_json::to_string(&SessionStatus::Idle).unwrap(),
            "\"idle\""
        );
        assert_eq!(
            serde_json::to_string(&SessionStatus::Ready).unwrap(),
            "\"ready\""
        );
        assert_eq!(
            serde_json::to_string(&SessionStatus::Stopped).unwrap(),
            "\"stopped\""
        );
        assert_eq!(
            serde_json::to_string(&SessionStatus::Lost).unwrap(),
            "\"lost\""
        );
    }

    #[test]
    fn test_session_status_deserialize() {
        assert_eq!(
            serde_json::from_str::<SessionStatus>("\"creating\"").unwrap(),
            SessionStatus::Creating
        );
        assert_eq!(
            serde_json::from_str::<SessionStatus>("\"active\"").unwrap(),
            SessionStatus::Active
        );
        assert_eq!(
            serde_json::from_str::<SessionStatus>("\"idle\"").unwrap(),
            SessionStatus::Idle
        );
        assert_eq!(
            serde_json::from_str::<SessionStatus>("\"ready\"").unwrap(),
            SessionStatus::Ready
        );
        assert_eq!(
            serde_json::from_str::<SessionStatus>("\"stopped\"").unwrap(),
            SessionStatus::Stopped
        );
        assert_eq!(
            serde_json::from_str::<SessionStatus>("\"lost\"").unwrap(),
            SessionStatus::Lost
        );
    }

    #[test]
    fn test_session_status_invalid_deserialize() {
        assert!(serde_json::from_str::<SessionStatus>("\"invalid\"").is_err());
    }

    #[test]
    fn test_session_status_display() {
        assert_eq!(SessionStatus::Creating.to_string(), "creating");
        assert_eq!(SessionStatus::Active.to_string(), "active");
        assert_eq!(SessionStatus::Idle.to_string(), "idle");
        assert_eq!(SessionStatus::Ready.to_string(), "ready");
        assert_eq!(SessionStatus::Stopped.to_string(), "stopped");
        assert_eq!(SessionStatus::Lost.to_string(), "lost");
    }

    #[test]
    fn test_session_status_from_str() {
        assert_eq!(
            "creating".parse::<SessionStatus>().unwrap(),
            SessionStatus::Creating
        );
        assert_eq!(
            "active".parse::<SessionStatus>().unwrap(),
            SessionStatus::Active
        );
        assert_eq!(
            "idle".parse::<SessionStatus>().unwrap(),
            SessionStatus::Idle
        );
        assert_eq!(
            "ready".parse::<SessionStatus>().unwrap(),
            SessionStatus::Ready
        );
        assert_eq!(
            "stopped".parse::<SessionStatus>().unwrap(),
            SessionStatus::Stopped
        );
        assert_eq!(
            "lost".parse::<SessionStatus>().unwrap(),
            SessionStatus::Lost
        );
    }

    #[test]
    fn test_session_status_from_str_invalid() {
        let err = "invalid".parse::<SessionStatus>().unwrap_err();
        assert!(err.contains("unknown session status"));
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
            status: SessionStatus::Creating,
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
            runtime: Runtime::Tmux,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"description\":null"));
        assert!(json.contains("\"echo hello\""));
    }

    #[test]
    fn test_session_status_clone_and_copy() {
        let s = SessionStatus::Active;
        let s2 = s;
        #[allow(clippy::clone_on_copy)]
        let s3 = s.clone();
        assert_eq!(s, s2);
        assert_eq!(s, s3);
    }

    #[test]
    fn test_session_status_debug() {
        assert_eq!(format!("{:?}", SessionStatus::Active), "Active");
    }

    #[test]
    fn test_session_debug() {
        let session = Session {
            id: Uuid::nil(),
            name: "test".into(),
            workdir: "/tmp".into(),
            command: "echo test".into(),
            description: None,
            status: SessionStatus::Active,
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
            runtime: Runtime::Tmux,
            created_at: Utc::now(),
            updated_at: Utc::now(),
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
            description: None,
            status: SessionStatus::Ready,
            exit_code: Some(0),
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
            runtime: Runtime::Tmux,
            created_at: Utc::now(),
            updated_at: Utc::now(),
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
            status: SessionStatus::Active,
            exit_code: None,
            backend_session_id: None,
            output_snapshot: None,
            metadata: Some(meta),
            ink: Some("coder".into()),
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
            runtime: Runtime::Tmux,
            created_at: Utc::now(),
            updated_at: Utc::now(),
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
    fn test_session_runtime_default_on_deserialize() {
        // When runtime field is missing, it should default to Tmux
        let json = r#"{"id":"00000000-0000-0000-0000-000000000000","name":"test","workdir":"/tmp","command":"echo","status":"active","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}"#;
        let session: Session = serde_json::from_str(json).unwrap();
        assert_eq!(session.runtime, Runtime::Tmux);
    }
}
