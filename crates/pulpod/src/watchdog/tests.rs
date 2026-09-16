use super::*;
use crate::backend::Backend;
use crate::store::test_store;
use anyhow::Result;
use pulpo_common::session::{Session, SessionStatus};
use std::sync::Mutex;
use tokio::time;

struct MockBackend {
    kill_calls: Mutex<Vec<String>>,
    capture_calls: Mutex<Vec<String>>,
    create_calls: Mutex<Vec<String>>,
    create_commands: Mutex<Vec<String>>,
    output: String,
    fail_capture: bool,
    fail_kill: bool,
    fail_create: bool,
    alive: bool,
    fail_is_alive: bool,
}

impl MockBackend {
    fn new() -> Self {
        Self {
            kill_calls: Mutex::new(Vec::new()),
            capture_calls: Mutex::new(Vec::new()),
            create_calls: Mutex::new(Vec::new()),
            create_commands: Mutex::new(Vec::new()),
            output: "test output".into(),
            fail_capture: false,
            fail_kill: false,
            fail_create: false,
            alive: true,
            fail_is_alive: false,
        }
    }

    fn with_output(self, output: &str) -> Self {
        Self {
            output: output.into(),
            ..self
        }
    }

    fn failing_capture() -> Self {
        Self {
            fail_capture: true,
            ..Self::new()
        }
    }

    fn failing_kill() -> Self {
        Self {
            fail_kill: true,
            ..Self::new()
        }
    }

    fn failing_create() -> Self {
        Self {
            fail_create: true,
            ..Self::new()
        }
    }

    /// `is_alive()` reports the backend gone — the eager dead-backend-resolution
    /// path this session's idle check should take.
    fn dead() -> Self {
        Self {
            alive: false,
            ..Self::new()
        }
    }

    /// `is_alive()` itself errors (e.g. the `tmux` binary vanished) — distinct
    /// from a clean "not alive" answer.
    fn failing_is_alive() -> Self {
        Self {
            fail_is_alive: true,
            ..Self::new()
        }
    }
}

impl Backend for MockBackend {
    fn create_session(&self, name: &str, _: &str, command: &str) -> Result<()> {
        self.create_calls.lock().unwrap().push(name.into());
        self.create_commands.lock().unwrap().push(command.into());
        if self.fail_create {
            anyhow::bail!("create failed");
        }
        Ok(())
    }
    fn kill_session(&self, name: &str) -> Result<()> {
        self.kill_calls.lock().unwrap().push(name.into());
        if self.fail_kill {
            anyhow::bail!("kill failed");
        }
        Ok(())
    }
    fn is_alive(&self, _: &str) -> Result<bool> {
        if self.fail_is_alive {
            anyhow::bail!("is_alive check failed");
        }
        Ok(self.alive)
    }
    fn capture_output(&self, name: &str, _: usize) -> Result<String> {
        self.capture_calls.lock().unwrap().push(name.into());
        if self.fail_capture {
            anyhow::bail!("capture failed");
        }
        Ok(self.output.clone())
    }
    fn send_input(&self, _: &str, _: &str) -> Result<()> {
        Ok(())
    }
    fn setup_logging(&self, _: &str, _: &str) -> Result<()> {
        Ok(())
    }
}

fn test_ready_ctx() -> ReadyContext {
    ReadyContext {
        event_tx: None,
        node_name: "test-node".into(),
    }
}

async fn create_running_session(store: &Store, name: &str) -> Session {
    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: name.into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        status: SessionStatus::Working,
        backend_session_id: Some(name.to_owned()),
        ..Default::default()
    };
    store.insert_session(&session).await.unwrap();
    session
}

fn make_config(interval: Duration, idle: IdleConfig) -> WatchdogRuntimeConfig {
    WatchdogRuntimeConfig {
        interval,
        idle,
        extra_waiting_patterns: Vec::new(),
    }
}

// ───────────────────────────────────────────────────────────
// Eager dead-backend resolution (HIGH follow-up on PR #129 / ADR 0009): the
// watchdog's own idle-check tick must notice a dead backend via `is_alive()`
// itself, resolve it through the shared `session::manager::resolve_dead_backend_session`,
// and fire the `lifecycle` event with the session's real previous status —
// not wait for something else to poll `GET /sessions/{id}`.
// ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_check_idle_sessions_resolves_dead_backend_to_done_eagerly() {
    let backend: Arc<dyn Backend> = Arc::new(MockBackend::dead());
    let store = test_store().await;
    let session = create_running_session(&store, "dead-backend-done").await;

    let code_path =
        crate::session::utils::exit_code_marker_path(store.data_dir(), &session.id.to_string());
    std::fs::create_dir_all(code_path.parent().unwrap()).unwrap();
    std::fs::write(&code_path, "7").unwrap();

    let (tx, mut rx) = broadcast::channel::<PulpoEvent>(16);
    let ctx = ReadyContext {
        event_tx: Some(tx),
        node_name: "test-node".into(),
    };
    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 600,
        action: IdleAction::Alert,
        threshold_secs: 60,
    };

    check_idle_sessions(&backend, &store, &idle_config, &ctx, &[]).await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, SessionStatus::Done);
    assert_eq!(fetched.status_reason.as_deref(), Some("exited"));
    assert_eq!(fetched.exit_code, Some(7));

    let event = rx.try_recv().expect("expected a lifecycle event");
    match event {
        PulpoEvent::Session(se) => {
            assert_eq!(se.status, "done");
            assert_eq!(
                se.previous_status.as_deref(),
                Some("working"),
                "must report the session's real previous status"
            );
        }
        other => panic!("expected a Session event, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_check_idle_sessions_resolves_dead_backend_to_lost_eagerly() {
    // No exit marker written — a dead backend with no evidence of a clean end
    // resolves to `Lost`, same as the lazy `get_session`/`list_sessions` path.
    let backend: Arc<dyn Backend> = Arc::new(MockBackend::dead());
    let store = test_store().await;
    let session = create_running_session(&store, "dead-backend-lost").await;

    check_idle_sessions(
        &backend,
        &store,
        &IdleConfig::default(),
        &test_ready_ctx(),
        &[],
    )
    .await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, SessionStatus::Lost);
}

#[tokio::test]
async fn test_run_watchdog_tick_resolves_dead_backend_even_when_idle_disabled() {
    // `idle_timeout_secs = 0` (`cfg.idle.enabled == false`) disables the
    // idle-timeout ALERT/KILL breaker — it must NOT also disable basic
    // dead-backend detection. Before this fix, the entire eager sweep lived
    // inside `check_idle_sessions`, itself skipped whenever idle detection was
    // disabled, so a session's dead backend would sit unresolved until the next
    // lazy `get_session`/`list_sessions` call.
    let backend: Arc<dyn Backend> = Arc::new(MockBackend::dead());
    let store = test_store().await;
    let session = create_running_session(&store, "dead-backend-idle-disabled").await;

    let idle_config = IdleConfig {
        enabled: false,
        ..IdleConfig::default()
    };
    let cfg = make_config(Duration::from_secs(10), idle_config);
    let (tx, mut rx) = broadcast::channel::<PulpoEvent>(16);
    let ctx = ReadyContext {
        event_tx: Some(tx),
        node_name: "test-node".into(),
    };

    run_watchdog_tick(&backend, &store, &cfg, &ctx).await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fetched.status,
        SessionStatus::Lost,
        "the dead backend must still be resolved even with idle detection disabled"
    );

    let event = rx.try_recv().expect("expected a lifecycle event");
    match event {
        PulpoEvent::Session(se) => assert_eq!(se.status, "lost"),
        other => panic!("expected a Session event, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_resolve_and_report_dead_session_skips_log_and_event_when_raced() {
    // A concurrent caller (another watchdog tick, or a `GET`/`list_sessions` call
    // racing this one) already resolved this session's backend-dead CAS by the
    // time this call runs — `resolve_and_report_dead_session` must not log
    // "resolved" or emit a stale `lifecycle` event for a transition it didn't
    // actually make (PR #129/#118 follow-up: `Ok(false)` was previously ignored
    // entirely).
    let backend: Arc<dyn Backend> = Arc::new(MockBackend::dead());
    let store = test_store().await;
    let session = create_running_session(&store, "raced-dead-backend").await;

    // Simulate the race directly: the row is already terminal (`Done`) by the
    // time this call's own CAS runs, even though the in-memory `session` value
    // below (an older snapshot, as a caller iterating `list_sessions()` would
    // hold) still says `Working`.
    store
        .update_session_status(&session.id.to_string(), SessionStatus::Done, None)
        .await
        .unwrap();

    let (tx, mut rx) = broadcast::channel::<PulpoEvent>(16);
    let ctx = ReadyContext {
        event_tx: Some(tx),
        node_name: "test-node".into(),
    };

    resolve_and_report_dead_session(
        &store,
        &backend,
        &resolve_backend_id(&session, backend.as_ref()),
        &session,
        &ctx,
    )
    .await;

    assert!(
        rx.try_recv().is_err(),
        "must not emit a lifecycle event for a transition this call didn't make"
    );
    // The row's real status (set by the "concurrent winner") must be left alone.
    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, SessionStatus::Done);
}

