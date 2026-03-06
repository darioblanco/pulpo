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

use crate::backend::Backend;
use crate::config::PersonaConfig;
use crate::store::Store;

#[derive(Clone)]
pub struct SessionManager {
    backend: Arc<dyn Backend>,
    store: Store,
    default_guard: GuardConfig,
    default_max_turns: Option<u32>,
    default_max_budget_usd: Option<f64>,
    default_output_format: Option<String>,
    personas: HashMap<String, PersonaConfig>,
    event_tx: Option<broadcast::Sender<PulpoEvent>>,
    node_name: String,
}

impl SessionManager {
    pub fn new(
        backend: Arc<dyn Backend>,
        store: Store,
        default_guard: GuardConfig,
        personas: HashMap<String, PersonaConfig>,
    ) -> Self {
        Self {
            backend,
            store,
            default_guard,
            default_max_turns: None,
            default_max_budget_usd: None,
            default_output_format: None,
            personas,
            event_tx: None,
            node_name: String::new(),
        }
    }

    #[must_use]
    pub fn with_guardrail_defaults(
        mut self,
        max_turns: Option<u32>,
        max_budget_usd: Option<f64>,
        output_format: Option<String>,
    ) -> Self {
        self.default_max_turns = max_turns;
        self.default_max_budget_usd = max_budget_usd;
        self.default_output_format = output_format;
        self
    }

    #[must_use]
    pub fn with_event_tx(mut self, tx: broadcast::Sender<PulpoEvent>, node_name: String) -> Self {
        self.event_tx = Some(tx);
        self.node_name = node_name;
        self
    }

