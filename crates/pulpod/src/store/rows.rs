use anyhow::Result;
use chrono::{DateTime, Utc};
use pulpo_common::session::{InterventionCode, Session, SessionStatus};
use sqlx::{Row, sqlite::SqliteRow};
use uuid::Uuid;

use super::InterventionEvent;

pub(super) fn row_to_session(row: &SqliteRow) -> Result<Session> {
    let id_str: String = row.get("id");
    let status_str: String = row.get("status");
    let created_str: String = row.get("created_at");
    let updated_str: String = row.get("updated_at");

    let metadata_json: Option<String> = row.get("metadata");
    let metadata = metadata_json
        .map(|s| serde_json::from_str::<std::collections::HashMap<String, String>>(&s))
        .transpose()?;

    // Unknown/retired codes (e.g. historical `burn_rate` rows from the removed
    // burn-velocity governor) degrade to `None` rather than failing the whole row —
    // same tolerance as `runtime` below. A single stale value must never make a
    // session (or the whole `list_sessions()` call) unreadable.
    let intervention_code_str: Option<String> = row.get("intervention_code");
    let intervention_code = intervention_code_str.and_then(|s| s.parse::<InterventionCode>().ok());

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
        status_reason: row.try_get("status_reason").unwrap_or(None),
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
        harness: row.try_get("harness").unwrap_or(None),
        harness_session_id: row.try_get("harness_session_id").unwrap_or(None),
        harness_last_event_at: {
            let s: Option<String> = row.try_get("harness_last_event_at").unwrap_or(None);
            s.map(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)))
                .transpose()?
        },
        created_at: DateTime::parse_from_rfc3339(&created_str)?.with_timezone(&Utc),
        updated_at: DateTime::parse_from_rfc3339(&updated_str)?.with_timezone(&Utc),
    })
}

#[allow(clippy::unnecessary_wraps)]
pub(super) fn row_to_schedule(row: &SqliteRow) -> Result<pulpo_common::api::Schedule> {
    Ok(pulpo_common::api::Schedule {
        id: row.try_get("id").unwrap_or_default(),
        name: row.try_get("name").unwrap_or_default(),
        cron: row.try_get("cron").unwrap_or_default(),
        command: row.try_get("command").unwrap_or_default(),
        workdir: row.try_get("workdir").unwrap_or_default(),
        ink: row.try_get("ink").unwrap_or(None),
        description: row.try_get("description").unwrap_or(None),
        runtime: row.try_get("runtime").unwrap_or(None),
        worktree: row.try_get("worktree").unwrap_or(None),
        worktree_base: row.try_get("worktree_base").unwrap_or(None),
        budget_cost_usd: row.try_get("budget_cost_usd").unwrap_or(None),
        enabled: row.try_get("enabled").unwrap_or(true),
        last_run_at: row.try_get("last_run_at").unwrap_or(None),
        last_session_id: row.try_get("last_session_id").unwrap_or(None),
        last_attempted_at: row.try_get("last_attempted_at").unwrap_or(None),
        last_error: row.try_get("last_error").unwrap_or(None),
        created_at: row.try_get("created_at").unwrap_or_default(),
    })
}

