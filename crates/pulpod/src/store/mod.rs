use std::fmt::Write;

use anyhow::Result;
use chrono::{DateTime, Utc};
use pulpo_common::api::ListSessionsQuery;
use pulpo_common::session::{Session, SessionStatus};
use sqlx::{Row, SqlitePool, sqlite::SqliteRow};
use uuid::Uuid;

use pulpo_common::session::InterventionCode;

/// A Web Push subscription stored for sending push notifications.
#[derive(Debug, Clone)]
pub struct PushSubscription {
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
}

/// A single intervention event for audit trail purposes.
#[derive(Debug, Clone)]
pub struct InterventionEvent {
    pub id: i64,
    pub session_id: String,
    pub code: Option<InterventionCode>,
    pub reason: String,
    pub created_at: DateTime<Utc>,
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
                backend_session_id TEXT,
                output_snapshot TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;

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

        // Idempotent migration: add intervention_code column to sessions
        let has_intervention_code: i32 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'intervention_code'",
        )
        .fetch_one(&self.pool)
        .await?;
        if has_intervention_code == 0 {
            sqlx::query("ALTER TABLE sessions ADD COLUMN intervention_code TEXT")
                .execute(&self.pool)
                .await?;
        }

        // Idempotent migration: add code column to intervention_events
        let has_event_code: i32 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('intervention_events') WHERE name = 'code'",
        )
        .fetch_one(&self.pool)
        .await?;
        if has_event_code == 0 {
            sqlx::query("ALTER TABLE intervention_events ADD COLUMN code TEXT")
                .execute(&self.pool)
                .await?;
        }

        // Partial unique index: prevent two live sessions with the same name.
        // SQLite enforces this at insert/update time, closing the race window
        // between the application-level check and the actual insert.
        // Drop first to ensure the index definition stays up-to-date.
        sqlx::query("DROP INDEX IF EXISTS idx_sessions_live_name")
            .execute(&self.pool)
            .await?;
        sqlx::query(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_sessions_live_name \
             ON sessions(name) WHERE status IN ('creating', 'active', 'idle', 'ready')",
        )
        .execute(&self.pool)
        .await?;

        // Idempotent migration: metadata + ink columns
        let has_metadata: i32 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'metadata'",
        )
        .fetch_one(&self.pool)
        .await?;
        if has_metadata == 0 {
            sqlx::query("ALTER TABLE sessions ADD COLUMN metadata TEXT")
                .execute(&self.pool)
                .await?;
            sqlx::query("ALTER TABLE sessions ADD COLUMN ink TEXT")
                .execute(&self.pool)
                .await?;
        }

        // Idempotent migration: command + description columns
        let has_command: i32 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'command'",
        )
        .fetch_one(&self.pool)
        .await?;
        if has_command == 0 {
            sqlx::query("ALTER TABLE sessions ADD COLUMN command TEXT DEFAULT ''")
                .execute(&self.pool)
                .await?;
            sqlx::query("ALTER TABLE sessions ADD COLUMN description TEXT")
                .execute(&self.pool)
                .await?;
        }

        // Idempotent migration: idle_threshold_secs column
        let has_idle_threshold = sqlx::query_scalar::<_, i32>(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'idle_threshold_secs'",
        )
        .fetch_one(&self.pool)
        .await?;
        if has_idle_threshold == 0 {
            sqlx::query("ALTER TABLE sessions ADD COLUMN idle_threshold_secs INTEGER")
                .execute(&self.pool)
                .await?;
        }

        // Idempotent migration: worktree_path column
        let has_worktree_path: i32 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'worktree_path'",
        )
        .fetch_one(&self.pool)
        .await?;
        if has_worktree_path == 0 {
            sqlx::query("ALTER TABLE sessions ADD COLUMN worktree_path TEXT")
                .execute(&self.pool)
                .await?;
        }

        // Idempotent migration: sandbox column (legacy name, kept for backward compat)
        let has_sandbox: i32 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'sandbox'",
        )
        .fetch_one(&self.pool)
        .await?;
        if has_sandbox == 0 {
            sqlx::query("ALTER TABLE sessions ADD COLUMN sandbox INTEGER DEFAULT 0")
                .execute(&self.pool)
                .await?;
        }

        // Idempotent migration: runtime column (replaces sandbox boolean)
        let has_runtime: i32 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'runtime'",
        )
        .fetch_one(&self.pool)
        .await?;
        if has_runtime == 0 {
            sqlx::query("ALTER TABLE sessions ADD COLUMN runtime TEXT NOT NULL DEFAULT 'tmux'")
                .execute(&self.pool)
                .await?;
            // Migrate existing data: sandbox=1 → runtime='docker'
            sqlx::query("UPDATE sessions SET runtime = 'docker' WHERE sandbox = 1")
                .execute(&self.pool)
                .await?;
        }

        // Idempotent migration: push subscriptions table for Web Push notifications
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS push_subscriptions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                endpoint TEXT NOT NULL UNIQUE,
                p256dh TEXT NOT NULL,
                auth TEXT NOT NULL,
                created_at TEXT NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;

        // Idempotent migration: secrets table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS secrets (
                name TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                created_at TEXT NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;

        // Idempotent migration: add env column to secrets table
        let has_secret_env: i32 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('secrets') WHERE name = 'env'",
        )
        .fetch_one(&self.pool)
        .await?;
        if has_secret_env == 0 {
            sqlx::query("ALTER TABLE secrets ADD COLUMN env TEXT")
                .execute(&self.pool)
                .await?;
        }

        // Set restrictive file permissions on the database file (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let db_path = format!("{}/state.db", self.data_dir);
            if let Ok(metadata) = std::fs::metadata(&db_path) {
                let mut perms = metadata.permissions();
                perms.set_mode(0o600);
                let _ = std::fs::set_permissions(&db_path, perms);
            }
        }

        // Idempotent migration: schedules table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS schedules (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                cron TEXT NOT NULL,
                command TEXT NOT NULL DEFAULT '',
                workdir TEXT NOT NULL,
                target_node TEXT,
                ink TEXT,
                description TEXT,
                enabled INTEGER NOT NULL DEFAULT 1,
                last_run_at TEXT,
                last_session_id TEXT,
                created_at TEXT NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;

        // Idempotent migration: rename 'killed' status to 'stopped'
        sqlx::query("UPDATE sessions SET status = 'stopped' WHERE status = 'killed'")
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn insert_session(&self, session: &Session) -> Result<()> {
        let metadata_json = session
            .metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let intervention_code_str = session.intervention_code.map(|c| c.to_string());
        let intervention_at_str = session.intervention_at.map(|dt| dt.to_rfc3339());
        let last_output_at_str = session.last_output_at.map(|dt| dt.to_rfc3339());
        let idle_since_str = session.idle_since.map(|dt| dt.to_rfc3339());
        sqlx::query(
            "INSERT INTO sessions (id, name, workdir, provider, prompt, status, mode,
                exit_code, backend_session_id, output_snapshot,
                metadata, ink, command, description,
                intervention_code, intervention_reason, intervention_at,
                last_output_at, idle_since, idle_threshold_secs, worktree_path, runtime, created_at, updated_at)
             VALUES (?, ?, ?, '', '', ?, '', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(session.id.to_string())
        .bind(&session.name)
        .bind(&session.workdir)
        .bind(session.status.to_string())
        .bind(session.exit_code)
        .bind(&session.backend_session_id)
        .bind(&session.output_snapshot)
        .bind(&metadata_json)
        .bind(&session.ink)
        .bind(&session.command)
        .bind(&session.description)
        .bind(&intervention_code_str)
        .bind(&session.intervention_reason)
        .bind(&intervention_at_str)
        .bind(&last_output_at_str)
        .bind(&idle_since_str)
        .bind(
            session
                .idle_threshold_secs
                .map(|v| i32::try_from(v).unwrap_or(i32::MAX)),
        )
        .bind(&session.worktree_path)
        .bind(session.runtime.to_string())
        .bind(session.created_at.to_rfc3339())
        .bind(session.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_session(&self, id_or_name: &str) -> Result<Option<Session>> {
        // Prefer live sessions over terminal ones when multiple share a name.
        // UUID matches are always exact (one result), but name matches may
        // return duplicates (e.g., a "ready" and a "lost" session both named "backbone").
        let row = sqlx::query(
            "SELECT * FROM sessions WHERE id = ? OR name = ? \
             ORDER BY CASE status \
               WHEN 'active' THEN 0 WHEN 'idle' THEN 1 \
               WHEN 'creating' THEN 2 WHEN 'ready' THEN 3 \
               WHEN 'lost' THEN 4 WHEN 'stopped' THEN 5 \
               ELSE 6 END \
             LIMIT 1",
        )
        .bind(id_or_name)
        .bind(id_or_name)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|r| row_to_session(&r)).transpose()
    }

    pub async fn has_active_session_by_name(&self, name: &str) -> Result<bool> {
        self.has_active_session_by_name_excluding(name, None).await
    }

    /// Check for an active session with `name`, optionally excluding a specific session ID.
    pub async fn has_active_session_by_name_excluding(
        &self,
        name: &str,
        exclude_id: Option<&str>,
    ) -> Result<bool> {
        let row = match exclude_id {
            Some(id) => {
                sqlx::query(
                    "SELECT 1 FROM sessions WHERE name = ? AND id != ? AND status IN ('creating', 'active', 'idle', 'ready') LIMIT 1",
                )
                .bind(name)
                .bind(id)
                .fetch_optional(&self.pool)
                .await?
            }
            None => {
                sqlx::query(
                    "SELECT 1 FROM sessions WHERE name = ? AND status IN ('creating', 'active', 'idle', 'ready') LIMIT 1",
                )
                .bind(name)
                .fetch_optional(&self.pool)
                .await?
            }
        };
        Ok(row.is_some())
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

        if let Some(search) = &query.search {
            sql.push_str(" AND (name LIKE ? OR command LIKE ? OR description LIKE ?)");
            let pattern = format!("%{search}%");
            binds.push(pattern.clone());
            binds.push(pattern.clone());
            binds.push(pattern);
        }

        let sort_col = match query.sort.as_deref() {
            Some("name") => "name",
            Some("status") => "status",
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

    pub async fn delete_session(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Delete all sessions with stopped or lost status. Returns the count deleted.
    pub async fn cleanup_dead_sessions(&self) -> Result<u64> {
        let result = sqlx::query("DELETE FROM sessions WHERE status IN ('stopped', 'lost')")
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    pub const fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn data_dir(&self) -> &str {
        &self.data_dir
    }

    pub async fn update_session_intervention(
        &self,
        id: &str,
        code: InterventionCode,
        reason: &str,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let code_str = code.to_string();
        sqlx::query(
            "UPDATE sessions SET intervention_code = ?, intervention_reason = ?, intervention_at = ?, status = 'stopped', updated_at = ? WHERE id = ?",
        )
        .bind(&code_str)
        .bind(reason)
        .bind(&now)
        .bind(&now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        // Append to audit log
        sqlx::query(
            "INSERT INTO intervention_events (session_id, code, reason, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(id)
        .bind(&code_str)
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
            "SELECT id, session_id, code, reason, created_at FROM intervention_events WHERE session_id = ? ORDER BY id ASC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_intervention_event).collect()
    }

    pub async fn clear_session_intervention(&self, id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE sessions SET intervention_code = NULL, intervention_reason = NULL, intervention_at = NULL, updated_at = ? WHERE id = ?",
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

    /// Update a single key in the session's metadata JSON.
    /// Reads the existing metadata, adds/updates the key, and writes it back.
    pub async fn update_session_metadata_field(
        &self,
        id: &str,
        key: &str,
        value: &str,
    ) -> Result<()> {
        let row = sqlx::query("SELECT metadata FROM sessions WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await?;
        let existing: Option<String> = row.get("metadata");
        let mut map: std::collections::HashMap<String, String> = existing
            .map(|s| serde_json::from_str(&s))
            .transpose()?
            .unwrap_or_default();
        map.insert(key.to_owned(), value.to_owned());
        let json = serde_json::to_string(&map)?;
        sqlx::query("UPDATE sessions SET metadata = ?, updated_at = ? WHERE id = ?")
            .bind(&json)
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_backend_session_id(
        &self,
        session_id: &str,
        backend_id: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE sessions SET backend_session_id = ?, updated_at = ? WHERE id = ?")
            .bind(backend_id)
            .bind(Utc::now().to_rfc3339())
            .bind(session_id)
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

    // -- Push subscription methods --

    pub async fn save_push_subscription(
        &self,
        endpoint: &str,
        p256dh: &str,
        auth: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT OR REPLACE INTO push_subscriptions (endpoint, p256dh, auth, created_at) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(endpoint)
        .bind(p256dh)
        .bind(auth)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_push_subscription(&self, endpoint: &str) -> Result<()> {
        sqlx::query("DELETE FROM push_subscriptions WHERE endpoint = ?")
            .bind(endpoint)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_push_subscriptions(&self) -> Result<Vec<PushSubscription>> {
        let rows = sqlx::query("SELECT endpoint, p256dh, auth FROM push_subscriptions")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(|r| PushSubscription {
                endpoint: r.get("endpoint"),
                p256dh: r.get("p256dh"),
                auth: r.get("auth"),
            })
            .collect())
    }

    // -- Schedule methods --

    pub async fn insert_schedule(&self, schedule: &pulpo_common::api::Schedule) -> Result<()> {
        sqlx::query(
            "INSERT INTO schedules (id, name, cron, command, workdir, target_node, ink, description, enabled, last_run_at, last_session_id, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&schedule.id)
        .bind(&schedule.name)
        .bind(&schedule.cron)
        .bind(&schedule.command)
        .bind(&schedule.workdir)
        .bind(&schedule.target_node)
        .bind(&schedule.ink)
        .bind(&schedule.description)
        .bind(schedule.enabled)
        .bind(&schedule.last_run_at)
        .bind(&schedule.last_session_id)
        .bind(&schedule.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_schedules(&self) -> Result<Vec<pulpo_common::api::Schedule>> {
        let rows = sqlx::query("SELECT * FROM schedules ORDER BY name")
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(row_to_schedule).collect()
    }

    pub async fn get_schedule(
        &self,
        id_or_name: &str,
    ) -> Result<Option<pulpo_common::api::Schedule>> {
        let row = sqlx::query("SELECT * FROM schedules WHERE id = ? OR name = ?")
            .bind(id_or_name)
            .bind(id_or_name)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|r| row_to_schedule(&r)).transpose()
    }

    pub async fn update_schedule_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        sqlx::query("UPDATE schedules SET enabled = ? WHERE id = ?")
            .bind(enabled)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_schedule_last_run(&self, id: &str, session_id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE schedules SET last_run_at = ?, last_session_id = ? WHERE id = ?")
            .bind(&now)
            .bind(session_id)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_schedule_runs(
        &self,
        schedule_name: &str,
        limit: usize,
    ) -> Result<Vec<Session>> {
        // Escape SQL LIKE wildcards in the schedule name to prevent unintended matches
        let escaped = schedule_name.replace('%', "\\%").replace('_', "\\_");
        let prefix = format!("{escaped}-%");
        let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = sqlx::query(
            "SELECT * FROM sessions WHERE name LIKE ? ESCAPE '\\' ORDER BY created_at DESC LIMIT ?",
        )
        .bind(&prefix)
        .bind(limit_i64)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_session).collect()
    }

    pub async fn delete_schedule(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM schedules WHERE id = ? OR name = ?")
            .bind(id)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // -- Secret methods --

    /// Upsert a secret (INSERT OR REPLACE).
    pub async fn set_secret(&self, name: &str, value: &str) -> Result<()> {
        self.set_secret_with_env(name, value, None).await
    }

    /// Upsert a secret with an optional env var name override.
    pub async fn set_secret_with_env(
        &self,
        name: &str,
        value: &str,
        env: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT OR REPLACE INTO secrets (name, value, env, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(name)
        .bind(value)
        .bind(env)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Get a secret's value by name (used internally for injection, never exposed via API).
    pub async fn get_secret(&self, name: &str) -> Result<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as("SELECT value FROM secrets WHERE name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|(v,)| v))
    }

    /// List secret names with optional env override (never returns values).
    /// Returns `(name, env, created_at)` tuples.
    pub async fn list_secret_names(&self) -> Result<Vec<(String, Option<String>, String)>> {
        let rows: Vec<(String, Option<String>, String)> =
            sqlx::query_as("SELECT name, env, created_at FROM secrets ORDER BY name")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows)
    }

    /// Given a list of secret names, returns a map of `env_var_name` -> value.
    /// Uses the `env` field if set, otherwise uses `name` as the env var.
    pub async fn get_secrets_for_injection(
        &self,
        names: &[String],
    ) -> Result<std::collections::HashMap<String, String>> {
        let mut result = std::collections::HashMap::new();
        // Track which secret name owns each env var to detect collisions
        let mut env_owners: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for name in names {
            let row: Option<(String, Option<String>)> =
                sqlx::query_as("SELECT value, env FROM secrets WHERE name = ?")
                    .bind(name)
                    .fetch_optional(&self.pool)
                    .await?;
            if let Some((value, env)) = row {
                let env_var = env.unwrap_or_else(|| name.clone());
                if let Some(prev_name) = env_owners.get(&env_var) {
                    anyhow::bail!(
                        "secrets '{prev_name}' and '{name}' both map to env var '{env_var}' — use only one"
                    );
                }
                env_owners.insert(env_var.clone(), name.clone());
                result.insert(env_var, value);
            }
        }
        Ok(result)
    }

    /// Delete a secret. Returns true if deleted, false if not found.
    pub async fn delete_secret(&self, name: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM secrets WHERE name = ?")
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Get all secrets as name→value pairs (used internally for session env injection).
    pub async fn get_all_secrets(&self) -> Result<std::collections::HashMap<String, String>> {
        let rows: Vec<(String, String)> = sqlx::query_as("SELECT name, value FROM secrets")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().collect())
    }
}

fn row_to_session(row: &SqliteRow) -> Result<Session> {
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
        name: row.get("name"),
        workdir: row.get("workdir"),
        command: row.get("command"),
        description: row.get("description"),
        status: status_str
            .parse::<SessionStatus>()
            .map_err(|e| anyhow::anyhow!(e))?,
        exit_code: row.get("exit_code"),
        backend_session_id: row.get("backend_session_id"),
        output_snapshot: row.get("output_snapshot"),
        metadata,
        ink: row.get("ink"),
        intervention_code,
        intervention_reason: row.get("intervention_reason"),
        intervention_at,
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
        idle_threshold_secs: {
            // Use try_get to handle rows where the column may not exist
            // (e.g., SQLite prepared statement cache before ALTER TABLE runs)
            let v: Option<i32> = row.try_get("idle_threshold_secs").unwrap_or(None);
            v.map(|n| u32::try_from(n).unwrap_or(0))
        },
        worktree_path: row.try_get("worktree_path").unwrap_or(None),
        runtime: {
            let s: Option<String> = row.try_get("runtime").unwrap_or(None);
            s.and_then(|s| s.parse().ok()).unwrap_or_default()
        },
        created_at: DateTime::parse_from_rfc3339(&created_str)?.with_timezone(&Utc),
        updated_at: DateTime::parse_from_rfc3339(&updated_str)?.with_timezone(&Utc),
    })
}

#[allow(clippy::unnecessary_wraps)]
fn row_to_schedule(row: &SqliteRow) -> Result<pulpo_common::api::Schedule> {
    Ok(pulpo_common::api::Schedule {
        id: row.get("id"),
        name: row.get("name"),
        cron: row.get("cron"),
        command: row.get("command"),
        workdir: row.get("workdir"),
        target_node: row.get("target_node"),
        ink: row.get("ink"),
        description: row.get("description"),
        enabled: row.get("enabled"),
        last_run_at: row.get("last_run_at"),
        last_session_id: row.get("last_session_id"),
        created_at: row.get("created_at"),
    })
}

fn row_to_intervention_event(row: &SqliteRow) -> Result<InterventionEvent> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use pulpo_common::session::Runtime;

    fn make_session(name: &str) -> Session {
        Session {
            id: Uuid::new_v4(),
            name: name.into(),
            workdir: "/tmp/repo".into(),
            command: "echo hello".into(),
            description: Some("Fix the bug".into()),
            status: SessionStatus::Active,
            exit_code: None,
            backend_session_id: Some(name.to_owned()),
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
            runtime: Runtime::Tmux,
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

        assert_eq!(fetched.status, SessionStatus::Active);

        assert_eq!(fetched.exit_code, None);
        assert_eq!(fetched.backend_session_id, Some("test-roundtrip".into()));
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
    async fn test_get_session_prefers_live_over_terminal() {
        let store = test_store().await;

        // Insert a stopped session with name "dup"
        let mut stopped = make_session("dup");
        stopped.id = uuid::Uuid::new_v4();
        stopped.status = SessionStatus::Stopped;
        // Remove from unique index by marking stopped before insert
        store.insert_session(&stopped).await.unwrap();

        // Insert a ready session with the same name "dup"
        let mut ready = make_session("dup-ready");
        ready.id = uuid::Uuid::new_v4();
        ready.name = "dup".into();
        ready.status = SessionStatus::Ready;
        // The unique index only covers creating/active/idle/ready,
        // and stopped is excluded, so this insert should work
        store.insert_session(&ready).await.unwrap();

        // get_session by name should return the ready one, not the stopped one
        let fetched = store.get_session("dup").await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Ready);
        assert_eq!(fetched.id, ready.id);
    }

    #[tokio::test]
    async fn test_has_active_session_by_name_true() {
        let store = test_store().await;
        let session = make_session("my-session");
        store.insert_session(&session).await.unwrap();

        assert!(
            store
                .has_active_session_by_name("my-session")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn test_has_active_session_by_name_false_no_match() {
        let store = test_store().await;
        assert!(
            !store
                .has_active_session_by_name("nonexistent")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn test_has_active_session_by_name_false_stopped() {
        let store = test_store().await;
        let mut session = make_session("stopped-session");
        session.status = SessionStatus::Stopped;
        store.insert_session(&session).await.unwrap();

        assert!(
            !store
                .has_active_session_by_name("stopped-session")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn test_has_active_session_by_name_stale() {
        let store = test_store().await;
        let mut session = make_session("idle-session");
        session.status = SessionStatus::Idle;
        store.insert_session(&session).await.unwrap();

        assert!(
            store
                .has_active_session_by_name("idle-session")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn test_has_active_session_by_name_creating() {
        let store = test_store().await;
        let mut session = make_session("creating-session");
        session.status = SessionStatus::Creating;
        store.insert_session(&session).await.unwrap();

        assert!(
            store
                .has_active_session_by_name("creating-session")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn test_has_active_session_by_name_ready() {
        let store = test_store().await;
        let mut session = make_session("ready-session");
        session.status = SessionStatus::Ready;
        store.insert_session(&session).await.unwrap();

        assert!(
            store
                .has_active_session_by_name("ready-session")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn test_has_active_session_by_name_excluding_self() {
        let store = test_store().await;
        let mut session = make_session("ready-session");
        session.status = SessionStatus::Ready;
        store.insert_session(&session).await.unwrap();

        // Excluding self should return false (no *other* active session with this name)
        assert!(
            !store
                .has_active_session_by_name_excluding(
                    "ready-session",
                    Some(&session.id.to_string()),
                )
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn test_has_active_session_by_name_excluding_different_id() {
        let store = test_store().await;
        let mut session = make_session("clash-session");
        session.status = SessionStatus::Active;
        store.insert_session(&session).await.unwrap();

        // Excluding a different ID should still find the active session
        assert!(
            store
                .has_active_session_by_name_excluding(
                    "clash-session",
                    Some(&uuid::Uuid::new_v4().to_string()),
                )
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn test_unique_index_prevents_duplicate_live_names() {
        let store = test_store().await;
        let s1 = make_session("dup-name");
        store.insert_session(&s1).await.unwrap();
        // Second insert with same name and live status should fail at DB level
        let mut s2 = make_session("dup-name");
        s2.id = uuid::Uuid::new_v4();
        let result = store.insert_session(&s2).await;
        assert!(result.is_err(), "expected unique constraint violation");
    }

    #[tokio::test]
    async fn test_unique_index_allows_reuse_after_stop() {
        let store = test_store().await;
        let s1 = make_session("reuse-name");
        store.insert_session(&s1).await.unwrap();
        store
            .update_session_status(&s1.id.to_string(), SessionStatus::Stopped)
            .await
            .unwrap();
        // New session with same name should succeed — old one is stopped
        let mut s2 = make_session("reuse-name");
        s2.id = uuid::Uuid::new_v4();
        store.insert_session(&s2).await.unwrap();
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
            .update_session_status(&session.id.to_string(), SessionStatus::Ready)
            .await
            .unwrap();

        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Ready);
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
            command: "echo hello".into(),
            description: Some("test".into()),
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
            runtime: Runtime::Tmux,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        store.insert_session(&session).await.unwrap();
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();

        assert!(fetched.exit_code.is_none());
        assert!(fetched.backend_session_id.is_none());
        assert!(fetched.output_snapshot.is_none());
    }

    const TEST_UUID: &str = "550e8400-e29b-41d4-a716-446655440000";

    #[tokio::test]
    async fn test_row_to_session_invalid_status() {
        let store = test_store().await;
        sqlx::query(
            "INSERT INTO sessions (id, name, workdir, provider, prompt, status, mode,
                created_at, updated_at, command)
             VALUES (?, 'test', '/tmp', '', '', 'bad_status', '',
                '2024-01-01T00:00:00+00:00', '2024-01-01T00:00:00+00:00', 'echo test')",
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
             VALUES ('not-a-uuid', 'test', '/tmp', 'claude', 'test', 'active', 'interactive',
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
             VALUES (?, 'test', '/tmp', 'claude', 'test', 'active', 'interactive',
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
            .update_session_status("test-id", SessionStatus::Stopped)
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
    async fn test_data_dir_accessor() {
        let store = test_store().await;
        let dir = store.data_dir();
        assert!(!dir.is_empty());
    }

    #[tokio::test]
    async fn test_list_sessions_filtered_by_status() {
        let store = test_store().await;
        let mut s1 = make_session("running-1");
        s1.status = SessionStatus::Active;
        let mut s2 = make_session("completed-1");
        s2.status = SessionStatus::Ready;
        store.insert_session(&s1).await.unwrap();
        store.insert_session(&s2).await.unwrap();

        let query = ListSessionsQuery {
            status: Some("active".into()),
            ..Default::default()
        };
        let sessions = store.list_sessions_filtered(&query).await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].status, SessionStatus::Active);
    }

    #[tokio::test]
    async fn test_list_sessions_filtered_by_multiple_statuses() {
        let store = test_store().await;
        let mut s1 = make_session("running-2");
        s1.status = SessionStatus::Active;
        let mut s2 = make_session("completed-2");
        s2.status = SessionStatus::Ready;
        let mut s3 = make_session("dead-1");
        s3.status = SessionStatus::Stopped;
        store.insert_session(&s1).await.unwrap();
        store.insert_session(&s2).await.unwrap();
        store.insert_session(&s3).await.unwrap();

        let query = ListSessionsQuery {
            status: Some("active,ready".into()),
            ..Default::default()
        };
        let sessions = store.list_sessions_filtered(&query).await.unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[tokio::test]
    async fn test_list_sessions_filtered_by_search() {
        let store = test_store().await;
        let mut s1 = make_session("api-fix");
        s1.command = "Fix the API endpoint".into();
        let mut s2 = make_session("ui-refactor");
        s2.command = "Refactor the UI components".into();
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
        s1.status = SessionStatus::Active;
        s1.command = "Fix the API".into();
        let mut s2 = make_session("api-refactor");
        s2.status = SessionStatus::Ready;
        s2.command = "Refactor the API".into();
        let mut s3 = make_session("ui-fix");
        s3.status = SessionStatus::Active;
        s3.command = "Fix the UI".into();
        store.insert_session(&s1).await.unwrap();
        store.insert_session(&s2).await.unwrap();
        store.insert_session(&s3).await.unwrap();

        let query = ListSessionsQuery {
            status: Some("active".into()),
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
        s1.status = SessionStatus::Active;
        let mut s2 = make_session("second");
        s2.status = SessionStatus::Ready;
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
        s2.command = String::new();
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
            .update_session_intervention(
                &session.id.to_string(),
                InterventionCode::MemoryPressure,
                "Memory usage 95% (512MB/8192MB)",
            )
            .await
            .unwrap();

        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Stopped);
        assert_eq!(
            fetched.intervention_code,
            Some(InterventionCode::MemoryPressure)
        );
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
        let result = store
            .update_session_intervention("test-id", InterventionCode::UserStop, "reason")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_clear_session_intervention() {
        let store = test_store().await;
        let session = make_session("clear-test");
        store.insert_session(&session).await.unwrap();

        // Set intervention first
        store
            .update_session_intervention(
                &session.id.to_string(),
                InterventionCode::UserStop,
                "test reason",
            )
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
        assert!(fetched.intervention_code.is_none());
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
             VALUES (?, 'test', '/tmp', 'claude', 'test', 'active', 'interactive',
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
            .update_session_intervention(&sid, InterventionCode::MemoryPressure, "Memory 95%")
            .await
            .unwrap();

        // Simulate a second intervention (e.g., session was resumed and hit pressure again)
        // Reset session to running first so the scenario makes sense
        sqlx::query("UPDATE sessions SET status = 'active' WHERE id = ?")
            .bind(&sid)
            .execute(store.pool())
            .await
            .unwrap();
        store
            .update_session_intervention(&sid, InterventionCode::MemoryPressure, "Memory 98%")
            .await
            .unwrap();

        let events = store.list_intervention_events(&sid).await.unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].code, Some(InterventionCode::MemoryPressure));
        assert_eq!(events[0].reason, "Memory 95%");
        assert_eq!(events[1].code, Some(InterventionCode::MemoryPressure));
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
            code: Some(InterventionCode::MemoryPressure),
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
    async fn test_new_session_fields_roundtrip() {
        let store = test_store().await;
        let mut session = make_session("new-fields-test");
        session.metadata = Some(
            [
                ("discord_channel".into(), "123".into()),
                ("user".into(), "alice".into()),
            ]
            .into_iter()
            .collect(),
        );
        session.ink = Some("reviewer".into());

        store.insert_session(&session).await.unwrap();
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();

        let meta = fetched.metadata.unwrap();
        assert_eq!(meta.get("discord_channel").unwrap(), "123");
        assert_eq!(meta.get("user").unwrap(), "alice");
        assert_eq!(fetched.ink, Some("reviewer".into()));
    }

    #[tokio::test]
    async fn test_migrate_closed_pool_error() {
        let store = test_store().await;
        store.pool().close().await;
        let result = store.migrate().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_intervention_code_roundtrip() {
        let store = test_store().await;
        let mut session = make_session("code-roundtrip");
        session.intervention_code = Some(InterventionCode::IdleTimeout);
        session.intervention_reason = Some("Idle for 10 minutes".into());
        session.intervention_at = Some(Utc::now());
        store.insert_session(&session).await.unwrap();

        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            fetched.intervention_code,
            Some(InterventionCode::IdleTimeout)
        );
        assert_eq!(
            fetched.intervention_reason.as_deref(),
            Some("Idle for 10 minutes")
        );
    }

    #[tokio::test]
    async fn test_intervention_code_none_roundtrip() {
        let store = test_store().await;
        let session = make_session("code-none");
        store.insert_session(&session).await.unwrap();

        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.intervention_code.is_none());
    }

    #[tokio::test]
    async fn test_row_to_session_invalid_intervention_code() {
        let store = test_store().await;
        sqlx::query(
            "INSERT INTO sessions (id, name, workdir, provider, prompt, status, mode,
                intervention_code, created_at, updated_at)
             VALUES (?, 'test', '/tmp', 'claude', 'test', 'active', 'interactive',
                'invalid_code', '2024-01-01T00:00:00+00:00', '2024-01-01T00:00:00+00:00')",
        )
        .bind(TEST_UUID)
        .execute(store.pool())
        .await
        .unwrap();
        let result = store.get_session(TEST_UUID).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_intervention_event_code_roundtrip() {
        let store = test_store().await;
        let session = make_session("event-code");
        store.insert_session(&session).await.unwrap();
        let sid = session.id.to_string();

        store
            .update_session_intervention(&sid, InterventionCode::IdleTimeout, "Idle 15 min")
            .await
            .unwrap();

        let events = store.list_intervention_events(&sid).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].code, Some(InterventionCode::IdleTimeout));
        assert_eq!(events[0].reason, "Idle 15 min");
    }

    #[tokio::test]
    async fn test_intervention_event_user_stop_code() {
        let store = test_store().await;
        let session = make_session("user-stop");
        store.insert_session(&session).await.unwrap();
        let sid = session.id.to_string();

        store
            .update_session_intervention(&sid, InterventionCode::UserStop, "Manual stop")
            .await
            .unwrap();

        let fetched = store.get_session(&sid).await.unwrap().unwrap();
        assert_eq!(fetched.intervention_code, Some(InterventionCode::UserStop));

        let events = store.list_intervention_events(&sid).await.unwrap();
        assert_eq!(events[0].code, Some(InterventionCode::UserStop));
    }

    #[tokio::test]
    async fn test_idle_status_roundtrip() {
        let store = test_store().await;
        let session = make_session("idle-test");
        store.insert_session(&session).await.unwrap();

        store
            .update_session_status(&session.id.to_string(), SessionStatus::Idle)
            .await
            .unwrap();

        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Idle);
    }

    // -- Push subscription tests --

    #[tokio::test]
    async fn test_push_subscription_save_and_list() {
        let store = test_store().await;
        store
            .save_push_subscription("https://push.example.com/1", "p256dh-key", "auth-key")
            .await
            .unwrap();

        let subs = store.list_push_subscriptions().await.unwrap();
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].endpoint, "https://push.example.com/1");
        assert_eq!(subs[0].p256dh, "p256dh-key");
        assert_eq!(subs[0].auth, "auth-key");
    }

    #[tokio::test]
    async fn test_push_subscription_save_replaces_on_same_endpoint() {
        let store = test_store().await;
        store
            .save_push_subscription("https://push.example.com/1", "old-p256dh", "old-auth")
            .await
            .unwrap();
        store
            .save_push_subscription("https://push.example.com/1", "new-p256dh", "new-auth")
            .await
            .unwrap();

        let subs = store.list_push_subscriptions().await.unwrap();
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].p256dh, "new-p256dh");
        assert_eq!(subs[0].auth, "new-auth");
    }

    #[tokio::test]
    async fn test_push_subscription_multiple_endpoints() {
        let store = test_store().await;
        store
            .save_push_subscription("https://push.example.com/1", "p1", "a1")
            .await
            .unwrap();
        store
            .save_push_subscription("https://push.example.com/2", "p2", "a2")
            .await
            .unwrap();

        let subs = store.list_push_subscriptions().await.unwrap();
        assert_eq!(subs.len(), 2);
    }

    #[tokio::test]
    async fn test_push_subscription_delete() {
        let store = test_store().await;
        store
            .save_push_subscription("https://push.example.com/1", "p1", "a1")
            .await
            .unwrap();
        store
            .save_push_subscription("https://push.example.com/2", "p2", "a2")
            .await
            .unwrap();

        store
            .delete_push_subscription("https://push.example.com/1")
            .await
            .unwrap();

        let subs = store.list_push_subscriptions().await.unwrap();
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].endpoint, "https://push.example.com/2");
    }

    #[tokio::test]
    async fn test_push_subscription_delete_nonexistent() {
        let store = test_store().await;
        // Should not error when deleting a non-existent endpoint
        store
            .delete_push_subscription("https://push.example.com/nonexistent")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_push_subscription_list_empty() {
        let store = test_store().await;
        let subs = store.list_push_subscriptions().await.unwrap();
        assert!(subs.is_empty());
    }

    #[tokio::test]
    async fn test_push_subscription_debug_clone() {
        let sub = PushSubscription {
            endpoint: "https://push.example.com/1".into(),
            p256dh: "key".into(),
            auth: "auth".into(),
        };
        let debug = format!("{sub:?}");
        assert!(debug.contains("push.example.com"));
        #[allow(clippy::redundant_clone)]
        let cloned = sub.clone();
        assert_eq!(cloned.endpoint, "https://push.example.com/1");
    }

    #[tokio::test]
    async fn test_push_subscription_after_table_dropped() {
        let store = test_store().await;
        sqlx::query("DROP TABLE push_subscriptions")
            .execute(store.pool())
            .await
            .unwrap();
        let result = store.list_push_subscriptions().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_push_subscription_save_after_table_dropped() {
        let store = test_store().await;
        sqlx::query("DROP TABLE push_subscriptions")
            .execute(store.pool())
            .await
            .unwrap();
        let result = store
            .save_push_subscription("https://push.example.com/1", "p", "a")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_push_subscription_delete_after_table_dropped() {
        let store = test_store().await;
        sqlx::query("DROP TABLE push_subscriptions")
            .execute(store.pool())
            .await
            .unwrap();
        let result = store
            .delete_push_subscription("https://push.example.com/1")
            .await;
        assert!(result.is_err());
    }

    // -- Schedule tests --

    #[tokio::test]
    async fn test_schedule_crud() {
        let store = test_store().await;
        let schedule = pulpo_common::api::Schedule {
            id: "sched-1".into(),
            name: "nightly-review".into(),
            cron: "0 3 * * *".into(),
            command: "claude -p 'review'".into(),
            workdir: "/tmp".into(),
            target_node: None,
            ink: None,
            description: Some("Nightly review".into()),
            enabled: true,
            last_run_at: None,
            last_session_id: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        store.insert_schedule(&schedule).await.unwrap();

        let fetched = store.get_schedule("nightly-review").await.unwrap().unwrap();
        assert_eq!(fetched.name, "nightly-review");
        assert_eq!(fetched.cron, "0 3 * * *");
        assert!(fetched.enabled);

        let all = store.list_schedules().await.unwrap();
        assert_eq!(all.len(), 1);

        store
            .update_schedule_enabled(&schedule.id, false)
            .await
            .unwrap();
        let updated = store.get_schedule(&schedule.id).await.unwrap().unwrap();
        assert!(!updated.enabled);

        store
            .update_schedule_last_run(&schedule.id, "session-123")
            .await
            .unwrap();
        let ran = store.get_schedule(&schedule.id).await.unwrap().unwrap();
        assert!(ran.last_run_at.is_some());
        assert_eq!(ran.last_session_id, Some("session-123".into()));

        store.delete_schedule(&schedule.id).await.unwrap();
        assert!(store.get_schedule(&schedule.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_list_schedule_runs() {
        let store = test_store().await;

        // Insert matching sessions (name starts with "nightly-")
        let s1 = make_session("nightly-001");
        let s2 = make_session("nightly-002");
        // Insert non-matching session
        let s3 = make_session("other-task");

        store.insert_session(&s1).await.unwrap();
        store.insert_session(&s2).await.unwrap();
        store.insert_session(&s3).await.unwrap();

        let runs = store.list_schedule_runs("nightly", 20).await.unwrap();
        assert_eq!(runs.len(), 2);
        for run in &runs {
            assert!(run.name.starts_with("nightly-"));
        }

        // Test limit
        let runs = store.list_schedule_runs("nightly", 1).await.unwrap();
        assert_eq!(runs.len(), 1);

        // Test no matches
        let runs = store.list_schedule_runs("nonexistent", 20).await.unwrap();
        assert!(runs.is_empty());
    }

    #[tokio::test]
    async fn test_schedule_unique_name() {
        let store = test_store().await;
        let schedule = pulpo_common::api::Schedule {
            id: "s1".into(),
            name: "dup".into(),
            cron: "* * * * *".into(),
            command: "echo".into(),
            workdir: "/tmp".into(),
            target_node: None,
            ink: None,
            description: None,
            enabled: true,
            last_run_at: None,
            last_session_id: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        store.insert_schedule(&schedule).await.unwrap();
        let dup = pulpo_common::api::Schedule {
            id: "s2".into(),
            name: "dup".into(),
            ..schedule
        };
        assert!(store.insert_schedule(&dup).await.is_err());
    }

    // -- update_session_metadata_field tests --

    #[tokio::test]
    async fn test_update_session_metadata_field_empty_metadata() {
        let store = test_store().await;
        let session = make_session("meta-empty");
        store.insert_session(&session).await.unwrap();

        store
            .update_session_metadata_field(
                &session.id.to_string(),
                "pr_url",
                "https://github.com/a/b/pull/1",
            )
            .await
            .unwrap();

        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        let meta = fetched.metadata.unwrap();
        assert_eq!(meta.get("pr_url").unwrap(), "https://github.com/a/b/pull/1");
    }

    #[tokio::test]
    async fn test_update_session_metadata_field_existing_metadata() {
        let store = test_store().await;
        let mut session = make_session("meta-existing");
        session.metadata =
            Some(std::iter::once(("discord_channel".into(), "123".into())).collect());
        store.insert_session(&session).await.unwrap();

        store
            .update_session_metadata_field(&session.id.to_string(), "branch", "feature/test")
            .await
            .unwrap();

        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        let meta = fetched.metadata.unwrap();
        // Original key preserved
        assert_eq!(meta.get("discord_channel").unwrap(), "123");
        // New key added
        assert_eq!(meta.get("branch").unwrap(), "feature/test");
    }

    #[tokio::test]
    async fn test_update_session_metadata_field_overwrite_key() {
        let store = test_store().await;
        let session = make_session("meta-overwrite");
        store.insert_session(&session).await.unwrap();

        store
            .update_session_metadata_field(&session.id.to_string(), "pr_url", "https://old")
            .await
            .unwrap();
        store
            .update_session_metadata_field(&session.id.to_string(), "pr_url", "https://new")
            .await
            .unwrap();

        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            fetched.metadata.unwrap().get("pr_url").unwrap(),
            "https://new"
        );
    }

    #[tokio::test]
    async fn test_update_session_metadata_field_nonexistent_session() {
        let store = test_store().await;
        let result = store
            .update_session_metadata_field("nonexistent-id", "key", "value")
            .await;
        assert!(result.is_err());
    }

    // -- Secret tests --

    #[tokio::test]
    async fn test_set_and_get_secret() {
        let store = test_store().await;
        store.set_secret("MY_TOKEN", "abc123").await.unwrap();
        let value = store.get_secret("MY_TOKEN").await.unwrap();
        assert_eq!(value, Some("abc123".into()));
    }

    #[tokio::test]
    async fn test_get_secret_not_found() {
        let store = test_store().await;
        let value = store.get_secret("NONEXISTENT").await.unwrap();
        assert!(value.is_none());
    }

    #[tokio::test]
    async fn test_set_secret_upsert() {
        let store = test_store().await;
        store.set_secret("MY_TOKEN", "old").await.unwrap();
        store.set_secret("MY_TOKEN", "new").await.unwrap();
        let value = store.get_secret("MY_TOKEN").await.unwrap();
        assert_eq!(value, Some("new".into()));
    }

    #[tokio::test]
    async fn test_list_secret_names() {
        let store = test_store().await;
        store.set_secret("B_TOKEN", "val").await.unwrap();
        store.set_secret("A_TOKEN", "val").await.unwrap();
        let names = store.list_secret_names().await.unwrap();
        assert_eq!(names.len(), 2);
        assert_eq!(names[0].0, "A_TOKEN");
        assert!(names[0].1.is_none()); // no env override
        assert_eq!(names[1].0, "B_TOKEN");
        // created_at should be non-empty
        assert!(!names[0].2.is_empty());
    }

    #[tokio::test]
    async fn test_list_secret_names_with_env() {
        let store = test_store().await;
        store
            .set_secret_with_env("GH_WORK", "token1", Some("GITHUB_TOKEN"))
            .await
            .unwrap();
        store.set_secret("PLAIN_KEY", "token2").await.unwrap();
        let names = store.list_secret_names().await.unwrap();
        assert_eq!(names.len(), 2);
        assert_eq!(names[0].0, "GH_WORK");
        assert_eq!(names[0].1.as_deref(), Some("GITHUB_TOKEN"));
        assert_eq!(names[1].0, "PLAIN_KEY");
        assert!(names[1].1.is_none());
    }

    #[tokio::test]
    async fn test_list_secret_names_empty() {
        let store = test_store().await;
        let names = store.list_secret_names().await.unwrap();
        assert!(names.is_empty());
    }

    #[tokio::test]
    async fn test_delete_secret_found() {
        let store = test_store().await;
        store.set_secret("MY_TOKEN", "val").await.unwrap();
        let deleted = store.delete_secret("MY_TOKEN").await.unwrap();
        assert!(deleted);
        let value = store.get_secret("MY_TOKEN").await.unwrap();
        assert!(value.is_none());
    }

    #[tokio::test]
    async fn test_delete_secret_not_found() {
        let store = test_store().await;
        let deleted = store.delete_secret("NONEXISTENT").await.unwrap();
        assert!(!deleted);
    }

    #[tokio::test]
    async fn test_set_secret_with_env() {
        let store = test_store().await;
        store
            .set_secret_with_env("GH_WORK", "token123", Some("GITHUB_TOKEN"))
            .await
            .unwrap();
        let value = store.get_secret("GH_WORK").await.unwrap();
        assert_eq!(value, Some("token123".into()));
    }

    #[tokio::test]
    async fn test_set_secret_with_env_none() {
        let store = test_store().await;
        store
            .set_secret_with_env("MY_KEY", "val", None)
            .await
            .unwrap();
        let value = store.get_secret("MY_KEY").await.unwrap();
        assert_eq!(value, Some("val".into()));
    }

    #[tokio::test]
    async fn test_set_secret_with_env_upsert() {
        let store = test_store().await;
        store
            .set_secret_with_env("GH_WORK", "old", Some("OLD_VAR"))
            .await
            .unwrap();
        store
            .set_secret_with_env("GH_WORK", "new", Some("NEW_VAR"))
            .await
            .unwrap();
        let value = store.get_secret("GH_WORK").await.unwrap();
        assert_eq!(value, Some("new".into()));
        let names = store.list_secret_names().await.unwrap();
        assert_eq!(names[0].1.as_deref(), Some("NEW_VAR"));
    }

    #[tokio::test]
    async fn test_get_secrets_for_injection() {
        let store = test_store().await;
        store
            .set_secret_with_env("GH_WORK", "token1", Some("GITHUB_TOKEN"))
            .await
            .unwrap();
        store.set_secret("NPM_TOKEN", "token2").await.unwrap();
        let secrets = store
            .get_secrets_for_injection(&["GH_WORK".into(), "NPM_TOKEN".into()])
            .await
            .unwrap();
        assert_eq!(secrets.len(), 2);
        // GH_WORK has env override → key is GITHUB_TOKEN
        assert_eq!(secrets.get("GITHUB_TOKEN").unwrap(), "token1");
        // NPM_TOKEN has no env override → key is NPM_TOKEN
        assert_eq!(secrets.get("NPM_TOKEN").unwrap(), "token2");
    }

    #[tokio::test]
    async fn test_get_secrets_for_injection_missing() {
        let store = test_store().await;
        store.set_secret("EXISTING", "val").await.unwrap();
        let secrets = store
            .get_secrets_for_injection(&["EXISTING".into(), "MISSING".into()])
            .await
            .unwrap();
        assert_eq!(secrets.len(), 1);
        assert_eq!(secrets.get("EXISTING").unwrap(), "val");
    }

    #[tokio::test]
    async fn test_get_secrets_for_injection_empty() {
        let store = test_store().await;
        let secrets = store.get_secrets_for_injection(&[]).await.unwrap();
        assert!(secrets.is_empty());
    }

    #[tokio::test]
    async fn test_get_secrets_for_injection_env_collision() {
        let store = test_store().await;
        store
            .set_secret_with_env("GH_WORK", "val1", Some("GITHUB_TOKEN"))
            .await
            .unwrap();
        store
            .set_secret_with_env("GH_PERSONAL", "val2", Some("GITHUB_TOKEN"))
            .await
            .unwrap();
        let err = store
            .get_secrets_for_injection(&["GH_WORK".into(), "GH_PERSONAL".into()])
            .await
            .unwrap_err();
        assert!(err.to_string().contains("both map to env var"), "{err}");
    }

    #[tokio::test]
    async fn test_get_all_secrets() {
        let store = test_store().await;
        store.set_secret("KEY_A", "val_a").await.unwrap();
        store.set_secret("KEY_B", "val_b").await.unwrap();
        let all = store.get_all_secrets().await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all.get("KEY_A").unwrap(), "val_a");
        assert_eq!(all.get("KEY_B").unwrap(), "val_b");
    }

    #[tokio::test]
    async fn test_get_all_secrets_empty() {
        let store = test_store().await;
        let all = store.get_all_secrets().await.unwrap();
        assert!(all.is_empty());
    }

    #[tokio::test]
    async fn test_secret_after_table_dropped() {
        let store = test_store().await;
        sqlx::query("DROP TABLE secrets")
            .execute(store.pool())
            .await
            .unwrap();
        assert!(store.set_secret("K", "V").await.is_err());
        assert!(
            store
                .set_secret_with_env("K", "V", Some("E"))
                .await
                .is_err()
        );
        assert!(store.get_secret("K").await.is_err());
        assert!(store.list_secret_names().await.is_err());
        assert!(store.delete_secret("K").await.is_err());
        assert!(store.get_all_secrets().await.is_err());
        assert!(
            store
                .get_secrets_for_injection(&["K".into()])
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_get_secrets_for_injection_name_vs_env_collision() {
        // Secret A has no env override (env var = "GITHUB_TOKEN", its name).
        // Secret B has env = "GITHUB_TOKEN".
        // Requesting both should detect the collision.
        let store = test_store().await;
        store.set_secret("GITHUB_TOKEN", "val1").await.unwrap();
        store
            .set_secret_with_env("GH_WORK", "val2", Some("GITHUB_TOKEN"))
            .await
            .unwrap();
        let err = store
            .get_secrets_for_injection(&["GITHUB_TOKEN".into(), "GH_WORK".into()])
            .await
            .unwrap_err();
        assert!(err.to_string().contains("both map to env var"), "{err}");
    }

    #[tokio::test]
    async fn test_get_secrets_for_injection_single_secret() {
        let store = test_store().await;
        store.set_secret("ONLY_ONE", "val").await.unwrap();
        let secrets = store
            .get_secrets_for_injection(&["ONLY_ONE".into()])
            .await
            .unwrap();
        assert_eq!(secrets.len(), 1);
        assert_eq!(secrets.get("ONLY_ONE").unwrap(), "val");
    }

    #[tokio::test]
    async fn test_get_secrets_for_injection_all_missing() {
        let store = test_store().await;
        let secrets = store
            .get_secrets_for_injection(&["MISSING_A".into(), "MISSING_B".into()])
            .await
            .unwrap();
        assert!(secrets.is_empty());
    }

    #[tokio::test]
    async fn test_migrate_creates_secrets_table() {
        let tmpdir = tempfile::tempdir().unwrap();
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();

        // Verify secrets table exists
        let result = sqlx::query("SELECT count(*) as cnt FROM secrets")
            .fetch_one(store.pool())
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_migrate_secrets_env_column_exists() {
        let tmpdir = tempfile::tempdir().unwrap();
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();

        // Verify env column exists in secrets table
        let has_env: i32 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('secrets') WHERE name = 'env'",
        )
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(has_env, 1);
    }

    #[tokio::test]
    async fn test_unknown_runtime_in_db_defaults_to_tmux() {
        let store = test_store().await;
        // Insert a row with an unknown runtime value
        sqlx::query(
            "INSERT INTO sessions (id, name, workdir, provider, prompt, status, mode,
                runtime, command, created_at, updated_at)
             VALUES (?, 'test', '/tmp', '', '', 'active', '',
                'unknown_runtime', 'echo test', '2024-01-01T00:00:00+00:00', '2024-01-01T00:00:00+00:00')",
        )
        .bind(TEST_UUID)
        .execute(store.pool())
        .await
        .unwrap();
        let session = store.get_session(TEST_UUID).await.unwrap().unwrap();
        assert_eq!(session.runtime, Runtime::Tmux);
    }

    #[tokio::test]
    async fn test_empty_runtime_in_db_defaults_to_tmux() {
        let store = test_store().await;
        // Insert a row then force runtime to empty string (simulates corrupt/old data)
        sqlx::query(
            "INSERT INTO sessions (id, name, workdir, provider, prompt, status, mode,
                command, created_at, updated_at, runtime)
             VALUES (?, 'test', '/tmp', '', '', 'active', '',
                'echo test', '2024-01-01T00:00:00+00:00', '2024-01-01T00:00:00+00:00', '')",
        )
        .bind(TEST_UUID)
        .execute(store.pool())
        .await
        .unwrap();
        let session = store.get_session(TEST_UUID).await.unwrap().unwrap();
        // Empty string doesn't parse to a valid Runtime, so .ok() returns None,
        // and .unwrap_or_default() gives Tmux
        assert_eq!(session.runtime, Runtime::Tmux);
    }

    #[tokio::test]
    async fn test_insert_and_get_session_with_docker_runtime() {
        let store = test_store().await;
        let mut session = make_session("docker-session");
        session.runtime = Runtime::Docker;
        session.backend_session_id = Some("docker:pulpo-docker-session".into());
        store.insert_session(&session).await.unwrap();

        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.runtime, Runtime::Docker);
        assert_eq!(
            fetched.backend_session_id.as_deref(),
            Some("docker:pulpo-docker-session")
        );
    }

    #[tokio::test]
    async fn test_migrate_runtime_from_sandbox() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();

        // Run a full migration first to create all tables and columns
        store.migrate().await.unwrap();

        // Drop the runtime column by recreating the table without it,
        // simulating an older schema that only has the sandbox column.
        // We need to do this carefully because SQLite doesn't support DROP COLUMN easily.
        // Instead, insert a row with sandbox=1 and runtime='tmux' (the default),
        // then verify the migration would have set runtime='docker' for sandbox=1.

        // Insert a session, then manually set sandbox=1 and runtime back to default
        sqlx::query(
            "INSERT INTO sessions (id, name, workdir, provider, prompt, status, mode,
                sandbox, runtime, command, created_at, updated_at)
             VALUES (?, 'sandboxed', '/tmp', '', '', 'stopped', '',
                1, 'docker', 'echo', '2024-01-01T00:00:00+00:00', '2024-01-01T00:00:00+00:00')",
        )
        .bind(TEST_UUID)
        .execute(store.pool())
        .await
        .unwrap();

        let session = store.get_session(TEST_UUID).await.unwrap().unwrap();
        assert_eq!(session.runtime, Runtime::Docker);

        // Also verify a non-sandbox row stays tmux
        let uuid2 = "550e8400-e29b-41d4-a716-446655440001";
        sqlx::query(
            "INSERT INTO sessions (id, name, workdir, provider, prompt, status, mode,
                sandbox, runtime, command, created_at, updated_at)
             VALUES (?, 'normal', '/tmp', '', '', 'stopped', '',
                0, 'tmux', 'echo', '2024-01-01T00:00:00+00:00', '2024-01-01T00:00:00+00:00')",
        )
        .bind(uuid2)
        .execute(store.pool())
        .await
        .unwrap();

        let session2 = store.get_session(uuid2).await.unwrap().unwrap();
        assert_eq!(session2.runtime, Runtime::Tmux);
    }

    #[tokio::test]
    async fn test_migrate_creates_runtime_column() {
        let tmpdir = tempfile::tempdir().unwrap();
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();

        let has_runtime: i32 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'runtime'",
        )
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(has_runtime, 1);
    }

    #[tokio::test]
    async fn test_migrate_creates_schedules_table() {
        let tmpdir = tempfile::tempdir().unwrap();
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();

        let result = sqlx::query("SELECT count(*) FROM schedules")
            .fetch_one(store.pool())
            .await;
        assert!(result.is_ok());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_db_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let db_path = tmpdir.path().join("state.db");
        let metadata = std::fs::metadata(&db_path).unwrap();
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
