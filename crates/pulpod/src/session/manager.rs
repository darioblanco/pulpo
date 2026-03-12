use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use chrono::Utc;
use pulpo_common::api::CreateSessionRequest;
use pulpo_common::event::{PulpoEvent, SessionEvent};
use pulpo_common::session::{Provider, Session, SessionMode, SessionStatus};
use tokio::sync::broadcast;
use uuid::Uuid;

use pulpo_common::guard::GuardConfig;

use tracing::{debug, warn};

use crate::backend::Backend;
use crate::config::{InkConfig, SessionDefaultsConfig};
use crate::culture::repo::CultureRepo;
use crate::guard::check_capability_warnings;
use crate::store::Store;

#[derive(Clone)]
pub struct SessionManager {
    backend: Arc<dyn Backend>,
    store: Store,
    culture_repo: Option<CultureRepo>,
    inject_culture: bool,
    #[cfg_attr(coverage, allow(dead_code))]
    curator_enabled: bool,
    #[cfg_attr(coverage, allow(dead_code))]
    curator_provider: Option<String>,
    default_guard: GuardConfig,
    default_provider: Option<String>,
    session_defaults: SessionDefaultsConfig,
    inks: HashMap<String, InkConfig>,
    event_tx: Option<broadcast::Sender<PulpoEvent>>,
    node_name: String,
}

impl SessionManager {
    pub fn new(
        backend: Arc<dyn Backend>,
        store: Store,
        default_guard: GuardConfig,
        inks: HashMap<String, InkConfig>,
    ) -> Self {
        Self {
            backend,
            store,
            culture_repo: None,
            inject_culture: true,
            curator_enabled: false,
            curator_provider: None,
            default_guard,
            default_provider: None,
            session_defaults: SessionDefaultsConfig::default(),
            inks,
            event_tx: None,
            node_name: String::new(),
        }
    }

    #[must_use]
    pub fn with_default_provider(mut self, provider: Option<String>) -> Self {
        self.default_provider = provider;
        self
    }

    #[must_use]
    pub fn with_session_defaults(mut self, defaults: SessionDefaultsConfig) -> Self {
        self.session_defaults = defaults;
        self
    }

    #[must_use]
    pub fn with_culture_repo(mut self, repo: CultureRepo, inject: bool) -> Self {
        self.inject_culture = inject;
        self.culture_repo = Some(repo);
        self
    }

    #[must_use]
    pub fn with_curator(mut self, enabled: bool, provider: Option<String>) -> Self {
        self.curator_enabled = enabled;
        self.curator_provider = provider;
        self
    }

