use anyhow::Result;
use chrono::{DateTime, Utc};
use pulpo_common::session::InterventionCode;
use sqlx::SqlitePool;
use sqlx::migrate::Migrator;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// A Web Push subscription stored for sending push notifications.
#[derive(Debug, Clone)]
pub struct PushSubscription {
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
}

/// A single intervention event for audit trail purposes.
#[derive(Debug, Clone)]
pub struct InterventionEvent {
    pub id: i64,
    pub session_id: String,
    pub code: Option<InterventionCode>,
    pub reason: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct EnrolledNode {
    pub node_name: String,
    pub token_hash: String,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub last_seen_address: Option<String>,
}

#[derive(Clone)]
pub struct Store {
    pub(super) pool: SqlitePool,
    pub(super) data_dir: String,
}

impl Store {
    pub async fn new(data_dir: &str) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let db_path = format!("{data_dir}/state.db");
        let url = format!("sqlite:{db_path}?mode=rwc");
        let pool = SqlitePool::connect(&url).await?;
        Ok(Self {
            pool,
            data_dir: data_dir.to_owned(),
        })
    }

    pub async fn migrate(&self) -> Result<()> {
        self.reject_unsupported_legacy_schema().await?;
        MIGRATOR.run(&self.pool).await?;
        self.enforce_db_permissions();

        Ok(())
    }

    async fn reject_unsupported_legacy_schema(&self) -> Result<()> {
        let has_sqlx_migrations: i32 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
        )
        .fetch_one(&self.pool)
        .await?;

        if has_sqlx_migrations > 0 {
            return Ok(());
        }

        let has_sessions_table: i32 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'sessions'",
        )
        .fetch_one(&self.pool)
        .await?;

        if has_sessions_table > 0 {
            anyhow::bail!(
                "unsupported legacy database schema detected; delete {}/state.db to reinitialize",
                self.data_dir
            );
        }

        Ok(())
    }

    fn enforce_db_permissions(&self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let db_path = format!("{}/state.db", self.data_dir);
            if let Ok(metadata) = std::fs::metadata(&db_path) {
                let mut perms = metadata.permissions();
                perms.set_mode(0o600);
                let _ = std::fs::set_permissions(&db_path, perms);
            }
        }
    }
}
