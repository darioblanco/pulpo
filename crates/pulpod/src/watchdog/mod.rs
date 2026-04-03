mod adopt;
mod git;
mod intervention;
pub mod memory;
mod metadata;
pub mod output_patterns;

use std::sync::Arc;
use std::time::Duration;

use adopt::adopt_tmux_sessions;
#[cfg(test)]
use adopt::classify_adopted_process;
use memory::MemoryReader;
#[cfg(test)]
use memory::MemorySnapshot;
use metadata::{build_session_event, detect_and_store_output_metadata};
use pulpo_common::event::PulpoEvent;
use pulpo_common::session::SessionStatus;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use pulpo_common::session::{InterventionCode, Session};

use crate::backend::Backend;
use crate::store::Store;
use git::update_git_info;
use intervention::intervene;

/// The marker emitted by the agent wrapper when the agent process exits.
const AGENT_EXIT_MARKER: &str = "[pulpo] Agent exited";

/// Check if the terminal output contains the agent exit marker.
pub fn detect_agent_exited(output: &str) -> bool {
    output.contains(AGENT_EXIT_MARKER)
}

/// Resolve the backend session ID from a session, falling back to session name.
fn resolve_backend_id(session: &Session, backend: &dyn Backend) -> String {
    session
        .backend_session_id
        .clone()
        .unwrap_or_else(|| backend.session_id(&session.name))
}

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
    /// Seconds of unchanged output before Active→Idle transition.
    pub threshold_secs: u64,
}

impl Default for IdleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            timeout_secs: 600,
            action: IdleAction::Alert,
            threshold_secs: 60,
        }
    }
}

/// Runtime configuration for the watchdog loop, updated via watch channel.
#[derive(Debug, Clone)]
pub struct WatchdogRuntimeConfig {
    pub threshold: u8,
    pub interval: Duration,
    pub breach_count: u32,
    pub idle: IdleConfig,
    /// Seconds after Ready before tmux shell is killed (0 = disabled).
    pub ready_ttl_secs: u64,
    /// Auto-adopt external tmux sessions into pulpo management.
    pub adopt_tmux: bool,
    /// Extra user-configured patterns for waiting-for-input detection.
    pub extra_waiting_patterns: Vec<String>,
}

/// Context for handling agent-ready transitions (status update + events).
#[cfg_attr(coverage, allow(dead_code))]
pub struct ReadyContext {
    pub event_tx: Option<broadcast::Sender<PulpoEvent>>,
    pub node_name: String,
}

