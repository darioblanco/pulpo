use anyhow::Result;
use chrono::Utc;
use sqlx::Row;

use super::{Store, WebhookOutboxRow};
use crate::store::rows::row_to_webhook_outbox;

impl Store {
    /// Enqueue a pending webhook delivery.
    ///
    /// `next_attempt_at` is when the row first becomes due (normally "now" for an
    /// immediate first attempt). The row carries the exact serialized canonical
    /// `Event` (`envelope_json`) that will be posted verbatim on every retry.
    pub async fn enqueue_webhook(
        &self,
        endpoint: &str,
        event_id: &str,
        envelope_json: &str,
        next_attempt_at: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO webhook_outbox \
             (endpoint, event_id, envelope_json, status, attempts, next_attempt_at, created_at) \
             VALUES (?, ?, ?, 'pending', 0, ?, ?)",
        )
        .bind(endpoint)
        .bind(event_id)
        .bind(envelope_json)
        .bind(next_attempt_at)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Fetch due pending deliveries (`status='pending'` AND `next_attempt_at` <=
    /// now), oldest first, capped at `limit`.
    pub async fn fetch_due_webhook_deliveries(
        &self,
        now_rfc3339: &str,
        limit: usize,
    ) -> Result<Vec<WebhookOutboxRow>> {
        let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = sqlx::query(
            "SELECT id, endpoint, event_id, envelope_json, status, attempts, \
                    next_attempt_at, last_error, created_at, delivered_at \
             FROM webhook_outbox \
             WHERE status = 'pending' AND next_attempt_at <= ? \
             ORDER BY id ASC LIMIT ?",
        )
        .bind(now_rfc3339)
        .bind(limit_i64)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_webhook_outbox).collect()
    }

    /// Mark a delivery row as successfully delivered.
    pub async fn mark_webhook_delivered(&self, id: i64, delivered_at: &str) -> Result<()> {
        sqlx::query(
            "UPDATE webhook_outbox \
             SET status = 'delivered', delivered_at = ?, last_error = NULL \
             WHERE id = ?",
        )
        .bind(delivered_at)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Reschedule a failed delivery for a later retry, keeping it pending.
    ///
    /// Records the new attempt count, the next due time, and the last error.
    pub async fn reschedule_webhook(
        &self,
        id: i64,
        attempts: i64,
        next_attempt_at: &str,
        last_error: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE webhook_outbox \
             SET attempts = ?, next_attempt_at = ?, last_error = ?, status = 'pending' \
             WHERE id = ?",
        )
        .bind(attempts)
        .bind(next_attempt_at)
        .bind(last_error)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Mark a delivery row as permanently failed (max attempts reached or the
    /// endpoint no longer exists in config).
    pub async fn mark_webhook_dead(&self, id: i64, last_error: &str) -> Result<()> {
        sqlx::query("UPDATE webhook_outbox SET status = 'dead', last_error = ? WHERE id = ?")
            .bind(last_error)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Count outbox rows grouped by status, for observability.
    pub async fn count_webhook_outbox_by_status(&self) -> Result<Vec<(String, i64)>> {
        let rows =
            sqlx::query("SELECT status, COUNT(*) AS cnt FROM webhook_outbox GROUP BY status")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows
            .iter()
            .map(|r| (r.get::<String, _>("status"), r.get::<i64, _>("cnt")))
            .collect())
    }
}
