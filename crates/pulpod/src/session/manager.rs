use std::collections::HashMap;
use std::sync::Arc;

#[cfg(not(coverage))]
use anyhow::Context;
use anyhow::{Result, anyhow, bail};
use chrono::Utc;
use pulpo_common::api::CreateSessionRequest;
use pulpo_common::event::{PulpoEvent, SessionEvent};
use pulpo_common::session::{Session, SessionStatus};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::backend::Backend;
use crate::config::InkConfig;
use crate::store::Store;

#[derive(Clone)]
pub struct SessionManager {
    backend: Arc<dyn Backend>,
    sandbox_backend: Option<Arc<dyn Backend>>,
    store: Store,
    inks: HashMap<String, InkConfig>,
    default_command: Option<String>,
    event_tx: Option<broadcast::Sender<PulpoEvent>>,
    node_name: String,
    /// Grace period (seconds) after session creation before staleness checks apply.
    /// Prevents race where `is_alive()` returns false before tmux is fully ready.
    stale_grace_secs: i64,
}

impl SessionManager {
    pub fn new(
        backend: Arc<dyn Backend>,
        store: Store,
        inks: HashMap<String, InkConfig>,
        default_command: Option<String>,
    ) -> Self {
        Self {
            backend,
            sandbox_backend: None,
            store,
            inks,
            default_command,
            event_tx: None,
            node_name: String::new(),
            stale_grace_secs: 5,
        }
    }

    #[must_use]
    pub fn with_sandbox_backend(mut self, backend: Arc<dyn Backend>) -> Self {
        self.sandbox_backend = Some(backend);
        self
    }

    /// Get the right backend for a session based on its `backend_session_id`.
    fn backend_for_id(&self, backend_id: &str) -> &Arc<dyn Backend> {
        if crate::backend::docker::is_docker_session(backend_id) {
            self.sandbox_backend.as_ref().unwrap_or(&self.backend)
        } else {
            &self.backend
        }
    }

    #[cfg(test)]
    #[must_use]
    pub const fn with_no_stale_grace(mut self) -> Self {
        self.stale_grace_secs = 0;
        self
    }

    #[must_use]
    pub fn with_event_tx(mut self, tx: broadcast::Sender<PulpoEvent>, node_name: String) -> Self {
        self.event_tx = Some(tx);
        self.node_name = node_name;
        self
    }

    pub const fn inks(&self) -> &HashMap<String, InkConfig> {
        &self.inks
    }

    pub fn backend(&self) -> Arc<dyn Backend> {
        self.backend.clone()
    }

    fn emit_event(&self, session: &Session, previous_status: Option<SessionStatus>) {
        if let Some(tx) = &self.event_tx {
            let event = SessionEvent {
                session_id: session.id.to_string(),
                session_name: session.name.clone(),
                status: session.status.to_string(),
                previous_status: previous_status.map(|s| s.to_string()),
                node_name: self.node_name.clone(),
                output_snippet: session.output_snapshot.clone(),
                timestamp: Utc::now().to_rfc3339(),
            };
            // Ignore send errors — no subscribers is OK
            let _ = tx.send(PulpoEvent::Session(event));
        }
    }

    /// Resolve the backend session ID for a session.
    /// Uses the stored `backend_session_id` if available, otherwise computes it
    /// from the session name (for legacy sessions created before this field existed).
    pub fn resolve_backend_id(&self, session: &Session) -> String {
        session
            .backend_session_id
            .clone()
            .unwrap_or_else(|| self.backend.session_id(&session.name))
    }

    pub async fn create_session(&self, req: CreateSessionRequest) -> Result<Session> {
        // Resolve ink → get command
        let (command, description) = self.resolve_ink(&req)?;

        // Default workdir to home dir
        let workdir = req.workdir.unwrap_or_else(|| {
            dirs::home_dir().map_or_else(|| "/tmp".to_owned(), |h| h.to_string_lossy().into_owned())
        });
        validate_workdir(&workdir)?;

        // Create git worktree if requested
        let (effective_workdir, worktree_path) = if req.worktree.unwrap_or(false) {
            #[cfg(not(coverage))]
            {
                let wt_path = create_worktree(&workdir, &req.name)?;
                (wt_path.clone(), Some(wt_path))
            }
            #[cfg(coverage)]
            {
                (workdir.clone(), None)
            }
        } else {
            (workdir.clone(), None)
        };

        // Reject duplicate names among live sessions
        if self.store.has_active_session_by_name(&req.name).await? {
            bail!(
                "a session named '{}' is already active — kill it first or use a different name",
                req.name
            );
        }

        let id = Uuid::new_v4();
        let name = req.name.clone();
        let is_sandbox = req.sandbox.unwrap_or(false);
        let backend_id = if is_sandbox {
            if self.sandbox_backend.is_none() {
                bail!("sandbox not configured — set [sandbox] image in config.toml");
            }
            format!("docker:pulpo-{}", req.name)
        } else {
            self.backend.session_id(&name)
        };

        // Sandbox sessions run the command directly; tmux sessions get the wrapper
        let final_command = if is_sandbox {
            command.clone()
        } else {
            wrap_command(&command, &id, &name)
        };

        let now = Utc::now();
        let session = Session {
            id,
            name: name.clone(),
            workdir: effective_workdir,
            command,
            description,
            status: SessionStatus::Creating,
            exit_code: None,
            backend_session_id: Some(backend_id.clone()),
            output_snapshot: None,
            metadata: req.metadata,
            ink: req.ink,
            intervention_code: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            idle_threshold_secs: req.idle_threshold_secs,
            worktree_path,
            sandbox: is_sandbox,
            created_at: now,
            updated_at: now,
        };

        self.store.insert_session(&session).await?;

        let active_backend = self.backend_for_id(&backend_id);
        if let Err(e) = active_backend.create_session(&backend_id, &session.workdir, &final_command)
        {
            self.store
                .update_session_status(&id.to_string(), SessionStatus::Killed)
                .await?;
            return Err(e);
        }

        // Query the tmux $N session ID and update if available (tmux only)
        let mut session = session;
        if !is_sandbox && let Ok(tmux_id) = self.backend.query_backend_id(&name) {
            let _ = self
                .store
                .update_backend_session_id(&id.to_string(), &tmux_id)
                .await;
            session.backend_session_id = Some(tmux_id);
        }

        self.store
            .update_session_status(&id.to_string(), SessionStatus::Active)
            .await?;

        // Set up output logging
        let log_dir = format!("{}/logs", self.store.data_dir());
        let _ = std::fs::create_dir_all(&log_dir);
        let log_path = format!("{log_dir}/{id}.log");
        let _ = active_backend.setup_logging(&backend_id, &log_path);

        // Return the session with updated status (avoids unnecessary re-fetch)
        session.status = SessionStatus::Active;
        session.updated_at = Utc::now();
        self.emit_event(&session, Some(SessionStatus::Creating));
        Ok(session)
    }