    pub const fn culture_repo(&self) -> Option<&CultureRepo> {
        self.culture_repo.as_ref()
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

    pub async fn create_session(
        &self,
        req: CreateSessionRequest,
    ) -> Result<(Session, Vec<String>)> {
        let mut req = self.apply_defaults(req);
        req = self.resolve_ink(req)?;
        let workdir = req.workdir.clone().unwrap_or_default();
        validate_workdir(&workdir)?;
        let id = Uuid::new_v4();
        let provider = req.provider.unwrap_or(Provider::Claude);

        // Check provider binary is available before creating the session
        if !is_provider_available(provider) {
            bail!("provider '{provider}' is not available on this node (binary not found in PATH)");
        }
        let mode = req.mode.unwrap_or_default();
        let guards = resolve_guard_config(&req, &self.default_guard);
        let name = if let Some(n) = req.name.take() {
            n
        } else {
            let existing: std::collections::HashSet<String> = self
                .store
                .list_sessions()
                .await?
                .into_iter()
                .map(|s| s.name)
                .collect();
            super::names::generate_name(&|candidate| existing.contains(candidate))
        };
        // Inject culture context after name is determined (write-back path uses session name)
        req = self.inject_culture_context(req, &name);
        let prompt = req.prompt.clone().unwrap_or_default();
        let backend_id = self.backend.session_id(&name);
        let mut spawn_params = build_spawn_params(
            &prompt,
            &guards,
            req.allowed_tools.as_deref(),
            req.model.as_deref(),
            req.system_prompt.as_deref(),
            req.max_turns,
            req.max_budget_usd,
            req.output_format.as_deref(),
            req.conversation_id.as_deref(),
        );
        if req.worktree.unwrap_or(false) {
            spawn_params.worktree = Some(name.clone());
        }
        let warnings = check_capability_warnings(provider, &spawn_params);
        let command = build_command(provider, mode, &spawn_params);

        let now = Utc::now();
        let session = Session {
            id,
            name: name.clone(),
            workdir,
            provider,
            prompt,
            status: SessionStatus::Creating,
            mode,
            conversation_id: None,
            exit_code: None,
            backend_session_id: Some(backend_id.clone()),
            output_snapshot: None,
            guard_config: Some(guards),
            model: req.model,
            allowed_tools: req.allowed_tools,
            system_prompt: req.system_prompt,
            metadata: req.metadata,
            ink: req.ink,
            max_turns: req.max_turns,
            max_budget_usd: req.max_budget_usd,
            output_format: req.output_format,
            intervention_code: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            created_at: now,
            updated_at: now,
        };

        self.store.insert_session(&session).await?;

        if let Err(e) = self
            .backend
            .create_session(&backend_id, &session.workdir, &command)
        {
            self.store
                .update_session_status(&id.to_string(), SessionStatus::Killed)
                .await?;
            return Err(e);
        }

        self.store
            .update_session_status(&id.to_string(), SessionStatus::Active)
            .await?;

        // Set up output logging
        let log_dir = format!("{}/logs", self.store.data_dir());
        let _ = std::fs::create_dir_all(&log_dir);
        let log_path = format!("{log_dir}/{id}.log");
        let _ = self.backend.setup_logging(&backend_id, &log_path);

        // Return the session with updated status (avoids unnecessary re-fetch)
        let mut session = session;
        session.status = SessionStatus::Active;
        session.updated_at = Utc::now();
        self.emit_event(&session, Some(SessionStatus::Creating));
        Ok((session, warnings))
    }

    /// Apply defaults for optional fields: workdir defaults to home directory,
    /// prompt defaults to empty string, provider defaults to config `default_provider`.
    fn apply_defaults(&self, mut req: CreateSessionRequest) -> CreateSessionRequest {
        if req.workdir.is_none() {
            req.workdir = Some(
                dirs::home_dir()
                    .map_or_else(|| "/tmp".to_owned(), |h| h.to_string_lossy().into_owned()),
            );
        }
        if req.prompt.is_none() {
            req.prompt = Some(String::new());
        }
        // Provider: session_defaults.provider > node.default_provider > Claude
        if req.provider.is_none() {
            let default = self
                .session_defaults
                .provider
                .as_ref()
                .or(self.default_provider.as_ref())
                .and_then(|p| p.parse().ok())
                .unwrap_or(Provider::Claude);
            req.provider = Some(default);
        }
        // Session defaults for remaining fields
        if req.model.is_none() {
            req.model.clone_from(&self.session_defaults.model);
        }
        if req.mode.is_none() {
            req.mode = self
                .session_defaults
                .mode
                .as_ref()
                .and_then(|m| m.parse().ok());
        }
        if req.max_turns.is_none() {
            req.max_turns = self.session_defaults.max_turns;
        }
        if req.max_budget_usd.is_none() {
            req.max_budget_usd = self.session_defaults.max_budget_usd;
        }
        if req.output_format.is_none() {
            req.output_format
                .clone_from(&self.session_defaults.output_format);
        }
        req
    }

    fn resolve_ink(&self, mut req: CreateSessionRequest) -> Result<CreateSessionRequest> {
        let ink_name = match &req.ink {
            Some(name) => name.clone(),
            None => return Ok(req),
        };
        let ink = self
            .inks
            .get(&ink_name)
            .ok_or_else(|| anyhow!("unknown ink: {ink_name}"))?;

        // Ink defaults — explicit request fields always win
        if req.provider.is_none() {
            req.provider = ink.provider.as_ref().and_then(|p| p.parse().ok());
        }
        if req.model.is_none() {
            req.model.clone_from(&ink.model);
        }
        if req.mode.is_none() {
            req.mode = ink.mode.as_ref().and_then(|m| m.parse().ok());
        }
        if req.unrestricted.is_none() {
            req.unrestricted = ink.unrestricted;
        }

        // Instructions: provider-aware routing
        if let Some(instructions) = &ink.instructions {
            let provider = req
                .provider
                .or_else(|| ink.provider.as_ref().and_then(|p| p.parse().ok()))
                .unwrap_or(Provider::Claude);
            if crate::guard::provider_capabilities(provider).system_prompt {
                // Native system prompt support (e.g. Claude)
                if req.system_prompt.is_none() {
                    req.system_prompt = Some(instructions.clone());
                }
            } else {
                // Universal fallback: prepend to prompt
                let prompt = req.prompt.unwrap_or_default();
                req.prompt = Some(format!("{instructions}\n\n{prompt}"));
            }
        }

        Ok(req)
    }

    /// Inject culture context into the request `prompt`/`system_prompt`.
    /// Reads the compiled AGENTS.md files (global + repo + ink scopes) and
    /// merges them into the session prompt, along with write-back instructions.
    fn inject_culture_context(
        &self,
        mut req: CreateSessionRequest,
        session_name: &str,
    ) -> CreateSessionRequest {
        if !self.inject_culture {
            return req;
        }
        // Shell sessions have no agent to read culture context
        if req.provider == Some(Provider::Shell) {
            return req;
        }
        let Some(repo) = &self.culture_repo else {
            return req;
        };

        let workdir = req.workdir.as_deref().unwrap_or_default();
        let root = repo.root().display().to_string();
        let context = build_culture_context(repo, workdir, req.ink.as_deref(), &root, session_name);

        let provider = req.provider.unwrap_or(Provider::Claude);
        if crate::guard::provider_capabilities(provider).system_prompt {
            let existing = req.system_prompt.unwrap_or_default();
            req.system_prompt = Some(if existing.is_empty() {
                context
            } else {
                format!("{existing}\n\n{context}")
            });
        } else {
            let prompt = req.prompt.unwrap_or_default();
            req.prompt = Some(format!("{context}\n\n{prompt}"));
        }

        req
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
    async fn check_and_mark_stale(&self, session: &mut Session) -> Result<bool> {
        if session.status != SessionStatus::Active {
            return Ok(false);
        }
        let backend_id = self.resolve_backend_id(session);
        let alive = self.backend.is_alive(&backend_id)?;
        if alive {
            return Ok(false);
        }
        self.store
            .update_session_status(&session.id.to_string(), SessionStatus::Lost)
            .await?;
        session.status = SessionStatus::Lost;
        self.extract_and_store_culture(session).await;
        Ok(true)
    }

    pub async fn kill_session(&self, id: &str) -> Result<()> {
        let session = self
            .store
            .get_session(id)
            .await?
            .ok_or_else(|| anyhow!("session not found: {id}"))?;

        let backend_id = self.resolve_backend_id(&session);
        if let Err(e) = self.backend.kill_session(&backend_id) {
            bail!("failed to kill session: {e}");
        }

        let previous = session.status;
        let session_id = session.id.to_string();
        self.store
            .update_session_status(&session_id, SessionStatus::Killed)
            .await?;
        let mut dead_session = session;
        dead_session.status = SessionStatus::Killed;
        self.extract_and_store_culture(&dead_session).await;
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
        let _ = self.backend.kill_session(&backend_id);

        self.store.delete_session(&session.id.to_string()).await?;
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
        if previous_status != SessionStatus::Lost && previous_status != SessionStatus::Finished {
            bail!("session cannot be resumed (status: {previous_status})");
        }

        // If the backend session is still alive, just re-mark it as running.
        // Only recreate the session if the backend process is gone.
        let backend_id = self.resolve_backend_id(&session);
        let alive = self.backend.is_alive(&backend_id)?;
        if !alive {
            let guards = session
                .guard_config
                .clone()
                .unwrap_or_else(|| self.default_guard.clone());
            let mut spawn_params = build_spawn_params(
                &session.prompt,
                &guards,
                session.allowed_tools.as_deref(),
                session.model.as_deref(),
                session.system_prompt.as_deref(),
                session.max_turns,
                session.max_budget_usd,
                session.output_format.as_deref(),
                session.conversation_id.as_deref(),
            );
            // Worktree is inherited by --resume, no need to re-set it
            spawn_params
                .conversation_id
                .clone_from(&session.conversation_id);
            let command = build_command(session.provider, session.mode, &spawn_params);

            self.backend
                .create_session(&backend_id, &session.workdir, &command)?;
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

    pub const fn store(&self) -> &Store {
        &self.store
    }

    /// Extract culture from a session and persist it to the git-backed culture repo.
    /// Best-effort: logs warnings on failure but does not propagate errors.
    async fn extract_and_store_culture(&self, session: &Session) {
        let Some(repo) = &self.culture_repo else {
            return;
        };

        // Harvest agent write-back from pending/ directory
        #[cfg_attr(coverage, allow(unused_variables))]
        let harvested = match repo
            .harvest_pending(
                &session.name,
                session.id,
                &session.workdir,
                session.ink.as_deref(),
            )
            .await
        {
            Ok(count) if count > 0 => {
                debug!(
                    session_id = %session.id,
                    count,
                    "Harvested culture from session"
                );
                true
            }
            Err(e) => {
                warn!(
                    session_id = %session.id,
                    "Failed to harvest culture: {e}"
                );
                false
            }
            _ => false,
        };

        // Curator fallback: spawn a curator session if no pending file was found
        // and this session is not itself a curator (avoids infinite recursion).
        #[cfg(not(coverage))]
        if !harvested
            && self.curator_enabled
            && !session
                .metadata
                .as_ref()
                .is_some_and(|m| m.contains_key("curator"))
        {
            self.spawn_curator(session);
        }
    }

    /// Spawn a fire-and-forget curator session to extract learnings from a
    /// completed session's output.
    #[cfg(not(coverage))]
    fn spawn_curator(&self, session: &Session) {
        let backend_id = self.resolve_backend_id(session);
        let output = self.capture_output(&session.id.to_string(), &backend_id, 200);
        if output.trim().is_empty() {
            return;
        }

        let repo_root = self
            .culture_repo
            .as_ref()
            .map(|r| r.root().display().to_string())
            .unwrap_or_default();

        let prompt = build_curator_prompt(&session.name, &output, &repo_root);
        let provider_str = self
            .curator_provider
            .clone()
            .unwrap_or_else(|| "claude".into());
        let provider = provider_str.parse::<Provider>().unwrap_or(Provider::Claude);

        let mut metadata = HashMap::new();
        metadata.insert("curator".into(), session.id.to_string());

        let req = CreateSessionRequest {
            name: None,
            workdir: Some(session.workdir.clone()),
            provider: Some(provider),
            prompt: Some(prompt),
            mode: Some(SessionMode::Autonomous),
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: Some(metadata),
            ink: None,
            max_turns: Some(1),
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
        };

        let mgr = self.clone();
        tokio::spawn(async move {
            match mgr.create_session(req).await {
                Ok((s, _)) => {
                    debug!(
                        curator_session = %s.id,
                        original_session = %s.metadata.as_ref()
                            .and_then(|m| m.get("curator"))
                            .map_or("?", String::as_str),
                        "Spawned curator session"
                    );
                }
                Err(e) => {
                    warn!("Failed to spawn curator session: {e}");
                }
            }
        });
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

fn resolve_guard_config(req: &CreateSessionRequest, default: &GuardConfig) -> GuardConfig {
    req.unrestricted
        .map_or_else(|| default.clone(), |u| GuardConfig { unrestricted: u })
}

#[allow(clippy::too_many_arguments)]
fn build_spawn_params(
    prompt: &str,
    guards: &GuardConfig,
    allowed_tools: Option<&[String]>,
    model: Option<&str>,
    system_prompt: Option<&str>,
    max_turns: Option<u32>,
    max_budget_usd: Option<f64>,
    output_format: Option<&str>,
    conversation_id: Option<&str>,
) -> crate::guard::SpawnParams {
    crate::guard::SpawnParams {
        prompt: prompt.into(),
        guards: guards.clone(),
        explicit_tools: allowed_tools.map(<[String]>::to_vec),
        model: model.map(Into::into),
        system_prompt: system_prompt.map(Into::into),
        max_turns,
        max_budget_usd,
        output_format: output_format.map(Into::into),
        worktree: None,
        conversation_id: conversation_id.map(Into::into),
    }
}

/// Resolve a provider binary name to its absolute path.
///
/// When pulpod runs as a service (launchd/systemd), the PATH is restricted and
/// may not include directories like `~/.local/bin` where agent CLIs live.
/// This function searches common user binary locations beyond the process PATH.
fn resolve_binary(name: &str) -> String {
    // First try the process PATH via `which`
    if let Some(path) = which_binary(name) {
        return path;
    }
    // Then try common user binary directories
    if let Some(home) = dirs::home_dir() {
        let candidates = [
            home.join(".local/bin").join(name),
            home.join(".cargo/bin").join(name),
            home.join("bin").join(name),
            home.join(".npm-global/bin").join(name),
        ];
        for candidate in &candidates {
            if candidate.exists() {
                return candidate.to_string_lossy().into_owned();
            }
        }
    }
    // Also check common system locations not always in service PATH
    let system_dirs = ["/usr/local/bin", "/opt/homebrew/bin"];
    for dir in &system_dirs {
        let candidate = std::path::Path::new(dir).join(name);
        if candidate.exists() {
            return candidate.to_string_lossy().into_owned();
        }
    }
    name.to_owned()
}

fn which_binary(name: &str) -> Option<String> {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|s| !s.is_empty())
}

/// Check if a provider's binary is available on this system.
/// Shell is always available. For agent providers, checks PATH and common locations.
pub fn is_provider_available(provider: Provider) -> bool {
    if provider == Provider::Shell {
        return true;
    }
    let name = provider.to_string();
    let resolved = resolve_binary(&name);
    // resolve_binary returns the bare name if not found — check if the resolved
    // path actually differs (was found) or if the bare name exists in PATH
    resolved != name || which_binary(&name).is_some()
}

/// Return the binary name/path for a provider.
/// Shell returns "bash". Agent providers use `resolve_binary`.
pub fn provider_binary(provider: Provider) -> String {
    if provider == Provider::Shell {
        return "bash".to_owned();
    }
    resolve_binary(&provider.to_string())
}

pub(crate) fn build_command(
    provider: Provider,
    mode: SessionMode,
    params: &crate::guard::SpawnParams,
) -> String {
    if provider == Provider::Shell {
        // Bare shell session — just start bash, no agent command
        return "bash".to_owned();
    }
    let binary = resolve_binary(&provider.to_string());
    let flags = crate::guard::build_flags(provider, mode, params);
    let inner = format!("{binary} {}", flags.join(" "));
    // Wrap in bash so the tmux session survives if the agent exits.
    // Use double quotes for `bash -c` to avoid conflicts with single-quoted
    // shell_escape() values inside the flags (prompts, system_prompt, etc.).
    let escaped = inner
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$");
    format!("bash -c \"{escaped}; echo '[pulpo] Agent exited'; exec bash\"")
}

/// Build the prompt for a curator session that extracts learnings from
/// another session's output.
#[cfg_attr(coverage, allow(dead_code))]
fn build_curator_prompt(session_name: &str, output: &str, culture_repo_root: &str) -> String {
    format!(
        "You are a culture curator for pulpo. Your ONLY job is to extract \
         non-obvious learnings from the following session output and write them \
         to a pending file.\n\n\
         ## Session: {session_name}\n\n\
         <output>\n{output}\n</output>\n\n\
         ## Instructions\n\n\
         1. Read the session output above carefully.\n\
         2. Identify any non-obvious learnings — environment quirks, gotchas, \
         patterns, or things a future agent couldn't figure out from the code.\n\
         3. If you find a learning, write it to:\n\
         `{culture_repo_root}/pending/{session_name}.md`\n\n\
         Use this format:\n\
         ```markdown\n\
         # <Short title (10-120 chars)>\n\n\
         <Detailed explanation (at least 30 chars). Explain WHY the finding matters,\n\
         not just WHAT happened.>\n\
         ```\n\n\
         Good: `# SQLite WAL mode required for concurrent readers` + explanation of \
         the failure mode without it.\n\
         Bad: `# fix` + `Fixed the bug.` — too vague, would be rejected.\n\n\
         4. If there are no non-obvious learnings, do nothing and exit. Most sessions \
         have nothing worth writing — that's fine.\n\
         5. Do NOT modify any code. Only write to the pending file."
    )
}

/// Build culture context string for injection into agent sessions.
///
/// Reads the compiled AGENTS.md files from relevant scopes (global, repo, ink)
/// and merges them. Includes write-back instructions so the agent can contribute
/// new learnings.
fn build_culture_context(
    repo: &CultureRepo,
    workdir: &str,
    ink: Option<&str>,
    repo_root: &str,
    session_name: &str,
) -> String {
    use std::fmt::Write;
    let mut ctx = String::new();

    ctx.push_str("## Culture from previous sessions\n\n");

    // Read compiled AGENTS.md from each applicable scope
    #[allow(clippy::useless_let_if_seq)]
    let mut has_content = false;

    // 1. Global culture
    if let Ok(Some(content)) = repo.read_agents_md("culture")
        && !is_empty_agents_md(&content)
    {
        ctx.push_str("### Global culture\n\n");
        ctx.push_str(content.trim());
        ctx.push_str("\n\n");
        has_content = true;
    }

    // 2. Repo-scoped culture
    if !workdir.is_empty() {
        let slug = std::path::Path::new(workdir)
            .file_name()
            .map_or_else(|| workdir.to_owned(), |n| n.to_string_lossy().to_string());
        let scope = format!("repos/{slug}");
        if let Ok(Some(content)) = repo.read_agents_md(&scope)
            && !is_empty_agents_md(&content)
        {
            let _ = write!(ctx, "### Repository: {slug}\n\n");
            ctx.push_str(content.trim());
            ctx.push_str("\n\n");
            has_content = true;
        }
    }

    // 3. Ink-scoped culture
    if let Some(ink_name) = ink {
        let scope = format!("inks/{ink_name}");
        if let Ok(Some(content)) = repo.read_agents_md(&scope)
            && !is_empty_agents_md(&content)
        {
            let _ = write!(ctx, "### Ink: {ink_name}\n\n");
            ctx.push_str(content.trim());
            ctx.push_str("\n\n");
            has_content = true;
        }
    }

    if !has_content {
        ctx.push_str("No previous findings for this repo/ink.\n\n");
    }

    // Write-back instructions
    let _ = write!(
        ctx,
        "## Write-back: share your learnings\n\n\
         When you finish your task, write any non-obvious learnings to:\n\n\
         ```\n\
         {repo_root}/pending/{session_name}.md\n\
         ```\n\n\
         Use this format:\n\n\
         ```markdown\n\
         # <Short title describing the learning (10-120 chars)>\n\n\
         <Detailed explanation (at least 30 chars). Focus on things a future agent\n\
         couldn't figure out from reading the code.>\n\n\
         supersedes: <id of old entry if this replaces one>\n\
         ```\n\n\
         Good example:\n\n\
         ```markdown\n\
         # SQLite WAL mode must be enabled before concurrent readers\n\n\
         The default journal mode blocks concurrent reads during writes. Set\n\
         `PRAGMA journal_mode=WAL` at connection init — without this, the watchdog\n\
         health checks timeout when a session save is in progress.\n\
         ```\n\n\
         Bad example (would be rejected):\n\n\
         ```markdown\n\
         # fix\n\n\
         Fixed the bug.\n\
         ```\n\n\
         Guidelines:\n\
         - Only write things that are NOT obvious from the code itself\n\
         - Title must be 10-120 characters, body at least 30 characters\n\
         - Include explanation, not just code blocks\n\
         - One learning per file\n\
         - Skip this if you didn't discover anything non-obvious\n\
         - If your learning corrects or replaces an existing one shown above, \
         add a `supersedes: <id>` line with the old entry's ID\n\
         - Pulpo validates entries automatically and rejects low-quality ones"
    );

    ctx
}

/// Check if an AGENTS.md file has only the bootstrap template with no actual entries.
fn is_empty_agents_md(content: &str) -> bool {
    !content.contains("### [")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulpo_common::culture::Culture;
    use pulpo_common::event::SessionEvent;
    use std::sync::Mutex;

    /// Extract the inner `SessionEvent` from a `PulpoEvent`.
    fn unwrap_session_event(event: PulpoEvent) -> SessionEvent {
        match event {
            PulpoEvent::Session(se) => se,
            PulpoEvent::Culture(_) => panic!("expected Session event, got Culture"),
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
        let manager = SessionManager::new(
            backend.clone(),
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
        (manager, backend, pool)
    }

    fn make_req(prompt: &str) -> CreateSessionRequest {
        CreateSessionRequest {
            name: None,
            workdir: Some("/tmp".into()),
            provider: None,
            prompt: Some(prompt.into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
        }
    }

    #[tokio::test]
    async fn test_create_session_defaults() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let (session, _) = mgr.create_session(make_req("Fix the bug")).await.unwrap();

        // Name is auto-generated (adjective-noun) when not provided
        assert_eq!(session.name.split('-').count(), 2);
        assert_eq!(session.provider, Provider::Claude);
        assert_eq!(session.mode, SessionMode::Interactive);
        assert_eq!(session.status, SessionStatus::Active);
        assert_eq!(session.workdir, "/tmp");
        assert_eq!(session.prompt, "Fix the bug");
        // MockBackend.session_id() returns just the name
        assert_eq!(session.backend_session_id, Some(session.name.clone()));

        let calls = backend.calls.lock().unwrap();
        let name = &session.name;
        // All commands wrapped in bash -c for session survival
        assert!(calls[0].contains(&format!("create:{name}:/tmp:bash -c")));
        assert!(calls[0].contains("claude"));
        assert!(calls[0].contains("Fix the bug"));
        assert!(calls[1].starts_with(&format!("setup_logging:{name}:")));
        assert_eq!(calls.len(), 2);
        drop(calls);
    }

    #[tokio::test]
    async fn test_apply_defaults_fills_workdir_and_prompt() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let req = CreateSessionRequest {
            name: None,
            workdir: None,
            provider: None,
            prompt: None,
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
        };
        let result = mgr.apply_defaults(req);
        assert!(result.workdir.is_some(), "workdir should be filled");
        assert_eq!(result.prompt.as_deref(), Some(""));
        assert_eq!(result.provider, Some(Provider::Claude));
    }

    #[tokio::test]
    async fn test_apply_defaults_preserves_explicit_values() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let req = CreateSessionRequest {
            name: None,
            workdir: Some("/my/dir".into()),
            provider: Some(Provider::Codex),
            prompt: Some("do stuff".into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
        };
        let result = mgr.apply_defaults(req);
        assert_eq!(result.workdir.as_deref(), Some("/my/dir"));
        assert_eq!(result.prompt.as_deref(), Some("do stuff"));
        assert_eq!(result.provider, Some(Provider::Codex));
    }

    #[tokio::test]
    async fn test_apply_defaults_uses_config_default_provider() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let mgr = mgr.with_default_provider(Some("codex".into()));
        let req = CreateSessionRequest {
            name: None,
            workdir: None,
            provider: None,
            prompt: None,
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
        };
        let result = mgr.apply_defaults(req);
        assert_eq!(result.provider, Some(Provider::Codex));
    }

    #[tokio::test]
    async fn test_apply_defaults_session_defaults_all_fields() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let mgr = mgr.with_session_defaults(SessionDefaultsConfig {
            provider: Some("gemini".into()),
            model: Some("gemini-2.5-pro".into()),
            mode: Some("autonomous".into()),
            max_turns: Some(50),
            max_budget_usd: Some(10.0),
            output_format: Some("json".into()),
        });
        let req = make_req("");
        let result = mgr.apply_defaults(req);
        assert_eq!(result.provider, Some(Provider::Gemini));
        assert_eq!(result.model.as_deref(), Some("gemini-2.5-pro"));
        assert_eq!(result.mode, Some(SessionMode::Autonomous));
        assert_eq!(result.max_turns, Some(50));
        assert!((result.max_budget_usd.unwrap() - 10.0).abs() < f64::EPSILON);
        assert_eq!(result.output_format.as_deref(), Some("json"));
    }

    #[tokio::test]
    async fn test_apply_defaults_explicit_overrides_session_defaults() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let mgr = mgr.with_session_defaults(SessionDefaultsConfig {
            provider: Some("gemini".into()),
            model: Some("default-model".into()),
            mode: Some("autonomous".into()),
            max_turns: Some(50),
            max_budget_usd: Some(10.0),
            output_format: Some("json".into()),
        });
        let req = CreateSessionRequest {
            name: None,
            workdir: Some("/tmp".into()),
            provider: Some(Provider::Claude),
            prompt: Some("test".into()),
            mode: Some(SessionMode::Interactive),
            unrestricted: None,
            model: Some("opus".into()),
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: Some(5),
            max_budget_usd: Some(1.0),
            output_format: Some("stream-json".into()),
            worktree: None,
            conversation_id: None,
        };
        let result = mgr.apply_defaults(req);
        // Explicit values win over session defaults
        assert_eq!(result.provider, Some(Provider::Claude));
        assert_eq!(result.model.as_deref(), Some("opus"));
        assert_eq!(result.mode, Some(SessionMode::Interactive));
        assert_eq!(result.max_turns, Some(5));
        assert!((result.max_budget_usd.unwrap() - 1.0).abs() < f64::EPSILON);
        assert_eq!(result.output_format.as_deref(), Some("stream-json"));
    }

    #[tokio::test]
    async fn test_apply_defaults_session_defaults_provider_overrides_node_default() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let mgr = mgr
            .with_default_provider(Some("codex".into()))
            .with_session_defaults(SessionDefaultsConfig {
                provider: Some("gemini".into()),
                ..SessionDefaultsConfig::default()
            });
        let req = make_req("test");
        let result = mgr.apply_defaults(req);
        // session_defaults.provider takes precedence over node.default_provider
        assert_eq!(result.provider, Some(Provider::Gemini));
    }

    #[tokio::test]
    async fn test_apply_defaults_empty_session_defaults_falls_through() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let mgr = mgr.with_session_defaults(SessionDefaultsConfig::default());
        let req = make_req("test");
        let result = mgr.apply_defaults(req);
        // Empty session defaults: falls through to Claude, no model/mode/etc
        assert_eq!(result.provider, Some(Provider::Claude));
        assert!(result.model.is_none());
        assert!(result.mode.is_none());
        assert!(result.max_turns.is_none());
        assert!(result.max_budget_usd.is_none());
        assert!(result.output_format.is_none());
    }

    #[tokio::test]
    async fn test_apply_defaults_partial_session_defaults() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let mgr = mgr.with_session_defaults(SessionDefaultsConfig {
            model: Some("opus".into()),
            max_turns: Some(25),
            ..SessionDefaultsConfig::default()
        });
        let req = make_req("test");
        let result = mgr.apply_defaults(req);
        // Partial: only model and max_turns are set
        assert_eq!(result.provider, Some(Provider::Claude));
        assert_eq!(result.model.as_deref(), Some("opus"));
        assert!(result.mode.is_none());
        assert_eq!(result.max_turns, Some(25));
        assert!(result.max_budget_usd.is_none());
        assert!(result.output_format.is_none());
    }

