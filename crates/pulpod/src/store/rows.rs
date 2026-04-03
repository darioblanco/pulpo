use anyhow::Result;
use chrono::{DateTime, Utc};
use pulpo_common::api::SessionIndexEntry;
use pulpo_common::session::{InterventionCode, Session, SessionStatus};
use sqlx::{Row, sqlite::SqliteRow};
use uuid::Uuid;

use super::{EnrolledNode, InterventionEvent};

pub(super) fn row_to_session(row: &SqliteRow) -> Result<Session> {
    let id_str: String = row.get("id");
    let status_str: String = row.get("status");
    let created_str: String = row.get("created_at");
    let updated_str: String = row.get("updated_at");

    let metadata_json: Option<String> = row.get("metadata");
    let metadata = metadata_json
        .map(|s| serde_json::from_str::<std::collections::HashMap<String, String>>(&s))
        .transpose()?;

    let intervention_code_str: Option<String> = row.get("intervention_code");
    let intervention_code = intervention_code_str
        .map(|s| {
            s.parse::<InterventionCode>()
                .map_err(|e| anyhow::anyhow!(e))
        })
        .transpose()?;

    let intervention_at_str: Option<String> = row.get("intervention_at");
    let intervention_at = intervention_at_str
        .map(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)))
        .transpose()?;

    Ok(Session {
        id: Uuid::parse_str(&id_str)?,
        name: row.try_get("name").unwrap_or_default(),
        workdir: row.try_get("workdir").unwrap_or_default(),
        command: row.try_get("command").unwrap_or_default(),
        description: row.try_get("description").unwrap_or(None),
        status: status_str
            .parse::<SessionStatus>()
            .map_err(|e| anyhow::anyhow!(e))?,
        exit_code: row.try_get("exit_code").unwrap_or(None),
        backend_session_id: row.try_get("backend_session_id").unwrap_or(None),
        output_snapshot: row.try_get("output_snapshot").unwrap_or(None),
        metadata,
        ink: row.try_get("ink").unwrap_or(None),
        intervention_code,
        intervention_reason: row.try_get("intervention_reason").unwrap_or(None),
        intervention_at,
        last_output_at: {
            let s: Option<String> = row.try_get("last_output_at").unwrap_or(None);
            s.map(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)))
                .transpose()?
        },
        idle_since: {
            let s: Option<String> = row.try_get("idle_since").unwrap_or(None);
            s.map(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)))
                .transpose()?
        },
        idle_threshold_secs: {
            let v: Option<i32> = row.try_get("idle_threshold_secs").unwrap_or(None);
            v.map(|n| u32::try_from(n).unwrap_or(0))
        },
        worktree_path: row.try_get("worktree_path").unwrap_or(None),
        worktree_branch: row.try_get("worktree_branch").unwrap_or(None),
        git_branch: row.try_get("git_branch").unwrap_or(None),
        git_commit: row.try_get("git_commit").unwrap_or(None),
        git_files_changed: {
            let v: Option<i32> = row.try_get("git_files_changed").unwrap_or(None);
            v.map(|n| u32::try_from(n).unwrap_or(0))
        },
        git_insertions: {
            let v: Option<i32> = row.try_get("git_insertions").unwrap_or(None);
            v.map(|n| u32::try_from(n).unwrap_or(0))
        },
        git_deletions: {
            let v: Option<i32> = row.try_get("git_deletions").unwrap_or(None);
            v.map(|n| u32::try_from(n).unwrap_or(0))
        },
        git_ahead: {
            let v: Option<i32> = row.try_get("git_ahead").unwrap_or(None);
            v.map(|n| u32::try_from(n).unwrap_or(0))
        },
        runtime: {
            let s: Option<String> = row.try_get("runtime").unwrap_or(None);
            s.and_then(|s| s.parse().ok()).unwrap_or_default()
        },
        created_at: DateTime::parse_from_rfc3339(&created_str)?.with_timezone(&Utc),
        updated_at: DateTime::parse_from_rfc3339(&updated_str)?.with_timezone(&Utc),
    })
}

#[allow(clippy::unnecessary_wraps)]
pub(super) fn row_to_schedule(row: &SqliteRow) -> Result<pulpo_common::api::Schedule> {
    let secrets_json: String = row.try_get("secrets").unwrap_or_else(|_| "[]".to_owned());
    let secrets: Vec<String> = serde_json::from_str(&secrets_json).unwrap_or_default();
    Ok(pulpo_common::api::Schedule {
        id: row.try_get("id").unwrap_or_default(),
        name: row.try_get("name").unwrap_or_default(),
        cron: row.try_get("cron").unwrap_or_default(),
        command: row.try_get("command").unwrap_or_default(),
        workdir: row.try_get("workdir").unwrap_or_default(),
        target_node: row.try_get("target_node").unwrap_or(None),
        ink: row.try_get("ink").unwrap_or(None),
        description: row.try_get("description").unwrap_or(None),
        runtime: row.try_get("runtime").unwrap_or(None),
        secrets,
        worktree: row.try_get("worktree").unwrap_or(None),
        worktree_base: row.try_get("worktree_base").unwrap_or(None),
        enabled: row.try_get("enabled").unwrap_or(true),
        last_run_at: row.try_get("last_run_at").unwrap_or(None),
        last_session_id: row.try_get("last_session_id").unwrap_or(None),
        created_at: row.try_get("created_at").unwrap_or_default(),
    })
}

pub(super) fn row_to_session_index_entry(row: &SqliteRow) -> Result<SessionIndexEntry> {
    Ok(SessionIndexEntry {
        session_id: row.try_get("session_id")?,
        node_name: row.try_get("node_name")?,
        node_address: row.try_get("node_address").unwrap_or(None),
        session_name: row.try_get("session_name")?,
        status: row.try_get("status")?,
        command: row.try_get("command").unwrap_or(None),
        updated_at: row.try_get("updated_at")?,
    })
}

pub(super) fn row_to_intervention_event(row: &SqliteRow) -> Result<InterventionEvent> {
    let created_str: String = row.get("created_at");
    let code_str: Option<String> = row.get("code");
    let code = code_str
        .map(|s| {
            s.parse::<InterventionCode>()
                .map_err(|e| anyhow::anyhow!(e))
        })
        .transpose()?;
    Ok(InterventionEvent {
        id: row.get("id"),
        session_id: row.get("session_id"),
        code,
        reason: row.get("reason"),
        created_at: DateTime::parse_from_rfc3339(&created_str)?.with_timezone(&Utc),
    })
}

pub(super) fn row_to_enrolled_node(row: &SqliteRow) -> Result<EnrolledNode> {
    let last_seen_at = row
        .try_get::<Option<String>, _>("last_seen_at")?
        .map(|value| DateTime::parse_from_rfc3339(&value).map(|dt| dt.with_timezone(&Utc)))
        .transpose()?;
    Ok(EnrolledNode {
        node_name: row.try_get("node_name")?,
        token_hash: row.try_get("token_hash")?,
        last_seen_at,
        last_seen_address: row.try_get("last_seen_address").unwrap_or(None),
    })
}
