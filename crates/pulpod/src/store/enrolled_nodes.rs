use anyhow::Result;

use super::{EnrolledNode, Store};
use crate::store::rows::row_to_enrolled_node;

impl Store {
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