    #[tokio::test]
    async fn test_create_session_with_no_args() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let req = CreateSessionRequest {
            name: None,
            workdir: None,
            provider: None,
            prompt: None,
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
        };
        let (session, _) = mgr.create_session(req).await.unwrap();
        assert_eq!(session.provider, Provider::Claude);
        assert!(session.prompt.is_empty());
        assert!(!session.workdir.is_empty());
    }

    #[tokio::test]
    async fn test_create_session_calls_setup_logging() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let (_session, _) = mgr.create_session(make_req("test")).await.unwrap();

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
            name: Some("custom-name".into()),
            ..make_req("test")
        };
        let (session, _) = mgr.create_session(req).await.unwrap();
        assert_eq!(session.name, "custom-name");
    }

    #[tokio::test]
    async fn test_create_session_autonomous() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let req = CreateSessionRequest {
            provider: Some(Provider::Claude),
            prompt: Some("Do something".into()),
            mode: Some(SessionMode::Autonomous),
            ..make_req("Do something")
        };
        let (session, _) = mgr.create_session(req).await.unwrap();
        assert_eq!(session.mode, SessionMode::Autonomous);
        // Default guard is restricted — uses --allowedTools, not --dangerously-skip-permissions
        assert!(session.guard_config.is_some());

        let calls = backend.calls.lock().unwrap();
        // Autonomous command is wrapped in bash -c '...'
        assert!(calls[0].contains("bash -c"));
        assert!(calls[0].contains("--allowedTools"));
        assert!(!calls[0].contains("--dangerously-skip-permissions"));
        assert!(calls[1].starts_with("setup_logging:"));
        // Autonomous mode should NOT send_input
        assert_eq!(calls.len(), 2);
        drop(calls);
    }

    #[tokio::test]
    async fn test_create_session_autonomous_unrestricted() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let req = CreateSessionRequest {
            provider: Some(Provider::Claude),
            mode: Some(SessionMode::Autonomous),
            unrestricted: Some(true),
            ..make_req("Do something")
        };
        let (session, _) = mgr.create_session(req).await.unwrap();
        assert_eq!(session.mode, SessionMode::Autonomous);

        let calls = backend.calls.lock().unwrap();
        // Autonomous command is wrapped in bash -c '...'
        assert!(calls[0].contains("bash -c"));
        assert!(calls[0].contains("--dangerously-skip-permissions"));
        assert!(!calls[0].contains("--allowedTools"));
        drop(calls);
    }

    #[tokio::test]
    async fn test_create_session_stores_guard_config() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.guard_config.is_some());
        let gc = fetched.guard_config.unwrap();
        // Default is restricted
        assert!(!gc.unrestricted);
    }

    #[tokio::test]
    async fn test_create_session_with_unrestricted() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let req = CreateSessionRequest {
            unrestricted: Some(true),
            ..make_req("test")
        };
        let (session, _) = mgr.create_session(req).await.unwrap();
        let gc = session.guard_config.unwrap();
        assert!(gc.unrestricted);
    }

    #[tokio::test]
    async fn test_create_session_with_unrestricted_false() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let req = CreateSessionRequest {
            unrestricted: Some(false),
            ..make_req("test")
        };
        let (session, _) = mgr.create_session(req).await.unwrap();
        let gc = session.guard_config.unwrap();
        assert!(!gc.unrestricted);
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
    async fn test_get_session_alive() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();

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
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Lost);
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
        let (s1, _) = mgr.create_session(make_req("first")).await.unwrap();

        let sessions = mgr.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
        // is_alive returns false, so Running → Stale
        assert_eq!(sessions[0].id, s1.id);
        assert_eq!(sessions[0].status, SessionStatus::Lost);
    }

    #[tokio::test]
    async fn test_kill_session() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();

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
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();

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

        let mgr = SessionManager::new(
            Arc::new(FailCapture),
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
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
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
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
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();

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
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();

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
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();

        // Session is Active, not Lost/Finished
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
    async fn test_resume_with_conversation_id() {
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();

        // Set conversation_id
        mgr.store()
            .update_session_conversation_id(&id, "conv-abc")
            .await
            .unwrap();

        // Mark stale via get_session
        let _ = mgr.get_session(&id).await.unwrap();

        // Resume
        let resumed = mgr.resume_session(&id).await.unwrap();
        assert_eq!(resumed.status, SessionStatus::Active);
    }

    #[tokio::test]
    async fn test_resume_session_without_guard_config_uses_default() {
        // Simulate a pre-migration session with guard_config: None
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let id = Uuid::new_v4();
        let now = Utc::now();
        let session = Session {
            id,
            name: "legacy".into(),
            workdir: "/tmp".into(),
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Active,
            mode: SessionMode::Autonomous,
            conversation_id: None,
            exit_code: None,
            backend_session_id: Some("legacy".into()),
            output_snapshot: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_code: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            created_at: now,
            updated_at: now,
        };
        mgr.store().insert_session(&session).await.unwrap();
        // Mark stale
        mgr.store()
            .update_session_status(&id.to_string(), SessionStatus::Lost)
            .await
            .unwrap();

        let resumed = mgr.resume_session(&id.to_string()).await.unwrap();
        assert_eq!(resumed.status, SessionStatus::Active);
    }

    #[tokio::test]
    async fn test_resume_backend_failure() {
        let backend = MockBackend::new().with_alive(false);
        let (mgr, backend_ref, _pool) = test_manager(backend).await;
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();

        // Mark stale
        let _ = mgr.get_session(&id).await.unwrap();

        // Make create_session fail for resume
        *backend_ref.create_result.lock().unwrap() = Err(anyhow!("backend not found"));
        let result = mgr.resume_session(&id).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_guard_config_default() {
        let req = make_req("test");
        let default = GuardConfig::default();
        let result = resolve_guard_config(&req, &default);
        assert!(!result.unrestricted);
    }

    #[test]
    fn test_resolve_guard_config_unrestricted() {
        let req = CreateSessionRequest {
            unrestricted: Some(true),
            ..make_req("test")
        };
        let result = resolve_guard_config(&req, &GuardConfig::default());
        assert!(result.unrestricted);
    }

    #[test]
    fn test_resolve_guard_config_restricted() {
        let req = CreateSessionRequest {
            unrestricted: Some(false),
            ..make_req("test")
        };
        let default = GuardConfig { unrestricted: true };
        let result = resolve_guard_config(&req, &default);
        // Explicit unrestricted=false wins over unrestricted default
        assert!(!result.unrestricted);
    }

    #[test]
    fn test_build_spawn_params_with_conversation_id() {
        let guards = GuardConfig::default();
        let params = build_spawn_params(
            "test",
            &guards,
            None,
            None,
            None,
            None,
            None,
            None,
            Some("conv-abc-123"),
        );
        assert_eq!(params.conversation_id.as_deref(), Some("conv-abc-123"));
    }

    #[test]
    fn test_build_spawn_params_without_conversation_id() {
        let guards = GuardConfig::default();
        let params = build_spawn_params("test", &guards, None, None, None, None, None, None, None);
        assert!(params.conversation_id.is_none());
    }

    #[test]
    fn test_build_command_interactive_claude_resume() {
        let guards = GuardConfig::default();
        let params = crate::guard::SpawnParams {
            prompt: "test".into(),
            guards,
            conversation_id: Some("conv-123".into()),
            ..crate::guard::SpawnParams::default()
        };
        let cmd = build_command(Provider::Claude, SessionMode::Interactive, &params);
        assert!(cmd.contains("bash -c"));
        assert!(cmd.contains("claude --resume conv-123"));
        assert!(cmd.contains("--allowedTools"));
    }

    #[test]
    fn test_build_command_autonomous_claude_resume_unrestricted() {
        let guards = GuardConfig { unrestricted: true };
        let params = crate::guard::SpawnParams {
            prompt: "Fix bug".into(),
            guards,
            conversation_id: Some("conv-456".into()),
            ..crate::guard::SpawnParams::default()
        };
        let cmd = build_command(Provider::Claude, SessionMode::Autonomous, &params);
        assert!(cmd.contains("bash -c"));
        assert!(cmd.contains("--resume conv-456"));
        assert!(cmd.contains("--dangerously-skip-permissions"));
    }

    #[test]
    fn test_build_command_interactive_claude_resume_with_model() {
        let guards = GuardConfig::default();
        let params = crate::guard::SpawnParams {
            prompt: "test".into(),
            guards,
            model: Some("sonnet".into()),
            conversation_id: Some("conv-123".into()),
            ..crate::guard::SpawnParams::default()
        };
        let cmd = build_command(Provider::Claude, SessionMode::Interactive, &params);
        assert!(cmd.contains("bash -c"));
        assert!(cmd.contains("claude --resume conv-123"));
        assert!(cmd.contains("--model sonnet"));
    }

    #[test]
    fn test_build_command_autonomous_claude_resume_all_flags() {
        let guards = GuardConfig::default();
        let params = crate::guard::SpawnParams {
            prompt: "Fix it".into(),
            guards,
            explicit_tools: Some(vec!["Read".into()]),
            model: Some("opus".into()),
            system_prompt: Some("Review only".into()),
            conversation_id: Some("conv-789".into()),
            ..crate::guard::SpawnParams::default()
        };
        let cmd = build_command(Provider::Claude, SessionMode::Autonomous, &params);
        assert!(cmd.contains("bash -c"));
        assert!(cmd.contains("--resume conv-789"));
        assert!(cmd.contains("--model"));
        assert!(cmd.contains("opus"));
        assert!(cmd.contains("--allowedTools"));
        assert!(cmd.contains("Read"));
        assert!(cmd.contains("--append-system-prompt"));
    }

    #[test]
    fn test_build_command_codex_resume() {
        let guards = GuardConfig::default();
        let params_interactive = crate::guard::SpawnParams {
            prompt: "test".into(),
            guards,
            conversation_id: Some("conv-codex".into()),
            ..crate::guard::SpawnParams::default()
        };
        let cmd_interactive = build_command(
            Provider::Codex,
            SessionMode::Interactive,
            &params_interactive,
        );
        assert!(cmd_interactive.contains("bash -c"));
        assert!(cmd_interactive.contains("codex"));
        assert!(cmd_interactive.contains("resume conv-codex"));

        let params_autonomous = crate::guard::SpawnParams {
            prompt: "test".into(),
            guards: GuardConfig::default(),
            conversation_id: Some("conv-codex-auto".into()),
            ..crate::guard::SpawnParams::default()
        };
        let cmd_autonomous =
            build_command(Provider::Codex, SessionMode::Autonomous, &params_autonomous);
        assert!(cmd_autonomous.contains("bash -c"));
        assert!(cmd_autonomous.contains("exec resume conv-codex-auto"));
    }

    #[test]
    fn test_build_command_interactive_claude() {
        let guards = GuardConfig::default();
        let params = crate::guard::SpawnParams {
            prompt: "test".into(),
            guards,
            ..crate::guard::SpawnParams::default()
        };
        let cmd = build_command(Provider::Claude, SessionMode::Interactive, &params);
        // All commands now wrapped in bash -c for session survival
        assert!(cmd.contains("bash -c"));
        assert!(cmd.contains("claude"));
        assert!(cmd.contains("'test'"));
    }

    #[test]
    fn test_build_command_interactive_claude_empty_prompt() {
        let guards = GuardConfig::default();
        let params = crate::guard::SpawnParams {
            prompt: String::new(),
            guards,
            ..crate::guard::SpawnParams::default()
        };
        let cmd = build_command(Provider::Claude, SessionMode::Interactive, &params);
        // Empty prompt should not produce a bare '' arg that makes Claude exit
        assert!(cmd.contains("bash -c"));
        assert!(cmd.contains("claude"));
        assert!(!cmd.contains("''"));
    }

    #[test]
    fn test_build_command_autonomous_claude_standard() {
        let guards = GuardConfig::default();
        let params = crate::guard::SpawnParams {
            prompt: "Fix bug".into(),
            guards,
            ..crate::guard::SpawnParams::default()
        };
        let cmd = build_command(Provider::Claude, SessionMode::Autonomous, &params);
        assert!(cmd.contains("bash -c"));
        assert!(cmd.contains("--allowedTools"));
        assert!(!cmd.contains("--dangerously-skip-permissions"));
    }

    #[test]
    fn test_build_command_autonomous_claude_unrestricted() {
        let guards = GuardConfig { unrestricted: true };
        let params = crate::guard::SpawnParams {
            prompt: "Fix bug".into(),
            guards,
            ..crate::guard::SpawnParams::default()
        };
        let cmd = build_command(Provider::Claude, SessionMode::Autonomous, &params);
        assert!(cmd.contains("bash -c"));
        assert!(cmd.contains("--dangerously-skip-permissions"));
        assert!(!cmd.contains("--allowedTools"));
    }

    #[test]
    fn test_build_command_codex() {
        let guards = GuardConfig::default();
        let params_interactive = crate::guard::SpawnParams {
            prompt: "test".into(),
            guards,
            ..crate::guard::SpawnParams::default()
        };
        let cmd_interactive = build_command(
            Provider::Codex,
            SessionMode::Interactive,
            &params_interactive,
        );
        // All commands wrapped in bash -c for session survival
        assert!(cmd_interactive.contains("bash -c"));
        assert!(cmd_interactive.contains("codex"));
        assert!(cmd_interactive.contains("'test'"));

        let params_autonomous = crate::guard::SpawnParams {
            prompt: "test".into(),
            guards: GuardConfig::default(),
            ..crate::guard::SpawnParams::default()
        };
        let cmd_autonomous =
            build_command(Provider::Codex, SessionMode::Autonomous, &params_autonomous);
        assert!(cmd_autonomous.contains("bash -c"));
        assert!(cmd_autonomous.contains("codex "));
    }

    #[test]
    fn test_build_command_with_model() {
        let guards = GuardConfig::default();
        let params = crate::guard::SpawnParams {
            prompt: "test".into(),
            guards,
            model: Some("opus".into()),
            ..crate::guard::SpawnParams::default()
        };
        let cmd = build_command(Provider::Claude, SessionMode::Interactive, &params);
        assert!(cmd.contains("bash -c"));
        assert!(cmd.contains("--model"));
        assert!(cmd.contains("opus"));
        assert!(cmd.contains("'test'"));
    }

    #[test]
    fn test_build_command_autonomous_with_all_new_flags() {
        let guards = GuardConfig::default();
        let params = crate::guard::SpawnParams {
            prompt: "Fix bug".into(),
            guards,
            explicit_tools: Some(vec!["Read".into(), "Grep".into()]),
            model: Some("opus".into()),
            system_prompt: Some("Be concise".into()),
            ..crate::guard::SpawnParams::default()
        };
        let cmd = build_command(Provider::Claude, SessionMode::Autonomous, &params);
        assert!(cmd.contains("bash -c"));
        assert!(cmd.contains("--model"));
        assert!(cmd.contains("opus"));
        assert!(cmd.contains("--allowedTools"));
        assert!(cmd.contains("Read,Grep"));
        assert!(cmd.contains("--append-system-prompt"));
        assert!(cmd.contains("Be concise"));
    }

    #[test]
    fn test_resolve_binary_falls_back_to_name() {
        // For a non-existent binary, resolve_binary should return the bare name
        let result = resolve_binary("definitely-not-a-real-binary-xyz");
        assert_eq!(result, "definitely-not-a-real-binary-xyz");
    }

    #[test]
    fn test_resolve_binary_finds_system_binary() {
        // `ls` should be found in standard system paths
        let result = resolve_binary("ls");
        assert!(
            result.starts_with('/'),
            "Expected absolute path, got: {result}"
        );
    }

    #[test]
    fn test_build_command_shell_bare() {
        let params = crate::guard::SpawnParams::default();
        let cmd = build_command(Provider::Shell, SessionMode::Interactive, &params);
        assert_eq!(cmd, "bash");
    }

    #[test]
    fn test_build_command_shell_autonomous() {
        let params = crate::guard::SpawnParams::default();
        let cmd = build_command(Provider::Shell, SessionMode::Autonomous, &params);
        assert_eq!(cmd, "bash");
    }

    #[test]
    fn test_is_provider_available_shell() {
        assert!(is_provider_available(Provider::Shell));
    }

    #[test]
    fn test_is_provider_available_nonexistent() {
        // Provider binaries that don't exist should return false
        // We can't test for specific providers since they might be installed,
        // but we can verify the function doesn't panic
        let _available = is_provider_available(Provider::OpenCode);
    }

    #[test]
    fn test_provider_binary_shell() {
        assert_eq!(provider_binary(Provider::Shell), "bash");
    }

    #[test]
    fn test_provider_binary_claude() {
        let binary = provider_binary(Provider::Claude);
        // Should return either "claude" or an absolute path to claude
        assert!(
            binary == "claude" || binary.contains("claude"),
            "Expected claude binary, got: {binary}"
        );
    }

    #[tokio::test]
    async fn test_create_session_shell_provider() {
        let (mgr, _, _) = test_manager(MockBackend::new()).await;
        let mut req = make_req("shell-test");
        req.provider = Some(Provider::Shell);
        let (session, warnings) = mgr.create_session(req).await.unwrap();
        assert_eq!(session.provider, Provider::Shell);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_inject_culture_shell_skipped() {
        let mgr = SessionManager {
            backend: Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            culture_repo: None,
            inject_culture: true,
            curator_enabled: false,
            curator_provider: None,
            store: unsafe_empty_store(),
            default_guard: GuardConfig::default(),
            default_provider: None,
            session_defaults: SessionDefaultsConfig::default(),
            inks: HashMap::new(),
            event_tx: None,
            node_name: String::new(),
        };
        let mut req = make_req("shell-culture-test");
        req.provider = Some(Provider::Shell);
        let result = mgr.inject_culture_context(req, "test-session");
        // Shell sessions should skip culture injection
        assert_eq!(result.system_prompt, None);
    }

    #[tokio::test]
    async fn test_list_sessions_filtered() {
        let (mgr, _, _) = test_manager(MockBackend::new()).await;
        let _ = mgr.create_session(make_req("filter-test")).await.unwrap().0;

        let query = pulpo_common::api::ListSessionsQuery {
            status: Some("active".into()),
            ..Default::default()
        };
        let sessions = mgr.list_sessions_filtered(&query).await.unwrap();
        assert_eq!(sessions.len(), 1);
    }

    #[tokio::test]
    async fn test_list_sessions_filtered_no_match() {
        let (mgr, _, _) = test_manager(MockBackend::new()).await;
        let _ = mgr.create_session(make_req("filter-test")).await.unwrap().0;

        let query = pulpo_common::api::ListSessionsQuery {
            status: Some("finished".into()),
            ..Default::default()
        };
        let sessions = mgr.list_sessions_filtered(&query).await.unwrap();
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn test_list_sessions_filtered_detects_stale() {
        let (mgr, _, _) = test_manager(MockBackend::new().with_alive(false)).await;
        let _ = mgr
            .create_session(make_req("stale-filter"))
            .await
            .unwrap()
            .0;

        let query = pulpo_common::api::ListSessionsQuery::default();
        let sessions = mgr.list_sessions_filtered(&query).await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].status, SessionStatus::Lost);
    }

    #[tokio::test]
    async fn test_list_sessions_filtered_store_failure() {
        let (mgr, _, pool) = test_manager(MockBackend::new()).await;
        sqlx::query("DROP TABLE sessions")
            .execute(&pool)
            .await
            .unwrap();
        let query = pulpo_common::api::ListSessionsQuery::default();
        let result = mgr.list_sessions_filtered(&query).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_ink_no_ink() {
        let inks = HashMap::new();
        let mgr = SessionManager {
            backend: Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            culture_repo: None,
            inject_culture: false,
            curator_enabled: false,
            curator_provider: None,
            store: unsafe_empty_store(),
            default_guard: GuardConfig::default(),
            inks,
            event_tx: None,
            default_provider: None,
            session_defaults: SessionDefaultsConfig::default(),
            node_name: String::new(),
        };
        let req = make_req("test");
        let resolved = mgr.resolve_ink(req).unwrap();
        assert!(resolved.model.is_none());
        assert!(resolved.system_prompt.is_none());
    }

    #[test]
    fn test_resolve_ink_unknown() {
        let inks = HashMap::new();
        let mgr = SessionManager {
            backend: Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            culture_repo: None,
            inject_culture: false,
            curator_enabled: false,
            curator_provider: None,
            store: unsafe_empty_store(),
            default_guard: GuardConfig::default(),
            inks,
            event_tx: None,
            default_provider: None,
            session_defaults: SessionDefaultsConfig::default(),
            node_name: String::new(),
        };
        let req = CreateSessionRequest {
            ink: Some("nonexistent".into()),
            ..make_req("test")
        };
        let result = mgr.resolve_ink(req);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown ink"));
    }

    #[test]
    fn test_resolve_ink_applies_defaults() {
        let mut inks = HashMap::new();
        inks.insert(
            "reviewer".into(),
            crate::config::InkConfig {
                description: None,
                provider: Some("claude".into()),
                model: None,
                mode: Some("autonomous".into()),
                unrestricted: Some(false),
                instructions: Some("Review code".into()),
            },
        );
        let mgr = SessionManager {
            backend: Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            culture_repo: None,
            inject_culture: false,
            curator_enabled: false,
            curator_provider: None,
            store: unsafe_empty_store(),
            default_guard: GuardConfig::default(),
            inks,
            event_tx: None,
            default_provider: None,
            session_defaults: SessionDefaultsConfig::default(),
            node_name: String::new(),
        };
        let req = CreateSessionRequest {
            ink: Some("reviewer".into()),
            ..make_req("test")
        };
        let resolved = mgr.resolve_ink(req).unwrap();
        assert_eq!(resolved.provider, Some(Provider::Claude));
        assert_eq!(resolved.mode, Some(SessionMode::Autonomous));
        assert_eq!(resolved.unrestricted, Some(false));
        // Claude supports system_prompt, so instructions → system_prompt
        assert_eq!(resolved.system_prompt, Some("Review code".into()));
    }

    #[test]
    fn test_resolve_ink_applies_model() {
        let mut inks = HashMap::new();
        inks.insert(
            "coder".into(),
            crate::config::InkConfig {
                description: None,
                provider: Some("claude".into()),
                model: Some("claude-sonnet-4-20250514".into()),
                mode: None,
                unrestricted: None,
                instructions: None,
            },
        );
        let mgr = SessionManager {
            backend: Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            culture_repo: None,
            inject_culture: false,
            curator_enabled: false,
            curator_provider: None,
            store: unsafe_empty_store(),
            default_guard: GuardConfig::default(),
            inks,
            event_tx: None,
            default_provider: None,
            session_defaults: SessionDefaultsConfig::default(),
            node_name: String::new(),
        };
        let req = CreateSessionRequest {
            ink: Some("coder".into()),
            ..make_req("test")
        };
        let resolved = mgr.resolve_ink(req).unwrap();
        assert_eq!(resolved.model, Some("claude-sonnet-4-20250514".into()));

        // Explicit model in request wins over ink model
        let req2 = CreateSessionRequest {
            ink: Some("coder".into()),
            model: Some("claude-opus-4-20250514".into()),
            ..make_req("test2")
        };
        let resolved2 = mgr.resolve_ink(req2).unwrap();
        assert_eq!(resolved2.model, Some("claude-opus-4-20250514".into()));
    }

    #[test]
    fn test_resolve_ink_instructions_prepend_for_non_claude() {
        let mut inks = HashMap::new();
        inks.insert(
            "coder".into(),
            crate::config::InkConfig {
                description: None,
                provider: Some("codex".into()),
                model: None,
                mode: None,
                unrestricted: None,
                instructions: Some("You are an expert coder.".into()),
            },
        );
        let mgr = SessionManager {
            backend: Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            culture_repo: None,
            inject_culture: false,
            curator_enabled: false,
            curator_provider: None,
            store: unsafe_empty_store(),
            default_guard: GuardConfig::default(),
            inks,
            event_tx: None,
            default_provider: None,
            session_defaults: SessionDefaultsConfig::default(),
            node_name: String::new(),
        };
        let req = CreateSessionRequest {
            ink: Some("coder".into()),
            ..make_req("Fix the bug")
        };
        let resolved = mgr.resolve_ink(req).unwrap();
        // Codex doesn't support system_prompt, so instructions are prepended to prompt
        assert!(resolved.system_prompt.is_none());
        assert_eq!(
            resolved.prompt.as_deref(),
            Some("You are an expert coder.\n\nFix the bug")
        );
        assert_eq!(resolved.provider, Some(Provider::Codex));
    }

    #[test]
    fn test_resolve_ink_request_overrides() {
        let mut inks = HashMap::new();
        inks.insert(
            "reviewer".into(),
            crate::config::InkConfig {
                description: None,
                provider: Some("claude".into()),
                model: None,
                mode: Some("autonomous".into()),
                unrestricted: Some(false),
                instructions: Some("Review code".into()),
            },
        );
        let mgr = SessionManager {
            backend: Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            culture_repo: None,
            inject_culture: false,
            curator_enabled: false,
            curator_provider: None,
            store: unsafe_empty_store(),
            default_guard: GuardConfig::default(),
            inks,
            event_tx: None,
            default_provider: None,
            session_defaults: SessionDefaultsConfig::default(),
            node_name: String::new(),
        };
        let req = CreateSessionRequest {
            provider: Some(Provider::Codex),
            mode: Some(SessionMode::Interactive),
            unrestricted: Some(true),
            model: Some("opus".into()),
            allowed_tools: Some(vec!["Bash".into()]),
            system_prompt: Some("Explicit prompt".into()),
            ink: Some("reviewer".into()),
            ..make_req("test")
        };
        let resolved = mgr.resolve_ink(req).unwrap();
        // Explicit request values win
        assert_eq!(resolved.provider, Some(Provider::Codex));
        assert_eq!(resolved.model, Some("opus".into()));
        assert_eq!(resolved.mode, Some(SessionMode::Interactive));
        assert_eq!(resolved.unrestricted, Some(true));
        assert_eq!(resolved.allowed_tools, Some(vec!["Bash".into()]));
        // Explicit system_prompt wins over ink instructions
        assert_eq!(resolved.system_prompt, Some("Explicit prompt".into()));
    }

    #[test]
    fn test_resolve_ink_explicit_unrestricted_blocks_ink() {
        let mut inks = HashMap::new();
        inks.insert(
            "coder".into(),
            crate::config::InkConfig {
                description: None,
                provider: None,
                model: None,
                mode: None,
                unrestricted: Some(false),
                instructions: None,
            },
        );
        let mgr = SessionManager {
            backend: Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            culture_repo: None,
            inject_culture: false,
            curator_enabled: false,
            curator_provider: None,
            store: unsafe_empty_store(),
            default_guard: GuardConfig::default(),
            inks,
            event_tx: None,
            default_provider: None,
            session_defaults: SessionDefaultsConfig::default(),
            node_name: String::new(),
        };
        let req = CreateSessionRequest {
            unrestricted: Some(true),
            ink: Some("coder".into()),
            ..make_req("test")
        };
        let resolved = mgr.resolve_ink(req).unwrap();
        // Explicit unrestricted=true wins over ink's unrestricted=false
        assert_eq!(resolved.unrestricted, Some(true));
    }

    #[test]
    fn test_resolve_ink_applies_unrestricted_from_ink() {
        let mut inks = HashMap::new();
        inks.insert(
            "safe-agent".into(),
            crate::config::InkConfig {
                description: Some("A safe agent".into()),
                provider: Some("claude".into()),
                model: None,
                mode: None,
                unrestricted: Some(false),
                instructions: Some("Be careful".into()),
            },
        );
        let mgr = SessionManager {
            backend: Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            culture_repo: None,
            inject_culture: false,
            curator_enabled: false,
            curator_provider: None,
            store: unsafe_empty_store(),
            default_guard: GuardConfig::default(),
            inks,
            event_tx: None,
            default_provider: None,
            session_defaults: SessionDefaultsConfig::default(),
            node_name: String::new(),
        };
        let req = CreateSessionRequest {
            ink: Some("safe-agent".into()),
            ..make_req("test")
        };
        let resolved = mgr.resolve_ink(req).unwrap();
        assert_eq!(resolved.provider, Some(Provider::Claude));
        assert_eq!(resolved.unrestricted, Some(false));
        // Claude supports system_prompt, so instructions → system_prompt
        assert_eq!(resolved.system_prompt, Some("Be careful".into()));
    }

    #[test]
    fn test_resolve_ink_explicit_guardrails_win() {
        let mut inks = HashMap::new();
        inks.insert(
            "safe-agent".into(),
            crate::config::InkConfig {
                description: None,
                provider: Some("claude".into()),
                model: None,
                mode: None,
                unrestricted: None,
                instructions: Some("Ink instructions".into()),
            },
        );
        let mgr = SessionManager {
            backend: Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            culture_repo: None,
            inject_culture: false,
            curator_enabled: false,
            curator_provider: None,
            store: unsafe_empty_store(),
            default_guard: GuardConfig::default(),
            inks,
            event_tx: None,
            default_provider: None,
            session_defaults: SessionDefaultsConfig::default(),
            node_name: String::new(),
        };
        let req = CreateSessionRequest {
            ink: Some("safe-agent".into()),
            max_turns: Some(3),
            max_budget_usd: Some(1.0),
            output_format: Some("stream-json".into()),
            ..make_req("test")
        };
        let resolved = mgr.resolve_ink(req).unwrap();
        // Explicit request guardrail values pass through (ink doesn't set them)
        assert_eq!(resolved.max_turns, Some(3));
        assert_eq!(resolved.max_budget_usd, Some(1.0));
        assert_eq!(resolved.output_format, Some("stream-json".into()));
        // Ink defaults applied for fields not set in request
        assert_eq!(resolved.provider, Some(Provider::Claude));
        // model is not set by inks (model is per-session only)
        assert_eq!(resolved.model, None);
        // Instructions → system_prompt (Claude provider)
        assert_eq!(resolved.system_prompt, Some("Ink instructions".into()));
    }

    /// Helper to create a `SessionManager` without a valid store (for sync-only tests).
    fn unsafe_empty_store() -> Store {
        // We can't create a real Store synchronously, so we use a trick:
        // create a tokio runtime briefly just for the Store creation.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let tmpdir = tempfile::tempdir().unwrap();
            let tmpdir = Box::leak(Box::new(tmpdir));
            Store::new(tmpdir.path().to_str().unwrap()).await.unwrap()
        })
    }

    #[tokio::test]
    async fn test_with_event_tx_builder() {
        let (mgr, _, _) = test_manager(MockBackend::new()).await;
        assert!(mgr.event_tx.is_none());
        assert!(mgr.node_name.is_empty());

        let (tx, _) = broadcast::channel(16);
        let mgr = mgr.with_event_tx(tx, "test-node".into());
        assert!(mgr.event_tx.is_some());
        assert_eq!(mgr.node_name, "test-node");
    }

    #[tokio::test]
    async fn test_emit_event_with_tx() {
        let (mgr, _, _) = test_manager(MockBackend::new()).await;
        let (tx, mut rx) = broadcast::channel(16);
        let mgr = mgr.with_event_tx(tx, "node-1".into());

        let session = Session {
            id: Uuid::new_v4(),
            name: "test-session".into(),
            workdir: "/tmp".into(),
            provider: Provider::Claude,
            prompt: "fix bug".into(),
            status: SessionStatus::Active,
            mode: SessionMode::Autonomous,
            conversation_id: None,
            exit_code: None,
            backend_session_id: None,
            output_snapshot: Some("output".into()),
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_code: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        mgr.emit_event(&session, Some(SessionStatus::Creating));
        let event = unwrap_session_event(rx.recv().await.unwrap());
        assert_eq!(event.session_name, "test-session");
        assert_eq!(event.status, "active");
        assert_eq!(event.previous_status, Some("creating".into()));
        assert_eq!(event.node_name, "node-1");
        assert_eq!(event.output_snippet, Some("output".into()));
    }

    #[tokio::test]
    async fn test_emit_event_without_tx_is_noop() {
        let (mgr, _, _) = test_manager(MockBackend::new()).await;
        // No event_tx set — emit_event should not panic
        let session = Session {
            id: Uuid::new_v4(),
            name: "s".into(),
            workdir: "/tmp".into(),
            provider: Provider::Claude,
            prompt: "p".into(),
            status: SessionStatus::Active,
            mode: SessionMode::Autonomous,
            conversation_id: None,
            exit_code: None,
            backend_session_id: None,
            output_snapshot: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_code: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        mgr.emit_event(&session, None);
    }

    #[tokio::test]
    async fn test_create_session_emits_event() {
        let (mgr, _, _) = test_manager(MockBackend::new()).await;
        let (tx, mut rx) = broadcast::channel(16);
        let mgr = mgr.with_event_tx(tx, "n".into());

        let _ = mgr.create_session(make_req("do it")).await.unwrap();
        let event = unwrap_session_event(rx.recv().await.unwrap());
        assert_eq!(event.status, "active");
        assert_eq!(event.previous_status, Some("creating".into()));
    }

    #[tokio::test]
    async fn test_kill_session_emits_event() {
        let (mgr, _, _) = test_manager(MockBackend::new()).await;
        let (tx, mut rx) = broadcast::channel(16);
        let mgr = mgr.with_event_tx(tx, "n".into());

        let (session, _) = mgr.create_session(make_req("work")).await.unwrap();
        // Drain the create event
        let _ = rx.recv().await.unwrap();

        mgr.kill_session(&session.id.to_string()).await.unwrap();
        let event = unwrap_session_event(rx.recv().await.unwrap());
        assert_eq!(event.status, "killed");
        assert_eq!(event.previous_status, Some("active".into()));
    }

    #[tokio::test]
    async fn test_get_session_stale_emits_event() {
        let backend = MockBackend::new().with_alive(false);
        let (mgr, _, _) = test_manager(backend).await;
        let (tx, mut rx) = broadcast::channel(16);
        let mgr = mgr.with_event_tx(tx, "n".into());

        // Insert a session that appears "active" in DB
        let req = CreateSessionRequest {
            mode: Some(SessionMode::Interactive),
            ..make_req("test")
        };
        // create_session will fail because is_alive returns false, but we need it in DB.
        // Actually create_session calls backend.create_session (not is_alive), so it succeeds.
        let (session, _) = mgr.create_session(req).await.unwrap();
        // Drain create event
        let _ = rx.recv().await.unwrap();

        // Now get_session checks is_alive → false → marks stale → emits event
        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Lost);

        let event = unwrap_session_event(rx.recv().await.unwrap());
        assert_eq!(event.status, "lost");
        assert_eq!(event.previous_status, Some("active".into()));
    }

    #[tokio::test]
    async fn test_resume_session_emits_event() {
        let (mgr, _, _) = test_manager(MockBackend::new().with_alive(false)).await;
        let (tx, mut rx) = broadcast::channel(16);
        let mgr = mgr.with_event_tx(tx, "n".into());

        let (session, _) = mgr.create_session(make_req("work")).await.unwrap();
        let _ = rx.recv().await.unwrap(); // drain create event

        // Mark stale via get_session
        let _ = mgr.get_session(&session.id.to_string()).await.unwrap();
        let _ = rx.recv().await.unwrap(); // drain stale event

        // Now resume — but we need the backend to succeed on create_session
        // The MockBackend already has create_result = Ok, and is_alive doesn't matter for resume
        let resumed = mgr.resume_session(&session.id.to_string()).await.unwrap();
        assert_eq!(resumed.status, SessionStatus::Active);

        let event = unwrap_session_event(rx.recv().await.unwrap());
        assert_eq!(event.status, "active");
        assert_eq!(event.previous_status, Some("lost".into()));
    }

    #[tokio::test]
    async fn test_emit_event_no_subscribers() {
        let (mgr, _, _) = test_manager(MockBackend::new()).await;
        let (tx, rx) = broadcast::channel::<PulpoEvent>(16);
        let mgr = mgr.with_event_tx(tx, "n".into());
        // Drop the only receiver
        drop(rx);

        // emit_event should not panic even with no subscribers
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();
        // Just verify the session was created successfully (emit silently failed)
        assert_eq!(session.status, SessionStatus::Active);
    }

    #[tokio::test]
    async fn test_delete_dead_session() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();

        // Kill first, then delete
        mgr.kill_session(&id).await.unwrap();
        mgr.delete_session(&id).await.unwrap();

        // Session should be gone from the database
        let fetched = mgr.get_session(&id).await.unwrap();
        assert!(fetched.is_none());
    }

    #[tokio::test]
    async fn test_delete_completed_session() {
        let (mgr, _, pool) = test_manager(MockBackend::new()).await;
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();

        // Manually set status to Finished
        sqlx::query("UPDATE sessions SET status = 'finished' WHERE id = ?")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();

        mgr.delete_session(&id).await.unwrap();
        assert!(mgr.get_session(&id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_delete_stale_session() {
        let (mgr, _, pool) = test_manager(MockBackend::new()).await;
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();

        // Manually set status to Lost
        sqlx::query("UPDATE sessions SET status = 'lost' WHERE id = ?")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();

        mgr.delete_session(&id).await.unwrap();
        assert!(mgr.get_session(&id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_delete_running_session_rejected() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();

        let result = mgr.delete_session(&id).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot delete"));
    }

    #[tokio::test]
    async fn test_delete_session_not_found() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let result = mgr.delete_session("nonexistent").await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("session not found")
        );
    }

    #[tokio::test]
    async fn test_delete_session_store_failure() {
        let (mgr, _, pool) = test_manager(MockBackend::new()).await;
        sqlx::query("DROP TABLE sessions")
            .execute(&pool)
            .await
            .unwrap();
        let result = mgr.delete_session("test").await;
        assert!(result.is_err());
    }

    // -- build_culture_context tests --

    fn make_culture_item(title: &str, kind: pulpo_common::culture::CultureKind) -> Culture {
        Culture {
            id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            kind,
            scope_repo: Some("/tmp/repo".into()),
            scope_ink: None,
            title: title.into(),
            body: "Details here.".into(),
            tags: vec![],
            relevance: 0.5,
            created_at: Utc::now(),
            last_referenced_at: None,
            reference_count: 0,
        }
    }

    #[tokio::test]
    async fn test_build_culture_context_empty() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();
        let root = repo.root().display().to_string();
        let ctx = build_culture_context(&repo, "/tmp/repo", None, &root, "test-session");
        assert!(ctx.contains("No previous findings"));
        assert!(ctx.contains("pending/"));
        assert!(ctx.contains("Write-back"));
    }

    #[tokio::test]
    async fn test_build_culture_context_with_items() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();
        repo.save(&make_culture_item(
            "Auth race condition",
            pulpo_common::culture::CultureKind::Failure,
        ))
        .await
        .unwrap();
        repo.save(&make_culture_item(
            "Uses pnpm not npm",
            pulpo_common::culture::CultureKind::Summary,
        ))
        .await
        .unwrap();

        let root = repo.root().display().to_string();
        let ctx = build_culture_context(&repo, "/tmp/repo", None, &root, "test-session");
        assert!(ctx.contains("[failure] Auth race condition"));
        assert!(ctx.contains("[summary] Uses pnpm not npm"));
        assert!(!ctx.contains("No previous findings"));
    }

    #[tokio::test]
    async fn test_build_culture_context_includes_repo_root() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();
        let root = repo.root().display().to_string();
        let ctx =
            build_culture_context(&repo, "/home/user/my-project", None, &root, "test-session");
        assert!(ctx.contains(&root));
    }

    #[tokio::test]
    async fn test_build_culture_context_write_back_instructions() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();
        let root = repo.root().display().to_string();
        let ctx = build_culture_context(&repo, "/tmp/repo", None, &root, "test-session");
        assert!(ctx.contains("Write-back: share your learnings"));
        assert!(ctx.contains("pending/"));
        assert!(ctx.contains(".md"));
        assert!(ctx.contains("non-obvious"));
        assert!(ctx.contains("Pulpo validates"));
    }

    #[tokio::test]
    async fn test_build_culture_context_quality_examples() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();
        let root = repo.root().display().to_string();
        let ctx = build_culture_context(&repo, "/tmp/repo", None, &root, "test-session");
        assert!(ctx.contains("Good example"));
        assert!(ctx.contains("Bad example"));
        assert!(ctx.contains("would be rejected"));
        assert!(ctx.contains("10-120 characters"));
        assert!(ctx.contains("at least 30 characters"));
    }

    #[tokio::test]
    async fn test_build_culture_context_supersedes_instruction() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();
        let root = repo.root().display().to_string();
        let ctx = build_culture_context(&repo, "/tmp/repo", None, &root, "test-session");
        assert!(ctx.contains("supersedes:"));
        assert!(ctx.contains("corrects or replaces"));
    }

    #[tokio::test]
    async fn test_build_culture_context_pending_path_uses_session_name() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();
        let root = repo.root().display().to_string();
        let ctx = build_culture_context(&repo, "/tmp/repo", None, &root, "indigo-wave");
        // The pending path should reference the culture repo root and session name
        let expected = format!("{root}/pending/indigo-wave.md");
        assert!(ctx.contains(&expected));
    }

    #[tokio::test]
    async fn test_build_culture_context_merges_scopes() {
        let tmpdir = tempfile::tempdir().unwrap();
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        // Add a global culture entry
        let mut global_item = make_culture_item(
            "Global convention",
            pulpo_common::culture::CultureKind::Summary,
        );
        global_item.scope_repo = None;
        repo.save(&global_item).await.unwrap();

        // Add a repo-scoped entry
        repo.save(&make_culture_item(
            "Repo finding",
            pulpo_common::culture::CultureKind::Summary,
        ))
        .await
        .unwrap();

        // Add an ink-scoped entry
        let mut ink_item =
            make_culture_item("Ink pattern", pulpo_common::culture::CultureKind::Summary);
        ink_item.scope_repo = None;
        ink_item.scope_ink = Some("coder".into());
        repo.save(&ink_item).await.unwrap();

        let root = repo.root().display().to_string();
        let ctx = build_culture_context(&repo, "/tmp/repo", Some("coder"), &root, "test-session");

        // Should contain all three scopes
        assert!(ctx.contains("Global culture"));
        assert!(ctx.contains("Global convention"));
        assert!(ctx.contains("Repository: repo"));
        assert!(ctx.contains("Repo finding"));
        assert!(ctx.contains("Ink: coder"));
        assert!(ctx.contains("Ink pattern"));
        assert!(!ctx.contains("No previous findings"));
    }

    // -- build_curator_prompt tests --

    #[test]
    fn test_build_curator_prompt_contains_session_name() {
        let prompt = build_curator_prompt("indigo-wave", "some output", "/tmp/culture");
        assert!(prompt.contains("indigo-wave"));
        assert!(prompt.contains("some output"));
        assert!(prompt.contains("/tmp/culture/pending/indigo-wave.md"));
    }

    #[test]
    fn test_build_curator_prompt_contains_instructions() {
        let prompt = build_curator_prompt("test-session", "output", "/root");
        assert!(prompt.contains("culture curator"));
        assert!(prompt.contains("non-obvious"));
        assert!(prompt.contains("Do NOT modify any code"));
    }

    #[test]
    fn test_build_curator_prompt_contains_quality_guidance() {
        let prompt = build_curator_prompt("test-session", "output", "/root");
        assert!(prompt.contains("10-120 chars"));
        assert!(prompt.contains("at least 30 chars"));
        assert!(prompt.contains("rejected"));
    }

    #[test]
    fn test_is_empty_agents_md() {
        assert!(is_empty_agents_md(
            "## Session Learnings\n\n<!-- No learnings yet -->\n"
        ));
        assert!(is_empty_agents_md(
            "# Culture\n\n## Commands\n\n## Testing\n"
        ));
        assert!(!is_empty_agents_md(
            "## Session Learnings\n\n### [summary] A finding\n"
        ));
        assert!(!is_empty_agents_md("### [failure] Some bug"));
    }

    // -- inject_culture_context tests --

    #[test]
    fn test_inject_culture_disabled() {
        let mgr = SessionManager {
            backend: Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            culture_repo: None,
            inject_culture: false,
            curator_enabled: false,
            curator_provider: None,
            store: unsafe_empty_store(),
            default_guard: GuardConfig::default(),
            inks: HashMap::new(),
            event_tx: None,
            default_provider: None,
            session_defaults: SessionDefaultsConfig::default(),
            node_name: String::new(),
        };
        let req = make_req("test");
        let result = mgr.inject_culture_context(req, "test-session");
        // No modification when disabled
        assert_eq!(result.prompt.as_deref(), Some("test"));
        assert_eq!(result.system_prompt, None);
    }

    #[test]
    fn test_inject_culture_no_repo() {
        let mgr = SessionManager {
            backend: Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            culture_repo: None,
            inject_culture: true,
            curator_enabled: false,
            curator_provider: None,
            store: unsafe_empty_store(),
            default_guard: GuardConfig::default(),
            inks: HashMap::new(),
            event_tx: None,
            default_provider: None,
            session_defaults: SessionDefaultsConfig::default(),
            node_name: String::new(),
        };
        let req = make_req("test");
        let result = mgr.inject_culture_context(req, "test-session");
        assert_eq!(result.prompt.as_deref(), Some("test"));
        assert_eq!(result.system_prompt, None);
    }

    async fn async_store() -> Store {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        store
    }

    #[tokio::test]
    async fn test_inject_culture_claude_appends_system_prompt() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();
        repo.save(&make_culture_item(
            "DB needs migration",
            pulpo_common::culture::CultureKind::Summary,
        ))
        .await
        .unwrap();

        let mgr = SessionManager {
            backend: Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            culture_repo: Some(repo),
            inject_culture: true,
            curator_enabled: false,
            curator_provider: None,
            store: async_store().await,
            default_guard: GuardConfig::default(),
            inks: HashMap::new(),
            event_tx: None,
            default_provider: None,
            session_defaults: SessionDefaultsConfig::default(),
            node_name: String::new(),
        };
        let mut req = make_req("test");
        req.provider = Some(Provider::Claude);
        req.workdir = Some("/tmp/repo".into());
        let result = mgr.inject_culture_context(req, "test-session");
        // Claude: culture goes to system_prompt
        let sp = result.system_prompt.unwrap();
        assert!(sp.contains("DB needs migration"));
        assert!(sp.contains("Write-back: share your learnings"));
    }

    #[tokio::test]
    async fn test_inject_culture_codex_prepends_prompt() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();
        repo.save(&make_culture_item(
            "Use pnpm",
            pulpo_common::culture::CultureKind::Summary,
        ))
        .await
        .unwrap();

        let mgr = SessionManager {
            backend: Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            culture_repo: Some(repo),
            inject_culture: true,
            curator_enabled: false,
            curator_provider: None,
            store: async_store().await,
            default_guard: GuardConfig::default(),
            inks: HashMap::new(),
            event_tx: None,
            default_provider: None,
            session_defaults: SessionDefaultsConfig::default(),
            node_name: String::new(),
        };
        let mut req = make_req("test");
        req.provider = Some(Provider::Codex);
        req.workdir = Some("/tmp/repo".into());
        let result = mgr.inject_culture_context(req, "test-session");
        // Codex: culture prepended to prompt
        let prompt = result.prompt.as_ref().unwrap();
        assert!(prompt.starts_with("## Culture from previous sessions"));
        assert!(prompt.contains("Use pnpm"));
        assert!(prompt.ends_with("test"));
        assert!(result.system_prompt.is_none());
    }

    #[tokio::test]
    async fn test_inject_culture_preserves_existing_system_prompt() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let repo = CultureRepo::init(tmpdir.path().to_str().unwrap(), None)
            .await
            .unwrap();

        let mgr = SessionManager {
            backend: Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            culture_repo: Some(repo),
            inject_culture: true,
            curator_enabled: false,
            curator_provider: None,
            store: async_store().await,
            default_guard: GuardConfig::default(),
            inks: HashMap::new(),
            event_tx: None,
            default_provider: None,
            session_defaults: SessionDefaultsConfig::default(),
            node_name: String::new(),
        };
        let mut req = make_req("test");
        req.provider = Some(Provider::Claude);
        req.workdir = Some("/tmp/repo".into());
        req.system_prompt = Some("Be careful with auth module.".into());
        let result = mgr.inject_culture_context(req, "test-session");
        let sp = result.system_prompt.unwrap();
        // Existing system prompt preserved, culture appended
        assert!(sp.starts_with("Be careful with auth module."));
        assert!(sp.contains("Culture from previous sessions"));
    }

    // ───────────────────────────────────────────────────────────
    // Stale / dead edge-case tests
    // ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_kill_backend_error_leaves_session_running() {
        // When backend.kill_session fails, the session must NOT be marked Dead.
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_kill_error()).await;
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();

        let result = mgr.kill_session(&id).await;
        assert!(result.is_err());

        // Session should still be Running (not Dead)
        let fetched = mgr.store.get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Active);
    }

    #[tokio::test]
    async fn test_kill_already_dead_session() {
        // Killing a session that is already Dead should still succeed
        // (backend kill is called, DB update is idempotent).
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();

        // First kill — succeeds
        mgr.kill_session(&id).await.unwrap();
        let fetched = mgr.store.get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Killed);

        // Second kill — backend kill called again, DB stays Dead
        backend.calls.lock().unwrap().clear();
        mgr.kill_session(&id).await.unwrap();
        let fetched = mgr.store.get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Killed);

        let has_kill = backend
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.starts_with("kill:"));
        assert!(has_kill);
    }

    #[tokio::test]
    async fn test_kill_stale_session() {
        // Killing a stale session should succeed even if backend session is already gone.
        let (mgr, backend, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();

        // Mark stale via get_session
        let fetched = mgr.get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Lost);

        // Kill the stale session — backend kill is still called (best-effort cleanup)
        backend.calls.lock().unwrap().clear();
        mgr.kill_session(&id).await.unwrap();
        let fetched = mgr.store.get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Killed);
    }

    #[tokio::test]
    async fn test_kill_stale_session_backend_error_propagates() {
        // When killing a stale session and backend.kill fails, error propagates
        // and session status should NOT change.
        let backend = MockBackend::new().with_alive(false);
        let (mgr, backend_ref, _pool) = test_manager(backend).await;
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();

        // Mark stale
        let _ = mgr.get_session(&id).await.unwrap().unwrap();

        // Now make kill fail
        *backend_ref.kill_result.lock().unwrap() = Err(anyhow!("kill failed"));
        let result = mgr.kill_session(&id).await;
        assert!(result.is_err());

        // Session remains Stale (not Dead, not Running)
        let fetched = mgr.store.get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Lost);
    }

    #[tokio::test]
    async fn test_resume_backend_failure_leaves_session_stale() {
        // When resume's backend.create_session fails, the session must stay Stale
        // (not be marked Running).
        let backend = MockBackend::new().with_alive(false);
        let (mgr, backend_ref, _pool) = test_manager(backend).await;
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();

        // Mark stale
        let _ = mgr.get_session(&id).await.unwrap();

        // Make create fail for resume
        *backend_ref.create_result.lock().unwrap() = Err(anyhow!("backend not found"));
        let result = mgr.resume_session(&id).await;
        assert!(result.is_err());

        // Session must still be Stale
        let fetched = mgr.store.get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Lost);
    }

    #[tokio::test]
    async fn test_get_session_reconciles_running_to_stale() {
        // Simulates daemon restart: session is Running in DB but the backend
        // process (tmux) is gone. get_session should detect this and mark Stale.
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();

        // Session was just created as Running. Backend says it's dead.
        let fetched = mgr.get_session(&id).await.unwrap().unwrap();
        assert_eq!(
            fetched.status,
            SessionStatus::Lost,
            "Running session with dead backend should be reconciled to Stale"
        );
    }

    #[tokio::test]
    async fn test_list_sessions_reconciles_running_to_stale() {
        // Same as above but via list_sessions — all Running sessions with dead
        // backend should be reconciled to Stale.
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let (s1, _) = mgr.create_session(make_req("test1")).await.unwrap();
        let (s2, _) = mgr.create_session(make_req("test2")).await.unwrap();

        let sessions = mgr.list_sessions().await.unwrap();
        for s in &sessions {
            assert_eq!(
                s.status,
                SessionStatus::Lost,
                "Session {} should be Stale after reconciliation",
                s.name
            );
        }

        // Verify DB is also updated (not just in-memory)
        let db_s1 = mgr
            .store
            .get_session(&s1.id.to_string())
            .await
            .unwrap()
            .unwrap();
        let db_s2 = mgr
            .store
            .get_session(&s2.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(db_s1.status, SessionStatus::Lost);
        assert_eq!(db_s2.status, SessionStatus::Lost);
    }

    #[tokio::test]
    async fn test_delete_session_tolerates_backend_kill_failure() {
        // delete_session does best-effort backend cleanup. Even if kill fails,
        // the session should still be deleted from the DB.
        let backend = MockBackend::new().with_alive(false);
        let (mgr, backend_ref, _pool) = test_manager(backend).await;
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();

        // Mark stale first (delete requires non-running status)
        let _ = mgr.get_session(&id).await.unwrap();

        // Make kill fail
        *backend_ref.kill_result.lock().unwrap() = Err(anyhow!("kill failed"));

        // Delete should still succeed (kill failure is best-effort)
        mgr.delete_session(&id).await.unwrap();
        let fetched = mgr.store.get_session(&id).await.unwrap();
        assert!(fetched.is_none(), "Session should be deleted from DB");
    }

    #[tokio::test]
    async fn test_resume_finished_session_succeeds() {
        // A finished session should be resumable (restarts agent).
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();

        // Mark finished
        mgr.store
            .update_session_status(&id, SessionStatus::Finished)
            .await
            .unwrap();

        let resumed = mgr.resume_session(&id).await.unwrap();
        assert_eq!(resumed.status, SessionStatus::Active);
    }

    #[tokio::test]
    async fn test_resume_dead_session_fails() {
        // A dead session should not be resumable.
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();

        mgr.kill_session(&id).await.unwrap();

        let result = mgr.resume_session(&id).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("cannot be resumed")
        );
    }

    #[tokio::test]
    async fn test_stale_detection_skips_non_running_statuses() {
        // check_and_mark_stale should only affect Running sessions.
        // Dead, Completed, Stale, Creating sessions should not be touched.
        let (mgr, _, _pool) = test_manager(MockBackend::new().with_alive(false)).await;
        let (session, _) = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();

        // Kill the session
        // (need alive=true for kill to proceed since we call backend.kill)
        // Actually the mock kill doesn't check is_alive, so this works.
        mgr.kill_session(&id).await.unwrap();

        // get_session on a Dead session should not transition it further
        let fetched = mgr.get_session(&id).await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Killed);
    }
}
