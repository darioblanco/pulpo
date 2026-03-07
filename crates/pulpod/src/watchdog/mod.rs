pub mod memory;

use std::sync::Arc;
use std::time::Duration;

use memory::{MemoryReader, MemorySnapshot};
use pulpo_common::session::SessionStatus;
use tracing::{debug, info, warn};

use crate::backend::Backend;
use crate::store::Store;

/// Action to take when a session is detected as idle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleAction {
    Alert,
    Kill,
}

/// Configuration for idle session detection.
#[derive(Debug, Clone)]
pub struct IdleConfig {
    pub enabled: bool,
    pub timeout_secs: u64,
    pub action: IdleAction,
}

impl Default for IdleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            timeout_secs: 600,
            action: IdleAction::Alert,
        }
    }
}

/// Runs the watchdog loop that monitors system memory and intervenes when sustained pressure
/// is detected. Kills running sessions after `breach_count` consecutive checks above `threshold`.
pub async fn run_watchdog_loop(
    backend: Arc<dyn Backend>,
    store: Store,
    reader: Box<dyn MemoryReader>,
    threshold: u8,
    interval: Duration,
    breach_count: u32,
    idle: IdleConfig,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    let mut tick = tokio::time::interval(interval);
    tick.tick().await; // first tick completes immediately
    let mut consecutive_breaches: u32 = 0;

    loop {
        tokio::select! {
            _ = tick.tick() => {
                match reader.read_memory() {
                    Ok(snapshot) => {
                        let usage = snapshot.usage_percent();
                        debug!(usage, threshold, consecutive_breaches, "Memory check");

                        if usage >= threshold {
                            consecutive_breaches += 1;
                            warn!(
                                usage,
                                threshold,
                                consecutive_breaches,
                                breach_count,
                                available_mb = snapshot.available_mb,
                                total_mb = snapshot.total_mb,
                                "Memory pressure detected"
                            );

                            if consecutive_breaches >= breach_count {
                                intervene(&backend, &store, &snapshot).await;
                                consecutive_breaches = 0;
                            }
                        } else {
                            if consecutive_breaches > 0 {
                                info!(
                                    usage,
                                    threshold,
                                    "Memory pressure subsided, resetting breach counter"
                                );
                            }
                            consecutive_breaches = 0;
                        }
                    }
                    Err(e) => {
                        warn!("Failed to read memory: {e}");
                    }
                }

                // Idle detection runs on every tick, independent of memory checks
                if idle.enabled {
                    check_idle_sessions(&backend, &store, &idle).await;
                }
            }
            _ = shutdown_rx.changed() => {
                info!("Watchdog shutting down");
                break;
            }
        }
    }
}

async fn intervene(backend: &Arc<dyn Backend>, store: &Store, snapshot: &MemorySnapshot) {
    let sessions = match store.list_sessions().await {
        Ok(s) => s,
        Err(e) => {
            warn!("Watchdog: failed to list sessions: {e}");
            return;
        }
    };

    let running: Vec<_> = sessions
        .into_iter()
        .filter(|s| s.status == SessionStatus::Running)
        .collect();

    if running.is_empty() {
        let usage = snapshot.usage_percent();
        warn!(
            usage,
            "Memory pressure but no running sessions to intervene on"
        );
        return;
    }

    for session in &running {
        // Capture output before killing
        match backend.capture_output(&session.name, 500) {
            Ok(output) => {
                if let Err(e) = store
                    .update_session_output_snapshot(&session.id.to_string(), &output)
                    .await
                {
                    warn!(
                        session_id = %session.id,
                        session_name = %session.name,
                        "Failed to save output snapshot: {e}"
                    );
                }
            }
            Err(e) => {
                warn!(
                    session_id = %session.id,
                    session_name = %session.name,
                    "Failed to capture output before intervention: {e}"
                );
            }
        }

        // Kill the session — only mark dead if kill succeeds
        if let Err(e) = backend.kill_session(&session.name) {
            warn!(
                session_id = %session.id,
                session_name = %session.name,
                "Failed to kill session during intervention (session still alive): {e}"
            );
            continue;
        }

        // Record intervention (only reached on successful kill)
        let reason = format!(
            "Memory usage {}% ({}/{}MB available)",
            snapshot.usage_percent(),
            snapshot.available_mb,
            snapshot.total_mb
        );
        if let Err(e) = store
            .update_session_intervention(&session.id.to_string(), &reason)
            .await
        {
            warn!(
                session_id = %session.id,
                session_name = %session.name,
                "Failed to record intervention: {e}"
            );
        }
        let usage = snapshot.usage_percent();
        warn!(
            session_id = %session.id,
            session_name = %session.name,
            usage,
            available_mb = snapshot.available_mb,
            total_mb = snapshot.total_mb,
            "Watchdog intervention: killed session due to memory pressure"
        );
    }
}