#[tokio::test]
async fn test_check_session_idle_is_alive_error_skips_tick_without_panicking() {
    // `is_alive()` itself failing (as opposed to cleanly reporting "not alive")
    // must not be treated as a dead backend — just skip this session for the
    // tick, same as a `capture_output` failure already did before this check
    // existed.
    let backend: Arc<dyn Backend> = Arc::new(MockBackend::failing_is_alive());
    let store = test_store().await;
    let session = create_running_session(&store, "is-alive-errors").await;

    check_idle_sessions(
        &backend,
        &store,
        &IdleConfig::default(),
        &test_ready_ctx(),
        &[],
    )
    .await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, SessionStatus::Working);
}

#[tokio::test]
async fn test_watchdog_shutdown() {
    let backend = Arc::new(MockBackend::new());
    let store = test_store().await;
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let handle = tokio::spawn(run_watchdog_loop(
        backend,
        store,
        make_config(
            Duration::from_millis(10),
            IdleConfig {
                enabled: false,
                ..IdleConfig::default()
            },
        ),
        shutdown_rx,
        test_ready_ctx(),
    ));

    // Let it run briefly then shutdown
    time::sleep(Duration::from_millis(50)).await;
    shutdown_tx.send(true).unwrap();
    handle.await.unwrap();
}

#[test]
fn test_mock_backend_methods() {
    let b = MockBackend::new();
    assert!(b.create_session("n", "d", "c").is_ok());
    assert!(b.is_alive("n").unwrap());
    assert!(b.send_input("n", "t").is_ok());
    assert!(b.setup_logging("n", "p").is_ok());
}

#[test]
fn test_mock_backend_failing_capture() {
    let b = MockBackend::failing_capture();
    assert!(b.capture_output("n", 10).is_err());
}

#[test]
fn test_mock_backend_failing_kill() {
    let b = MockBackend::failing_kill();
    assert!(b.kill_session("n").is_err());
}

#[test]
fn test_mock_backend_failing_create() {
    let b = MockBackend::failing_create();
    assert!(b.create_session("n", "d", "c").is_err());
}

#[test]
fn test_idle_config_default() {
    let ic = IdleConfig::default();
    assert!(ic.enabled);
    assert_eq!(ic.timeout_secs, 600);
    assert_eq!(ic.action, IdleAction::Alert);
    assert_eq!(ic.threshold_secs, 60);
}

#[test]
fn test_idle_config_debug_clone() {
    let ic = IdleConfig {
        enabled: true,
        timeout_secs: 300,
        action: IdleAction::Kill,
        threshold_secs: 60,
    };
    let debug = format!("{ic:?}");
    assert!(debug.contains("Kill"));
    #[allow(clippy::redundant_clone)]
    let cloned = ic.clone();
    assert!(cloned.enabled);
    assert_eq!(cloned.action, IdleAction::Kill);
}

#[test]
fn test_idle_action_eq() {
    assert_eq!(IdleAction::Alert, IdleAction::Alert);
    assert_eq!(IdleAction::Kill, IdleAction::Kill);
    assert_ne!(IdleAction::Alert, IdleAction::Kill);
}

#[test]
fn test_idle_action_copy() {
    let a = IdleAction::Alert;
    let b = a;
    assert_eq!(a, b);
}

#[test]
fn test_idle_action_debug() {
    assert_eq!(format!("{:?}", IdleAction::Alert), "Alert");
    assert_eq!(format!("{:?}", IdleAction::Kill), "Kill");
}

#[tokio::test]
async fn test_idle_detection_marks_idle() {
    let backend = Arc::new(MockBackend::new());
    let store = test_store().await;

    // Create a session with old last_output_at (well past timeout)
    let mut session = Session {
        id: uuid::Uuid::new_v4(),
        name: "idle-session".into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        status: SessionStatus::Working,
        backend_session_id: Some("idle-session".into()),
        output_snapshot: Some("test output".into()),
        last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
        ..Default::default()
    };
    // Set the output_snapshot to match what MockBackend returns
    session.output_snapshot = Some("test output".into());
    store.insert_session(&session).await.unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 600,
        action: IdleAction::Alert,
        threshold_secs: 60,
    };

    let dyn_backend: Arc<dyn Backend> = backend;
    check_idle_sessions(&dyn_backend, &store, &idle_config, &test_ready_ctx(), &[]).await;

    // Session should have transitioned from Active to Idle (output unchanged > 20s)
    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, SessionStatus::Waiting);
}

#[tokio::test]
async fn test_idle_threshold_override_honored() {
    let backend = Arc::new(MockBackend::new());
    let store = test_store().await;

    // Global threshold is very high (would never trigger for 700s of quiet
    // output), but the session's own override (60s) should be used instead.
    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "override-session".into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        status: SessionStatus::Working,
        backend_session_id: Some("override-session".into()),
        output_snapshot: Some("test output".into()),
        last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
        idle_threshold_secs: Some(60),
        ..Default::default()
    };
    store.insert_session(&session).await.unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 6000,
        action: IdleAction::Alert,
        threshold_secs: 6000,
    };

    let dyn_backend: Arc<dyn Backend> = backend;
    check_idle_sessions(&dyn_backend, &store, &idle_config, &test_ready_ctx(), &[]).await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, SessionStatus::Waiting);
}

#[tokio::test]
async fn test_idle_threshold_zero_disables_transition() {
    let backend = Arc::new(MockBackend::new());
    let store = test_store().await;

    // Global threshold would normally mark this idle (700s > 60s), but the
    // session's own override of 0 disables the time-based transition.
    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "never-idle-session".into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        status: SessionStatus::Working,
        backend_session_id: Some("never-idle-session".into()),
        output_snapshot: Some("test output".into()),
        last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
        idle_threshold_secs: Some(0),
        ..Default::default()
    };
    store.insert_session(&session).await.unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 6000,
        action: IdleAction::Alert,
        threshold_secs: 60,
    };

    let dyn_backend: Arc<dyn Backend> = backend;
    check_idle_sessions(&dyn_backend, &store, &idle_config, &test_ready_ctx(), &[]).await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, SessionStatus::Working);
    assert!(fetched.idle_since.is_none());
}

#[tokio::test]
async fn test_idle_threshold_none_falls_back_to_global() {
    let backend = Arc::new(MockBackend::new());
    let store = test_store().await;

    // No per-session override: the global threshold_secs (60) applies.
    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "global-threshold-session".into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        status: SessionStatus::Working,
        backend_session_id: Some("global-threshold-session".into()),
        output_snapshot: Some("test output".into()),
        last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
        idle_threshold_secs: None,
        ..Default::default()
    };
    store.insert_session(&session).await.unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 6000,
        action: IdleAction::Alert,
        threshold_secs: 60,
    };

    let dyn_backend: Arc<dyn Backend> = backend;
    check_idle_sessions(&dyn_backend, &store, &idle_config, &test_ready_ctx(), &[]).await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, SessionStatus::Waiting);
}

