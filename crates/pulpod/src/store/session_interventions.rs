use anyhow::Result;
use chrono::Utc;
use pulpo_common::session::InterventionCode;

use super::{InterventionEvent, Store};
use crate::store::rows::row_to_intervention_event;

impl Store {
    /// Record a watchdog intervention (budget/idle-timeout breaker): the audit-trail
    /// row in `intervention_events`, plus the session itself moving to `done` with
    /// `status_reason` set to the intervention code's own `Display` string
    /// (`idle_timeout`/`budget_exceeded`/`memory_pressure`) — see ADR 0009's
    /// five-state model. `pulpo ls`/the web UI render this as e.g. `done (budget
    /// exceeded)`.
    ///
    /// The session-row update is a compare-and-set (`WHERE status IN (...)`), same
    /// as `session::manager::resolve_dead_backend_session`/`mark_session_stopped`:
    /// an intervention kill can race the watchdog's own eager dead-backend check
    /// (killing the backend makes `is_alive()` false right as this runs) or a
    /// concurrent `pulpo stop`. Returns `true` only when this call actually made
    /// the transition — callers must skip emitting the intervention event on
    /// `false`, and the audit-trail row is only inserted when it's `true` (an
    /// intervention that lost the race shouldn't leave an orphaned audit record
    /// for a status change that never happened).
    pub async fn update_session_intervention(
        &self,
        id: &str,
        code: InterventionCode,
        reason: &str,
    ) -> Result<bool> {
        let now = Utc::now().to_rfc3339();
        let code_str = code.to_string();
        let result = sqlx::query(
            "UPDATE sessions SET intervention_code = ?, intervention_reason = ?, intervention_at = ?, status = 'done', status_reason = ?, updated_at = ? \
             WHERE id = ? AND status IN ('starting', 'working', 'waiting')",
        )
        .bind(&code_str)
        .bind(reason)
        .bind(&now)
        .bind(&code_str)
        .bind(&now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        let transitioned = result.rows_affected() > 0;
        if transitioned {
            sqlx::query(
                "INSERT INTO intervention_events (session_id, code, reason, created_at) VALUES (?, ?, ?, ?)",
            )
            .bind(id)
            .bind(&code_str)
            .bind(reason)
            .bind(&now)
            .execute(&self.pool)
            .await?;
        }
        Ok(transitioned)
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
