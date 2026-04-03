use std::collections::HashMap;

use anyhow::Result;
use chrono::Utc;
use sqlx::Row;

use super::Store;

impl Store {
    pub async fn update_session_output_snapshot(&self, id: &str, snapshot: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
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

    pub async fn update_session_metadata_field(
        &self,
        id: &str,
        key: &str,
        value: &str,
    ) -> Result<()> {
        let mut map = self.load_session_metadata(id).await?;
        map.insert(key.to_owned(), value.to_owned());
        self.save_session_metadata(id, &map).await
    }

    pub async fn remove_session_metadata_field(&self, id: &str, key: &str) -> Result<()> {
        let mut map = self.load_session_metadata(id).await?;
        map.remove(key);
        self.save_session_metadata(id, &map).await
    }

    pub async fn batch_update_session_metadata(
        &self,
        id: &str,
        updates: &[(&str, &str)],
        removes: &[&str],
    ) -> Result<()> {
        let mut map = self.load_session_metadata(id).await?;
        for &(key, value) in updates {
            map.insert(key.to_owned(), value.to_owned());
        }
        for &key in removes {
            map.remove(key);
        }
        self.save_session_metadata(id, &map).await
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

    async fn load_session_metadata(&self, id: &str) -> Result<HashMap<String, String>> {
        let row = sqlx::query("SELECT metadata FROM sessions WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await?;
        let existing: Option<String> = row.get("metadata");
        existing
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map(Option::unwrap_or_default)
            .map_err(Into::into)
    }

    async fn save_session_metadata(&self, id: &str, map: &HashMap<String, String>) -> Result<()> {
        let json = serde_json::to_string(map)?;
        sqlx::query("UPDATE sessions SET metadata = ?, updated_at = ? WHERE id = ?")
            .bind(&json)
            .bind(Utc::now().to_rfc3339())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