    /// Resolve ink: if ink has command, use it. Request command takes precedence.
    fn resolve_ink(&self, req: &CreateSessionRequest) -> Result<(String, Option<String>)> {
        // If a command is explicitly provided, use it
        if let Some(ref cmd) = req.command {
            return Ok((cmd.clone(), req.description.clone()));
        }

        // If an ink is specified, resolve it
        if let Some(ref ink_name) = req.ink {
            let ink = self
                .inks
                .get(ink_name)
                .ok_or_else(|| anyhow!("unknown ink: {ink_name}"))?;
            let command = ink.command.clone().unwrap_or_default();
            // Ink description as fallback for session description
            let description = req.description.clone().or_else(|| ink.description.clone());
            if !command.is_empty() {
                return Ok((command, description));
            }
            // Ink has no command — fall through to default_command / $SHELL
        }

        // No command and no ink — fall back to default_command from config
        if let Some(ref default_cmd) = self.default_command {
            return Ok((default_cmd.clone(), req.description.clone()));
        }

        // No fallback available — fall back to $SHELL (or /bin/sh)
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned());
        Ok((shell, req.description.clone()))
    }

    pub async fn get_session(&self, id: &str) -> Result<Option<Session>> {
        let session = self.store.get_session(id).await?;
        match session {
            Some(mut s) => {
                if self.check_and_mark_stale(&mut s).await? {
                    self.emit_event(&s, Some(SessionStatus::Active));
                }
                Ok(Some(s))
            }
            None => Ok(None),
        }
    }

    pub async fn list_sessions(&self) -> Result<Vec<Session>> {
        let mut sessions = self.store.list_sessions().await?;
        for session in &mut sessions {
            let _ = self.check_and_mark_stale(session).await;
        }
        Ok(sessions)
    }

    pub async fn list_sessions_filtered(
        &self,
        query: &pulpo_common::api::ListSessionsQuery,
    ) -> Result<Vec<Session>> {
        let mut sessions = self.store.list_sessions_filtered(query).await?;
        for session in &mut sessions {
            let _ = self.check_and_mark_stale(session).await;
        }
        Ok(sessions)
    }

    /// Check if a running session is still alive; if not, mark it stale.
    /// Returns `Ok(true)` if the session was transitioned to stale.
    ///
    /// Checks both `Active` and `Idle` sessions — after a reboot, tmux
    /// sessions are gone but DB status may still say Idle.
    async fn check_and_mark_stale(&self, session: &mut Session) -> Result<bool> {
        if session.status != SessionStatus::Active && session.status != SessionStatus::Idle {
            return Ok(false);
        }
        // Grace period: skip staleness check for recently created sessions to avoid a
        // race where `is_alive()` returns false before tmux is fully ready.
        let age = Utc::now() - session.created_at;
        if age.num_seconds() < self.stale_grace_secs {
            return Ok(false);
        }
        let backend_id = self.resolve_backend_id(session);
        let alive = self.backend_for_id(&backend_id).is_alive(&backend_id)?;
        if alive {
            return Ok(false);
        }
        self.store
            .update_session_status(&session.id.to_string(), SessionStatus::Lost)
            .await?;
        session.status = SessionStatus::Lost;
        Ok(true)
    }

    pub async fn kill_session(&self, id: &str) -> Result<()> {
        let session = self
            .store
            .get_session(id)
            .await?
            .ok_or_else(|| anyhow!("session not found: {id}"))?;

        let backend_id = self.resolve_backend_id(&session);
        let backend = self.backend_for_id(&backend_id);
        if let Err(e) = backend.kill_session(&backend_id) {
            bail!("failed to kill session: {e}");
        }

        let previous = session.status;
        let session_id = session.id.to_string();
        self.store
            .update_session_status(&session_id, SessionStatus::Killed)
            .await?;
        let mut dead_session = session;
        dead_session.status = SessionStatus::Killed;
        // Clean up worktree if this was a worktree session
        if let Some(ref wt_path) = dead_session.worktree_path {
            cleanup_worktree(wt_path);
        }
        self.emit_event(&dead_session, Some(previous));
        Ok(())
    }

    pub async fn delete_session(&self, id: &str) -> Result<()> {
        let session = self
            .store
            .get_session(id)
            .await?
            .ok_or_else(|| anyhow!("session not found: {id}"))?;

        match session.status {
            SessionStatus::Active | SessionStatus::Creating => {
                bail!(
                    "cannot delete session in '{}' state — kill it first",
                    session.status
                );
            }
            _ => {}
        }

        // Best-effort cleanup of any lingering backend session
        let backend_id = self.resolve_backend_id(&session);
        let _ = self.backend_for_id(&backend_id).kill_session(&backend_id);

        self.store.delete_session(&session.id.to_string()).await?;
        // Clean up worktree if this was a worktree session
        if let Some(ref wt_path) = session.worktree_path {
            cleanup_worktree(wt_path);
        }
        Ok(())
    }

    pub fn capture_output(&self, id: &str, backend_id: &str, lines: usize) -> String {
        self.backend
            .capture_output(backend_id, lines)
            .unwrap_or_else(|_| self.read_log_tail(id, lines))
    }

    fn read_log_tail(&self, id: &str, lines: usize) -> String {
        let log_path = format!("{}/logs/{id}.log", self.store.data_dir());
        let content = std::fs::read_to_string(&log_path).unwrap_or_default();
        let mut tail: Vec<&str> = content.lines().rev().take(lines).collect();
        tail.reverse();
        tail.join("\n")
    }

    pub fn send_input(&self, backend_id: &str, text: &str) -> Result<()> {
        self.backend.send_input(backend_id, text)
    }

    pub async fn resume_session(&self, id: &str) -> Result<Session> {
        let session = self
            .store
            .get_session(id)
            .await?
            .ok_or_else(|| anyhow!("session not found: {id}"))?;

        let previous_status = session.status;
        if previous_status != SessionStatus::Lost && previous_status != SessionStatus::Ready {
            bail!("session cannot be resumed (status: {previous_status})");
        }

        // Check for name collision with another live session (exclude self)
        if self
            .store
            .has_active_session_by_name_excluding(&session.name, Some(&session.id.to_string()))
            .await?
        {
            bail!(
                "another session named '{}' is already active — kill it first before resuming",
                session.name
            );
        }

        // If the backend session is still alive, just re-mark it as running.
        // Only recreate the session if the backend process is gone.
        let backend_id = self.resolve_backend_id(&session);
        let active_backend = self.backend_for_id(&backend_id);
        let alive = active_backend.is_alive(&backend_id)?;
        if !alive {
            let final_command = if session.sandbox {
                session.command.clone()
            } else {
                wrap_command(&session.command, &session.id, &session.name)
            };
            active_backend.create_session(&backend_id, &session.workdir, &final_command)?;

            // Query the new tmux $N session ID (tmux only)
            if !session.sandbox
                && let Ok(tmux_id) = self.backend.query_backend_id(&session.name)
            {
                let _ = self
                    .store
                    .update_backend_session_id(&session.id.to_string(), &tmux_id)
                    .await;
            }
        }

        let session_id = session.id.to_string();
        self.store
            .update_session_status(&session_id, SessionStatus::Active)
            .await?;

        let mut session = session;
        session.status = SessionStatus::Active;
        session.updated_at = Utc::now();
        self.emit_event(&session, Some(previous_status));
        Ok(session)
    }

    /// Resume all sessions that were Active or Idle but have dead backends.
    /// Called on startup to recover sessions lost during a reboot.
    /// Returns the number of sessions successfully resumed.
    pub async fn resume_lost_sessions(&self) -> Result<usize> {
        let sessions = self.store.list_sessions().await?;
        let mut resumed = 0;
        for session in sessions {
            if session.status != SessionStatus::Active && session.status != SessionStatus::Idle {
                continue;
            }
            let backend_id = self.resolve_backend_id(&session);
            let active_backend = self.backend_for_id(&backend_id);
            let alive = active_backend.is_alive(&backend_id).unwrap_or(false);
            if alive {
                continue;
            }
            // Backend is dead — resume the session
            let final_command = if session.sandbox {
                session.command.clone()
            } else {
                wrap_command(&session.command, &session.id, &session.name)
            };
            if let Err(e) =
                active_backend.create_session(&backend_id, &session.workdir, &final_command)
            {
                tracing::warn!(
                    session = %session.name,
                    error = %e,
                    "Failed to auto-resume session on startup"
                );
                self.store
                    .update_session_status(&session.id.to_string(), SessionStatus::Lost)
                    .await?;
                continue;
            }

            // Query the new tmux $N session ID (tmux only)
            if !session.sandbox
                && let Ok(tmux_id) = self.backend.query_backend_id(&session.name)
            {
                let _ = self
                    .store
                    .update_backend_session_id(&session.id.to_string(), &tmux_id)
                    .await;
            }

            // Re-mark as Active
            self.store
                .update_session_status(&session.id.to_string(), SessionStatus::Active)
                .await?;
            tracing::info!(session = %session.name, "Auto-resumed session after restart");
            resumed += 1;
        }
        Ok(resumed)
    }

    pub const fn store(&self) -> &Store {
        &self.store
    }
}

