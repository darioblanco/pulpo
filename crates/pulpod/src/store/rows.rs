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

#[cfg(test)]
mod tests {
    use super::*;
    use pulpo_common::session::{Runtime, SessionStatus};
    use sqlx::SqlitePool;

    async fn memory_pool() -> SqlitePool {
        SqlitePool::connect("sqlite::memory:").await.unwrap()
    }

    #[tokio::test]
    async fn test_row_to_session_invalid_metadata_returns_error() {
        let pool = memory_pool().await;
        let row = sqlx::query(
            r"
            SELECT
                ? AS id,
                'sess' AS name,
                '/tmp/repo' AS workdir,
                'echo hi' AS command,
                NULL AS description,
                'active' AS status,
                NULL AS exit_code,
                'backend-1' AS backend_session_id,
                NULL AS output_snapshot,
                '{bad-json' AS metadata,
                NULL AS ink,
                NULL AS intervention_code,
                NULL AS intervention_reason,
                NULL AS intervention_at,
                NULL AS last_output_at,
                NULL AS idle_since,
                30 AS idle_threshold_secs,
                NULL AS worktree_path,
                NULL AS worktree_branch,
                NULL AS git_branch,
                NULL AS git_commit,
                1 AS git_files_changed,
                2 AS git_insertions,
                3 AS git_deletions,
                4 AS git_ahead,
                'tmux' AS runtime,
                '2024-01-01T00:00:00Z' AS created_at,
                '2024-01-01T00:00:00Z' AS updated_at
            ",
        )
        .bind(Uuid::new_v4().to_string())
        .fetch_one(&pool)
        .await
        .unwrap();

        let err = row_to_session(&row).unwrap_err().to_string();
        assert!(err.contains("key"));
    }

    #[tokio::test]
    async fn test_row_to_session_invalid_intervention_code_returns_error() {
        let pool = memory_pool().await;
        let row = sqlx::query(
            r"
            SELECT
                ? AS id,
                'sess' AS name,
                '/tmp/repo' AS workdir,
                'echo hi' AS command,
                NULL AS description,
                'active' AS status,
                NULL AS exit_code,
                'backend-1' AS backend_session_id,
                NULL AS output_snapshot,
                '{}' AS metadata,
                NULL AS ink,
                'bogus' AS intervention_code,
                NULL AS intervention_reason,
                NULL AS intervention_at,
                NULL AS last_output_at,
                NULL AS idle_since,
                30 AS idle_threshold_secs,
                NULL AS worktree_path,
                NULL AS worktree_branch,
                NULL AS git_branch,
                NULL AS git_commit,
                1 AS git_files_changed,
                2 AS git_insertions,
                3 AS git_deletions,
                4 AS git_ahead,
                'tmux' AS runtime,
                '2024-01-01T00:00:00Z' AS created_at,
                '2024-01-01T00:00:00Z' AS updated_at
            ",
        )
        .bind(Uuid::new_v4().to_string())
        .fetch_one(&pool)
        .await
        .unwrap();

        let err = row_to_session(&row).unwrap_err().to_string();
        assert!(err.contains("bogus"));
    }

    #[tokio::test]
    async fn test_row_to_session_clamps_negative_counts_and_defaults_runtime() {
        let pool = memory_pool().await;
        let row = sqlx::query(
            r"
            SELECT
                ? AS id,
                'sess' AS name,
                '/tmp/repo' AS workdir,
                'echo hi' AS command,
                NULL AS description,
                'idle' AS status,
                NULL AS exit_code,
                'backend-1' AS backend_session_id,
                NULL AS output_snapshot,
                '{}' AS metadata,
                NULL AS ink,
                NULL AS intervention_code,
                NULL AS intervention_reason,
                NULL AS intervention_at,
                NULL AS last_output_at,
                NULL AS idle_since,
                -1 AS idle_threshold_secs,
                NULL AS worktree_path,
                NULL AS worktree_branch,
                NULL AS git_branch,
                NULL AS git_commit,
                -2 AS git_files_changed,
                -3 AS git_insertions,
                -4 AS git_deletions,
                -5 AS git_ahead,
                'not-a-runtime' AS runtime,
                '2024-01-01T00:00:00Z' AS created_at,
                '2024-01-01T00:00:00Z' AS updated_at
            ",
        )
        .bind(Uuid::new_v4().to_string())
        .fetch_one(&pool)
        .await
        .unwrap();

        let session = row_to_session(&row).unwrap();
        assert_eq!(session.status, SessionStatus::Idle);
        assert_eq!(session.idle_threshold_secs, Some(0));
        assert_eq!(session.git_files_changed, Some(0));
        assert_eq!(session.git_insertions, Some(0));
        assert_eq!(session.git_deletions, Some(0));
        assert_eq!(session.git_ahead, Some(0));
        assert_eq!(session.runtime, Runtime::default());
    }

    #[tokio::test]
    async fn test_row_to_schedule_invalid_secrets_defaults_empty() {
        let pool = memory_pool().await;
        let row = sqlx::query(
            r"
            SELECT
                'sched-1' AS id,
                'nightly' AS name,
                '0 0 * * *' AS cron,
                'echo hi' AS command,
                '/tmp/repo' AS workdir,
                NULL AS target_node,
                NULL AS ink,
                NULL AS description,
                NULL AS runtime,
                'not-json' AS secrets,
                1 AS worktree,
                'main' AS worktree_base,
                1 AS enabled,
                NULL AS last_run_at,
                NULL AS last_session_id,
                '2024-01-01T00:00:00Z' AS created_at
            ",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let schedule = row_to_schedule(&row).unwrap();
        assert!(schedule.secrets.is_empty());
        assert_eq!(schedule.worktree, Some(true));
    }

    #[tokio::test]
    async fn test_row_to_enrolled_node_invalid_last_seen_returns_error() {
        let pool = memory_pool().await;
        let row = sqlx::query(
            r"
            SELECT
                'node-1' AS node_name,
                'hash' AS token_hash,
                'not-a-timestamp' AS last_seen_at,
                'http://node' AS last_seen_address
            ",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let err = row_to_enrolled_node(&row).unwrap_err().to_string();
        assert!(err.contains("timestamp") || err.contains("input"));
    }
}
