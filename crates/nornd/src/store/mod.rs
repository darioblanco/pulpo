use anyhow::Result;
use sqlx::SqlitePool;

pub struct Store {
    pool: SqlitePool,
}

impl Store {
    pub async fn new(data_dir: &str) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let db_path = format!("{data_dir}/state.db");
        let url = format!("sqlite:{db_path}?mode=rwc");
        let pool = SqlitePool::connect(&url).await?;
        Ok(Self { pool })
    }

    pub async fn migrate(&self) -> Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                repo_path TEXT NOT NULL,
                provider TEXT NOT NULL,
                prompt TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'creating',
                conversation_id TEXT,
                exit_code INTEGER,
                tmux_session TEXT,
                docker_container TEXT,
                output_snapshot TEXT,
                git_branch TEXT,
                git_sha TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    #[allow(dead_code)]
    pub const fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}