fn validate_workdir(workdir: &str) -> Result<()> {
    let path = std::path::Path::new(workdir);
    if !path.exists() {
        bail!("working directory does not exist: {workdir}");
    }
    if !path.is_dir() {
        bail!("working directory is not a directory: {workdir}");
    }
    Ok(())
}

/// Wrap a command for tmux: escape single quotes, wrap in bash -l -c with agent exit marker.
/// Prepends `PULPO_SESSION_ID` and `PULPO_SESSION_NAME` env vars so tools inside
/// sessions can identify their pulpo context.
/// Known shell binaries — when the command is a bare shell, skip the agent exit wrapper.
const SHELL_COMMANDS: &[&str] = &["bash", "zsh", "sh", "fish", "nu"];

/// Check if a command is a bare shell (no agent work to wrap).
fn is_shell_command(command: &str) -> bool {
    let basename = command.rsplit('/').next().unwrap_or(command).trim();
    SHELL_COMMANDS.contains(&basename)
}

/// Wrap an agent command with env vars, exit marker, and fallback shell.
/// Shell commands are run directly (no exit marker or fallback bash).
///
/// Create a git worktree for a session.
/// Returns the worktree path on success.
#[cfg(not(coverage))]
fn create_worktree(repo_dir: &str, session_name: &str) -> Result<String> {
    let worktree_dir = format!("{repo_dir}/.pulpo/worktrees/{session_name}");
    let branch_name = format!("pulpo/{session_name}");

    // Ensure parent directory exists
    std::fs::create_dir_all(format!("{repo_dir}/.pulpo/worktrees"))?;

    let output = std::process::Command::new("git")
        .args(["worktree", "add", "-b", &branch_name, &worktree_dir])
        .current_dir(repo_dir)
        .output()
        .context("failed to run git worktree add")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git worktree add failed: {}", stderr.trim());
    }

    Ok(worktree_dir)
}

/// Remove a git worktree and prune stale entries.
pub(crate) fn cleanup_worktree(worktree_path: &str) {
    if !std::path::Path::new(worktree_path).exists() {
        return;
    }
    // Worktree path is <repo>/.pulpo/worktrees/<session-name>
    let repo_root = std::path::Path::new(worktree_path)
        .parent() // .pulpo/worktrees/
        .and_then(|p| p.parent()) // .pulpo/
        .and_then(|p| p.parent()); // <repo>/

    let _ = std::fs::remove_dir_all(worktree_path);

    if let Some(root) = repo_root {
        let _ = std::process::Command::new("git")
            .args(["worktree", "prune"])
            .current_dir(root)
            .output();
    }
}