#[tokio::test]
async fn test_idle_detection_kill_action() {
    let backend = Arc::new(MockBackend::new());
    let store = test_store().await;

    // Session is already Idle with idle_since set — tests the kill path in
    // handle_idle_session (Active sessions now transition to Idle first).
    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "kill-idle".into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        status: SessionStatus::Waiting,
        backend_session_id: Some("kill-idle".into()),
        output_snapshot: Some("test output".into()),
        last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
        idle_since: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
        ..Default::default()
    };
    store.insert_session(&session).await.unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 600,
        action: IdleAction::Kill,
        threshold_secs: 60,
    };

    let backend_clone = backend.clone();
    let dyn_backend: Arc<dyn Backend> = backend_clone;
    check_idle_sessions(&dyn_backend, &store, &idle_config, &test_ready_ctx(), &[]).await;

    // Session should be dead with intervention reason
    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, SessionStatus::Done);
    assert!(fetched.intervention_reason.unwrap().contains("Idle"));

    // Kill should have been called
    assert!(
        backend
            .kill_calls
            .lock()
            .unwrap()
            .contains(&"kill-idle".to_owned())
    );
}

#[tokio::test]
async fn test_idle_timeout_does_not_kill_needs_input_session() {
    // A session parked on `waiting:needs_input:<reason>` is blocked on a real
    // decision from the operator (a permission prompt), not "idle" in the sense
    // `idle_timeout_secs` means — it must never be force-stopped by the same
    // breaker that kills a session nobody's touched. Same fixture shape as
    // `test_idle_detection_kill_action` (well past the timeout), except for the
    // `status_reason`.
    let backend = Arc::new(MockBackend::new());
    let store = test_store().await;

    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "needs-input-not-killed".into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        status: SessionStatus::Waiting,
        status_reason: Some("needs_input:permission".into()),
        backend_session_id: Some("needs-input-not-killed".into()),
        output_snapshot: Some("test output".into()),
        last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
        idle_since: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
        ..Default::default()
    };
    store.insert_session(&session).await.unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 600,
        action: IdleAction::Kill,
        threshold_secs: 60,
    };

    let dyn_backend: Arc<dyn Backend> = backend.clone();
    check_idle_sessions(&dyn_backend, &store, &idle_config, &test_ready_ctx(), &[]).await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, SessionStatus::Waiting);
    assert_eq!(
        fetched.status_reason.as_deref(),
        Some("needs_input:permission")
    );
    assert!(
        backend.kill_calls.lock().unwrap().is_empty(),
        "idle-timeout must never kill a session waiting on operator input"
    );
}

#[tokio::test]
async fn test_idle_kill_records_output_snapshot() {
    // Regression: idle-killed sessions must save a final output snapshot before
    // the kill (previously the idle Kill path skipped the capture step, losing
    // the session's last output).
    let backend = Arc::new(MockBackend::new().with_output("final agent output"));
    let store = test_store().await;

    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "kill-snap".into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        status: SessionStatus::Waiting,
        backend_session_id: Some("kill-snap".into()),
        last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
        idle_since: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
        ..Default::default()
    };
    store.insert_session(&session).await.unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 600,
        action: IdleAction::Kill,
        threshold_secs: 60,
    };
    let now = chrono::Utc::now();
    let timeout = chrono::Duration::seconds(600);
    let dyn_backend: Arc<dyn Backend> = backend;

    handle_idle_session(
        &dyn_backend,
        &store,
        &idle_config,
        &session,
        now,
        timeout,
        &test_ready_ctx(),
    )
    .await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, SessionStatus::Done);
    assert_eq!(
        fetched.intervention_code,
        Some(pulpo_common::session::InterventionCode::IdleTimeout)
    );
    // The final output was captured and stored before the kill.
    assert_eq!(
        fetched.output_snapshot.as_deref(),
        Some("final agent output")
    );
}

#[tokio::test]
async fn test_idle_detection_clears_when_active() {
    let backend = Arc::new(MockBackend::new());
    let store = test_store().await;

    // Create session that was idle but now has new output
    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "active-again".into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        status: SessionStatus::Working,
        backend_session_id: Some("active-again".into()),
        output_snapshot: Some("old output".into()),
        last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
        idle_since: Some(chrono::Utc::now() - chrono::Duration::seconds(100)),
        ..Default::default()
    };
    store.insert_session(&session).await.unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 600,
        action: IdleAction::Alert,
        threshold_secs: 60,
    };

    let dyn_backend: Arc<dyn Backend> = backend;
    check_idle_sessions(&dyn_backend, &store, &idle_config, &test_ready_ctx(), &[]).await;

    // idle_since should be cleared (output changed from "old output" to "test output")
    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert!(fetched.idle_since.is_none());
    assert_eq!(fetched.status, SessionStatus::Working);
}

#[tokio::test]
async fn test_idle_detection_skips_non_running() {
    let backend = Arc::new(MockBackend::new());
    let store = test_store().await;

    // Create completed session
    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "completed-session".into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        status: SessionStatus::Done,
        exit_code: Some(0),
        ..Default::default()
    };
    store.insert_session(&session).await.unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 1,
        action: IdleAction::Kill,
        threshold_secs: 60,
    };

    let dyn_backend: Arc<dyn Backend> = backend;
    check_idle_sessions(&dyn_backend, &store, &idle_config, &test_ready_ctx(), &[]).await;

    // Session should remain completed
    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, SessionStatus::Done);
}

#[tokio::test]
async fn test_idle_detection_capture_failure() {
    let backend = Arc::new(MockBackend::failing_capture());
    let store = test_store().await;
    create_running_session(&store, "cap-fail-idle").await;

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 1,
        action: IdleAction::Kill,
        threshold_secs: 60,
    };

    let dyn_backend: Arc<dyn Backend> = backend;
    check_idle_sessions(&dyn_backend, &store, &idle_config, &test_ready_ctx(), &[]).await;

    // Session should remain running — capture failed so idle check skipped
    let sessions = store.list_sessions().await.unwrap();
    assert_eq!(sessions[0].status, SessionStatus::Working);
}

#[tokio::test]
async fn test_idle_detection_not_yet_timed_out() {
    let backend = Arc::new(MockBackend::new());
    let store = test_store().await;

    // Create session with recent output
    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "recent-session".into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        status: SessionStatus::Working,
        backend_session_id: Some("recent-session".into()),
        output_snapshot: Some("test output".into()),
        last_output_at: Some(chrono::Utc::now()), // very recent
        ..Default::default()
    };
    store.insert_session(&session).await.unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 600,
        action: IdleAction::Alert,
        threshold_secs: 60,
    };

    let dyn_backend: Arc<dyn Backend> = backend;
    check_idle_sessions(&dyn_backend, &store, &idle_config, &test_ready_ctx(), &[]).await;

    // Should NOT be marked idle (not enough time elapsed)
    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert!(fetched.idle_since.is_none());
}

// The former `test_code_marker_transitions_active_session_to_ready_with_exit_code`
// tested `check_session_idle`'s own marker check, which ADR 0009 removed along with
// the rest of the `Ready` mechanism: `wrap_command` no longer keeps a fallback shell
// alive after the agent exits, so a session's backend is never "still alive with a
// marker already written" as a persistent, watchdog-visible state — that transition
// is now `SessionManager::resolve_dead_backend_session`'s job (backend confirmed
// dead), covered in `session/manager.rs`'s tests.

