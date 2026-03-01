use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::guard::GuardConfig;
use crate::session::{Provider, SessionMode};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConcurrencyPolicy {
    #[default]
    Skip,
    Allow,
    Replace,
}

impl fmt::Display for ConcurrencyPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Skip => write!(f, "skip"),
            Self::Allow => write!(f, "allow"),
            Self::Replace => write!(f, "replace"),
        }
    }
}

impl FromStr for ConcurrencyPolicy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "skip" => Ok(Self::Skip),
            "allow" => Ok(Self::Allow),
            "replace" => Ok(Self::Replace),
            other => Err(format!("unknown concurrency policy: {other}")),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleStatus {
    #[default]
    Active,
    Paused,
    Exhausted,
}

impl fmt::Display for ScheduleStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Paused => write!(f, "paused"),
            Self::Exhausted => write!(f, "exhausted"),
        }
    }
}

impl FromStr for ScheduleStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(Self::Active),
            "paused" => Ok(Self::Paused),
            "exhausted" => Ok(Self::Exhausted),
            other => Err(format!("unknown schedule status: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Spawned,
    Skipped,
    Failed,
}

impl fmt::Display for ExecutionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawned => write!(f, "spawned"),
            Self::Skipped => write!(f, "skipped"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

impl FromStr for ExecutionStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "spawned" => Ok(Self::Spawned),
            "skipped" => Ok(Self::Skipped),
            "failed" => Ok(Self::Failed),
            other => Err(format!("unknown execution status: {other}")),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeCleanup {
    #[default]
    OnComplete,
    Keep,
}

impl fmt::Display for WorktreeCleanup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OnComplete => write!(f, "on_complete"),
            Self::Keep => write!(f, "keep"),
        }
    }
}

