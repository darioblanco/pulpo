use anyhow::Result;
use chrono::Utc;

use super::Store;

impl Store {
    pub async fn set_secret(&self, name: &str, value: &str) -> Result<()> {
        self.set_secret_with_env(name, value, None).await
    }

    pub async fn set_secret_with_env(
        &self,
        name: &str,
        value: &str,
        env: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT OR REPLACE INTO secrets (name, value, env, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(name)
        .bind(value)
        .bind(env)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_secret(&self, name: &str) -> Result<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as("SELECT value FROM secrets WHERE name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|(v,)| v))
    }

    pub async fn list_secret_names(&self) -> Result<Vec<(String, Option<String>, String)>> {
        let rows: Vec<(String, Option<String>, String)> =
            sqlx::query_as("SELECT name, env, created_at FROM secrets ORDER BY name")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows)
    }

    pub async fn get_secrets_for_injection(
        &self,
        names: &[String],
    ) -> Result<std::collections::HashMap<String, String>> {
        let mut result = std::collections::HashMap::new();
        let mut env_owners: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for name in names {
            let row: Option<(String, Option<String>)> =
                sqlx::query_as("SELECT value, env FROM secrets WHERE name = ?")
                    .bind(name)
                    .fetch_optional(&self.pool)
                    .await?;
            if let Some((value, env)) = row {
                let env_var = env.unwrap_or_else(|| name.clone());
                if let Some(prev_name) = env_owners.get(&env_var) {
                    anyhow::bail!(
                        "secrets '{prev_name}' and '{name}' both map to env var '{env_var}' — use only one"
                    );
                }
                env_owners.insert(env_var.clone(), name.clone());
                result.insert(env_var, value);
            }
        }
        Ok(result)
    }

    pub async fn delete_secret(&self, name: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM secrets WHERE name = ?")
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn get_all_secrets(&self) -> Result<std::collections::HashMap<String, String>> {
        let rows: Vec<(String, String)> = sqlx::query_as("SELECT name, value FROM secrets")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().collect())
    }
}
