use super::*;
use chrono::Utc;
use pulpo_common::api::ListSessionsQuery;
use pulpo_common::session::InterventionCode;
use pulpo_common::session::{Session, SessionStatus};
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection};
use sqlx::{ConnectOptions, Connection};
use uuid::Uuid;

/// Open a dedicated, throwaway connection to `store`'s own database file,
/// never part of `store.pool()`.
///
/// Mirrors `Store::migrate()`'s own connection isolation (see its doc comment
/// for the full story): a `sqlx-sqlite` 0.8.6 defect mis-sizes a query's
/// cached column-count metadata once a table gains a column elsewhere in the
/// database's lifetime, regardless of which connection ran the `ALTER TABLE`
/// — so a fixture that runs a *partial* migrator or inserts legacy-shaped rows
/// directly, ahead of the real `store.migrate()` call under test, must do it
/// on a connection that's never reused afterward, exactly like production
/// never queries the app pool before migration completes. Using `store.pool()`
/// for this setup SQL instead reproduced the intermittent
/// `index out of bounds` panic this avoids (`SqliteRow::current`).
async fn dedicated_test_conn(store: &Store) -> SqliteConnection {
    SqliteConnectOptions::new()
        .filename(format!("{}/state.db", store.data_dir))
        .statement_cache_capacity(0)
        .connect()
        .await
        .unwrap()
}

fn make_session(name: &str) -> Session {
    Session {
        id: Uuid::new_v4(),
        name: name.into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("Fix the bug".into()),
        status: SessionStatus::Working,
        backend_session_id: Some(name.to_owned()),
        ..Default::default()
    }
}

#[tokio::test]
async fn test_new_creates_directory() {
    let tmpdir = tempfile::tempdir().unwrap();
    let data_dir = tmpdir.path().join("nested/deep");
    let store = Store::new(data_dir.to_str().unwrap()).await.unwrap();
    assert!(data_dir.exists());
    drop(store);
}

#[tokio::test]
async fn test_migrate_creates_sessions_table() {
    let tmpdir = tempfile::tempdir().unwrap();
    let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
    store.migrate().await.unwrap();

    // Verify table exists by running a query
    let result = sqlx::query("SELECT count(*) as cnt FROM sessions")
        .fetch_one(store.pool())
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_migrate_uses_sqlx_migrations_table() {
    let tmpdir = tempfile::tempdir().unwrap();
    let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
    store.migrate().await.unwrap();

    let versions: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version")
            .fetch_all(store.pool())
            .await
            .unwrap();
    assert_eq!(versions, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);

    // Web Push and the durable outbox were removed: both tables must be gone
    // after migrating.
    for table in ["push_subscriptions", "webhook_outbox"] {
        let has_table: i32 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?")
                .bind(table)
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(has_table, 0, "{table} should be dropped");
    }

    let has_sandbox: i32 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'sandbox'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(has_sandbox, 0);

    // The secrets store was removed: the `secrets` table and the
    // `schedules.secrets` column must both be gone after migrating.
    let has_secrets_table: i32 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='secrets'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(has_secrets_table, 0);

    let has_secrets_column: i32 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('schedules') WHERE name = 'secrets'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(has_secrets_column, 0);
}

/// Build a database at the pre-0008 schema (migrations 1-7 applied, `secrets`
/// intact) by running a runtime `Migrator` over a copy of the migrations
/// directory with `0008_drop_secrets.sql` excluded. Same migration file
/// content as the embedded `MIGRATOR`, so checksums line up when `migrate()`
/// (which uses the real, full `MIGRATOR`) is run afterwards.
async fn store_at_migration_0007() -> Store {
    let tmpdir = tempfile::tempdir().unwrap();
    let tmpdir = Box::leak(Box::new(tmpdir));
    store_at_migration_0007_in(tmpdir.path().to_str().unwrap()).await
}

/// Like [`store_at_migration_0007`], but against a caller-owned `dir` (the
/// canonical `state.db` path) instead of a freshly leaked tempdir — for tests
/// that need to inspect or manipulate the directory afterward (e.g. forcing
/// the pre-migration backup copy to fail).
async fn store_at_migration_0007_in(dir: &str) -> Store {
    let store = Store::new(dir).await.unwrap();

    let migrations_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let partial_dir = tempfile::tempdir().unwrap();
    for entry in std::fs::read_dir(&migrations_src).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("0008") {
            continue;
        }
        std::fs::copy(entry.path(), partial_dir.path().join(name)).unwrap();
    }
    let partial = sqlx::migrate::Migrator::new(partial_dir.path())
        .await
        .unwrap();
    let mut conn = dedicated_test_conn(&store).await;
    partial.run_direct(&mut conn).await.unwrap();
    conn.close().await.unwrap();

    store
}

#[tokio::test]
async fn test_migrate_warns_before_dropping_secrets() {
    let store = store_at_migration_0007().await;

    sqlx::query("INSERT INTO secrets (name, value, created_at) VALUES (?, ?, ?)")
        .bind("token-a")
        .bind("value-a")
        .bind("2026-01-01T00:00:00Z")
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO secrets (name, value, created_at) VALUES (?, ?, ?)")
        .bind("token-b")
        .bind("value-b")
        .bind("2026-01-01T00:00:00Z")
        .execute(store.pool())
        .await
        .unwrap();

    let count_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM secrets")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(count_before, 2);

    // Migrating drops the secrets table (migration 0008) but must not fail or
    // block startup — the warning is best-effort operator notice, not a gate.
    store.migrate().await.unwrap();

    let has_secrets_table: i32 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='secrets'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(has_secrets_table, 0);
}

#[tokio::test]
async fn test_migrate_warns_before_dropping_secrets_noop_when_empty() {
    // No rows in `secrets` (or no table at all, on a fresh DB) — migrate() must
    // still succeed with no warning path exercised beyond the early return.
    let store = store_at_migration_0007().await;

    let count_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM secrets")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(count_before, 0);

    store.migrate().await.unwrap();
}

/// Build a database at the pre-0009 schema (migrations 1-8 applied,
/// `push_subscriptions`/`webhook_outbox` intact) the same way
/// [`store_at_migration_0007`] builds a pre-0008 one.
async fn store_at_migration_0008() -> Store {
    let tmpdir = tempfile::tempdir().unwrap();
    let tmpdir = Box::leak(Box::new(tmpdir));
    let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();

    let migrations_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let partial_dir = tempfile::tempdir().unwrap();
    for entry in std::fs::read_dir(&migrations_src).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("0009") {
            continue;
        }
        std::fs::copy(entry.path(), partial_dir.path().join(name)).unwrap();
    }
    let partial = sqlx::migrate::Migrator::new(partial_dir.path())
        .await
        .unwrap();
    let mut conn = dedicated_test_conn(&store).await;
    partial.run_direct(&mut conn).await.unwrap();
    conn.close().await.unwrap();

    store
}