async fn check_idle_sessions(backend: &Arc<dyn Backend>, store: &Store, idle_config: &IdleConfig) {
    let sessions = match store.list_sessions().await {
        Ok(s) => s,
        Err(e) => {
            warn!("Idle check: failed to list sessions: {e}");
            return;
        }
    };

    let running: Vec<_> = sessions
        .into_iter()
        .filter(|s| s.status == SessionStatus::Running)
        .collect();

    let now = chrono::Utc::now();
    let timeout =
        chrono::Duration::seconds(idle_config.timeout_secs.try_into().unwrap_or(i64::MAX));

    for session in &running {
        check_session_idle(backend, store, idle_config, session, now, timeout).await;
    }
}

/// Patterns that indicate the agent is waiting for user input.
const WAITING_PATTERNS: &[&str] = &[
    "Do you trust",
    "Yes / No",
    "(y/n)",
    "Press Enter",
    "[Y/n]",
    "[yes/no]",
    "(yes/no)",
    "? [Y/n]",
    "? (y/N)",
    "approve this",
];

/// Check if the terminal output suggests the agent is waiting for user input.
/// Inspects the last 5 lines of output for known prompt patterns.
pub fn detect_waiting_for_input(output: &str) -> bool {
    let last_lines: Vec<&str> = output.lines().rev().take(5).collect();
    for line in &last_lines {
        let lower = line.to_lowercase();
        for pattern in WAITING_PATTERNS {
            if lower.contains(&pattern.to_lowercase()) {
                return true;
            }
        }
    }
    false
}

async fn check_session_idle(
    backend: &Arc<dyn Backend>,
    store: &Store,
    idle_config: &IdleConfig,
    session: &pulpo_common::session::Session,
    now: chrono::DateTime<chrono::Utc>,
    timeout: chrono::Duration,
) {
    // Capture current output to track activity
    let current_output = match backend.capture_output(&session.name, 500) {
        Ok(o) => o,
        Err(e) => {
            debug!(
                "Idle check: failed to capture output for {}: {e}",
                session.name
            );
            return;
        }
    };

    // Update snapshot (conditionally sets last_output_at if content changed)
    if let Err(e) = store
        .update_session_output_snapshot(&session.id.to_string(), &current_output)
        .await
    {
        warn!(
            "Idle check: failed to update output snapshot for {}: {e}",
            session.name
        );
        return;
    }

    // Check if agent is waiting for user input
    let waiting = detect_waiting_for_input(&current_output);
    if waiting != session.waiting_for_input {
        if let Err(e) = store
            .update_session_waiting_for_input(&session.id.to_string(), waiting)
            .await
        {
            warn!(
                "Idle check: failed to update waiting_for_input for {}: {e}",
                session.name
            );
        } else if waiting {
            info!(
                "Session {} appears to be waiting for user input",
                session.name
            );
        }
    }

    // Determine if output changed since last check
    let output_changed = session.output_snapshot.as_deref() != Some(current_output.as_str());

    if output_changed {
        handle_active_session(store, session).await;
    } else {
        handle_idle_session(
            backend,
            store,
            idle_config,
            session,
            &session.name,
            now,
            timeout,
        )
        .await;
    }
}

async fn handle_active_session(store: &Store, session: &pulpo_common::session::Session) {
    if session.idle_since.is_none() {
        return;
    }
    info!(
        "Idle check: session {} active again, clearing idle status",
        session.name
    );
    if let Err(e) = store
        .clear_session_idle_since(&session.id.to_string())
        .await
    {
        warn!(
            "Idle check: failed to clear idle_since for {}: {e}",
            session.name
        );
    }
}

