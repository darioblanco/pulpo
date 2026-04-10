use super::*;
use chrono::Utc;
use pulpo_common::api::{ListSessionsQuery, SessionIndexEntry};
use pulpo_common::session::InterventionCode;
use pulpo_common::session::{Runtime, Session, SessionStatus};
use uuid::Uuid;

fn make_session(name: &str) -> Session {
    Session {
        id: Uuid::new_v4(),
        name: name.into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("Fix the bug".into()),
        status: SessionStatus::Active,
        backend_session_id: Some(name.to_owned()),
        ..Default::default()
    }
}

async fn test_store() -> Store {
    let tmpdir = tempfile::tempdir().unwrap();
    // Leak so it persists for test lifetime
    let tmpdir = Box::leak(Box::new(tmpdir));
    let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
    store.migrate().await.unwrap();
    store
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
    assert_eq!(versions, vec![1, 2, 3]);

    let has_sandbox: i32 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'sandbox'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(has_sandbox, 0);
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

    assert_eq!(fetched.status, SessionStatus::Active);

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

    // Insert a stopped session with name "dup"
    let mut stopped = make_session("dup");
    stopped.id = uuid::Uuid::new_v4();
    stopped.status = SessionStatus::Stopped;
    // Remove from unique index by marking stopped before insert
    store.insert_session(&stopped).await.unwrap();

    // Insert a ready session with the same name "dup"
    let mut ready = make_session("dup-ready");
    ready.id = uuid::Uuid::new_v4();
    ready.name = "dup".into();
    ready.status = SessionStatus::Ready;
    // The unique index only covers creating/active/idle/ready,
    // and stopped is excluded, so this insert should work
    store.insert_session(&ready).await.unwrap();

    // get_session by name should return the ready one, not the stopped one
    let fetched = store.get_session("dup").await.unwrap().unwrap();
    assert_eq!(fetched.status, SessionStatus::Ready);
    assert_eq!(fetched.id, ready.id);
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
    session.status = SessionStatus::Stopped;
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
    session.status = SessionStatus::Idle;
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
    session.status = SessionStatus::Creating;
    store.insert_session(&session).await.unwrap();

    assert!(
        store
            .has_active_session_by_name("creating-session")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn test_has_active_session_by_name_ready() {
    let store = test_store().await;
    let mut session = make_session("ready-session");
    session.status = SessionStatus::Ready;
    store.insert_session(&session).await.unwrap();

    assert!(
        store
            .has_active_session_by_name("ready-session")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn test_has_active_session_by_name_excluding_self() {
    let store = test_store().await;
    let mut session = make_session("ready-session");
    session.status = SessionStatus::Ready;
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
    session.status = SessionStatus::Active;
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
        .update_session_status(&s1.id.to_string(), SessionStatus::Stopped)
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
        .update_session_status(&session.id.to_string(), SessionStatus::Ready)
        .await
        .unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, SessionStatus::Ready);
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
        .update_session_status("test-id", SessionStatus::Stopped)
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
    s1.status = SessionStatus::Active;
    let mut s2 = make_session("completed-1");
    s2.status = SessionStatus::Ready;
    store.insert_session(&s1).await.unwrap();
    store.insert_session(&s2).await.unwrap();

    let query = ListSessionsQuery {
        status: Some("active".into()),
        ..Default::default()
    };
    let sessions = store.list_sessions_filtered(&query).await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].status, SessionStatus::Active);
}

#[tokio::test]
async fn test_list_sessions_filtered_by_multiple_statuses() {
    let store = test_store().await;
    let mut s1 = make_session("running-2");
    s1.status = SessionStatus::Active;
    let mut s2 = make_session("completed-2");
    s2.status = SessionStatus::Ready;
    let mut s3 = make_session("dead-1");
    s3.status = SessionStatus::Stopped;
    store.insert_session(&s1).await.unwrap();
    store.insert_session(&s2).await.unwrap();
    store.insert_session(&s3).await.unwrap();

    let query = ListSessionsQuery {
        status: Some("active,ready".into()),
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
    s1.status = SessionStatus::Active;
    s1.command = "Fix the API".into();
    let mut s2 = make_session("api-refactor");
    s2.status = SessionStatus::Ready;
    s2.command = "Refactor the API".into();
    let mut s3 = make_session("ui-fix");
    s3.status = SessionStatus::Active;
    s3.command = "Fix the UI".into();
    store.insert_session(&s1).await.unwrap();
    store.insert_session(&s2).await.unwrap();
    store.insert_session(&s3).await.unwrap();

    let query = ListSessionsQuery {
        status: Some("active".into()),
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
    s1.status = SessionStatus::Active;
    let mut s2 = make_session("second");
    s2.status = SessionStatus::Ready;
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
    assert_eq!(fetched.status, SessionStatus::Stopped);
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
async fn test_clear_session_intervention() {
    let store = test_store().await;
    let session = make_session("clear-test");
    store.insert_session(&session).await.unwrap();

    // Set intervention first
    store
        .update_session_intervention(
            &session.id.to_string(),
            InterventionCode::UserStop,
            "test reason",
        )
        .await
        .unwrap();

    // Clear it
    store
        .clear_session_intervention(&session.id.to_string())
        .await
        .unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert!(fetched.intervention_code.is_none());
    assert!(fetched.intervention_reason.is_none());
    assert!(fetched.intervention_at.is_none());
}

#[tokio::test]
async fn test_clear_session_intervention_after_table_dropped() {
    let store = test_store().await;
    sqlx::query("DROP TABLE sessions")
        .execute(store.pool())
        .await
        .unwrap();
    let result = store.clear_session_intervention("test-id").await;
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
    // Reset session to running first so the scenario makes sense
    sqlx::query("UPDATE sessions SET status = 'active' WHERE id = ?")
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
async fn test_migrate_closed_pool_error() {
    let store = test_store().await;
    store.pool().close().await;
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
    let result = store.get_session(TEST_UUID).await;
    assert!(result.is_err());
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
        .update_session_status(&session.id.to_string(), SessionStatus::Idle)
        .await
        .unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, SessionStatus::Idle);
}

// -- Push subscription tests --

#[tokio::test]
async fn test_push_subscription_save_and_list() {
    let store = test_store().await;
    store
        .save_push_subscription("https://push.example.com/1", "p256dh-key", "auth-key")
        .await
        .unwrap();

    let subs = store.list_push_subscriptions().await.unwrap();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].endpoint, "https://push.example.com/1");
    assert_eq!(subs[0].p256dh, "p256dh-key");
    assert_eq!(subs[0].auth, "auth-key");
}

#[tokio::test]
async fn test_push_subscription_save_replaces_on_same_endpoint() {
    let store = test_store().await;
    store
        .save_push_subscription("https://push.example.com/1", "old-p256dh", "old-auth")
        .await
        .unwrap();
    store
        .save_push_subscription("https://push.example.com/1", "new-p256dh", "new-auth")
        .await
        .unwrap();

    let subs = store.list_push_subscriptions().await.unwrap();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].p256dh, "new-p256dh");
    assert_eq!(subs[0].auth, "new-auth");
}