pub(super) fn row_to_intervention_event(row: &SqliteRow) -> Result<InterventionEvent> {
    let created_str: String = row.get("created_at");
    // Same tolerance as `row_to_session`'s `intervention_code` — a retired code
    // (e.g. historical `burn_rate` rows) degrades to `None`, not a hard error.
    let code_str: Option<String> = row.get("code");
    let code = code_str.and_then(|s| s.parse::<InterventionCode>().ok());
    Ok(InterventionEvent {
        id: row.get("id"),
        session_id: row.get("session_id"),
        code,
        reason: row.get("reason"),
        created_at: DateTime::parse_from_rfc3339(&created_str)?.with_timezone(&Utc),
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
                'working' AS status,
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
    async fn test_row_to_session_invalid_intervention_code_defaults_to_none() {
        // An unrecognized intervention_code (garbage data, or a retired code like the
        // removed burn-velocity governor's "burn_rate") must not fail the whole row —
        // it degrades to `None` so the rest of the session stays readable.
        let pool = memory_pool().await;
        let row = sqlx::query(
            r"
            SELECT
                ? AS id,
                'sess' AS name,
                '/tmp/repo' AS workdir,
                'echo hi' AS command,
                NULL AS description,
                'working' AS status,
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

        let session = row_to_session(&row).unwrap();
        assert_eq!(session.intervention_code, None);
    }

    #[tokio::test]
    async fn test_row_to_session_tolerates_retired_burn_rate_intervention_code() {
        // A session stopped by the (now-removed) burn-velocity governor before its
        // removal still has `intervention_code = 'burn_rate'` in the DB. It must
        // remain readable — just with no reconstructable code.
        let pool = memory_pool().await;
        let row = sqlx::query(
            r"
            SELECT
                ? AS id,
                'sess' AS name,
                '/tmp/repo' AS workdir,
                'echo hi' AS command,
                NULL AS description,
                'done' AS status,
                NULL AS exit_code,
                'backend-1' AS backend_session_id,
                NULL AS output_snapshot,
                '{}' AS metadata,
                NULL AS ink,
                'burn_rate' AS intervention_code,
                'Cost $5.00/hr exceeded ceiling $2.00/hr' AS intervention_reason,
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

        let session = row_to_session(&row).unwrap();
        assert_eq!(session.intervention_code, None);
        assert!(session.intervention_reason.is_some());
    }

    #[tokio::test]
    async fn test_row_to_session_clamps_negative_counts_and_defaults_runtime() {
        // Incidentally also exercises `SessionStatus::from_str`'s pre-ADR-0009 alias:
        // a raw `'idle'` row (old vocabulary, still on disk for any row not yet
        // touched by migration 0010, or a downgrade) must still parse to `Waiting`.
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
        assert_eq!(session.status, SessionStatus::Waiting);
        assert_eq!(session.idle_threshold_secs, Some(0));
        assert_eq!(session.git_files_changed, Some(0));
        assert_eq!(session.git_insertions, Some(0));
        assert_eq!(session.git_deletions, Some(0));
        assert_eq!(session.git_ahead, Some(0));
        assert_eq!(session.runtime, Runtime::default());
    }

    #[tokio::test]
    async fn test_row_to_session_parses_harness_fields() {
        let pool = memory_pool().await;
        let row = sqlx::query(
            r"
            SELECT
                ? AS id,
                'sess' AS name,
                '/tmp/repo' AS workdir,
                'claude' AS command,
                NULL AS description,
                'working' AS status,
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
                NULL AS idle_threshold_secs,
                NULL AS worktree_path,
                NULL AS worktree_branch,
                NULL AS git_branch,
                NULL AS git_commit,
                NULL AS git_files_changed,
                NULL AS git_insertions,
                NULL AS git_deletions,
                NULL AS git_ahead,
                'tmux' AS runtime,
                'claude' AS harness,
                'sid-abc' AS harness_session_id,
                '2024-01-01T00:00:00Z' AS harness_last_event_at,
                '2024-01-01T00:00:00Z' AS created_at,
                '2024-01-01T00:00:00Z' AS updated_at
            ",
        )
        .bind(Uuid::new_v4().to_string())
        .fetch_one(&pool)
        .await
        .unwrap();

        let session = row_to_session(&row).unwrap();
        assert_eq!(session.harness.as_deref(), Some("claude"));
        assert_eq!(session.harness_session_id.as_deref(), Some("sid-abc"));
        assert!(session.harness_last_event_at.is_some());
    }

    #[tokio::test]
    async fn test_row_to_session_invalid_harness_last_event_at_returns_error() {
        let pool = memory_pool().await;
        let row = sqlx::query(
            r"
            SELECT
                ? AS id,
                'sess' AS name,
                '/tmp/repo' AS workdir,
                'claude' AS command,
                NULL AS description,
                'working' AS status,
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
                NULL AS idle_threshold_secs,
                NULL AS worktree_path,
                NULL AS worktree_branch,
                NULL AS git_branch,
                NULL AS git_commit,
                NULL AS git_files_changed,
                NULL AS git_insertions,
                NULL AS git_deletions,
                NULL AS git_ahead,
                'tmux' AS runtime,
                'claude' AS harness,
                'sid-abc' AS harness_session_id,
                'not-a-date' AS harness_last_event_at,
                '2024-01-01T00:00:00Z' AS created_at,
                '2024-01-01T00:00:00Z' AS updated_at
            ",
        )
        .bind(Uuid::new_v4().to_string())
        .fetch_one(&pool)
        .await
        .unwrap();

        assert!(row_to_session(&row).is_err());
    }

    #[tokio::test]
    async fn test_row_to_schedule_reads_fields() {
        let pool = memory_pool().await;
        let row = sqlx::query(
            r"
            SELECT
                'sched-1' AS id,
                'nightly' AS name,
                '0 0 * * *' AS cron,
                'echo hi' AS command,
                '/tmp/repo' AS workdir,
                NULL AS ink,
                NULL AS description,
                NULL AS runtime,
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
        assert_eq!(schedule.id, "sched-1");
        assert_eq!(schedule.worktree, Some(true));
    }

    #[tokio::test]
    async fn test_row_to_intervention_event_parses_known_code() {
        let pool = memory_pool().await;
        let row = sqlx::query(
            r"
            SELECT
                1 AS id,
                'sess-1' AS session_id,
                'budget_exceeded' AS code,
                'Cost $2.00 reached budget $2.00' AS reason,
                '2024-01-01T00:00:00Z' AS created_at
            ",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let event = row_to_intervention_event(&row).unwrap();
        assert_eq!(event.session_id, "sess-1");
        assert_eq!(event.code, Some(InterventionCode::BudgetExceeded));
    }

    #[tokio::test]
    async fn test_row_to_intervention_event_tolerates_retired_code() {
        // A retired code (e.g. historical "burn_rate" rows from the removed
        // burn-velocity governor) degrades to `None` instead of erroring the row.
        let pool = memory_pool().await;
        let row = sqlx::query(
            r"
            SELECT
                2 AS id,
                'sess-2' AS session_id,
                'burn_rate' AS code,
                'Burn rate exceeded ceiling' AS reason,
                '2024-01-01T00:00:00Z' AS created_at
            ",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let event = row_to_intervention_event(&row).unwrap();
        assert_eq!(event.session_id, "sess-2");
        assert_eq!(event.code, None);
    }

    #[tokio::test]
    async fn test_row_to_intervention_event_null_code() {
        let pool = memory_pool().await;
        let row = sqlx::query(
            r"
            SELECT
                3 AS id,
                'sess-3' AS session_id,
                NULL AS code,
                'Manual stop' AS reason,
                '2024-01-01T00:00:00Z' AS created_at
            ",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let event = row_to_intervention_event(&row).unwrap();
        assert_eq!(event.code, None);
    }
}
