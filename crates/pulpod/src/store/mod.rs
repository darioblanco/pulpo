use std::fmt::Write;

use anyhow::Result;
use chrono::{DateTime, Utc};
use pulpo_common::api::ListSessionsQuery;
use pulpo_common::guard::GuardConfig;
use pulpo_common::knowledge::{Knowledge, KnowledgeKind};
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
            sqlx::query("ALTER TABLE sessions ADD COLUMN ink TEXT")
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

        // Idempotent migration: rename persona → ink
        let has_persona: i32 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'persona'",
        )
        .fetch_one(&self.pool)
        .await?;
        if has_persona > 0 {
            sqlx::query("ALTER TABLE sessions RENAME COLUMN persona TO ink")
                .execute(&self.pool)
                .await?;
        }

        // Idempotent migration: rename tmux_session → backend_session_id
        let has_tmux_session: i32 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'tmux_session'",
        )
        .fetch_one(&self.pool)
        .await?;
        if has_tmux_session > 0 {
            sqlx::query("ALTER TABLE sessions RENAME COLUMN tmux_session TO backend_session_id")
                .execute(&self.pool)
                .await?;
        }

        // Idempotent migration: knowledge table for extracted session learnings
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS knowledge (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                scope_repo TEXT,
                scope_ink TEXT,
                title TEXT NOT NULL,
                body TEXT NOT NULL,
                tags TEXT NOT NULL DEFAULT '[]',
                relevance REAL NOT NULL DEFAULT 0.5,
                created_at TEXT NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;

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
                conversation_id, exit_code, backend_session_id,
                output_snapshot, guard_config,
                model, allowed_tools, system_prompt, metadata, ink,
                max_turns, max_budget_usd, output_format,
                intervention_reason, intervention_at,
                last_output_at, idle_since, waiting_for_input, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
        .bind(&session.backend_session_id)
        .bind(&session.output_snapshot)
        .bind(&guard_json)
        .bind(&session.model)
        .bind(&allowed_tools_json)
        .bind(&session.system_prompt)
        .bind(&metadata_json)
        .bind(&session.ink)
        .bind(max_turns_i32)
        .bind(session.max_budget_usd)
        .bind(&session.output_format)
        .bind(&session.intervention_reason)
        .bind(&intervention_at_str)
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

    // ── Knowledge CRUD ──────────────────────────────────────────────────

    pub async fn insert_knowledge(&self, k: &Knowledge) -> Result<()> {
        let tags_json = serde_json::to_string(&k.tags)?;
        sqlx::query(
            "INSERT INTO knowledge (id, session_id, kind, scope_repo, scope_ink,
                title, body, tags, relevance, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(k.id.to_string())
        .bind(k.session_id.to_string())
        .bind(k.kind.to_string())
        .bind(&k.scope_repo)
        .bind(&k.scope_ink)
        .bind(&k.title)
        .bind(&k.body)
        .bind(&tags_json)
        .bind(k.relevance)
        .bind(k.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_knowledge(&self, id: &str) -> Result<Option<Knowledge>> {
        let row = sqlx::query("SELECT * FROM knowledge WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|r| row_to_knowledge(&r)).transpose()
    }

    pub async fn list_knowledge(
        &self,
        session_id: Option<&str>,
        kind: Option<&str>,
        repo: Option<&str>,
        ink: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<Knowledge>> {
        let mut sql = String::from("SELECT * FROM knowledge WHERE 1=1");
        let mut binds: Vec<String> = Vec::new();

        if let Some(sid) = session_id {
            sql.push_str(" AND session_id = ?");
            binds.push(sid.to_owned());
        }
        if let Some(k) = kind {
            sql.push_str(" AND kind = ?");
            binds.push(k.to_owned());
        }
        if let Some(r) = repo {
            sql.push_str(" AND scope_repo = ?");
            binds.push(r.to_owned());
        }
        if let Some(i) = ink {
            sql.push_str(" AND scope_ink = ?");
            binds.push(i.to_owned());
        }

        sql.push_str(" ORDER BY created_at DESC");

        if let Some(lim) = limit {
            let _ = write!(sql, " LIMIT {lim}");
        }

        let mut q = sqlx::query(&sql);
        for bind in &binds {
            q = q.bind(bind);
        }

        let rows = q.fetch_all(&self.pool).await?;
        rows.iter().map(row_to_knowledge).collect()
    }

    /// Query knowledge relevant to a workdir/ink combination for context injection.
    /// Returns knowledge scoped to the repo, the ink, or global, ordered by relevance.
    pub async fn query_knowledge_for_context(
        &self,
        workdir: Option<&str>,
        ink: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Knowledge>> {
        // Match: exact repo, exact ink, repo+ink, or global (both NULL)
        let mut sql = String::from(
            "SELECT * FROM knowledge WHERE
                (scope_repo IS NULL OR scope_repo = ?)
                AND (scope_ink IS NULL OR scope_ink = ?)",
        );
        let _ = write!(
            sql,
            " ORDER BY relevance DESC, created_at DESC LIMIT {limit}"
        );

        let rows = sqlx::query(&sql)
            .bind(workdir.unwrap_or(""))
            .bind(ink.unwrap_or(""))
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(row_to_knowledge).collect()
    }

    pub async fn delete_knowledge(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM knowledge WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_knowledge_by_session(&self, session_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM knowledge WHERE session_id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
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
        backend_session_id: row.get("backend_session_id"),
        output_snapshot: row.get("output_snapshot"),
        guard_config,
        model: row.get("model"),
        allowed_tools,
        system_prompt: row.get("system_prompt"),
        metadata,
        ink: row.get("ink"),
        max_turns: {
            let v: Option<i32> = row.get("max_turns");
            v.map(i32::cast_unsigned)
        },
        max_budget_usd: row.get("max_budget_usd"),
        output_format: row.get("output_format"),
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

fn row_to_knowledge(row: &SqliteRow) -> Result<Knowledge> {
    let id_str: String = row.get("id");
    let session_id_str: String = row.get("session_id");
    let kind_str: String = row.get("kind");
    let tags_json: String = row.get("tags");
    let created_str: String = row.get("created_at");

    Ok(Knowledge {
        id: Uuid::parse_str(&id_str)?,
        session_id: Uuid::parse_str(&session_id_str)?,
        kind: kind_str
            .parse::<KnowledgeKind>()
            .map_err(|e| anyhow::anyhow!(e))?,
        scope_repo: row.get("scope_repo"),
        scope_ink: row.get("scope_ink"),
        title: row.get("title"),
        body: row.get("body"),
        tags: serde_json::from_str(&tags_json)?,
        relevance: row.get("relevance"),
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
            backend_session_id: Some(format!("pulpo-{name}")),
            output_snapshot: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
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
        assert_eq!(
            fetched.backend_session_id,
            Some("pulpo-test-roundtrip".into())
        );
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
            backend_session_id: None,
            output_snapshot: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
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
        assert!(fetched.backend_session_id.is_none());
        assert!(fetched.output_snapshot.is_none());
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
        use pulpo_common::guard::GuardConfig;

        let store = test_store().await;
        let mut session = make_session("guard-test");
        let strict = GuardConfig { unrestricted: true };
        session.guard_config = Some(strict.clone());

        store.insert_session(&session).await.unwrap();
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();

        assert!(fetched.guard_config.is_some());
        let gc = fetched.guard_config.unwrap();
        assert_eq!(gc, strict);
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
        session.ink = Some("reviewer".into());

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
        assert_eq!(fetched.ink, Some("reviewer".into()));
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
                backend_session_id TEXT,
                output_snapshot TEXT,
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
    async fn test_migrate_renames_tmux_session_to_backend_session_id() {
        let tmpdir = tempfile::tempdir().unwrap();
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();

        // Create legacy schema with tmux_session column (old name)
        sqlx::query(
            "CREATE TABLE sessions (
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
                output_snapshot TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .execute(store.pool())
        .await
        .unwrap();

        // Insert a row using the old column name
        sqlx::query(
            "INSERT INTO sessions (id, name, workdir, provider, prompt, status, mode, tmux_session, created_at, updated_at)
             VALUES ('test-id', 'test', '/tmp', 'claude', 'test', 'running', 'interactive', 'pulpo-test',
                     '2024-01-01T00:00:00+00:00', '2024-01-01T00:00:00+00:00')",
        )
        .execute(store.pool())
        .await
        .unwrap();

        // Run migration — should rename column
        store.migrate().await.unwrap();

        // Verify we can query the new column name
        let backend_id: String =
            sqlx::query_scalar("SELECT backend_session_id FROM sessions WHERE id = 'test-id'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(backend_id, "pulpo-test");

        // Running migration again should be idempotent
        store.migrate().await.unwrap();
    }

    #[tokio::test]
    async fn test_migrate_closed_pool_error() {
        let store = test_store().await;
        store.pool().close().await;
        let result = store.migrate().await;
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

    // ── Knowledge store tests ───────────────────────────────────────────

    fn make_knowledge(title: &str) -> Knowledge {
        Knowledge {
            id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            kind: KnowledgeKind::Summary,
            scope_repo: Some("/tmp/repo".into()),
            scope_ink: Some("coder".into()),
            title: title.into(),
            body: "Detailed body text.".into(),
            tags: vec!["claude".into(), "completed".into()],
            relevance: 0.5,
            created_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_insert_and_get_knowledge() {
        let store = test_store().await;
        let k = make_knowledge("Test finding");
        store.insert_knowledge(&k).await.unwrap();

        let fetched = store
            .get_knowledge(&k.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.id, k.id);
        assert_eq!(fetched.session_id, k.session_id);
        assert_eq!(fetched.kind, KnowledgeKind::Summary);
        assert_eq!(fetched.scope_repo.as_deref(), Some("/tmp/repo"));
        assert_eq!(fetched.scope_ink.as_deref(), Some("coder"));
        assert_eq!(fetched.title, "Test finding");
        assert_eq!(fetched.body, "Detailed body text.");
        assert_eq!(fetched.tags, vec!["claude", "completed"]);
        assert!((fetched.relevance - 0.5).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_get_knowledge_not_found() {
        let store = test_store().await;
        let result = store.get_knowledge("nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_knowledge_with_null_scopes() {
        let store = test_store().await;
        let k = Knowledge {
            scope_repo: None,
            scope_ink: None,
            ..make_knowledge("global finding")
        };
        store.insert_knowledge(&k).await.unwrap();

        let fetched = store
            .get_knowledge(&k.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.scope_repo.is_none());
        assert!(fetched.scope_ink.is_none());
    }

    #[tokio::test]
    async fn test_knowledge_failure_kind() {
        let store = test_store().await;
        let k = Knowledge {
            kind: KnowledgeKind::Failure,
            relevance: 0.8,
            ..make_knowledge("crash report")
        };
        store.insert_knowledge(&k).await.unwrap();

        let fetched = store
            .get_knowledge(&k.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.kind, KnowledgeKind::Failure);
        assert!((fetched.relevance - 0.8).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_list_knowledge_all() {
        let store = test_store().await;
        store
            .insert_knowledge(&make_knowledge("one"))
            .await
            .unwrap();
        store
            .insert_knowledge(&make_knowledge("two"))
            .await
            .unwrap();

        let all = store
            .list_knowledge(None, None, None, None, None)
            .await
            .unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_list_knowledge_by_session() {
        let store = test_store().await;
        let k1 = make_knowledge("from-session-1");
        let k2 = Knowledge {
            session_id: k1.session_id,
            ..make_knowledge("also-from-session-1")
        };
        let k3 = make_knowledge("different-session");

        store.insert_knowledge(&k1).await.unwrap();
        store.insert_knowledge(&k2).await.unwrap();
        store.insert_knowledge(&k3).await.unwrap();

        let filtered = store
            .list_knowledge(Some(&k1.session_id.to_string()), None, None, None, None)
            .await
            .unwrap();
        assert_eq!(filtered.len(), 2);
    }

    #[tokio::test]
    async fn test_list_knowledge_by_kind() {
        let store = test_store().await;
        store
            .insert_knowledge(&make_knowledge("summary-1"))
            .await
            .unwrap();
        store
            .insert_knowledge(&Knowledge {
                kind: KnowledgeKind::Failure,
                ..make_knowledge("failure-1")
            })
            .await
            .unwrap();

        let failures = store
            .list_knowledge(None, Some("failure"), None, None, None)
            .await
            .unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].kind, KnowledgeKind::Failure);
    }

    #[tokio::test]
    async fn test_list_knowledge_by_repo() {
        let store = test_store().await;
        store
            .insert_knowledge(&make_knowledge("repo-match"))
            .await
            .unwrap();
        store
            .insert_knowledge(&Knowledge {
                scope_repo: Some("/other/repo".into()),
                ..make_knowledge("other-repo")
            })
            .await
            .unwrap();

        let filtered = store
            .list_knowledge(None, None, Some("/tmp/repo"), None, None)
            .await
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "repo-match");
    }

    #[tokio::test]
    async fn test_list_knowledge_by_ink() {
        let store = test_store().await;
        store
            .insert_knowledge(&make_knowledge("coder-match"))
            .await
            .unwrap();
        store
            .insert_knowledge(&Knowledge {
                scope_ink: Some("reviewer".into()),
                ..make_knowledge("reviewer-match")
            })
            .await
            .unwrap();

        let filtered = store
            .list_knowledge(None, None, None, Some("coder"), None)
            .await
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "coder-match");
    }

    #[tokio::test]
    async fn test_list_knowledge_with_limit() {
        let store = test_store().await;
        for i in 0..5 {
            store
                .insert_knowledge(&make_knowledge(&format!("item-{i}")))
                .await
                .unwrap();
        }

        let limited = store
            .list_knowledge(None, None, None, None, Some(3))
            .await
            .unwrap();
        assert_eq!(limited.len(), 3);
    }

    #[tokio::test]
    async fn test_list_knowledge_combined_filters() {
        let store = test_store().await;
        let session_id = Uuid::new_v4();
        store
            .insert_knowledge(&Knowledge {
                session_id,
                kind: KnowledgeKind::Failure,
                scope_repo: Some("/tmp/repo".into()),
                scope_ink: Some("coder".into()),
                ..make_knowledge("target")
            })
            .await
            .unwrap();
        store
            .insert_knowledge(&Knowledge {
                session_id,
                kind: KnowledgeKind::Summary,
                ..make_knowledge("wrong-kind")
            })
            .await
            .unwrap();

        let filtered = store
            .list_knowledge(
                Some(&session_id.to_string()),
                Some("failure"),
                Some("/tmp/repo"),
                Some("coder"),
                None,
            )
            .await
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "target");
    }

    #[tokio::test]
    async fn test_query_knowledge_for_context() {
        let store = test_store().await;
        // Exact repo+ink match
        store
            .insert_knowledge(&Knowledge {
                scope_repo: Some("/tmp/repo".into()),
                scope_ink: Some("coder".into()),
                relevance: 0.9,
                ..make_knowledge("exact-match")
            })
            .await
            .unwrap();
        // Global (both null)
        store
            .insert_knowledge(&Knowledge {
                scope_repo: None,
                scope_ink: None,
                relevance: 0.3,
                ..make_knowledge("global")
            })
            .await
            .unwrap();
        // Different repo — should NOT match
        store
            .insert_knowledge(&Knowledge {
                scope_repo: Some("/other/repo".into()),
                scope_ink: Some("coder".into()),
                relevance: 0.7,
                ..make_knowledge("other-repo")
            })
            .await
            .unwrap();

        let results = store
            .query_knowledge_for_context(Some("/tmp/repo"), Some("coder"), 10)
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        // Ordered by relevance DESC
        assert_eq!(results[0].title, "exact-match");
        assert_eq!(results[1].title, "global");
    }

    #[tokio::test]
    async fn test_query_knowledge_for_context_no_workdir() {
        let store = test_store().await;
        store
            .insert_knowledge(&Knowledge {
                scope_repo: None,
                scope_ink: None,
                ..make_knowledge("global-only")
            })
            .await
            .unwrap();
        store
            .insert_knowledge(&Knowledge {
                scope_repo: Some("/some/repo".into()),
                scope_ink: None,
                ..make_knowledge("scoped")
            })
            .await
            .unwrap();

        let results = store
            .query_knowledge_for_context(None, None, 10)
            .await
            .unwrap();
        // Only global matches (scope_repo IS NULL matches, but "/some/repo" != "")
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "global-only");
    }

    #[tokio::test]
    async fn test_query_knowledge_for_context_with_limit() {
        let store = test_store().await;
        for i in 0..5 {
            store
                .insert_knowledge(&Knowledge {
                    scope_repo: None,
                    scope_ink: None,
                    relevance: f64::from(i) * 0.1,
                    ..make_knowledge(&format!("item-{i}"))
                })
                .await
                .unwrap();
        }

        let results = store
            .query_knowledge_for_context(None, None, 2)
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_delete_knowledge() {
        let store = test_store().await;
        let k = make_knowledge("to-delete");
        store.insert_knowledge(&k).await.unwrap();

        store.delete_knowledge(&k.id.to_string()).await.unwrap();
        let result = store.get_knowledge(&k.id.to_string()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_delete_knowledge_by_session() {
        let store = test_store().await;
        let session_id = Uuid::new_v4();
        store
            .insert_knowledge(&Knowledge {
                session_id,
                ..make_knowledge("k1")
            })
            .await
            .unwrap();
        store
            .insert_knowledge(&Knowledge {
                session_id,
                ..make_knowledge("k2")
            })
            .await
            .unwrap();
        store
            .insert_knowledge(&make_knowledge("other"))
            .await
            .unwrap();

        store
            .delete_knowledge_by_session(&session_id.to_string())
            .await
            .unwrap();

        let all = store
            .list_knowledge(None, None, None, None, None)
            .await
            .unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].title, "other");
    }

    #[tokio::test]
    async fn test_knowledge_empty_tags() {
        let store = test_store().await;
        let k = Knowledge {
            tags: vec![],
            ..make_knowledge("no-tags")
        };
        store.insert_knowledge(&k).await.unwrap();

        let fetched = store
            .get_knowledge(&k.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.tags.is_empty());
    }

    #[tokio::test]
    async fn test_knowledge_invalid_uuid() {
        let store = test_store().await;
        sqlx::query(
            "INSERT INTO knowledge (id, session_id, kind, title, body, tags, relevance, created_at)
             VALUES ('not-a-uuid', ?, 'summary', 'test', 'body', '[]', 0.5, '2024-01-01T00:00:00+00:00')",
        )
        .bind(Uuid::new_v4().to_string())
        .execute(store.pool())
        .await
        .unwrap();

        let result = store.get_knowledge("not-a-uuid").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_knowledge_invalid_kind() {
        let store = test_store().await;
        sqlx::query(
            "INSERT INTO knowledge (id, session_id, kind, title, body, tags, relevance, created_at)
             VALUES (?, ?, 'invalid_kind', 'test', 'body', '[]', 0.5, '2024-01-01T00:00:00+00:00')",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(Uuid::new_v4().to_string())
        .execute(store.pool())
        .await
        .unwrap();

        let all = store.list_knowledge(None, None, None, None, None).await;
        assert!(all.is_err());
    }

    #[tokio::test]
    async fn test_knowledge_invalid_tags_json() {
        let store = test_store().await;
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO knowledge (id, session_id, kind, title, body, tags, relevance, created_at)
             VALUES (?, ?, 'summary', 'test', 'body', 'not-json', 0.5, '2024-01-01T00:00:00+00:00')",
        )
        .bind(&id)
        .bind(Uuid::new_v4().to_string())
        .execute(store.pool())
        .await
        .unwrap();

        let result = store.get_knowledge(&id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_knowledge_invalid_created_at() {
        let store = test_store().await;
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO knowledge (id, session_id, kind, title, body, tags, relevance, created_at)
             VALUES (?, ?, 'summary', 'test', 'body', '[]', 0.5, 'not-a-date')",
        )
        .bind(&id)
        .bind(Uuid::new_v4().to_string())
        .execute(store.pool())
        .await
        .unwrap();

        let result = store.get_knowledge(&id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_knowledge_invalid_session_uuid() {
        let store = test_store().await;
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO knowledge (id, session_id, kind, title, body, tags, relevance, created_at)
             VALUES (?, 'not-a-uuid', 'summary', 'test', 'body', '[]', 0.5, '2024-01-01T00:00:00+00:00')",
        )
        .bind(&id)
        .execute(store.pool())
        .await
        .unwrap();

        let result = store.get_knowledge(&id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_knowledge_after_table_dropped() {
        let store = test_store().await;
        sqlx::query("DROP TABLE knowledge")
            .execute(store.pool())
            .await
            .unwrap();

        let result = store.list_knowledge(None, None, None, None, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_insert_knowledge_after_table_dropped() {
        let store = test_store().await;
        sqlx::query("DROP TABLE knowledge")
            .execute(store.pool())
            .await
            .unwrap();

        let result = store.insert_knowledge(&make_knowledge("fail")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_knowledge_after_table_dropped() {
        let store = test_store().await;
        sqlx::query("DROP TABLE knowledge")
            .execute(store.pool())
            .await
            .unwrap();

        let result = store.delete_knowledge("any-id").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_knowledge_by_session_after_table_dropped() {
        let store = test_store().await;
        sqlx::query("DROP TABLE knowledge")
            .execute(store.pool())
            .await
            .unwrap();

        let result = store.delete_knowledge_by_session("any-id").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_query_knowledge_for_context_after_table_dropped() {
        let store = test_store().await;
        sqlx::query("DROP TABLE knowledge")
            .execute(store.pool())
            .await
            .unwrap();

        let result = store.query_knowledge_for_context(None, None, 10).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_migrate_creates_knowledge_table() {
        let tmpdir = tempfile::tempdir().unwrap();
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();

        let count: i32 = sqlx::query_scalar("SELECT count(*) FROM knowledge")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_list_knowledge_empty() {
        let store = test_store().await;
        let all = store
            .list_knowledge(None, None, None, None, None)
            .await
            .unwrap();
        assert!(all.is_empty());
    }
}