#[tokio::test]
async fn test_push_subscription_multiple_endpoints() {
    let store = test_store().await;
    store
        .save_push_subscription("https://push.example.com/1", "p1", "a1")
        .await
        .unwrap();
    store
        .save_push_subscription("https://push.example.com/2", "p2", "a2")
        .await
        .unwrap();

    let subs = store.list_push_subscriptions().await.unwrap();
    assert_eq!(subs.len(), 2);
}

#[tokio::test]
async fn test_push_subscription_delete() {
    let store = test_store().await;
    store
        .save_push_subscription("https://push.example.com/1", "p1", "a1")
        .await
        .unwrap();
    store
        .save_push_subscription("https://push.example.com/2", "p2", "a2")
        .await
        .unwrap();

    store
        .delete_push_subscription("https://push.example.com/1")
        .await
        .unwrap();

    let subs = store.list_push_subscriptions().await.unwrap();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].endpoint, "https://push.example.com/2");
}

#[tokio::test]
async fn test_push_subscription_delete_nonexistent() {
    let store = test_store().await;
    // Should not error when deleting a non-existent endpoint
    store
        .delete_push_subscription("https://push.example.com/nonexistent")
        .await
        .unwrap();
}

#[tokio::test]
async fn test_push_subscription_list_empty() {
    let store = test_store().await;
    let subs = store.list_push_subscriptions().await.unwrap();
    assert!(subs.is_empty());
}

#[tokio::test]
async fn test_push_subscription_debug_clone() {
    let sub = PushSubscription {
        endpoint: "https://push.example.com/1".into(),
        p256dh: "key".into(),
        auth: "auth".into(),
    };
    let debug = format!("{sub:?}");
    assert!(debug.contains("push.example.com"));
    #[allow(clippy::redundant_clone)]
    let cloned = sub.clone();
    assert_eq!(cloned.endpoint, "https://push.example.com/1");
}