/// Uses `bash -l -c` (login shell) so that `.bash_profile` / `.zprofile` are sourced,
/// ensuring PATH includes Homebrew, nvm, and other tools — critical when pulpod runs
/// as a launchd/systemd service where the environment is minimal.
fn wrap_command(command: &str, session_id: &uuid::Uuid, session_name: &str) -> String {
    if is_shell_command(command) {
        // Shell session: set env vars and exec the shell directly.
        // No exit marker, no fallback bash — exiting the shell kills the tmux session.
        let escaped = command.replace('\'', "'\\''");
        return format!(
            "bash -l -c 'export PULPO_SESSION_ID={session_id}; export PULPO_SESSION_NAME={session_name}; exec {escaped}'"
        );
    }
    let escaped = command.replace('\'', "'\\''");
    // Use $SHELL for the fallback shell so the user gets their preferred shell (zsh, fish, etc.)
    // after the agent exits. Falls back to bash if $SHELL is unset.
    format!(
        "bash -l -c 'export PULPO_SESSION_ID={session_id}; export PULPO_SESSION_NAME={session_name}; {escaped}; echo '\\''[pulpo] Agent exited (session: {session_name}). Run: pulpo resume {session_name}'\\''; exec ${{SHELL:-bash}} -l'"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulpo_common::event::SessionEvent;
    use std::sync::Mutex;

    /// Extract the inner `SessionEvent` from a `PulpoEvent`.
    fn unwrap_session_event(event: PulpoEvent) -> SessionEvent {
        match event {
            PulpoEvent::Session(se) => se,
        }
    }

    struct MockBackend {
        create_result: Mutex<Result<()>>,
        kill_result: Mutex<Result<()>>,
        alive: Mutex<bool>,
        captured_output: Mutex<String>,
        calls: Mutex<Vec<String>>,
    }

    impl MockBackend {
        fn new() -> Self {
            Self {
                create_result: Mutex::new(Ok(())),
                kill_result: Mutex::new(Ok(())),
                alive: Mutex::new(true),
                captured_output: Mutex::new("test output".into()),
                calls: Mutex::new(vec![]),
            }
        }

        fn with_create_error(self) -> Self {
            *self.create_result.lock().unwrap() = Err(anyhow!("backend not found"));
            self
        }

        fn with_kill_error(self) -> Self {
            *self.kill_result.lock().unwrap() = Err(anyhow!("kill failed"));
            self
        }

        fn with_alive(self, alive: bool) -> Self {
            *self.alive.lock().unwrap() = alive;
            self
        }
    }

    impl Backend for MockBackend {
        fn create_session(&self, name: &str, working_dir: &str, command: &str) -> Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("create:{name}:{working_dir}:{command}"));
            let mut result = self.create_result.lock().unwrap();
            std::mem::replace(&mut *result, Ok(()))
        }

        fn kill_session(&self, name: &str) -> Result<()> {
            self.calls.lock().unwrap().push(format!("kill:{name}"));
            let mut result = self.kill_result.lock().unwrap();
            std::mem::replace(&mut *result, Ok(()))
        }

        fn is_alive(&self, name: &str) -> Result<bool> {
            self.calls.lock().unwrap().push(format!("is_alive:{name}"));
            Ok(*self.alive.lock().unwrap())
        }

        fn capture_output(&self, name: &str, lines: usize) -> Result<String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("capture:{name}:{lines}"));
            Ok(self.captured_output.lock().unwrap().clone())
        }

        fn send_input(&self, name: &str, text: &str) -> Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("send_input:{name}:{text}"));
            Ok(())
        }

        fn setup_logging(&self, name: &str, log_path: &str) -> Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("setup_logging:{name}:{log_path}"));
            Ok(())
        }

        fn query_backend_id(&self, name: &str) -> anyhow::Result<String> {
            Ok(format!("${}", name.len()))
        }

        fn list_sessions(&self) -> anyhow::Result<Vec<(String, String)>> {
            Ok(Vec::new())
        }
    }

    struct FailCapture;
    impl Backend for FailCapture {
        fn create_session(&self, _: &str, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn kill_session(&self, _: &str) -> Result<()> {
            Ok(())
        }
        fn is_alive(&self, _: &str) -> Result<bool> {
            Ok(true)
        }
        fn capture_output(&self, _: &str, _: usize) -> Result<String> {
            Err(anyhow!("session not alive"))
        }
        fn send_input(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn setup_logging(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
    }

    async fn test_manager(
        backend: MockBackend,
    ) -> (SessionManager, Arc<MockBackend>, sqlx::SqlitePool) {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let pool = store.pool().clone();
        let backend = Arc::new(backend);
        let manager =
            SessionManager::new(backend.clone(), store, HashMap::new(), None).with_no_stale_grace();
        (manager, backend, pool)
    }

    fn make_req(name: &str) -> CreateSessionRequest {
        CreateSessionRequest {
            name: name.to_owned(),
            workdir: Some("/tmp".into()),
            command: Some("echo hello".into()),
            ink: None,
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            sandbox: None,
        }
    }

    #[tokio::test]
    async fn test_create_session_defaults() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr.create_session(make_req("fix-the-bug")).await.unwrap();

        assert_eq!(session.name, "fix-the-bug");
        assert_eq!(session.command, "echo hello");
        assert_eq!(session.status, SessionStatus::Active);
        assert_eq!(session.workdir, "/tmp");
        // MockBackend.query_backend_id() returns $N where N is the name length
        assert_eq!(session.backend_session_id, Some("$11".into()));

        let calls = backend.calls.lock().unwrap();
        // All commands wrapped in bash -l -c for session survival
        assert!(calls[0].contains("create:fix-the-bug:/tmp:bash -l -c"));
        assert!(calls[0].contains("echo hello"));
        assert!(calls[1].starts_with("setup_logging:fix-the-bug:"));
        assert_eq!(calls.len(), 2);
        drop(calls);
    }

    #[tokio::test]
    async fn test_create_session_no_command_falls_back_to_shell() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let req = CreateSessionRequest {
            name: "test".into(),
            workdir: Some("/tmp".into()),
            command: None,
            ink: None,
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            sandbox: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        // Should fall back to $SHELL or /bin/sh
        assert!(!session.command.is_empty());
        let calls = backend.calls.lock().unwrap();
        assert!(calls[0].contains("create:test:/tmp:bash -l -c"));
        drop(calls);
    }

    #[tokio::test]
    async fn test_create_session_with_ink() {
        let mut inks = HashMap::new();
        inks.insert(
            "coder".into(),
            InkConfig {
                description: Some("Coder ink".into()),
                command: Some("claude -p 'implement'".into()),
            },
        );
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(MockBackend::new());
        let mgr = SessionManager::new(backend, store, inks, None).with_no_stale_grace();

        let req = CreateSessionRequest {
            name: "ink-test".into(),
            workdir: Some("/tmp".into()),
            command: None,
            ink: Some("coder".into()),
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            sandbox: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        assert_eq!(session.command, "claude -p 'implement'");
        assert_eq!(session.description, Some("Coder ink".into()));
    }

    #[tokio::test]
    async fn test_create_session_command_overrides_ink() {
        let mut inks = HashMap::new();
        inks.insert(
            "coder".into(),
            InkConfig {
                description: Some("Coder ink".into()),
                command: Some("claude -p 'default'".into()),
            },
        );
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(MockBackend::new());
        let mgr = SessionManager::new(backend, store, inks, None).with_no_stale_grace();

        let req = CreateSessionRequest {
            name: "override-test".into(),
            workdir: Some("/tmp".into()),
            command: Some("my-custom-command".into()),
            ink: Some("coder".into()),
            description: Some("My desc".into()),
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            sandbox: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        // Explicit command wins over ink command
        assert_eq!(session.command, "my-custom-command");
        assert_eq!(session.description, Some("My desc".into()));
    }

    #[tokio::test]
    async fn test_create_session_unknown_ink() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let req = CreateSessionRequest {
            name: "test".into(),
            workdir: Some("/tmp".into()),
            command: None,
            ink: Some("nonexistent".into()),
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            sandbox: None,
        };
        let result = mgr.create_session(req).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown ink"), "got: {err}");
    }

    #[tokio::test]
    async fn test_create_session_default_workdir() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let req = CreateSessionRequest {
            name: "defaults-test".into(),
            workdir: None,
            command: Some("echo test".into()),
            ink: None,
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            sandbox: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        assert!(!session.workdir.is_empty());
    }

    #[tokio::test]
    async fn test_create_session_calls_setup_logging() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let _session = mgr.create_session(make_req("test")).await.unwrap();

        let calls = backend.calls.lock().unwrap();
        assert!(
            calls.iter().any(|c| c.starts_with("setup_logging:")),
            "Expected setup_logging call, got: {calls:?}"
        );
        drop(calls);
    }

    #[tokio::test]
    async fn test_create_session_explicit_name() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let req = CreateSessionRequest {
            name: "custom-name".into(),
            ..make_req("test")
        };
        let session = mgr.create_session(req).await.unwrap();
        assert_eq!(session.name, "custom-name");
    }

    #[tokio::test]
    async fn test_create_session_workdir_not_found() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let req = CreateSessionRequest {
            workdir: Some("/nonexistent/path/that/does/not/exist".into()),
            ..make_req("test")
        };
        let result = mgr.create_session(req).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("does not exist"), "got: {err}");
    }

    #[tokio::test]
    async fn test_create_session_workdir_is_file() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_owned();
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let req = CreateSessionRequest {
            workdir: Some(path),
            ..make_req("test")
        };
        let result = mgr.create_session(req).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not a directory"), "got: {err}");
    }

    #[tokio::test]
    async fn test_create_session_backend_failure() {
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_create_error()).await;
        let result = mgr.create_session(make_req("test")).await;
        assert!(result.is_err());

        // Session should be marked Dead in store
        let sessions = mgr.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].status, SessionStatus::Killed);
    }

    #[tokio::test]
    async fn test_create_session_duplicate_name_rejected() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        mgr.create_session(make_req("dupe")).await.unwrap();
        let err = mgr.create_session(make_req("dupe")).await.unwrap_err();
        assert!(
            err.to_string().contains("already active"),
            "expected duplicate name error, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_create_session_reuse_name_after_kill() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        mgr.create_session(make_req("reuse")).await.unwrap();
        mgr.kill_session("reuse").await.unwrap();
        // Should succeed — the old session is killed
        mgr.create_session(make_req("reuse")).await.unwrap();
    }

    #[tokio::test]
    async fn test_get_session_alive() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Active);
    }

    #[tokio::test]
    async fn test_get_session_dead_lazy_update() {
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Lost);
    }

    #[tokio::test]
    async fn test_get_session_idle_with_dead_backend_transitions_to_lost() {
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();

        // Manually set session to Idle (simulates watchdog marking it idle before reboot)
        mgr.store()
            .update_session_status(&session.id.to_string(), SessionStatus::Idle)
            .await
            .unwrap();

        // When fetched, check_and_mark_stale should detect dead backend and mark Lost
        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Lost);
    }

    #[tokio::test]
    async fn test_get_session_idle_with_alive_backend_stays_idle() {
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(true)).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();

        mgr.store()
            .update_session_status(&session.id.to_string(), SessionStatus::Idle)
            .await
            .unwrap();

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Idle);
    }

    #[tokio::test]
    async fn test_list_sessions_idle_with_dead_backend_transitions_to_lost() {
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();

        // Manually set session to Idle
        mgr.store()
            .update_session_status(&session.id.to_string(), SessionStatus::Idle)
            .await
            .unwrap();

        let sessions = mgr.list_sessions().await.unwrap();
        assert_eq!(sessions[0].status, SessionStatus::Lost);
    }

    #[tokio::test]
    async fn test_get_session_not_found() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let result = mgr.get_session("nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_list_sessions_with_mixed_status() {
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let s1 = mgr.create_session(make_req("first")).await.unwrap();

        let sessions = mgr.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
        // is_alive returns false, so Running → Stale
        assert_eq!(sessions[0].id, s1.id);
        assert_eq!(sessions[0].status, SessionStatus::Lost);
    }

    #[tokio::test]
    async fn test_kill_session() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();

        mgr.kill_session(&session.id.to_string()).await.unwrap();

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Killed);
    }

    #[tokio::test]
    async fn test_kill_session_not_found() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let result = mgr.kill_session("nonexistent").await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("session not found")
        );
    }

    #[tokio::test]
    async fn test_kill_session_backend_error() {
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_kill_error()).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();

        let result = mgr.kill_session(&session.id.to_string()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("failed to kill"));
    }

    #[tokio::test]
    async fn test_capture_output() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let output = mgr.capture_output("some-id", "my-session", 100);
        assert_eq!(output, "test output");
    }

    #[tokio::test]
    async fn test_capture_output_falls_back_to_log() {
        let backend = MockBackend::new();
        *backend.captured_output.lock().unwrap() = String::new();
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let data_dir = tmpdir.path().to_str().unwrap();
        let store = Store::new(data_dir).await.unwrap();
        store.migrate().await.unwrap();

        // Create a log file
        let log_dir = format!("{data_dir}/logs");
        std::fs::create_dir_all(&log_dir).unwrap();
        std::fs::write(
            format!("{log_dir}/test-id.log"),
            "line 1\nline 2\nline 3\nline 4\nline 5\n",
        )
        .unwrap();

        // Verify all FailCapture backend methods for coverage
        let fc = FailCapture;
        assert!(fc.create_session("n", "d", "c").is_ok());
        assert!(fc.kill_session("n").is_ok());
        assert!(fc.is_alive("n").unwrap());
        assert!(fc.capture_output("n", 10).is_err());
        assert!(fc.send_input("n", "t").is_ok());
        assert!(fc.setup_logging("n", "p").is_ok());

        let mgr = SessionManager::new(Arc::new(FailCapture), store, HashMap::new(), None);
        let output = mgr.capture_output("test-id", "whatever", 3);
        assert_eq!(output, "line 3\nline 4\nline 5");
    }

    #[tokio::test]
    async fn test_read_log_tail_missing_file() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let mgr = SessionManager::new(
            Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            store,
            HashMap::new(),
            None,
        );
        // read_log_tail for nonexistent file returns empty string
        let output = mgr.read_log_tail("nonexistent", 10);
        assert!(output.is_empty());
    }

    #[tokio::test]
    async fn test_send_input() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        mgr.send_input("my-session", "hello").unwrap();

        let calls = backend.calls.lock().unwrap();
        assert!(
            calls
                .iter()
                .any(|c| c.contains("send_input:my-session:hello"))
        );
        drop(calls);
    }

    #[tokio::test]
    async fn test_create_session_store_insert_failure() {
        let (mgr, _, pool) = test_manager(MockBackend::new()).await;
        // Drop the table to make insert_session fail
        sqlx::query("DROP TABLE sessions")
            .execute(&pool)
            .await
            .unwrap();
        let result = mgr.create_session(make_req("test")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_sessions_store_failure() {
        let (mgr, _, pool) = test_manager(MockBackend::new()).await;
        sqlx::query("DROP TABLE sessions")
            .execute(&pool)
            .await
            .unwrap();
        let result = mgr.list_sessions().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_session_store_failure() {
        let (mgr, _, pool) = test_manager(MockBackend::new()).await;
        sqlx::query("DROP TABLE sessions")
            .execute(&pool)
            .await
            .unwrap();
        let result = mgr.get_session("test").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_kill_session_store_failure() {
        let (mgr, _, pool) = test_manager(MockBackend::new()).await;
        sqlx::query("DROP TABLE sessions")
            .execute(&pool)
            .await
            .unwrap();
        let result = mgr.kill_session("test").await;
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_workdir_ok() {
        assert!(validate_workdir("/tmp").is_ok());
    }

    #[test]
    fn test_validate_workdir_missing() {
        let err = validate_workdir("/nonexistent/path")
            .unwrap_err()
            .to_string();
        assert!(err.contains("does not exist"), "got: {err}");
    }

    #[test]
    fn test_validate_workdir_is_file() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap();
        let err = validate_workdir(path).unwrap_err().to_string();
        assert!(err.contains("not a directory"), "got: {err}");
    }

    #[tokio::test]
    async fn test_resume_stale_session() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();

        // get_session marks it Stale since is_alive returns false
        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Lost);

        // Now resume it — backend session is still alive, so it should skip create_session
        *backend.alive.lock().unwrap() = true;
        backend.calls.lock().unwrap().clear();
        let resumed = mgr.resume_session(&session.id.to_string()).await.unwrap();
        assert_eq!(resumed.status, SessionStatus::Active);

        // Verify create_session was NOT called (backend session already exists)
        let calls: Vec<_> = backend.calls.lock().unwrap().clone();
        assert!(
            !calls.iter().any(|c| c.starts_with("create:")),
            "should not recreate backend session when alive; calls: {calls:?}"
        );
    }

    #[tokio::test]
    async fn test_resume_stale_session_recreates_when_backend_dead() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();

        // get_session marks it Stale since is_alive returns false
        let _ = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();

        // Resume while backend session is dead — should recreate
        backend.calls.lock().unwrap().clear();
        let resumed = mgr.resume_session(&session.id.to_string()).await.unwrap();
        assert_eq!(resumed.status, SessionStatus::Active);

        // Verify create_session WAS called
        let calls: Vec<_> = backend.calls.lock().unwrap().clone();
        assert!(
            calls.iter().any(|c| c.starts_with("create:")),
            "should recreate backend session when dead; calls: {calls:?}"
        );
    }

    #[tokio::test]
    async fn test_resume_non_stale_session_fails() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();

        // Session is Active, not Lost/Ready
        let result = mgr.resume_session(&session.id.to_string()).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("cannot be resumed")
        );
    }

    #[tokio::test]
    async fn test_resume_nonexistent_session() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let result = mgr.resume_session("nonexistent").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_resume_name_collision_rejected() {
        let (mgr, _, pool) = test_manager(MockBackend::new().with_alive(false)).await;
        // Create "dup", then mark it lost so it's resumable
        let old = mgr.create_session(make_req("dup")).await.unwrap();
        let old_id = old.id.to_string();
        sqlx::query("UPDATE sessions SET status = 'lost' WHERE id = ?")
            .bind(&old_id)
            .execute(&pool)
            .await
            .unwrap();
        // Create a new active "dup"
        mgr.create_session(make_req("dup")).await.unwrap();
        // Resuming the old lost one should fail — name collision
        let err = mgr.resume_session(&old_id).await.unwrap_err();
        assert!(err.to_string().contains("already active"), "{err}");
    }

    #[tokio::test]
    async fn test_resume_backend_failure() {
        let backend = MockBackend::new().with_alive(false);
        let (mgr, backend_ref, _pool) = test_manager(backend).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();

        let id = session.id.to_string();

        // Mark stale
        let _ = mgr.get_session(&id).await.unwrap();

        // Make create_session fail for resume
        *backend_ref.create_result.lock().unwrap() = Err(anyhow!("backend not found"));
        let result = mgr.resume_session(&id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_resume_ready_session_does_not_self_collide() {
        let (mgr, _, pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr.create_session(make_req("ready-test")).await.unwrap();
        let id = session.id.to_string();

        // Mark session as Ready (simulates a session whose agent finished)
        sqlx::query("UPDATE sessions SET status = 'ready' WHERE id = ?")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();

        // Resuming a Ready session should succeed — it must not collide with itself
        let resumed = mgr.resume_session(&id).await.unwrap();
        assert_eq!(resumed.status, SessionStatus::Active);
    }

    #[test]
    fn test_wrap_command_basic() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command("echo hello", &id, "test-session");
        assert!(cmd.contains("bash -l -c"));
        assert!(cmd.contains("echo hello"));
        assert!(cmd.contains("[pulpo] Agent exited (session: test-session)"));
        assert!(cmd.contains("Run: pulpo resume test-session"));
        // Fallback shell uses $SHELL with bash as default, run as login shell
        #[allow(clippy::literal_string_with_formatting_args)]
        let expected_fallback = "exec ${SHELL:-bash} -l";
        assert!(cmd.contains(expected_fallback));
        assert!(cmd.contains(&format!("PULPO_SESSION_ID={id}")));
        assert!(cmd.contains("PULPO_SESSION_NAME=test-session"));
    }

    #[test]
    fn test_wrap_command_single_quotes() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command("claude -p 'Fix the bug'", &id, "my-task");
        assert!(cmd.contains("bash -l -c"));
        // Single quotes should be properly escaped
        assert!(cmd.contains("claude -p"));
        assert!(cmd.contains("Fix the bug"));
        assert!(cmd.contains("PULPO_SESSION_ID="));
        assert!(cmd.contains("PULPO_SESSION_NAME=my-task"));
        assert!(cmd.contains("(session: my-task)"));
        assert!(cmd.contains("Run: pulpo resume my-task"));
    }

    #[test]
    fn test_wrap_command_quoting_is_valid_shell() {
        // Verify the wrapped command has balanced single quotes so it doesn't
        // cause "unmatched '" errors when tmux passes it to the shell.
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command("claude", &id, "test-session");

        // Count single quotes outside of escaped sequences (\')
        // The '\'' pattern (end-quote, escaped-quote, start-quote) is valid.
        // After removing all '\'' patterns, remaining quotes must be balanced.
        let simplified = cmd.replace("'\\''", "X");
        let quote_count = simplified.chars().filter(|&c| c == '\'').count();
        assert_eq!(
            quote_count % 2,
            0,
            "unbalanced single quotes in wrapped command: {cmd}"
        );
    }

    #[test]
    fn test_is_shell_command() {
        assert!(is_shell_command("bash"));
        assert!(is_shell_command("zsh"));
        assert!(is_shell_command("sh"));
        assert!(is_shell_command("fish"));
        assert!(is_shell_command("nu"));
        assert!(is_shell_command("/bin/bash"));
        assert!(is_shell_command("/usr/bin/zsh"));
        assert!(!is_shell_command("claude"));
        assert!(!is_shell_command("claude -p 'fix'"));
        assert!(!is_shell_command("npm run lint"));
        assert!(!is_shell_command("bash -c 'echo hello'"));
    }

    #[test]
    fn test_wrap_command_shell_no_exit_marker() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command("bash", &id, "my-shell");
        assert!(cmd.contains("exec bash"));
        assert!(cmd.contains(&format!("PULPO_SESSION_ID={id}")));
        assert!(cmd.contains("PULPO_SESSION_NAME=my-shell"));
        // Shell sessions should NOT have exit marker or fallback bash
        assert!(!cmd.contains("[pulpo] Agent exited"));
        assert!(!cmd.contains("Run: pulpo resume"));
    }

    #[test]
    fn test_wrap_command_shell_with_path() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command("/usr/bin/zsh", &id, "zsh-session");
        assert!(cmd.contains("exec /usr/bin/zsh"));
        assert!(!cmd.contains("[pulpo] Agent exited"));
    }

    #[tokio::test]
    async fn test_create_session_emits_event() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let mgr = mgr.with_event_tx(event_tx, "test-node".into());
        let _session = mgr.create_session(make_req("event-test")).await.unwrap();

        let event = event_rx.recv().await.unwrap();
        let se = unwrap_session_event(event);
        assert_eq!(se.session_name, "event-test");
        assert_eq!(se.status, "active");
        assert_eq!(se.previous_status.as_deref(), Some("creating"));
        assert_eq!(se.node_name, "test-node");
    }

    #[tokio::test]
    async fn test_kill_session_emits_event() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let mgr = mgr.with_event_tx(event_tx, "test-node".into());
        let session = mgr.create_session(make_req("kill-event")).await.unwrap();
        // Drain the create event
        let _ = event_rx.recv().await;

        mgr.kill_session(&session.id.to_string()).await.unwrap();
        let event = event_rx.recv().await.unwrap();
        let se = unwrap_session_event(event);
        assert_eq!(se.status, "killed");
    }

    #[tokio::test]
    async fn test_delete_session_active_fails() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();

        let result = mgr.delete_session(&session.id.to_string()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("kill it first"));
    }

    #[tokio::test]
    async fn test_delete_session_killed_succeeds() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();
        mgr.kill_session(&id).await.unwrap();
        mgr.delete_session(&id).await.unwrap();

        let fetched = mgr.get_session(&id).await.unwrap();
        assert!(fetched.is_none());
    }

    #[tokio::test]
    async fn test_delete_session_not_found() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let result = mgr.delete_session("nonexistent").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_resolve_backend_id_with_stored() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();
        let backend_id = mgr.resolve_backend_id(&session);
        // MockBackend.query_backend_id returns $N where N is name length
        assert_eq!(backend_id, "$4");
    }

    #[tokio::test]
    async fn test_resolve_ink_no_command_no_ink_falls_back_to_shell() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let req = CreateSessionRequest {
            name: "test".into(),
            workdir: Some("/tmp".into()),
            command: None,
            ink: None,
            description: Some("desc".into()),
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            sandbox: None,
        };
        let (cmd, desc) = mgr.resolve_ink(&req).unwrap();
        // Falls back to $SHELL or /bin/sh
        assert!(!cmd.is_empty());
        assert_eq!(desc, Some("desc".into()));
    }

    #[tokio::test]
    async fn test_ink_description_fallback() {
        let mut inks = HashMap::new();
        inks.insert(
            "test-ink".into(),
            InkConfig {
                description: Some("Ink desc".into()),
                command: Some("echo test".into()),
            },
        );
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(MockBackend::new());
        let mgr = SessionManager::new(backend, store, inks, None).with_no_stale_grace();

        let req = CreateSessionRequest {
            name: "fallback-test".into(),
            workdir: Some("/tmp".into()),
            command: None,
            ink: Some("test-ink".into()),
            description: None, // Should fall back to ink description
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            sandbox: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        assert_eq!(session.description, Some("Ink desc".into()));
    }

    #[tokio::test]
    async fn test_ink_with_no_command_falls_back_to_shell() {
        let mut inks = HashMap::new();
        inks.insert(
            "empty-ink".into(),
            InkConfig {
                description: Some("Empty".into()),
                command: None,
            },
        );
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(MockBackend::new());
        let mgr = SessionManager::new(backend, store, inks, None).with_no_stale_grace();

        let req = CreateSessionRequest {
            name: "empty-test".into(),
            workdir: Some("/tmp".into()),
            command: None,
            ink: Some("empty-ink".into()),
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            sandbox: None,
        };
        // Ink with no command falls back to $SHELL
        let session = mgr.create_session(req).await.unwrap();
        assert!(!session.command.is_empty());
    }

    #[tokio::test]
    async fn test_create_session_uses_default_command() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(MockBackend::new());
        let mgr = SessionManager::new(backend, store, HashMap::new(), Some("claude".into()))
            .with_no_stale_grace();

        let req = CreateSessionRequest {
            name: "default-cmd-test".into(),
            workdir: Some("/tmp".into()),
            command: None,
            ink: None,
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            sandbox: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        assert_eq!(session.command, "claude");
    }

    #[tokio::test]
    async fn test_create_session_explicit_command_overrides_default() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(MockBackend::new());
        let mgr = SessionManager::new(backend, store, HashMap::new(), Some("claude".into()))
            .with_no_stale_grace();

        let req = CreateSessionRequest {
            name: "explicit-cmd-test".into(),
            workdir: Some("/tmp".into()),
            command: Some("custom-agent".into()),
            ink: None,
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            sandbox: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        assert_eq!(session.command, "custom-agent");
    }

    #[tokio::test]
    async fn test_create_session_ink_overrides_default_command() {
        let mut inks = HashMap::new();
        inks.insert(
            "coder".into(),
            InkConfig {
                description: Some("Coder ink".into()),
                command: Some("claude -p 'implement'".into()),
            },
        );
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(MockBackend::new());
        let mgr = SessionManager::new(backend, store, inks, Some("default-agent".into()))
            .with_no_stale_grace();

        let req = CreateSessionRequest {
            name: "ink-over-default".into(),
            workdir: Some("/tmp".into()),
            command: None,
            ink: Some("coder".into()),
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            sandbox: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        assert_eq!(session.command, "claude -p 'implement'");
    }

    #[tokio::test]
    async fn test_create_session_no_command_no_default_falls_back_to_shell() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let req = CreateSessionRequest {
            name: "no-fallback".into(),
            workdir: Some("/tmp".into()),
            command: None,
            ink: None,
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            sandbox: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        // Should fall back to $SHELL or /bin/sh
        assert!(!session.command.is_empty());
        let calls = backend.calls.lock().unwrap();
        assert!(calls[0].contains("create:no-fallback:/tmp:bash -l -c"));
        drop(calls);
    }

    #[tokio::test]
    async fn test_resume_lost_sessions_resumes_active_with_dead_backend() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        // Create a session (backend is alive)
        mgr.create_session(make_req("sess-a")).await.unwrap();

        // Now simulate reboot: backend reports dead
        *backend.alive.lock().unwrap() = false;

        // Build a new manager that uses the same store but with dead backend
        let resumed = mgr.resume_lost_sessions().await.unwrap();
        assert_eq!(resumed, 1);

        // Backend should have received a create call for the resumed session
        // 2 create calls: one from original create_session, one from resume
        let resume_creates = backend
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.starts_with("create:"))
            .count();
        assert_eq!(resume_creates, 2);
    }

    #[tokio::test]
    async fn test_resume_lost_sessions_skips_killed_sessions() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        mgr.create_session(make_req("killed-sess")).await.unwrap();
        mgr.kill_session("killed-sess").await.unwrap();

        *backend.alive.lock().unwrap() = false;
        let resumed = mgr.resume_lost_sessions().await.unwrap();
        assert_eq!(resumed, 0);
    }

    #[tokio::test]
    async fn test_resume_lost_sessions_skips_alive_sessions() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        mgr.create_session(make_req("alive-sess")).await.unwrap();

        // Backend is alive — should not resume
        let resumed = mgr.resume_lost_sessions().await.unwrap();
        assert_eq!(resumed, 0);
    }

    #[tokio::test]
    async fn test_resume_lost_sessions_resumes_idle_sessions() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr.create_session(make_req("idle-sess")).await.unwrap();

        // Manually set to Idle (simulates watchdog marking it idle before reboot)
        mgr.store()
            .update_session_status(&session.id.to_string(), SessionStatus::Idle)
            .await
            .unwrap();

        *backend.alive.lock().unwrap() = false;
        let resumed = mgr.resume_lost_sessions().await.unwrap();
        assert_eq!(resumed, 1);

        // Simulate the resumed session being alive now
        *backend.alive.lock().unwrap() = true;

        // Session should be Active again
        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Active);
    }

    #[tokio::test]
    async fn test_resume_lost_sessions_marks_lost_on_backend_failure() {
        let (mgr, _, _pool) =
            test_manager(MockBackend::new().with_alive(false).with_create_error()).await;
        // Force-insert a session that looks active but backend will fail on resume
        let session = Session {
            id: Uuid::new_v4(),
            name: "fail-resume".into(),
            workdir: "/tmp".into(),
            command: "echo hello".into(),
            description: None,
            status: SessionStatus::Active,
            exit_code: None,
            backend_session_id: Some("fail-resume".into()),
            output_snapshot: None,
            metadata: None,
            ink: None,
            intervention_code: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            idle_threshold_secs: None,
            worktree_path: None,
            sandbox: false,
            created_at: Utc::now() - chrono::Duration::hours(1),
            updated_at: Utc::now(),
        };
        mgr.store().insert_session(&session).await.unwrap();

        let resumed = mgr.resume_lost_sessions().await.unwrap();
        assert_eq!(resumed, 0);

        // Session should be marked Lost (not Active)
        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Lost);
    }

    #[tokio::test]
    async fn test_resume_lost_sessions_returns_zero_when_empty() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let resumed = mgr.resume_lost_sessions().await.unwrap();
        assert_eq!(resumed, 0);
    }

    #[tokio::test]
    async fn test_with_sandbox_backend_routes_docker_sessions() {
        let sandbox = Arc::new(MockBackend::new());
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let main_backend = Arc::new(MockBackend::new());
        let mgr = SessionManager::new(main_backend.clone(), store, HashMap::new(), None)
            .with_sandbox_backend(sandbox.clone())
            .with_no_stale_grace();
        // Create a sandbox session
        let req = CreateSessionRequest {
            name: "docker-test".to_owned(),
            workdir: Some("/tmp".into()),
            command: Some("echo hi".into()),
            ink: None,
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            sandbox: Some(true),
        };
        let session = mgr.create_session(req).await.unwrap();
        assert!(session.sandbox);
        assert!(
            session
                .backend_session_id
                .as_deref()
                .unwrap()
                .starts_with("docker:")
        );
        // Sandbox backend should have received the create call, not the main backend
        let sandbox_calls: Vec<_> = sandbox.calls.lock().unwrap().clone();
        assert!(
            sandbox_calls.iter().any(|c| c.starts_with("create:")),
            "sandbox backend should handle docker sessions"
        );
        let main_calls: Vec<_> = main_backend.calls.lock().unwrap().clone();
        assert!(
            !main_calls.iter().any(|c| c.starts_with("create:")),
            "main backend should not handle docker sessions"
        );
    }

    #[tokio::test]
    async fn test_sandbox_no_config_fails() {
        // No sandbox backend configured
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let req = CreateSessionRequest {
            name: "docker-test".to_owned(),
            workdir: Some("/tmp".into()),
            command: Some("echo hi".into()),
            ink: None,
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            sandbox: Some(true),
        };
        let err = mgr.create_session(req).await.unwrap_err();
        assert!(err.to_string().contains("sandbox not configured"), "{err}");
    }

    #[tokio::test]
    async fn test_sandbox_command_not_wrapped() {
        let sandbox = Arc::new(MockBackend::new());
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let mgr = SessionManager::new(Arc::new(MockBackend::new()), store, HashMap::new(), None)
            .with_sandbox_backend(sandbox.clone())
            .with_no_stale_grace();
        let req = CreateSessionRequest {
            name: "sandbox-cmd".to_owned(),
            workdir: Some("/tmp".into()),
            command: Some("claude".into()),
            ink: None,
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            sandbox: Some(true),
        };
        mgr.create_session(req).await.unwrap();
        // Sandbox command should NOT be wrapped with bash -l -c
        let calls: Vec<_> = sandbox.calls.lock().unwrap().clone();
        let create_call = calls.iter().find(|c| c.starts_with("create:")).unwrap();
        assert!(
            !create_call.contains("bash -l -c"),
            "sandbox command should not be wrapped: {create_call}"
        );
        assert!(
            create_call.contains("claude"),
            "sandbox command should be raw: {create_call}"
        );
    }

    #[tokio::test]
    async fn test_stale_grace_period_prevents_early_marking() {
        // Use default stale_grace_secs (5) — don't use with_no_stale_grace
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(MockBackend::new().with_alive(false));
        let mgr = SessionManager::new(backend, store, HashMap::new(), None);

        let session = mgr.create_session(make_req("young")).await.unwrap();
        // Session was just created — within grace period, so check_and_mark_stale returns false
        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        // Status should remain Active despite backend being dead (grace period)
        assert_eq!(fetched.status, SessionStatus::Active);
    }

    #[test]
    fn test_cleanup_worktree_nonexistent_path() {
        // Should not panic on nonexistent path
        cleanup_worktree("/tmp/nonexistent-worktree-path-for-test");
    }

    #[test]
    fn test_cleanup_worktree_existing_path() {
        // Create a temporary directory structure: repo/.pulpo/worktrees/session
        let tmpdir = tempfile::tempdir().unwrap();
        let wt_path = tmpdir
            .path()
            .join(".pulpo")
            .join("worktrees")
            .join("test-session");
        std::fs::create_dir_all(&wt_path).unwrap();
        let wt_str = wt_path.to_str().unwrap();
        assert!(wt_path.exists());
        cleanup_worktree(wt_str);
        // The worktree directory should be removed
        assert!(!wt_path.exists());
    }

    #[tokio::test]
    async fn test_kill_session_with_worktree() {
        let (mgr, _, pool) = test_manager(MockBackend::new()).await;
        let session = mgr.create_session(make_req("wt-kill")).await.unwrap();
        let id = session.id.to_string();
        // Simulate a session with a worktree path (nonexistent — cleanup is best-effort)
        sqlx::query("UPDATE sessions SET worktree_path = ? WHERE id = ?")
            .bind("/tmp/nonexistent-wt-kill-test")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();
        // Kill should succeed even with a worktree path
        mgr.kill_session(&id).await.unwrap();
    }

    #[tokio::test]
    async fn test_delete_session_with_worktree() {
        let (mgr, _, pool) = test_manager(MockBackend::new()).await;
        let session = mgr.create_session(make_req("wt-del")).await.unwrap();
        let id = session.id.to_string();
        // Mark as killed so we can delete
        sqlx::query("UPDATE sessions SET status = 'killed', worktree_path = ? WHERE id = ?")
            .bind("/tmp/nonexistent-wt-del-test")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();
        mgr.delete_session(&id).await.unwrap();
    }

    #[tokio::test]
    async fn test_resume_lost_sessions_sandbox_command_not_wrapped() {
        let sandbox = Arc::new(MockBackend::new().with_alive(false));
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let pool = store.pool().clone();
        let mgr = SessionManager::new(
            Arc::new(MockBackend::new().with_alive(false)),
            store,
            HashMap::new(),
            None,
        )
        .with_sandbox_backend(sandbox.clone())
        .with_no_stale_grace();

        // Create a sandbox session and mark it active with dead backend
        let req = CreateSessionRequest {
            name: "auto-resume-docker".to_owned(),
            workdir: Some("/tmp".into()),
            command: Some("echo hi".into()),
            ink: None,
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            sandbox: Some(true),
        };
        let session = mgr.create_session(req).await.unwrap();
        // Mark it active (create_session left it as Active) — backend is dead
        sandbox.calls.lock().unwrap().clear();

        let resumed = mgr.resume_lost_sessions().await.unwrap();
        assert_eq!(resumed, 1);
        // Sandbox auto-resume should NOT wrap the command
        let calls: Vec<_> = sandbox.calls.lock().unwrap().clone();
        let create_call = calls.iter().find(|c| c.starts_with("create:"));
        assert!(
            create_call.is_some(),
            "sandbox backend should re-create session"
        );
        assert!(
            !create_call.unwrap().contains("bash -l -c"),
            "auto-resumed sandbox command should not be wrapped"
        );
        // Verify the session ID was stored correctly
        let updated = sqlx::query_as::<_, (String,)>("SELECT status FROM sessions WHERE id = ?")
            .bind(session.id.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(updated.0, "active");
    }
}