/// Runs the watchdog loop that monitors system memory and intervenes when sustained pressure
/// is detected. Kills running sessions after `breach_count` consecutive checks above `threshold`.
///
/// The loop dynamically picks up config changes sent via the `config_rx` watch channel.
pub async fn run_watchdog_loop(
    backend: Arc<dyn Backend>,
    store: Store,
    reader: Box<dyn MemoryReader>,
    config_rx: tokio::sync::watch::Receiver<WatchdogRuntimeConfig>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ready_ctx: ReadyContext,
) {
    let initial = config_rx.borrow().clone();
    let mut current_interval = initial.interval;
    let mut tick = tokio::time::interval(current_interval);
    tick.tick().await; // first tick completes immediately
    let mut consecutive_breaches: u32 = 0;

    loop {
        tokio::select! {
            _ = tick.tick() => {
                let cfg = config_rx.borrow().clone();

                // If interval changed, recreate the ticker
                if cfg.interval != current_interval {
                    info!(
                        old_interval_secs = current_interval.as_secs(),
                        new_interval_secs = cfg.interval.as_secs(),
                        "Watchdog interval changed, resetting ticker"
                    );
                    current_interval = cfg.interval;
                    tick = tokio::time::interval(current_interval);
                    tick.tick().await; // consume immediate first tick
                }

                match reader.read_memory() {
                    Ok(snapshot) => {
                        let usage = snapshot.usage_percent();
                        debug!(usage, threshold = cfg.threshold, consecutive_breaches, "Memory check");

                        if usage >= cfg.threshold {
                            consecutive_breaches += 1;
                            warn!(
                                usage,
                                threshold = cfg.threshold,
                                consecutive_breaches,
                                breach_count = cfg.breach_count,
                                available_mb = snapshot.available_mb,
                                total_mb = snapshot.total_mb,
                                "Memory pressure detected"
                            );

                            if consecutive_breaches >= cfg.breach_count {
                                intervene(&backend, &store, &snapshot).await;
                                consecutive_breaches = 0;
                            }
                        } else {
                            if consecutive_breaches > 0 {
                                info!(
                                    usage,
                                    threshold = cfg.threshold,
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

                // Idle + ready detection runs on every tick, independent of memory checks
                if cfg.idle.enabled {
                    check_idle_sessions(&backend, &store, &cfg.idle, &ready_ctx, &cfg.extra_waiting_patterns).await;
                }

                // Clean up ready sessions whose tmux shell has exceeded the TTL
                if cfg.ready_ttl_secs > 0 {
                    cleanup_ready_sessions(&backend, &store, cfg.ready_ttl_secs).await;
                }

                // Auto-adopt external tmux sessions
                if cfg.adopt_tmux {
                    adopt_tmux_sessions(&backend, &store, &ready_ctx).await;
                }

                // Update git branch/commit info for active sessions
                update_git_info(&store).await;
            }
            _ = shutdown_rx.changed() => {
                info!("Watchdog shutting down");
                break;
            }
        }
    }
}

/// Patterns that indicate the agent is waiting for user input.
/// Pre-lowercased for efficient matching against lowercased output lines.
const DEFAULT_WAITING_PATTERNS: &[&str] = &[
    // Generic confirmation prompts
    "(y/n)",
    "[y/n]",
    "[yes/no]",
    "(yes/no)",
    "yes / no",
    "do you trust",
    "press enter",
    "approve this",
    "are you sure",
    "continue?",
    "confirm?",
    "proceed?",
    // Claude Code
    "(y)es",
    "(n)o",
    "(a)lways",
    "do you want to proceed",
    // Codex CLI
    "allow command?",
    // Gemini CLI
    "allow?",
    "approve?",
    // Aider
    "to the chat?",
    "apply edit?",
    "shell command?",
    "create new file",
    // Amazon Q
    "allow this action?",
    "accept suggestion?",
    // SSH/sudo
    "continue connecting (yes/no)",
    "'s password:",
    "[sudo] password",
];

/// Check if the terminal output suggests the agent is waiting for user input.
/// Inspects the last 5 lines of output for known prompt patterns and extra user-configured patterns.
pub fn detect_waiting_for_input(output: &str, extra_patterns: &[String]) -> bool {
    let last_lines: Vec<&str> = output.lines().rev().take(5).collect();
    for line in &last_lines {
        let lower = line.to_lowercase();
        // DEFAULT_WAITING_PATTERNS are pre-lowercased
        for pattern in DEFAULT_WAITING_PATTERNS {
            if lower.contains(pattern) {
                return true;
            }
        }
        for pattern in extra_patterns {
            if lower.contains(&pattern.to_lowercase()) {
                return true;
            }
        }
    }
    false
}

async fn check_idle_sessions(
    backend: &Arc<dyn Backend>,
    store: &Store,
    idle_config: &IdleConfig,
    ready_ctx: &ReadyContext,
    extra_waiting_patterns: &[String],
) {
    let sessions = match store.list_sessions().await {
        Ok(s) => s,
        Err(e) => {
            warn!("Idle check: failed to list sessions: {e}");
            return;
        }
    };

    // Process both Active and Idle sessions — Active may become Idle, Idle may become Active
    let live: Vec<_> = sessions
        .into_iter()
        .filter(|s| s.status == SessionStatus::Active || s.status == SessionStatus::Idle)
        .collect();

    let now = chrono::Utc::now();
    let timeout =
        chrono::Duration::seconds(idle_config.timeout_secs.try_into().unwrap_or(i64::MAX));

    for session in &live {
        check_session_idle(
            backend,
            store,
            idle_config,
            session,
            now,
            timeout,
            ready_ctx,
            extra_waiting_patterns,
        )
        .await;
    }
}

async fn check_session_idle(
    backend: &Arc<dyn Backend>,
    store: &Store,
    idle_config: &IdleConfig,
    session: &pulpo_common::session::Session,
    now: chrono::DateTime<chrono::Utc>,
    timeout: chrono::Duration,
    ready_ctx: &ReadyContext,
    extra_waiting_patterns: &[String],
) {
    // Capture current output to track activity
    let bid = resolve_backend_id(session, backend.as_ref());
    let current_output = match backend.capture_output(&bid, 500) {
        Ok(o) => o,
        Err(e) => {
            debug!(
                "Idle check: failed to capture output for {}: {e}",
                session.name
            );
            return;
        }
    };

    // Check for agent exit marker → transition to Ready
    if detect_agent_exited(&current_output) {
        handle_session_ready(store, session, ready_ctx).await;
        return;
    }

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

    // Detect PR URL and branch from output (only if not already stored)
    detect_and_store_output_metadata(store, session, &current_output).await;

    // Determine if output changed since last check
    let output_changed = session.output_snapshot.as_deref() != Some(current_output.as_str());

    if output_changed {
        handle_active_session(store, session, ready_ctx).await;
    } else {
        // Output unchanged since last tick — transition Active → Idle.
        // Known waiting-for-input patterns trigger immediate transition on the
        // first unchanged tick. Without a pattern match, we require 2 consecutive
        // unchanged ticks (last_output_at older than check interval) to avoid
        // false positives during brief pauses in output.
        if session.status == SessionStatus::Active {
            let immediate = detect_waiting_for_input(&current_output, extra_waiting_patterns);
            let last_change = session.last_output_at.unwrap_or(session.created_at);
            let sustained = (now - last_change).num_seconds()
                >= i64::try_from(idle_config.threshold_secs).unwrap_or(i64::MAX);
            if immediate || sustained {
                info!(
                    "Session {} idle ({}), transitioning to idle",
                    session.name,
                    if immediate {
                        "waiting pattern"
                    } else {
                        "output unchanged"
                    }
                );
                if let Err(e) = store
                    .update_session_status(&session.id.to_string(), SessionStatus::Idle)
                    .await
                {
                    warn!(
                        "Idle check: failed to transition {} to idle: {e}",
                        session.name
                    );
                } else if let Some(tx) = &ready_ctx.event_tx {
                    let event = build_session_event(
                        session,
                        SessionStatus::Idle,
                        Some(SessionStatus::Active),
                        &ready_ctx.node_name,
                        Some(current_output.clone()),
                    );
                    let _ = tx.send(PulpoEvent::Session(event));
                }
                return;
            }
        }
        handle_idle_session(backend, store, idle_config, session, &bid, now, timeout).await;
    }
}

/// Handle a session whose agent has exited: transition to Ready and emit event.
async fn handle_session_ready(store: &Store, session: &Session, ctx: &ReadyContext) {
    let previous = session.status;
    info!(
        session_name = %session.name,
        "Agent exited, transitioning to ready"
    );
    if let Err(e) = store
        .update_session_status(&session.id.to_string(), SessionStatus::Ready)
        .await
    {
        warn!(
            session_name = %session.name,
            "Failed to transition to ready: {e}"
        );
        return;
    }

    // Emit SSE event
    if let Some(tx) = &ctx.event_tx {
        let event = build_session_event(
            session,
            SessionStatus::Ready,
            Some(previous),
            &ctx.node_name,
            session.output_snapshot.clone(),
        );
        let _ = tx.send(PulpoEvent::Session(event));
    }
}

async fn handle_active_session(
    store: &Store,
    session: &pulpo_common::session::Session,
    ready_ctx: &ReadyContext,
) {
    // If session was Idle and output changed, transition back to Active
    if session.status == SessionStatus::Idle {
        info!(
            "Session {} has new output, transitioning back to active",
            session.name
        );
        if let Err(e) = store
            .update_session_status(&session.id.to_string(), SessionStatus::Active)
            .await
        {
            warn!(
                "Idle check: failed to transition {} back to active: {e}",
                session.name
            );
        } else if let Some(tx) = &ready_ctx.event_tx {
            let event = build_session_event(
                session,
                SessionStatus::Active,
                Some(SessionStatus::Idle),
                &ready_ctx.node_name,
                session.output_snapshot.clone(),
            );
            let _ = tx.send(PulpoEvent::Session(event));
        }
    }
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
    backend_id: &str,
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

            if let Err(e) = backend.kill_session(backend_id) {
                warn!(
                    "Idle check: failed to kill idle session {}: {e}",
                    session.name
                );
                return;
            }

            if let Err(e) = store
                .update_session_intervention(
                    &session.id.to_string(),
                    InterventionCode::IdleTimeout,
                    &reason,
                )
                .await
            {
                warn!(
                    "Idle check: failed to record intervention for {}: {e}",
                    session.name
                );
            }
            // Clean up worktree if this was a worktree session
            if let Some(ref wt_path) = session.worktree_path {
                crate::session::manager::cleanup_worktree(wt_path, &session.workdir);
            }
            warn!(
                "Idle check: stopped idle session {} after {minutes} minutes",
                session.name
            );
        }
    }
}

/// Kill tmux shells for Ready sessions that have exceeded the TTL grace period.
async fn cleanup_ready_sessions(backend: &Arc<dyn Backend>, store: &Store, ready_ttl_secs: u64) {
    let sessions = match store.list_sessions().await {
        Ok(s) => s,
        Err(e) => {
            warn!("Ready cleanup: failed to list sessions: {e}");
            return;
        }
    };

    let now = chrono::Utc::now();
    let ttl = chrono::Duration::seconds(ready_ttl_secs.try_into().unwrap_or(i64::MAX));

    for session in sessions.iter().filter(|s| s.status == SessionStatus::Ready) {
        let age = now - session.updated_at;
        if age <= ttl {
            continue;
        }

        let bid = resolve_backend_id(session, backend.as_ref());
        if let Err(e) = backend.kill_session(&bid) {
            debug!(
                session_name = %session.name,
                "Ready cleanup: tmux already gone: {e}"
            );
        }
        if let Err(e) = store
            .update_session_status(&session.id.to_string(), SessionStatus::Stopped)
            .await
        {
            warn!(
                session_name = %session.name,
                "Ready cleanup: failed to mark stopped: {e}"
            );
        } else {
            info!(
                session_name = %session.name,
                age_secs = age.num_seconds(),
                "Ready cleanup: stopped tmux shell after TTL"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Backend;
    use anyhow::Result;
    use pulpo_common::session::{Session, SessionStatus};
    use std::collections::HashMap;
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
        tmux_sessions: Vec<(String, String)>,
        pane_infos: HashMap<String, (String, String)>,
        pane_command_lines: HashMap<String, String>,
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
                tmux_sessions: Vec::new(),
                pane_infos: HashMap::new(),
                pane_command_lines: HashMap::new(),
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
        fn list_sessions(&self) -> Result<Vec<(String, String)>> {
            Ok(self.tmux_sessions.clone())
        }
        fn pane_info(&self, backend_id: &str) -> Result<(String, String)> {
            self.pane_infos
                .get(backend_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("no pane info for {backend_id}"))
        }
        fn pane_command_line(&self, backend_id: &str) -> Result<String> {
            self.pane_command_lines
                .get(backend_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("no command line for {backend_id}"))
        }
    }

    async fn test_store() -> Store {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        store
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
            status: SessionStatus::Active,
            backend_session_id: Some(name.to_owned()),
            ..Default::default()
        };
        store.insert_session(&session).await.unwrap();
        session
    }

    fn make_config(
        threshold: u8,
        interval: Duration,
        breach_count: u32,
        idle: IdleConfig,
    ) -> tokio::sync::watch::Receiver<WatchdogRuntimeConfig> {
        let cfg = WatchdogRuntimeConfig {
            threshold,
            interval,
            breach_count,
            idle,
            ready_ttl_secs: 0,
            adopt_tmux: false,
            extra_waiting_patterns: Vec::new(),
        };
        let (_, rx) = tokio::sync::watch::channel(cfg);
        rx
    }

    fn make_config_with_tx(
        threshold: u8,
        interval: Duration,
        breach_count: u32,
        idle: IdleConfig,
    ) -> (
        tokio::sync::watch::Sender<WatchdogRuntimeConfig>,
        tokio::sync::watch::Receiver<WatchdogRuntimeConfig>,
    ) {
        let cfg = WatchdogRuntimeConfig {
            threshold,
            interval,
            breach_count,
            idle,
            ready_ttl_secs: 0,
            adopt_tmux: false,
            extra_waiting_patterns: Vec::new(),
        };
        tokio::sync::watch::channel(cfg)
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
            make_config(
                90,
                Duration::from_millis(10),
                3,
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
            make_config(
                90,
                Duration::from_millis(10),
                3,
                IdleConfig {
                    enabled: false,
                    ..IdleConfig::default()
                },
            ),
            shutdown_rx,
            test_ready_ctx(),
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
            make_config(
                90,
                Duration::from_millis(10),
                3,
                IdleConfig {
                    enabled: false,
                    ..IdleConfig::default()
                },
            ),
            shutdown_rx,
            test_ready_ctx(),
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
            make_config(
                90,
                Duration::from_millis(10),
                3,
                IdleConfig {
                    enabled: false,
                    ..IdleConfig::default()
                },
            ),
            shutdown_rx,
            test_ready_ctx(),
        ));

        time::sleep(Duration::from_millis(80)).await;
        shutdown_tx.send(true).unwrap();
        handle.await.unwrap();

        // Session should have been stopped
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
        assert_eq!(fetched.status, SessionStatus::Stopped);
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
            make_config(
                90,
                Duration::from_millis(10),
                3,
                IdleConfig {
                    enabled: false,
                    ..IdleConfig::default()
                },
            ),
            shutdown_rx,
            test_ready_ctx(),
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
            make_config(
                90,
                Duration::from_millis(10),
                3,
                IdleConfig {
                    enabled: false,
                    ..IdleConfig::default()
                },
            ),
            shutdown_rx,
            test_ready_ctx(),
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
            make_config(
                90,
                Duration::from_millis(10),
                3,
                IdleConfig {
                    enabled: false,
                    ..IdleConfig::default()
                },
            ),
            shutdown_rx,
            test_ready_ctx(),
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
        assert_eq!(fetched.status, SessionStatus::Stopped);
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
            make_config(
                90,
                Duration::from_millis(10),
                3,
                IdleConfig {
                    enabled: false,
                    ..IdleConfig::default()
                },
            ),
            shutdown_rx,
            test_ready_ctx(),
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
        assert_eq!(fetched.status, SessionStatus::Active);
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
            command: "echo hello".into(),
            description: Some("test".into()),
            status: SessionStatus::Active,
            ..Default::default()
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
            make_config(
                90,
                Duration::from_millis(10),
                3,
                IdleConfig {
                    enabled: false,
                    ..IdleConfig::default()
                },
            ),
            shutdown_rx,
            test_ready_ctx(),
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
            make_config(
                90,
                Duration::from_millis(10),
                3,
                IdleConfig {
                    enabled: false,
                    ..IdleConfig::default()
                },
            ),
            shutdown_rx,
            test_ready_ctx(),
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
            make_config(
                90,
                Duration::from_millis(10),
                3,
                IdleConfig {
                    enabled: false,
                    ..IdleConfig::default()
                },
            ),
            shutdown_rx,
            test_ready_ctx(),
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
            status: SessionStatus::Active,
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
        assert_eq!(fetched.status, SessionStatus::Idle);
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
            status: SessionStatus::Idle,
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
        assert_eq!(fetched.status, SessionStatus::Stopped);
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
            command: "echo hello".into(),
            description: Some("test".into()),
            status: SessionStatus::Active,
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
        assert_eq!(fetched.status, SessionStatus::Active);
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
            status: SessionStatus::Ready,
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
        assert_eq!(fetched.status, SessionStatus::Ready);
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
        assert_eq!(sessions[0].status, SessionStatus::Active);
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
            status: SessionStatus::Active,
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
            status: SessionStatus::Idle,
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
        assert_eq!(fetched.status, SessionStatus::Idle);
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
            status: SessionStatus::Idle,
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
        assert_eq!(fetched.status, SessionStatus::Idle);
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
            status: SessionStatus::Active,
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
        assert_eq!(fetched.status, SessionStatus::Idle);
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
            status: SessionStatus::Active,
            backend_session_id: Some("loop-idle".into()),
            output_snapshot: Some("test output".into()),
            last_output_at: Some(chrono::Utc::now() - chrono::Duration::seconds(700)),
            ..Default::default()
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
            threshold_secs: 60,
        };

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let backend_clone = backend.clone();
        let store_clone = store.clone();

        let handle = tokio::spawn(run_watchdog_loop(
            backend_clone,
            store_clone,
            Box::new(reader),
            make_config(90, Duration::from_millis(10), 3, idle_config),
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
            status: SessionStatus::Active,
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
        handle_active_session(&store, &session, &test_ready_ctx()).await;
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
            status: SessionStatus::Active,
            backend_session_id: Some("not-idle".into()),
            output_snapshot: Some("test output".into()),
            last_output_at: Some(chrono::Utc::now()),
            ..Default::default()
        };

        // idle_since is None — early return, no store call
        handle_active_session(&store, &session, &test_ready_ctx()).await;
    }

    #[tokio::test]
    async fn test_idle_transition_emits_sse_event() {
        let backend =
            Arc::new(MockBackend::new().with_output("Building...\nDo you trust this file?"));
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

        // Session should be idle (waiting pattern detected immediately)
        let updated = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, SessionStatus::Idle);

        // SSE event should have been emitted
        let event = rx.try_recv().expect("should receive idle SSE event");
        match event {
            PulpoEvent::Session(se) => {
                assert_eq!(se.status, "idle");
                assert_eq!(se.previous_status, Some("active".into()));
                assert_eq!(se.session_name, "idle-sse");
                assert!(se.output_snippet.is_some());
            }
            PulpoEvent::SessionDeleted(_) => panic!("expected session event"),
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
            .update_session_status(&session.id.to_string(), SessionStatus::Idle)
            .await
            .unwrap();
        session.status = SessionStatus::Idle;
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
        assert_eq!(updated.status, SessionStatus::Active);

        // SSE event should have been emitted
        let event = rx.try_recv().expect("should receive active SSE event");
        match event {
            PulpoEvent::Session(se) => {
                assert_eq!(se.status, "active");
                assert_eq!(se.previous_status, Some("idle".into()));
                assert_eq!(se.session_name, "active-sse");
            }
            PulpoEvent::SessionDeleted(_) => panic!("expected session event"),
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
            status: SessionStatus::Active,
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
            command: "echo hello".into(),
            description: Some("test".into()),
            status: SessionStatus::Active,
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

        // Session with backend_session_id = None (falls back to session name)
        let session = Session {
            id: uuid::Uuid::new_v4(),
            name: "no-tmux".into(),
            workdir: "/tmp/repo".into(),
            command: "echo hello".into(),
            description: Some("test".into()),
            status: SessionStatus::Active,
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
        assert_eq!(fetched.status, SessionStatus::Idle);
    }

    // ───────────────────────────────────────────────────────────
    // Stale / dead edge-case tests
    // ───────────────────────────────────────────────────────────

    /// Backend that fails kill only for specific session names.
    struct SelectiveKillBackend {
        fail_names: Vec<String>,
        kill_calls: Mutex<Vec<String>>,
    }

    impl SelectiveKillBackend {
        fn new(fail_names: Vec<&str>) -> Self {
            Self {
                fail_names: fail_names.into_iter().map(Into::into).collect(),
                kill_calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl Backend for SelectiveKillBackend {
        fn create_session(&self, _: &str, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn kill_session(&self, name: &str) -> Result<()> {
            self.kill_calls.lock().unwrap().push(name.into());
            if self.fail_names.iter().any(|n| n == name) {
                anyhow::bail!("selective kill failed for {name}");
            }
            Ok(())
        }
        fn is_alive(&self, _: &str) -> Result<bool> {
            Ok(true)
        }
        fn capture_output(&self, _: &str, _: usize) -> Result<String> {
            Ok("output".into())
        }
        fn send_input(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn setup_logging(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_intervene_partial_kill_failure() {
        // When multiple sessions are running and one kill fails, the other
        // should still be stopped and recorded as an intervention.
        let backend = Arc::new(SelectiveKillBackend::new(vec!["fail-session"]));
        let store = test_store().await;

        create_running_session(&store, "success-session").await;
        create_running_session(&store, "fail-session").await;

        let snapshot = MemorySnapshot {
            available_mb: 100,
            total_mb: 8192,
        };

        intervene(&(backend.clone() as Arc<dyn Backend>), &store, &snapshot).await;

        // Both sessions should have been attempted
        let call_count = backend.kill_calls.lock().unwrap().len();
        assert_eq!(call_count, 2);

        // success-session should be Dead with intervention reason
        let success = store.get_session("success-session").await.unwrap().unwrap();
        assert_eq!(success.status, SessionStatus::Stopped);
        assert!(success.intervention_reason.is_some());

        // fail-session should remain Running (kill failed)
        let fail = store.get_session("fail-session").await.unwrap().unwrap();
        assert_eq!(fail.status, SessionStatus::Active);
        assert!(fail.intervention_reason.is_none());
    }

    #[tokio::test]
    async fn test_intervene_skips_non_running_sessions() {
        // intervene() should only kill Running sessions, ignoring Dead/Stale/Completed.
        let backend = Arc::new(MockBackend::new());
        let store = test_store().await;

        let running = create_running_session(&store, "running-one").await;
        let stale = create_running_session(&store, "stale-one").await;
        store
            .update_session_status(&stale.id.to_string(), SessionStatus::Lost)
            .await
            .unwrap();

        let snapshot = MemorySnapshot {
            available_mb: 100,
            total_mb: 8192,
        };

        intervene(&(backend.clone() as Arc<dyn Backend>), &store, &snapshot).await;

        // Only running-one should be stopped
        let kills: Vec<String> = backend.kill_calls.lock().unwrap().clone();
        assert_eq!(kills.len(), 1);
        assert_eq!(kills[0], "running-one");

        // Verify statuses
        let r = store
            .get_session(&running.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(r.status, SessionStatus::Stopped);

        let s = store
            .get_session(&stale.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(s.status, SessionStatus::Lost);
    }

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
            status: SessionStatus::Active,
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
            "vanishing",
            now,
            timeout,
        )
        .await;
    }

    #[tokio::test]
    async fn test_watchdog_live_config_reload_threshold() {
        // Start with high threshold (95) — no intervention should happen
        let backend = Arc::new(MockBackend::new());
        let store = test_store().await;
        let session = create_running_session(&store, "reload-test").await;
        // 90% usage — below 95 threshold initially
        let reader = MockMemoryReader::new(vec![
            MemorySnapshot {
                available_mb: 820,
                total_mb: 8192,
            },
            MemorySnapshot {
                available_mb: 820,
                total_mb: 8192,
            },
            MemorySnapshot {
                available_mb: 820,
                total_mb: 8192,
            },
            MemorySnapshot {
                available_mb: 820,
                total_mb: 8192,
            },
        ]);

        let (config_tx, config_rx) = make_config_with_tx(
            95,
            Duration::from_millis(10),
            1,
            IdleConfig {
                enabled: false,
                ..IdleConfig::default()
            },
        );
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        let handle = tokio::spawn(run_watchdog_loop(
            backend.clone(),
            store.clone(),
            Box::new(reader),
            config_rx,
            shutdown_rx,
            test_ready_ctx(),
        ));

        // Let it run a tick with high threshold — no intervention
        time::sleep(Duration::from_millis(30)).await;
        assert!(backend.kill_calls.lock().unwrap().is_empty());

        // Lower threshold to 80 — now 90% usage should trigger intervention
        config_tx
            .send(WatchdogRuntimeConfig {
                threshold: 80,
                interval: Duration::from_millis(10),
                breach_count: 1,
                idle: IdleConfig {
                    enabled: false,
                    ..IdleConfig::default()
                },
                ready_ttl_secs: 0,
                adopt_tmux: false,
                extra_waiting_patterns: Vec::new(),
            })
            .unwrap();

        time::sleep(Duration::from_millis(30)).await;
        shutdown_tx.send(true).unwrap();
        handle.await.unwrap();

        // Now a kill should have happened because threshold was lowered
        assert!(
            !backend.kill_calls.lock().unwrap().is_empty(),
            "Expected kill after threshold lowered"
        );
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Stopped);
    }

    #[tokio::test]
    async fn test_watchdog_runtime_config_debug() {
        let cfg = WatchdogRuntimeConfig {
            threshold: 90,
            interval: Duration::from_secs(10),
            breach_count: 3,
            idle: IdleConfig::default(),
            ready_ttl_secs: 0,
            adopt_tmux: false,
            extra_waiting_patterns: Vec::new(),
        };
        let debug = format!("{cfg:?}");
        assert!(debug.contains("90"));
        assert!(debug.contains("breach_count"));
    }

    #[tokio::test]
    async fn test_watchdog_runtime_config_clone() {
        let cfg = WatchdogRuntimeConfig {
            threshold: 80,
            interval: Duration::from_secs(5),
            breach_count: 2,
            idle: IdleConfig {
                enabled: true,
                timeout_secs: 300,
                action: IdleAction::Kill,
                threshold_secs: 60,
            },
            ready_ttl_secs: 0,
            adopt_tmux: false,
            extra_waiting_patterns: Vec::new(),
        };
        #[allow(clippy::redundant_clone)]
        let cloned = cfg.clone();
        assert_eq!(cloned.threshold, 80);
        assert_eq!(cloned.interval, Duration::from_secs(5));
        assert_eq!(cloned.breach_count, 2);
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
        let backend =
            Arc::new(MockBackend::new().with_output("Building...\nDo you trust this file?"));
        let store = test_store().await;

        // Create an Active session whose output_snapshot matches mock output
        // (so output_changed == false, triggering the waiting-for-input check)
        let session = Session {
            id: uuid::Uuid::new_v4(),
            name: "active-to-idle".into(),
            workdir: "/tmp/repo".into(),
            command: "echo hello".into(),
            description: Some("test".into()),
            status: SessionStatus::Active,
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
        assert_eq!(fetched.status, SessionStatus::Idle);
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
            status: SessionStatus::Idle,
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
        assert_eq!(fetched.status, SessionStatus::Active);
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

        let active_session = make_idle_test_session("active-one", SessionStatus::Active, None);
        let idle_since = Some(chrono::Utc::now() - chrono::Duration::seconds(100));
        let idle_session = make_idle_test_session("idle-one", SessionStatus::Idle, idle_since);
        let dead_session = make_idle_test_session("dead-one", SessionStatus::Stopped, None);

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
        assert_eq!(fetched_active.status, SessionStatus::Idle);

        let fetched_idle = store
            .get_session(&idle_session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched_idle.status, SessionStatus::Idle);

        // Dead session should NOT have been processed
        assert!(
            !backend
                .capture_calls
                .lock()
                .unwrap()
                .contains(&"dead-one".to_string())
        );
    }

    // --- S3: Agent exit / Ready detection tests ---

    #[test]
    fn test_detect_agent_exited_present() {
        let output = "doing work...\n[pulpo] Agent exited\n$ ";
        assert!(detect_agent_exited(output));
    }

    #[test]
    fn test_detect_agent_exited_absent() {
        let output = "doing work...\nsome other output\n$ ";
        assert!(!detect_agent_exited(output));
    }

    #[test]
    fn test_detect_agent_exited_empty() {
        assert!(!detect_agent_exited(""));
    }

    #[test]
    fn test_detect_agent_exited_partial() {
        // Should NOT match partial marker
        assert!(!detect_agent_exited("[pulpo] Agent"));
        assert!(!detect_agent_exited("Agent exited"));
    }

    #[tokio::test]
    async fn test_ready_transition_on_agent_exit() {
        // Backend returns output containing agent exit marker
        let backend =
            Arc::new(MockBackend::new().with_output("work done\n[pulpo] Agent exited\n$ "));
        let store = test_store().await;
        let session = create_running_session(&store, "finish-me").await;

        let idle_config = IdleConfig {
            enabled: true,
            timeout_secs: 600,
            action: IdleAction::Alert,
            threshold_secs: 60,
        };

        let dyn_backend: Arc<dyn Backend> = backend;
        check_idle_sessions(&dyn_backend, &store, &idle_config, &test_ready_ctx(), &[]).await;

        // Session should now be Ready
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Ready);
    }

    #[tokio::test]
    async fn test_ready_transition_emits_event() {
        let backend = Arc::new(MockBackend::new().with_output("done\n[pulpo] Agent exited\n$ "));
        let store = test_store().await;
        let session = create_running_session(&store, "event-me").await;

        let (event_tx, mut event_rx) = broadcast::channel::<PulpoEvent>(16);
        let ctx = ReadyContext {
            event_tx: Some(event_tx),
            node_name: "test-node".into(),
        };

        let idle_config = IdleConfig {
            enabled: true,
            timeout_secs: 600,
            action: IdleAction::Alert,
            threshold_secs: 60,
        };

        let dyn_backend: Arc<dyn Backend> = backend;
        check_idle_sessions(&dyn_backend, &store, &idle_config, &ctx, &[]).await;

        // Should have received a Ready event
        let event = event_rx.try_recv().unwrap();
        match event {
            PulpoEvent::Session(se) => {
                assert_eq!(se.session_id, session.id.to_string());
                assert_eq!(se.status, "ready");
                assert_eq!(se.previous_status, Some("active".into()));
                assert_eq!(se.node_name, "test-node");
            }
            PulpoEvent::SessionDeleted(_) => panic!("expected session event"),
        }
    }

    #[tokio::test]
    async fn test_ready_skips_idle_logic() {
        // If agent exited, session should NOT go through idle detection
        let backend = Arc::new(MockBackend::new().with_output("[pulpo] Agent exited"));
        let store = test_store().await;
        let session = create_running_session(&store, "skip-idle").await;
        // Set old last_output_at so it would normally trigger idle
        store
            .update_session_idle_since(&session.id.to_string())
            .await
            .unwrap();

        let idle_config = IdleConfig {
            enabled: true,
            timeout_secs: 1, // very short, would trigger idle action
            action: IdleAction::Kill,
            threshold_secs: 60,
        };

        let dyn_backend: Arc<dyn Backend> = backend;
        check_idle_sessions(&dyn_backend, &store, &idle_config, &test_ready_ctx(), &[]).await;

        // Should be Ready, NOT Stopped
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Ready);
    }

    #[tokio::test]
    async fn test_ready_from_idle_state() {
        // An Idle session should also transition to Ready if agent exits
        let backend =
            Arc::new(MockBackend::new().with_output("waiting...\n[pulpo] Agent exited\n$ "));
        let store = test_store().await;
        let mut session = create_running_session(&store, "idle-to-finish").await;
        // Mark as Idle first
        store
            .update_session_status(&session.id.to_string(), SessionStatus::Idle)
            .await
            .unwrap();
        session.status = SessionStatus::Idle;

        let (event_tx, mut event_rx) = broadcast::channel::<PulpoEvent>(16);
        let ctx = ReadyContext {
            event_tx: Some(event_tx),
            node_name: "n".into(),
        };

        let idle_config = IdleConfig {
            enabled: true,
            timeout_secs: 600,
            action: IdleAction::Alert,
            threshold_secs: 60,
        };

        let dyn_backend: Arc<dyn Backend> = backend;
        check_idle_sessions(&dyn_backend, &store, &idle_config, &ctx, &[]).await;

        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Ready);

        // Event should say previous was "idle"
        let event = event_rx.try_recv().unwrap();
        match event {
            PulpoEvent::Session(se) => {
                assert_eq!(se.previous_status, Some("idle".into()));
            }
            PulpoEvent::SessionDeleted(_) => panic!("expected session event"),
        }
    }

    // --- S4: Ready TTL cleanup tests ---

    #[tokio::test]
    async fn test_cleanup_ready_sessions_kills_expired() {
        let backend = Arc::new(MockBackend::new());
        let store = test_store().await;
        let session = create_running_session(&store, "expired").await;

        // Mark as Ready with old updated_at
        store
            .update_session_status(&session.id.to_string(), SessionStatus::Ready)
            .await
            .unwrap();
        // Manually set updated_at to 2 hours ago
        sqlx::query("UPDATE sessions SET updated_at = ? WHERE id = ?")
            .bind((chrono::Utc::now() - chrono::Duration::seconds(7200)).to_rfc3339())
            .bind(session.id.to_string())
            .execute(store.pool())
            .await
            .unwrap();

        let dyn_backend: Arc<dyn Backend> = backend.clone();
        cleanup_ready_sessions(&dyn_backend, &store, 3600).await; // TTL = 1 hour

        // Should be Stopped now
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Stopped);
        // Backend kill should have been called
        assert!(
            backend
                .kill_calls
                .lock()
                .unwrap()
                .contains(&"expired".to_string())
        );
    }

    #[tokio::test]
    async fn test_cleanup_ready_sessions_skips_recent() {
        let backend = Arc::new(MockBackend::new());
        let store = test_store().await;
        let session = create_running_session(&store, "recent").await;

        // Mark as Ready (just now, so within TTL)
        store
            .update_session_status(&session.id.to_string(), SessionStatus::Ready)
            .await
            .unwrap();

        let dyn_backend: Arc<dyn Backend> = backend.clone();
        cleanup_ready_sessions(&dyn_backend, &store, 3600).await; // TTL = 1 hour

        // Should still be Ready
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Ready);
        assert!(backend.kill_calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_cleanup_ready_sessions_ignores_active() {
        let backend = Arc::new(MockBackend::new());
        let store = test_store().await;
        let _session = create_running_session(&store, "active-one").await;

        let dyn_backend: Arc<dyn Backend> = backend.clone();
        cleanup_ready_sessions(&dyn_backend, &store, 1).await; // TTL = 1 sec

        // Should still be Active (cleanup only targets Ready)
        assert!(backend.kill_calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_cleanup_ready_kill_failure_still_marks_stopped() {
        // Even if backend.kill_session fails (tmux already gone), status should update
        let backend = Arc::new(MockBackend::failing_kill());
        let store = test_store().await;
        let session = create_running_session(&store, "gone").await;

        store
            .update_session_status(&session.id.to_string(), SessionStatus::Ready)
            .await
            .unwrap();
        // Set updated_at to 2 hours ago
        sqlx::query("UPDATE sessions SET updated_at = ? WHERE id = ?")
            .bind((chrono::Utc::now() - chrono::Duration::seconds(7200)).to_rfc3339())
            .bind(session.id.to_string())
            .execute(store.pool())
            .await
            .unwrap();

        let dyn_backend: Arc<dyn Backend> = backend;
        cleanup_ready_sessions(&dyn_backend, &store, 3600).await;

        // Should still be marked as Stopped even though backend.kill failed
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Stopped);
    }

    #[test]
    fn test_agent_exit_marker_constant() {
        assert_eq!(AGENT_EXIT_MARKER, "[pulpo] Agent exited");
    }

    #[tokio::test]
    async fn test_ready_transitions_to_ready() {
        let store = test_store().await;
        let session = create_running_session(&store, "finish-test").await;

        let ctx = ReadyContext {
            event_tx: None,
            node_name: "n".into(),
        };
        handle_session_ready(&store, &session, &ctx).await;

        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Ready);
    }

    #[tokio::test]
    async fn test_cleanup_ready_no_ready_sessions() {
        let backend = Arc::new(MockBackend::new());
        let store = test_store().await;

        // No sessions at all
        let dyn_backend: Arc<dyn Backend> = backend.clone();
        cleanup_ready_sessions(&dyn_backend, &store, 3600).await;

        assert!(backend.kill_calls.lock().unwrap().is_empty());
    }

    // --- S5: classify_adopted_process tests ---

    #[test]
    fn test_classify_agent_processes() {
        assert_eq!(classify_adopted_process("claude"), SessionStatus::Active);
        assert_eq!(classify_adopted_process("codex"), SessionStatus::Active);
        assert_eq!(classify_adopted_process("gemini"), SessionStatus::Active);
        assert_eq!(classify_adopted_process("opencode"), SessionStatus::Active);
    }

    #[test]
    fn test_classify_agent_case_insensitive() {
        assert_eq!(classify_adopted_process("Claude"), SessionStatus::Active);
        assert_eq!(classify_adopted_process("CODEX"), SessionStatus::Active);
    }

    #[test]
    fn test_classify_shell_processes() {
        assert_eq!(classify_adopted_process("bash"), SessionStatus::Ready);
        assert_eq!(classify_adopted_process("zsh"), SessionStatus::Ready);
        assert_eq!(classify_adopted_process("sh"), SessionStatus::Ready);
        assert_eq!(classify_adopted_process("fish"), SessionStatus::Ready);
        assert_eq!(classify_adopted_process("nu"), SessionStatus::Ready);
    }

    #[test]
    fn test_classify_unknown_process() {
        // Unknown processes are conservatively Active
        assert_eq!(classify_adopted_process("python"), SessionStatus::Active);
        assert_eq!(classify_adopted_process("node"), SessionStatus::Active);
    }

    // --- S6: adopt_tmux_sessions tests ---

    #[tokio::test]
    async fn test_adopt_no_tmux_sessions() {
        let backend = Arc::new(MockBackend::new());
        let store = test_store().await;
        let ctx = test_ready_ctx();

        let dyn_backend: Arc<dyn Backend> = backend;
        adopt_tmux_sessions(&dyn_backend, &store, &ctx).await;

        let sessions = store.list_sessions().await.unwrap();
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn test_adopt_skips_known_sessions() {
        let mut backend = MockBackend::new();
        backend.tmux_sessions = vec![("$0".into(), "existing".into())];
        backend
            .pane_infos
            .insert("existing".into(), ("bash".into(), "/tmp".into()));
        let backend = Arc::new(backend);
        let store = test_store().await;
        // Create a session that has backend_session_id matching a tmux name
        let _session = create_running_session(&store, "existing").await;

        let ctx = test_ready_ctx();
        let dyn_backend: Arc<dyn Backend> = backend;
        adopt_tmux_sessions(&dyn_backend, &store, &ctx).await;

        // Only the original session should exist (not adopted again)
        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
    }

    #[tokio::test]
    async fn test_adopt_agent_session_as_active() {
        let mut backend = MockBackend::new();
        backend.tmux_sessions = vec![("$0".into(), "my-claude".into())];
        backend
            .pane_infos
            .insert("my-claude".into(), ("claude".into(), "/home/user".into()));
        let backend = Arc::new(backend);
        let store = test_store().await;

        let ctx = test_ready_ctx();
        let dyn_backend: Arc<dyn Backend> = backend;
        adopt_tmux_sessions(&dyn_backend, &store, &ctx).await;

        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "my-claude");
        assert_eq!(sessions[0].status, SessionStatus::Active);
        assert_eq!(sessions[0].command, "claude");
        assert_eq!(sessions[0].workdir, "/home/user");
        assert_eq!(sessions[0].description, Some("Adopted from tmux".into()));
        assert_eq!(sessions[0].backend_session_id, Some("$0".into()));
    }

    #[tokio::test]
    async fn test_adopt_shell_session_as_ready() {
        let mut backend = MockBackend::new();
        backend.tmux_sessions = vec![("$0".into(), "bare-shell".into())];
        backend
            .pane_infos
            .insert("bare-shell".into(), ("bash".into(), "/tmp".into()));
        let backend = Arc::new(backend);
        let store = test_store().await;

        let ctx = test_ready_ctx();
        let dyn_backend: Arc<dyn Backend> = backend;
        adopt_tmux_sessions(&dyn_backend, &store, &ctx).await;

        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].status, SessionStatus::Ready);
        assert_eq!(sessions[0].command, "bash");
    }

    #[tokio::test]
    async fn test_adopt_emits_sse_event() {
        let mut backend = MockBackend::new();
        backend.tmux_sessions = vec![("$0".into(), "event-session".into())];
        backend
            .pane_infos
            .insert("event-session".into(), ("codex".into(), "/repo".into()));
        let backend = Arc::new(backend);
        let store = test_store().await;

        let (event_tx, mut event_rx) = broadcast::channel::<PulpoEvent>(16);
        let ctx = ReadyContext {
            event_tx: Some(event_tx),
            node_name: "test-node".into(),
        };

        let dyn_backend: Arc<dyn Backend> = backend;
        adopt_tmux_sessions(&dyn_backend, &store, &ctx).await;

        let event = event_rx.try_recv().unwrap();
        match event {
            PulpoEvent::Session(se) => {
                assert_eq!(se.session_name, "event-session");
                assert_eq!(se.status, "active");
                assert!(se.previous_status.is_none());
                assert_eq!(se.node_name, "test-node");
            }
            PulpoEvent::SessionDeleted(_) => panic!("expected session event"),
        }
    }

    #[tokio::test]
    async fn test_adopt_skips_pane_info_failure() {
        let mut backend = MockBackend::new();
        backend.tmux_sessions = vec![("$0".into(), "no-info".into())];
        // No pane_info entry → pane_info will return error
        let backend = Arc::new(backend);
        let store = test_store().await;

        let ctx = test_ready_ctx();
        let dyn_backend: Arc<dyn Backend> = backend;
        adopt_tmux_sessions(&dyn_backend, &store, &ctx).await;

        // Should not have adopted (pane_info failed)
        let sessions = store.list_sessions().await.unwrap();
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn test_adopt_skips_claude_teammate_sessions() {
        let mut backend = MockBackend::new();
        // Claude teammate sessions have names like "claude-<hex>" (long names)
        backend.tmux_sessions = vec![
            ("$0".into(), "claude-abc123def456".into()),
            ("$1".into(), "my-session".into()),
        ];
        backend
            .pane_infos
            .insert("my-session".into(), ("claude".into(), "/repo".into()));
        // No pane_info for claude-abc123def456 — it should be skipped before pane_info is called
        let backend = Arc::new(backend);
        let store = test_store().await;

        let ctx = test_ready_ctx();
        let dyn_backend: Arc<dyn Backend> = backend;
        adopt_tmux_sessions(&dyn_backend, &store, &ctx).await;

        // Only my-session should be adopted, not the claude teammate session
        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "my-session");
    }

    #[tokio::test]
    async fn test_adopt_allows_short_claude_names() {
        // Short names like "claude" or "claude-pr" should NOT be skipped
        let mut backend = MockBackend::new();
        backend.tmux_sessions = vec![("$0".into(), "claude-pr".into())];
        backend
            .pane_infos
            .insert("claude-pr".into(), ("claude".into(), "/repo".into()));
        let backend = Arc::new(backend);
        let store = test_store().await;

        let ctx = test_ready_ctx();
        let dyn_backend: Arc<dyn Backend> = backend;
        adopt_tmux_sessions(&dyn_backend, &store, &ctx).await;

        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "claude-pr");
    }

    #[tokio::test]
    async fn test_adopt_multiple_sessions() {
        let mut backend = MockBackend::new();
        backend.tmux_sessions = vec![
            ("$0".into(), "agent-1".into()),
            ("$1".into(), "shell-1".into()),
        ];
        backend
            .pane_infos
            .insert("agent-1".into(), ("claude".into(), "/code".into()));
        backend
            .pane_infos
            .insert("shell-1".into(), ("zsh".into(), "/home".into()));
        let backend = Arc::new(backend);
        let store = test_store().await;

        let ctx = test_ready_ctx();
        let dyn_backend: Arc<dyn Backend> = backend;
        adopt_tmux_sessions(&dyn_backend, &store, &ctx).await;

        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[tokio::test]
    async fn test_adopt_skips_by_live_name() {
        // Ready session with same name should prevent adoption
        let mut backend = MockBackend::new();
        backend.tmux_sessions = vec![("$0".into(), "my-session".into())];
        backend
            .pane_infos
            .insert("my-session".into(), ("bash".into(), "/tmp".into()));
        let backend = Arc::new(backend);
        let store = test_store().await;
        // Create a ready session with the same name
        let mut session = create_running_session(&store, "my-session").await;
        store
            .update_session_status(&session.id.to_string(), SessionStatus::Ready)
            .await
            .unwrap();
        session.status = SessionStatus::Ready;

        let ctx = test_ready_ctx();
        let dyn_backend: Arc<dyn Backend> = backend;
        adopt_tmux_sessions(&dyn_backend, &store, &ctx).await;

        // Should still be just 1 session
        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
    }

    #[tokio::test]
    async fn test_adopt_ghost_fix_stopped_session_does_not_block() {
        // A stopped session with old backend_session_id should NOT block adoption
        // of a new tmux session with the same name.
        let mut backend = MockBackend::new();
        backend.tmux_sessions = vec![("$5".into(), "reused-name".into())];
        backend
            .pane_infos
            .insert("reused-name".into(), ("claude".into(), "/repo".into()));
        let backend = Arc::new(backend);
        let store = test_store().await;

        // Create a stopped session with the same name and old backend_session_id
        let stopped_session = Session {
            id: uuid::Uuid::new_v4(),
            name: "reused-name".into(),
            workdir: "/old".into(),
            command: "old-command".into(),
            status: SessionStatus::Stopped,
            backend_session_id: Some("reused-name".into()),
            ..Default::default()
        };
        store.insert_session(&stopped_session).await.unwrap();

        let ctx = test_ready_ctx();
        let dyn_backend: Arc<dyn Backend> = backend;
        adopt_tmux_sessions(&dyn_backend, &store, &ctx).await;

        // Should have 2 sessions: the old stopped one + the newly adopted one
        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 2);
        let adopted = sessions.iter().find(|s| s.status == SessionStatus::Active);
        assert!(adopted.is_some(), "new session should be adopted");
        let adopted = adopted.unwrap();
        assert_eq!(adopted.name, "reused-name");
        assert_eq!(adopted.backend_session_id, Some("$5".into()));
    }

    #[tokio::test]
    async fn test_adopt_uses_full_command_line() {
        // When pane_command_line returns a result, adoption uses it as the command
        let mut backend = MockBackend::new();
        backend.tmux_sessions = vec![("$0".into(), "full-cmd".into())];
        backend
            .pane_infos
            .insert("full-cmd".into(), ("claude".into(), "/repo".into()));
        backend.pane_command_lines.insert(
            "$0".into(),
            "claude -p 'review code' --workdir /repo".into(),
        );
        let backend = Arc::new(backend);
        let store = test_store().await;

        let ctx = test_ready_ctx();
        let dyn_backend: Arc<dyn Backend> = backend;
        adopt_tmux_sessions(&dyn_backend, &store, &ctx).await;

        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(
            sessions[0].command,
            "claude -p 'review code' --workdir /repo"
        );
    }

    // -- detect_and_store_output_metadata tests --

    #[tokio::test]
    async fn test_detect_and_store_pr_url() {
        let store = test_store().await;
        let session = create_running_session(&store, "pr-detect").await;

        let output = "Pushing...\nremote: Create a pull request:\nremote:   https://github.com/owner/repo/pull/42\n";
        detect_and_store_output_metadata(&store, &session, output).await;

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
        detect_and_store_output_metadata(&store, &session, output).await;

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
        detect_and_store_output_metadata(&store, &session, output).await;

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
        detect_and_store_output_metadata(&store, &session, output).await;

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
        detect_and_store_output_metadata(&store, &session, output).await;

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
        detect_and_store_output_metadata(&store, &session, output).await;

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
        detect_and_store_output_metadata(&store, &session, output1).await;

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
        detect_and_store_output_metadata(&store, &session2, output2).await;

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
        detect_and_store_output_metadata(&store, &session, output).await;

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
        detect_and_store_output_metadata(&store, &session, output1).await;

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
        detect_and_store_output_metadata(&store, &session2, output2).await;

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
        detect_and_store_output_metadata(&store, &session, output).await;

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
        detect_and_store_output_metadata(&store, &session, output).await;

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
}
