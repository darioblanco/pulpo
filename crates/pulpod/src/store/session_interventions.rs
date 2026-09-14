use anyhow::Result;
use chrono::Utc;
use pulpo_common::session::InterventionCode;

use super::{InterventionEvent, Store};
use crate::store::rows::row_to_intervention_event;

impl Store {
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

    /// Delete every intervention event recorded for a session. There's no foreign-key
    /// cascade on `intervention_events.session_id` (see migration 0001), so a session
    /// purge (`pulpo rm`, `stop --purge`) must call this itself alongside the row
    /// delete — otherwise these rows outlive the session forever with no code path
    /// left that can ever look them up again.
    pub async fn delete_intervention_events(&self, session_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM intervention_events WHERE session_id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