#[tokio::test]
async fn test_push_subscription_after_table_dropped() {
    let store = test_store().await;
    sqlx::query("DROP TABLE push_subscriptions")
        .execute(store.pool())
        .await
        .unwrap();
    let result = store.list_push_subscriptions().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_push_subscription_save_after_table_dropped() {
    let store = test_store().await;
    sqlx::query("DROP TABLE push_subscriptions")
        .execute(store.pool())
        .await
        .unwrap();
    let result = store
        .save_push_subscription("https://push.example.com/1", "p", "a")
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_push_subscription_delete_after_table_dropped() {
    let store = test_store().await;
    sqlx::query("DROP TABLE push_subscriptions")
        .execute(store.pool())
        .await
        .unwrap();
    let result = store
        .delete_push_subscription("https://push.example.com/1")
        .await;
    assert!(result.is_err());
}

// -- Schedule tests --

#[tokio::test]
async fn test_schedule_crud() {
    let store = test_store().await;
    let schedule = pulpo_common::api::Schedule {
        id: "sched-1".into(),
        name: "nightly-review".into(),
        cron: "0 3 * * *".into(),
        command: "claude -p 'review'".into(),
        workdir: "/tmp".into(),
        target_node: None,
        ink: None,
        description: Some("Nightly review".into()),
        runtime: None,
        secrets: vec![],
        worktree: None,
        worktree_base: None,
        enabled: true,
        last_run_at: None,
        last_session_id: None,
        last_attempted_at: None,
        last_error: None,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    store.insert_schedule(&schedule).await.unwrap();

    let fetched = store.get_schedule("nightly-review").await.unwrap().unwrap();
    assert_eq!(fetched.name, "nightly-review");
    assert_eq!(fetched.cron, "0 3 * * *");
    assert!(fetched.enabled);

    let all = store.list_schedules().await.unwrap();
    assert_eq!(all.len(), 1);

    store
        .update_schedule_enabled(&schedule.id, false)
        .await
        .unwrap();
    let updated = store.get_schedule(&schedule.id).await.unwrap().unwrap();
    assert!(!updated.enabled);

    store
        .update_schedule_last_run(&schedule.id, "session-123")
        .await
        .unwrap();
    let ran = store.get_schedule(&schedule.id).await.unwrap().unwrap();
    assert!(ran.last_run_at.is_some());
    assert_eq!(ran.last_session_id, Some("session-123".into()));

    store.delete_schedule(&schedule.id).await.unwrap();
    assert!(store.get_schedule(&schedule.id).await.unwrap().is_none());
}

#[tokio::test]
async fn test_list_schedule_runs() {
    let store = test_store().await;

    // Insert matching sessions (name starts with "nightly-")
    let s1 = make_session("nightly-001");
    let s2 = make_session("nightly-002");
    // Insert non-matching session
    let s3 = make_session("other-task");

    store.insert_session(&s1).await.unwrap();
    store.insert_session(&s2).await.unwrap();
    store.insert_session(&s3).await.unwrap();

    let runs = store.list_schedule_runs("nightly", 20).await.unwrap();
    assert_eq!(runs.len(), 2);
    for run in &runs {
        assert!(run.name.starts_with("nightly-"));
    }

    // Test limit
    let runs = store.list_schedule_runs("nightly", 1).await.unwrap();
    assert_eq!(runs.len(), 1);

    // Test no matches
    let runs = store.list_schedule_runs("nonexistent", 20).await.unwrap();
    assert!(runs.is_empty());
}

#[tokio::test]
async fn test_schedule_unique_name() {
    let store = test_store().await;
    let schedule = pulpo_common::api::Schedule {
        id: "s1".into(),
        name: "dup".into(),
        cron: "* * * * *".into(),
        command: "echo".into(),
        workdir: "/tmp".into(),
        target_node: None,
        ink: None,
        description: None,
        runtime: None,
        secrets: vec![],
        worktree: None,
        worktree_base: None,
        enabled: true,
        last_run_at: None,
        last_session_id: None,
        last_attempted_at: None,
        last_error: None,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    store.insert_schedule(&schedule).await.unwrap();
    let dup = pulpo_common::api::Schedule {
        id: "s2".into(),
        name: "dup".into(),
        ..schedule
    };
    assert!(store.insert_schedule(&dup).await.is_err());
}

#[tokio::test]
async fn test_schedule_execution_fields_roundtrip() {
    let store = test_store().await;
    let schedule = pulpo_common::api::Schedule {
        id: "sched-exec".into(),
        name: "docker-review".into(),
        cron: "0 3 * * *".into(),
        command: "claude -p 'review'".into(),
        workdir: "/tmp".into(),
        target_node: None,
        ink: Some("coder".into()),
        description: Some("Docker review".into()),
        runtime: Some("docker".into()),
        secrets: vec!["GH_TOKEN".into(), "NPM_TOKEN".into()],
        worktree: Some(true),
        worktree_base: Some("main".into()),
        enabled: true,
        last_run_at: None,
        last_session_id: None,
        last_attempted_at: None,
        last_error: None,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    store.insert_schedule(&schedule).await.unwrap();

    let fetched = store.get_schedule("docker-review").await.unwrap().unwrap();
    assert_eq!(fetched.runtime, Some("docker".into()));
    assert_eq!(fetched.secrets, vec!["GH_TOKEN", "NPM_TOKEN"]);
    assert_eq!(fetched.worktree, Some(true));
    assert_eq!(fetched.worktree_base, Some("main".into()));
}

#[tokio::test]
async fn test_schedule_execution_fields_default_empty() {
    let store = test_store().await;
    let schedule = pulpo_common::api::Schedule {
        id: "sched-empty".into(),
        name: "plain".into(),
        cron: "0 3 * * *".into(),
        command: "echo".into(),
        workdir: "/tmp".into(),
        target_node: None,
        ink: None,
        description: None,
        runtime: None,
        secrets: vec![],
        worktree: None,
        worktree_base: None,
        enabled: true,
        last_run_at: None,
        last_session_id: None,
        last_attempted_at: None,
        last_error: None,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    store.insert_schedule(&schedule).await.unwrap();

    let fetched = store.get_schedule("plain").await.unwrap().unwrap();
    assert!(fetched.runtime.is_none());
    assert!(fetched.secrets.is_empty());
    assert!(fetched.worktree.is_none());
    assert!(fetched.worktree_base.is_none());
}

#[tokio::test]
async fn test_record_schedule_failure_updates_attempted() {
    let store = test_store().await;
    let schedule = pulpo_common::api::Schedule {
        id: "sched-fail".into(),
        name: "failing-schedule".into(),
        cron: "0 0 * * *".into(),
        command: "echo".into(),
        workdir: "/tmp".into(),
        target_node: None,
        ink: None,
        description: None,
        runtime: None,
        secrets: vec![],
        worktree: None,
        worktree_base: None,
        enabled: true,
        last_run_at: None,
        last_session_id: None,
        last_attempted_at: None,
        last_error: None,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    store.insert_schedule(&schedule).await.unwrap();

    store
        .record_schedule_failure(&schedule.id, "node unavailable")
        .await
        .unwrap();

    let fetched = store.get_schedule(&schedule.id).await.unwrap().unwrap();
    assert!(fetched.last_attempted_at.is_some());
    assert_eq!(fetched.last_error.as_deref(), Some("node unavailable"));
}

// -- update_session_metadata_field tests --

#[tokio::test]
async fn test_update_session_metadata_field_empty_metadata() {
    let store = test_store().await;
    let session = make_session("meta-empty");
    store.insert_session(&session).await.unwrap();

    store
        .update_session_metadata_field(
            &session.id.to_string(),
            "pr_url",
            "https://github.com/a/b/pull/1",
        )
        .await
        .unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    let meta = fetched.metadata.unwrap();
    assert_eq!(meta.get("pr_url").unwrap(), "https://github.com/a/b/pull/1");
}

#[tokio::test]
async fn test_update_session_metadata_field_existing_metadata() {
    let store = test_store().await;
    let mut session = make_session("meta-existing");
    session.metadata = Some(std::iter::once(("discord_channel".into(), "123".into())).collect());
    store.insert_session(&session).await.unwrap();

    store
        .update_session_metadata_field(&session.id.to_string(), "branch", "feature/test")
        .await
        .unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    let meta = fetched.metadata.unwrap();
    // Original key preserved
    assert_eq!(meta.get("discord_channel").unwrap(), "123");
    // New key added
    assert_eq!(meta.get("branch").unwrap(), "feature/test");
}

#[tokio::test]
async fn test_update_session_metadata_field_overwrite_key() {
    let store = test_store().await;
    let session = make_session("meta-overwrite");
    store.insert_session(&session).await.unwrap();

    store
        .update_session_metadata_field(&session.id.to_string(), "pr_url", "https://old")
        .await
        .unwrap();
    store
        .update_session_metadata_field(&session.id.to_string(), "pr_url", "https://new")
        .await
        .unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fetched.metadata.unwrap().get("pr_url").unwrap(),
        "https://new"
    );
}

#[tokio::test]
async fn test_update_session_metadata_field_nonexistent_session() {
    let store = test_store().await;
    let result = store
        .update_session_metadata_field("nonexistent-id", "key", "value")
        .await;
    assert!(result.is_err());
}

// -- Secret tests --

#[tokio::test]
async fn test_set_and_get_secret() {
    let store = test_store().await;
    store.set_secret("MY_TOKEN", "abc123").await.unwrap();
    let value = store.get_secret("MY_TOKEN").await.unwrap();
    assert_eq!(value, Some("abc123".into()));
}

#[tokio::test]
async fn test_get_secret_not_found() {
    let store = test_store().await;
    let value = store.get_secret("NONEXISTENT").await.unwrap();
    assert!(value.is_none());
}

#[tokio::test]
async fn test_set_secret_upsert() {
    let store = test_store().await;
    store.set_secret("MY_TOKEN", "old").await.unwrap();
    store.set_secret("MY_TOKEN", "new").await.unwrap();
    let value = store.get_secret("MY_TOKEN").await.unwrap();
    assert_eq!(value, Some("new".into()));
}

#[tokio::test]
async fn test_list_secret_names() {
    let store = test_store().await;
    store.set_secret("B_TOKEN", "val").await.unwrap();
    store.set_secret("A_TOKEN", "val").await.unwrap();
    let names = store.list_secret_names().await.unwrap();
    assert_eq!(names.len(), 2);
    assert_eq!(names[0].0, "A_TOKEN");
    assert!(names[0].1.is_none()); // no env override
    assert_eq!(names[1].0, "B_TOKEN");
    // created_at should be non-empty
    assert!(!names[0].2.is_empty());
}

#[tokio::test]
async fn test_list_secret_names_with_env() {
    let store = test_store().await;
    store
        .set_secret_with_env("GH_WORK", "token1", Some("GITHUB_TOKEN"))
        .await
        .unwrap();
    store.set_secret("PLAIN_KEY", "token2").await.unwrap();
    let names = store.list_secret_names().await.unwrap();
    assert_eq!(names.len(), 2);
    assert_eq!(names[0].0, "GH_WORK");
    assert_eq!(names[0].1.as_deref(), Some("GITHUB_TOKEN"));
    assert_eq!(names[1].0, "PLAIN_KEY");
    assert!(names[1].1.is_none());
}

#[tokio::test]
async fn test_list_secret_names_empty() {
    let store = test_store().await;
    let names = store.list_secret_names().await.unwrap();
    assert!(names.is_empty());
}

#[tokio::test]
async fn test_delete_secret_found() {
    let store = test_store().await;
    store.set_secret("MY_TOKEN", "val").await.unwrap();
    let deleted = store.delete_secret("MY_TOKEN").await.unwrap();
    assert!(deleted);
    let value = store.get_secret("MY_TOKEN").await.unwrap();
    assert!(value.is_none());
}

#[tokio::test]
async fn test_delete_secret_not_found() {
    let store = test_store().await;
    let deleted = store.delete_secret("NONEXISTENT").await.unwrap();
    assert!(!deleted);
}

#[tokio::test]
async fn test_set_secret_with_env() {
    let store = test_store().await;
    store
        .set_secret_with_env("GH_WORK", "token123", Some("GITHUB_TOKEN"))
        .await
        .unwrap();
    let value = store.get_secret("GH_WORK").await.unwrap();
    assert_eq!(value, Some("token123".into()));
}

#[tokio::test]
async fn test_set_secret_with_env_none() {
    let store = test_store().await;
    store
        .set_secret_with_env("MY_KEY", "val", None)
        .await
        .unwrap();
    let value = store.get_secret("MY_KEY").await.unwrap();
    assert_eq!(value, Some("val".into()));
}

#[tokio::test]
async fn test_set_secret_with_env_upsert() {
    let store = test_store().await;
    store
        .set_secret_with_env("GH_WORK", "old", Some("OLD_VAR"))
        .await
        .unwrap();
    store
        .set_secret_with_env("GH_WORK", "new", Some("NEW_VAR"))
        .await
        .unwrap();
    let value = store.get_secret("GH_WORK").await.unwrap();
    assert_eq!(value, Some("new".into()));
    let names = store.list_secret_names().await.unwrap();
    assert_eq!(names[0].1.as_deref(), Some("NEW_VAR"));
}

#[tokio::test]
async fn test_get_secrets_for_injection() {
    let store = test_store().await;
    store
        .set_secret_with_env("GH_WORK", "token1", Some("GITHUB_TOKEN"))
        .await
        .unwrap();
    store.set_secret("NPM_TOKEN", "token2").await.unwrap();
    let secrets = store
        .get_secrets_for_injection(&["GH_WORK".into(), "NPM_TOKEN".into()])
        .await
        .unwrap();
    assert_eq!(secrets.len(), 2);
    // GH_WORK has env override → key is GITHUB_TOKEN
    assert_eq!(secrets.get("GITHUB_TOKEN").unwrap(), "token1");
    // NPM_TOKEN has no env override → key is NPM_TOKEN
    assert_eq!(secrets.get("NPM_TOKEN").unwrap(), "token2");
}

#[tokio::test]
async fn test_get_secrets_for_injection_missing() {
    let store = test_store().await;
    store.set_secret("EXISTING", "val").await.unwrap();
    let secrets = store
        .get_secrets_for_injection(&["EXISTING".into(), "MISSING".into()])
        .await
        .unwrap();
    assert_eq!(secrets.len(), 1);
    assert_eq!(secrets.get("EXISTING").unwrap(), "val");
}

#[tokio::test]
async fn test_get_secrets_for_injection_empty() {
    let store = test_store().await;
    let secrets = store.get_secrets_for_injection(&[]).await.unwrap();
    assert!(secrets.is_empty());
}

#[tokio::test]
async fn test_get_secrets_for_injection_env_collision() {
    let store = test_store().await;
    store
        .set_secret_with_env("GH_WORK", "val1", Some("GITHUB_TOKEN"))
        .await
        .unwrap();
    store
        .set_secret_with_env("GH_PERSONAL", "val2", Some("GITHUB_TOKEN"))
        .await
        .unwrap();
    let err = store
        .get_secrets_for_injection(&["GH_WORK".into(), "GH_PERSONAL".into()])
        .await
        .unwrap_err();
    assert!(err.to_string().contains("both map to env var"), "{err}");
}

#[tokio::test]
async fn test_get_all_secrets() {
    let store = test_store().await;
    store.set_secret("KEY_A", "val_a").await.unwrap();
    store.set_secret("KEY_B", "val_b").await.unwrap();
    let all = store.get_all_secrets().await.unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all.get("KEY_A").unwrap(), "val_a");
    assert_eq!(all.get("KEY_B").unwrap(), "val_b");
}

#[tokio::test]
async fn test_get_all_secrets_empty() {
    let store = test_store().await;
    let all = store.get_all_secrets().await.unwrap();
    assert!(all.is_empty());
}

#[tokio::test]
async fn test_secret_after_table_dropped() {
    let store = test_store().await;
    sqlx::query("DROP TABLE secrets")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(store.set_secret("K", "V").await.is_err());
    assert!(
        store
            .set_secret_with_env("K", "V", Some("E"))
            .await
            .is_err()
    );
    assert!(store.get_secret("K").await.is_err());
    assert!(store.list_secret_names().await.is_err());
    assert!(store.delete_secret("K").await.is_err());
    assert!(store.get_all_secrets().await.is_err());
    assert!(
        store
            .get_secrets_for_injection(&["K".into()])
            .await
            .is_err()
    );
}

#[tokio::test]
async fn test_get_secrets_for_injection_name_vs_env_collision() {
    // Secret A has no env override (env var = "GITHUB_TOKEN", its name).
    // Secret B has env = "GITHUB_TOKEN".
    // Requesting both should detect the collision.
    let store = test_store().await;
    store.set_secret("GITHUB_TOKEN", "val1").await.unwrap();
    store
        .set_secret_with_env("GH_WORK", "val2", Some("GITHUB_TOKEN"))
        .await
        .unwrap();
    let err = store
        .get_secrets_for_injection(&["GITHUB_TOKEN".into(), "GH_WORK".into()])
        .await
        .unwrap_err();
    assert!(err.to_string().contains("both map to env var"), "{err}");
}

#[tokio::test]
async fn test_get_secrets_for_injection_single_secret() {
    let store = test_store().await;
    store.set_secret("ONLY_ONE", "val").await.unwrap();
    let secrets = store
        .get_secrets_for_injection(&["ONLY_ONE".into()])
        .await
        .unwrap();
    assert_eq!(secrets.len(), 1);
    assert_eq!(secrets.get("ONLY_ONE").unwrap(), "val");
}

#[tokio::test]
async fn test_get_secrets_for_injection_all_missing() {
    let store = test_store().await;
    let secrets = store
        .get_secrets_for_injection(&["MISSING_A".into(), "MISSING_B".into()])
        .await
        .unwrap();
    assert!(secrets.is_empty());
}

#[tokio::test]
async fn test_migrate_creates_secrets_table() {
    let tmpdir = tempfile::tempdir().unwrap();
    let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
    store.migrate().await.unwrap();

    // Verify secrets table exists
    let result = sqlx::query("SELECT count(*) as cnt FROM secrets")
        .fetch_one(store.pool())
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_migrate_secrets_env_column_exists() {
    let tmpdir = tempfile::tempdir().unwrap();
    let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
    store.migrate().await.unwrap();

    // Verify env column exists in secrets table
    let has_env: i32 =
        sqlx::query_scalar("SELECT COUNT(*) FROM pragma_table_info('secrets') WHERE name = 'env'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(has_env, 1);
}

#[tokio::test]
async fn test_unknown_runtime_in_db_defaults_to_tmux() {
    let store = test_store().await;
    // Insert a row with an unknown runtime value
    sqlx::query(
            "INSERT INTO sessions (id, name, workdir, provider, prompt, status, mode,
                runtime, command, created_at, updated_at)
             VALUES (?, 'test', '/tmp', '', '', 'active', '',
                'unknown_runtime', 'echo test', '2024-01-01T00:00:00+00:00', '2024-01-01T00:00:00+00:00')",
        )
        .bind(TEST_UUID)
        .execute(store.pool())
        .await
        .unwrap();
    let session = store.get_session(TEST_UUID).await.unwrap().unwrap();
    assert_eq!(session.runtime, Runtime::Tmux);
}

#[tokio::test]
async fn test_empty_runtime_in_db_defaults_to_tmux() {
    let store = test_store().await;
    // Insert a row then force runtime to empty string (simulates corrupt/old data)
    sqlx::query(
        "INSERT INTO sessions (id, name, workdir, provider, prompt, status, mode,
                command, created_at, updated_at, runtime)
             VALUES (?, 'test', '/tmp', '', '', 'active', '',
                'echo test', '2024-01-01T00:00:00+00:00', '2024-01-01T00:00:00+00:00', '')",
    )
    .bind(TEST_UUID)
    .execute(store.pool())
    .await
    .unwrap();
    let session = store.get_session(TEST_UUID).await.unwrap().unwrap();
    // Empty string doesn't parse to a valid Runtime, so .ok() returns None,
    // and .unwrap_or_default() gives Tmux
    assert_eq!(session.runtime, Runtime::Tmux);
}

#[tokio::test]
async fn test_insert_and_get_session_with_docker_runtime() {
    let store = test_store().await;
    let mut session = make_session("docker-session");
    session.runtime = Runtime::Docker;
    session.backend_session_id = Some("docker:pulpo-docker-session".into());
    store.insert_session(&session).await.unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.runtime, Runtime::Docker);
    assert_eq!(
        fetched.backend_session_id.as_deref(),
        Some("docker:pulpo-docker-session")
    );
}

#[tokio::test]
async fn test_migrate_creates_runtime_column() {
    let tmpdir = tempfile::tempdir().unwrap();
    let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
    store.migrate().await.unwrap();

    let has_runtime: i32 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'runtime'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(has_runtime, 1);
}

#[tokio::test]
async fn test_migrate_creates_schedules_table() {
    let tmpdir = tempfile::tempdir().unwrap();
    let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
    store.migrate().await.unwrap();

    let result = sqlx::query("SELECT count(*) FROM schedules")
        .fetch_one(store.pool())
        .await;
    assert!(result.is_ok());
}

#[cfg(unix)]
#[tokio::test]
async fn test_db_file_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let tmpdir = tempfile::tempdir().unwrap();
    let tmpdir = Box::leak(Box::new(tmpdir));
    let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
    store.migrate().await.unwrap();
    let db_path = tmpdir.path().join("state.db");
    let metadata = std::fs::metadata(&db_path).unwrap();
    let mode = metadata.permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[tokio::test]
async fn test_update_session_git_info() {
    let store = test_store().await;
    let mut session = make_session("git-test");
    session.id = Uuid::new_v4();
    store.insert_session(&session).await.unwrap();

    store
        .update_session_git_info(&session.id.to_string(), Some("main"), Some("abc1234"))
        .await
        .unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.git_branch, Some("main".into()));
    assert_eq!(fetched.git_commit, Some("abc1234".into()));
}

#[tokio::test]
async fn test_update_session_git_info_clears() {
    let store = test_store().await;
    let mut session = make_session("git-clear");
    session.id = Uuid::new_v4();
    session.git_branch = Some("feat".into());
    session.git_commit = Some("deadbeef".into());
    store.insert_session(&session).await.unwrap();

    store
        .update_session_git_info(&session.id.to_string(), None, None)
        .await
        .unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert!(fetched.git_branch.is_none());
    assert!(fetched.git_commit.is_none());
}

#[tokio::test]
async fn test_insert_session_with_git_info() {
    let store = test_store().await;
    let mut session = make_session("git-insert");
    session.id = Uuid::new_v4();
    session.git_branch = Some("develop".into());
    session.git_commit = Some("ff00ff".into());
    store.insert_session(&session).await.unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.git_branch, Some("develop".into()));
    assert_eq!(fetched.git_commit, Some("ff00ff".into()));
}

#[tokio::test]
async fn test_update_session_git_diff() {
    let store = test_store().await;
    let session = make_session("git-diff-test");
    store.insert_session(&session).await.unwrap();

    store
        .update_session_git_diff(&session.id.to_string(), Some(3), Some(42), Some(7))
        .await
        .unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.git_files_changed, Some(3));
    assert_eq!(fetched.git_insertions, Some(42));
    assert_eq!(fetched.git_deletions, Some(7));
}

#[tokio::test]
async fn test_update_session_git_diff_none() {
    let store = test_store().await;
    let mut session = make_session("git-diff-none");
    session.git_files_changed = Some(5);
    session.git_insertions = Some(10);
    session.git_deletions = Some(3);
    store.insert_session(&session).await.unwrap();

    // Clear diff stats
    store
        .update_session_git_diff(&session.id.to_string(), None, None, None)
        .await
        .unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.git_files_changed, None);
    assert_eq!(fetched.git_insertions, None);
    assert_eq!(fetched.git_deletions, None);
}