    pub const fn personas(&self) -> &HashMap<String, PersonaConfig> {
        &self.personas
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
                waiting_for_input: Some(session.waiting_for_input),
                timestamp: Utc::now().to_rfc3339(),
            };
            // Ignore send errors — no subscribers is OK
            let _ = tx.send(PulpoEvent::Session(event));
        }
    }

    pub async fn create_session(&self, req: CreateSessionRequest) -> Result<Session> {
        let mut req = self.resolve_persona(req)?;
        self.apply_guardrail_defaults(&mut req);
        validate_workdir(&req.workdir)?;
        let id = Uuid::new_v4();
        let provider = req.provider.unwrap_or(Provider::Claude);
        self.backend.check_provider(&provider.to_string())?;
        let mode = req.mode.unwrap_or_default();
        let guards = resolve_guard_config(&req, &self.default_guard);
        let name = req.name.unwrap_or_else(|| derive_name(&req.workdir));
        let mut spawn_params = build_spawn_params(
            &req.prompt,
            &guards,
            req.allowed_tools.as_deref(),
            req.model.as_deref(),
            req.system_prompt.as_deref(),
            req.max_turns,
            req.max_budget_usd,
            req.output_format.as_deref(),
        );
        spawn_params.worktree = Some(name.clone());
        let command = build_command(provider, mode, &spawn_params);

        let now = Utc::now();
        let session = Session {
            id,
            name: name.clone(),
            workdir: req.workdir,
            provider,
            prompt: req.prompt.clone(),
            status: SessionStatus::Creating,
            mode,
            conversation_id: None,
            exit_code: None,
            backend_session_id: Some(self.backend.session_id(&name)),
            output_snapshot: None,
            guard_config: Some(guards),
            model: req.model,
            allowed_tools: req.allowed_tools,
            system_prompt: req.system_prompt,
            metadata: req.metadata,
            persona: req.persona,
            max_turns: req.max_turns,
            max_budget_usd: req.max_budget_usd,
            output_format: req.output_format,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: now,
            updated_at: now,
        };

        self.store.insert_session(&session).await?;

        if let Err(e) = self
            .backend
            .create_session(&name, &session.workdir, &command)
        {
            self.store
                .update_session_status(&id.to_string(), SessionStatus::Dead)
                .await?;
            return Err(e);
        }

        self.store
            .update_session_status(&id.to_string(), SessionStatus::Running)
            .await?;

        // Set up output logging
        let log_dir = format!("{}/logs", self.store.data_dir());
        let _ = std::fs::create_dir_all(&log_dir);
        let log_path = format!("{log_dir}/{id}.log");
        let _ = self.backend.setup_logging(&name, &log_path);

        // Return the session with updated status (avoids unnecessary re-fetch)
        let mut session = session;
        session.status = SessionStatus::Running;
        session.updated_at = Utc::now();
        self.emit_event(&session, Some(SessionStatus::Creating));
        Ok(session)
    }

    fn resolve_persona(&self, mut req: CreateSessionRequest) -> Result<CreateSessionRequest> {
        let persona_name = match &req.persona {
            Some(name) => name.clone(),
            None => return Ok(req),
        };
        let persona = self
            .personas
            .get(&persona_name)
            .ok_or_else(|| anyhow!("unknown persona: {persona_name}"))?;

        // Persona defaults — explicit request fields always win
        if req.provider.is_none() {
            req.provider = persona.provider.as_ref().and_then(|p| p.parse().ok());
        }
        if req.model.is_none() {
            req.model.clone_from(&persona.model);
        }
        if req.mode.is_none() {
            req.mode = persona.mode.as_ref().and_then(|m| m.parse().ok());
        }
        if req.guard_preset.is_none() && req.guard_config.is_none() {
            req.guard_preset = persona.guard_preset.as_ref().and_then(|g| g.parse().ok());
        }
        if req.allowed_tools.is_none() {
            req.allowed_tools.clone_from(&persona.allowed_tools);
        }
        if req.system_prompt.is_none() {
            req.system_prompt.clone_from(&persona.system_prompt);
        }
        if req.max_turns.is_none() {
            req.max_turns = persona.max_turns;
        }
        if req.max_budget_usd.is_none() {
            req.max_budget_usd = persona.max_budget_usd;
        }
        if req.output_format.is_none() {
            req.output_format.clone_from(&persona.output_format);
        }
        Ok(req)
    }

    fn apply_guardrail_defaults(&self, req: &mut CreateSessionRequest) {
        if req.max_turns.is_none() {
            req.max_turns = self.default_max_turns;
        }
        if req.max_budget_usd.is_none() {
            req.max_budget_usd = self.default_max_budget_usd;
        }
        if req.output_format.is_none() {
            req.output_format.clone_from(&self.default_output_format);
        }
    }

    pub async fn get_session(&self, id: &str) -> Result<Option<Session>> {
        let session = self.store.get_session(id).await?;
        match session {
            Some(mut s) => {
                if self.check_and_mark_stale(&mut s).await? {
                    self.emit_event(&s, Some(SessionStatus::Running));
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
        if session.status != SessionStatus::Running {
            return Ok(false);
        }
        let alive = self.backend.is_alive(&session.name)?;
        if alive {
            return Ok(false);
        }
        self.store
            .update_session_status(&session.id.to_string(), SessionStatus::Stale)
            .await?;
        session.status = SessionStatus::Stale;
        Ok(true)
    }

    pub async fn kill_session(&self, id: &str) -> Result<()> {
        let session = self
            .store
            .get_session(id)
            .await?
            .ok_or_else(|| anyhow!("session not found: {id}"))?;

        if let Err(e) = self.backend.kill_session(&session.name) {
            bail!("failed to kill session: {e}");
        }

        let previous = session.status;
        let session_id = session.id.to_string();
        self.store
            .update_session_status(&session_id, SessionStatus::Dead)
            .await?;
        let mut dead_session = session;
        dead_session.status = SessionStatus::Dead;
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
            SessionStatus::Running | SessionStatus::Creating => {
                bail!(
                    "cannot delete session in '{}' state — kill it first",
                    session.status
                );
            }
            _ => {}
        }

        // Best-effort cleanup of any lingering backend session
        let _ = self.backend.kill_session(&session.name);

        self.store.delete_session(&session.id.to_string()).await?;
        Ok(())
    }

    pub fn capture_output(&self, id: &str, name: &str, lines: usize) -> String {
        self.backend
            .capture_output(name, lines)
            .unwrap_or_else(|_| self.read_log_tail(id, lines))
    }

    fn read_log_tail(&self, id: &str, lines: usize) -> String {
        let log_path = format!("{}/logs/{id}.log", self.store.data_dir());
        let content = std::fs::read_to_string(&log_path).unwrap_or_default();
        let mut tail: Vec<&str> = content.lines().rev().take(lines).collect();
        tail.reverse();
        tail.join("\n")
    }

    pub fn send_input(&self, id: &str, name: &str, text: &str) -> Result<()> {
        let _ = id;
        self.backend.send_input(name, text)
    }

    pub async fn resume_session(&self, id: &str) -> Result<Session> {
        let session = self
            .store
            .get_session(id)
            .await?
            .ok_or_else(|| anyhow!("session not found: {id}"))?;

        if session.status != SessionStatus::Stale {
            bail!("session is not stale (status: {})", session.status);
        }

        // If the backend session is still alive, just re-mark it as running.
        // Only recreate the session if the backend process is gone.
        let alive = self.backend.is_alive(&session.name)?;
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
            );
            spawn_params.worktree = Some(session.name.clone());
            spawn_params
                .conversation_id
                .clone_from(&session.conversation_id);
            let command = build_command(session.provider, session.mode, &spawn_params);

            self.backend
                .create_session(&session.name, &session.workdir, &command)?;
        }

        let session_id = session.id.to_string();
        self.store
            .update_session_status(&session_id, SessionStatus::Running)
            .await?;

        let mut session = session;
        session.status = SessionStatus::Running;
        session.updated_at = Utc::now();
        self.emit_event(&session, Some(SessionStatus::Stale));
        Ok(session)
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

fn derive_name(workdir: &str) -> String {
    std::path::Path::new(workdir)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("session")
        .to_owned()
}

fn resolve_guard_config(req: &CreateSessionRequest, default: &GuardConfig) -> GuardConfig {
    req.guard_config
        .clone()
        .or_else(|| req.guard_preset.map(|p| GuardConfig { preset: p }))
        .unwrap_or_else(|| default.clone())
}

fn build_spawn_params(
    prompt: &str,
    guards: &GuardConfig,
    allowed_tools: Option<&[String]>,
    model: Option<&str>,
    system_prompt: Option<&str>,
    max_turns: Option<u32>,
    max_budget_usd: Option<f64>,
    output_format: Option<&str>,
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
        conversation_id: None,
    }
}