#[tokio::test]
async fn test_idle_detection_already_marked_stays() {
    let backend = Arc::new(MockBackend::new());
    let store = test_store().await;

    // Already idle, output still the same
    let idle_time = chrono::Utc::now() - chrono::Duration::seconds(100);
    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "already-idle".into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        status: SessionStatus::Waiting,
        backend_session_id: Some("already-idle".into()),
        output_snapshot: Some("test output".into()),
        last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
        idle_since: Some(idle_time),
        ..Default::default()
    };
    store.insert_session(&session).await.unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 600,
        action: IdleAction::Alert,
        threshold_secs: 60,
    };

    let dyn_backend: Arc<dyn Backend> = backend;
    check_idle_sessions(&dyn_backend, &store, &idle_config, &test_ready_ctx(), &[]).await;

    // idle_since should still be set and status stays Idle
    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert!(fetched.idle_since.is_some());
    assert_eq!(fetched.status, SessionStatus::Waiting);
}

#[tokio::test]
async fn test_idle_detection_kill_failure() {
    let backend = Arc::new(MockBackend::failing_kill());
    let store = test_store().await;

    // Session is already Idle with idle_since set — tests kill failure path
    // in handle_idle_session (Active sessions now transition to Idle first).
    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "kill-fail-idle".into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        status: SessionStatus::Waiting,
        backend_session_id: Some("kill-fail-idle".into()),
        output_snapshot: Some("test output".into()),
        last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
        idle_since: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
        ..Default::default()
    };
    store.insert_session(&session).await.unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 600,
        action: IdleAction::Kill,
        threshold_secs: 60,
    };

    let dyn_backend: Arc<dyn Backend> = backend;
    check_idle_sessions(&dyn_backend, &store, &idle_config, &test_ready_ctx(), &[]).await;

    // Session should remain Idle since kill failed
    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, SessionStatus::Waiting);
}

#[tokio::test]
async fn test_idle_detection_store_list_failure() {
    let backend = Arc::new(MockBackend::new());
    let store = test_store().await;

    // Drop sessions table so list fails
    sqlx::query("DROP TABLE sessions")
        .execute(store.pool())
        .await
        .unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 1,
        action: IdleAction::Kill,
        threshold_secs: 60,
    };

    let dyn_backend: Arc<dyn Backend> = backend;
    // Should not panic
    check_idle_sessions(&dyn_backend, &store, &idle_config, &test_ready_ctx(), &[]).await;
}

#[tokio::test]
async fn test_idle_detection_uses_created_at_when_no_last_output() {
    let backend = Arc::new(MockBackend::new());
    let store = test_store().await;

    // Session with no last_output_at but old created_at, output hasn't changed
    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "no-output-ts".into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        status: SessionStatus::Working,
        backend_session_id: Some("no-output-ts".into()),
        output_snapshot: Some("test output".into()),
        created_at: chrono::Utc::now() - chrono::Duration::seconds(700),
        ..Default::default()
    };
    store.insert_session(&session).await.unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 600,
        action: IdleAction::Alert,
        threshold_secs: 60,
    };

    let dyn_backend: Arc<dyn Backend> = backend;
    check_idle_sessions(&dyn_backend, &store, &idle_config, &test_ready_ctx(), &[]).await;

    // Should transition to Idle (created_at used as fallback for last_output_at)
    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, SessionStatus::Waiting);
}

#[tokio::test]
async fn test_idle_detection_snapshot_update_failure() {
    let backend = Arc::new(MockBackend::new());
    let store = test_store().await;
    create_running_session(&store, "snap-fail-idle").await;

    // Rename output_snapshot column to break the update
    sqlx::query("ALTER TABLE sessions RENAME COLUMN output_snapshot TO output_snapshot_old")
        .execute(store.pool())
        .await
        .unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 1,
        action: IdleAction::Kill,
        threshold_secs: 60,
    };

    let dyn_backend: Arc<dyn Backend> = backend;
    // Should not panic
    check_idle_sessions(&dyn_backend, &store, &idle_config, &test_ready_ctx(), &[]).await;
}

#[tokio::test]
async fn test_idle_detection_in_watchdog_loop() {
    // Test that idle detection runs inside the watchdog loop
    let backend = Arc::new(MockBackend::new());
    let store = test_store().await;

    // Create a session that will be detected as idle
    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "loop-idle".into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        status: SessionStatus::Working,
        backend_session_id: Some("loop-idle".into()),
        output_snapshot: Some("test output".into()),
        last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
        ..Default::default()
    };
    store.insert_session(&session).await.unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 600,
        action: IdleAction::Alert,
        threshold_secs: 60,
    };

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let backend_clone = backend.clone();
    let store_clone = store.clone();

    let handle = tokio::spawn(run_watchdog_loop(
        backend_clone,
        store_clone,
        make_config(Duration::from_millis(10), idle_config),
        shutdown_rx,
        test_ready_ctx(),
    ));

    // Poll until idle_since is set, with a generous timeout
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        if fetched.idle_since.is_some() {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "idle_since was not set within 2s"
        );
        time::sleep(Duration::from_millis(10)).await;
    }
    shutdown_tx.send(true).unwrap();
    handle.await.unwrap();
}

#[tokio::test]
async fn test_handle_active_session_clear_fails() {
    let store = test_store().await;

    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "clear-fail".into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        status: SessionStatus::Working,
        backend_session_id: Some("clear-fail".into()),
        output_snapshot: Some("test output".into()),
        last_output_at: Some(chrono::Utc::now()),
        idle_since: Some(chrono::Utc::now()),
        ..Default::default()
    };

    // Drop sessions table to make store operations fail
    sqlx::query("DROP TABLE sessions")
        .execute(store.pool())
        .await
        .unwrap();

    // Should not panic — logs warning and returns
    handle_active_session(&store, &session, &test_ready_ctx(), HarnessSignals::none()).await;
}

#[tokio::test]
async fn test_handle_active_session_not_idle() {
    let store = test_store().await;

    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "not-idle".into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        status: SessionStatus::Working,
        backend_session_id: Some("not-idle".into()),
        output_snapshot: Some("test output".into()),
        last_output_at: Some(chrono::Utc::now()),
        ..Default::default()
    };

    // idle_since is None — early return, no store call
    handle_active_session(&store, &session, &test_ready_ctx(), HarnessSignals::none()).await;
}

#[tokio::test]
async fn test_handle_active_session_skips_when_lifecycle_owned() {
    // Regression for the Idle→Active-on-output-change bug: a harness-owned session
    // (`signals.lifecycle`) must not be flipped back to Active, and its `idle_since`
    // must not be cleared, just because `handle_active_session` was called — only the
    // adapter's own hook events may do that once lifecycle is owned.
    let store = test_store().await;

    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "lifecycle-owned".into(),
        workdir: "/tmp/repo".into(),
        command: "claude -p fix".into(),
        status: SessionStatus::Waiting,
        backend_session_id: Some("lifecycle-owned".into()),
        output_snapshot: Some("test output".into()),
        last_output_at: Some(chrono::Utc::now()),
        idle_since: Some(chrono::Utc::now()),
        harness: Some("claude".into()),
        harness_last_event_at: Some(chrono::Utc::now()),
        ..Default::default()
    };
    store.insert_session(&session).await.unwrap();

    handle_active_session(&store, &session, &test_ready_ctx(), HarnessSignals::all()).await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, SessionStatus::Waiting);
    assert!(fetched.idle_since.is_some());
}

