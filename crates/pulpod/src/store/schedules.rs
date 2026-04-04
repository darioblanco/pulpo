use anyhow::Result;
use chrono::Utc;
use pulpo_common::session::Session;

use super::Store;
use crate::store::rows::{row_to_schedule, row_to_session};

impl Store {
    pub async fn insert_schedule(&self, schedule: &pulpo_common::api::Schedule) -> Result<()> {
        let secrets_json = serde_json::to_string(&schedule.secrets)?;
        sqlx::query(
            "INSERT INTO schedules (
                id, name, cron, command, workdir, target_node, ink, description,
                runtime, secrets, worktree, worktree_base, enabled,
                last_run_at, last_session_id, last_attempted_at, last_error, created_at
             )
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&schedule.id)
        .bind(&schedule.name)
        .bind(&schedule.cron)
        .bind(&schedule.command)
        .bind(&schedule.workdir)
        .bind(&schedule.target_node)
        .bind(&schedule.ink)
        .bind(&schedule.description)
        .bind(&schedule.runtime)
        .bind(&secrets_json)
        .bind(schedule.worktree)
        .bind(&schedule.worktree_base)
        .bind(schedule.enabled)
        .bind(&schedule.last_run_at)
        .bind(&schedule.last_session_id)
        .bind(&schedule.last_attempted_at)
        .bind(&schedule.last_error)
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
        sqlx::query("UPDATE schedules SET last_run_at = ?, last_session_id = ?, last_attempted_at = ?, last_error = NULL WHERE id = ?")
            .bind(&now)
            .bind(session_id)
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn record_schedule_failure(&self, id: &str, error: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE schedules SET last_attempted_at = ?, last_error = ? WHERE id = ?")
            .bind(&now)
            .bind(error)
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
}
