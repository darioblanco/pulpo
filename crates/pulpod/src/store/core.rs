use anyhow::Result;
use chrono::{DateTime, Utc};
use pulpo_common::session::InterventionCode;
use sqlx::SqlitePool;
use sqlx::migrate::Migrator;
use tracing::warn;

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

/// A durable webhook-delivery row from the `webhook_outbox` table.
///
/// One pending/delivered/dead delivery attempt of a single canonical event to a
/// single endpoint. The stored `envelope_json` is posted verbatim on every
/// retry so the receiver can dedupe on the stable `event_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookOutboxRow {
    pub id: i64,
    pub endpoint: String,
    pub event_id: String,
    pub envelope_json: String,
    pub status: String,
    pub attempts: i64,
    pub next_attempt_at: String,
    pub last_error: Option<String>,
    pub created_at: String,
    pub delivered_at: Option<String>,
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
        self.warn_before_dropping_secrets().await?;
        MIGRATOR.run(&self.pool).await?;
        self.enforce_db_permissions();

        Ok(())
    }

    /// Migration 0008 drops the `secrets` table (and `schedules.secrets` column):
    /// the secrets store was removed because every supported agent reads its own
    /// credentials from its own config now. That migration is irreversible — there
    /// is no `pulpo-secrets-backup` export tool — so warn loudly here, before it
    /// runs, if a pre-0008 database still has rows in `secrets`. This never blocks
    /// startup; it only gives the operator a chance to notice before the data is
    /// gone (downgrading to pulpo 0.1.1 is the only way to read it back out).
    async fn warn_before_dropping_secrets(&self) -> Result<()> {
        let has_secrets_table: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'secrets'",
        )
        .fetch_one(&self.pool)
        .await?;
        if has_secrets_table == 0 {
            return Ok(());
        }

        let secret_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM secrets")
            .fetch_one(&self.pool)
            .await?;
        if secret_count > 0 {
            warn!(
                secret_count,
                "store: the secrets table is about to be dropped irreversibly by migration \
                 0008 — {secret_count} stored secret(s) will be lost. There is no export \
                 tool; downgrade to pulpo 0.1.1 first if you need to read them out before \
                 upgrading."
            );
        }

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

/// Shared test-only builder: a tempdir-backed, migrated `Store`. The tempdir is
/// leaked so it persists for the test's lifetime (mirrors the pattern every
/// call site used to hand-roll).
#[cfg(test)]
pub async fn test_store() -> Store {
    let tmpdir = tempfile::tempdir().unwrap();
    let tmpdir = Box::leak(Box::new(tmpdir));
    let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
    store.migrate().await.unwrap();
    store
}