impl FromStr for WorktreeCleanup {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "on_complete" => Ok(Self::OnComplete),
            "keep" => Ok(Self::Keep),
            other => Err(format!("unknown worktree cleanup: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorktreeConfig {
    pub branch: Option<String>,
    pub new_branch: Option<String>,
    pub cleanup: WorktreeCleanup,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Schedule {
    pub id: Uuid,
    pub name: String,
    pub cron: String,
    pub workdir: String,
    pub prompt: String,
    pub provider: Provider,
    pub mode: SessionMode,
    pub guard_preset: Option<String>,
    pub guard_config: Option<GuardConfig>,
    pub model: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub system_prompt: Option<String>,
    pub metadata: Option<HashMap<String, String>>,
    pub persona: Option<String>,
    pub max_turns: Option<u32>,
    pub max_budget_usd: Option<f64>,
    pub output_format: Option<String>,
    pub concurrency: ConcurrencyPolicy,
    pub status: ScheduleStatus,
    pub max_executions: Option<u32>,
    pub execution_count: u32,
    pub last_run_at: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub last_session_id: Option<Uuid>,
    pub worktree: Option<WorktreeConfig>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScheduleExecution {
    pub id: i64,
    pub schedule_id: Uuid,
    pub session_id: Option<Uuid>,
    pub status: ExecutionStatus,
    pub error: Option<String>,
    pub triggered_by: String,
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- ConcurrencyPolicy tests ---

    #[test]
    fn test_concurrency_policy_serialize() {
        assert_eq!(
            serde_json::to_string(&ConcurrencyPolicy::Skip).unwrap(),
            "\"skip\""
        );
        assert_eq!(
            serde_json::to_string(&ConcurrencyPolicy::Allow).unwrap(),
            "\"allow\""
        );
        assert_eq!(
            serde_json::to_string(&ConcurrencyPolicy::Replace).unwrap(),
            "\"replace\""
        );
    }

    #[test]
    fn test_concurrency_policy_deserialize() {
        assert_eq!(
            serde_json::from_str::<ConcurrencyPolicy>("\"skip\"").unwrap(),
            ConcurrencyPolicy::Skip
        );
        assert_eq!(
            serde_json::from_str::<ConcurrencyPolicy>("\"allow\"").unwrap(),
            ConcurrencyPolicy::Allow
        );
        assert_eq!(
            serde_json::from_str::<ConcurrencyPolicy>("\"replace\"").unwrap(),
            ConcurrencyPolicy::Replace
        );
    }

    #[test]
    fn test_concurrency_policy_invalid_deserialize() {
        assert!(serde_json::from_str::<ConcurrencyPolicy>("\"invalid\"").is_err());
    }

    #[test]
    fn test_concurrency_policy_display() {
        assert_eq!(ConcurrencyPolicy::Skip.to_string(), "skip");
        assert_eq!(ConcurrencyPolicy::Allow.to_string(), "allow");
        assert_eq!(ConcurrencyPolicy::Replace.to_string(), "replace");
    }

    #[test]
    fn test_concurrency_policy_from_str() {
        assert_eq!(
            "skip".parse::<ConcurrencyPolicy>().unwrap(),
            ConcurrencyPolicy::Skip
        );
        assert_eq!(
            "allow".parse::<ConcurrencyPolicy>().unwrap(),
            ConcurrencyPolicy::Allow
        );
        assert_eq!(
            "replace".parse::<ConcurrencyPolicy>().unwrap(),
            ConcurrencyPolicy::Replace
        );
    }

    #[test]
    fn test_concurrency_policy_from_str_invalid() {
        let err = "invalid".parse::<ConcurrencyPolicy>().unwrap_err();
        assert!(err.contains("unknown concurrency policy"));
    }

    #[test]
    fn test_concurrency_policy_default() {
        assert_eq!(ConcurrencyPolicy::default(), ConcurrencyPolicy::Skip);
    }

    #[test]
    fn test_concurrency_policy_clone_and_copy() {
        let c = ConcurrencyPolicy::Allow;
        let c2 = c;
        #[allow(clippy::clone_on_copy)]
        let c3 = c.clone();
        assert_eq!(c, c2);
        assert_eq!(c, c3);
    }

    #[test]
    fn test_concurrency_policy_debug() {
        assert_eq!(format!("{:?}", ConcurrencyPolicy::Skip), "Skip");
        assert_eq!(format!("{:?}", ConcurrencyPolicy::Allow), "Allow");
        assert_eq!(format!("{:?}", ConcurrencyPolicy::Replace), "Replace");
    }

    // --- ScheduleStatus tests ---

    #[test]
    fn test_schedule_status_serialize() {
        assert_eq!(
            serde_json::to_string(&ScheduleStatus::Active).unwrap(),
            "\"active\""
        );
        assert_eq!(
            serde_json::to_string(&ScheduleStatus::Paused).unwrap(),
            "\"paused\""
        );
        assert_eq!(
            serde_json::to_string(&ScheduleStatus::Exhausted).unwrap(),
            "\"exhausted\""
        );
    }

    #[test]
    fn test_schedule_status_deserialize() {
        assert_eq!(
            serde_json::from_str::<ScheduleStatus>("\"active\"").unwrap(),
            ScheduleStatus::Active
        );
        assert_eq!(
            serde_json::from_str::<ScheduleStatus>("\"paused\"").unwrap(),
            ScheduleStatus::Paused
        );
        assert_eq!(
            serde_json::from_str::<ScheduleStatus>("\"exhausted\"").unwrap(),
            ScheduleStatus::Exhausted
        );
    }

    #[test]
    fn test_schedule_status_invalid_deserialize() {
        assert!(serde_json::from_str::<ScheduleStatus>("\"invalid\"").is_err());
    }

    #[test]
    fn test_schedule_status_display() {
        assert_eq!(ScheduleStatus::Active.to_string(), "active");
        assert_eq!(ScheduleStatus::Paused.to_string(), "paused");
        assert_eq!(ScheduleStatus::Exhausted.to_string(), "exhausted");
    }

    #[test]
    fn test_schedule_status_from_str() {
        assert_eq!(
            "active".parse::<ScheduleStatus>().unwrap(),
            ScheduleStatus::Active
        );
        assert_eq!(
            "paused".parse::<ScheduleStatus>().unwrap(),
            ScheduleStatus::Paused
        );
        assert_eq!(
            "exhausted".parse::<ScheduleStatus>().unwrap(),
            ScheduleStatus::Exhausted
        );
    }

    #[test]
    fn test_schedule_status_from_str_invalid() {
        let err = "invalid".parse::<ScheduleStatus>().unwrap_err();
        assert!(err.contains("unknown schedule status"));
    }

    #[test]
    fn test_schedule_status_default() {
        assert_eq!(ScheduleStatus::default(), ScheduleStatus::Active);
    }

    #[test]
    fn test_schedule_status_clone_and_copy() {
        let s = ScheduleStatus::Paused;
        let s2 = s;
        #[allow(clippy::clone_on_copy)]
        let s3 = s.clone();
        assert_eq!(s, s2);
        assert_eq!(s, s3);
    }

    #[test]
    fn test_schedule_status_debug() {
        assert_eq!(format!("{:?}", ScheduleStatus::Active), "Active");
        assert_eq!(format!("{:?}", ScheduleStatus::Paused), "Paused");
        assert_eq!(format!("{:?}", ScheduleStatus::Exhausted), "Exhausted");
    }

    // --- ExecutionStatus tests ---

    #[test]
    fn test_execution_status_serialize() {
        assert_eq!(
            serde_json::to_string(&ExecutionStatus::Spawned).unwrap(),
            "\"spawned\""
        );
        assert_eq!(
            serde_json::to_string(&ExecutionStatus::Skipped).unwrap(),
            "\"skipped\""
        );
        assert_eq!(
            serde_json::to_string(&ExecutionStatus::Failed).unwrap(),
            "\"failed\""
        );
    }

    #[test]
    fn test_execution_status_deserialize() {
        assert_eq!(
            serde_json::from_str::<ExecutionStatus>("\"spawned\"").unwrap(),
            ExecutionStatus::Spawned
        );
        assert_eq!(
            serde_json::from_str::<ExecutionStatus>("\"skipped\"").unwrap(),
            ExecutionStatus::Skipped
        );
        assert_eq!(
            serde_json::from_str::<ExecutionStatus>("\"failed\"").unwrap(),
            ExecutionStatus::Failed
        );
    }

    #[test]
    fn test_execution_status_invalid_deserialize() {
        assert!(serde_json::from_str::<ExecutionStatus>("\"invalid\"").is_err());
    }

    #[test]
    fn test_execution_status_display() {
        assert_eq!(ExecutionStatus::Spawned.to_string(), "spawned");
        assert_eq!(ExecutionStatus::Skipped.to_string(), "skipped");
        assert_eq!(ExecutionStatus::Failed.to_string(), "failed");
    }

    #[test]
    fn test_execution_status_from_str() {
        assert_eq!(
            "spawned".parse::<ExecutionStatus>().unwrap(),
            ExecutionStatus::Spawned
        );
        assert_eq!(
            "skipped".parse::<ExecutionStatus>().unwrap(),
            ExecutionStatus::Skipped
        );
        assert_eq!(
            "failed".parse::<ExecutionStatus>().unwrap(),
            ExecutionStatus::Failed
        );
    }

    #[test]
    fn test_execution_status_from_str_invalid() {
        let err = "invalid".parse::<ExecutionStatus>().unwrap_err();
        assert!(err.contains("unknown execution status"));
    }

    #[test]
    fn test_execution_status_clone_and_copy() {
        let e = ExecutionStatus::Spawned;
        let e2 = e;
        #[allow(clippy::clone_on_copy)]
        let e3 = e.clone();
        assert_eq!(e, e2);
        assert_eq!(e, e3);
    }

    #[test]
    fn test_execution_status_debug() {
        assert_eq!(format!("{:?}", ExecutionStatus::Spawned), "Spawned");
        assert_eq!(format!("{:?}", ExecutionStatus::Skipped), "Skipped");
        assert_eq!(format!("{:?}", ExecutionStatus::Failed), "Failed");
    }

    // --- WorktreeCleanup tests ---

    #[test]
    fn test_worktree_cleanup_serialize() {
        assert_eq!(
            serde_json::to_string(&WorktreeCleanup::OnComplete).unwrap(),
            "\"on_complete\""
        );
        assert_eq!(
            serde_json::to_string(&WorktreeCleanup::Keep).unwrap(),
            "\"keep\""
        );
    }

    #[test]
    fn test_worktree_cleanup_deserialize() {
        assert_eq!(
            serde_json::from_str::<WorktreeCleanup>("\"on_complete\"").unwrap(),
            WorktreeCleanup::OnComplete
        );
        assert_eq!(
            serde_json::from_str::<WorktreeCleanup>("\"keep\"").unwrap(),
            WorktreeCleanup::Keep
        );
    }

    #[test]
    fn test_worktree_cleanup_invalid_deserialize() {
        assert!(serde_json::from_str::<WorktreeCleanup>("\"invalid\"").is_err());
    }

    #[test]
    fn test_worktree_cleanup_display() {
        assert_eq!(WorktreeCleanup::OnComplete.to_string(), "on_complete");
        assert_eq!(WorktreeCleanup::Keep.to_string(), "keep");
    }

    #[test]
    fn test_worktree_cleanup_from_str() {
        assert_eq!(
            "on_complete".parse::<WorktreeCleanup>().unwrap(),
            WorktreeCleanup::OnComplete
        );
        assert_eq!(
            "keep".parse::<WorktreeCleanup>().unwrap(),
            WorktreeCleanup::Keep
        );
    }

    #[test]
    fn test_worktree_cleanup_from_str_invalid() {
        let err = "invalid".parse::<WorktreeCleanup>().unwrap_err();
        assert!(err.contains("unknown worktree cleanup"));
    }

    #[test]
    fn test_worktree_cleanup_default() {
        assert_eq!(WorktreeCleanup::default(), WorktreeCleanup::OnComplete);
    }

    #[test]
    fn test_worktree_cleanup_clone_and_copy() {
        let w = WorktreeCleanup::Keep;
        let w2 = w;
        #[allow(clippy::clone_on_copy)]
        let w3 = w.clone();
        assert_eq!(w, w2);
        assert_eq!(w, w3);
    }

    #[test]
    fn test_worktree_cleanup_debug() {
        assert_eq!(format!("{:?}", WorktreeCleanup::OnComplete), "OnComplete");
        assert_eq!(format!("{:?}", WorktreeCleanup::Keep), "Keep");
    }

    // --- WorktreeConfig tests ---

    #[test]
    fn test_worktree_config_serialize_roundtrip() {
        let config = WorktreeConfig {
            branch: Some("main".into()),
            new_branch: Some("{schedule}-{date}".into()),
            cleanup: WorktreeCleanup::OnComplete,
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: WorktreeConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.branch, Some("main".into()));
        assert_eq!(deserialized.new_branch, Some("{schedule}-{date}".into()));
        assert_eq!(deserialized.cleanup, WorktreeCleanup::OnComplete);
    }

    #[test]
    fn test_worktree_config_with_none_optionals() {
        let config = WorktreeConfig {
            branch: None,
            new_branch: None,
            cleanup: WorktreeCleanup::Keep,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"branch\":null"));
        assert!(json.contains("\"new_branch\":null"));
    }

    #[test]
    fn test_worktree_config_debug_clone() {
        let config = WorktreeConfig {
            branch: Some("main".into()),
            new_branch: None,
            cleanup: WorktreeCleanup::OnComplete,
        };
        let cloned = config.clone();
        assert_eq!(format!("{config:?}"), format!("{cloned:?}"));
    }

    // --- Schedule tests ---

    fn make_schedule() -> Schedule {
        Schedule {
            id: Uuid::new_v4(),
            name: "nightly-review".into(),
            cron: "0 2 * * *".into(),
            workdir: "/tmp/repo".into(),
            prompt: "Review code changes from today".into(),
            provider: Provider::Claude,
            mode: SessionMode::Autonomous,
            guard_preset: Some("standard".into()),
            guard_config: None,
            model: Some("opus".into()),
            allowed_tools: Some(vec!["Read".into(), "Grep".into()]),
            system_prompt: Some("Be thorough".into()),
            metadata: None,
            persona: Some("reviewer".into()),
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            concurrency: ConcurrencyPolicy::Skip,
            status: ScheduleStatus::Active,
            max_executions: Some(100),
            execution_count: 42,
            last_run_at: Some(Utc::now()),
            next_run_at: Some(Utc::now()),
            last_session_id: Some(Uuid::new_v4()),
            worktree: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_schedule_serialize_roundtrip() {
        let schedule = make_schedule();
        let json = serde_json::to_string(&schedule).unwrap();
        let deserialized: Schedule = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, schedule.id);
        assert_eq!(deserialized.name, schedule.name);
        assert_eq!(deserialized.cron, schedule.cron);
        assert_eq!(deserialized.provider, schedule.provider);
        assert_eq!(deserialized.mode, schedule.mode);
        assert_eq!(deserialized.concurrency, schedule.concurrency);
        assert_eq!(deserialized.status, schedule.status);
        assert_eq!(deserialized.execution_count, 42);
    }

    #[test]
    fn test_schedule_with_all_none_optionals() {
        let schedule = Schedule {
            id: Uuid::new_v4(),
            name: "minimal".into(),
            cron: "* * * * *".into(),
            workdir: "/tmp".into(),
            prompt: "test".into(),
            provider: Provider::Codex,
            mode: SessionMode::Interactive,
            guard_preset: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            concurrency: ConcurrencyPolicy::Allow,
            status: ScheduleStatus::Paused,
            max_executions: None,
            execution_count: 0,
            last_run_at: None,
            next_run_at: None,
            last_session_id: None,
            worktree: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let json = serde_json::to_string(&schedule).unwrap();
        assert!(json.contains("\"guard_preset\":null"));
        assert!(json.contains("\"model\":null"));
        assert!(json.contains("\"max_executions\":null"));
    }

    #[test]
    #[allow(clippy::literal_string_with_formatting_args)]
    fn test_schedule_with_worktree() {
        let schedule = Schedule {
            worktree: Some(WorktreeConfig {
                branch: Some("main".into()),
                new_branch: Some("{schedule}-{run}".into()),
                cleanup: WorktreeCleanup::OnComplete,
            }),
            ..make_schedule()
        };
        let json = serde_json::to_string(&schedule).unwrap();
        let deserialized: Schedule = serde_json::from_str(&json).unwrap();
        let wt = deserialized.worktree.unwrap();
        assert_eq!(wt.branch, Some("main".into()));
        assert_eq!(wt.new_branch, Some("{schedule}-{run}".into()));
        assert_eq!(wt.cleanup, WorktreeCleanup::OnComplete);
    }

    #[test]
    fn test_schedule_debug() {
        let schedule = make_schedule();
        let debug = format!("{schedule:?}");
        assert!(debug.contains("nightly-review"));
    }

    #[test]
    fn test_schedule_clone() {
        let schedule = make_schedule();
        let cloned = schedule.clone();
        assert_eq!(cloned.id, schedule.id);
        assert_eq!(cloned.name, schedule.name);
        assert_eq!(cloned.execution_count, schedule.execution_count);
    }

    // --- ScheduleExecution tests ---

    #[test]
    fn test_execution_serialize_roundtrip() {
        let execution = ScheduleExecution {
            id: 1,
            schedule_id: Uuid::new_v4(),
            session_id: Some(Uuid::new_v4()),
            status: ExecutionStatus::Spawned,
            error: None,
            triggered_by: "cron".into(),
            created_at: Utc::now(),
        };
        let json = serde_json::to_string(&execution).unwrap();
        let deserialized: ScheduleExecution = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, 1);
        assert_eq!(deserialized.schedule_id, execution.schedule_id);
        assert_eq!(deserialized.status, ExecutionStatus::Spawned);
        assert_eq!(deserialized.triggered_by, "cron");
    }

    #[test]
    fn test_execution_without_optionals() {
        let execution = ScheduleExecution {
            id: 2,
            schedule_id: Uuid::new_v4(),
            session_id: None,
            status: ExecutionStatus::Skipped,
            error: None,
            triggered_by: "cron".into(),
            created_at: Utc::now(),
        };
        let json = serde_json::to_string(&execution).unwrap();
        assert!(json.contains("\"session_id\":null"));
        assert!(json.contains("\"error\":null"));
    }

    #[test]
    fn test_execution_with_error() {
        let execution = ScheduleExecution {
            id: 3,
            schedule_id: Uuid::new_v4(),
            session_id: None,
            status: ExecutionStatus::Failed,
            error: Some("spawn failed".into()),
            triggered_by: "manual".into(),
            created_at: Utc::now(),
        };
        let json = serde_json::to_string(&execution).unwrap();
        assert!(json.contains("spawn failed"));
        assert!(json.contains("\"manual\""));
    }

    #[test]
    fn test_execution_debug_clone() {
        let execution = ScheduleExecution {
            id: 1,
            schedule_id: Uuid::new_v4(),
            session_id: None,
            status: ExecutionStatus::Spawned,
            error: None,
            triggered_by: "cron".into(),
            created_at: Utc::now(),
        };
        let cloned = execution.clone();
        assert_eq!(format!("{execution:?}"), format!("{cloned:?}"));
    }
}