#[tokio::test]
async fn test_handle_active_session_status_update_failure_emits_no_event() {
    let store = test_store().await;

    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "idle-update-fail".into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        status: SessionStatus::Waiting,
        backend_session_id: Some("idle-update-fail".into()),
        output_snapshot: Some("test output".into()),
        last_output_at: Some(chrono::Utc::now()),
        idle_since: Some(chrono::Utc::now()),
        ..Default::default()
    };

    let (tx, mut rx) = broadcast::channel::<PulpoEvent>(16);
    let ctx = ReadyContext {
        event_tx: Some(tx),
        node_name: "test-node".into(),
    };

    sqlx::query("DROP TABLE sessions")
        .execute(store.pool())
        .await
        .unwrap();

    handle_active_session(&store, &session, &ctx, HarnessSignals::none()).await;
    assert!(matches!(
        rx.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
}

#[tokio::test]
async fn test_idle_transition_emits_sse_event() {
    let backend = Arc::new(MockBackend::new().with_output("Building...\nDo you trust this file?"));
    let store = test_store().await;

    let mut session = create_running_session(&store, "idle-sse").await;
    // Set output_snapshot to match mock output (unchanged → triggers idle check)
    let output = "Building...\nDo you trust this file?";
    store
        .update_session_output_snapshot(&session.id.to_string(), output)
        .await
        .unwrap();
    session.output_snapshot = Some(output.into());

    let (tx, mut rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
    let ctx = ReadyContext {
        event_tx: Some(tx),
        node_name: "test-node".into(),
    };

    let idle_config = IdleConfig {
        enabled: true,
        threshold_secs: 60,
        action: IdleAction::Alert,
        timeout_secs: 600,
    };

    let dyn_backend: Arc<dyn Backend> = backend;
    check_idle_sessions(&dyn_backend, &store, &idle_config, &ctx, &[]).await;

    // Session should be waiting (waiting pattern detected immediately)
    let updated = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.status, SessionStatus::Waiting);
    // A matched waiting-for-input pattern implies "blocked on the user".
    assert_eq!(
        updated.status_reason.as_deref(),
        Some("needs_input:permission")
    );

    // SSE event should have been emitted
    let event = rx.try_recv().expect("should receive idle SSE event");
    match event {
        PulpoEvent::Session(se) => {
            assert_eq!(se.status, "waiting");
            assert_eq!(se.status_reason.as_deref(), Some("needs_input:permission"));
            assert_eq!(se.needs_input.as_deref(), Some("permission"));
            assert_eq!(se.previous_status, Some("working".into()));
            assert_eq!(se.session_name, "idle-sse");
            assert!(se.output_snippet.is_some());
        }
        PulpoEvent::SessionDeleted(_)
        | PulpoEvent::UsageAlert(_)
        | PulpoEvent::Intervention(_)
        | PulpoEvent::Daemon(_) => {
            panic!("expected session event")
        }
    }
}

#[tokio::test]
async fn test_active_transition_emits_sse_event() {
    // Backend returns new output (different from stored snapshot)
    let backend = Arc::new(MockBackend::new().with_output("New output line"));
    let store = test_store().await;

    let mut session = create_running_session(&store, "active-sse").await;
    // Mark as Idle with stale snapshot
    store
        .update_session_status(&session.id.to_string(), SessionStatus::Waiting, None)
        .await
        .unwrap();
    session.status = SessionStatus::Waiting;
    session.output_snapshot = Some("Old output".into());
    session.idle_since = Some(chrono::Utc::now());

    let (tx, mut rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
    let ctx = ReadyContext {
        event_tx: Some(tx),
        node_name: "test-node".into(),
    };

    let idle_config = IdleConfig {
        enabled: true,
        threshold_secs: 60,
        action: IdleAction::Alert,
        timeout_secs: 600,
    };

    let dyn_backend: Arc<dyn Backend> = backend;
    check_idle_sessions(&dyn_backend, &store, &idle_config, &ctx, &[]).await;

    // Session should be active again
    let updated = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.status, SessionStatus::Working);

    // SSE event should have been emitted
    let event = rx.try_recv().expect("should receive active SSE event");
    match event {
        PulpoEvent::Session(se) => {
            assert_eq!(se.status, "working");
            assert_eq!(se.status_reason, None);
            assert_eq!(se.previous_status, Some("waiting".into()));
            assert_eq!(se.session_name, "active-sse");
        }
        PulpoEvent::SessionDeleted(_)
        | PulpoEvent::UsageAlert(_)
        | PulpoEvent::Intervention(_)
        | PulpoEvent::Daemon(_) => {
            panic!("expected session event")
        }
    }
}

#[tokio::test]
async fn test_handle_idle_session_alert_update_fails() {
    let backend = Arc::new(MockBackend::new());
    let store = test_store().await;

    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "alert-fail".into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        status: SessionStatus::Working,
        backend_session_id: Some("alert-fail".into()),
        output_snapshot: Some("test output".into()),
        last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
        ..Default::default()
    };

    // Drop sessions table to make store operations fail
    sqlx::query("DROP TABLE sessions")
        .execute(store.pool())
        .await
        .unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 600,
        action: IdleAction::Alert,
        threshold_secs: 60,
    };
    let now = chrono::Utc::now();
    let timeout = chrono::Duration::seconds(600);
    let dyn_backend: Arc<dyn Backend> = backend;

    // Should not panic — logs warning and returns
    handle_idle_session(
        &dyn_backend,
        &store,
        &idle_config,
        &session,
        now,
        timeout,
        &test_ready_ctx(),
    )
    .await;
}

#[tokio::test]
async fn test_handle_idle_session_kill_intervention_record_fails() {
    let backend = Arc::new(MockBackend::new());
    let store = test_store().await;

    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "kill-record-fail".into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        status: SessionStatus::Working,
        backend_session_id: Some("kill-record-fail".into()),
        output_snapshot: Some("test output".into()),
        last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
        ..Default::default()
    };

    // Drop sessions table to make store operations fail (kill succeeds, store fails)
    sqlx::query("DROP TABLE sessions")
        .execute(store.pool())
        .await
        .unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 600,
        action: IdleAction::Kill,
        threshold_secs: 60,
    };
    let now = chrono::Utc::now();
    let timeout = chrono::Duration::seconds(600);
    let dyn_backend: Arc<dyn Backend> = backend;

    // Should not panic — kill succeeds but store record fails
    handle_idle_session(
        &dyn_backend,
        &store,
        &idle_config,
        &session,
        now,
        timeout,
        &test_ready_ctx(),
    )
    .await;
}

#[tokio::test]
async fn test_check_session_idle_without_backend_session_id() {
    let backend = Arc::new(MockBackend::new());
    let store = test_store().await;

    // Session with backend_session_id = None (falls back to session name)
    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "no-tmux".into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        status: SessionStatus::Working,
        output_snapshot: Some("test output".into()),
        last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
        ..Default::default()
    };
    store.insert_session(&session).await.unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 600,
        action: IdleAction::Alert,
        threshold_secs: 60,
    };
    let now = chrono::Utc::now();
    let timeout = chrono::Duration::seconds(600);
    let dyn_backend: Arc<dyn Backend> = backend;

    // Should use session.name for backend calls
    check_session_idle(
        &dyn_backend,
        &store,
        &idle_config,
        &session,
        now,
        timeout,
        &test_ready_ctx(),
        &[],
    )
    .await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    // Active session with unchanged output > threshold_secs transitions to Idle
    assert_eq!(fetched.status, SessionStatus::Waiting);
}

// ───────────────────────────────────────────────────────────
// Harness watchdog bypass (spec §5): a session with `harness_last_event_at` set
// skips scrollback heuristics entirely — hook events own its state instead.
// ───────────────────────────────────────────────────────────

#[test]
fn test_harness_owns_state_true_when_last_event_at_set() {
    let session = Session {
        harness_last_event_at: Some(chrono::Utc::now()),
        ..Default::default()
    };
    assert!(harness_owns_state(&session));
}

#[test]
fn test_harness_owns_state_false_when_unset() {
    assert!(!harness_owns_state(&Session::default()));
}