#[tokio::test]
async fn test_update_session_git_ahead() {
    let store = test_store().await;
    let session = make_session("git-ahead-test");
    store.insert_session(&session).await.unwrap();

    store
        .update_session_git_ahead(&session.id.to_string(), Some(5))
        .await
        .unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.git_ahead, Some(5));
}

#[tokio::test]
async fn test_update_session_git_ahead_none() {
    let store = test_store().await;
    let mut session = make_session("git-ahead-none");
    session.git_ahead = Some(3);
    store.insert_session(&session).await.unwrap();

    store
        .update_session_git_ahead(&session.id.to_string(), None)
        .await
        .unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.git_ahead, None);
}

#[tokio::test]
async fn test_remove_session_metadata_field() {
    let store = test_store().await;
    let session = make_session("meta-remove");
    store.insert_session(&session).await.unwrap();

    // Add two metadata fields
    store
        .update_session_metadata_field(&session.id.to_string(), "error_status", "Panic")
        .await
        .unwrap();
    store
        .update_session_metadata_field(&session.id.to_string(), "other_key", "value")
        .await
        .unwrap();

    // Remove one
    store
        .remove_session_metadata_field(&session.id.to_string(), "error_status")
        .await
        .unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    let meta = fetched.metadata.unwrap();
    assert!(!meta.contains_key("error_status"));
    assert_eq!(meta.get("other_key"), Some(&"value".into()));
}