#[tokio::test]
async fn test_migrate_warns_before_dropping_push_subscriptions() {
    let store = store_at_migration_0008().await;

    sqlx::query(
        "INSERT INTO push_subscriptions (endpoint, p256dh, auth, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind("https://push.example.com/1")
    .bind("p256dh-key")
    .bind("auth-key")
    .bind("2026-01-01T00:00:00Z")
    .execute(store.pool())
    .await
    .unwrap();

    let count_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM push_subscriptions")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(count_before, 1);

    // Migrating drops push_subscriptions (migration 0009) but must not fail or
    // block startup — the warning is best-effort operator notice, not a gate.
    store.migrate().await.unwrap();

    let has_table: i32 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='push_subscriptions'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(has_table, 0);
}

#[tokio::test]
async fn test_migrate_warns_before_dropping_push_subscriptions_noop_when_empty() {
    // No rows in push_subscriptions — migrate() must still succeed with no
    // warning path exercised beyond the early return.
    let store = store_at_migration_0008().await;

    let count_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM push_subscriptions")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(count_before, 0);

    store.migrate().await.unwrap();
}

/// Build a database at the pre-0010 schema (migrations 1-9 applied, six-state
/// `status` values and no `status_reason` column) the same way
/// [`store_at_migration_0008`] builds a pre-0009 one.
async fn store_at_migration_0009() -> Store {
    let tmpdir = tempfile::tempdir().unwrap();
    let tmpdir = Box::leak(Box::new(tmpdir));
    let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();

    let migrations_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let partial_dir = tempfile::tempdir().unwrap();
    for entry in std::fs::read_dir(&migrations_src).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("0010") {
            continue;
        }
        std::fs::copy(entry.path(), partial_dir.path().join(name)).unwrap();
    }
    let partial = sqlx::migrate::Migrator::new(partial_dir.path())
        .await
        .unwrap();
    let mut conn = dedicated_test_conn(&store).await;
    partial.run_direct(&mut conn).await.unwrap();
    conn.close().await.unwrap();

    store
}

