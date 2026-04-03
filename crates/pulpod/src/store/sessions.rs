use std::fmt::Write;

use anyhow::Result;
use chrono::Utc;
use pulpo_common::api::ListSessionsQuery;
use pulpo_common::session::{Session, SessionStatus};

use super::Store;
use crate::store::rows::row_to_session;

impl Store {
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
                last_output_at, idle_since, idle_threshold_secs, worktree_path, worktree_branch,
                git_branch, git_commit, git_files_changed, git_insertions, git_deletions, git_ahead,
                runtime, created_at, updated_at)
             VALUES (?, ?, ?, '', '', ?, '', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
        .bind(&session.worktree_branch)
        .bind(&session.git_branch)
        .bind(&session.git_commit)
        .bind(session.git_files_changed.map(|v| i32::try_from(v).unwrap_or(i32::MAX)))
        .bind(session.git_insertions.map(|v| i32::try_from(v).unwrap_or(i32::MAX)))
        .bind(session.git_deletions.map(|v| i32::try_from(v).unwrap_or(i32::MAX)))
        .bind(session.git_ahead.map(|v| i32::try_from(v).unwrap_or(i32::MAX)))
        .bind(session.runtime.to_string())
        .bind(session.created_at.to_rfc3339())
        .bind(session.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_session(&self, id_or_name: &str) -> Result<Option<Session>> {
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

    pub async fn update_session_git_info(
        &self,
        id: &str,
        branch: Option<&str>,
        commit: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE sessions SET git_branch = ?, git_commit = ?, updated_at = ? WHERE id = ?",
        )
        .bind(branch)
        .bind(commit)
        .bind(Utc::now().to_rfc3339())
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_session_git_diff(
        &self,
        id: &str,
        files_changed: Option<u32>,
        insertions: Option<u32>,
        deletions: Option<u32>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE sessions SET git_files_changed = ?, git_insertions = ?, git_deletions = ?, updated_at = ? WHERE id = ?",
        )
        .bind(files_changed.map(|v| i32::try_from(v).unwrap_or(i32::MAX)))
        .bind(insertions.map(|v| i32::try_from(v).unwrap_or(i32::MAX)))
        .bind(deletions.map(|v| i32::try_from(v).unwrap_or(i32::MAX)))
        .bind(Utc::now().to_rfc3339())
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_session_git_ahead(&self, id: &str, ahead: Option<u32>) -> Result<()> {
        sqlx::query("UPDATE sessions SET git_ahead = ?, updated_at = ? WHERE id = ?")
            .bind(ahead.map(|v| i32::try_from(v).unwrap_or(i32::MAX)))
            .bind(Utc::now().to_rfc3339())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
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

    pub async fn cleanup_dead_sessions(&self) -> Result<u64> {
        let result = sqlx::query("DELETE FROM sessions WHERE status IN ('stopped', 'lost')")
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    pub const fn pool(&self) -> &sqlx::SqlitePool {
        &self.pool
    }

    pub fn data_dir(&self) -> &str {
        &self.data_dir
    }
}