async fn handle_idle_session(
    backend: &Arc<dyn Backend>,
    store: &Store,
    idle_config: &IdleConfig,
    session: &pulpo_common::session::Session,
    session_name: &str,
    now: chrono::DateTime<chrono::Utc>,
    timeout: chrono::Duration,
) {
    let last_activity = session.last_output_at.unwrap_or(session.created_at);
    let idle_duration = now - last_activity;

    if idle_duration <= timeout {
        return;
    }

    let minutes = idle_duration.num_minutes();

    match idle_config.action {
        IdleAction::Alert => {
            if session.idle_since.is_none() {
                warn!(
                    "Idle check: session {} idle for {minutes} minutes, marking as idle",
                    session.name
                );
                if let Err(e) = store
                    .update_session_idle_since(&session.id.to_string())
                    .await
                {
                    warn!(
                        "Idle check: failed to set idle_since for {}: {e}",
                        session.name
                    );
                }
            }
        }
        IdleAction::Kill => {
            let reason = format!("Idle for {minutes} minutes");

            if let Err(e) = backend.kill_session(session_name) {
                warn!(
                    "Idle check: failed to kill idle session {}: {e}",
                    session.name
                );
                return;
            }

            if let Err(e) = store
                .update_session_intervention(&session.id.to_string(), &reason)
                .await
            {
                warn!(
                    "Idle check: failed to record intervention for {}: {e}",
                    session.name
                );
            }
            warn!(
                "Idle check: killed idle session {} after {minutes} minutes",
                session.name
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Backend;
    use anyhow::Result;
    use pulpo_common::session::*;
    use std::sync::Mutex;
    use tokio::time;

    struct MockMemoryReader {
        snapshots: Mutex<Vec<MemorySnapshot>>,
    }

    impl MockMemoryReader {
        fn new(snapshots: Vec<MemorySnapshot>) -> Self {
            Self {
                snapshots: Mutex::new(snapshots),
            }
        }
    }

    impl MemoryReader for MockMemoryReader {
        fn read_memory(&self) -> Result<MemorySnapshot> {
            let mut snapshots = self.snapshots.lock().unwrap();
            if snapshots.is_empty() {
                // Default: low usage
                Ok(MemorySnapshot {
                    available_mb: 4096,
                    total_mb: 8192,
                })
            } else {
                Ok(snapshots.remove(0))
            }
        }
    }

    struct ErrorMemoryReader;

    impl MemoryReader for ErrorMemoryReader {
        fn read_memory(&self) -> Result<MemorySnapshot> {
            anyhow::bail!("sensor failure")
        }
    }

    struct MockBackend {
        kill_calls: Mutex<Vec<String>>,
        capture_calls: Mutex<Vec<String>>,
        create_calls: Mutex<Vec<String>>,
        create_commands: Mutex<Vec<String>>,
        output: String,
        fail_capture: bool,
        fail_kill: bool,
        fail_create: bool,
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
            Ok(true)
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

    async fn test_store() -> Store {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        store
    }

    fn make_running_session(name: &str) -> Session {
        Session {
            id: uuid::Uuid::new_v4(),
            name: name.into(),
            workdir: "/tmp/repo".into(),
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            backend_session_id: Some(format!("pulpo-{name}")),
            output_snapshot: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    async fn create_running_session(store: &Store, name: &str) -> Session {
        let session = Session {
            id: uuid::Uuid::new_v4(),
            name: name.into(),
            workdir: "/tmp/repo".into(),
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            backend_session_id: Some(format!("pulpo-{name}")),
            output_snapshot: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        store.insert_session(&session).await.unwrap();
        session
    }

    #[tokio::test]
    async fn test_watchdog_shutdown() {
        let backend = Arc::new(MockBackend::new());
        let store = test_store().await;
        let reader = MockMemoryReader::new(vec![]);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        let handle = tokio::spawn(run_watchdog_loop(
            backend,
            store,
            Box::new(reader),
            90,
            Duration::from_millis(10),
            3,
            IdleConfig {
                enabled: false,
                ..IdleConfig::default()
            },
            shutdown_rx,
        ));

        // Let it run briefly then shutdown
        time::sleep(Duration::from_millis(50)).await;
        shutdown_tx.send(true).unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_watchdog_below_threshold_no_intervention() {
        let backend = Arc::new(MockBackend::new());
        let store = test_store().await;
        create_running_session(&store, "safe-session").await;

        // All readings below threshold
        let reader = MockMemoryReader::new(vec![
            MemorySnapshot {
                available_mb: 2048,
                total_mb: 8192,
            },
            MemorySnapshot {
                available_mb: 2048,
                total_mb: 8192,
            },
        ]);

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let backend_clone = backend.clone();

        let handle = tokio::spawn(run_watchdog_loop(
            backend_clone,
            store,
            Box::new(reader),
            90,
            Duration::from_millis(10),
            3,
            IdleConfig {
                enabled: false,
                ..IdleConfig::default()
            },
            shutdown_rx,
        ));

        time::sleep(Duration::from_millis(50)).await;
        shutdown_tx.send(true).unwrap();
        handle.await.unwrap();

        // No kills should have happened
        assert!(backend.kill_calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_watchdog_breach_count_not_reached() {
        let backend = Arc::new(MockBackend::new());
        let store = test_store().await;
        create_running_session(&store, "spike-session").await;

        // 2 high readings then subsides (breach_count=3, so no intervention)
        let reader = MockMemoryReader::new(vec![
            MemorySnapshot {
                available_mb: 200,
                total_mb: 8192,
            },
            MemorySnapshot {
                available_mb: 200,
                total_mb: 8192,
            },
            MemorySnapshot {
                available_mb: 4096,
                total_mb: 8192,
            }, // subsides
        ]);

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let backend_clone = backend.clone();

        let handle = tokio::spawn(run_watchdog_loop(
            backend_clone,
            store,
            Box::new(reader),
            90,
            Duration::from_millis(10),
            3,
            IdleConfig {
                enabled: false,
                ..IdleConfig::default()
            },
            shutdown_rx,
        ));

        time::sleep(Duration::from_millis(80)).await;
        shutdown_tx.send(true).unwrap();
        handle.await.unwrap();

        assert!(backend.kill_calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_watchdog_intervention_after_breach_count() {
        let backend = Arc::new(MockBackend::new());
        let store = test_store().await;
        let session = create_running_session(&store, "oom-session").await;

        // 3 consecutive high readings → intervention
        let reader = MockMemoryReader::new(vec![
            MemorySnapshot {
                available_mb: 200,
                total_mb: 8192,
            },
            MemorySnapshot {
                available_mb: 200,
                total_mb: 8192,
            },
            MemorySnapshot {
                available_mb: 200,
                total_mb: 8192,
            },
        ]);

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let backend_clone = backend.clone();
        let store_clone = store.clone();

        let handle = tokio::spawn(run_watchdog_loop(
            backend_clone,
            store_clone,
            Box::new(reader),
            90,
            Duration::from_millis(10),
            3,
            IdleConfig {
                enabled: false,
                ..IdleConfig::default()
            },
            shutdown_rx,
        ));

        time::sleep(Duration::from_millis(80)).await;
        shutdown_tx.send(true).unwrap();
        handle.await.unwrap();

        // Session should have been killed
        assert!(
            backend
                .kill_calls
                .lock()
                .unwrap()
                .contains(&"oom-session".to_owned())
        );

        // Session should be dead with intervention reason
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Dead);
        assert!(fetched.intervention_reason.is_some());
        assert!(fetched.intervention_at.is_some());
        assert!(fetched.output_snapshot.is_some());
    }

    #[tokio::test]
    async fn test_watchdog_no_running_sessions() {
        let backend = Arc::new(MockBackend::new());
        let store = test_store().await;
        // No sessions at all

        let reader = MockMemoryReader::new(vec![
            MemorySnapshot {
                available_mb: 200,
                total_mb: 8192,
            },
            MemorySnapshot {
                available_mb: 200,
                total_mb: 8192,
            },
            MemorySnapshot {
                available_mb: 200,
                total_mb: 8192,
            },
        ]);

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let backend_clone = backend.clone();

        let handle = tokio::spawn(run_watchdog_loop(
            backend_clone,
            store,
            Box::new(reader),
            90,
            Duration::from_millis(10),
            3,
            IdleConfig {
                enabled: false,
                ..IdleConfig::default()
            },
            shutdown_rx,
        ));

        time::sleep(Duration::from_millis(80)).await;
        shutdown_tx.send(true).unwrap();
        handle.await.unwrap();

        assert!(backend.kill_calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_watchdog_error_reading_memory() {
        let backend = Arc::new(MockBackend::new());
        let store = test_store().await;
        let reader = ErrorMemoryReader;

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        let handle = tokio::spawn(run_watchdog_loop(
            backend,
            store,
            Box::new(reader),
            90,
            Duration::from_millis(10),
            3,
            IdleConfig {
                enabled: false,
                ..IdleConfig::default()
            },
            shutdown_rx,
        ));

        time::sleep(Duration::from_millis(50)).await;
        shutdown_tx.send(true).unwrap();
        handle.await.unwrap();
        // Should not panic, just logs warnings
    }

    #[tokio::test]
    async fn test_watchdog_capture_failure_still_kills() {
        let backend = Arc::new(MockBackend::failing_capture());
        let store = test_store().await;
        let session = create_running_session(&store, "cap-fail").await;

        let reader = MockMemoryReader::new(vec![
            MemorySnapshot {
                available_mb: 100,
                total_mb: 8192,
            },
            MemorySnapshot {
                available_mb: 100,
                total_mb: 8192,
            },
            MemorySnapshot {
                available_mb: 100,
                total_mb: 8192,
            },
        ]);

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let backend_clone = backend.clone();
        let store_clone = store.clone();

        let handle = tokio::spawn(run_watchdog_loop(
            backend_clone,
            store_clone,
            Box::new(reader),
            90,
            Duration::from_millis(10),
            3,
            IdleConfig {
                enabled: false,
                ..IdleConfig::default()
            },
            shutdown_rx,
        ));

        time::sleep(Duration::from_millis(80)).await;
        shutdown_tx.send(true).unwrap();
        handle.await.unwrap();

        // Kill should still be called despite capture failure
        assert!(
            backend
                .kill_calls
                .lock()
                .unwrap()
                .contains(&"cap-fail".to_owned())
        );

        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Dead);
        assert!(fetched.intervention_reason.is_some());
        // No snapshot since capture failed
        assert!(fetched.output_snapshot.is_none());
    }

    #[tokio::test]
    async fn test_watchdog_kill_failure_skips_intervention_record() {
        let backend = Arc::new(MockBackend::failing_kill());
        let store = test_store().await;
        let session = create_running_session(&store, "kill-fail").await;

        let reader = MockMemoryReader::new(vec![
            MemorySnapshot {
                available_mb: 100,
                total_mb: 8192,
            },
            MemorySnapshot {
                available_mb: 100,
                total_mb: 8192,
            },
            MemorySnapshot {
                available_mb: 100,
                total_mb: 8192,
            },
        ]);

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let backend_clone = backend.clone();
        let store_clone = store.clone();

        let handle = tokio::spawn(run_watchdog_loop(
            backend_clone,
            store_clone,
            Box::new(reader),
            90,
            Duration::from_millis(10),
            3,
            IdleConfig {
                enabled: false,
                ..IdleConfig::default()
            },
            shutdown_rx,
        ));

        time::sleep(Duration::from_millis(80)).await;
        shutdown_tx.send(true).unwrap();
        handle.await.unwrap();

        // Session should remain Running — kill failed so no status change
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Running);
        assert!(fetched.intervention_reason.is_none());
    }

    #[tokio::test]
    async fn test_watchdog_session_without_backend_session_id() {
        let backend = Arc::new(MockBackend::new());
        let store = test_store().await;

        // Create session without explicit backend_session_id
        let session = Session {
            id: uuid::Uuid::new_v4(),
            name: "no-tmux".into(),
            workdir: "/tmp/repo".into(),
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            backend_session_id: None,
            output_snapshot: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        store.insert_session(&session).await.unwrap();

        let reader = MockMemoryReader::new(vec![
            MemorySnapshot {
                available_mb: 100,
                total_mb: 8192,
            },
            MemorySnapshot {
                available_mb: 100,
                total_mb: 8192,
            },
            MemorySnapshot {
                available_mb: 100,
                total_mb: 8192,
            },
        ]);

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let backend_clone = backend.clone();

        let handle = tokio::spawn(run_watchdog_loop(
            backend_clone,
            store,
            Box::new(reader),
            90,
            Duration::from_millis(10),
            3,
            IdleConfig {
                enabled: false,
                ..IdleConfig::default()
            },
            shutdown_rx,
        ));

        time::sleep(Duration::from_millis(80)).await;
        shutdown_tx.send(true).unwrap();
        handle.await.unwrap();

        // Should use the session name (backend handles the mapping internally)
        assert!(
            backend
                .kill_calls
                .lock()
                .unwrap()
                .contains(&"no-tmux".to_owned())
        );
    }

    #[tokio::test]
    async fn test_watchdog_breach_counter_resets() {
        let backend = Arc::new(MockBackend::new());
        let store = test_store().await;
        create_running_session(&store, "reset-test").await;

        // 2 high, 1 low (resets), 2 high → no intervention (breach_count=3)
        let reader = MockMemoryReader::new(vec![
            MemorySnapshot {
                available_mb: 200,
                total_mb: 8192,
            },
            MemorySnapshot {
                available_mb: 200,
                total_mb: 8192,
            },
            MemorySnapshot {
                available_mb: 4096,
                total_mb: 8192,
            }, // reset
            MemorySnapshot {
                available_mb: 200,
                total_mb: 8192,
            },
            MemorySnapshot {
                available_mb: 200,
                total_mb: 8192,
            },
        ]);

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let backend_clone = backend.clone();

        let handle = tokio::spawn(run_watchdog_loop(
            backend_clone,
            store,
            Box::new(reader),
            90,
            Duration::from_millis(10),
            3,
            IdleConfig {
                enabled: false,
                ..IdleConfig::default()
            },
            shutdown_rx,
        ));

        time::sleep(Duration::from_millis(100)).await;
        shutdown_tx.send(true).unwrap();
        handle.await.unwrap();

        assert!(backend.kill_calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_watchdog_store_list_failure() {
        let backend = Arc::new(MockBackend::new());
        let store = test_store().await;

        // Drop the sessions table so list_sessions fails during intervention
        sqlx::query("DROP TABLE sessions")
            .execute(store.pool())
            .await
            .unwrap();

        let reader = MockMemoryReader::new(vec![
            MemorySnapshot {
                available_mb: 100,
                total_mb: 8192,
            },
            MemorySnapshot {
                available_mb: 100,
                total_mb: 8192,
            },
            MemorySnapshot {
                available_mb: 100,
                total_mb: 8192,
            },
        ]);

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        let handle = tokio::spawn(run_watchdog_loop(
            backend,
            store,
            Box::new(reader),
            90,
            Duration::from_millis(10),
            3,
            IdleConfig {
                enabled: false,
                ..IdleConfig::default()
            },
            shutdown_rx,
        ));

        time::sleep(Duration::from_millis(80)).await;
        shutdown_tx.send(true).unwrap();
        handle.await.unwrap();
        // Should not panic — logs warning about list failure
    }

    #[tokio::test]
    async fn test_intervene_snapshot_save_failure() {
        // Test that snapshot save failure is handled gracefully.
        // We do this by creating a session, then corrupting the store
        // so update_session_output_snapshot fails but the session can still be listed.
        let backend = Arc::new(MockBackend::new());
        let store = test_store().await;
        create_running_session(&store, "snap-err").await;

        // Rename the snapshot column to break the UPDATE query
        sqlx::query("ALTER TABLE sessions RENAME COLUMN output_snapshot TO output_snapshot_old")
            .execute(store.pool())
            .await
            .unwrap();

        let snapshot = MemorySnapshot {
            available_mb: 100,
            total_mb: 8192,
        };
        let dyn_backend: Arc<dyn Backend> = backend.clone();
        intervene(&dyn_backend, &store, &snapshot).await;

        // Kill should still have been called despite snapshot save failure
        assert!(
            backend
                .kill_calls
                .lock()
                .unwrap()
                .contains(&"snap-err".to_owned())
        );
    }

    #[tokio::test]
    async fn test_intervene_record_failure() {
        // Test that intervention recording failure is handled gracefully.
        let backend = Arc::new(MockBackend::new());
        let store = test_store().await;
        create_running_session(&store, "rec-err").await;

        // Rename intervention_reason column to break the UPDATE query
        sqlx::query(
            "ALTER TABLE sessions RENAME COLUMN intervention_reason TO intervention_reason_old",
        )
        .execute(store.pool())
        .await
        .unwrap();

        let snapshot = MemorySnapshot {
            available_mb: 100,
            total_mb: 8192,
        };
        let dyn_backend: Arc<dyn Backend> = backend.clone();
        intervene(&dyn_backend, &store, &snapshot).await;

        // Kill should still have been called
        assert!(
            backend
                .kill_calls
                .lock()
                .unwrap()
                .contains(&"rec-err".to_owned())
        );
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
    }

    #[test]
    fn test_idle_config_debug_clone() {
        let ic = IdleConfig {
            enabled: true,
            timeout_secs: 300,
            action: IdleAction::Kill,
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
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            backend_session_id: Some("pulpo-idle-session".into()),
            output_snapshot: Some("test output".into()),
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
            idle_since: None,
            waiting_for_input: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        // Set the output_snapshot to match what MockBackend returns
        session.output_snapshot = Some("test output".into());
        store.insert_session(&session).await.unwrap();

        let idle_config = IdleConfig {
            enabled: true,
            timeout_secs: 600,
            action: IdleAction::Alert,
        };

        let dyn_backend: Arc<dyn Backend> = backend;
        check_idle_sessions(&dyn_backend, &store, &idle_config).await;

        // Session should now have idle_since set
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.idle_since.is_some());
        // Session should still be running (alert mode doesn't kill)
        assert_eq!(fetched.status, SessionStatus::Running);
    }

    #[tokio::test]
    async fn test_idle_detection_kill_action() {
        let backend = Arc::new(MockBackend::new());
        let store = test_store().await;

        let session = Session {
            id: uuid::Uuid::new_v4(),
            name: "kill-idle".into(),
            workdir: "/tmp/repo".into(),
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            backend_session_id: Some("pulpo-kill-idle".into()),
            output_snapshot: Some("test output".into()),
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
            idle_since: None,
            waiting_for_input: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        store.insert_session(&session).await.unwrap();

        let idle_config = IdleConfig {
            enabled: true,
            timeout_secs: 600,
            action: IdleAction::Kill,
        };

        let backend_clone = backend.clone();
        let dyn_backend: Arc<dyn Backend> = backend_clone;
        check_idle_sessions(&dyn_backend, &store, &idle_config).await;

        // Session should be dead with intervention reason
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Dead);
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
    async fn test_idle_detection_clears_when_active() {
        let backend = Arc::new(MockBackend::new());
        let store = test_store().await;

        // Create session that was idle but now has new output
        let session = Session {
            id: uuid::Uuid::new_v4(),
            name: "active-again".into(),
            workdir: "/tmp/repo".into(),
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            backend_session_id: Some("pulpo-active-again".into()),
            output_snapshot: Some("old output".into()), // different from "test output"
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
            idle_since: Some(chrono::Utc::now() - chrono::Duration::seconds(100)),
            waiting_for_input: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        store.insert_session(&session).await.unwrap();

        let idle_config = IdleConfig {
            enabled: true,
            timeout_secs: 600,
            action: IdleAction::Alert,
        };

        let dyn_backend: Arc<dyn Backend> = backend;
        check_idle_sessions(&dyn_backend, &store, &idle_config).await;

        // idle_since should be cleared (output changed from "old output" to "test output")
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.idle_since.is_none());
        assert_eq!(fetched.status, SessionStatus::Running);
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
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Completed,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: Some(0),
            backend_session_id: None,
            output_snapshot: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        store.insert_session(&session).await.unwrap();

        let idle_config = IdleConfig {
            enabled: true,
            timeout_secs: 1,
            action: IdleAction::Kill,
        };

        let dyn_backend: Arc<dyn Backend> = backend;
        check_idle_sessions(&dyn_backend, &store, &idle_config).await;

        // Session should remain completed
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Completed);
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
        };

        let dyn_backend: Arc<dyn Backend> = backend;
        check_idle_sessions(&dyn_backend, &store, &idle_config).await;

        // Session should remain running — capture failed so idle check skipped
        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions[0].status, SessionStatus::Running);
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
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            backend_session_id: Some("pulpo-recent-session".into()),
            output_snapshot: Some("test output".into()),
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: Some(chrono::Utc::now()), // very recent
            idle_since: None,
            waiting_for_input: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        store.insert_session(&session).await.unwrap();

        let idle_config = IdleConfig {
            enabled: true,
            timeout_secs: 600,
            action: IdleAction::Alert,
        };

        let dyn_backend: Arc<dyn Backend> = backend;
        check_idle_sessions(&dyn_backend, &store, &idle_config).await;

        // Should NOT be marked idle (not enough time elapsed)
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.idle_since.is_none());
    }

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
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            backend_session_id: Some("pulpo-already-idle".into()),
            output_snapshot: Some("test output".into()),
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
            idle_since: Some(idle_time),
            waiting_for_input: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        store.insert_session(&session).await.unwrap();

        let idle_config = IdleConfig {
            enabled: true,
            timeout_secs: 600,
            action: IdleAction::Alert,
        };

        let dyn_backend: Arc<dyn Backend> = backend;
        check_idle_sessions(&dyn_backend, &store, &idle_config).await;

        // idle_since should still be set (already marked, not re-marked)
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.idle_since.is_some());
        assert_eq!(fetched.status, SessionStatus::Running);
    }

    #[tokio::test]
    async fn test_idle_detection_kill_failure() {
        let backend = Arc::new(MockBackend::failing_kill());
        let store = test_store().await;

        let session = Session {
            id: uuid::Uuid::new_v4(),
            name: "kill-fail-idle".into(),
            workdir: "/tmp/repo".into(),
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            backend_session_id: Some("pulpo-kill-fail-idle".into()),
            output_snapshot: Some("test output".into()),
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
            idle_since: None,
            waiting_for_input: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        store.insert_session(&session).await.unwrap();

        let idle_config = IdleConfig {
            enabled: true,
            timeout_secs: 600,
            action: IdleAction::Kill,
        };

        let dyn_backend: Arc<dyn Backend> = backend;
        check_idle_sessions(&dyn_backend, &store, &idle_config).await;

        // Session should remain running since kill failed
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Running);
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
        };

        let dyn_backend: Arc<dyn Backend> = backend;
        // Should not panic
        check_idle_sessions(&dyn_backend, &store, &idle_config).await;
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
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            backend_session_id: Some("pulpo-no-output-ts".into()),
            output_snapshot: Some("test output".into()),
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: chrono::Utc::now() - chrono::Duration::seconds(700),
            updated_at: chrono::Utc::now(),
        };
        store.insert_session(&session).await.unwrap();

        let idle_config = IdleConfig {
            enabled: true,
            timeout_secs: 600,
            action: IdleAction::Alert,
        };

        let dyn_backend: Arc<dyn Backend> = backend;
        check_idle_sessions(&dyn_backend, &store, &idle_config).await;

        // Should be marked idle (created_at is used as fallback)
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.idle_since.is_some());
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
        };

        let dyn_backend: Arc<dyn Backend> = backend;
        // Should not panic
        check_idle_sessions(&dyn_backend, &store, &idle_config).await;
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
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            backend_session_id: Some("pulpo-loop-idle".into()),
            output_snapshot: Some("test output".into()),
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
            idle_since: None,
            waiting_for_input: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        store.insert_session(&session).await.unwrap();

        let reader = MockMemoryReader::new(vec![MemorySnapshot {
            available_mb: 4096,
            total_mb: 8192,
        }]);

        let idle_config = IdleConfig {
            enabled: true,
            timeout_secs: 600,
            action: IdleAction::Alert,
        };

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let backend_clone = backend.clone();
        let store_clone = store.clone();

        let handle = tokio::spawn(run_watchdog_loop(
            backend_clone,
            store_clone,
            Box::new(reader),
            90,
            Duration::from_millis(10),
            3,
            idle_config,
            shutdown_rx,
        ));

        time::sleep(Duration::from_millis(50)).await;
        shutdown_tx.send(true).unwrap();
        handle.await.unwrap();

        // Session should have been marked idle
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.idle_since.is_some());
    }

    #[tokio::test]
    async fn test_handle_active_session_clear_fails() {
        let store = test_store().await;

        let session = Session {
            id: uuid::Uuid::new_v4(),
            name: "clear-fail".into(),
            workdir: "/tmp/repo".into(),
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            backend_session_id: Some("pulpo-clear-fail".into()),
            output_snapshot: Some("test output".into()),
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: Some(chrono::Utc::now()),
            idle_since: Some(chrono::Utc::now()),
            waiting_for_input: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        // Drop sessions table to make store operations fail
        sqlx::query("DROP TABLE sessions")
            .execute(store.pool())
            .await
            .unwrap();

        // Should not panic — logs warning and returns
        handle_active_session(&store, &session).await;
    }

    #[tokio::test]
    async fn test_handle_active_session_not_idle() {
        let store = test_store().await;

        let session = Session {
            id: uuid::Uuid::new_v4(),
            name: "not-idle".into(),
            workdir: "/tmp/repo".into(),
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            backend_session_id: Some("pulpo-not-idle".into()),
            output_snapshot: Some("test output".into()),
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: Some(chrono::Utc::now()),
            idle_since: None,
            waiting_for_input: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        // idle_since is None — early return, no store call
        handle_active_session(&store, &session).await;
    }

    #[tokio::test]
    async fn test_handle_idle_session_alert_update_fails() {
        let backend = Arc::new(MockBackend::new());
        let store = test_store().await;

        let session = Session {
            id: uuid::Uuid::new_v4(),
            name: "alert-fail".into(),
            workdir: "/tmp/repo".into(),
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            backend_session_id: Some("pulpo-alert-fail".into()),
            output_snapshot: Some("test output".into()),
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
            idle_since: None,
            waiting_for_input: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
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
            "alert-fail",
            now,
            timeout,
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
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            backend_session_id: Some("pulpo-kill-record-fail".into()),
            output_snapshot: Some("test output".into()),
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
            idle_since: None,
            waiting_for_input: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
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
            "kill-record-fail",
            now,
            timeout,
        )
        .await;
    }

    #[tokio::test]
    async fn test_check_session_idle_without_backend_session_id() {
        let backend = Arc::new(MockBackend::new());
        let store = test_store().await;

        // Session with backend_session_id = None (falls back to "pulpo-{name}")
        let session = Session {
            id: uuid::Uuid::new_v4(),
            name: "no-tmux".into(),
            workdir: "/tmp/repo".into(),
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            backend_session_id: None,
            output_snapshot: Some("test output".into()),
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
            idle_since: None,
            waiting_for_input: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        store.insert_session(&session).await.unwrap();

        let idle_config = IdleConfig {
            enabled: true,
            timeout_secs: 600,
            action: IdleAction::Alert,
        };
        let now = chrono::Utc::now();
        let timeout = chrono::Duration::seconds(600);
        let dyn_backend: Arc<dyn Backend> = backend;

        // Should use session.name for backend calls
        check_session_idle(&dyn_backend, &store, &idle_config, &session, now, timeout).await;

        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.idle_since.is_some());
    }

    #[test]
    fn test_detect_waiting_for_input_trust_prompt() {
        let output = "Welcome to Claude Code!\n\nDo you trust the files in this folder?\nYes / No";
        assert!(detect_waiting_for_input(output));
    }

    #[test]
    fn test_detect_waiting_for_input_yn_prompt() {
        let output = "Some output\nInstall dependencies? (y/n)";
        assert!(detect_waiting_for_input(output));
    }

    #[test]
    fn test_detect_waiting_for_input_bracket_yn() {
        let output = "Continue? [Y/n]";
        assert!(detect_waiting_for_input(output));
    }

    #[test]
    fn test_detect_waiting_for_input_press_enter() {
        let output = "Setup complete.\nPress Enter to continue...";
        assert!(detect_waiting_for_input(output));
    }

    #[test]
    fn test_detect_waiting_for_input_no_match() {
        let output = "Building project...\nCompiling src/main.rs\nFinished in 2.3s";
        assert!(!detect_waiting_for_input(output));
    }

    #[test]
    fn test_detect_waiting_for_input_empty() {
        assert!(!detect_waiting_for_input(""));
    }

    #[test]
    fn test_detect_waiting_for_input_only_checks_last_5_lines() {
        // The pattern is on line 1 (6+ lines ago), outside the last 5
        let output = "Do you trust this?\nline2\nline3\nline4\nline5\nline6\nline7";
        assert!(!detect_waiting_for_input(output));
    }

    #[test]
    fn test_detect_waiting_for_input_case_insensitive() {
        let output = "DO YOU TRUST this folder?";
        assert!(detect_waiting_for_input(output));
    }

    #[test]
    fn test_detect_waiting_for_input_yes_no_brackets() {
        let output = "Are you sure? [yes/no]";
        assert!(detect_waiting_for_input(output));
    }

    #[test]
    fn test_detect_waiting_for_input_approve() {
        let output = "Please approve this action";
        assert!(detect_waiting_for_input(output));
    }

    #[tokio::test]
    async fn test_idle_check_sets_waiting_for_input() {
        let backend = MockBackend::new().with_output("Do you trust this folder?\nYes / No");
        let store = test_store().await;
        let session = make_running_session("waiting-detect");
        store.insert_session(&session).await.unwrap();

        let now = chrono::Utc::now();
        let timeout = chrono::Duration::seconds(600);
        let idle_config = IdleConfig::default();

        let dyn_backend: Arc<dyn Backend> = Arc::new(backend);
        check_session_idle(&dyn_backend, &store, &idle_config, &session, now, timeout).await;

        let updated = store.get_session("waiting-detect").await.unwrap().unwrap();
        assert!(updated.waiting_for_input);
    }

    #[tokio::test]
    async fn test_idle_check_clears_waiting_for_input() {
        let backend = MockBackend::new().with_output("Building project...\nDone.");
        let store = test_store().await;
        let mut session = make_running_session("waiting-clear");
        session.waiting_for_input = true;
        store.insert_session(&session).await.unwrap();
        // Also set it in the DB explicitly
        store
            .update_session_waiting_for_input(&session.id.to_string(), true)
            .await
            .unwrap();

        let now = chrono::Utc::now();
        let timeout = chrono::Duration::seconds(600);
        let idle_config = IdleConfig::default();

        // Re-read to get the DB state
        let session = store.get_session("waiting-clear").await.unwrap().unwrap();

        let dyn_backend: Arc<dyn Backend> = Arc::new(backend);
        check_session_idle(&dyn_backend, &store, &idle_config, &session, now, timeout).await;

        let updated = store.get_session("waiting-clear").await.unwrap().unwrap();
        assert!(!updated.waiting_for_input);
    }

    #[tokio::test]
    async fn test_idle_check_waiting_update_error() {
        let backend = MockBackend::new().with_output("Do you trust this folder?\nYes / No");
        let store = test_store().await;
        let session = make_running_session("waiting-err");
        store.insert_session(&session).await.unwrap();

        // Drop the waiting_for_input column to make the update query fail
        // while output snapshot update still succeeds
        sqlx::query("ALTER TABLE sessions DROP COLUMN waiting_for_input")
            .execute(store.pool())
            .await
            .unwrap();

        let now = chrono::Utc::now();
        let timeout = chrono::Duration::seconds(600);
        let idle_config = IdleConfig::default();

        // Should log a warning but not panic
        let dyn_backend: Arc<dyn Backend> = Arc::new(backend);
        check_session_idle(&dyn_backend, &store, &idle_config, &session, now, timeout).await;
    }
}