#[tokio::test]
async fn test_check_session_idle_bypasses_waiting_pattern_and_time_based_idle() {
    // Output unchanged, well past the idle threshold, AND matches a waiting-pattern
    // — without the bypass this would immediately go Idle. With `harness_last_event_at`
    // set, hook events own the state: the session must stay Active.
    let backend = Arc::new(MockBackend::new().with_output("Do you want to proceed?"));
    let store = test_store().await;
    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "harness-bypass".into(),
        workdir: "/tmp/repo".into(),
        command: "claude -p fix".into(),
        status: SessionStatus::Working,
        backend_session_id: Some("harness-bypass".into()),
        output_snapshot: Some("Do you want to proceed?".into()),
        last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
        harness: Some("claude".into()),
        harness_last_event_at: Some(chrono::Utc::now()),
        ..Default::default()
    };
    store.insert_session(&session).await.unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 600,
        action: IdleAction::Alert,
        threshold_secs: 60,
    };
    let now = chrono::Utc::now();
    let timeout = chrono::Duration::seconds(600);
    let dyn_backend: Arc<dyn Backend> = backend;

    check_session_idle(
        &dyn_backend,
        &store,
        &idle_config,
        &session,
        now,
        timeout,
        &test_ready_ctx(),
        &[],
    )
    .await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fetched.status,
        SessionStatus::Working,
        "harness-owned session must not idle via scrollback heuristics"
    );
}

#[tokio::test]
async fn test_check_session_idle_bypasses_rate_limit_and_error_scraping() {
    let backend = Arc::new(MockBackend::new().with_output("Error: rate limit exceeded (429)"));
    let store = test_store().await;
    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "harness-bypass-scrape".into(),
        workdir: "/tmp/repo".into(),
        command: "claude -p fix".into(),
        status: SessionStatus::Working,
        backend_session_id: Some("harness-bypass-scrape".into()),
        harness: Some("claude".into()),
        harness_last_event_at: Some(chrono::Utc::now()),
        ..Default::default()
    };
    store.insert_session(&session).await.unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 600,
        action: IdleAction::Alert,
        threshold_secs: 60,
    };
    let now = chrono::Utc::now();
    let timeout = chrono::Duration::seconds(600);
    let dyn_backend: Arc<dyn Backend> = backend;

    check_session_idle(
        &dyn_backend,
        &store,
        &idle_config,
        &session,
        now,
        timeout,
        &test_ready_ctx(),
        &[],
    )
    .await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fetched.meta_str(pulpo_common::session::meta::RATE_LIMIT),
        None
    );
    assert_eq!(
        fetched.meta_str(pulpo_common::session::meta::ERROR_STATUS),
        None
    );
}

#[tokio::test]
async fn test_check_session_idle_output_change_does_not_revert_owned_idle_status() {
    // Regression: a hook-driven Idle/needs_input must not be reverted by the very
    // next watchdog tick just because the TUI repainted (the tmux capture differs
    // from the stored `output_snapshot`) — only the adapter's own events may move a
    // harness-owned session's status.
    let backend = Arc::new(MockBackend::new().with_output("repainted prompt"));
    let store = test_store().await;
    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "harness-idle-repaint".into(),
        workdir: "/tmp/repo".into(),
        command: "claude -p fix".into(),
        status: SessionStatus::Waiting,
        backend_session_id: Some("harness-idle-repaint".into()),
        output_snapshot: Some("stale prompt".into()),
        harness: Some("claude".into()),
        harness_last_event_at: Some(chrono::Utc::now()),
        idle_since: Some(chrono::Utc::now()),
        ..Default::default()
    };
    store.insert_session(&session).await.unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 600,
        action: IdleAction::Alert,
        threshold_secs: 60,
    };
    let now = chrono::Utc::now();
    let timeout = chrono::Duration::seconds(600);
    let dyn_backend: Arc<dyn Backend> = backend;

    check_session_idle(
        &dyn_backend,
        &store,
        &idle_config,
        &session,
        now,
        timeout,
        &test_ready_ctx(),
        &[],
    )
    .await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fetched.status,
        SessionStatus::Waiting,
        "harness-owned Idle session must stay Idle across an output-only change"
    );
    assert!(
        fetched.idle_since.is_some(),
        "idle_since must not be cleared for a harness-owned session"
    );
}

#[tokio::test]
async fn test_check_session_idle_codex_harness_keeps_rate_limit_scraping() {
    // Codex's `owned_signals()` is `lifecycle_only()` (no error/rate-limit hook) —
    // unlike the all-signals Claude adapter above, a Codex session with
    // `harness_last_event_at` set must still have `detect_rate_limit` scrape its
    // output, proving `lifecycle_only()` actually keeps that heuristic running
    // rather than the bypass silently covering every harness the same way.
    let backend = Arc::new(MockBackend::new().with_output("Error: rate limit exceeded (429)"));
    let store = test_store().await;
    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "codex-harness-rate-limit".into(),
        workdir: "/tmp/repo".into(),
        command: "codex".into(),
        status: SessionStatus::Working,
        backend_session_id: Some("codex-harness-rate-limit".into()),
        harness: Some("codex".into()),
        harness_last_event_at: Some(chrono::Utc::now()),
        ..Default::default()
    };
    store.insert_session(&session).await.unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 600,
        action: IdleAction::Alert,
        threshold_secs: 60,
    };
    let now = chrono::Utc::now();
    let timeout = chrono::Duration::seconds(600);
    let dyn_backend: Arc<dyn Backend> = backend;

    check_session_idle(
        &dyn_backend,
        &store,
        &idle_config,
        &session,
        now,
        timeout,
        &test_ready_ctx(),
        &[],
    )
    .await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert!(
        fetched
            .meta_str(pulpo_common::session::meta::RATE_LIMIT)
            .is_some(),
        "codex's lifecycle_only() signals must leave rate-limit scraping active"
    );
}

#[tokio::test]
async fn test_check_session_idle_without_harness_events_keeps_scraping() {
    // Regression guard: a session with no `harness_last_event_at` (generic harness,
    // or hooks never fired) must keep today's scrollback heuristics unchanged.
    let backend = Arc::new(MockBackend::new().with_output("Error: rate limit exceeded (429)"));
    let store = test_store().await;
    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "no-harness-events".into(),
        workdir: "/tmp/repo".into(),
        command: "claude -p fix".into(),
        status: SessionStatus::Working,
        backend_session_id: Some("no-harness-events".into()),
        ..Default::default()
    };
    store.insert_session(&session).await.unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 600,
        action: IdleAction::Alert,
        threshold_secs: 60,
    };
    let now = chrono::Utc::now();
    let timeout = chrono::Duration::seconds(600);
    let dyn_backend: Arc<dyn Backend> = backend;

    check_session_idle(
        &dyn_backend,
        &store,
        &idle_config,
        &session,
        now,
        timeout,
        &test_ready_ctx(),
        &[],
    )
    .await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert!(
        fetched
            .meta_str(pulpo_common::session::meta::RATE_LIMIT)
            .is_some()
    );
}

// ───────────────────────────────────────────────────────────
// Stale / dead edge-case tests
// ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_idle_kill_succeeds_but_session_disappears() {
    // Edge case: backend kill succeeds, but the session was deleted from
    // the DB between the list and the intervention. The store update should
    // fail gracefully (warn, not panic).
    let backend = Arc::new(MockBackend::new());
    let store = test_store().await;

    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "vanishing".into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        status: SessionStatus::Working,
        backend_session_id: Some("vanishing".into()),
        output_snapshot: Some("test output".into()),
        last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
        ..Default::default()
    };

    // Don't insert — simulate the session vanishing between list and kill.
    // handle_idle_session gets a session struct but the DB no longer has it.
    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 600,
        action: IdleAction::Kill,
        threshold_secs: 60,
    };
    let now = chrono::Utc::now();
    let timeout = chrono::Duration::seconds(600);
    let dyn_backend: Arc<dyn Backend> = backend;

    // Should not panic — kill succeeds, store update warns
    handle_idle_session(
        &dyn_backend,
        &store,
        &idle_config,
        &session,
        now,
        timeout,
        &test_ready_ctx(),
    )
    .await;
}

#[tokio::test]
async fn test_watchdog_runtime_config_debug() {
    let cfg = WatchdogRuntimeConfig {
        interval: Duration::from_secs(10),
        idle: IdleConfig::default(),
        extra_waiting_patterns: Vec::new(),
    };
    let debug = format!("{cfg:?}");
    assert!(debug.contains("interval"));
}