pub(crate) fn build_command(
    provider: Provider,
    mode: SessionMode,
    params: &crate::guard::SpawnParams,
) -> String {
    let binary = provider.to_string();
    let flags = crate::guard::build_flags(provider, mode, params);
    match mode {
        SessionMode::Interactive => {
            format!("{binary} {}", flags.join(" "))
        }
        SessionMode::Autonomous => {
            let inner = format!("{binary} {}", flags.join(" "));
            format!("bash -c '{inner}; echo \"[pulpo] Agent exited ($?)\"; exec bash'")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulpo_common::event::SessionEvent;
    use pulpo_common::guard::GuardPreset;
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
        fn session_id(&self, name: &str) -> String {
            name.to_owned()
        }
        fn spawn_attach(&self, _: &str) -> anyhow::Result<tokio::process::Child> {
            anyhow::bail!("not supported in mock")
        }
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
        fn session_id(&self, name: &str) -> String {
            name.to_owned()
        }
        fn spawn_attach(&self, _: &str) -> anyhow::Result<tokio::process::Child> {
            anyhow::bail!("not supported in mock")
        }
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
            workdir: "/tmp".into(),
            provider: None,
            prompt: prompt.into(),
            mode: None,
            guard_preset: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
        }
    }

    #[tokio::test]
    async fn test_create_session_defaults() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr.create_session(make_req("Fix the bug")).await.unwrap();

        assert_eq!(session.name, "tmp");
        assert_eq!(session.provider, Provider::Claude);
        assert_eq!(session.mode, SessionMode::Interactive);
        assert_eq!(session.status, SessionStatus::Running);
        assert_eq!(session.workdir, "/tmp");
        assert_eq!(session.prompt, "Fix the bug");
        // MockBackend.session_id() returns just the name
        assert_eq!(session.backend_session_id, Some("tmp".into()));

        let calls = backend.calls.lock().unwrap();
        // Interactive Claude: create session with prompt as positional arg, then setup logging
        assert!(calls[0].contains("create:tmp:/tmp:claude"));
        assert!(calls[0].contains("Fix the bug"));
        assert!(calls[1].starts_with("setup_logging:tmp:"));
        assert_eq!(calls.len(), 2);
        drop(calls);
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
            name: Some("custom-name".into()),
            workdir: "/tmp".into(),
            provider: None,
            prompt: "test".into(),
            mode: None,
            guard_preset: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        assert_eq!(session.name, "custom-name");
    }

    #[tokio::test]
    async fn test_create_session_autonomous() {
        let (mgr, backend, _pool) = test_manager(MockBackend::new()).await;
        let req = CreateSessionRequest {
            name: None,
            workdir: "/tmp".into(),
            provider: Some(Provider::Claude),
            prompt: "Do something".into(),
            mode: Some(SessionMode::Autonomous),
            guard_preset: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        assert_eq!(session.mode, SessionMode::Autonomous);
        // Default guard is Standard — uses --allowedTools, not --dangerously-skip-permissions
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
            name: None,
            workdir: "/tmp".into(),
            provider: Some(Provider::Claude),
            prompt: "Do something".into(),
            mode: Some(SessionMode::Autonomous),
            guard_preset: Some(pulpo_common::guard::GuardPreset::Unrestricted),
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
        };
        let session = mgr.create_session(req).await.unwrap();
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
        let session = mgr.create_session(make_req("test")).await.unwrap();

        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.guard_config.is_some());
        let gc = fetched.guard_config.unwrap();
        // Default is Standard preset
        assert_eq!(gc.preset, pulpo_common::guard::GuardPreset::Standard);
    }

    #[tokio::test]
    async fn test_create_session_with_guard_preset() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let req = CreateSessionRequest {
            name: None,
            workdir: "/tmp".into(),
            provider: None,
            prompt: "test".into(),
            mode: None,
            guard_preset: Some(pulpo_common::guard::GuardPreset::Strict),
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        let gc = session.guard_config.unwrap();
        assert_eq!(gc.preset, pulpo_common::guard::GuardPreset::Strict);
    }

    #[tokio::test]
    async fn test_create_session_with_guard_config_override() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let custom = GuardConfig {
            preset: pulpo_common::guard::GuardPreset::Unrestricted,
        };
        let req = CreateSessionRequest {
            name: None,
            workdir: "/tmp".into(),
            provider: None,
            prompt: "test".into(),
            mode: None,
            guard_preset: Some(pulpo_common::guard::GuardPreset::Strict),
            guard_config: Some(custom.clone()),
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
        };
        let session = mgr.create_session(req).await.unwrap();
        // guard_config takes precedence over guard_preset
        let gc = session.guard_config.unwrap();
        assert_eq!(gc.preset, pulpo_common::guard::GuardPreset::Unrestricted);
    }

    #[tokio::test]
    async fn test_create_session_workdir_not_found() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let req = CreateSessionRequest {
            workdir: "/nonexistent/path/that/does/not/exist".into(),
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
            workdir: path,
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
        assert_eq!(sessions[0].status, SessionStatus::Dead);
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
        assert_eq!(fetched.status, SessionStatus::Running);
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
        assert_eq!(fetched.status, SessionStatus::Stale);
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
        assert_eq!(sessions[0].status, SessionStatus::Stale);
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
        assert_eq!(fetched.status, SessionStatus::Dead);
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
        mgr.send_input("some-id", "my-session", "hello").unwrap();

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
    fn test_derive_name() {
        assert_eq!(derive_name("/tmp/my-project"), "my-project");
        assert_eq!(derive_name("/home/user/code/api"), "api");
        assert_eq!(derive_name("repo"), "repo");
    }

    #[test]
    fn test_derive_name_root() {
        assert_eq!(derive_name("/"), "session");
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
        assert_eq!(fetched.status, SessionStatus::Stale);

        // Now resume it — backend session is still alive, so it should skip create_session
        *backend.alive.lock().unwrap() = true;
        backend.calls.lock().unwrap().clear();
        let resumed = mgr.resume_session(&session.id.to_string()).await.unwrap();
        assert_eq!(resumed.status, SessionStatus::Running);

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
        assert_eq!(resumed.status, SessionStatus::Running);

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

        // Session is Running, not Stale
        let result = mgr.resume_session(&session.id.to_string()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not stale"));
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
        let session = mgr.create_session(make_req("test")).await.unwrap();
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
        assert_eq!(resumed.status, SessionStatus::Running);
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
            status: SessionStatus::Running,
            mode: SessionMode::Autonomous,
            conversation_id: None,
            exit_code: None,
            backend_session_id: Some("pulpo-legacy".into()),
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
            created_at: now,
            updated_at: now,
        };
        mgr.store().insert_session(&session).await.unwrap();
        // Mark stale
        mgr.store()
            .update_session_status(&id.to_string(), SessionStatus::Stale)
            .await
            .unwrap();

        let resumed = mgr.resume_session(&id.to_string()).await.unwrap();
        assert_eq!(resumed.status, SessionStatus::Running);
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

    #[test]
    fn test_resolve_guard_config_default() {
        let req = make_req("test");
        let default = GuardConfig::default();
        let result = resolve_guard_config(&req, &default);
        assert_eq!(result.preset, pulpo_common::guard::GuardPreset::Standard);
    }

    #[test]
    fn test_resolve_guard_config_preset() {
        let req = CreateSessionRequest {
            name: None,
            workdir: "/tmp".into(),
            provider: None,
            prompt: "test".into(),
            mode: None,
            guard_preset: Some(pulpo_common::guard::GuardPreset::Strict),
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
        };
        let result = resolve_guard_config(&req, &GuardConfig::default());
        assert_eq!(result.preset, pulpo_common::guard::GuardPreset::Strict);
    }

    #[test]
    fn test_resolve_guard_config_config_wins() {
        let custom = GuardConfig {
            preset: pulpo_common::guard::GuardPreset::Unrestricted,
        };
        let req = CreateSessionRequest {
            name: None,
            workdir: "/tmp".into(),
            provider: None,
            prompt: "test".into(),
            mode: None,
            guard_preset: Some(pulpo_common::guard::GuardPreset::Strict),
            guard_config: Some(custom),
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
        };
        let result = resolve_guard_config(&req, &GuardConfig::default());
        // guard_config wins over guard_preset
        assert_eq!(
            result.preset,
            pulpo_common::guard::GuardPreset::Unrestricted
        );
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
        assert!(cmd.contains("claude --resume conv-123"));
        assert!(cmd.contains("--allowedTools"));
    }

    #[test]
    fn test_build_command_autonomous_claude_resume_unrestricted() {
        let guards = GuardConfig {
            preset: pulpo_common::guard::GuardPreset::Unrestricted,
        };
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
        assert!(cmd_interactive.starts_with("codex "));
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
        // Interactive Claude passes the prompt as a positional arg (shell-escaped)
        assert!(cmd.starts_with("claude "));
        assert!(cmd.contains("'test'"));
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
        let guards = GuardConfig {
            preset: pulpo_common::guard::GuardPreset::Unrestricted,
        };
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
        // Interactive Codex passes prompt as positional arg
        assert!(cmd_interactive.starts_with("codex "));
        assert!(cmd_interactive.contains("'test'"));

        let params_autonomous = crate::guard::SpawnParams {
            prompt: "test".into(),
            guards: GuardConfig::default(),
            ..crate::guard::SpawnParams::default()
        };
        let cmd_autonomous =
            build_command(Provider::Codex, SessionMode::Autonomous, &params_autonomous);
        // Autonomous Codex wraps in bash -c '...'
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

    #[tokio::test]
    async fn test_list_sessions_filtered() {
        let (mgr, _, _) = test_manager(MockBackend::new()).await;
        let _ = mgr.create_session(make_req("filter-test")).await.unwrap();

        let query = pulpo_common::api::ListSessionsQuery {
            status: Some("running".into()),
            ..Default::default()
        };
        let sessions = mgr.list_sessions_filtered(&query).await.unwrap();
        assert_eq!(sessions.len(), 1);
    }

    #[tokio::test]
    async fn test_list_sessions_filtered_no_match() {
        let (mgr, _, _) = test_manager(MockBackend::new()).await;
        let _ = mgr.create_session(make_req("filter-test")).await.unwrap();

        let query = pulpo_common::api::ListSessionsQuery {
            status: Some("completed".into()),
            ..Default::default()
        };
        let sessions = mgr.list_sessions_filtered(&query).await.unwrap();
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn test_list_sessions_filtered_detects_stale() {
        let (mgr, _, _) = test_manager(MockBackend::new().with_alive(false)).await;
        let _ = mgr.create_session(make_req("stale-filter")).await.unwrap();

        let query = pulpo_common::api::ListSessionsQuery::default();
        let sessions = mgr.list_sessions_filtered(&query).await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].status, SessionStatus::Stale);
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
    fn test_resolve_persona_no_persona() {
        let personas = HashMap::new();
        let mgr = SessionManager {
            backend: Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            store: unsafe_empty_store(),
            default_guard: GuardConfig::default(),
            default_max_turns: None,
            default_max_budget_usd: None,
            default_output_format: None,
            personas,
            event_tx: None,
            node_name: String::new(),
        };
        let req = CreateSessionRequest {
            name: None,
            workdir: "/tmp".into(),
            provider: None,
            prompt: "test".into(),
            mode: None,
            guard_preset: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
        };
        let resolved = mgr.resolve_persona(req).unwrap();
        assert!(resolved.model.is_none());
        assert!(resolved.system_prompt.is_none());
    }

    #[test]
    fn test_resolve_persona_unknown() {
        let personas = HashMap::new();
        let mgr = SessionManager {
            backend: Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            store: unsafe_empty_store(),
            default_guard: GuardConfig::default(),
            default_max_turns: None,
            default_max_budget_usd: None,
            default_output_format: None,
            personas,
            event_tx: None,
            node_name: String::new(),
        };
        let req = CreateSessionRequest {
            name: None,
            workdir: "/tmp".into(),
            provider: None,
            prompt: "test".into(),
            mode: None,
            guard_preset: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: Some("nonexistent".into()),
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
        };
        let result = mgr.resolve_persona(req);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown persona"));
    }

    #[test]
    fn test_resolve_persona_applies_defaults() {
        let mut personas = HashMap::new();
        personas.insert(
            "reviewer".into(),
            crate::config::PersonaConfig {
                provider: Some("claude".into()),
                model: Some("sonnet".into()),
                mode: Some("autonomous".into()),
                guard_preset: Some("strict".into()),
                allowed_tools: Some(vec!["Read".into(), "Glob".into()]),
                system_prompt: Some("Review code".into()),
                max_turns: None,
                max_budget_usd: None,
                output_format: None,
            },
        );
        let mgr = SessionManager {
            backend: Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            store: unsafe_empty_store(),
            default_guard: GuardConfig::default(),
            default_max_turns: None,
            default_max_budget_usd: None,
            default_output_format: None,
            personas,
            event_tx: None,
            node_name: String::new(),
        };
        let req = CreateSessionRequest {
            name: None,
            workdir: "/tmp".into(),
            provider: None,
            prompt: "test".into(),
            mode: None,
            guard_preset: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: Some("reviewer".into()),
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
        };
        let resolved = mgr.resolve_persona(req).unwrap();
        assert_eq!(resolved.provider, Some(Provider::Claude));
        assert_eq!(resolved.model, Some("sonnet".into()));
        assert_eq!(resolved.mode, Some(SessionMode::Autonomous));
        assert_eq!(resolved.guard_preset, Some(GuardPreset::Strict));
        assert_eq!(
            resolved.allowed_tools,
            Some(vec!["Read".into(), "Glob".into()])
        );
        assert_eq!(resolved.system_prompt, Some("Review code".into()));
    }

    #[test]
    fn test_resolve_persona_request_overrides() {
        let mut personas = HashMap::new();
        personas.insert(
            "reviewer".into(),
            crate::config::PersonaConfig {
                provider: Some("claude".into()),
                model: Some("sonnet".into()),
                mode: Some("autonomous".into()),
                guard_preset: Some("strict".into()),
                allowed_tools: Some(vec!["Read".into()]),
                system_prompt: Some("Review code".into()),
                max_turns: None,
                max_budget_usd: None,
                output_format: None,
            },
        );
        let mgr = SessionManager {
            backend: Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            store: unsafe_empty_store(),
            default_guard: GuardConfig::default(),
            default_max_turns: None,
            default_max_budget_usd: None,
            default_output_format: None,
            personas,
            event_tx: None,
            node_name: String::new(),
        };
        let req = CreateSessionRequest {
            name: None,
            workdir: "/tmp".into(),
            provider: Some(Provider::Codex),
            prompt: "test".into(),
            mode: Some(SessionMode::Interactive),
            guard_preset: Some(GuardPreset::Unrestricted),
            guard_config: None,
            model: Some("opus".into()),
            allowed_tools: Some(vec!["Bash".into()]),
            system_prompt: Some("Explicit prompt".into()),
            metadata: None,
            persona: Some("reviewer".into()),
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
        };
        let resolved = mgr.resolve_persona(req).unwrap();
        // Explicit request values win
        assert_eq!(resolved.provider, Some(Provider::Codex));
        assert_eq!(resolved.model, Some("opus".into()));
        assert_eq!(resolved.mode, Some(SessionMode::Interactive));
        assert_eq!(resolved.guard_preset, Some(GuardPreset::Unrestricted));
        assert_eq!(resolved.allowed_tools, Some(vec!["Bash".into()]));
        assert_eq!(resolved.system_prompt, Some("Explicit prompt".into()));
    }

    #[test]
    fn test_resolve_persona_guard_config_blocks_preset() {
        let mut personas = HashMap::new();
        personas.insert(
            "coder".into(),
            crate::config::PersonaConfig {
                provider: None,
                model: None,
                mode: None,
                guard_preset: Some("strict".into()),
                allowed_tools: None,
                system_prompt: None,
                max_turns: None,
                max_budget_usd: None,
                output_format: None,
            },
        );
        let mgr = SessionManager {
            backend: Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            store: unsafe_empty_store(),
            default_guard: GuardConfig::default(),
            default_max_turns: None,
            default_max_budget_usd: None,
            default_output_format: None,
            personas,
            event_tx: None,
            node_name: String::new(),
        };
        let req = CreateSessionRequest {
            name: None,
            workdir: "/tmp".into(),
            provider: None,
            prompt: "test".into(),
            mode: None,
            guard_preset: None,
            guard_config: Some(GuardConfig::default()),
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: Some("coder".into()),
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
        };
        let resolved = mgr.resolve_persona(req).unwrap();
        // guard_config is set, so persona's guard_preset should NOT be applied
        assert!(resolved.guard_preset.is_none());
        assert!(resolved.guard_config.is_some());
    }

    #[test]
    fn test_resolve_persona_applies_guardrail_defaults() {
        let mut personas = HashMap::new();
        personas.insert(
            "safe-agent".into(),
            crate::config::PersonaConfig {
                provider: None,
                model: None,
                mode: None,
                guard_preset: None,
                allowed_tools: None,
                system_prompt: None,
                max_turns: Some(10),
                max_budget_usd: Some(5.0),
                output_format: Some("json".into()),
            },
        );
        let mgr = SessionManager {
            backend: Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            store: unsafe_empty_store(),
            default_guard: GuardConfig::default(),
            default_max_turns: None,
            default_max_budget_usd: None,
            default_output_format: None,
            personas,
            event_tx: None,
            node_name: String::new(),
        };
        let req = CreateSessionRequest {
            name: None,
            workdir: "/tmp".into(),
            provider: None,
            prompt: "test".into(),
            mode: None,
            guard_preset: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: Some("safe-agent".into()),
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
        };
        let resolved = mgr.resolve_persona(req).unwrap();
        assert_eq!(resolved.max_turns, Some(10));
        assert_eq!(resolved.max_budget_usd, Some(5.0));
        assert_eq!(resolved.output_format, Some("json".into()));
    }

    #[test]
    fn test_resolve_persona_explicit_guardrails_win() {
        let mut personas = HashMap::new();
        personas.insert(
            "safe-agent".into(),
            crate::config::PersonaConfig {
                provider: None,
                model: None,
                mode: None,
                guard_preset: None,
                allowed_tools: None,
                system_prompt: None,
                max_turns: Some(10),
                max_budget_usd: Some(5.0),
                output_format: Some("json".into()),
            },
        );
        let mgr = SessionManager {
            backend: Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            store: unsafe_empty_store(),
            default_guard: GuardConfig::default(),
            default_max_turns: None,
            default_max_budget_usd: None,
            default_output_format: None,
            personas,
            event_tx: None,
            node_name: String::new(),
        };
        let req = CreateSessionRequest {
            name: None,
            workdir: "/tmp".into(),
            provider: None,
            prompt: "test".into(),
            mode: None,
            guard_preset: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: Some("safe-agent".into()),
            max_turns: Some(3),
            max_budget_usd: Some(1.0),
            output_format: Some("stream-json".into()),
        };
        let resolved = mgr.resolve_persona(req).unwrap();
        // Explicit request values win over persona defaults
        assert_eq!(resolved.max_turns, Some(3));
        assert_eq!(resolved.max_budget_usd, Some(1.0));
        assert_eq!(resolved.output_format, Some("stream-json".into()));
    }

    #[tokio::test]
    async fn test_global_guardrail_defaults_applied() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let mgr = mgr.with_guardrail_defaults(Some(50), Some(10.0), Some("stream-json".into()));

        let session = mgr.create_session(make_req("test")).await.unwrap();
        assert_eq!(session.max_turns, Some(50));
        assert_eq!(session.max_budget_usd, Some(10.0));
        assert_eq!(session.output_format, Some("stream-json".into()));
    }

    #[tokio::test]
    async fn test_global_guardrail_defaults_overridden_by_request() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let mgr = mgr.with_guardrail_defaults(Some(50), Some(10.0), Some("stream-json".into()));

        let req = CreateSessionRequest {
            max_turns: Some(5),
            max_budget_usd: Some(1.0),
            output_format: Some("json".into()),
            ..make_req("test")
        };
        let session = mgr.create_session(req).await.unwrap();
        assert_eq!(session.max_turns, Some(5));
        assert_eq!(session.max_budget_usd, Some(1.0));
        assert_eq!(session.output_format, Some("json".into()));
    }

    #[tokio::test]
    async fn test_global_guardrail_defaults_overridden_by_persona() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let mut personas = HashMap::new();
        personas.insert(
            "strict-agent".into(),
            crate::config::PersonaConfig {
                provider: None,
                model: None,
                mode: None,
                guard_preset: None,
                allowed_tools: None,
                system_prompt: None,
                max_turns: Some(10),
                max_budget_usd: Some(2.0),
                output_format: Some("json".into()),
            },
        );
        let mgr = SessionManager::new(
            Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            store,
            GuardConfig::default(),
            personas,
        )
        .with_guardrail_defaults(Some(50), Some(10.0), Some("stream-json".into()));

        let req = CreateSessionRequest {
            persona: Some("strict-agent".into()),
            ..make_req("test")
        };
        let session = mgr.create_session(req).await.unwrap();
        // Persona values win over global defaults
        assert_eq!(session.max_turns, Some(10));
        assert_eq!(session.max_budget_usd, Some(2.0));
        assert_eq!(session.output_format, Some("json".into()));
    }

    #[tokio::test]
    async fn test_global_guardrail_defaults_none_when_unset() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        // No guardrail defaults set
        let session = mgr.create_session(make_req("test")).await.unwrap();
        assert!(session.max_turns.is_none());
        assert!(session.max_budget_usd.is_none());
        assert!(session.output_format.is_none());
    }

    #[test]
    fn test_with_guardrail_defaults_builder() {
        let mgr = SessionManager {
            backend: Arc::new(MockBackend::new()) as Arc<dyn Backend>,
            store: unsafe_empty_store(),
            default_guard: GuardConfig::default(),
            default_max_turns: None,
            default_max_budget_usd: None,
            default_output_format: None,
            personas: HashMap::new(),
            event_tx: None,
            node_name: String::new(),
        };

        let mgr = mgr.with_guardrail_defaults(Some(100), Some(25.0), Some("json".into()));
        assert_eq!(mgr.default_max_turns, Some(100));
        assert_eq!(mgr.default_max_budget_usd, Some(25.0));
        assert_eq!(mgr.default_output_format, Some("json".into()));
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
            status: SessionStatus::Running,
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
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        mgr.emit_event(&session, Some(SessionStatus::Creating));
        let event = unwrap_session_event(rx.recv().await.unwrap());
        assert_eq!(event.session_name, "test-session");
        assert_eq!(event.status, "running");
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
            status: SessionStatus::Running,
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
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
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

        mgr.create_session(make_req("do it")).await.unwrap();
        let event = unwrap_session_event(rx.recv().await.unwrap());
        assert_eq!(event.status, "running");
        assert_eq!(event.previous_status, Some("creating".into()));
    }

    #[tokio::test]
    async fn test_kill_session_emits_event() {
        let (mgr, _, _) = test_manager(MockBackend::new()).await;
        let (tx, mut rx) = broadcast::channel(16);
        let mgr = mgr.with_event_tx(tx, "n".into());

        let session = mgr.create_session(make_req("work")).await.unwrap();
        // Drain the create event
        let _ = rx.recv().await.unwrap();

        mgr.kill_session(&session.id.to_string()).await.unwrap();
        let event = unwrap_session_event(rx.recv().await.unwrap());
        assert_eq!(event.status, "dead");
        assert_eq!(event.previous_status, Some("running".into()));
    }

    #[tokio::test]
    async fn test_get_session_stale_emits_event() {
        let backend = MockBackend::new().with_alive(false);
        let (mgr, _, _) = test_manager(backend).await;
        let (tx, mut rx) = broadcast::channel(16);
        let mgr = mgr.with_event_tx(tx, "n".into());

        // Insert a session that appears "running" in DB
        let req = CreateSessionRequest {
            mode: Some(SessionMode::Interactive),
            ..make_req("test")
        };
        // create_session will fail because is_alive returns false, but we need it in DB.
        // Actually create_session calls backend.create_session (not is_alive), so it succeeds.
        let session = mgr.create_session(req).await.unwrap();
        // Drain create event
        let _ = rx.recv().await.unwrap();

        // Now get_session checks is_alive → false → marks stale → emits event
        let fetched = mgr
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, SessionStatus::Stale);

        let event = unwrap_session_event(rx.recv().await.unwrap());
        assert_eq!(event.status, "stale");
        assert_eq!(event.previous_status, Some("running".into()));
    }

    #[tokio::test]
    async fn test_resume_session_emits_event() {
        let (mgr, _, _) = test_manager(MockBackend::new().with_alive(false)).await;
        let (tx, mut rx) = broadcast::channel(16);
        let mgr = mgr.with_event_tx(tx, "n".into());

        let session = mgr.create_session(make_req("work")).await.unwrap();
        let _ = rx.recv().await.unwrap(); // drain create event

        // Mark stale via get_session
        let _ = mgr.get_session(&session.id.to_string()).await.unwrap();
        let _ = rx.recv().await.unwrap(); // drain stale event

        // Now resume — but we need the backend to succeed on create_session
        // The MockBackend already has create_result = Ok, and is_alive doesn't matter for resume
        let resumed = mgr.resume_session(&session.id.to_string()).await.unwrap();
        assert_eq!(resumed.status, SessionStatus::Running);

        let event = unwrap_session_event(rx.recv().await.unwrap());
        assert_eq!(event.status, "running");
        assert_eq!(event.previous_status, Some("stale".into()));
    }

    #[tokio::test]
    async fn test_emit_event_no_subscribers() {
        let (mgr, _, _) = test_manager(MockBackend::new()).await;
        let (tx, rx) = broadcast::channel::<PulpoEvent>(16);
        let mgr = mgr.with_event_tx(tx, "n".into());
        // Drop the only receiver
        drop(rx);

        // emit_event should not panic even with no subscribers
        let session = mgr.create_session(make_req("test")).await.unwrap();
        // Just verify the session was created successfully (emit silently failed)
        assert_eq!(session.status, SessionStatus::Running);
    }

    #[tokio::test]
    async fn test_delete_dead_session() {
        let (mgr, _, _pool) = test_manager(MockBackend::new()).await;
        let session = mgr.create_session(make_req("test")).await.unwrap();
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
        let session = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();

        // Manually set status to Completed
        sqlx::query("UPDATE sessions SET status = 'completed' WHERE id = ?")
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
        let session = mgr.create_session(make_req("test")).await.unwrap();
        let id = session.id.to_string();

        // Manually set status to Stale
        sqlx::query("UPDATE sessions SET status = 'stale' WHERE id = ?")
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
        let session = mgr.create_session(make_req("test")).await.unwrap();
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
}
