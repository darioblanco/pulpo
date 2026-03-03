use std::fmt::Write;

use anyhow::Result;
use chrono::{DateTime, Utc};
use pulpo_common::api::ListSessionsQuery;
use pulpo_common::guard::GuardConfig;
use pulpo_common::schedule::{
    ConcurrencyPolicy, ExecutionStatus, Schedule, ScheduleExecution, ScheduleStatus, WorktreeConfig,
};
use pulpo_common::session::{Provider, Session, SessionMode, SessionStatus};
use sqlx::{Row, SqlitePool, sqlite::SqliteRow};
use uuid::Uuid;

/// A single intervention event for audit trail purposes.
#[derive(Debug, Clone)]
pub struct InterventionEvent {
    pub id: i64,
    pub session_id: String,
    pub reason: String,
    pub created_at: DateTime<Utc>,
}

/// A detection event for tracking watchdog accuracy (false-positive analysis).
#[derive(Debug, Clone)]
pub struct DetectionEvent {
    pub id: i64,
    pub session_id: String,
    pub detector: String,
    pub action: String,
    pub was_false_positive: bool,
    pub created_at: DateTime<Utc>,
}

/// Per-detector statistics for false-positive tracking.
#[derive(Debug, Clone)]
pub struct DetectorStats {
    pub total: i64,
    pub false_positives: i64,
    pub rate: f64,
}

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
    data_dir: String,
}

