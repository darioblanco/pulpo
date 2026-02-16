use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Claude,
    Codex,
    Aider,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Creating,
    Running,
    Completed,
    Dead,
    Stale,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub name: String,
    pub repo_path: String,
    pub provider: Provider,
    pub prompt: String,
    pub status: SessionStatus,
    pub conversation_id: Option<String>,
    pub exit_code: Option<i32>,
    pub tmux_session: Option<String>,
    pub docker_container: Option<String>,
    pub output_snapshot: Option<String>,
    pub git_branch: Option<String>,
    pub git_sha: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
