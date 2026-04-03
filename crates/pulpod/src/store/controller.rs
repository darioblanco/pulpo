use anyhow::Result;
use chrono::{DateTime, Utc};
use pulpo_common::api::SessionIndexEntry;

use super::{EnrolledNode, Store};
use crate::store::rows::{row_to_enrolled_node, row_to_session_index_entry};

impl Store {
    pub async fn upsert_controller_session_index_entry(
        &self,
        entry: &SessionIndexEntry,
    ) -> Result<()> {
        sqlx::query(
            "INSERT OR REPLACE INTO controller_session_index
             (session_id, node_name, node_address, session_name, status, command, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&entry.session_id)
        .bind(&entry.node_name)
        .bind(&entry.node_address)
        .bind(&entry.session_name)
        .bind(&entry.status)
        .bind(&entry.command)
        .bind(&entry.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_controller_session_index_entry(&self, session_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM controller_session_index WHERE session_id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_controller_session_index_entries(&self) -> Result<Vec<SessionIndexEntry>> {
        let rows = sqlx::query("SELECT * FROM controller_session_index ORDER BY session_name")
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(row_to_session_index_entry).collect()
    }

    pub async fn touch_controller_node(&self, node_name: &str, seen_at: &str) -> Result<()> {
        sqlx::query(
            "INSERT OR REPLACE INTO controller_nodes (node_name, last_seen_at) VALUES (?, ?)",
        )
        .bind(node_name)
        .bind(seen_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_controller_nodes(&self) -> Result<Vec<(String, DateTime<Utc>)>> {
        let rows =
            sqlx::query("SELECT node_name, last_seen_at FROM controller_nodes ORDER BY node_name")
                .fetch_all(&self.pool)
                .await?;
        rows.into_iter()
            .map(|row| {
                let node_name: String = sqlx::Row::get(&row, "node_name");
                let last_seen_at: String = sqlx::Row::get(&row, "last_seen_at");
                let parsed = DateTime::parse_from_rfc3339(&last_seen_at)?.with_timezone(&Utc);
                Ok((node_name, parsed))
            })
            .collect()
    }

    pub async fn enroll_controller_node(
        &self,
        node_name: &str,
        token_hash: &str,
        seen_at: Option<&str>,
        last_seen_address: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO controller_enrolled_nodes (node_name, token_hash, last_seen_at, last_seen_address)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(node_name) DO UPDATE SET
               token_hash = excluded.token_hash,
               last_seen_at = excluded.last_seen_at,
               last_seen_address = excluded.last_seen_address",
        )
        .bind(node_name)
        .bind(token_hash)
        .bind(seen_at)
        .bind(last_seen_address)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_enrolled_controller_node_by_name(
        &self,
        node_name: &str,
    ) -> Result<Option<EnrolledNode>> {
        let row = sqlx::query(
            "SELECT node_name, token_hash, last_seen_at, last_seen_address
             FROM controller_enrolled_nodes WHERE node_name = ?",
        )
        .bind(node_name)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| row_to_enrolled_node(&row)).transpose()
    }

    pub async fn get_enrolled_controller_node_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<EnrolledNode>> {
        let row = sqlx::query(
            "SELECT node_name, token_hash, last_seen_at, last_seen_address
             FROM controller_enrolled_nodes WHERE token_hash = ?",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| row_to_enrolled_node(&row)).transpose()
    }

    pub async fn list_enrolled_controller_nodes(&self) -> Result<Vec<EnrolledNode>> {
        let rows = sqlx::query(
            "SELECT node_name, token_hash, last_seen_at, last_seen_address
             FROM controller_enrolled_nodes ORDER BY node_name",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_enrolled_node).collect()
    }

    pub async fn touch_enrolled_controller_node(
        &self,
        node_name: &str,
        seen_at: &str,
        last_seen_address: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE controller_enrolled_nodes
             SET last_seen_at = ?, last_seen_address = COALESCE(?, last_seen_address)
             WHERE node_name = ?",
        )
        .bind(seen_at)
        .bind(last_seen_address)
        .bind(node_name)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