impl Store {
    pub async fn new(data_dir: &str) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let db_path = format!("{data_dir}/state.db");
        let url = format!("sqlite:{db_path}?mode=rwc");
        let pool = SqlitePool::connect(&url).await?;
        Ok(Self {
            pool,
            data_dir: data_dir.to_owned(),
        })
    }

    #[allow(clippy::too_many_lines)]
    pub async fn migrate(&self) -> Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                workdir TEXT NOT NULL,
                provider TEXT NOT NULL,
                prompt TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'creating',
                mode TEXT NOT NULL DEFAULT 'interactive',
                conversation_id TEXT,
                exit_code INTEGER,
                tmux_session TEXT,
                docker_container TEXT,
                output_snapshot TEXT,
                git_branch TEXT,
                git_sha TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;

        // Idempotent migration: add guard_config column if missing
        let has_guard = sqlx::query_scalar::<_, i32>(
            "SELECT count(*) FROM pragma_table_info('sessions') WHERE name = 'guard_config'",
        )
        .fetch_one(&self.pool)
        .await?;
        if has_guard == 0 {
            sqlx::query("ALTER TABLE sessions ADD COLUMN guard_config TEXT")
                .execute(&self.pool)
                .await?;
        }

        // Idempotent migration: add intervention columns if missing
        let has_intervention = sqlx::query_scalar::<_, i32>(
            "SELECT count(*) FROM pragma_table_info('sessions') WHERE name = 'intervention_reason'",
        )
        .fetch_one(&self.pool)
        .await?;
        if has_intervention == 0 {
            sqlx::query("ALTER TABLE sessions ADD COLUMN intervention_reason TEXT")
                .execute(&self.pool)
                .await?;
            sqlx::query("ALTER TABLE sessions ADD COLUMN intervention_at TEXT")
                .execute(&self.pool)
                .await?;
        }

        // Idempotent migration: recovery_count column
        let has_recovery: i32 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'recovery_count'",
        )
        .fetch_one(&self.pool)
        .await?;
        if has_recovery == 0 {
            sqlx::query("ALTER TABLE sessions ADD COLUMN recovery_count INTEGER DEFAULT 0")
                .execute(&self.pool)
                .await?;
        }

        // Idempotent migration: last_output_at column
        let has_last_output: i32 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'last_output_at'",
        )
        .fetch_one(&self.pool)
        .await?;
        if has_last_output == 0 {
            sqlx::query("ALTER TABLE sessions ADD COLUMN last_output_at TEXT")
                .execute(&self.pool)
                .await?;
        }

        // Idempotent migration: idle_since column
        let has_idle_since: i32 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'idle_since'",
        )
        .fetch_one(&self.pool)
        .await?;
        if has_idle_since == 0 {
            sqlx::query("ALTER TABLE sessions ADD COLUMN idle_since TEXT")
                .execute(&self.pool)
                .await?;
        }

        // Idempotent migration: model column
        let has_model: i32 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'model'",
        )
        .fetch_one(&self.pool)
        .await?;
        if has_model == 0 {
            sqlx::query("ALTER TABLE sessions ADD COLUMN model TEXT")
                .execute(&self.pool)
                .await?;
            sqlx::query("ALTER TABLE sessions ADD COLUMN allowed_tools TEXT")
                .execute(&self.pool)
                .await?;
            sqlx::query("ALTER TABLE sessions ADD COLUMN system_prompt TEXT")
                .execute(&self.pool)
                .await?;
            sqlx::query("ALTER TABLE sessions ADD COLUMN metadata TEXT")
                .execute(&self.pool)
                .await?;
            sqlx::query("ALTER TABLE sessions ADD COLUMN persona TEXT")
                .execute(&self.pool)
                .await?;
        }

        // Idempotent migration: max_turns, max_budget_usd, output_format columns
        let has_max_turns: i32 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'max_turns'",
        )
        .fetch_one(&self.pool)
        .await?;
        if has_max_turns == 0 {
            sqlx::query("ALTER TABLE sessions ADD COLUMN max_turns INTEGER")
                .execute(&self.pool)
                .await?;
            sqlx::query("ALTER TABLE sessions ADD COLUMN max_budget_usd REAL")
                .execute(&self.pool)
                .await?;
            sqlx::query("ALTER TABLE sessions ADD COLUMN output_format TEXT")
                .execute(&self.pool)
                .await?;
        }

        // Idempotent migration: waiting_for_input column
        let has_waiting_for_input: i32 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'waiting_for_input'",
        )
        .fetch_one(&self.pool)
        .await?;
        if has_waiting_for_input == 0 {
            sqlx::query(
                "ALTER TABLE sessions ADD COLUMN waiting_for_input INTEGER NOT NULL DEFAULT 0",
            )
            .execute(&self.pool)
            .await?;
        }

        // Idempotent migration: append-only intervention events table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS intervention_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                reason TEXT NOT NULL,
                created_at TEXT NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;

        // Idempotent migration: detection events for false-positive tracking
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS detection_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                detector TEXT NOT NULL,
                action TEXT NOT NULL,
                was_false_positive INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;

        // Idempotent migration: rename repo_path → workdir
        let has_repo_path: i32 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'repo_path'",
        )
        .fetch_one(&self.pool)
        .await?;
        if has_repo_path > 0 {
            sqlx::query("ALTER TABLE sessions RENAME COLUMN repo_path TO workdir")
                .execute(&self.pool)
                .await?;
        }

        // Idempotent migration: schedules table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS schedules (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                cron TEXT NOT NULL,
                workdir TEXT NOT NULL,
                prompt TEXT NOT NULL,
                provider TEXT NOT NULL DEFAULT 'claude',
                mode TEXT NOT NULL DEFAULT 'interactive',
                guard_preset TEXT,
                guard_config TEXT,
                model TEXT,
                allowed_tools TEXT,
                system_prompt TEXT,
                metadata TEXT,
                persona TEXT,
                max_turns INTEGER,
                max_budget_usd REAL,
                output_format TEXT,
                concurrency TEXT NOT NULL DEFAULT 'skip',
                status TEXT NOT NULL DEFAULT 'active',
                max_executions INTEGER,
                execution_count INTEGER NOT NULL DEFAULT 0,
                last_run_at TEXT,
                next_run_at TEXT,
                last_session_id TEXT,
                worktree TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;

        // Idempotent migration: schedule_executions table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS schedule_executions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                schedule_id TEXT NOT NULL,
                session_id TEXT,
                status TEXT NOT NULL,
                error TEXT,
                triggered_by TEXT NOT NULL,
                created_at TEXT NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;

        // Idempotent migration: max_turns, max_budget_usd, output_format for schedules
        let has_sched_max_turns: i32 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('schedules') WHERE name = 'max_turns'",
        )
        .fetch_one(&self.pool)
        .await?;
        if has_sched_max_turns == 0 {
            sqlx::query("ALTER TABLE schedules ADD COLUMN max_turns INTEGER")
                .execute(&self.pool)
                .await?;
            sqlx::query("ALTER TABLE schedules ADD COLUMN max_budget_usd REAL")
                .execute(&self.pool)
                .await?;
            sqlx::query("ALTER TABLE schedules ADD COLUMN output_format TEXT")
                .execute(&self.pool)
                .await?;
        }

        Ok(())
    }

    pub async fn insert_session(&self, session: &Session) -> Result<()> {
        let guard_json = session
            .guard_config
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let allowed_tools_json = session
            .allowed_tools
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let metadata_json = session
            .metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let intervention_at_str = session.intervention_at.map(|dt| dt.to_rfc3339());
        let last_output_at_str = session.last_output_at.map(|dt| dt.to_rfc3339());
        let idle_since_str = session.idle_since.map(|dt| dt.to_rfc3339());
        #[allow(clippy::cast_possible_wrap)]
        let max_turns_i32 = session.max_turns.map(|n| n as i32);
        sqlx::query(
            "INSERT INTO sessions (id, name, workdir, provider, prompt, status, mode,
                conversation_id, exit_code, tmux_session,
                output_snapshot, git_branch, git_sha, guard_config,
                model, allowed_tools, system_prompt, metadata, persona,
                max_turns, max_budget_usd, output_format,
                intervention_reason, intervention_at, recovery_count,
                last_output_at, idle_since, waiting_for_input, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(session.id.to_string())
        .bind(&session.name)
        .bind(&session.workdir)
        .bind(session.provider.to_string())
        .bind(&session.prompt)
        .bind(session.status.to_string())
        .bind(session.mode.to_string())
        .bind(&session.conversation_id)
        .bind(session.exit_code)
        .bind(&session.tmux_session)
        .bind(&session.output_snapshot)
        .bind(&session.git_branch)
        .bind(&session.git_sha)
        .bind(&guard_json)
        .bind(&session.model)
        .bind(&allowed_tools_json)
        .bind(&session.system_prompt)
        .bind(&metadata_json)
        .bind(&session.persona)
        .bind(max_turns_i32)
        .bind(session.max_budget_usd)
        .bind(&session.output_format)
        .bind(&session.intervention_reason)
        .bind(&intervention_at_str)
        .bind(session.recovery_count)
        .bind(&last_output_at_str)
        .bind(&idle_since_str)
        .bind(session.waiting_for_input)
        .bind(session.created_at.to_rfc3339())
        .bind(session.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_session(&self, id_or_name: &str) -> Result<Option<Session>> {
        let row = sqlx::query("SELECT * FROM sessions WHERE id = ? OR name = ?")
            .bind(id_or_name)
            .bind(id_or_name)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|r| row_to_session(&r)).transpose()
    }

    pub async fn list_sessions(&self) -> Result<Vec<Session>> {
        let rows = sqlx::query("SELECT * FROM sessions ORDER BY created_at DESC")
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(row_to_session).collect()
    }

    pub async fn list_sessions_filtered(&self, query: &ListSessionsQuery) -> Result<Vec<Session>> {
        let mut sql = String::from("SELECT * FROM sessions WHERE 1=1");
        let mut binds: Vec<String> = Vec::new();

        if let Some(status) = &query.status {
            let statuses: Vec<&str> = status.split(',').map(str::trim).collect();
            let placeholders: Vec<String> = statuses.iter().map(|_| "?".to_owned()).collect();
            let _ = write!(sql, " AND status IN ({})", placeholders.join(","));
            binds.extend(statuses.iter().map(|s| (*s).to_owned()));
        }

        if let Some(provider) = &query.provider {
            let providers: Vec<&str> = provider.split(',').map(str::trim).collect();
            let placeholders: Vec<String> = providers.iter().map(|_| "?".to_owned()).collect();
            let _ = write!(sql, " AND provider IN ({})", placeholders.join(","));
            binds.extend(providers.iter().map(|s| (*s).to_owned()));
        }

        if let Some(search) = &query.search {
            sql.push_str(" AND (name LIKE ? OR prompt LIKE ?)");
            let pattern = format!("%{search}%");
            binds.push(pattern.clone());
            binds.push(pattern);
        }

        let sort_col = match query.sort.as_deref() {
            Some("name") => "name",
            Some("status") => "status",
            Some("provider") => "provider",
            _ => "created_at",
        };
        let order = match query.order.as_deref() {
            Some("asc") => "ASC",
            _ => "DESC",
        };
        let _ = write!(sql, " ORDER BY {sort_col} {order}");

        let mut q = sqlx::query(&sql);
        for bind in &binds {
            q = q.bind(bind);
        }

        let rows = q.fetch_all(&self.pool).await?;
        rows.iter().map(row_to_session).collect()
    }

    pub async fn update_session_status(&self, id: &str, status: SessionStatus) -> Result<()> {
        sqlx::query("UPDATE sessions SET status = ?, updated_at = ? WHERE id = ?")
            .bind(status.to_string())
            .bind(Utc::now().to_rfc3339())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_session_conversation_id(
        &self,
        id: &str,
        conversation_id: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE sessions SET conversation_id = ?, updated_at = ? WHERE id = ?")
            .bind(conversation_id)
            .bind(Utc::now().to_rfc3339())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_session(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub const fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn data_dir(&self) -> &str {
        &self.data_dir
    }

    pub async fn update_session_intervention(&self, id: &str, reason: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE sessions SET intervention_reason = ?, intervention_at = ?, status = 'dead', updated_at = ? WHERE id = ?",
        )
        .bind(reason)
        .bind(&now)
        .bind(&now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        // Append to audit log
        sqlx::query(
            "INSERT INTO intervention_events (session_id, reason, created_at) VALUES (?, ?, ?)",
        )
        .bind(id)
        .bind(reason)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_intervention_events(
        &self,
        session_id: &str,
    ) -> Result<Vec<InterventionEvent>> {
        let rows = sqlx::query(
            "SELECT id, session_id, reason, created_at FROM intervention_events WHERE session_id = ? ORDER BY id ASC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_intervention_event).collect()
    }

    pub async fn clear_session_intervention(&self, id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE sessions SET intervention_reason = NULL, intervention_at = NULL, updated_at = ? WHERE id = ?",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_session_output_snapshot(&self, id: &str, snapshot: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        // Only update last_output_at when the output actually changed
        sqlx::query(
            "UPDATE sessions SET output_snapshot = ?,
                last_output_at = CASE WHEN output_snapshot IS NULL OR output_snapshot != ? THEN ? ELSE last_output_at END,
                updated_at = ?
             WHERE id = ?",
        )
        .bind(snapshot)
        .bind(snapshot)
        .bind(&now)
        .bind(&now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn increment_recovery_count(&self, id: &str) -> Result<u32> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE sessions SET recovery_count = recovery_count + 1, updated_at = ? WHERE id = ?",
        )
        .bind(&now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        let row = sqlx::query("SELECT recovery_count FROM sessions WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await?;
        let count: i32 = row.get("recovery_count");
        Ok(count.cast_unsigned())
    }

    pub async fn resume_dead_session(&self, id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE sessions SET status = 'running', intervention_reason = NULL, intervention_at = NULL, updated_at = ? WHERE id = ?",
        )
        .bind(&now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_session_idle_since(&self, id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE sessions SET idle_since = ?, updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn clear_session_idle_since(&self, id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE sessions SET idle_since = NULL, updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_session_waiting_for_input(&self, id: &str, waiting: bool) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE sessions SET waiting_for_input = ?, updated_at = ? WHERE id = ?")
            .bind(waiting)
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn insert_detection_event(
        &self,
        session_id: &str,
        detector: &str,
        action: &str,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        let result = sqlx::query(
            "INSERT INTO detection_events (session_id, detector, action, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(session_id)
        .bind(detector)
        .bind(action)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    pub async fn list_detection_events(
        &self,
        detector: Option<&str>,
    ) -> Result<Vec<DetectionEvent>> {
        let rows = if let Some(det) = detector {
            sqlx::query(
                "SELECT id, session_id, detector, action, was_false_positive, created_at
                 FROM detection_events WHERE detector = ? ORDER BY id ASC",
            )
            .bind(det)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT id, session_id, detector, action, was_false_positive, created_at
                 FROM detection_events ORDER BY id ASC",
            )
            .fetch_all(&self.pool)
            .await?
        };
        rows.iter().map(row_to_detection_event).collect()
    }

    pub async fn mark_detection_false_positive(&self, id: i64) -> Result<bool> {
        let result = sqlx::query("UPDATE detection_events SET was_false_positive = 1 WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    // --- Schedule CRUD ---

    #[allow(clippy::too_many_lines)]
    pub async fn insert_schedule(&self, schedule: &Schedule) -> Result<()> {
        let guard_json = schedule
            .guard_config
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let allowed_tools_json = schedule
            .allowed_tools
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let metadata_json = schedule
            .metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let worktree_json = schedule
            .worktree
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let last_run_at_str = schedule.last_run_at.map(|dt| dt.to_rfc3339());
        let next_run_at_str = schedule.next_run_at.map(|dt| dt.to_rfc3339());
        let last_session_id_str = schedule.last_session_id.map(|id| id.to_string());

        #[allow(clippy::cast_possible_wrap)]
        let max_turns_i32 = schedule.max_turns.map(|n| n as i32);

        sqlx::query(
            "INSERT INTO schedules (id, name, cron, workdir, prompt, provider, mode,
                guard_preset, guard_config, model, allowed_tools, system_prompt,
                metadata, persona, max_turns, max_budget_usd, output_format,
                concurrency, status, max_executions,
                execution_count, last_run_at, next_run_at, last_session_id,
                worktree, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(schedule.id.to_string())
        .bind(&schedule.name)
        .bind(&schedule.cron)
        .bind(&schedule.workdir)
        .bind(&schedule.prompt)
        .bind(schedule.provider.to_string())
        .bind(schedule.mode.to_string())
        .bind(&schedule.guard_preset)
        .bind(&guard_json)
        .bind(&schedule.model)
        .bind(&allowed_tools_json)
        .bind(&schedule.system_prompt)
        .bind(&metadata_json)
        .bind(&schedule.persona)
        .bind(max_turns_i32)
        .bind(schedule.max_budget_usd)
        .bind(&schedule.output_format)
        .bind(schedule.concurrency.to_string())
        .bind(schedule.status.to_string())
        .bind(schedule.max_executions.map(i64::from))
        .bind(i64::from(schedule.execution_count))
        .bind(&last_run_at_str)
        .bind(&next_run_at_str)
        .bind(&last_session_id_str)
        .bind(&worktree_json)
        .bind(schedule.created_at.to_rfc3339())
        .bind(schedule.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_schedule(&self, id: &str) -> Result<Option<Schedule>> {
        let row = sqlx::query("SELECT * FROM schedules WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|r| row_to_schedule(&r)).transpose()
    }

    pub async fn get_schedule_by_name(&self, name: &str) -> Result<Option<Schedule>> {
        let row = sqlx::query("SELECT * FROM schedules WHERE name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|r| row_to_schedule(&r)).transpose()
    }

    pub async fn get_schedule_by_id_or_name(&self, id_or_name: &str) -> Result<Option<Schedule>> {
        let row = sqlx::query("SELECT * FROM schedules WHERE id = ? OR name = ?")
            .bind(id_or_name)
            .bind(id_or_name)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|r| row_to_schedule(&r)).transpose()
    }

    pub async fn list_schedules(&self) -> Result<Vec<Schedule>> {
        let rows = sqlx::query("SELECT * FROM schedules ORDER BY created_at DESC")
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(row_to_schedule).collect()
    }

    pub async fn list_active_schedules(&self) -> Result<Vec<Schedule>> {
        let rows =
            sqlx::query("SELECT * FROM schedules WHERE status = 'active' ORDER BY next_run_at ASC")
                .fetch_all(&self.pool)
                .await?;
        rows.iter().map(row_to_schedule).collect()
    }

    pub async fn update_schedule(&self, schedule: &Schedule) -> Result<()> {
        let guard_json = schedule
            .guard_config
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let allowed_tools_json = schedule
            .allowed_tools
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let metadata_json = schedule
            .metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let worktree_json = schedule
            .worktree
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let last_run_at_str = schedule.last_run_at.map(|dt| dt.to_rfc3339());
        let next_run_at_str = schedule.next_run_at.map(|dt| dt.to_rfc3339());
        let last_session_id_str = schedule.last_session_id.map(|id| id.to_string());

        #[allow(clippy::cast_possible_wrap)]
        let max_turns_i32 = schedule.max_turns.map(|n| n as i32);

        sqlx::query(
            "UPDATE schedules SET cron = ?, workdir = ?, prompt = ?, provider = ?, mode = ?,
                guard_preset = ?, guard_config = ?, model = ?, allowed_tools = ?,
                system_prompt = ?, metadata = ?, persona = ?,
                max_turns = ?, max_budget_usd = ?, output_format = ?,
                concurrency = ?,
                status = ?, max_executions = ?, execution_count = ?,
                last_run_at = ?, next_run_at = ?, last_session_id = ?,
                worktree = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&schedule.cron)
        .bind(&schedule.workdir)
        .bind(&schedule.prompt)
        .bind(schedule.provider.to_string())
        .bind(schedule.mode.to_string())
        .bind(&schedule.guard_preset)
        .bind(&guard_json)
        .bind(&schedule.model)
        .bind(&allowed_tools_json)
        .bind(&schedule.system_prompt)
        .bind(&metadata_json)
        .bind(&schedule.persona)
        .bind(max_turns_i32)
        .bind(schedule.max_budget_usd)
        .bind(&schedule.output_format)
        .bind(schedule.concurrency.to_string())
        .bind(schedule.status.to_string())
        .bind(schedule.max_executions.map(i64::from))
        .bind(i64::from(schedule.execution_count))
        .bind(&last_run_at_str)
        .bind(&next_run_at_str)
        .bind(&last_session_id_str)
        .bind(&worktree_json)
        .bind(schedule.updated_at.to_rfc3339())
        .bind(schedule.id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_schedule(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM schedules WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM schedule_executions WHERE schedule_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn insert_execution(&self, execution: &ScheduleExecution) -> Result<i64> {
        let session_id_str = execution.session_id.map(|id| id.to_string());
        let result = sqlx::query(
            "INSERT INTO schedule_executions (schedule_id, session_id, status, error, triggered_by, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(execution.schedule_id.to_string())
        .bind(&session_id_str)
        .bind(execution.status.to_string())
        .bind(&execution.error)
        .bind(&execution.triggered_by)
        .bind(execution.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    pub async fn list_executions(
        &self,
        schedule_id: &str,
        limit: u32,
    ) -> Result<Vec<ScheduleExecution>> {
        let rows = sqlx::query(
            "SELECT * FROM schedule_executions WHERE schedule_id = ? ORDER BY id DESC LIMIT ?",
        )
        .bind(schedule_id)
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_execution).collect()
    }

    pub async fn detection_event_stats(
        &self,
    ) -> Result<std::collections::HashMap<String, DetectorStats>> {
        let rows = sqlx::query(
            "SELECT detector,
                    COUNT(*) as total,
                    SUM(was_false_positive) as false_positives
             FROM detection_events GROUP BY detector",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut map = std::collections::HashMap::new();
        for row in &rows {
            let detector: String = row.get("detector");
            let total: i64 = row.get("total");
            let false_positives: i64 = row.get("false_positives");
            // total is always >= 1 (GROUP BY ensures at least one row per detector)
            #[allow(clippy::cast_precision_loss)]
            let rate = (false_positives as f64) / (total as f64);
            map.insert(
                detector,
                DetectorStats {
                    total,
                    false_positives,
                    rate,
                },
            );
        }
        Ok(map)
    }
}

fn row_to_session(row: &SqliteRow) -> Result<Session> {
    let id_str: String = row.get("id");
    let provider_str: String = row.get("provider");
    let status_str: String = row.get("status");
    let mode_str: String = row.get("mode");
    let created_str: String = row.get("created_at");
    let updated_str: String = row.get("updated_at");

    let guard_json: Option<String> = row.get("guard_config");
    let guard_config = guard_json
        .map(|s| serde_json::from_str::<GuardConfig>(&s))
        .transpose()?;

    let allowed_tools_json: Option<String> = row.get("allowed_tools");
    let allowed_tools = allowed_tools_json
        .map(|s| serde_json::from_str::<Vec<String>>(&s))
        .transpose()?;

    let metadata_json: Option<String> = row.get("metadata");
    let metadata = metadata_json
        .map(|s| serde_json::from_str::<std::collections::HashMap<String, String>>(&s))
        .transpose()?;

    let intervention_at_str: Option<String> = row.get("intervention_at");
    let intervention_at = intervention_at_str
        .map(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)))
        .transpose()?;

    Ok(Session {
        id: Uuid::parse_str(&id_str)?,
        name: row.get("name"),
        workdir: row.get("workdir"),
        provider: provider_str
            .parse::<Provider>()
            .map_err(|e| anyhow::anyhow!(e))?,
        prompt: row.get("prompt"),
        status: status_str
            .parse::<SessionStatus>()
            .map_err(|e| anyhow::anyhow!(e))?,
        mode: mode_str
            .parse::<SessionMode>()
            .map_err(|e| anyhow::anyhow!(e))?,
        conversation_id: row.get("conversation_id"),
        exit_code: row.get("exit_code"),
        tmux_session: row.get("tmux_session"),
        output_snapshot: row.get("output_snapshot"),
        git_branch: row.get("git_branch"),
        git_sha: row.get("git_sha"),
        guard_config,
        model: row.get("model"),
        allowed_tools,
        system_prompt: row.get("system_prompt"),
        metadata,
        persona: row.get("persona"),
        max_turns: {
            let v: Option<i32> = row.get("max_turns");
            v.map(i32::cast_unsigned)
        },
        max_budget_usd: row.get("max_budget_usd"),
        output_format: row.get("output_format"),
        intervention_reason: row.get("intervention_reason"),
        intervention_at,
        recovery_count: row.get::<i32, _>("recovery_count").cast_unsigned(),
        last_output_at: {
            let s: Option<String> = row.get("last_output_at");
            s.map(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)))
                .transpose()?
        },
        idle_since: {
            let s: Option<String> = row.get("idle_since");
            s.map(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)))
                .transpose()?
        },
        waiting_for_input: {
            let v: Option<i32> = row.try_get("waiting_for_input").unwrap_or(None);
            v.is_some_and(|n| n != 0)
        },
        created_at: DateTime::parse_from_rfc3339(&created_str)?.with_timezone(&Utc),
        updated_at: DateTime::parse_from_rfc3339(&updated_str)?.with_timezone(&Utc),
    })
}

fn row_to_intervention_event(row: &SqliteRow) -> Result<InterventionEvent> {
    let created_str: String = row.get("created_at");
    Ok(InterventionEvent {
        id: row.get("id"),
        session_id: row.get("session_id"),
        reason: row.get("reason"),
        created_at: DateTime::parse_from_rfc3339(&created_str)?.with_timezone(&Utc),
    })
}

fn row_to_detection_event(row: &SqliteRow) -> Result<DetectionEvent> {
    let created_str: String = row.get("created_at");
    let fp_int: i32 = row.get("was_false_positive");
    Ok(DetectionEvent {
        id: row.get("id"),
        session_id: row.get("session_id"),
        detector: row.get("detector"),
        action: row.get("action"),
        was_false_positive: fp_int != 0,
        created_at: DateTime::parse_from_rfc3339(&created_str)?.with_timezone(&Utc),
    })
}

fn parse_optional_datetime(row: &SqliteRow, col: &str) -> Result<Option<DateTime<Utc>>> {
    let s: Option<String> = row.get(col);
    s.map(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)))
        .transpose()
        .map_err(Into::into)
}

fn parse_optional_uuid(row: &SqliteRow, col: &str) -> Result<Option<Uuid>> {
    let s: Option<String> = row.get(col);
    s.map(|s| Uuid::parse_str(&s))
        .transpose()
        .map_err(Into::into)
}

fn row_to_schedule(row: &SqliteRow) -> Result<Schedule> {
    let id_str: String = row.get("id");
    let provider_str: String = row.get("provider");
    let mode_str: String = row.get("mode");
    let concurrency_str: String = row.get("concurrency");
    let status_str: String = row.get("status");
    let created_str: String = row.get("created_at");
    let updated_str: String = row.get("updated_at");

    let guard_json: Option<String> = row.get("guard_config");
    let guard_config = guard_json
        .map(|s| serde_json::from_str::<GuardConfig>(&s))
        .transpose()?;

    let allowed_tools_json: Option<String> = row.get("allowed_tools");
    let allowed_tools = allowed_tools_json
        .map(|s| serde_json::from_str::<Vec<String>>(&s))
        .transpose()?;

    let metadata_json: Option<String> = row.get("metadata");
    let metadata = metadata_json
        .map(|s| serde_json::from_str::<std::collections::HashMap<String, String>>(&s))
        .transpose()?;

    let worktree_json: Option<String> = row.get("worktree");
    let worktree = worktree_json
        .map(|s| serde_json::from_str::<WorktreeConfig>(&s))
        .transpose()?;

    let max_executions: Option<i64> = row.get("max_executions");
    let execution_count: i32 = row.get("execution_count");

    Ok(Schedule {
        id: Uuid::parse_str(&id_str)?,
        name: row.get("name"),
        cron: row.get("cron"),
        workdir: row.get("workdir"),
        prompt: row.get("prompt"),
        provider: provider_str
            .parse::<Provider>()
            .map_err(|e| anyhow::anyhow!(e))?,
        mode: mode_str
            .parse::<SessionMode>()
            .map_err(|e| anyhow::anyhow!(e))?,
        guard_preset: row.get("guard_preset"),
        guard_config,
        model: row.get("model"),
        allowed_tools,
        system_prompt: row.get("system_prompt"),
        metadata,
        persona: row.get("persona"),
        max_turns: {
            let v: Option<i32> = row.get("max_turns");
            v.map(i32::cast_unsigned)
        },
        max_budget_usd: row.get("max_budget_usd"),
        output_format: row.get("output_format"),
        concurrency: concurrency_str
            .parse::<ConcurrencyPolicy>()
            .map_err(|e| anyhow::anyhow!(e))?,
        status: status_str
            .parse::<ScheduleStatus>()
            .map_err(|e| anyhow::anyhow!(e))?,
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        max_executions: max_executions.map(|v| v.clamp(0, i64::from(u32::MAX)) as u32),
        execution_count: execution_count.cast_unsigned(),
        last_run_at: parse_optional_datetime(row, "last_run_at")?,
        next_run_at: parse_optional_datetime(row, "next_run_at")?,
        last_session_id: parse_optional_uuid(row, "last_session_id")?,
        worktree,
        created_at: DateTime::parse_from_rfc3339(&created_str)?.with_timezone(&Utc),
        updated_at: DateTime::parse_from_rfc3339(&updated_str)?.with_timezone(&Utc),
    })
}

fn row_to_execution(row: &SqliteRow) -> Result<ScheduleExecution> {
    let schedule_id_str: String = row.get("schedule_id");
    let session_id_str: Option<String> = row.get("session_id");
    let status_str: String = row.get("status");
    let created_str: String = row.get("created_at");

    Ok(ScheduleExecution {
        id: row.get("id"),
        schedule_id: Uuid::parse_str(&schedule_id_str)?,
        session_id: session_id_str.map(|s| Uuid::parse_str(&s)).transpose()?,
        status: status_str
            .parse::<ExecutionStatus>()
            .map_err(|e| anyhow::anyhow!(e))?,
        error: row.get("error"),
        triggered_by: row.get("triggered_by"),
        created_at: DateTime::parse_from_rfc3339(&created_str)?.with_timezone(&Utc),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_session(name: &str) -> Session {
        Session {
            id: Uuid::new_v4(),
            name: name.into(),
            workdir: "/tmp/repo".into(),
            provider: Provider::Claude,
            prompt: "Fix the bug".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: Some("conv-123".into()),
            exit_code: None,
            tmux_session: Some(format!("pulpo-{name}")),
            output_snapshot: None,
            git_branch: Some("main".into()),
            git_sha: Some("abc123".into()),
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            recovery_count: 0,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    async fn test_store() -> Store {
        let tmpdir = tempfile::tempdir().unwrap();
        // Leak so it persists for test lifetime
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        store
    }

    #[tokio::test]
    async fn test_new_creates_directory() {
        let tmpdir = tempfile::tempdir().unwrap();
        let data_dir = tmpdir.path().join("nested/deep");
        let store = Store::new(data_dir.to_str().unwrap()).await.unwrap();
        assert!(data_dir.exists());
        drop(store);
    }

    #[tokio::test]
    async fn test_migrate_creates_sessions_table() {
        let tmpdir = tempfile::tempdir().unwrap();
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();

        // Verify table exists by running a query
        let result = sqlx::query("SELECT count(*) as cnt FROM sessions")
            .fetch_one(store.pool())
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_migrate_is_idempotent() {
        let tmpdir = tempfile::tempdir().unwrap();
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        // Running migrate again should not error
        store.migrate().await.unwrap();
    }

    #[tokio::test]
    async fn test_pool_returns_valid_pool() {
        let tmpdir = tempfile::tempdir().unwrap();
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        let pool = store.pool();
        // Verify pool works
        let row = sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(row, 1);
    }

    #[tokio::test]
    async fn test_insert_and_get_session() {
        let store = test_store().await;
        let session = make_session("test-roundtrip");

        store.insert_session(&session).await.unwrap();
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(fetched.id, session.id);
        assert_eq!(fetched.name, "test-roundtrip");
        assert_eq!(fetched.workdir, "/tmp/repo");
        assert_eq!(fetched.provider, Provider::Claude);
        assert_eq!(fetched.prompt, "Fix the bug");
        assert_eq!(fetched.status, SessionStatus::Running);
        assert_eq!(fetched.mode, SessionMode::Interactive);
        assert_eq!(fetched.conversation_id, Some("conv-123".into()));
        assert_eq!(fetched.exit_code, None);
        assert_eq!(fetched.tmux_session, Some("pulpo-test-roundtrip".into()));
        assert_eq!(fetched.git_branch, Some("main".into()));
        assert_eq!(fetched.git_sha, Some("abc123".into()));
    }

    #[tokio::test]
    async fn test_get_session_not_found() {
        let store = test_store().await;
        let result = store.get_session("nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_get_session_by_name() {
        let store = test_store().await;
        let session = make_session("lookup-by-name");
        store.insert_session(&session).await.unwrap();

        let fetched = store.get_session("lookup-by-name").await.unwrap().unwrap();
        assert_eq!(fetched.id, session.id);
        assert_eq!(fetched.name, "lookup-by-name");
    }

    #[tokio::test]
    async fn test_get_session_by_name_not_found() {
        let store = test_store().await;
        let session = make_session("existing");
        store.insert_session(&session).await.unwrap();

        let result = store.get_session("nonexistent-name").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_list_sessions_empty() {
        let store = test_store().await;
        let sessions = store.list_sessions().await.unwrap();
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn test_list_sessions_multiple() {
        let store = test_store().await;
        let s1 = make_session("first");
        let s2 = make_session("second");

        store.insert_session(&s1).await.unwrap();
        store.insert_session(&s2).await.unwrap();

        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[tokio::test]
    async fn test_update_session_status() {
        let store = test_store().await;
        let session = make_session("update-test");
        store.insert_session(&session).await.unwrap();

        store
            .update_session_status(&session.id.to_string(), SessionStatus::Completed)
            .await
            .unwrap();

        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Completed);
    }

    #[tokio::test]
    async fn test_update_session_conversation_id() {
        let store = test_store().await;
        let session = make_session("conv-test");
        store.insert_session(&session).await.unwrap();

        store
            .update_session_conversation_id(&session.id.to_string(), "conv-xyz")
            .await
            .unwrap();

        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.conversation_id, Some("conv-xyz".into()));
    }

    #[tokio::test]
    async fn test_update_session_conversation_id_after_table_dropped() {
        let store = test_store().await;
        sqlx::query("DROP TABLE sessions")
            .execute(store.pool())
            .await
            .unwrap();
        let result = store
            .update_session_conversation_id("test-id", "conv-abc")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_session() {
        let store = test_store().await;
        let session = make_session("delete-test");
        store.insert_session(&session).await.unwrap();

        store.delete_session(&session.id.to_string()).await.unwrap();

        let result = store.get_session(&session.id.to_string()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_insert_session_with_all_none_optionals() {
        let store = test_store().await;
        let session = Session {
            id: Uuid::new_v4(),
            name: "minimal".into(),
            workdir: "/tmp".into(),
            provider: Provider::Codex,
            prompt: "test".into(),
            status: SessionStatus::Creating,
            mode: SessionMode::Autonomous,
            conversation_id: None,
            exit_code: None,
            tmux_session: None,
            output_snapshot: None,
            git_branch: None,
            git_sha: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            recovery_count: 0,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        store.insert_session(&session).await.unwrap();
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(fetched.provider, Provider::Codex);
        assert_eq!(fetched.mode, SessionMode::Autonomous);
        assert!(fetched.conversation_id.is_none());
        assert!(fetched.exit_code.is_none());
        assert!(fetched.tmux_session.is_none());
        assert!(fetched.output_snapshot.is_none());
        assert!(fetched.git_branch.is_none());
        assert!(fetched.git_sha.is_none());
    }

    #[tokio::test]
    async fn test_insert_session_with_guardrail_fields() {
        let store = test_store().await;
        let mut session = make_session("guardrailed");
        session.max_turns = Some(10);
        session.max_budget_usd = Some(5.5);
        session.output_format = Some("json".into());

        store.insert_session(&session).await.unwrap();
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(fetched.max_turns, Some(10));
        assert_eq!(fetched.max_budget_usd, Some(5.5));
        assert_eq!(fetched.output_format.as_deref(), Some("json"));
    }

    const TEST_UUID: &str = "550e8400-e29b-41d4-a716-446655440000";

    #[tokio::test]
    async fn test_row_to_session_invalid_provider() {
        let store = test_store().await;
        sqlx::query(
            "INSERT INTO sessions (id, name, workdir, provider, prompt, status, mode,
                created_at, updated_at)
             VALUES (?, 'test', '/tmp', 'invalid_provider', 'test', 'running', 'interactive',
                '2024-01-01T00:00:00+00:00', '2024-01-01T00:00:00+00:00')",
        )
        .bind(TEST_UUID)
        .execute(store.pool())
        .await
        .unwrap();
        let result = store.get_session(TEST_UUID).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_row_to_session_invalid_status() {
        let store = test_store().await;
        sqlx::query(
            "INSERT INTO sessions (id, name, workdir, provider, prompt, status, mode,
                created_at, updated_at)
             VALUES (?, 'test', '/tmp', 'claude', 'test', 'bad_status', 'interactive',
                '2024-01-01T00:00:00+00:00', '2024-01-01T00:00:00+00:00')",
        )
        .bind(TEST_UUID)
        .execute(store.pool())
        .await
        .unwrap();
        let result = store.get_session(TEST_UUID).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_row_to_session_invalid_mode() {
        let store = test_store().await;
        sqlx::query(
            "INSERT INTO sessions (id, name, workdir, provider, prompt, status, mode,
                created_at, updated_at)
             VALUES (?, 'test', '/tmp', 'claude', 'test', 'running', 'bad_mode',
                '2024-01-01T00:00:00+00:00', '2024-01-01T00:00:00+00:00')",
        )
        .bind(TEST_UUID)
        .execute(store.pool())
        .await
        .unwrap();
        let result = store.get_session(TEST_UUID).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_row_to_session_invalid_uuid() {
        let store = test_store().await;
        sqlx::query(
            "INSERT INTO sessions (id, name, workdir, provider, prompt, status, mode,
                created_at, updated_at)
             VALUES ('not-a-uuid', 'test', '/tmp', 'claude', 'test', 'running', 'interactive',
                '2024-01-01T00:00:00+00:00', '2024-01-01T00:00:00+00:00')",
        )
        .execute(store.pool())
        .await
        .unwrap();
        let result = store.get_session("not-a-uuid").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_row_to_session_invalid_datetime() {
        let store = test_store().await;
        sqlx::query(
            "INSERT INTO sessions (id, name, workdir, provider, prompt, status, mode,
                created_at, updated_at)
             VALUES (?, 'test', '/tmp', 'claude', 'test', 'running', 'interactive',
                'not-a-date', '2024-01-01T00:00:00+00:00')",
        )
        .bind(TEST_UUID)
        .execute(store.pool())
        .await
        .unwrap();
        let result = store.get_session(TEST_UUID).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_sessions_after_table_dropped() {
        let store = test_store().await;
        sqlx::query("DROP TABLE sessions")
            .execute(store.pool())
            .await
            .unwrap();
        let result = store.list_sessions().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_session_after_table_dropped() {
        let store = test_store().await;
        sqlx::query("DROP TABLE sessions")
            .execute(store.pool())
            .await
            .unwrap();
        let result = store.get_session("test-id").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_insert_session_after_table_dropped() {
        let store = test_store().await;
        sqlx::query("DROP TABLE sessions")
            .execute(store.pool())
            .await
            .unwrap();
        let session = make_session("fail-test");
        let result = store.insert_session(&session).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_update_session_status_after_table_dropped() {
        let store = test_store().await;
        sqlx::query("DROP TABLE sessions")
            .execute(store.pool())
            .await
            .unwrap();
        let result = store
            .update_session_status("test-id", SessionStatus::Dead)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_session_after_table_dropped() {
        let store = test_store().await;
        sqlx::query("DROP TABLE sessions")
            .execute(store.pool())
            .await
            .unwrap();
        let result = store.delete_session("test-id").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_store_is_clone() {
        let store = test_store().await;
        let cloned = store.clone();
        // Both should work
        let sessions = cloned.list_sessions().await.unwrap();
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn test_guard_config_roundtrip() {
        use pulpo_common::guard::{GuardConfig, GuardPreset};

        let store = test_store().await;
        let mut session = make_session("guard-test");
        session.guard_config = Some(GuardConfig::from_preset(GuardPreset::Strict));

        store.insert_session(&session).await.unwrap();
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();

        assert!(fetched.guard_config.is_some());
        let gc = fetched.guard_config.unwrap();
        assert_eq!(gc, GuardConfig::from_preset(GuardPreset::Strict));
    }

    #[tokio::test]
    async fn test_guard_config_none_roundtrip() {
        let store = test_store().await;
        let session = make_session("no-guard");

        store.insert_session(&session).await.unwrap();
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();

        assert!(fetched.guard_config.is_none());
    }

    #[tokio::test]
    async fn test_guard_config_with_env_roundtrip() {
        use pulpo_common::guard::{EnvFilter, GuardConfig, GuardPreset};

        let store = test_store().await;
        let mut session = make_session("guard-env-test");
        let mut config = GuardConfig::from_preset(GuardPreset::Standard);
        config.env = EnvFilter {
            allow: vec!["PATH".into(), "HOME".into()],
            deny: vec!["AWS_*".into()],
        };
        session.guard_config = Some(config.clone());

        store.insert_session(&session).await.unwrap();
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(fetched.guard_config.unwrap(), config);
    }

    #[tokio::test]
    async fn test_migrate_adds_guard_config_column() {
        let tmpdir = tempfile::tempdir().unwrap();
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();

        // First migration creates table
        store.migrate().await.unwrap();

        // Second call is idempotent — column already exists
        store.migrate().await.unwrap();

        // Verify column exists by inserting with guard_config
        let session = make_session("migration-test");
        store.insert_session(&session).await.unwrap();
    }

    #[tokio::test]
    async fn test_row_to_session_invalid_guard_json() {
        let store = test_store().await;
        sqlx::query(
            "INSERT INTO sessions (id, name, workdir, provider, prompt, status, mode,
                guard_config, created_at, updated_at)
             VALUES (?, 'test', '/tmp', 'claude', 'test', 'running', 'interactive',
                'not-valid-json', '2024-01-01T00:00:00+00:00', '2024-01-01T00:00:00+00:00')",
        )
        .bind(TEST_UUID)
        .execute(store.pool())
        .await
        .unwrap();
        let result = store.get_session(TEST_UUID).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_data_dir_accessor() {
        let store = test_store().await;
        let dir = store.data_dir();
        assert!(!dir.is_empty());
    }

    #[tokio::test]
    async fn test_list_sessions_filtered_by_status() {
        let store = test_store().await;
        let mut s1 = make_session("running-1");
        s1.status = SessionStatus::Running;
        let mut s2 = make_session("completed-1");
        s2.status = SessionStatus::Completed;
        store.insert_session(&s1).await.unwrap();
        store.insert_session(&s2).await.unwrap();

        let query = ListSessionsQuery {
            status: Some("running".into()),
            ..Default::default()
        };
        let sessions = store.list_sessions_filtered(&query).await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].status, SessionStatus::Running);
    }

    #[tokio::test]
    async fn test_list_sessions_filtered_by_multiple_statuses() {
        let store = test_store().await;
        let mut s1 = make_session("running-2");
        s1.status = SessionStatus::Running;
        let mut s2 = make_session("completed-2");
        s2.status = SessionStatus::Completed;
        let mut s3 = make_session("dead-1");
        s3.status = SessionStatus::Dead;
        store.insert_session(&s1).await.unwrap();
        store.insert_session(&s2).await.unwrap();
        store.insert_session(&s3).await.unwrap();

        let query = ListSessionsQuery {
            status: Some("running,completed".into()),
            ..Default::default()
        };
        let sessions = store.list_sessions_filtered(&query).await.unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[tokio::test]
    async fn test_list_sessions_filtered_by_provider() {
        let store = test_store().await;
        let s1 = make_session("claude-task");
        let mut s2 = make_session("codex-task");
        s2.provider = Provider::Codex;
        store.insert_session(&s1).await.unwrap();
        store.insert_session(&s2).await.unwrap();

        let query = ListSessionsQuery {
            provider: Some("codex".into()),
            ..Default::default()
        };
        let sessions = store.list_sessions_filtered(&query).await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].provider, Provider::Codex);
    }

    #[tokio::test]
    async fn test_list_sessions_filtered_by_search() {
        let store = test_store().await;
        let mut s1 = make_session("api-fix");
        s1.prompt = "Fix the API endpoint".into();
        let mut s2 = make_session("ui-refactor");
        s2.prompt = "Refactor the UI components".into();
        store.insert_session(&s1).await.unwrap();
        store.insert_session(&s2).await.unwrap();

        let query = ListSessionsQuery {
            search: Some("API".into()),
            ..Default::default()
        };
        let sessions = store.list_sessions_filtered(&query).await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "api-fix");
    }

    #[tokio::test]
    async fn test_list_sessions_filtered_search_by_name() {
        let store = test_store().await;
        let s1 = make_session("frontend-fix");
        let s2 = make_session("backend-fix");
        store.insert_session(&s1).await.unwrap();
        store.insert_session(&s2).await.unwrap();

        let query = ListSessionsQuery {
            search: Some("frontend".into()),
            ..Default::default()
        };
        let sessions = store.list_sessions_filtered(&query).await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "frontend-fix");
    }

    #[tokio::test]
    async fn test_list_sessions_filtered_sort_by_name() {
        let store = test_store().await;
        let s1 = make_session("aaa");
        let s2 = make_session("zzz");
        store.insert_session(&s1).await.unwrap();
        store.insert_session(&s2).await.unwrap();

        let query = ListSessionsQuery {
            sort: Some("name".into()),
            order: Some("asc".into()),
            ..Default::default()
        };
        let sessions = store.list_sessions_filtered(&query).await.unwrap();
        assert_eq!(sessions[0].name, "aaa");
        assert_eq!(sessions[1].name, "zzz");
    }

    #[tokio::test]
    async fn test_list_sessions_filtered_sort_desc() {
        let store = test_store().await;
        let s1 = make_session("aaa");
        let s2 = make_session("zzz");
        store.insert_session(&s1).await.unwrap();
        store.insert_session(&s2).await.unwrap();

        let query = ListSessionsQuery {
            sort: Some("name".into()),
            order: Some("desc".into()),
            ..Default::default()
        };
        let sessions = store.list_sessions_filtered(&query).await.unwrap();
        assert_eq!(sessions[0].name, "zzz");
        assert_eq!(sessions[1].name, "aaa");
    }

    #[tokio::test]
    async fn test_list_sessions_filtered_empty_returns_all() {
        let store = test_store().await;
        let s1 = make_session("one");
        let s2 = make_session("two");
        store.insert_session(&s1).await.unwrap();
        store.insert_session(&s2).await.unwrap();

        let query = ListSessionsQuery::default();
        let sessions = store.list_sessions_filtered(&query).await.unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[tokio::test]
    async fn test_list_sessions_filtered_combined_filters() {
        let store = test_store().await;
        let mut s1 = make_session("api-fix");
        s1.status = SessionStatus::Running;
        s1.prompt = "Fix the API".into();
        let mut s2 = make_session("api-refactor");
        s2.status = SessionStatus::Completed;
        s2.prompt = "Refactor the API".into();
        let mut s3 = make_session("ui-fix");
        s3.status = SessionStatus::Running;
        s3.prompt = "Fix the UI".into();
        store.insert_session(&s1).await.unwrap();
        store.insert_session(&s2).await.unwrap();
        store.insert_session(&s3).await.unwrap();

        let query = ListSessionsQuery {
            status: Some("running".into()),
            search: Some("API".into()),
            ..Default::default()
        };
        let sessions = store.list_sessions_filtered(&query).await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "api-fix");
    }

    #[tokio::test]
    async fn test_list_sessions_filtered_sort_by_status() {
        let store = test_store().await;
        let mut s1 = make_session("first");
        s1.status = SessionStatus::Running;
        let mut s2 = make_session("second");
        s2.status = SessionStatus::Completed;
        store.insert_session(&s1).await.unwrap();
        store.insert_session(&s2).await.unwrap();

        let query = ListSessionsQuery {
            sort: Some("status".into()),
            order: Some("asc".into()),
            ..Default::default()
        };
        let sessions = store.list_sessions_filtered(&query).await.unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[tokio::test]
    async fn test_list_sessions_filtered_sort_by_provider() {
        let store = test_store().await;
        let s1 = make_session("claude-task");
        let mut s2 = make_session("codex-task");
        s2.provider = Provider::Codex;
        store.insert_session(&s1).await.unwrap();
        store.insert_session(&s2).await.unwrap();

        let query = ListSessionsQuery {
            sort: Some("provider".into()),
            order: Some("asc".into()),
            ..Default::default()
        };
        let sessions = store.list_sessions_filtered(&query).await.unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[tokio::test]
    async fn test_update_session_intervention() {
        let store = test_store().await;
        let session = make_session("intervene-test");
        store.insert_session(&session).await.unwrap();

        store
            .update_session_intervention(&session.id.to_string(), "Memory usage 95% (512MB/8192MB)")
            .await
            .unwrap();

        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Dead);
        assert_eq!(
            fetched.intervention_reason.as_deref(),
            Some("Memory usage 95% (512MB/8192MB)")
        );
        assert!(fetched.intervention_at.is_some());
    }

    #[tokio::test]
    async fn test_update_session_intervention_after_table_dropped() {
        let store = test_store().await;
        sqlx::query("DROP TABLE sessions")
            .execute(store.pool())
            .await
            .unwrap();
        let result = store.update_session_intervention("test-id", "reason").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_clear_session_intervention() {
        let store = test_store().await;
        let session = make_session("clear-test");
        store.insert_session(&session).await.unwrap();

        // Set intervention first
        store
            .update_session_intervention(&session.id.to_string(), "test reason")
            .await
            .unwrap();

        // Clear it
        store
            .clear_session_intervention(&session.id.to_string())
            .await
            .unwrap();

        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.intervention_reason.is_none());
        assert!(fetched.intervention_at.is_none());
    }

    #[tokio::test]
    async fn test_clear_session_intervention_after_table_dropped() {
        let store = test_store().await;
        sqlx::query("DROP TABLE sessions")
            .execute(store.pool())
            .await
            .unwrap();
        let result = store.clear_session_intervention("test-id").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_update_session_output_snapshot() {
        let store = test_store().await;
        let session = make_session("snapshot-test");
        store.insert_session(&session).await.unwrap();

        store
            .update_session_output_snapshot(
                &session.id.to_string(),
                "$ vitest\nrunning tests...\nOOM killed",
            )
            .await
            .unwrap();

        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            fetched.output_snapshot.as_deref(),
            Some("$ vitest\nrunning tests...\nOOM killed")
        );
    }

    #[tokio::test]
    async fn test_update_session_output_snapshot_after_table_dropped() {
        let store = test_store().await;
        sqlx::query("DROP TABLE sessions")
            .execute(store.pool())
            .await
            .unwrap();
        let result = store
            .update_session_output_snapshot("test-id", "snapshot")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_intervention_roundtrip_with_insert() {
        let store = test_store().await;
        let mut session = make_session("intervention-insert");
        session.intervention_reason = Some("pre-set reason".into());
        session.intervention_at = Some(Utc::now());

        store.insert_session(&session).await.unwrap();
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            fetched.intervention_reason.as_deref(),
            Some("pre-set reason")
        );
        assert!(fetched.intervention_at.is_some());
    }

    #[tokio::test]
    async fn test_row_to_session_invalid_intervention_at() {
        let store = test_store().await;
        sqlx::query(
            "INSERT INTO sessions (id, name, workdir, provider, prompt, status, mode,
                intervention_at, created_at, updated_at)
             VALUES (?, 'test', '/tmp', 'claude', 'test', 'running', 'interactive',
                'not-a-date', '2024-01-01T00:00:00+00:00', '2024-01-01T00:00:00+00:00')",
        )
        .bind(TEST_UUID)
        .execute(store.pool())
        .await
        .unwrap();
        let result = store.get_session(TEST_UUID).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_sessions_filtered_after_table_dropped() {
        let store = test_store().await;
        sqlx::query("DROP TABLE sessions")
            .execute(store.pool())
            .await
            .unwrap();
        let query = ListSessionsQuery::default();
        let result = store.list_sessions_filtered(&query).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_intervention_events_appended() {
        let store = test_store().await;
        let session = make_session("events-test");
        store.insert_session(&session).await.unwrap();
        let sid = session.id.to_string();

        // First intervention
        store
            .update_session_intervention(&sid, "Memory 95%")
            .await
            .unwrap();

        // Simulate a second intervention (e.g., session was resumed and hit pressure again)
        // Reset session to running first so the scenario makes sense
        sqlx::query("UPDATE sessions SET status = 'running' WHERE id = ?")
            .bind(&sid)
            .execute(store.pool())
            .await
            .unwrap();
        store
            .update_session_intervention(&sid, "Memory 98%")
            .await
            .unwrap();

        let events = store.list_intervention_events(&sid).await.unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].reason, "Memory 95%");
        assert_eq!(events[1].reason, "Memory 98%");
        assert_eq!(events[0].session_id, sid);
        assert_eq!(events[1].session_id, sid);
        assert!(events[0].id < events[1].id);
    }

    #[tokio::test]
    async fn test_intervention_events_empty_for_unknown_session() {
        let store = test_store().await;
        let events = store
            .list_intervention_events("nonexistent-id")
            .await
            .unwrap();
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn test_intervention_events_after_table_dropped() {
        let store = test_store().await;
        sqlx::query("DROP TABLE intervention_events")
            .execute(store.pool())
            .await
            .unwrap();
        let result = store.list_intervention_events("any-id").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_intervention_event_debug_clone() {
        let event = InterventionEvent {
            id: 1,
            session_id: "test-id".into(),
            reason: "Memory 95%".into(),
            created_at: Utc::now(),
        };
        let debug = format!("{event:?}");
        assert!(debug.contains("Memory 95%"));
        #[allow(clippy::redundant_clone)]
        let cloned = event.clone();
        assert_eq!(cloned.reason, "Memory 95%");
    }

    #[tokio::test]
    async fn test_recovery_count_roundtrip() {
        let store = test_store().await;
        let mut session = make_session("recovery-test");
        session.recovery_count = 3;

        store.insert_session(&session).await.unwrap();
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(fetched.recovery_count, 3);
    }

    #[tokio::test]
    async fn test_increment_recovery_count() {
        let store = test_store().await;
        let session = make_session("inc-recovery");

        store.insert_session(&session).await.unwrap();
        let id = session.id.to_string();

        let count = store.increment_recovery_count(&id).await.unwrap();
        assert_eq!(count, 1);

        let count = store.increment_recovery_count(&id).await.unwrap();
        assert_eq!(count, 2);

        let fetched = store.get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.recovery_count, 2);
    }

    #[tokio::test]
    async fn test_increment_recovery_count_after_table_dropped() {
        let store = test_store().await;
        sqlx::query("DROP TABLE sessions")
            .execute(store.pool())
            .await
            .unwrap();
        let result = store.increment_recovery_count("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_resume_dead_session() {
        let store = test_store().await;
        let session = make_session("resume-test");
        let id = session.id.to_string();

        store.insert_session(&session).await.unwrap();
        store
            .update_session_intervention(&id, "Memory 95%")
            .await
            .unwrap();

        let fetched = store.get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Dead);
        assert!(fetched.intervention_reason.is_some());
        assert!(fetched.intervention_at.is_some());

        store.resume_dead_session(&id).await.unwrap();

        let fetched = store.get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Running);
        assert!(fetched.intervention_reason.is_none());
        assert!(fetched.intervention_at.is_none());
    }

    #[tokio::test]
    async fn test_resume_dead_session_after_table_dropped() {
        let store = test_store().await;
        sqlx::query("DROP TABLE sessions")
            .execute(store.pool())
            .await
            .unwrap();
        let result = store.resume_dead_session("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_last_output_at_updated_on_change() {
        let store = test_store().await;
        let session = make_session("output-ts");
        let id = session.id.to_string();
        store.insert_session(&session).await.unwrap();

        // Initially null
        let fetched = store.get_session(&id).await.unwrap().unwrap();
        assert!(fetched.last_output_at.is_none());

        // First snapshot — sets last_output_at
        store
            .update_session_output_snapshot(&id, "hello")
            .await
            .unwrap();
        let fetched = store.get_session(&id).await.unwrap().unwrap();
        assert!(fetched.last_output_at.is_some());
        let ts1 = fetched.last_output_at.unwrap();

        // Different content — updates last_output_at
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        store
            .update_session_output_snapshot(&id, "world")
            .await
            .unwrap();
        let fetched = store.get_session(&id).await.unwrap().unwrap();
        let ts2 = fetched.last_output_at.unwrap();
        assert!(ts2 > ts1);
    }

    #[tokio::test]
    async fn test_last_output_at_not_updated_on_same() {
        let store = test_store().await;
        let session = make_session("output-same");
        let id = session.id.to_string();
        store.insert_session(&session).await.unwrap();

        // Set initial snapshot
        store
            .update_session_output_snapshot(&id, "same content")
            .await
            .unwrap();
        let fetched = store.get_session(&id).await.unwrap().unwrap();
        let ts1 = fetched.last_output_at.unwrap();

        // Same content — last_output_at should NOT change
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        store
            .update_session_output_snapshot(&id, "same content")
            .await
            .unwrap();
        let fetched = store.get_session(&id).await.unwrap().unwrap();
        let ts2 = fetched.last_output_at.unwrap();
        assert_eq!(ts1, ts2);
    }

    #[tokio::test]
    async fn test_get_session_invalid_last_output_at() {
        let store = test_store().await;
        let session = make_session("bad-ts");
        store.insert_session(&session).await.unwrap();

        sqlx::query("UPDATE sessions SET last_output_at = 'not-a-date' WHERE id = ?")
            .bind(session.id.to_string())
            .execute(store.pool())
            .await
            .unwrap();

        let result = store.get_session(&session.id.to_string()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_session_invalid_updated_at() {
        let store = test_store().await;
        let session = make_session("bad-updated");
        store.insert_session(&session).await.unwrap();

        sqlx::query("UPDATE sessions SET updated_at = 'not-a-date' WHERE id = ?")
            .bind(session.id.to_string())
            .execute(store.pool())
            .await
            .unwrap();

        let result = store.get_session(&session.id.to_string()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_insert_session_with_last_output_at() {
        let store = test_store().await;
        let mut session = make_session("with-output-ts");
        session.last_output_at = Some(Utc::now());
        store.insert_session(&session).await.unwrap();

        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.last_output_at.is_some());
    }

    #[tokio::test]
    async fn test_get_session_invalid_uuid() {
        let store = test_store().await;
        let session = make_session("bad-uuid");
        store.insert_session(&session).await.unwrap();

        sqlx::query("UPDATE sessions SET id = 'not-a-uuid' WHERE id = ?")
            .bind(session.id.to_string())
            .execute(store.pool())
            .await
            .unwrap();

        let result = store.get_session("not-a-uuid").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_intervention_events_invalid_created_at() {
        let store = test_store().await;
        let session = make_session("bad-event");
        store.insert_session(&session).await.unwrap();

        // Insert event with invalid timestamp directly
        sqlx::query(
            "INSERT INTO intervention_events (session_id, reason, created_at) VALUES (?, ?, ?)",
        )
        .bind(session.id.to_string())
        .bind("test")
        .bind("not-a-date")
        .execute(store.pool())
        .await
        .unwrap();

        let result = store
            .list_intervention_events(&session.id.to_string())
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_update_session_idle_since() {
        let store = test_store().await;
        let session = make_session("idle-test");
        store.insert_session(&session).await.unwrap();

        // Initially None
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.idle_since.is_none());

        // Set idle_since
        store
            .update_session_idle_since(&session.id.to_string())
            .await
            .unwrap();
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.idle_since.is_some());
    }

    #[tokio::test]
    async fn test_clear_session_idle_since() {
        let store = test_store().await;
        let session = make_session("idle-clear");
        store.insert_session(&session).await.unwrap();

        // Set idle_since
        store
            .update_session_idle_since(&session.id.to_string())
            .await
            .unwrap();
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.idle_since.is_some());

        // Clear idle_since
        store
            .clear_session_idle_since(&session.id.to_string())
            .await
            .unwrap();
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.idle_since.is_none());
    }

    #[tokio::test]
    async fn test_insert_session_with_idle_since() {
        let store = test_store().await;
        let mut session = make_session("with-idle");
        session.idle_since = Some(Utc::now());
        store.insert_session(&session).await.unwrap();

        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.idle_since.is_some());
    }

    #[tokio::test]
    async fn test_get_session_invalid_idle_since() {
        let store = test_store().await;
        let session = make_session("bad-idle");
        store.insert_session(&session).await.unwrap();

        sqlx::query("UPDATE sessions SET idle_since = 'not-a-date' WHERE id = ?")
            .bind(session.id.to_string())
            .execute(store.pool())
            .await
            .unwrap();

        let result = store.get_session(&session.id.to_string()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_insert_detection_event() {
        let store = test_store().await;
        let id = store
            .insert_detection_event("sess-1", "memory", "kill")
            .await
            .unwrap();
        assert!(id > 0);

        let id2 = store
            .insert_detection_event("sess-1", "idle", "alert")
            .await
            .unwrap();
        assert!(id2 > id);
    }

    #[tokio::test]
    async fn test_list_detection_events_all() {
        let store = test_store().await;
        store
            .insert_detection_event("sess-1", "memory", "kill")
            .await
            .unwrap();
        store
            .insert_detection_event("sess-2", "idle", "alert")
            .await
            .unwrap();

        let all = store.list_detection_events(None).await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].detector, "memory");
        assert_eq!(all[1].detector, "idle");
    }

    #[tokio::test]
    async fn test_list_detection_events_filtered() {
        let store = test_store().await;
        store
            .insert_detection_event("sess-1", "memory", "kill")
            .await
            .unwrap();
        store
            .insert_detection_event("sess-2", "idle", "alert")
            .await
            .unwrap();

        let memory_only = store.list_detection_events(Some("memory")).await.unwrap();
        assert_eq!(memory_only.len(), 1);
        assert_eq!(memory_only[0].detector, "memory");
        assert_eq!(memory_only[0].action, "kill");
        assert!(!memory_only[0].was_false_positive);

        let idle_only = store.list_detection_events(Some("idle")).await.unwrap();
        assert_eq!(idle_only.len(), 1);
        assert_eq!(idle_only[0].detector, "idle");
    }

    #[tokio::test]
    async fn test_mark_detection_false_positive() {
        let store = test_store().await;
        let id = store
            .insert_detection_event("sess-1", "memory", "kill")
            .await
            .unwrap();

        // Initially not false positive
        let events = store.list_detection_events(None).await.unwrap();
        assert!(!events[0].was_false_positive);

        // Mark as false positive
        let found = store.mark_detection_false_positive(id).await.unwrap();
        assert!(found);

        // Now it should be marked
        let events = store.list_detection_events(None).await.unwrap();
        assert!(events[0].was_false_positive);
    }

    #[tokio::test]
    async fn test_mark_detection_false_positive_not_found() {
        let store = test_store().await;
        let found = store.mark_detection_false_positive(999).await.unwrap();
        assert!(!found);
    }

    #[tokio::test]
    async fn test_detection_event_stats_empty() {
        let store = test_store().await;
        let stats = store.detection_event_stats().await.unwrap();
        assert!(stats.is_empty());
    }

    #[tokio::test]
    async fn test_detection_event_stats() {
        let store = test_store().await;
        let id1 = store
            .insert_detection_event("s1", "memory", "kill")
            .await
            .unwrap();
        store
            .insert_detection_event("s2", "memory", "kill")
            .await
            .unwrap();
        store
            .insert_detection_event("s3", "idle", "alert")
            .await
            .unwrap();

        // Mark one memory event as false positive
        store.mark_detection_false_positive(id1).await.unwrap();

        let stats = store.detection_event_stats().await.unwrap();
        assert_eq!(stats.len(), 2);

        let mem = &stats["memory"];
        assert_eq!(mem.total, 2);
        assert_eq!(mem.false_positives, 1);
        assert!((mem.rate - 0.5).abs() < f64::EPSILON);

        let idle = &stats["idle"];
        assert_eq!(idle.total, 1);
        assert_eq!(idle.false_positives, 0);
        assert!((idle.rate - 0.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_detection_event_fields() {
        let store = test_store().await;
        let id = store
            .insert_detection_event("sess-abc", "memory", "kill")
            .await
            .unwrap();

        let events = store.list_detection_events(None).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, id);
        assert_eq!(events[0].session_id, "sess-abc");
        assert_eq!(events[0].detector, "memory");
        assert_eq!(events[0].action, "kill");
        assert!(!events[0].was_false_positive);
        // created_at should be a valid timestamp
        assert!(events[0].created_at.to_rfc3339().contains('T'));
    }

    #[tokio::test]
    async fn test_detection_event_debug_clone() {
        let store = test_store().await;
        store
            .insert_detection_event("s1", "memory", "kill")
            .await
            .unwrap();
        let events = store.list_detection_events(None).await.unwrap();
        let event = &events[0];
        let cloned = event.clone();
        assert_eq!(format!("{event:?}"), format!("{cloned:?}"));
    }

    #[tokio::test]
    async fn test_new_session_fields_roundtrip() {
        let store = test_store().await;
        let mut session = make_session("new-fields-test");
        session.model = Some("opus".into());
        session.allowed_tools = Some(vec!["Read".into(), "Write".into(), "Bash".into()]);
        session.system_prompt = Some("You are a code reviewer.".into());
        session.metadata = Some(
            [
                ("discord_channel".into(), "123".into()),
                ("user".into(), "alice".into()),
            ]
            .into_iter()
            .collect(),
        );
        session.persona = Some("reviewer".into());

        store.insert_session(&session).await.unwrap();
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(fetched.model, Some("opus".into()));
        assert_eq!(
            fetched.allowed_tools,
            Some(vec!["Read".into(), "Write".into(), "Bash".into()])
        );
        assert_eq!(
            fetched.system_prompt,
            Some("You are a code reviewer.".into())
        );
        let meta = fetched.metadata.unwrap();
        assert_eq!(meta.get("discord_channel").unwrap(), "123");
        assert_eq!(meta.get("user").unwrap(), "alice");
        assert_eq!(fetched.persona, Some("reviewer".into()));
    }

    #[tokio::test]
    async fn test_migrate_renames_repo_path_to_workdir() {
        let tmpdir = tempfile::tempdir().unwrap();
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();

        // Create legacy schema with repo_path column
        sqlx::query(
            "CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                repo_path TEXT NOT NULL,
                provider TEXT NOT NULL,
                prompt TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'creating',
                mode TEXT NOT NULL DEFAULT 'interactive',
                conversation_id TEXT,
                exit_code INTEGER,
                tmux_session TEXT,
                docker_container TEXT,
                output_snapshot TEXT,
                git_branch TEXT,
                git_sha TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .execute(store.pool())
        .await
        .unwrap();

        // Insert a row using the old column name
        sqlx::query(
            "INSERT INTO sessions (id, name, repo_path, provider, prompt, status, mode, created_at, updated_at)
             VALUES ('test-id', 'test', '/old/path', 'claude', 'test', 'running', 'interactive',
                     '2024-01-01T00:00:00+00:00', '2024-01-01T00:00:00+00:00')",
        )
        .execute(store.pool())
        .await
        .unwrap();

        // Run migration — should rename column
        store.migrate().await.unwrap();

        // Verify we can query the new column name
        let workdir: String =
            sqlx::query_scalar("SELECT workdir FROM sessions WHERE id = 'test-id'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(workdir, "/old/path");

        // Running migration again should be idempotent
        store.migrate().await.unwrap();
    }

    #[tokio::test]
    async fn test_migrate_adds_schedule_guardrail_columns() {
        let tmpdir = tempfile::tempdir().unwrap();
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();

        // Create schedules table WITHOUT the new guardrail columns (old schema)
        sqlx::query(
            "CREATE TABLE schedules (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                cron TEXT NOT NULL,
                workdir TEXT NOT NULL,
                prompt TEXT NOT NULL,
                provider TEXT NOT NULL DEFAULT 'claude',
                mode TEXT NOT NULL DEFAULT 'interactive',
                guard_preset TEXT,
                guard_config TEXT,
                model TEXT,
                allowed_tools TEXT,
                system_prompt TEXT,
                metadata TEXT,
                persona TEXT,
                concurrency TEXT NOT NULL DEFAULT 'skip',
                status TEXT NOT NULL DEFAULT 'active',
                max_executions INTEGER,
                execution_count INTEGER NOT NULL DEFAULT 0,
                last_run_at TEXT,
                next_run_at TEXT,
                last_session_id TEXT,
                worktree TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .execute(store.pool())
        .await
        .unwrap();

        // Run migration — should add max_turns, max_budget_usd, output_format
        store.migrate().await.unwrap();

        // Verify each guardrail column was added via pragma (avoids pool
        // schema-cache issues that can occur with SELECT * after ALTER TABLE)
        for col in &["max_turns", "max_budget_usd", "output_format"] {
            let count: i32 = sqlx::query_scalar(&format!(
                "SELECT COUNT(*) FROM pragma_table_info('schedules') WHERE name = '{col}'"
            ))
            .fetch_one(store.pool())
            .await
            .unwrap();
            assert_eq!(count, 1, "column {col} should exist after migration");
        }
    }

    #[tokio::test]
    async fn test_detector_stats_debug_clone() {
        let stats = DetectorStats {
            total: 5,
            false_positives: 1,
            rate: 0.2,
        };
        let cloned = stats.clone();
        assert_eq!(format!("{stats:?}"), format!("{cloned:?}"));
    }

    // --- Schedule store tests ---

    fn make_schedule(name: &str) -> Schedule {
        Schedule {
            id: Uuid::new_v4(),
            name: name.into(),
            cron: "0 2 * * *".into(),
            workdir: "/tmp/repo".into(),
            prompt: "Review code".into(),
            provider: Provider::Claude,
            mode: SessionMode::Autonomous,
            guard_preset: Some("standard".into()),
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            concurrency: ConcurrencyPolicy::Skip,
            status: ScheduleStatus::Active,
            max_executions: None,
            execution_count: 0,
            last_run_at: None,
            next_run_at: Some(Utc::now()),
            last_session_id: None,
            worktree: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_schedule_insert_and_get() {
        let store = test_store().await;
        let schedule = make_schedule("nightly-review");
        store.insert_schedule(&schedule).await.unwrap();

        let fetched = store.get_schedule(&schedule.id.to_string()).await.unwrap();
        assert!(fetched.is_some());
        let s = fetched.unwrap();
        assert_eq!(s.name, "nightly-review");
        assert_eq!(s.cron, "0 2 * * *");
        assert_eq!(s.provider, Provider::Claude);
        assert_eq!(s.mode, SessionMode::Autonomous);
        assert_eq!(s.concurrency, ConcurrencyPolicy::Skip);
        assert_eq!(s.status, ScheduleStatus::Active);
    }

    #[tokio::test]
    async fn test_schedule_insert_with_guardrail_fields() {
        let store = test_store().await;
        let mut schedule = make_schedule("guarded-sched");
        schedule.max_turns = Some(5);
        schedule.max_budget_usd = Some(2.5);
        schedule.output_format = Some("stream-json".into());
        store.insert_schedule(&schedule).await.unwrap();

        let fetched = store
            .get_schedule(&schedule.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.max_turns, Some(5));
        assert_eq!(fetched.max_budget_usd, Some(2.5));
        assert_eq!(fetched.output_format.as_deref(), Some("stream-json"));
    }

    #[tokio::test]
    #[allow(clippy::literal_string_with_formatting_args)]
    async fn test_schedule_roundtrip_with_optional_json_fields() {
        let store = test_store().await;
        let mut schedule = make_schedule("full-fields");
        schedule.guard_config = Some(GuardConfig::default());
        schedule.allowed_tools = Some(vec!["bash".into(), "read".into()]);
        schedule.metadata = Some(std::iter::once(("key".into(), "val".into())).collect());
        schedule.worktree = Some(WorktreeConfig {
            branch: Some("main".into()),
            new_branch: Some("{schedule}-{date}".into()),
            cleanup: pulpo_common::schedule::WorktreeCleanup::OnComplete,
        });
        store.insert_schedule(&schedule).await.unwrap();

        let fetched = store
            .get_schedule(&schedule.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.guard_config.is_some());
        assert_eq!(fetched.allowed_tools.unwrap().len(), 2);
        assert_eq!(fetched.metadata.unwrap()["key"], "val");
        assert!(fetched.worktree.is_some());
    }

    #[tokio::test]
    async fn test_row_to_schedule_invalid_provider() {
        let store = test_store().await;
        let schedule = make_schedule("bad-provider");
        store.insert_schedule(&schedule).await.unwrap();
        // Corrupt the provider field
        sqlx::query("UPDATE schedules SET provider = 'INVALID' WHERE id = ?")
            .bind(schedule.id.to_string())
            .execute(store.pool())
            .await
            .unwrap();
        let result = store.get_schedule(&schedule.id.to_string()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_row_to_schedule_invalid_mode() {
        let store = test_store().await;
        let schedule = make_schedule("bad-mode");
        store.insert_schedule(&schedule).await.unwrap();
        sqlx::query("UPDATE schedules SET mode = 'INVALID' WHERE id = ?")
            .bind(schedule.id.to_string())
            .execute(store.pool())
            .await
            .unwrap();
        let result = store.get_schedule(&schedule.id.to_string()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_row_to_schedule_invalid_concurrency() {
        let store = test_store().await;
        let schedule = make_schedule("bad-concurrency");
        store.insert_schedule(&schedule).await.unwrap();
        sqlx::query("UPDATE schedules SET concurrency = 'INVALID' WHERE id = ?")
            .bind(schedule.id.to_string())
            .execute(store.pool())
            .await
            .unwrap();
        let result = store.get_schedule(&schedule.id.to_string()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_row_to_schedule_invalid_status() {
        let store = test_store().await;
        let schedule = make_schedule("bad-status");
        store.insert_schedule(&schedule).await.unwrap();
        sqlx::query("UPDATE schedules SET status = 'INVALID' WHERE id = ?")
            .bind(schedule.id.to_string())
            .execute(store.pool())
            .await
            .unwrap();
        let result = store.get_schedule(&schedule.id.to_string()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_row_to_execution_invalid_status() {
        let store = test_store().await;
        let schedule = make_schedule("exec-bad-status");
        store.insert_schedule(&schedule).await.unwrap();
        let execution = ScheduleExecution {
            id: 0,
            schedule_id: schedule.id,
            session_id: None,
            status: ExecutionStatus::Spawned,
            error: None,
            triggered_by: "cron".into(),
            created_at: Utc::now(),
        };
        store.insert_execution(&execution).await.unwrap();
        // Corrupt the status
        sqlx::query("UPDATE schedule_executions SET status = 'INVALID' WHERE schedule_id = ?")
            .bind(schedule.id.to_string())
            .execute(store.pool())
            .await
            .unwrap();
        let result = store.list_executions(&schedule.id.to_string(), 10).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_schedule_get_by_name() {
        let store = test_store().await;
        let schedule = make_schedule("weekly-test");
        store.insert_schedule(&schedule).await.unwrap();

        let fetched = store.get_schedule_by_name("weekly-test").await.unwrap();
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().id, schedule.id);
    }

    #[tokio::test]
    async fn test_schedule_get_by_id_or_name() {
        let store = test_store().await;
        let schedule = make_schedule("test-sched");
        store.insert_schedule(&schedule).await.unwrap();

        let by_id = store
            .get_schedule_by_id_or_name(&schedule.id.to_string())
            .await
            .unwrap();
        assert!(by_id.is_some());

        let by_name = store
            .get_schedule_by_id_or_name("test-sched")
            .await
            .unwrap();
        assert!(by_name.is_some());
        assert_eq!(by_id.unwrap().id, by_name.unwrap().id);
    }

    #[tokio::test]
    async fn test_schedule_get_nonexistent() {
        let store = test_store().await;
        let fetched = store.get_schedule("nonexistent").await.unwrap();
        assert!(fetched.is_none());
    }

    #[tokio::test]
    async fn test_schedule_list() {
        let store = test_store().await;
        store.insert_schedule(&make_schedule("a")).await.unwrap();
        store.insert_schedule(&make_schedule("b")).await.unwrap();

        let all = store.list_schedules().await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_schedule_list_active() {
        let store = test_store().await;
        let mut active = make_schedule("active-one");
        active.status = ScheduleStatus::Active;
        let mut paused = make_schedule("paused-one");
        paused.status = ScheduleStatus::Paused;

        store.insert_schedule(&active).await.unwrap();
        store.insert_schedule(&paused).await.unwrap();

        let actives = store.list_active_schedules().await.unwrap();
        assert_eq!(actives.len(), 1);
        assert_eq!(actives[0].name, "active-one");
    }

    #[tokio::test]
    async fn test_schedule_update() {
        let store = test_store().await;
        let mut schedule = make_schedule("updatable");
        store.insert_schedule(&schedule).await.unwrap();

        schedule.cron = "0 4 * * *".into();
        schedule.concurrency = ConcurrencyPolicy::Replace;
        schedule.execution_count = 5;
        schedule.status = ScheduleStatus::Paused;
        schedule.updated_at = Utc::now();
        store.update_schedule(&schedule).await.unwrap();

        let fetched = store
            .get_schedule(&schedule.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.cron, "0 4 * * *");
        assert_eq!(fetched.concurrency, ConcurrencyPolicy::Replace);
        assert_eq!(fetched.execution_count, 5);
        assert_eq!(fetched.status, ScheduleStatus::Paused);
    }

    #[tokio::test]
    async fn test_schedule_delete() {
        let store = test_store().await;
        let schedule = make_schedule("deletable");
        store.insert_schedule(&schedule).await.unwrap();

        // Add an execution too
        let execution = ScheduleExecution {
            id: 0,
            schedule_id: schedule.id,
            session_id: None,
            status: ExecutionStatus::Spawned,
            error: None,
            triggered_by: "cron".into(),
            created_at: Utc::now(),
        };
        store.insert_execution(&execution).await.unwrap();

        store
            .delete_schedule(&schedule.id.to_string())
            .await
            .unwrap();

        let fetched = store.get_schedule(&schedule.id.to_string()).await.unwrap();
        assert!(fetched.is_none());

        // Executions should also be deleted
        let execs = store
            .list_executions(&schedule.id.to_string(), 100)
            .await
            .unwrap();
        assert!(execs.is_empty());
    }

    #[tokio::test]
    #[allow(clippy::literal_string_with_formatting_args)]
    async fn test_schedule_with_all_fields() {
        let store = test_store().await;
        let session_id = Uuid::new_v4();
        let schedule = Schedule {
            id: Uuid::new_v4(),
            name: "full-schedule".into(),
            cron: "*/5 * * * *".into(),
            workdir: "/home/dev/repo".into(),
            prompt: "Run tests for {date}".into(),
            provider: Provider::Codex,
            mode: SessionMode::Interactive,
            guard_preset: Some("strict".into()),
            guard_config: None,
            model: Some("opus".into()),
            allowed_tools: Some(vec!["Read".into(), "Grep".into()]),
            system_prompt: Some("Be concise".into()),
            metadata: Some({
                let mut m = std::collections::HashMap::new();
                m.insert("team".into(), "backend".into());
                m
            }),
            persona: Some("reviewer".into()),
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            concurrency: ConcurrencyPolicy::Replace,
            status: ScheduleStatus::Active,
            max_executions: Some(100),
            execution_count: 42,
            last_run_at: Some(Utc::now()),
            next_run_at: Some(Utc::now()),
            last_session_id: Some(session_id),
            worktree: Some(WorktreeConfig {
                branch: Some("main".into()),
                new_branch: Some("{schedule}-{run}".into()),
                cleanup: pulpo_common::schedule::WorktreeCleanup::OnComplete,
            }),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        store.insert_schedule(&schedule).await.unwrap();
        let fetched = store
            .get_schedule(&schedule.id.to_string())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(fetched.provider, Provider::Codex);
        assert_eq!(fetched.model, Some("opus".into()));
        assert_eq!(
            fetched.allowed_tools,
            Some(vec!["Read".into(), "Grep".into()])
        );
        assert_eq!(fetched.persona, Some("reviewer".into()));
        assert_eq!(fetched.max_executions, Some(100));
        assert_eq!(fetched.execution_count, 42);
        assert_eq!(fetched.last_session_id, Some(session_id));
        assert!(fetched.worktree.is_some());
        let wt = fetched.worktree.unwrap();
        assert_eq!(wt.branch, Some("main".into()));
    }

    #[tokio::test]
    async fn test_schedule_duplicate_name() {
        let store = test_store().await;
        store
            .insert_schedule(&make_schedule("unique"))
            .await
            .unwrap();
        let result = store.insert_schedule(&make_schedule("unique")).await;
        assert!(result.is_err());
    }

    // --- Execution store tests ---

    #[tokio::test]
    async fn test_insert_and_list_executions() {
        let store = test_store().await;
        let schedule = make_schedule("exec-test");
        store.insert_schedule(&schedule).await.unwrap();

        let session_id = Uuid::new_v4();
        let exec1 = ScheduleExecution {
            id: 0,
            schedule_id: schedule.id,
            session_id: Some(session_id),
            status: ExecutionStatus::Spawned,
            error: None,
            triggered_by: "cron".into(),
            created_at: Utc::now(),
        };
        let id1 = store.insert_execution(&exec1).await.unwrap();
        assert!(id1 > 0);

        let exec2 = ScheduleExecution {
            id: 0,
            schedule_id: schedule.id,
            session_id: None,
            status: ExecutionStatus::Skipped,
            error: None,
            triggered_by: "cron".into(),
            created_at: Utc::now(),
        };
        store.insert_execution(&exec2).await.unwrap();

        let exec3 = ScheduleExecution {
            id: 0,
            schedule_id: schedule.id,
            session_id: None,
            status: ExecutionStatus::Failed,
            error: Some("spawn failed".into()),
            triggered_by: "manual".into(),
            created_at: Utc::now(),
        };
        store.insert_execution(&exec3).await.unwrap();

        let results = store
            .list_executions(&schedule.id.to_string(), 100)
            .await
            .unwrap();
        assert_eq!(results.len(), 3);
        // Most recent first
        assert_eq!(results[0].status, ExecutionStatus::Failed);
        assert_eq!(results[0].error, Some("spawn failed".into()));
        assert_eq!(results[0].triggered_by, "manual");
        assert_eq!(results[1].status, ExecutionStatus::Skipped);
        assert_eq!(results[2].status, ExecutionStatus::Spawned);
        assert_eq!(results[2].session_id, Some(session_id));
    }

    #[tokio::test]
    async fn test_list_executions_with_limit() {
        let store = test_store().await;
        let schedule = make_schedule("limit-test");
        store.insert_schedule(&schedule).await.unwrap();

        for _ in 0..5 {
            let exec = ScheduleExecution {
                id: 0,
                schedule_id: schedule.id,
                session_id: None,
                status: ExecutionStatus::Spawned,
                error: None,
                triggered_by: "cron".into(),
                created_at: Utc::now(),
            };
            store.insert_execution(&exec).await.unwrap();
        }

        let execs = store
            .list_executions(&schedule.id.to_string(), 3)
            .await
            .unwrap();
        assert_eq!(execs.len(), 3);
    }

    #[tokio::test]
    async fn test_migration_idempotent_schedules() {
        let store = test_store().await;
        // Running migrate again should not fail
        store.migrate().await.unwrap();

        // Should still work after double-migrate
        let schedule = make_schedule("idempotent");
        store.insert_schedule(&schedule).await.unwrap();
        let fetched = store.get_schedule(&schedule.id.to_string()).await.unwrap();
        assert!(fetched.is_some());
    }

    #[tokio::test]
    async fn test_migrate_closed_pool_error() {
        let store = test_store().await;
        store.pool().close().await;
        let result = store.migrate().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_schedule_closed_pool_error() {
        let store = test_store().await;
        store.pool().close().await;
        let result = store.get_schedule("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_schedule_executions_query_error() {
        let store = test_store().await;
        let schedule = make_schedule("del-exec-err");
        store.insert_schedule(&schedule).await.unwrap();
        // Drop the executions table so the second DELETE in delete_schedule fails
        sqlx::query("DROP TABLE schedule_executions")
            .execute(store.pool())
            .await
            .unwrap();
        let result = store.delete_schedule(&schedule.id.to_string()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_insert_execution_closed_pool_error() {
        let store = test_store().await;
        let execution = ScheduleExecution {
            id: 0,
            schedule_id: Uuid::new_v4(),
            session_id: None,
            status: ExecutionStatus::Spawned,
            error: None,
            triggered_by: "cron".into(),
            created_at: Utc::now(),
        };
        store.pool().close().await;
        let result = store.insert_execution(&execution).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_update_session_waiting_for_input() {
        let store = test_store().await;
        let session = make_session("waiting-test");
        store.insert_session(&session).await.unwrap();

        // Initially false
        let s = store.get_session("waiting-test").await.unwrap().unwrap();
        assert!(!s.waiting_for_input);

        // Set to true
        store
            .update_session_waiting_for_input(&session.id.to_string(), true)
            .await
            .unwrap();
        let s = store.get_session("waiting-test").await.unwrap().unwrap();
        assert!(s.waiting_for_input);

        // Set back to false
        store
            .update_session_waiting_for_input(&session.id.to_string(), false)
            .await
            .unwrap();
        let s = store.get_session("waiting-test").await.unwrap().unwrap();
        assert!(!s.waiting_for_input);
    }
}