/// Insert a session row using the pre-0010 (six-state) schema directly — bypasses
/// `Store::insert_session`, which assumes the post-migration `status_reason` column
/// already exists.
#[allow(clippy::too_many_arguments)]
/// Insert a session row using the pre-0010 (six-state) schema, keyed by `name`
/// (a real, random UUID is generated for `id` — `row_to_session` requires one).
async fn insert_legacy_session(
    store: &Store,
    name: &str,
    status: &str,
    metadata: Option<&str>,
    intervention_code: Option<&str>,
) {
    let mut conn = dedicated_test_conn(store).await;
    sqlx::query(
        "INSERT INTO sessions (id, name, workdir, provider, prompt, status, mode, metadata, intervention_code, created_at, updated_at) \
         VALUES (?, ?, '/tmp/repo', '', '', ?, '', ?, ?, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(name)
    .bind(status)
    .bind(metadata)
    .bind(intervention_code)
    .execute(&mut conn)
    .await
    .unwrap();
    conn.close().await.unwrap();
}

/// End-to-end proof of migration `0010_five_state_status.sql`'s mapping — every old
/// status/metadata/intervention-code combination the migration's `CASE`
/// expressions branch on, run through the real embedded migrator (not a
/// reimplementation of the SQL in Rust).
#[tokio::test]
async fn test_migration_0010_rewrites_status_and_status_reason() {
    let store = store_at_migration_0009().await;

    insert_legacy_session(&store, "creating-sess", "creating", None, None).await;
    insert_legacy_session(&store, "active-sess", "active", None, None).await;
    insert_legacy_session(&store, "idle-plain-sess", "idle", None, None).await;
    insert_legacy_session(
        &store,
        "idle-needs-input-sess",
        "idle",
        Some(r#"{"needs_input":"permission","other_key":"kept"}"#),
        None,
    )
    .await;
    insert_legacy_session(&store, "ready-sess", "ready", None, None).await;
    insert_legacy_session(&store, "stopped-plain-sess", "stopped", None, None).await;
    insert_legacy_session(
        &store,
        "stopped-budget-sess",
        "stopped",
        None,
        Some("budget_exceeded"),
    )
    .await;
    insert_legacy_session(
        &store,
        "stopped-idle-timeout-sess",
        "stopped",
        None,
        Some("idle_timeout"),
    )
    .await;
    insert_legacy_session(
        &store,
        "stopped-user-sess",
        "stopped",
        None,
        Some("user_stop"),
    )
    .await;
    insert_legacy_session(&store, "killed-sess", "killed", None, Some("user_kill")).await;
    insert_legacy_session(&store, "lost-sess", "lost", None, None).await;

    store.migrate().await.unwrap();

    let get = |name: &'static str| {
        let store = &store;
        async move { store.get_session(name).await.unwrap().unwrap() }
    };

    let creating = get("creating-sess").await;
    assert_eq!(creating.status, SessionStatus::Starting);
    assert_eq!(creating.status_reason, None);

    let active = get("active-sess").await;
    assert_eq!(active.status, SessionStatus::Working);
    assert_eq!(active.status_reason, None);

    let idle_plain = get("idle-plain-sess").await;
    assert_eq!(idle_plain.status, SessionStatus::Waiting);
    assert_eq!(idle_plain.status_reason.as_deref(), Some("idle"));

    let idle_needs_input = get("idle-needs-input-sess").await;
    assert_eq!(idle_needs_input.status, SessionStatus::Waiting);
    assert_eq!(
        idle_needs_input.status_reason.as_deref(),
        Some("needs_input:permission")
    );
    // The `needs_input` key is stripped from metadata; unrelated keys survive.
    assert!(idle_needs_input.meta_str("needs_input").is_none());
    assert_eq!(idle_needs_input.meta_str("other_key"), Some("kept"));

    let ready = get("ready-sess").await;
    assert_eq!(ready.status, SessionStatus::Done);
    assert_eq!(ready.status_reason.as_deref(), Some("exited"));

    let stopped_plain = get("stopped-plain-sess").await;
    assert_eq!(stopped_plain.status, SessionStatus::Done);
    assert_eq!(stopped_plain.status_reason.as_deref(), Some("stopped"));

    let stopped_budget = get("stopped-budget-sess").await;
    assert_eq!(stopped_budget.status, SessionStatus::Done);
    assert_eq!(
        stopped_budget.status_reason.as_deref(),
        Some("budget_exceeded")
    );

    let stopped_idle_timeout = get("stopped-idle-timeout-sess").await;
    assert_eq!(stopped_idle_timeout.status, SessionStatus::Done);
    assert_eq!(
        stopped_idle_timeout.status_reason.as_deref(),
        Some("idle_timeout")
    );

    // `user_stop` isn't one of the reasons this model recognizes as a distinct
    // `done` reason — it collapses to the generic `stopped`.
    let stopped_user = get("stopped-user-sess").await;
    assert_eq!(stopped_user.status, SessionStatus::Done);
    assert_eq!(stopped_user.status_reason.as_deref(), Some("stopped"));

    // The legacy `killed` alias for `stopped` maps the same way.
    let killed = get("killed-sess").await;
    assert_eq!(killed.status, SessionStatus::Done);
    assert_eq!(killed.status_reason.as_deref(), Some("stopped"));

    let lost = get("lost-sess").await;
    assert_eq!(lost.status, SessionStatus::Lost);
    assert_eq!(lost.status_reason, None);
}

/// The "one live session per name" unique index must only cover
/// `starting`/`working`/`waiting` post-migration — a `done` row can share a name with
/// a fresh session (see `test_has_active_session_by_name_done_does_not_block_reuse`).
#[tokio::test]
async fn test_migration_0010_rebuilds_live_name_index_for_new_statuses() {
    let store = store_at_migration_0009().await;
    insert_legacy_session(&store, "shared-name", "ready", None, None).await;
    store.migrate().await.unwrap();

    // A `done` row (migrated from `ready`) must not block a fresh same-named spawn.
    assert!(
        !store
            .has_active_session_by_name("shared-name")
            .await
            .unwrap()
    );

    let mut fresh = make_session("shared-name");
    fresh.id = uuid::Uuid::new_v4();
    store.insert_session(&fresh).await.unwrap();
    assert!(
        store
            .has_active_session_by_name("shared-name")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn test_migrate_rejects_unsupported_legacy_schema() {
    let tmpdir = tempfile::tempdir().unwrap();
    let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();

    sqlx::query("CREATE TABLE sessions (id TEXT PRIMARY KEY)")
        .execute(store.pool())
        .await
        .unwrap();

    let err = store.migrate().await.unwrap_err().to_string();
    assert!(err.contains("unsupported legacy database schema detected"));
    assert!(err.contains("state.db"));
}

#[tokio::test]
async fn test_migrate_is_idempotent() {
    let tmpdir = tempfile::tempdir().unwrap();
    let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
    store.migrate().await.unwrap();
    // Running migrate again should not error
    store.migrate().await.unwrap();

    // Neither call had a pending migration to protect (the first ran against
    // a brand-new file, the second against an already-fully-migrated one), so
    // no `state.db.pre-*` backup should exist either time.
    let backups: Vec<_> = std::fs::read_dir(tmpdir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("state.db.pre-"))
        .collect();
    assert!(backups.is_empty(), "unexpected backups: {backups:?}");
}

#[tokio::test]
async fn test_migrate_backs_up_before_running_pending_migrations() {
    // Named after the highest migration already applied rather than
    // `CARGO_PKG_VERSION` — see `Store::backup_before_migrating`'s doc comment for
    // why. `store_at_migration_0009` (unlike `store_at_migration_0007`, which
    // skips only the single `0008` file and so still applies every migration
    // *after* it, landing on whatever the newest migration happens to be) excludes
    // `0010` and everything would-be-after it, so it reliably leaves the highest
    // applied version at exactly 9.
    let store = store_at_migration_0009().await;
    let backup_path = format!("{}/state.db.pre-m9", store.data_dir);
    assert!(!std::path::Path::new(&backup_path).exists());

    store.migrate().await.unwrap();

    assert!(
        std::path::Path::new(&backup_path).exists(),
        "expected a pre-migration backup at {backup_path}"
    );
}

#[tokio::test]
async fn test_migrate_backup_overwrites_stale_same_version_file() {
    let store = store_at_migration_0009().await;
    let backup_path = format!("{}/state.db.pre-m9", store.data_dir);
    std::fs::write(&backup_path, b"stale placeholder").unwrap();

    store.migrate().await.unwrap();

    let contents = std::fs::read(&backup_path).unwrap();
    assert_ne!(contents, b"stale placeholder");
}

#[tokio::test]
async fn test_open_and_migrate_recovers_from_unsupported_legacy_schema() {
    let tmpdir = tempfile::tempdir().unwrap();
    let dir = tmpdir.path().to_str().unwrap();
    {
        let store = Store::new(dir).await.unwrap();
        sqlx::query("CREATE TABLE sessions (id TEXT PRIMARY KEY)")
            .execute(store.pool())
            .await
            .unwrap();
    }

    let (store, recovered) = crate::store::open_and_migrate(dir).await.unwrap();

    let recovered = recovered.expect("expected recovery from a legacy schema");
    assert!(
        recovered
            .reason
            .contains("unsupported legacy database schema")
    );
    assert!(std::path::Path::new(&recovered.moved_to).exists());
    // A fully-migrated, usable database now lives at the canonical path.
    let versions: Vec<i64> = sqlx::query_scalar("SELECT version FROM _sqlx_migrations")
        .fetch_all(store.pool())
        .await
        .unwrap();
    assert!(!versions.is_empty());
}

#[tokio::test]
async fn test_open_and_migrate_recovers_from_downgrade_version_missing() {
    let tmpdir = tempfile::tempdir().unwrap();
    let dir = tmpdir.path().to_str().unwrap();
    {
        // Simulate a downgrade: a newer `pulpod` applied a migration
        // (version 9999) this binary's embedded `MIGRATOR` has never heard
        // of. sqlx's own migrations-table schema, minimally reproduced.
        let store = Store::new(dir).await.unwrap();
        sqlx::query(
            "CREATE TABLE _sqlx_migrations (
                version BIGINT PRIMARY KEY,
                description TEXT NOT NULL,
                installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                success BOOLEAN NOT NULL,
                checksum BLOB NOT NULL,
                execution_time BIGINT NOT NULL
            )",
        )
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO _sqlx_migrations
                (version, description, success, checksum, execution_time)
             VALUES (9999, 'from the future', 1, x'00', 0)",
        )
        .execute(store.pool())
        .await
        .unwrap();
    }

    let (store, recovered) = crate::store::open_and_migrate(dir).await.unwrap();

    let recovered = recovered.expect("expected recovery from a downgrade");
    assert!(recovered.reason.contains("9999"));
    assert!(std::path::Path::new(&recovered.moved_to).exists());
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

/// #126 follow-up: a failure unrelated to the database's own integrity — here, the
/// pre-migration backup copy itself failing — must NOT quarantine `state.db`.
/// Forces the failure deterministically by putting a directory at the exact path
/// `backup_before_migrating` would copy `state.db` to, rather than fiddling with
/// filesystem permissions (which the earlier connect-time-only unwritable-dir test
/// already covers, and which no longer even reaches the backup step under the new
/// classification — see `is_quarantine_worthy`).
#[tokio::test]
async fn test_open_and_migrate_refuses_without_quarantine_when_backup_fails() {
    let tmpdir = tempfile::tempdir().unwrap();
    let dir = tmpdir.path().to_str().unwrap();
    // A database with pending migrations (8, 9, 10) so `migrate()` attempts a
    // pre-migration backup at all.
    store_at_migration_0007_in(dir).await;

    // Backups are named after the highest *applied* migration
    // (`backup_before_migrating`), so for a 0007-shaped fixture the copy targets
    // `state.db.pre-m7`. Occupy that exact path with a directory so the copy
    // fails deterministically on every platform.
    let backup_path = format!("{dir}/state.db.pre-m7");
    std::fs::create_dir(&backup_path).unwrap();

    // `Store` isn't `Debug`, so `.unwrap_err()` (which needs `T: Debug` to format the
    // Ok case in its panic message) doesn't apply here — match instead.
    let err = match crate::store::open_and_migrate(dir).await {
        Ok(_) => panic!("expected open_and_migrate to refuse when the backup copy fails"),
        Err(e) => e,
    };
    assert!(format!("{err:#}").contains("failed to back up"));

    // The original database must still be at the canonical path, untouched — not
    // quarantined — since a failed backup copy says nothing about its integrity.
    assert!(std::path::Path::new(&format!("{dir}/state.db")).exists());
    let unusable: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains("unusable"))
        .collect();
    assert!(
        unusable.is_empty(),
        "must not quarantine on a backup failure: {unusable:?}"
    );
}

#[tokio::test]
async fn test_pool_returns_valid_pool() {
    let tmpdir = tempfile::tempdir().unwrap();
    let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
    let pool = store.pool();
    // Verify pool works
    let row = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(row, 1);
}

#[tokio::test]
async fn test_insert_and_get_session() {
    let store = test_store().await;
    let session = make_session("test-roundtrip");

    store.insert_session(&session).await.unwrap();
    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(fetched.id, session.id);
    assert_eq!(fetched.name, "test-roundtrip");
    assert_eq!(fetched.workdir, "/tmp/repo");

    assert_eq!(fetched.status, SessionStatus::Working);

    assert_eq!(fetched.exit_code, None);
    assert_eq!(fetched.backend_session_id, Some("test-roundtrip".into()));
}

#[tokio::test]
async fn test_get_session_not_found() {
    let store = test_store().await;
    let result = store.get_session("nonexistent").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_session_by_name() {
    let store = test_store().await;
    let session = make_session("lookup-by-name");
    store.insert_session(&session).await.unwrap();

    let fetched = store.get_session("lookup-by-name").await.unwrap().unwrap();
    assert_eq!(fetched.id, session.id);
    assert_eq!(fetched.name, "lookup-by-name");
}

#[tokio::test]
async fn test_get_session_by_name_not_found() {
    let store = test_store().await;
    let session = make_session("existing");
    store.insert_session(&session).await.unwrap();

    let result = store.get_session("nonexistent-name").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_session_prefers_live_over_terminal() {
    let store = test_store().await;

    // Insert a `done` session with name "dup" — the unique live-name index
    // (`starting`/`working`/`waiting` only) doesn't cover it, so a second row with
    // the same name can coexist.
    let mut done = make_session("dup");
    done.id = uuid::Uuid::new_v4();
    done.status = SessionStatus::Done;
    store.insert_session(&done).await.unwrap();

    // Insert a still-`working` session with the same name "dup".
    let mut working = make_session("dup-working");
    working.id = uuid::Uuid::new_v4();
    working.name = "dup".into();
    working.status = SessionStatus::Working;
    store.insert_session(&working).await.unwrap();

    // get_session by name should return the live one, not the terminal one.
    let fetched = store.get_session("dup").await.unwrap().unwrap();
    assert_eq!(fetched.status, SessionStatus::Working);
    assert_eq!(fetched.id, working.id);
}

#[tokio::test]
async fn test_get_session_prefers_lost_over_done() {
    // Among two terminal statuses sharing a name, `lost` (something to look into)
    // outranks `done` (an expected/clean finish) — see `get_session`'s `ORDER BY`.
    let store = test_store().await;

    let mut done = make_session("dup2");
    done.id = uuid::Uuid::new_v4();
    done.status = SessionStatus::Done;
    store.insert_session(&done).await.unwrap();

    let mut lost = make_session("dup2-lost");
    lost.id = uuid::Uuid::new_v4();
    lost.name = "dup2".into();
    lost.status = SessionStatus::Lost;
    store.insert_session(&lost).await.unwrap();

    let fetched = store.get_session("dup2").await.unwrap().unwrap();
    assert_eq!(fetched.status, SessionStatus::Lost);
    assert_eq!(fetched.id, lost.id);
}

#[tokio::test]
async fn test_has_active_session_by_name_true() {
    let store = test_store().await;
    let session = make_session("my-session");
    store.insert_session(&session).await.unwrap();

    assert!(
        store
            .has_active_session_by_name("my-session")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn test_has_active_session_by_name_false_no_match() {
    let store = test_store().await;
    assert!(
        !store
            .has_active_session_by_name("nonexistent")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn test_has_active_session_by_name_false_stopped() {
    let store = test_store().await;
    let mut session = make_session("stopped-session");
    session.status = SessionStatus::Done;
    store.insert_session(&session).await.unwrap();

    assert!(
        !store
            .has_active_session_by_name("stopped-session")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn test_has_active_session_by_name_stale() {
    let store = test_store().await;
    let mut session = make_session("idle-session");
    session.status = SessionStatus::Waiting;
    store.insert_session(&session).await.unwrap();

    assert!(
        store
            .has_active_session_by_name("idle-session")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn test_has_active_session_by_name_creating() {
    let store = test_store().await;
    let mut session = make_session("creating-session");
    session.status = SessionStatus::Starting;
    store.insert_session(&session).await.unwrap();

    assert!(
        store
            .has_active_session_by_name("creating-session")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn test_has_active_session_by_name_done_does_not_block_reuse() {
    // Unlike the old `ready` status (a real, alive tmux backend still bound to the
    // session's name), `done` has no backend left at all — ADR 0009 removed the
    // fallback shell that used to keep it alive. A `done` session's name is free to
    // reuse for a brand-new spawn, same as `lost` already was pre-ADR-0009.
    let store = test_store().await;
    let mut session = make_session("done-session");
    session.status = SessionStatus::Done;
    store.insert_session(&session).await.unwrap();

    assert!(
        !store
            .has_active_session_by_name("done-session")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn test_has_active_session_by_name_excluding_self() {
    let store = test_store().await;
    let mut session = make_session("ready-session");
    session.status = SessionStatus::Done;
    store.insert_session(&session).await.unwrap();

    // Excluding self should return false (no *other* active session with this name)
    assert!(
        !store
            .has_active_session_by_name_excluding("ready-session", Some(&session.id.to_string()),)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn test_has_active_session_by_name_excluding_different_id() {
    let store = test_store().await;
    let mut session = make_session("clash-session");
    session.status = SessionStatus::Working;
    store.insert_session(&session).await.unwrap();

    // Excluding a different ID should still find the active session
    assert!(
        store
            .has_active_session_by_name_excluding(
                "clash-session",
                Some(&uuid::Uuid::new_v4().to_string()),
            )
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn test_find_live_sessions_by_worktree_finds_other_live_session() {
    let store = test_store().await;
    let mut source = make_session("plan-auth");
    source.worktree_path = Some("/tmp/wt/plan-auth".into());
    store.insert_session(&source).await.unwrap();

    let mut handoff = make_session("plan-auth-2");
    handoff.worktree_path = Some("/tmp/wt/plan-auth".into());
    handoff.status = SessionStatus::Working;
    store.insert_session(&handoff).await.unwrap();

    let others = store
        .find_live_sessions_by_worktree("/tmp/wt/plan-auth", &source.id.to_string())
        .await
        .unwrap();
    assert_eq!(others.len(), 1);
    assert_eq!(others[0].name, "plan-auth-2");
}

#[tokio::test]
async fn test_find_live_sessions_by_worktree_excludes_dead_sessions() {
    let store = test_store().await;
    let mut source = make_session("plan-auth");
    source.worktree_path = Some("/tmp/wt/plan-auth".into());
    store.insert_session(&source).await.unwrap();

    let mut dead = make_session("plan-auth-2");
    dead.worktree_path = Some("/tmp/wt/plan-auth".into());
    dead.status = SessionStatus::Done;
    store.insert_session(&dead).await.unwrap();

    let others = store
        .find_live_sessions_by_worktree("/tmp/wt/plan-auth", &source.id.to_string())
        .await
        .unwrap();
    assert!(
        others.is_empty(),
        "a stopped session must not count as in-use"
    );
}

#[tokio::test]
async fn test_find_live_sessions_by_worktree_excludes_self() {
    let store = test_store().await;
    let mut source = make_session("solo-task");
    source.worktree_path = Some("/tmp/wt/solo-task".into());
    store.insert_session(&source).await.unwrap();

    let others = store
        .find_live_sessions_by_worktree("/tmp/wt/solo-task", &source.id.to_string())
        .await
        .unwrap();
    assert!(others.is_empty());
}

#[tokio::test]
async fn test_find_live_sessions_by_worktree_no_match() {
    let store = test_store().await;
    let others = store
        .find_live_sessions_by_worktree("/tmp/wt/nonexistent", "some-id")
        .await
        .unwrap();
    assert!(others.is_empty());
}

#[tokio::test]
async fn test_worktree_in_use_elsewhere_true_when_another_live_session_shares_it() {
    let store = test_store().await;
    let mut source = make_session("plan-auth");
    source.worktree_path = Some("/tmp/wt/plan-auth".into());
    store.insert_session(&source).await.unwrap();

    let mut handoff = make_session("plan-auth-2");
    handoff.worktree_path = Some("/tmp/wt/plan-auth".into());
    handoff.status = SessionStatus::Working;
    store.insert_session(&handoff).await.unwrap();

    assert!(
        store
            .worktree_in_use_elsewhere("/tmp/wt/plan-auth", &source.id.to_string())
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn test_worktree_in_use_elsewhere_false_when_no_other_session_shares_it() {
    let store = test_store().await;
    let mut source = make_session("solo-task");
    source.worktree_path = Some("/tmp/wt/solo-task".into());
    store.insert_session(&source).await.unwrap();

    assert!(
        !store
            .worktree_in_use_elsewhere("/tmp/wt/solo-task", &source.id.to_string())
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn test_unique_index_prevents_duplicate_live_names() {
    let store = test_store().await;
    let s1 = make_session("dup-name");
    store.insert_session(&s1).await.unwrap();
    // Second insert with same name and live status should fail at DB level
    let mut s2 = make_session("dup-name");
    s2.id = uuid::Uuid::new_v4();
    let result = store.insert_session(&s2).await;
    assert!(result.is_err(), "expected unique constraint violation");
}

#[tokio::test]
async fn test_unique_index_allows_reuse_after_stop() {
    let store = test_store().await;
    let s1 = make_session("reuse-name");
    store.insert_session(&s1).await.unwrap();
    store
        .update_session_status(&s1.id.to_string(), SessionStatus::Done, None)
        .await
        .unwrap();
    // New session with same name should succeed — old one is stopped
    let mut s2 = make_session("reuse-name");
    s2.id = uuid::Uuid::new_v4();
    store.insert_session(&s2).await.unwrap();
}

#[tokio::test]
async fn test_list_sessions_empty() {
    let store = test_store().await;
    let sessions = store.list_sessions().await.unwrap();
    assert!(sessions.is_empty());
}

#[tokio::test]
async fn test_list_sessions_multiple() {
    let store = test_store().await;
    let s1 = make_session("first");
    let s2 = make_session("second");

    store.insert_session(&s1).await.unwrap();
    store.insert_session(&s2).await.unwrap();

    let sessions = store.list_sessions().await.unwrap();
    assert_eq!(sessions.len(), 2);
}

#[tokio::test]
async fn test_update_session_status() {
    let store = test_store().await;
    let session = make_session("update-test");
    store.insert_session(&session).await.unwrap();

    store
        .update_session_status(&session.id.to_string(), SessionStatus::Done, None)
        .await
        .unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, SessionStatus::Done);
}

#[tokio::test]
async fn test_update_session_exit_code() {
    let store = test_store().await;
    let session = make_session("exit-code-test");
    store.insert_session(&session).await.unwrap();

    store
        .update_session_exit_code(&session.id.to_string(), 130)
        .await
        .unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.exit_code, Some(130));
}

#[tokio::test]
async fn test_update_session_exit_code_after_table_dropped() {
    let store = test_store().await;
    sqlx::query("DROP TABLE sessions")
        .execute(store.pool())
        .await
        .unwrap();
    let result = store.update_session_exit_code("test-id", 1).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_delete_session() {
    let store = test_store().await;
    let session = make_session("delete-test");
    store.insert_session(&session).await.unwrap();

    store.delete_session(&session.id.to_string()).await.unwrap();

    let result = store.get_session(&session.id.to_string()).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_insert_session_with_all_none_optionals() {
    let store = test_store().await;
    let session = Session {
        id: Uuid::new_v4(),
        name: "minimal".into(),
        workdir: "/tmp".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        ..Default::default()
    };

    store.insert_session(&session).await.unwrap();
    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();

    assert!(fetched.exit_code.is_none());
    assert!(fetched.backend_session_id.is_none());
    assert!(fetched.output_snapshot.is_none());
}

const TEST_UUID: &str = "550e8400-e29b-41d4-a716-446655440000";

#[tokio::test]
async fn test_row_to_session_invalid_status() {
    let store = test_store().await;
    sqlx::query(
        "INSERT INTO sessions (id, name, workdir, provider, prompt, status, mode,
                created_at, updated_at, command)
             VALUES (?, 'test', '/tmp', '', '', 'bad_status', '',
                '2024-01-01T00:00:00+00:00', '2024-01-01T00:00:00+00:00', 'echo test')",
    )
    .bind(TEST_UUID)
    .execute(store.pool())
    .await
    .unwrap();
    let result = store.get_session(TEST_UUID).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_row_to_session_invalid_uuid() {
    let store = test_store().await;
    sqlx::query(
        "INSERT INTO sessions (id, name, workdir, provider, prompt, status, mode,
                created_at, updated_at)
             VALUES ('not-a-uuid', 'test', '/tmp', 'claude', 'test', 'active', 'interactive',
                '2024-01-01T00:00:00+00:00', '2024-01-01T00:00:00+00:00')",
    )
    .execute(store.pool())
    .await
    .unwrap();
    let result = store.get_session("not-a-uuid").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_row_to_session_invalid_datetime() {
    let store = test_store().await;
    sqlx::query(
        "INSERT INTO sessions (id, name, workdir, provider, prompt, status, mode,
                created_at, updated_at)
             VALUES (?, 'test', '/tmp', 'claude', 'test', 'active', 'interactive',
                'not-a-date', '2024-01-01T00:00:00+00:00')",
    )
    .bind(TEST_UUID)
    .execute(store.pool())
    .await
    .unwrap();
    let result = store.get_session(TEST_UUID).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_list_sessions_after_table_dropped() {
    let store = test_store().await;
    sqlx::query("DROP TABLE sessions")
        .execute(store.pool())
        .await
        .unwrap();
    let result = store.list_sessions().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_get_session_after_table_dropped() {
    let store = test_store().await;
    sqlx::query("DROP TABLE sessions")
        .execute(store.pool())
        .await
        .unwrap();
    let result = store.get_session("test-id").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_insert_session_after_table_dropped() {
    let store = test_store().await;
    sqlx::query("DROP TABLE sessions")
        .execute(store.pool())
        .await
        .unwrap();
    let session = make_session("fail-test");
    let result = store.insert_session(&session).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_update_session_status_after_table_dropped() {
    let store = test_store().await;
    sqlx::query("DROP TABLE sessions")
        .execute(store.pool())
        .await
        .unwrap();
    let result = store
        .update_session_status("test-id", SessionStatus::Done, None)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_delete_session_after_table_dropped() {
    let store = test_store().await;
    sqlx::query("DROP TABLE sessions")
        .execute(store.pool())
        .await
        .unwrap();
    let result = store.delete_session("test-id").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_store_is_clone() {
    let store = test_store().await;
    let cloned = store.clone();
    // Both should work
    let sessions = cloned.list_sessions().await.unwrap();
    assert!(sessions.is_empty());
}

#[tokio::test]
async fn test_data_dir_accessor() {
    let store = test_store().await;
    let dir = store.data_dir();
    assert!(!dir.is_empty());
}

#[tokio::test]
async fn test_list_sessions_filtered_by_status() {
    let store = test_store().await;
    let mut s1 = make_session("running-1");
    s1.status = SessionStatus::Working;
    let mut s2 = make_session("completed-1");
    s2.status = SessionStatus::Done;
    store.insert_session(&s1).await.unwrap();
    store.insert_session(&s2).await.unwrap();

    let query = ListSessionsQuery {
        status: Some("working".into()),
        ..Default::default()
    };
    let sessions = store.list_sessions_filtered(&query).await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].status, SessionStatus::Working);
}

#[tokio::test]
async fn test_list_sessions_filtered_by_multiple_statuses() {
    let store = test_store().await;
    let mut s1 = make_session("running-2");
    s1.status = SessionStatus::Working;
    let mut s2 = make_session("completed-2");
    s2.status = SessionStatus::Done;
    let mut s3 = make_session("dead-1");
    s3.status = SessionStatus::Lost;
    store.insert_session(&s1).await.unwrap();
    store.insert_session(&s2).await.unwrap();
    store.insert_session(&s3).await.unwrap();

    let query = ListSessionsQuery {
        status: Some("working,done".into()),
        ..Default::default()
    };
    let sessions = store.list_sessions_filtered(&query).await.unwrap();
    assert_eq!(sessions.len(), 2);
}

#[tokio::test]
async fn test_list_sessions_filtered_by_search() {
    let store = test_store().await;
    let mut s1 = make_session("api-fix");
    s1.command = "Fix the API endpoint".into();
    let mut s2 = make_session("ui-refactor");
    s2.command = "Refactor the UI components".into();
    store.insert_session(&s1).await.unwrap();
    store.insert_session(&s2).await.unwrap();

    let query = ListSessionsQuery {
        search: Some("API".into()),
        ..Default::default()
    };
    let sessions = store.list_sessions_filtered(&query).await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].name, "api-fix");
}

#[tokio::test]
async fn test_list_sessions_filtered_search_by_name() {
    let store = test_store().await;
    let s1 = make_session("frontend-fix");
    let s2 = make_session("backend-fix");
    store.insert_session(&s1).await.unwrap();
    store.insert_session(&s2).await.unwrap();

    let query = ListSessionsQuery {
        search: Some("frontend".into()),
        ..Default::default()
    };
    let sessions = store.list_sessions_filtered(&query).await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].name, "frontend-fix");
}

#[tokio::test]
async fn test_list_sessions_filtered_sort_by_name() {
    let store = test_store().await;
    let s1 = make_session("aaa");
    let s2 = make_session("zzz");
    store.insert_session(&s1).await.unwrap();
    store.insert_session(&s2).await.unwrap();

    let query = ListSessionsQuery {
        sort: Some("name".into()),
        order: Some("asc".into()),
        ..Default::default()
    };
    let sessions = store.list_sessions_filtered(&query).await.unwrap();
    assert_eq!(sessions[0].name, "aaa");
    assert_eq!(sessions[1].name, "zzz");
}

#[tokio::test]
async fn test_list_sessions_filtered_sort_desc() {
    let store = test_store().await;
    let s1 = make_session("aaa");
    let s2 = make_session("zzz");
    store.insert_session(&s1).await.unwrap();
    store.insert_session(&s2).await.unwrap();

    let query = ListSessionsQuery {
        sort: Some("name".into()),
        order: Some("desc".into()),
        ..Default::default()
    };
    let sessions = store.list_sessions_filtered(&query).await.unwrap();
    assert_eq!(sessions[0].name, "zzz");
    assert_eq!(sessions[1].name, "aaa");
}

#[tokio::test]
async fn test_list_sessions_filtered_empty_returns_all() {
    let store = test_store().await;
    let s1 = make_session("one");
    let s2 = make_session("two");
    store.insert_session(&s1).await.unwrap();
    store.insert_session(&s2).await.unwrap();

    let query = ListSessionsQuery::default();
    let sessions = store.list_sessions_filtered(&query).await.unwrap();
    assert_eq!(sessions.len(), 2);
}

#[tokio::test]
async fn test_list_sessions_filtered_combined_filters() {
    let store = test_store().await;
    let mut s1 = make_session("api-fix");
    s1.status = SessionStatus::Working;
    s1.command = "Fix the API".into();
    let mut s2 = make_session("api-refactor");
    s2.status = SessionStatus::Done;
    s2.command = "Refactor the API".into();
    let mut s3 = make_session("ui-fix");
    s3.status = SessionStatus::Working;
    s3.command = "Fix the UI".into();
    store.insert_session(&s1).await.unwrap();
    store.insert_session(&s2).await.unwrap();
    store.insert_session(&s3).await.unwrap();

    let query = ListSessionsQuery {
        status: Some("working".into()),
        search: Some("API".into()),
        ..Default::default()
    };
    let sessions = store.list_sessions_filtered(&query).await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].name, "api-fix");
}

#[tokio::test]
async fn test_list_sessions_filtered_sort_by_status() {
    let store = test_store().await;
    let mut s1 = make_session("first");
    s1.status = SessionStatus::Working;
    let mut s2 = make_session("second");
    s2.status = SessionStatus::Done;
    store.insert_session(&s1).await.unwrap();
    store.insert_session(&s2).await.unwrap();

    let query = ListSessionsQuery {
        sort: Some("status".into()),
        order: Some("asc".into()),
        ..Default::default()
    };
    let sessions = store.list_sessions_filtered(&query).await.unwrap();
    assert_eq!(sessions.len(), 2);
}

#[tokio::test]
async fn test_list_sessions_filtered_sort_by_provider() {
    let store = test_store().await;
    let s1 = make_session("claude-task");
    let mut s2 = make_session("codex-task");
    s2.command = String::new();
    store.insert_session(&s1).await.unwrap();
    store.insert_session(&s2).await.unwrap();

    let query = ListSessionsQuery {
        sort: Some("provider".into()),
        order: Some("asc".into()),
        ..Default::default()
    };
    let sessions = store.list_sessions_filtered(&query).await.unwrap();
    assert_eq!(sessions.len(), 2);
}

#[tokio::test]
async fn test_update_session_intervention() {
    let store = test_store().await;
    let session = make_session("intervene-test");
    store.insert_session(&session).await.unwrap();

    store
        .update_session_intervention(
            &session.id.to_string(),
            InterventionCode::MemoryPressure,
            "Memory usage 95% (512MB/8192MB)",
        )
        .await
        .unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, SessionStatus::Done);
    assert_eq!(
        fetched.intervention_code,
        Some(InterventionCode::MemoryPressure)
    );
    assert_eq!(
        fetched.intervention_reason.as_deref(),
        Some("Memory usage 95% (512MB/8192MB)")
    );
    assert!(fetched.intervention_at.is_some());
}

#[tokio::test]
async fn test_update_session_intervention_after_table_dropped() {
    let store = test_store().await;
    sqlx::query("DROP TABLE sessions")
        .execute(store.pool())
        .await
        .unwrap();
    let result = store
        .update_session_intervention("test-id", InterventionCode::UserStop, "reason")
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_update_session_output_snapshot() {
    let store = test_store().await;
    let session = make_session("snapshot-test");
    store.insert_session(&session).await.unwrap();

    store
        .update_session_output_snapshot(
            &session.id.to_string(),
            "$ vitest\nrunning tests...\nOOM killed",
        )
        .await
        .unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fetched.output_snapshot.as_deref(),
        Some("$ vitest\nrunning tests...\nOOM killed")
    );
}

#[tokio::test]
async fn test_update_session_output_snapshot_after_table_dropped() {
    let store = test_store().await;
    sqlx::query("DROP TABLE sessions")
        .execute(store.pool())
        .await
        .unwrap();
    let result = store
        .update_session_output_snapshot("test-id", "snapshot")
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_intervention_roundtrip_with_insert() {
    let store = test_store().await;
    let mut session = make_session("intervention-insert");
    session.intervention_reason = Some("pre-set reason".into());
    session.intervention_at = Some(Utc::now());

    store.insert_session(&session).await.unwrap();
    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        fetched.intervention_reason.as_deref(),
        Some("pre-set reason")
    );
    assert!(fetched.intervention_at.is_some());
}

#[tokio::test]
async fn test_row_to_session_invalid_intervention_at() {
    let store = test_store().await;
    sqlx::query(
        "INSERT INTO sessions (id, name, workdir, provider, prompt, status, mode,
                intervention_at, created_at, updated_at)
             VALUES (?, 'test', '/tmp', 'claude', 'test', 'active', 'interactive',
                'not-a-date', '2024-01-01T00:00:00+00:00', '2024-01-01T00:00:00+00:00')",
    )
    .bind(TEST_UUID)
    .execute(store.pool())
    .await
    .unwrap();
    let result = store.get_session(TEST_UUID).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_list_sessions_filtered_after_table_dropped() {
    let store = test_store().await;
    sqlx::query("DROP TABLE sessions")
        .execute(store.pool())
        .await
        .unwrap();
    let query = ListSessionsQuery::default();
    let result = store.list_sessions_filtered(&query).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_intervention_events_appended() {
    let store = test_store().await;
    let session = make_session("events-test");
    store.insert_session(&session).await.unwrap();
    let sid = session.id.to_string();

    // First intervention
    store
        .update_session_intervention(&sid, InterventionCode::MemoryPressure, "Memory 95%")
        .await
        .unwrap();

    // Simulate a second intervention (e.g., session was resumed and hit pressure again)
    // Reset session to running first so the scenario makes sense — `update_session_intervention`
    // is now a compare-and-set that only fires from a live status (`starting`/`working`/`waiting`).
    sqlx::query("UPDATE sessions SET status = 'working' WHERE id = ?")
        .bind(&sid)
        .execute(store.pool())
        .await
        .unwrap();
    store
        .update_session_intervention(&sid, InterventionCode::MemoryPressure, "Memory 98%")
        .await
        .unwrap();

    let events = store.list_intervention_events(&sid).await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].code, Some(InterventionCode::MemoryPressure));
    assert_eq!(events[0].reason, "Memory 95%");
    assert_eq!(events[1].code, Some(InterventionCode::MemoryPressure));
    assert_eq!(events[1].reason, "Memory 98%");
    assert_eq!(events[0].session_id, sid);
    assert_eq!(events[1].session_id, sid);
    assert!(events[0].id < events[1].id);
}

#[tokio::test]
async fn test_intervention_events_empty_for_unknown_session() {
    let store = test_store().await;
    let events = store
        .list_intervention_events("nonexistent-id")
        .await
        .unwrap();
    assert!(events.is_empty());
}

#[tokio::test]
async fn test_intervention_events_after_table_dropped() {
    let store = test_store().await;
    sqlx::query("DROP TABLE intervention_events")
        .execute(store.pool())
        .await
        .unwrap();
    let result = store.list_intervention_events("any-id").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_delete_intervention_events_removes_only_that_sessions_rows() {
    let store = test_store().await;
    let session = make_session("purge-events-test");
    store.insert_session(&session).await.unwrap();
    let sid = session.id.to_string();

    let other = make_session("other-session");
    store.insert_session(&other).await.unwrap();
    let other_id = other.id.to_string();

    store
        .update_session_intervention(&sid, InterventionCode::MemoryPressure, "Memory 95%")
        .await
        .unwrap();
    store
        .update_session_intervention(&other_id, InterventionCode::IdleTimeout, "Idle 10m")
        .await
        .unwrap();

    store.delete_intervention_events(&sid).await.unwrap();

    assert!(
        store
            .list_intervention_events(&sid)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store
            .list_intervention_events(&other_id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn test_delete_intervention_events_noop_for_unknown_session() {
    let store = test_store().await;
    // No rows to delete — must succeed rather than error.
    store
        .delete_intervention_events("nonexistent-id")
        .await
        .unwrap();
}

#[tokio::test]
async fn test_intervention_event_debug_clone() {
    let event = InterventionEvent {
        id: 1,
        session_id: "test-id".into(),
        code: Some(InterventionCode::MemoryPressure),
        reason: "Memory 95%".into(),
        created_at: Utc::now(),
    };
    let debug = format!("{event:?}");
    assert!(debug.contains("Memory 95%"));
    #[allow(clippy::redundant_clone)]
    let cloned = event.clone();
    assert_eq!(cloned.reason, "Memory 95%");
}

#[tokio::test]
async fn test_last_output_at_updated_on_change() {
    let store = test_store().await;
    let session = make_session("output-ts");
    let id = session.id.to_string();
    store.insert_session(&session).await.unwrap();

    // Initially null
    let fetched = store.get_session(&id).await.unwrap().unwrap();
    assert!(fetched.last_output_at.is_none());

    // First snapshot — sets last_output_at
    store
        .update_session_output_snapshot(&id, "hello")
        .await
        .unwrap();
    let fetched = store.get_session(&id).await.unwrap().unwrap();
    assert!(fetched.last_output_at.is_some());
    let ts1 = fetched.last_output_at.unwrap();

    // Different content — updates last_output_at
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    store
        .update_session_output_snapshot(&id, "world")
        .await
        .unwrap();
    let fetched = store.get_session(&id).await.unwrap().unwrap();
    let ts2 = fetched.last_output_at.unwrap();
    assert!(ts2 > ts1);
}

#[tokio::test]
async fn test_last_output_at_not_updated_on_same() {
    let store = test_store().await;
    let session = make_session("output-same");
    let id = session.id.to_string();
    store.insert_session(&session).await.unwrap();

    // Set initial snapshot
    store
        .update_session_output_snapshot(&id, "same content")
        .await
        .unwrap();
    let fetched = store.get_session(&id).await.unwrap().unwrap();
    let ts1 = fetched.last_output_at.unwrap();

    // Same content — last_output_at should NOT change
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    store
        .update_session_output_snapshot(&id, "same content")
        .await
        .unwrap();
    let fetched = store.get_session(&id).await.unwrap().unwrap();
    let ts2 = fetched.last_output_at.unwrap();
    assert_eq!(ts1, ts2);
}

#[tokio::test]
async fn test_get_session_invalid_last_output_at() {
    let store = test_store().await;
    let session = make_session("bad-ts");
    store.insert_session(&session).await.unwrap();

    sqlx::query("UPDATE sessions SET last_output_at = 'not-a-date' WHERE id = ?")
        .bind(session.id.to_string())
        .execute(store.pool())
        .await
        .unwrap();

    let result = store.get_session(&session.id.to_string()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_get_session_invalid_updated_at() {
    let store = test_store().await;
    let session = make_session("bad-updated");
    store.insert_session(&session).await.unwrap();

    sqlx::query("UPDATE sessions SET updated_at = 'not-a-date' WHERE id = ?")
        .bind(session.id.to_string())
        .execute(store.pool())
        .await
        .unwrap();

    let result = store.get_session(&session.id.to_string()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_insert_session_with_last_output_at() {
    let store = test_store().await;
    let mut session = make_session("with-output-ts");
    session.last_output_at = Some(Utc::now());
    store.insert_session(&session).await.unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert!(fetched.last_output_at.is_some());
}

#[tokio::test]
async fn test_get_session_invalid_uuid() {
    let store = test_store().await;
    let session = make_session("bad-uuid");
    store.insert_session(&session).await.unwrap();

    sqlx::query("UPDATE sessions SET id = 'not-a-uuid' WHERE id = ?")
        .bind(session.id.to_string())
        .execute(store.pool())
        .await
        .unwrap();

    let result = store.get_session("not-a-uuid").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_list_intervention_events_invalid_created_at() {
    let store = test_store().await;
    let session = make_session("bad-event");
    store.insert_session(&session).await.unwrap();

    // Insert event with invalid timestamp directly
    sqlx::query(
        "INSERT INTO intervention_events (session_id, reason, created_at) VALUES (?, ?, ?)",
    )
    .bind(session.id.to_string())
    .bind("test")
    .bind("not-a-date")
    .execute(store.pool())
    .await
    .unwrap();

    let result = store
        .list_intervention_events(&session.id.to_string())
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_update_session_idle_since() {
    let store = test_store().await;
    let session = make_session("idle-test");
    store.insert_session(&session).await.unwrap();

    // Initially None
    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert!(fetched.idle_since.is_none());

    // Set idle_since
    store
        .update_session_idle_since(&session.id.to_string())
        .await
        .unwrap();
    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert!(fetched.idle_since.is_some());
}

#[tokio::test]
async fn test_clear_session_idle_since() {
    let store = test_store().await;
    let session = make_session("idle-clear");
    store.insert_session(&session).await.unwrap();

    // Set idle_since
    store
        .update_session_idle_since(&session.id.to_string())
        .await
        .unwrap();
    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert!(fetched.idle_since.is_some());

    // Clear idle_since
    store
        .clear_session_idle_since(&session.id.to_string())
        .await
        .unwrap();
    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert!(fetched.idle_since.is_none());
}

#[tokio::test]
async fn test_update_session_harness_sets_both_fields() {
    let store = test_store().await;
    let session = make_session("harness-test");
    store.insert_session(&session).await.unwrap();

    store
        .update_session_harness(&session.id.to_string(), "claude", Some("sid-1"))
        .await
        .unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.harness.as_deref(), Some("claude"));
    assert_eq!(fetched.harness_session_id.as_deref(), Some("sid-1"));
}

#[tokio::test]
async fn test_update_session_harness_with_no_id_keeps_existing_id() {
    let store = test_store().await;
    let session = make_session("harness-keep-id");
    store.insert_session(&session).await.unwrap();
    store
        .update_session_harness(&session.id.to_string(), "claude", Some("sid-1"))
        .await
        .unwrap();

    // A later call with no id (e.g. re-resolving the adapter on resume) must not
    // clobber the previously known harness_session_id.
    store
        .update_session_harness(&session.id.to_string(), "claude", None)
        .await
        .unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.harness_session_id.as_deref(), Some("sid-1"));
}

#[tokio::test]
async fn test_update_session_harness_session_id() {
    let store = test_store().await;
    let session = make_session("harness-session-id");
    store.insert_session(&session).await.unwrap();

    store
        .update_session_harness_session_id(&session.id.to_string(), "sid-from-hook")
        .await
        .unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.harness_session_id.as_deref(), Some("sid-from-hook"));
}

#[tokio::test]
async fn test_touch_harness_last_event_at() {
    let store = test_store().await;
    let session = make_session("harness-events");
    store.insert_session(&session).await.unwrap();

    let before = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert!(before.harness_last_event_at.is_none());

    store
        .touch_harness_last_event_at(&session.id.to_string())
        .await
        .unwrap();

    let after = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert!(after.harness_last_event_at.is_some());
}

#[tokio::test]
async fn test_clear_harness_last_event_at() {
    let store = test_store().await;
    let session = make_session("clear-harness-events");
    store.insert_session(&session).await.unwrap();
    store
        .touch_harness_last_event_at(&session.id.to_string())
        .await
        .unwrap();
    let before = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert!(before.harness_last_event_at.is_some());

    store
        .clear_harness_last_event_at(&session.id.to_string())
        .await
        .unwrap();

    let after = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert!(after.harness_last_event_at.is_none());
}

#[tokio::test]
async fn test_insert_session_with_harness_fields() {
    let store = test_store().await;
    let mut session = make_session("harness-insert");
    session.harness = Some("claude".into());
    session.harness_session_id = Some("sid-preset".into());
    store.insert_session(&session).await.unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.harness.as_deref(), Some("claude"));
    assert_eq!(fetched.harness_session_id.as_deref(), Some("sid-preset"));
}

#[tokio::test]
async fn test_insert_session_with_idle_since() {
    let store = test_store().await;
    let mut session = make_session("with-idle");
    session.idle_since = Some(Utc::now());
    store.insert_session(&session).await.unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert!(fetched.idle_since.is_some());
}

#[tokio::test]
async fn test_get_session_invalid_idle_since() {
    let store = test_store().await;
    let session = make_session("bad-idle");
    store.insert_session(&session).await.unwrap();

    sqlx::query("UPDATE sessions SET idle_since = 'not-a-date' WHERE id = ?")
        .bind(session.id.to_string())
        .execute(store.pool())
        .await
        .unwrap();

    let result = store.get_session(&session.id.to_string()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_new_session_fields_roundtrip() {
    let store = test_store().await;
    let mut session = make_session("new-fields-test");
    session.metadata = Some(
        [
            ("discord_channel".into(), "123".into()),
            ("user".into(), "alice".into()),
        ]
        .into_iter()
        .collect(),
    );
    session.ink = Some("reviewer".into());

    store.insert_session(&session).await.unwrap();
    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();

    let meta = fetched.metadata.unwrap();
    assert_eq!(meta.get("discord_channel").unwrap(), "123");
    assert_eq!(meta.get("user").unwrap(), "alice");
    assert_eq!(fetched.ink, Some("reviewer".into()));
}

#[tokio::test]
async fn test_migrate_dedicated_connection_open_failure() {
    // `migrate()` opens its own dedicated connection to `{data_dir}/state.db`,
    // independent of `self.pool` (see its doc comment) — closing the pool no
    // longer affects it at all (that's the point: the pool and the migration
    // connection can never poison each other). What *does* still make
    // `migrate()` fail is the dedicated connection itself failing to open —
    // e.g. the database file having vanished out from under the store.
    let tmpdir = tempfile::tempdir().unwrap();
    let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
    std::fs::remove_dir_all(tmpdir.path()).unwrap();

    let result = store.migrate().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_intervention_code_roundtrip() {
    let store = test_store().await;
    let mut session = make_session("code-roundtrip");
    session.intervention_code = Some(InterventionCode::IdleTimeout);
    session.intervention_reason = Some("Idle for 10 minutes".into());
    session.intervention_at = Some(Utc::now());
    store.insert_session(&session).await.unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fetched.intervention_code,
        Some(InterventionCode::IdleTimeout)
    );
    assert_eq!(
        fetched.intervention_reason.as_deref(),
        Some("Idle for 10 minutes")
    );
}

#[tokio::test]
async fn test_intervention_code_none_roundtrip() {
    let store = test_store().await;
    let session = make_session("code-none");
    store.insert_session(&session).await.unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert!(fetched.intervention_code.is_none());
}

#[tokio::test]
async fn test_row_to_session_invalid_intervention_code() {
    // An unrecognized intervention_code (garbage, or a retired code like the removed
    // burn-velocity governor's "burn_rate") must not fail the whole session read — it
    // degrades to `None` so the rest of the session stays readable (see `store::rows`).
    let store = test_store().await;
    sqlx::query(
        "INSERT INTO sessions (id, name, workdir, provider, prompt, status, mode,
                intervention_code, created_at, updated_at)
             VALUES (?, 'test', '/tmp', 'claude', 'test', 'active', 'interactive',
                'invalid_code', '2024-01-01T00:00:00+00:00', '2024-01-01T00:00:00+00:00')",
    )
    .bind(TEST_UUID)
    .execute(store.pool())
    .await
    .unwrap();
    let session = store.get_session(TEST_UUID).await.unwrap().unwrap();
    assert_eq!(session.intervention_code, None);
}

#[tokio::test]
async fn test_intervention_event_code_roundtrip() {
    let store = test_store().await;
    let session = make_session("event-code");
    store.insert_session(&session).await.unwrap();
    let sid = session.id.to_string();

    store
        .update_session_intervention(&sid, InterventionCode::IdleTimeout, "Idle 15 min")
        .await
        .unwrap();

    let events = store.list_intervention_events(&sid).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].code, Some(InterventionCode::IdleTimeout));
    assert_eq!(events[0].reason, "Idle 15 min");
}

#[tokio::test]
async fn test_intervention_event_user_stop_code() {
    let store = test_store().await;
    let session = make_session("user-stop");
    store.insert_session(&session).await.unwrap();
    let sid = session.id.to_string();

    store
        .update_session_intervention(&sid, InterventionCode::UserStop, "Manual stop")
        .await
        .unwrap();

    let fetched = store.get_session(&sid).await.unwrap().unwrap();
    assert_eq!(fetched.intervention_code, Some(InterventionCode::UserStop));

    let events = store.list_intervention_events(&sid).await.unwrap();
    assert_eq!(events[0].code, Some(InterventionCode::UserStop));
}

#[tokio::test]
async fn test_idle_status_roundtrip() {
    let store = test_store().await;
    let session = make_session("idle-test");
    store.insert_session(&session).await.unwrap();

    store
        .update_session_status(&session.id.to_string(), SessionStatus::Waiting, None)
        .await
        .unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, SessionStatus::Waiting);
}