#[tokio::test]
async fn test_watchdog_runtime_config_clone() {
    let cfg = WatchdogRuntimeConfig {
        interval: Duration::from_secs(5),
        idle: IdleConfig {
            enabled: true,
            timeout_secs: 300,
            action: IdleAction::Kill,
            threshold_secs: 60,
        },
        extra_waiting_patterns: Vec::new(),
    };
    #[allow(clippy::redundant_clone)]
    let cloned = cfg.clone();
    assert_eq!(cloned.interval, Duration::from_secs(5));
    assert!(cloned.idle.enabled);
    assert_eq!(cloned.idle.timeout_secs, 300);
    assert_eq!(cloned.idle.action, IdleAction::Kill);
}

#[test]
fn test_detect_waiting_for_input_basic() {
    // Returns true for "Do you trust this file?"
    assert!(detect_waiting_for_input(
        "Some output\nDo you trust this file?",
        &[],
    ));

    // Returns true for "[Y/n]"
    assert!(detect_waiting_for_input("Continue? [Y/n]", &[]));

    // Returns false for regular output
    assert!(!detect_waiting_for_input(
        "Building project...\nCompilation succeeded.",
        &[],
    ));

    // Only checks last 5 lines — pattern on line 1 of 7 is out of range
    let output = "Do you trust this file?\nline2\nline3\nline4\nline5\nline6\nline7";
    assert!(!detect_waiting_for_input(output, &[]));

    // Pattern within last 5 lines should still match
    let output = "line1\nline2\nline3\nDo you trust this file?\nline5";
    assert!(detect_waiting_for_input(output, &[]));
}

#[test]
fn test_detect_waiting_for_input_case_insensitive() {
    assert!(detect_waiting_for_input("DO YOU TRUST THIS FILE?", &[]));
    assert!(detect_waiting_for_input("do you trust this file?", &[]));
    assert!(detect_waiting_for_input("press enter to continue", &[]));
    assert!(detect_waiting_for_input("PRESS ENTER", &[]));
    assert!(detect_waiting_for_input("Approve This action", &[]));
}

#[test]
fn test_detect_waiting_claude_code() {
    assert!(detect_waiting_for_input("Some output\n(Y)es / (N)o\n", &[]));
    assert!(detect_waiting_for_input("(A)lways allow\n", &[]));
    assert!(detect_waiting_for_input("Do you want to proceed?\n", &[]));
}

#[test]
fn test_detect_waiting_claude_trust_dialog() {
    // Shown before Claude's SessionStart hook fires, so only scrollback can catch it.
    let output = "Quick safety check: Is this a project you created or one you trust?\n\
                  \u{276f} No, exit\n  Yes, I trust this folder\n  Enter to confirm \u{b7} Esc to cancel";
    assert!(detect_waiting_for_input(output, &[]));
}

#[test]
fn test_detect_waiting_extra_patterns() {
    let extras = vec!["custom prompt>".to_string()];
    assert!(detect_waiting_for_input(
        "custom prompt> waiting\n",
        &extras
    ));
    assert!(!detect_waiting_for_input("normal output\n", &extras));
}

#[test]
fn test_detect_waiting_aider_patterns() {
    assert!(detect_waiting_for_input("Add foo.py to the chat?\n", &[]));
    assert!(detect_waiting_for_input("Apply edit?\n", &[]));
    assert!(detect_waiting_for_input("Run shell command?\n", &[]));
    assert!(detect_waiting_for_input("Create new file bar.rs?\n", &[]));
}

#[test]
fn test_detect_waiting_generic_patterns() {
    assert!(detect_waiting_for_input("Continue?\n", &[]));
    assert!(detect_waiting_for_input("Are you sure (y/n)?\n", &[]));
    assert!(detect_waiting_for_input("user@host's password:\n", &[]));
    assert!(detect_waiting_for_input("[sudo] password for user:\n", &[]));
}

#[test]
fn test_detect_waiting_gemini_patterns() {
    assert!(detect_waiting_for_input("Approve? (y/n/always) ->\n", &[]));
    assert!(detect_waiting_for_input("Allow?\n", &[]));
}

#[test]
fn test_detect_waiting_codex_patterns() {
    assert!(detect_waiting_for_input("Allow command?\n", &[]));
}

#[tokio::test]
async fn test_idle_transition_active_to_idle() {
    // Mock backend returns output containing a waiting pattern
    let backend = Arc::new(MockBackend::new().with_output("Building...\nDo you trust this file?"));
    let store = test_store().await;

    // Create an Active session whose output_snapshot matches mock output
    // (so output_changed == false, triggering the waiting-for-input check)
    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "active-to-idle".into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        status: SessionStatus::Working,
        backend_session_id: Some("active-to-idle".into()),
        output_snapshot: Some("Building...\nDo you trust this file?".into()),
        last_output_at: Some(chrono::Utc::now()),
        ..Default::default()
    };
    store.insert_session(&session).await.unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 600,
        action: IdleAction::Alert,
        threshold_secs: 60,
    };

    let dyn_backend: Arc<dyn Backend> = backend;
    check_idle_sessions(&dyn_backend, &store, &idle_config, &test_ready_ctx(), &[]).await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, SessionStatus::Waiting);
}

#[tokio::test]
async fn test_idle_transition_idle_to_active() {
    // Mock backend returns "new output" — different from the stored snapshot
    let backend = Arc::new(MockBackend::new().with_output("new output from agent"));
    let store = test_store().await;

    // Create an Idle session with a different output_snapshot
    let session = Session {
        id: uuid::Uuid::new_v4(),
        name: "idle-to-active".into(),
        workdir: "/tmp/repo".into(),
        command: "echo hello".into(),
        description: Some("test".into()),
        status: SessionStatus::Waiting,
        backend_session_id: Some("idle-to-active".into()),
        output_snapshot: Some("old stale output".into()),
        last_output_at: Some(chrono::Utc::now()),
        idle_since: Some(chrono::Utc::now() - chrono::Duration::seconds(60)),
        ..Default::default()
    };
    store.insert_session(&session).await.unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 600,
        action: IdleAction::Alert,
        threshold_secs: 60,
    };

    let dyn_backend: Arc<dyn Backend> = backend;
    check_idle_sessions(&dyn_backend, &store, &idle_config, &test_ready_ctx(), &[]).await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, SessionStatus::Working);
    assert!(fetched.idle_since.is_none());
}

fn make_idle_test_session(
    name: &str,
    status: SessionStatus,
    idle_since: Option<chrono::DateTime<chrono::Utc>>,
) -> Session {
    Session {
        id: uuid::Uuid::new_v4(),
        name: name.into(),
        workdir: "/tmp/repo".into(),
        command: "echo test".into(),
        status,
        backend_session_id: Some(name.into()),
        output_snapshot: Some("unchanged output".into()),
        last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
        idle_since,
        ..Default::default()
    }
}