#[tokio::test]
async fn test_insert_and_read_git_telemetry_fields() {
    let store = test_store().await;
    let mut session = make_session("telemetry-roundtrip");
    session.git_files_changed = Some(10);
    session.git_insertions = Some(100);
    session.git_deletions = Some(50);
    session.git_ahead = Some(7);
    store.insert_session(&session).await.unwrap();

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.git_files_changed, Some(10));
    assert_eq!(fetched.git_insertions, Some(100));
    assert_eq!(fetched.git_deletions, Some(50));
    assert_eq!(fetched.git_ahead, Some(7));
}

#[tokio::test]
async fn test_controller_session_index_roundtrip() {
    let store = test_store().await;
    let entry = SessionIndexEntry {
        session_id: "remote-1".into(),
        node_name: "node-1".into(),
        node_address: Some("node-1.tail:7433".into()),
        session_name: "nightly-review".into(),
        status: "active".into(),
        command: Some("claude -p 'review'".into()),
        updated_at: "2026-04-01T20:00:00Z".into(),
    };

    store
        .upsert_controller_session_index_entry(&entry)
        .await
        .unwrap();
    let entries = store.list_controller_session_index_entries().await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].session_id, "remote-1");
    assert_eq!(entries[0].node_name, "node-1");
    assert_eq!(entries[0].node_address.as_deref(), Some("node-1.tail:7433"));
}

