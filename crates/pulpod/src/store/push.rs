use anyhow::Result;
use chrono::Utc;
use sqlx::Row;

use super::{PushSubscription, Store};

impl Store {
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
}