#[tokio::test]
async fn test_idle_check_includes_idle_sessions() {
    let backend = Arc::new(MockBackend::new().with_output("unchanged output"));
    let store = test_store().await;

    let active_session = make_idle_test_session("active-one", SessionStatus::Working, None);
    let idle_since = Some(chrono::Utc::now() - chrono::Duration::seconds(100));
    let idle_session = make_idle_test_session("idle-one", SessionStatus::Waiting, idle_since);
    let dead_session = make_idle_test_session("dead-one", SessionStatus::Done, None);

    store.insert_session(&active_session).await.unwrap();
    store.insert_session(&idle_session).await.unwrap();
    store.insert_session(&dead_session).await.unwrap();

    let idle_config = IdleConfig {
        enabled: true,
        timeout_secs: 600,
        action: IdleAction::Alert,
        threshold_secs: 60,
    };

    let dyn_backend: Arc<dyn Backend> = backend.clone();
    check_idle_sessions(&dyn_backend, &store, &idle_config, &test_ready_ctx(), &[]).await;

    // Both Active and Idle sessions should have been processed
    let capture_count = backend.capture_calls.lock().unwrap().len();
    assert_eq!(capture_count, 2);

    let fetched_active = store
        .get_session(&active_session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    // Active session with unchanged output > 20s transitions to Idle
    assert_eq!(fetched_active.status, SessionStatus::Waiting);

    let fetched_idle = store
        .get_session(&idle_session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched_idle.status, SessionStatus::Waiting);

    // Dead session should NOT have been processed
    assert!(
        !backend
            .capture_calls
            .lock()
            .unwrap()
            .contains(&"dead-one".to_string())
    );
}

// ADR 0009 removed the Ready-specific mechanism this block used to test
// (`detect_agent_exited`, `handle_session_ready`, `sweep_ready_exit_code`) —
// `wrap_command` no longer keeps a fallback shell alive after the agent exits,
// so there is no more scrollback-detectable/backend-still-alive "done" window to
// sweep. The marker-driven exit-code recording and Done/Lost classification this
// used to cover are now tested against `SessionManager::resolve_dead_backend_session`
// / `apply_harness_event` in `session/manager.rs`.

// -- detect_and_store_output_metadata tests --

#[tokio::test]
async fn test_detect_and_store_pr_url() {
    let store = test_store().await;
    let session = create_running_session(&store, "pr-detect").await;

    let output = "Pushing...\nremote: Create a pull request:\nremote:   https://github.com/owner/repo/pull/42\n";
    detect_and_store_output_metadata(&store, &session, output, None, HarnessSignals::none()).await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    let meta = fetched.metadata.unwrap();
    assert_eq!(
        meta.get("pr_url").unwrap(),
        "https://github.com/owner/repo/pull/42"
    );
}

#[tokio::test]
async fn test_detect_and_store_branch() {
    let store = test_store().await;
    let session = create_running_session(&store, "branch-detect").await;

    let output = "To github.com:owner/repo.git\n * [new branch]      feature/x -> feature/x\n";
    detect_and_store_output_metadata(&store, &session, output, None, HarnessSignals::none()).await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    let meta = fetched.metadata.unwrap();
    assert_eq!(meta.get("branch").unwrap(), "feature/x");
}

#[tokio::test]
async fn test_detect_skips_if_already_stored() {
    let store = test_store().await;
    let session = create_running_session(&store, "already-stored").await;

    // Pre-set pr_url in metadata
    store
        .update_session_metadata_field(&session.id.to_string(), "pr_url", "https://old")
        .await
        .unwrap();

    // Re-fetch the session to get updated metadata
    let session = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();

    let output = "https://github.com/owner/repo/pull/99\n";
    detect_and_store_output_metadata(&store, &session, output, None, HarnessSignals::none()).await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    let meta = fetched.metadata.unwrap();
    // Should keep old value, not overwrite
    assert_eq!(meta.get("pr_url").unwrap(), "https://old");
}

#[tokio::test]
async fn test_detect_no_match() {
    let store = test_store().await;
    let session = create_running_session(&store, "no-match").await;

    let output = "$ cargo test\nrunning tests...\nall passed\n";
    detect_and_store_output_metadata(&store, &session, output, None, HarnessSignals::none()).await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    assert!(fetched.metadata.is_none());
}

#[tokio::test]
async fn test_detect_both_pr_and_branch() {
    let store = test_store().await;
    let session = create_running_session(&store, "both-detect").await;

    let output = "remote: Create a pull request for 'feat/x' on GitHub:\nremote:   https://github.com/owner/repo/pull/5\n";
    detect_and_store_output_metadata(&store, &session, output, None, HarnessSignals::none()).await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    let meta = fetched.metadata.unwrap();
    assert_eq!(
        meta.get("pr_url").unwrap(),
        "https://github.com/owner/repo/pull/5"
    );
    assert_eq!(meta.get("branch").unwrap(), "feat/x");
}

#[tokio::test]
async fn test_detect_and_store_rate_limit() {
    let store = test_store().await;
    let session = create_running_session(&store, "rate-limit-detect").await;

    let output = "Working...\nError: Rate limit exceeded. Please wait.\n";
    detect_and_store_output_metadata(&store, &session, output, None, HarnessSignals::none()).await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    let meta = fetched.metadata.unwrap();
    assert_eq!(meta.get("rate_limit").unwrap(), "Rate limited");
    assert!(meta.contains_key("rate_limit_at"));
}

#[tokio::test]
async fn test_detect_rate_limit_updates_on_every_tick() {
    let store = test_store().await;
    let session = create_running_session(&store, "rate-limit-update").await;

    // First detection
    let output1 = "Error: too many requests\n";
    detect_and_store_output_metadata(&store, &session, output1, None, HarnessSignals::none()).await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    let meta = fetched.metadata.unwrap();
    assert_eq!(
        meta.get("rate_limit").unwrap(),
        "Rate limited: too many requests"
    );
    let first_ts = meta.get("rate_limit_at").unwrap().clone();

    // Second detection with different message — should update
    let output2 = "RESOURCE_EXHAUSTED: quota used up\n";
    // Re-fetch session with updated metadata
    let session2 = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    detect_and_store_output_metadata(&store, &session2, output2, None, HarnessSignals::none())
        .await;

    let fetched2 = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    let meta2 = fetched2.metadata.unwrap();
    assert_eq!(
        meta2.get("rate_limit").unwrap(),
        "Rate limited: resource exhausted"
    );
    // Timestamp should have been updated
    let second_ts = meta2.get("rate_limit_at").unwrap();
    assert!(second_ts >= &first_ts);
}

#[tokio::test]
async fn test_detect_no_rate_limit() {
    let store = test_store().await;
    let session = create_running_session(&store, "no-rate-limit").await;

    let output = "$ cargo test\nrunning tests...\nall passed\n";
    detect_and_store_output_metadata(&store, &session, output, None, HarnessSignals::none()).await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    // No metadata should be set
    assert!(fetched.metadata.is_none());
}

#[tokio::test]
async fn test_rate_limit_not_cleared_after_recovery() {
    let store = test_store().await;
    let session = create_running_session(&store, "rate-recover").await;

    // First: detect rate limit
    let output1 = "Error: Rate limit exceeded\n";
    detect_and_store_output_metadata(&store, &session, output1, None, HarnessSignals::none()).await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    let meta = fetched.metadata.as_ref().unwrap();
    assert!(meta.contains_key("rate_limit"));

    // Second: output without rate limit — rate_limit key should persist
    // (detect_and_store_output_metadata only writes, never deletes)
    let output2 = "Working normally again...\n";
    let session2 = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    detect_and_store_output_metadata(&store, &session2, output2, None, HarnessSignals::none())
        .await;

    let fetched2 = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    let meta2 = fetched2.metadata.unwrap();
    // rate_limit key still present — it was NOT cleared
    assert!(
        meta2.contains_key("rate_limit"),
        "rate_limit should persist after recovery (by design)"
    );
}

#[tokio::test]
async fn test_detect_gitlab_mr_in_output_metadata() {
    let store = test_store().await;
    let session = create_running_session(&store, "gitlab-detect").await;

    let output = "Created: https://gitlab.com/group/project/-/merge_requests/42\n";
    detect_and_store_output_metadata(&store, &session, output, None, HarnessSignals::none()).await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    let meta = fetched.metadata.unwrap();
    assert_eq!(
        meta.get("pr_url").unwrap(),
        "https://gitlab.com/group/project/-/merge_requests/42"
    );
}

#[tokio::test]
async fn test_detect_bitbucket_pr_in_output_metadata() {
    let store = test_store().await;
    let session = create_running_session(&store, "bitbucket-detect").await;

    let output = "PR: https://bitbucket.org/owner/repo/pull-requests/7\n";
    detect_and_store_output_metadata(&store, &session, output, None, HarnessSignals::none()).await;

    let fetched = store
        .get_session(&session.id.to_string())
        .await
        .unwrap()
        .unwrap();
    let meta = fetched.metadata.unwrap();
    assert_eq!(
        meta.get("pr_url").unwrap(),
        "https://bitbucket.org/owner/repo/pull-requests/7"
    );
}
