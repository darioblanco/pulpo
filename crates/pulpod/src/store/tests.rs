use super::*;
use chrono::Utc;
use pulpo_common::api::ListSessionsQuery;
use pulpo_common::session::InterventionCode;
use pulpo_common::session::{Session, SessionStatus};
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
    assert_eq!(versions, vec![1, 2, 3, 4, 5, 6]);

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
async fn test_find_live_sessions_by_worktree_finds_other_live_session() {
    let store = test_store().await;
    let mut source = make_session("plan-auth");
    source.worktree_path = Some("/tmp/wt/plan-auth".into());
    store.insert_session(&source).await.unwrap();

    let mut handoff = make_session("plan-auth-2");
    handoff.worktree_path = Some("/tmp/wt/plan-auth".into());
    handoff.status = SessionStatus::Active;
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
    dead.status = SessionStatus::Stopped;
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
}
