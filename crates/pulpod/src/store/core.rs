use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use pulpo_common::session::InterventionCode;
use sqlx::SqlitePool;
use sqlx::migrate::Migrator;
use tracing::{error, info, warn};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// How many `state.db.pre-*` migration backups to keep (see
/// [`Store::backup_before_migrating`]) — oldest are pruned beyond this.
const MAX_PRE_MIGRATION_BACKUPS: usize = 3;

/// A single intervention event for audit trail purposes.
#[derive(Debug, Clone)]
pub struct InterventionEvent {
    pub id: i64,
    pub session_id: String,
    pub code: Option<InterventionCode>,
    pub reason: String,
    pub created_at: DateTime<Utc>,
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
        self.warn_before_dropping_push_subscriptions().await?;
        if self.has_pending_migrations().await? {
            self.backup_before_migrating().await?;
        }
        MIGRATOR.run(&self.pool).await?;
        self.warm_up_after_migrating().await;
        self.enforce_db_permissions();

        Ok(())
    }

    /// Work around a `sqlx-sqlite` 0.8.6 defect: on a connection that has just
    /// executed an `ALTER TABLE ... ADD COLUMN` (as several migrations here do),
    /// the *first* subsequent query bound with 2+ parameters against that table
    /// mis-sizes its cached column metadata and panics the connection's worker
    /// thread (`index out of bounds` in `SqliteRow::current`) — which silently
    /// kills that connection for the rest of the pool's life (later queries on it
    /// return no rows instead of erroring). Reproduced independently of this
    /// crate's own SQL — a bare `ALTER TABLE t ADD COLUMN x` followed directly by
    /// any 2-parameter `SELECT` on `t` triggers it. A single zero-bind query
    /// against the *same table* on the connection in between avoids it (a
    /// zero-bind query against an unrelated table/expression, e.g. plain `SELECT
    /// 1`, was not reliably enough in testing with many prior statements on the
    /// connection — `SELECT * FROM sessions LIMIT 0` is). Run it right after
    /// migrating (a no-op if nothing was pending) so every migration that adds a
    /// column is protected without each one needing to know about this itself.
    /// Best-effort: failure here must never block startup.
    async fn warm_up_after_migrating(&self) {
        let _ = sqlx::query("SELECT * FROM sessions LIMIT 0")
            .fetch_optional(&self.pool)
            .await;
    }

    /// Whether this database already has migration history (i.e. isn't a
    /// brand-new file) with at least one migration defined in [`MIGRATOR`]
    /// not yet applied to it. Used to decide whether an in-place migration
    /// run is about to modify pre-existing data worth backing up first — a
    /// freshly-created, never-migrated database has nothing to protect.
    async fn has_pending_migrations(&self) -> Result<bool> {
        let has_sqlx_migrations: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
        )
        .fetch_one(&self.pool)
        .await?;
        if has_sqlx_migrations == 0 {
            return Ok(false);
        }

        let applied_versions: Vec<i64> = sqlx::query_scalar("SELECT version FROM _sqlx_migrations")
            .fetch_all(&self.pool)
            .await?;
        let applied: std::collections::HashSet<i64> = applied_versions.into_iter().collect();
        Ok(MIGRATOR.iter().any(|m| !applied.contains(&m.version)))
    }

    /// Copy `state.db` to `state.db.pre-m<highest applied migration>` before
    /// [`MIGRATOR::run`] modifies it in place — migrations can be
    /// irreversible (0008 drops `secrets`, 0009 drops `push_subscriptions`),
    /// so an operator upgrading across several releases at once always has a
    /// pre-migration snapshot to fall back to.
    ///
    /// Named after the migration version rather than `CARGO_PKG_VERSION`:
    /// `release-please` only bumps the crate version at release time, so two
    /// PRs landing between releases (each adding a migration) would otherwise
    /// both back up to the exact same `state.db.pre-<version>` name and
    /// silently clobber each other's snapshot. The highest applied migration
    /// number is monotonic and unique to what's actually about to be
    /// rewritten, regardless of release cadence. Overwrites a same-named
    /// backup from a previous run at the same migration level, and prunes
    /// down to the [`MAX_PRE_MIGRATION_BACKUPS`] most recent backups afterward.
    async fn backup_before_migrating(&self) -> Result<()> {
        let db_path = format!("{}/state.db", self.data_dir);
        let highest_applied: i64 =
            sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(0);
        let backup_path = format!("{db_path}.pre-m{highest_applied}");
        std::fs::copy(&db_path, &backup_path)
            .with_context(|| format!("failed to back up {db_path} to {backup_path}"))?;
        info!(backup = %backup_path, "store: backed up database before running pending migrations");
        self.prune_old_backups()?;
        Ok(())
    }

    /// Keep only the [`MAX_PRE_MIGRATION_BACKUPS`] most recently modified
    /// `state.db.pre-*` files in the data dir, removing older ones.
    fn prune_old_backups(&self) -> Result<()> {
        let prefix = "state.db.pre-";
        let mut backups: Vec<(std::time::SystemTime, std::path::PathBuf)> =
            std::fs::read_dir(&self.data_dir)?
                .filter_map(std::result::Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().starts_with(prefix))
                .filter_map(|entry| {
                    let modified = entry.metadata().ok()?.modified().ok()?;
                    Some((modified, entry.path()))
                })
                .collect();
        backups.sort_by(|a, b| b.0.cmp(&a.0));
        for (_, path) in backups.into_iter().skip(MAX_PRE_MIGRATION_BACKUPS) {
            let _ = std::fs::remove_file(path);
        }
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

    /// Migration 0009 drops the `push_subscriptions` table: Web Push was
    /// removed — `[[webhooks]]` is now the only notification channel. Unlike
    /// the secrets table this data isn't sensitive or irreplaceable (it's just
    /// stale browser push endpoints a client would recreate by re-subscribing),
    /// but warn loudly here, before the drop runs, so an operator relying on
    /// push notifications isn't surprised when they silently stop working.
    async fn warn_before_dropping_push_subscriptions(&self) -> Result<()> {
        let has_table: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'push_subscriptions'",
        )
        .fetch_one(&self.pool)
        .await?;
        if has_table == 0 {
            return Ok(());
        }

        let subscription_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM push_subscriptions")
            .fetch_one(&self.pool)
            .await?;
        if subscription_count > 0 {
            warn!(
                subscription_count,
                "store: the push_subscriptions table is about to be dropped by migration 0009 \
                 (Web Push was removed) — {subscription_count} stored subscription(s) will be \
                 lost. Notifications now flow only through [[webhooks]]."
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

/// What happened when [`open_and_migrate`] had to recover from an unusable
/// database, for the caller to log/notify about (e.g. as a `PulpoEvent` a
/// webhook can carry).
#[derive(Debug, Clone)]
pub struct RecoveredUnusableDb {
    /// Why the original database was rejected (the error `open`/`migrate`
    /// returned), as a display string.
    pub reason: String,
    /// Where the original, now-unusable file was moved to.
    pub moved_to: String,
}

/// Open (creating if absent) and migrate the database at
/// `{data_dir}/state.db`, recovering automatically from an unusable database
/// instead of crash-looping on it — but only when the failure means the file
/// itself is corrupt or on a schema this binary can't read (see
/// [`is_quarantine_worthy`]). Anything else (a lock held by another process,
/// an I/O failure, a failed pre-migration backup copy) is *not* quarantined:
/// the file could be a perfectly healthy database, and renaming a healthy
/// database out from under a process that still holds a valid handle to it
/// would be data loss (see the single-instance lock in `store::lock`, which
/// exists precisely to keep two `pulpod`s from racing to find out).
///
/// A pre-0.1.0 (pre-migration) database, a downgrade (a newer `pulpod`
/// having applied a migration this binary doesn't know — surfaces as sqlx's
/// `VersionMissing`/`VersionMismatch`), or an outright corrupt/non-SQLite
/// file all fail here in a quarantine-worthy way: the unusable file (and any
/// `-wal`/`-shm` siblings) is renamed to `state.db.unusable-<UTC timestamp>`
/// and a fresh database is opened and migrated in its place. An error is
/// returned if that fresh attempt *also* fails, or if the original failure
/// wasn't quarantine-worthy in the first place — the caller (`build_app`)
/// treats either as "refuse to start" (exit non-zero without touching the
/// database).
pub async fn open_and_migrate(data_dir: &str) -> Result<(Store, Option<RecoveredUnusableDb>)> {
    match try_open_and_migrate(data_dir).await {
        Ok(store) => Ok((store, None)),
        Err(first_err) => {
            let reason = format!("{first_err:#}");
            if !is_quarantine_worthy(&first_err) {
                error!(
                    reason = %reason,
                    "store: database open/migrate failed for a reason that is neither \
                     corruption nor a schema incompatibility (e.g. a lock held by another \
                     process, an I/O failure, or a failed pre-migration backup) — refusing to \
                     start rather than risk quarantining a possibly-healthy database"
                );
                return Err(first_err);
            }

            let moved_to = quarantine_unusable_db(data_dir).with_context(|| {
                format!("database at {data_dir}/state.db is unusable ({reason}) and could not be quarantined")
            })?;
            error!(
                reason = %reason,
                moved_to = %moved_to,
                "store: database unusable — quarantined and starting fresh"
            );
            let store = try_open_and_migrate(data_dir).await.with_context(|| {
                format!(
                    "quarantined unusable database to {moved_to}, but creating a fresh one also failed"
                )
            })?;
            Ok((store, Some(RecoveredUnusableDb { reason, moved_to })))
        }
    }
}

async fn try_open_and_migrate(data_dir: &str) -> Result<Store> {
    let store = Store::new(data_dir).await?;
    store.migrate().await?;
    Ok(store)
}

/// Whether an error from [`try_open_and_migrate`] means `state.db` itself is
/// corrupt or on a schema this binary can't read — the only classes of
/// failure where quarantining (renaming the file out of the way and starting
/// fresh) is safe:
///
/// - The legacy-schema rejection (`Store::reject_unsupported_legacy_schema`):
///   a pre-0.1.0 database with a `sessions` table but no `_sqlx_migrations`.
/// - SQLite reporting the file isn't a database at all, or is corrupt
///   (`"file is not a database"` / `"malformed"` — e.g. `SQLITE_NOTADB`,
///   `SQLITE_CORRUPT`).
/// - `sqlx::migrate::MigrateError::VersionMissing`/`VersionMismatch`: a
///   downgrade where a newer `pulpod` applied a migration (or changed one)
///   that this binary's embedded migrator doesn't recognize.
///
/// Everything else — `"database is locked"`, a plain I/O error, a failed
/// backup copy — returns `false`: those say nothing about the file's own
/// integrity, so quarantining on them risks renaming a perfectly healthy
/// database out from under a process that still holds a valid handle to it.
fn is_quarantine_worthy(err: &anyhow::Error) -> bool {
    // sqlx's migrator returns its own error type directly (`MIGRATOR.run(..).await?`
    // in `Store::migrate`, with no added `.context()`), so it survives as the exact
    // root of the anyhow chain here — downcast rather than string-match so a
    // `Dirty`/`VersionTooOld`/`Execute(..)` (e.g. wrapping a "database is locked"
    // error) variant is correctly treated as NOT quarantine-worthy.
    if let Some(migrate_err) = err.downcast_ref::<sqlx::migrate::MigrateError>() {
        return matches!(
            migrate_err,
            sqlx::migrate::MigrateError::VersionMissing(_)
                | sqlx::migrate::MigrateError::VersionMismatch(_)
        );
    }

    let rendered = format!("{err:#}").to_lowercase();
    rendered.contains("unsupported legacy database schema")
        || rendered.contains("file is not a database")
        || rendered.contains("malformed")
}

/// Rename `{data_dir}/state.db` (and any `-wal`/`-shm` siblings) out of the
/// way so a fresh database can be opened at the canonical path. Returns the
/// new path of the primary file.
fn quarantine_unusable_db(data_dir: &str) -> Result<String> {
    let db_path = format!("{data_dir}/state.db");
    if !Path::new(&db_path).exists() {
        anyhow::bail!("no {db_path} to quarantine");
    }
    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    let quarantined = format!("{db_path}.unusable-{timestamp}");
    std::fs::rename(&db_path, &quarantined)?;
    for suffix in ["-wal", "-shm"] {
        let sidecar = format!("{db_path}{suffix}");
        if Path::new(&sidecar).exists() {
            let _ = std::fs::rename(&sidecar, format!("{quarantined}{suffix}"));
        }
    }
    Ok(quarantined)
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

/// Unit tests for the private helpers backing [`open_and_migrate`] and the
/// pending-migration backup — behavior reachable only through public API
/// (`Store::migrate`, `open_and_migrate`) is instead covered end-to-end in
/// `store/tests.rs`.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quarantine_unusable_db_bails_when_file_missing() {
        let tmpdir = tempfile::tempdir().unwrap();
        let err = quarantine_unusable_db(tmpdir.path().to_str().unwrap()).unwrap_err();
        assert!(err.to_string().contains("no"));
        assert!(err.to_string().contains("state.db"));
    }

    #[test]
    fn test_quarantine_unusable_db_renames_file_and_siblings() {
        let tmpdir = tempfile::tempdir().unwrap();
        let dir = tmpdir.path().to_str().unwrap();
        std::fs::write(tmpdir.path().join("state.db"), b"garbage").unwrap();
        std::fs::write(tmpdir.path().join("state.db-wal"), b"wal").unwrap();
        std::fs::write(tmpdir.path().join("state.db-shm"), b"shm").unwrap();

        let quarantined = quarantine_unusable_db(dir).unwrap();

        assert!(!tmpdir.path().join("state.db").exists());
        assert!(!tmpdir.path().join("state.db-wal").exists());
        assert!(!tmpdir.path().join("state.db-shm").exists());
        assert!(Path::new(&quarantined).exists());
        assert!(quarantined.contains("state.db.unusable-"));
        assert!(Path::new(&format!("{quarantined}-wal")).exists());
        assert!(Path::new(&format!("{quarantined}-shm")).exists());
        assert_eq!(std::fs::read(&quarantined).unwrap(), b"garbage");
    }

    #[test]
    fn test_quarantine_unusable_db_ignores_missing_siblings() {
        let tmpdir = tempfile::tempdir().unwrap();
        let dir = tmpdir.path().to_str().unwrap();
        std::fs::write(tmpdir.path().join("state.db"), b"garbage").unwrap();

        let quarantined = quarantine_unusable_db(dir).unwrap();

        assert!(Path::new(&quarantined).exists());
        assert!(!Path::new(&format!("{quarantined}-wal")).exists());
        assert!(!Path::new(&format!("{quarantined}-shm")).exists());
    }

    #[tokio::test]
    async fn test_prune_old_backups_keeps_only_most_recent() {
        let store = test_store().await;
        for i in 0..5 {
            std::fs::write(
                format!("{}/state.db.pre-m{i}", store.data_dir),
                format!("backup-{i}"),
            )
            .unwrap();
            // Ensure distinct mtimes across filesystems with coarse resolution.
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        store.prune_old_backups().unwrap();

        let remaining: Vec<String> = std::fs::read_dir(&store.data_dir)
            .unwrap()
            .filter_map(std::result::Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("state.db.pre-"))
            .collect();
        assert_eq!(remaining.len(), MAX_PRE_MIGRATION_BACKUPS);
        // The three most recently written backups (m2, m3, m4) survive.
        for kept in ["state.db.pre-m2", "state.db.pre-m3", "state.db.pre-m4"] {
            assert!(remaining.contains(&kept.to_owned()), "{remaining:?}");
        }
    }

    #[tokio::test]
    async fn test_prune_old_backups_noop_under_the_cap() {
        let store = test_store().await;
        std::fs::write(format!("{}/state.db.pre-m1", store.data_dir), b"a").unwrap();

        store.prune_old_backups().unwrap();

        assert!(Path::new(&format!("{}/state.db.pre-m1", store.data_dir)).exists());
    }

    #[tokio::test]
    async fn test_backup_before_migrating_names_backup_after_highest_applied_migration() {
        // Regression test: the backup filename used to be
        // `state.db.pre-<CARGO_PKG_VERSION>`, which collides across every PR that
        // lands between two `release-please` releases (the crate version only
        // bumps at release time). Naming it after the highest already-applied
        // migration version is monotonic and unique to what's about to change,
        // regardless of release cadence. `test_store()` runs every migration in
        // `MIGRATOR` (currently through 0010) — bump the expected suffix here
        // when a new migration file is added.
        let store = test_store().await;

        store.backup_before_migrating().await.unwrap();

        let backup_path = format!("{}/state.db.pre-m10", store.data_dir);
        assert!(
            Path::new(&backup_path).exists(),
            "expected a backup named after migration 0010, the highest applied \
             version after test_store()'s full migration run"
        );
    }

    #[tokio::test]
    async fn test_open_and_migrate_success_reports_no_recovery() {
        let tmpdir = tempfile::tempdir().unwrap();
        let (store, recovered) = open_and_migrate(tmpdir.path().to_str().unwrap())
            .await
            .unwrap();
        assert!(recovered.is_none());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_open_and_migrate_recovers_from_garbage_file() {
        let tmpdir = tempfile::tempdir().unwrap();
        let dir = tmpdir.path().to_str().unwrap();
        std::fs::write(tmpdir.path().join("state.db"), b"not a sqlite database").unwrap();

        let (store, recovered) = open_and_migrate(dir).await.unwrap();

        let recovered = recovered.expect("expected recovery from a garbage file");
        assert!(Path::new(&recovered.moved_to).exists());
        assert_eq!(
            std::fs::read(&recovered.moved_to).unwrap(),
            b"not a sqlite database"
        );
        // The fresh database at the canonical path is usable.
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_open_and_migrate_refuses_when_directory_unwritable() {
        use std::os::unix::fs::PermissionsExt;

        let tmpdir = tempfile::tempdir().unwrap();
        let dir = tmpdir.path().to_str().unwrap();
        let mut perms = std::fs::metadata(tmpdir.path()).unwrap().permissions();
        perms.set_mode(0o500); // read + execute only, no write
        std::fs::set_permissions(tmpdir.path(), perms.clone()).unwrap();

        let result = open_and_migrate(dir).await;

        // Restore permissions unconditionally so the tempdir can be cleaned up.
        perms.set_mode(0o700);
        std::fs::set_permissions(tmpdir.path(), perms).unwrap();

        assert!(result.is_err(), "expected open_and_migrate to refuse");
    }

    // -- is_quarantine_worthy: error classification (#126 follow-up) ------------

    #[test]
    fn test_is_quarantine_worthy_legacy_schema_rejection() {
        let err = anyhow::anyhow!(
            "unsupported legacy database schema detected; delete /data/state.db to reinitialize"
        );
        assert!(is_quarantine_worthy(&err));
    }

    #[test]
    fn test_is_quarantine_worthy_sqlite_not_a_database() {
        let err =
            anyhow::anyhow!("error returned from database: (code: 26) file is not a database");
        assert!(is_quarantine_worthy(&err));
    }

    #[test]
    fn test_is_quarantine_worthy_sqlite_malformed() {
        let err = anyhow::anyhow!(
            "error returned from database: (code: 11) database disk image is malformed"
        );
        assert!(is_quarantine_worthy(&err));
    }

    #[test]
    fn test_is_quarantine_worthy_migrate_version_missing() {
        let err: anyhow::Error = sqlx::migrate::MigrateError::VersionMissing(9999).into();
        assert!(is_quarantine_worthy(&err));
    }

    #[test]
    fn test_is_quarantine_worthy_migrate_version_mismatch() {
        let err: anyhow::Error = sqlx::migrate::MigrateError::VersionMismatch(3).into();
        assert!(is_quarantine_worthy(&err));
    }

    #[test]
    fn test_is_quarantine_worthy_false_for_locked_database() {
        let err = anyhow::anyhow!("error returned from database: (code: 5) database is locked");
        assert!(!is_quarantine_worthy(&err));
    }

    #[test]
    fn test_is_quarantine_worthy_false_for_plain_io_error() {
        let err = anyhow::Error::new(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Permission denied (os error 13)",
        ));
        assert!(!is_quarantine_worthy(&err));
    }

    #[test]
    fn test_is_quarantine_worthy_false_for_failed_backup() {
        let err = anyhow::anyhow!("failed to back up /data/state.db to /data/state.db.pre-0.3.1");
        assert!(!is_quarantine_worthy(&err));
    }

    #[test]
    fn test_is_quarantine_worthy_false_for_other_migrate_error_variants() {
        // A `MigrateError` variant other than `VersionMissing`/`VersionMismatch` —
        // e.g. a partially-applied migration, or `Execute` wrapping a transient
        // "database is locked" — must not be treated as corruption.
        let dirty: anyhow::Error = sqlx::migrate::MigrateError::Dirty(1).into();
        assert!(!is_quarantine_worthy(&dirty));

        let too_old: anyhow::Error = sqlx::migrate::MigrateError::VersionTooOld(1, 2).into();
        assert!(!is_quarantine_worthy(&too_old));
    }
}