#[tokio::test]
async fn test_controller_session_index_delete() {
    let store = test_store().await;
    let entry = SessionIndexEntry {
        session_id: "remote-2".into(),
        node_name: "node-2".into(),
        node_address: None,
        session_name: "batch-run".into(),
        status: "idle".into(),
        command: None,
        updated_at: "2026-04-01T21:00:00Z".into(),
    };

    store
        .upsert_controller_session_index_entry(&entry)
        .await
        .unwrap();
    store
        .delete_controller_session_index_entry("remote-2")
        .await
        .unwrap();
    assert!(
        store
            .list_controller_session_index_entries()
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn test_controller_nodes_roundtrip() {
    let store = test_store().await;
    store
        .touch_controller_node("node-1", "2026-04-01T22:00:00Z")
        .await
        .unwrap();
    store
        .touch_controller_node("node-2", "2026-04-01T22:05:00Z")
        .await
        .unwrap();

    let workers = store.list_controller_nodes().await.unwrap();
    assert_eq!(workers.len(), 2);
    assert_eq!(workers[0].0, "node-1");
    assert_eq!(workers[1].0, "node-2");
    assert_eq!(workers[0].1.to_rfc3339(), "2026-04-01T22:00:00+00:00");
}

#[tokio::test]
async fn test_controller_enrolled_nodes_roundtrip() {
    let store = test_store().await;
    store
        .enroll_controller_node(
            "node-1",
            "hash-1",
            Some("2026-04-01T22:00:00Z"),
            Some("10.0.0.10"),
        )
        .await
        .unwrap();

    let by_name = store
        .get_enrolled_controller_node_by_name("node-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(by_name.node_name, "node-1");
    assert_eq!(by_name.token_hash, "hash-1");
    assert_eq!(by_name.last_seen_address.as_deref(), Some("10.0.0.10"));

    let by_hash = store
        .get_enrolled_controller_node_by_token_hash("hash-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(by_hash.node_name, "node-1");
}

#[tokio::test]
async fn test_touch_enrolled_controller_node_updates_seen_fields() {
    let store = test_store().await;
    store
        .enroll_controller_node("node-2", "hash-2", None, None)
        .await
        .unwrap();
    store
        .touch_enrolled_controller_node("node-2", "2026-04-01T22:30:00Z", Some("10.0.0.20"))
        .await
        .unwrap();

    let node = store
        .get_enrolled_controller_node_by_name("node-2")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        node.last_seen_at.unwrap().to_rfc3339(),
        "2026-04-01T22:30:00+00:00"
    );
    assert_eq!(node.last_seen_address.as_deref(), Some("10.0.0.20"));
}

#[tokio::test]
async fn test_delete_sessions_bulk_empty_ids() {
    let store = test_store().await;
    // Should be a no-op without error
    store.delete_sessions_bulk(&[]).await.unwrap();
}

#[tokio::test]
async fn test_delete_sessions_bulk_deletes_all() {
    let store = test_store().await;

    let s1 = make_session("bulk-a");
    let s2 = make_session("bulk-b");
    store.insert_session(&s1).await.unwrap();
    store.insert_session(&s2).await.unwrap();

    let ids = vec![s1.id.to_string(), s2.id.to_string()];
    store.delete_sessions_bulk(&ids).await.unwrap();

    assert!(
        store
            .get_session(&s1.id.to_string())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .get_session(&s2.id.to_string())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn test_fetch_dead_sessions_empty() {
    let store = test_store().await;
    let dead = store.fetch_dead_sessions().await.unwrap();
    assert!(dead.is_empty());
}

#[tokio::test]
async fn test_fetch_dead_sessions_returns_stopped_and_lost() {
    let store = test_store().await;

    let mut stopped = make_session("stopped-one");
    stopped.status = SessionStatus::Stopped;
    store.insert_session(&stopped).await.unwrap();

    let mut lost = make_session("lost-one");
    lost.status = SessionStatus::Lost;
    store.insert_session(&lost).await.unwrap();

    let dead = store.fetch_dead_sessions().await.unwrap();
    assert_eq!(dead.len(), 2);
    let names: Vec<&str> = dead.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"stopped-one"));
    assert!(names.contains(&"lost-one"));
}

#[tokio::test]
async fn test_fetch_dead_sessions_excludes_active() {
    let store = test_store().await;

    let active = make_session("active-one");
    store.insert_session(&active).await.unwrap();

    let mut stopped = make_session("stopped-two");
    stopped.status = SessionStatus::Stopped;
    store.insert_session(&stopped).await.unwrap();

    let dead = store.fetch_dead_sessions().await.unwrap();
    assert_eq!(dead.len(), 1);
    assert_eq!(dead[0].name, "stopped-two");
}

#[tokio::test]
async fn test_fetch_dead_sessions_preserves_worktree_path() {
    let store = test_store().await;

    let mut s = make_session("wt-session");
    s.status = SessionStatus::Stopped;
    s.worktree_path = Some("/home/user/.pulpo/worktrees/wt-session".into());
    store.insert_session(&s).await.unwrap();

    let dead = store.fetch_dead_sessions().await.unwrap();
    assert_eq!(dead.len(), 1);
    assert_eq!(
        dead[0].worktree_path.as_deref(),
        Some("/home/user/.pulpo/worktrees/wt-session")
    );
}
