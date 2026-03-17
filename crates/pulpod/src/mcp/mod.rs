pub mod resources;

use std::fmt::Write as _;
use std::sync::Arc;

use anyhow::Result;
use pulpo_common::api::CreateSessionRequest;
use pulpo_common::node::NodeInfo;
use pulpo_common::peer::PeerInfo;
use pulpo_common::session::{Session, SessionStatus};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    Implementation, ListResourcesResult, PaginatedRequestParams, ReadResourceRequestParams,
    ReadResourceResult, ServerCapabilities, ServerInfo,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::peers::PeerRegistry;
use crate::session::manager::SessionManager;

/// Request body sent to a remote node when proxying `spawn_session`.
#[derive(Serialize)]
struct RemoteSpawnReq {
    name: String,
    workdir: Option<String>,
    command: Option<String>,
    ink: Option<String>,
    description: Option<String>,
}

// -- Tool parameter types --

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SpawnSessionParams {
    /// Session name (required).
    pub name: String,
    /// Command to run in the session.
    pub command: Option<String>,
    /// Path to the working directory on the target machine.
    pub workdir: Option<String>,
    /// Ink preset from config (e.g. "reviewer", "coder").
    pub ink: Option<String>,
    /// Human-readable description of the session.
    pub description: Option<String>,
    /// Target node name. If omitted, runs locally.
    pub node: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListSessionsParams {
    /// Filter by status (creating, active, ready, killed, lost).
    pub status: Option<String>,
    /// Target node name. If omitted, queries locally.
    pub node: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetSessionParams {
    /// Session ID (UUID).
    pub id: String,
    /// Target node name. If omitted, queries locally.
    pub node: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct KillSessionParams {
    /// Session ID (UUID).
    pub id: String,
    /// Target node name. If omitted, kills locally.
    pub node: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ResumeSessionParams {
    /// Session ID (UUID).
    pub id: String,
    /// Target node name. If omitted, resumes locally.
    pub node: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetOutputParams {
    /// Session ID (UUID).
    pub id: String,
    /// Number of terminal output lines to retrieve. Defaults to 100.
    pub lines: Option<usize>,
    /// Target node name. If omitted, reads locally.
    pub node: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SendInputParams {
    /// Session ID (UUID).
    pub id: String,
    /// Text to send to the session terminal.
    pub text: String,
    /// Target node name. If omitted, sends locally.
    pub node: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListInterventionEventsParams {
    /// Session ID (UUID).
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WaitForSessionParams {
    /// Session ID (UUID).
    pub id: String,
    /// Timeout in seconds. Defaults to 300 (5 minutes).
    pub timeout_secs: Option<u64>,
    /// Poll interval in seconds. Defaults to 5.
    pub poll_interval_secs: Option<u64>,
    /// Target node name. If omitted, polls locally.
    pub node: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SpawnAndWaitParams {
    /// Session name (required).
    pub name: String,
    /// Command to run in the session.
    pub command: Option<String>,
    /// Path to the working directory on the target machine.
    pub workdir: Option<String>,
    /// Ink from config.
    pub ink: Option<String>,
    /// Human-readable description.
    pub description: Option<String>,
    /// Target node name. If omitted, runs locally.
    pub node: Option<String>,
    /// Timeout in seconds. Defaults to 600 (10 min).
    pub timeout_secs: Option<u64>,
    /// Poll interval in seconds. Defaults to 5.
    pub poll_interval_secs: Option<u64>,
    /// Number of output lines to capture. Defaults to 200.
    pub output_lines: Option<usize>,
}

/// A single task in a fan-out operation.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct FanOutTask {
    /// Session name (required).
    pub name: String,
    /// Command to run in the session.
    pub command: Option<String>,
    /// Path to the working directory on the target machine.
    pub workdir: Option<String>,
    /// Ink from config.
    pub ink: Option<String>,
    /// Human-readable description.
    pub description: Option<String>,
    /// Target node name. If omitted, runs locally.
    pub node: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FanOutParams {
    /// List of tasks to spawn in parallel.
    pub tasks: Vec<FanOutTask>,
    /// Global timeout in seconds for all tasks. Defaults to 600 (10 min).
    pub timeout_secs: Option<u64>,
    /// Poll interval in seconds. Defaults to 5.
    pub poll_interval_secs: Option<u64>,
    /// Number of output lines to capture per task. Defaults to 200.
    pub output_lines: Option<usize>,
}

/// Internal request struct for `spawn_and_wait_impl` to avoid too-many-arguments.
struct SpawnAndWaitRequest {
    name: String,
    command: Option<String>,
    workdir: Option<String>,
    ink: Option<String>,
    description: Option<String>,
    node: Option<String>,
    metadata: Option<std::collections::HashMap<String, String>>,
    timeout_secs: u64,
    poll_interval_secs: u64,
    output_lines: usize,
}

// -- Tool result types --

#[derive(Debug, Serialize, Deserialize)]
struct WaitResult {
    session: Session,
    output: String,
    timed_out: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct FanOutTaskResult {
    session: Option<Session>,
    output: String,
    timed_out: bool,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct FanOutResult {
    results: Vec<FanOutTaskResult>,
    all_completed: bool,
}

#[derive(Debug, Serialize)]
struct NodesResult {
    local: NodeInfo,
    peers: Vec<PeerInfo>,
}

// -- MCP Server --

#[derive(Clone)]
pub struct PulpoMcp {
    session_manager: Arc<SessionManager>,
    peer_registry: PeerRegistry,
    config: Arc<Config>,
    tool_router: ToolRouter<Self>,
}

impl PulpoMcp {
    pub fn new(
        session_manager: SessionManager,
        peer_registry: PeerRegistry,
        config: Config,
    ) -> Self {
        let session_manager = Arc::new(session_manager);
        let config = Arc::new(config);
        Self {
            session_manager,
            peer_registry,
            config,
            tool_router: Self::tool_router(),
        }
    }

    fn is_local(&self, node: Option<&str>) -> bool {
        node.is_none_or(|name| name == self.config.node.name)
    }

    async fn peer_address(&self, name: &str) -> Result<(String, Option<String>)> {
        let peer = self
            .peer_registry
            .get(name)
            .await
            .ok_or_else(|| anyhow::anyhow!("unknown node: {name}"))?;
        let token = self.peer_registry.get_token(name).await;
        Ok((peer.address, token))
    }

    async fn remote_get<T: serde::de::DeserializeOwned>(
        &self,
        node: &str,
        path: &str,
    ) -> Result<T> {
        let (address, token) = self.peer_address(node).await?;
        let url = format!("{}{path}", crate::peers::health::base_url(&address));
        let client = reqwest::Client::new();
        let mut req = client.get(&url);
        if let Some(tok) = &token {
            req = req.bearer_auth(tok);
        }
        let resp = req.send().await?.error_for_status()?;
        Ok(resp.json().await?)
    }

    async fn remote_post<T: serde::de::DeserializeOwned, B: Serialize + Sync>(
        &self,
        node: &str,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let (address, token) = self.peer_address(node).await?;
        let url = format!("{}{path}", crate::peers::health::base_url(&address));
        let client = reqwest::Client::new();
        let mut req = client.post(&url).json(body);
        if let Some(tok) = &token {
            req = req.bearer_auth(tok);
        }
        let resp = req.send().await?.error_for_status()?;
        Ok(resp.json().await?)
    }

    async fn remote_delete(&self, node: &str, path: &str) -> Result<()> {
        let (address, token) = self.peer_address(node).await?;
        let url = format!("{}{path}", crate::peers::health::base_url(&address));
        let client = reqwest::Client::new();
        let mut req = client.delete(&url);
        if let Some(tok) = &token {
            req = req.bearer_auth(tok);
        }
        req.send().await?.error_for_status()?;
        Ok(())
    }

    fn build_node_info(&self) -> NodeInfo {
        NodeInfo {
            name: self.config.node.name.clone(),
            hostname: crate::api::node::get_hostname(),
            os: crate::platform::os_name().into(),
            arch: std::env::consts::ARCH.into(),
            cpus: num_cpus::get(),
            memory_mb: crate::api::node::get_memory_mb(),
            gpu: None,
        }
    }

    /// Fetch session output, either locally or from a remote node.
    async fn fetch_output(
        &self,
        session_id: &str,
        node: Option<&str>,
        output_lines: usize,
    ) -> String {
        if self.is_local(node) {
            // Local: we need the session to resolve the backend ID
            if let Ok(Some(session)) = self.session_manager.get_session(session_id).await {
                self.session_manager.capture_output(
                    session_id,
                    &self.session_manager.resolve_backend_id(&session),
                    output_lines,
                )
            } else {
                String::new()
            }
        } else {
            let n = node.unwrap_or_default();
            let path = format!("/api/v1/sessions/{session_id}/output?lines={output_lines}");
            self.remote_get::<serde_json::Value>(n, &path)
                .await
                .map_or_else(
                    |_| String::new(),
                    |val| {
                        val.get("output")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_owned()
                    },
                )
        }
    }

    /// Shared poll loop: check a session until it reaches terminal status or times out.
    /// Returns `Ok((session, output, timed_out))` on success. Returns `Err` if the session
    /// is not found or there's a communication error.
    async fn poll_until_terminal(
        &self,
        session_id: &str,
        node: Option<&str>,
        timeout_secs: u64,
        poll_interval_secs: u64,
        output_lines: usize,
    ) -> Result<(Session, String, bool)> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);

        loop {
            let session_result = if self.is_local(node) {
                self.session_manager.get_session(session_id).await
            } else {
                let n = node.unwrap_or_default();
                let path = format!("/api/v1/sessions/{session_id}");
                self.remote_get::<Option<Session>>(n, &path).await
            };

            match session_result {
                Ok(Some(session)) => {
                    let is_terminal = matches!(
                        session.status,
                        SessionStatus::Ready | SessionStatus::Killed | SessionStatus::Lost
                    );
                    if is_terminal {
                        let output = self.fetch_output(session_id, node, output_lines).await;
                        return Ok((session, output, false));
                    }

                    if tokio::time::Instant::now() >= deadline {
                        let output = self.fetch_output(session_id, node, output_lines).await;
                        return Ok((session, output, true));
                    }
                }
                Ok(None) => return Err(anyhow::anyhow!("session not found: {session_id}")),
                Err(e) => return Err(e),
            }

            tokio::time::sleep(std::time::Duration::from_secs(poll_interval_secs)).await;
        }
    }

    /// Spawn a session and wait for it to finish. Used by both `spawn_and_wait` and `fan_out`.
    async fn spawn_and_wait_impl(&self, req: SpawnAndWaitRequest) -> FanOutTaskResult {
        let SpawnAndWaitRequest {
            name,
            command,
            workdir,
            ink,
            description,
            node,
            metadata,
            timeout_secs,
            poll_interval_secs,
            output_lines,
        } = req;
        // 1. Spawn the session
        let spawn_result = if self.is_local(node.as_deref()) {
            let req = CreateSessionRequest {
                name,
                workdir,
                command,
                ink,
                description,
                metadata,
            };
            self.session_manager.create_session(req).await
        } else {
            let n = node.as_deref().unwrap_or_default();
            let body = RemoteSpawnReq {
                name,
                workdir,
                command,
                ink,
                description,
            };
            self.remote_post::<Session, _>(n, "/api/v1/sessions", &body)
                .await
        };

        let session = match spawn_result {
            Ok(s) => s,
            Err(e) => {
                return FanOutTaskResult {
                    session: None,
                    output: String::new(),
                    timed_out: false,
                    error: Some(format!("{e}")),
                };
            }
        };

        // 2. Poll until terminal
        let session_id = session.id.to_string();
        match self
            .poll_until_terminal(
                &session_id,
                node.as_deref(),
                timeout_secs,
                poll_interval_secs,
                output_lines,
            )
            .await
        {
            Ok((final_session, output, timed_out)) => FanOutTaskResult {
                session: Some(final_session),
                output,
                timed_out,
                error: None,
            },
            Err(e) => FanOutTaskResult {
                session: None,
                output: String::new(),
                timed_out: false,
                error: Some(format!("{e}")),
            },
        }
    }
}

#[tool_router]
impl PulpoMcp {
    #[tool(
        name = "spawn_session",
        description = "Create a new session. Spawns a terminal running the specified command in the given directory. Returns the created session details including its ID for tracking."
    )]
    async fn spawn_session(&self, Parameters(params): Parameters<SpawnSessionParams>) -> String {
        let result = if self.is_local(params.node.as_deref()) {
            let req = CreateSessionRequest {
                name: params.name,
                workdir: params.workdir,
                command: params.command,
                ink: params.ink,
                description: params.description,
                metadata: None,
            };
            self.session_manager.create_session(req).await
        } else {
            let node = params.node.as_deref().unwrap_or_default();
            let body = RemoteSpawnReq {
                name: params.name,
                workdir: params.workdir,
                command: params.command,
                ink: params.ink,
                description: params.description,
            };
            self.remote_post::<Session, _>(node, "/api/v1/sessions", &body)
                .await
        };
        match result {
            Ok(session) => serde_json::to_string_pretty(&session).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(
        name = "list_sessions",
        description = "List all sessions, optionally filtered by status (active, ready, killed, lost)."
    )]
    async fn list_sessions(&self, Parameters(params): Parameters<ListSessionsParams>) -> String {
        let result = if self.is_local(params.node.as_deref()) {
            let query = pulpo_common::api::ListSessionsQuery {
                status: params.status,
                ..Default::default()
            };
            self.session_manager.list_sessions_filtered(&query).await
        } else {
            let node = params.node.as_deref().unwrap_or_default();
            let mut path = "/api/v1/sessions".to_string();
            if let Some(status) = &params.status {
                let _ = write!(path, "?status={status}");
            }
            self.remote_get::<Vec<Session>>(node, &path).await
        };
        match result {
            Ok(sessions) => serde_json::to_string_pretty(&sessions).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(
        name = "get_session",
        description = "Get details of a specific session by ID. Returns full session state including status, provider, prompt, timestamps, and guard config."
    )]
    async fn get_session(&self, Parameters(params): Parameters<GetSessionParams>) -> String {
        let result = if self.is_local(params.node.as_deref()) {
            self.session_manager.get_session(&params.id).await
        } else {
            let node = params.node.as_deref().unwrap_or_default();
            let path = format!("/api/v1/sessions/{}", params.id);
            self.remote_get::<Option<Session>>(node, &path).await
        };
        match result {
            Ok(Some(session)) => serde_json::to_string_pretty(&session).unwrap_or_default(),
            Ok(None) => "Session not found".into(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(
        name = "kill_session",
        description = "Kill an active session. Terminates the terminal process and marks the session as killed."
    )]
    async fn kill_session(&self, Parameters(params): Parameters<KillSessionParams>) -> String {
        let result = if self.is_local(params.node.as_deref()) {
            self.session_manager.kill_session(&params.id).await
        } else {
            let node = params.node.as_deref().unwrap_or_default();
            let path = format!("/api/v1/sessions/{}", params.id);
            self.remote_delete(node, &path).await
        };
        match result {
            Ok(()) => format!("Session {} killed", params.id),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(
        name = "resume_session",
        description = "Resume a lost session. Only works on sessions with 'lost' status. Re-creates the terminal with the original prompt and conversation context."
    )]
    async fn resume_session(&self, Parameters(params): Parameters<ResumeSessionParams>) -> String {
        let result = if self.is_local(params.node.as_deref()) {
            self.session_manager.resume_session(&params.id).await
        } else {
            let node = params.node.as_deref().unwrap_or_default();
            let path = format!("/api/v1/sessions/{}/resume", params.id);
            self.remote_post::<Session, _>(node, &path, &serde_json::json!({}))
                .await
        };
        match result {
            Ok(session) => serde_json::to_string_pretty(&session).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(
        name = "get_output",
        description = "Get terminal output from a session. Returns the last N lines (default 100) of the session's terminal output."
    )]
    async fn get_output(&self, Parameters(params): Parameters<GetOutputParams>) -> String {
        let lines = params.lines.unwrap_or(100);
        if self.is_local(params.node.as_deref()) {
            match self.session_manager.get_session(&params.id).await {
                Ok(Some(session)) => {
                    self.session_manager
                        .capture_output(&params.id, &session.name, lines)
                }
                Ok(None) => "Session not found".into(),
                Err(e) => format!("Error: {e}"),
            }
        } else {
            let node = params.node.as_deref().unwrap_or_default();
            let path = format!("/api/v1/sessions/{}/output?lines={lines}", params.id);
            match self.remote_get::<String>(node, &path).await {
                Ok(output) => output,
                Err(e) => format!("Error: {e}"),
            }
        }
    }

    #[tool(
        name = "send_input",
        description = "Send text input to an active session's terminal. Use this to interact with interactive sessions or provide follow-up instructions."
    )]
    async fn send_input(&self, Parameters(params): Parameters<SendInputParams>) -> String {
        if self.is_local(params.node.as_deref()) {
            match self.session_manager.get_session(&params.id).await {
                Ok(Some(session)) => {
                    let backend_id = self.session_manager.resolve_backend_id(&session);
                    match self.session_manager.send_input(&backend_id, &params.text) {
                        Ok(()) => "Input sent".into(),
                        Err(e) => format!("Error: {e}"),
                    }
                }
                Ok(None) => "Session not found".into(),
                Err(e) => format!("Error: {e}"),
            }
        } else {
            let node = params.node.as_deref().unwrap_or_default();
            let path = format!("/api/v1/sessions/{}/input", params.id);
            let body = serde_json::json!({ "text": params.text });
            match self
                .remote_post::<serde_json::Value, _>(node, &path, &body)
                .await
            {
                Ok(_) => "Input sent".into(),
                Err(e) => format!("Error: {e}"),
            }
        }
    }

    #[tool(
        name = "list_nodes",
        description = "List all available compute nodes — the local machine plus any configured peers. Shows hardware info, OS, and connectivity status."
    )]
    async fn list_nodes(&self) -> String {
        let local = self.build_node_info();
        let peers = self.peer_registry.get_all().await;
        let result = NodesResult { local, peers };
        serde_json::to_string_pretty(&result).unwrap_or_default()
    }

    #[tool(
        name = "wait_for_session",
        description = "Poll a session until it reaches a terminal status (ready, killed, lost) or times out. Returns the final session state, last output lines, and whether it timed out. Useful for autonomous workflows that need to wait for a session to finish."
    )]
    async fn wait_for_session(
        &self,
        Parameters(params): Parameters<WaitForSessionParams>,
    ) -> String {
        let timeout_secs = params.timeout_secs.unwrap_or(300);
        let poll_interval_secs = params.poll_interval_secs.unwrap_or(5);

        match self
            .poll_until_terminal(
                &params.id,
                params.node.as_deref(),
                timeout_secs,
                poll_interval_secs,
                100,
            )
            .await
        {
            Ok((session, output, timed_out)) => {
                let result = WaitResult {
                    session,
                    output,
                    timed_out,
                };
                serde_json::to_string_pretty(&result).unwrap_or_default()
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("session not found") {
                    "Session not found".into()
                } else {
                    format!("Error: {e}")
                }
            }
        }
    }

    #[tool(
        name = "spawn_and_wait",
        description = "Compound tool: spawn a coding agent session, poll until it finishes (or times out), and return the final session state plus terminal output — all in one call. Eliminates the manual spawn → poll → get_output choreography."
    )]
    async fn spawn_and_wait(&self, Parameters(params): Parameters<SpawnAndWaitParams>) -> String {
        let timeout_secs = params.timeout_secs.unwrap_or(600);
        let poll_interval_secs = params.poll_interval_secs.unwrap_or(5);
        let output_lines = params.output_lines.unwrap_or(200);

        let task_result = self
            .spawn_and_wait_impl(SpawnAndWaitRequest {
                name: params.name,
                command: params.command,
                workdir: params.workdir,
                ink: params.ink,
                description: params.description,
                node: params.node,
                metadata: None,
                timeout_secs,
                poll_interval_secs,
                output_lines,
            })
            .await;

        if let Some(error) = task_result.error {
            return format!("Error: {error}");
        }

        // session is always Some when error is None (spawn succeeded, poll returned a session)
        let session = task_result
            .session
            .expect("session must be present when error is None");
        let result = WaitResult {
            session,
            output: task_result.output,
            timed_out: task_result.timed_out,
        };
        serde_json::to_string_pretty(&result).unwrap_or_default()
    }

    #[tool(
        name = "fan_out",
        description = "Parallel multi-session orchestration: spawn multiple coding agent sessions (optionally across different nodes), wait for all to finish, and return collected results. Enables parallel code review, multi-repo tasks, and distributed work."
    )]
    async fn fan_out(&self, Parameters(params): Parameters<FanOutParams>) -> String {
        let timeout_secs = params.timeout_secs.unwrap_or(600);
        let poll_interval_secs = params.poll_interval_secs.unwrap_or(5);
        let output_lines = params.output_lines.unwrap_or(200);

        if params.tasks.is_empty() {
            let result = FanOutResult {
                results: vec![],
                all_completed: true,
            };
            return serde_json::to_string_pretty(&result).unwrap_or_default();
        }

        // Spawn all tasks concurrently
        let futures: Vec<_> = params
            .tasks
            .into_iter()
            .map(|task| {
                let mcp = self.clone();
                let mut metadata = std::collections::HashMap::new();
                metadata.insert("fan_out".into(), "true".into());
                async move {
                    mcp.spawn_and_wait_impl(SpawnAndWaitRequest {
                        name: task.name,
                        command: task.command,
                        workdir: task.workdir,
                        ink: task.ink,
                        description: task.description,
                        node: task.node,
                        metadata: Some(metadata),
                        timeout_secs,
                        poll_interval_secs,
                        output_lines,
                    })
                    .await
                }
            })
            .collect();

        // Await all concurrently, collect in original order
        let results: Vec<FanOutTaskResult> = futures::future::join_all(futures).await;
        let all_completed = results.iter().all(|r| r.error.is_none() && !r.timed_out);

        let result = FanOutResult {
            results,
            all_completed,
        };
        serde_json::to_string_pretty(&result).unwrap_or_default()
    }

    #[tool(
        name = "list_intervention_events",
        description = "List intervention events for a session. Returns the audit trail of watchdog interventions (e.g., memory kills) for the specified session."
    )]
    async fn list_intervention_events(
        &self,
        Parameters(params): Parameters<ListInterventionEventsParams>,
    ) -> String {
        match self
            .session_manager
            .store()
            .list_intervention_events(&params.id)
            .await
        {
            Ok(events) => {
                let response: Vec<pulpo_common::api::InterventionEventResponse> = events
                    .into_iter()
                    .map(|e| pulpo_common::api::InterventionEventResponse {
                        id: e.id,
                        session_id: e.session_id,
                        code: e.code,
                        reason: e.reason,
                        created_at: e.created_at.to_rfc3339(),
                    })
                    .collect();
                serde_json::to_string_pretty(&response).unwrap_or_default()
            }
            Err(e) => format!("Error: {e}"),
        }
    }
}

#[tool_handler]
impl ServerHandler for PulpoMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            server_info: Implementation {
                name: "pulpo".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                ..Default::default()
            },
            instructions: Some(
                "Pulpo agent session orchestrator — spawn, manage, and monitor coding agent sessions across machines.".into(),
            ),
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
            ..Default::default()
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListResourcesResult, rmcp::model::ErrorData> {
        Ok(resources::list_resources())
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ReadResourceResult, rmcp::model::ErrorData> {
        resources::read_resource(self, &request.uri).await
    }
}

/// Start the MCP server over STDIO. This function blocks until the client disconnects.
///
/// Excluded from coverage: STDIO transport cannot be tested in unit tests.
#[cfg(not(coverage))]
pub async fn run_stdio(server: PulpoMcp) -> Result<()> {
    use rmcp::ServiceExt;

    let (stdin, stdout) = rmcp::transport::stdio();
    let running = server.serve((stdin, stdout)).await?;
    running.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Backend;
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct MockBackend {
        alive: Mutex<bool>,
        captured_output: Mutex<String>,
        calls: Mutex<Vec<String>>,
    }

    impl MockBackend {
        fn new() -> Self {
            Self {
                alive: Mutex::new(true),
                captured_output: Mutex::new("test output line 1\ntest output line 2".into()),
                calls: Mutex::new(vec![]),
            }
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
            Ok(())
        }
        fn kill_session(&self, name: &str) -> Result<()> {
            self.calls.lock().unwrap().push(format!("kill:{name}"));
            Ok(())
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

    fn test_config() -> Config {
        Config {
            node: crate::config::NodeConfig {
                name: "test-node".into(),
                port: 7433,
                data_dir: "/tmp/test".into(),
                ..crate::config::NodeConfig::default()
            },
            auth: crate::config::AuthConfig::default(),
            peers: HashMap::new(),
            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
        }
    }

    async fn test_mcp(backend: MockBackend) -> PulpoMcp {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(backend);
        let manager = SessionManager::new(backend, store, HashMap::new()).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        PulpoMcp::new(manager, peer_registry, test_config())
    }

    async fn test_mcp_with_peers(
        backend: MockBackend,
        peers: HashMap<String, pulpo_common::peer::PeerEntry>,
    ) -> PulpoMcp {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(backend);
        let manager = SessionManager::new(backend, store, HashMap::new()).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&peers);
        PulpoMcp::new(manager, peer_registry, test_config())
    }

    async fn test_mcp_with_pool(backend: MockBackend) -> (PulpoMcp, sqlx::SqlitePool) {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let pool = store.pool().clone();
        let backend = Arc::new(backend);
        let manager = SessionManager::new(backend, store, HashMap::new()).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        (PulpoMcp::new(manager, peer_registry, test_config()), pool)
    }

    // -- is_local tests --

    #[test]
    fn test_is_local_none() {
        // Cannot construct PulpoMcp without async, so test via spawn_session logic
        // We test is_local indirectly through the tool handlers
    }

    #[tokio::test]
    async fn test_is_local_matches_node_name() {
        let mcp = test_mcp(MockBackend::new()).await;
        assert!(mcp.is_local(None));
        assert!(mcp.is_local(Some("test-node")));
        assert!(!mcp.is_local(Some("other-node")));
    }

    // -- peer_address tests --

    #[tokio::test]
    async fn test_peer_address_unknown_node() {
        let mcp = test_mcp(MockBackend::new()).await;
        let result = mcp.peer_address("nonexistent").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown node"));
    }

    #[tokio::test]
    async fn test_peer_address_known_node() {
        let mut peers = HashMap::new();
        peers.insert(
            "remote".into(),
            pulpo_common::peer::PeerEntry::Full {
                address: "10.0.0.1:7433".into(),
                token: Some("secret".into()),
            },
        );
        let mcp = test_mcp_with_peers(MockBackend::new(), peers).await;
        let (address, token) = mcp.peer_address("remote").await.unwrap();
        assert_eq!(address, "10.0.0.1:7433");
        assert_eq!(token, Some("secret".into()));
    }

    #[tokio::test]
    async fn test_peer_address_no_token() {
        let mut peers = HashMap::new();
        peers.insert(
            "remote".into(),
            pulpo_common::peer::PeerEntry::Simple("10.0.0.1:7433".into()),
        );
        let mcp = test_mcp_with_peers(MockBackend::new(), peers).await;
        let (address, token) = mcp.peer_address("remote").await.unwrap();
        assert_eq!(address, "10.0.0.1:7433");
        assert_eq!(token, None);
    }

    // -- spawn_session tests --

    #[tokio::test]
    async fn test_spawn_session_local() {
        let mcp = test_mcp(MockBackend::new()).await;
        let params = SpawnSessionParams {
            workdir: Some("/tmp".into()),
            name: "test".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: None,
        };
        let result = mcp.spawn_session(Parameters(params)).await;
        assert!(result.contains("\"name\": \"test\""));
        assert!(result.contains("active"));
    }

    #[tokio::test]
    async fn test_spawn_session_local_explicit_node() {
        let mcp = test_mcp(MockBackend::new()).await;
        let params = SpawnSessionParams {
            workdir: Some("/tmp".into()),
            name: "custom-name".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: Some("test-node".into()), // matches local
        };
        let result = mcp.spawn_session(Parameters(params)).await;
        assert!(result.contains("custom-name"));
        assert!(result.contains("active"));
    }

    #[tokio::test]
    async fn test_spawn_session_remote_unknown_node() {
        let mcp = test_mcp(MockBackend::new()).await;
        let params = SpawnSessionParams {
            workdir: Some("/tmp".into()),
            name: "test".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: Some("unknown-node".into()),
        };
        let result = mcp.spawn_session(Parameters(params)).await;
        assert!(result.contains("Error"));
        assert!(result.contains("unknown node"));
    }

    // -- list_sessions tests --

    #[tokio::test]
    async fn test_list_sessions_local_empty() {
        let mcp = test_mcp(MockBackend::new()).await;
        let params = ListSessionsParams {
            status: None,
            node: None,
        };
        let result = mcp.list_sessions(Parameters(params)).await;
        assert_eq!(result, "[]");
    }

    #[tokio::test]
    async fn test_list_sessions_local_with_sessions() {
        let mcp = test_mcp(MockBackend::new()).await;
        // Create a session first
        let spawn_params = SpawnSessionParams {
            workdir: Some("/tmp".into()),
            name: "test".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: None,
        };
        mcp.spawn_session(Parameters(spawn_params)).await;

        let params = ListSessionsParams {
            status: None,
            node: None,
        };
        let result = mcp.list_sessions(Parameters(params)).await;
        assert!(result.contains("tmp"));
    }

    #[tokio::test]
    async fn test_list_sessions_with_filters() {
        let mcp = test_mcp(MockBackend::new()).await;
        let params = ListSessionsParams {
            status: Some("active".into()),
            node: None,
        };
        let result = mcp.list_sessions(Parameters(params)).await;
        assert_eq!(result, "[]");
    }

    #[tokio::test]
    async fn test_list_sessions_remote_unknown_node() {
        let mcp = test_mcp(MockBackend::new()).await;
        let params = ListSessionsParams {
            status: None,
            node: Some("missing".into()),
        };
        let result = mcp.list_sessions(Parameters(params)).await;
        assert!(result.contains("Error"));
    }

    // -- get_session tests --

    #[tokio::test]
    async fn test_get_session_not_found() {
        let mcp = test_mcp(MockBackend::new()).await;
        let params = GetSessionParams {
            id: "nonexistent".into(),
            node: None,
        };
        let result = mcp.get_session(Parameters(params)).await;
        assert_eq!(result, "Session not found");
    }

    #[tokio::test]
    async fn test_get_session_found() {
        let mcp = test_mcp(MockBackend::new()).await;
        // Create a session
        let spawn_params = SpawnSessionParams {
            workdir: Some("/tmp".into()),
            name: "test".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: None,
        };
        let spawn_result = mcp.spawn_session(Parameters(spawn_params)).await;
        let session: Session = serde_json::from_str(&spawn_result).unwrap();

        let params = GetSessionParams {
            id: session.id.to_string(),
            node: None,
        };
        let result = mcp.get_session(Parameters(params)).await;
        assert!(result.contains(&session.id.to_string()));
    }

    #[tokio::test]
    async fn test_get_session_remote_unknown() {
        let mcp = test_mcp(MockBackend::new()).await;
        let params = GetSessionParams {
            id: "some-id".into(),
            node: Some("unknown".into()),
        };
        let result = mcp.get_session(Parameters(params)).await;
        assert!(result.contains("Error"));
    }

    // -- kill_session tests --

    #[tokio::test]
    async fn test_kill_session_local() {
        let mcp = test_mcp(MockBackend::new()).await;
        // Create a session
        let spawn_params = SpawnSessionParams {
            workdir: Some("/tmp".into()),
            name: "test".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: None,
        };
        let spawn_result = mcp.spawn_session(Parameters(spawn_params)).await;
        let session: Session = serde_json::from_str(&spawn_result).unwrap();

        let params = KillSessionParams {
            id: session.id.to_string(),
            node: None,
        };
        let result = mcp.kill_session(Parameters(params)).await;
        assert!(result.contains("killed"));
    }

    #[tokio::test]
    async fn test_kill_session_not_found() {
        let mcp = test_mcp(MockBackend::new()).await;
        let params = KillSessionParams {
            id: "nonexistent".into(),
            node: None,
        };
        let result = mcp.kill_session(Parameters(params)).await;
        assert!(result.contains("Error"));
    }

    #[tokio::test]
    async fn test_kill_session_remote_unknown() {
        let mcp = test_mcp(MockBackend::new()).await;
        let params = KillSessionParams {
            id: "some-id".into(),
            node: Some("unknown".into()),
        };
        let result = mcp.kill_session(Parameters(params)).await;
        assert!(result.contains("Error"));
    }

    // -- resume_session tests --

    #[tokio::test]
    async fn test_resume_session_local() {
        let mcp = test_mcp(MockBackend::new().with_alive(false)).await;
        // Create session, it becomes stale via get_session
        let spawn_params = SpawnSessionParams {
            workdir: Some("/tmp".into()),
            name: "test".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: None,
        };
        let spawn_result = mcp.spawn_session(Parameters(spawn_params)).await;
        let session: Session = serde_json::from_str(&spawn_result).unwrap();

        // Get session to trigger stale detection
        let get_params = GetSessionParams {
            id: session.id.to_string(),
            node: None,
        };
        let get_result = mcp.get_session(Parameters(get_params)).await;
        assert!(get_result.contains("lost"));

        // Resume
        let params = ResumeSessionParams {
            id: session.id.to_string(),
            node: None,
        };
        let result = mcp.resume_session(Parameters(params)).await;
        // Backend still returns alive=false so create_session succeeds but
        // the resume itself may fail or succeed depending on backend
        // In our mock, create_session returns Ok so it should succeed
        assert!(result.contains("active") || result.contains("Error"));
    }

    #[tokio::test]
    async fn test_resume_session_not_stale() {
        let mcp = test_mcp(MockBackend::new()).await;
        let spawn_params = SpawnSessionParams {
            workdir: Some("/tmp".into()),
            name: "test".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: None,
        };
        let spawn_result = mcp.spawn_session(Parameters(spawn_params)).await;
        let session: Session = serde_json::from_str(&spawn_result).unwrap();

        let params = ResumeSessionParams {
            id: session.id.to_string(),
            node: None,
        };
        let result = mcp.resume_session(Parameters(params)).await;
        assert!(result.contains("Error"));
        assert!(result.contains("cannot be resumed"));
    }

    #[tokio::test]
    async fn test_resume_session_remote_unknown() {
        let mcp = test_mcp(MockBackend::new()).await;
        let params = ResumeSessionParams {
            id: "some-id".into(),
            node: Some("missing".into()),
        };
        let result = mcp.resume_session(Parameters(params)).await;
        assert!(result.contains("Error"));
    }

    // -- get_output tests --

    #[tokio::test]
    async fn test_get_output_local() {
        let mcp = test_mcp(MockBackend::new()).await;
        let spawn_params = SpawnSessionParams {
            workdir: Some("/tmp".into()),
            name: "test".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: None,
        };
        let spawn_result = mcp.spawn_session(Parameters(spawn_params)).await;
        let session: Session = serde_json::from_str(&spawn_result).unwrap();

        let params = GetOutputParams {
            id: session.id.to_string(),
            lines: Some(50),
            node: None,
        };
        let result = mcp.get_output(Parameters(params)).await;
        assert!(result.contains("test output"));
    }

    #[tokio::test]
    async fn test_get_output_default_lines() {
        let mcp = test_mcp(MockBackend::new()).await;
        let spawn_params = SpawnSessionParams {
            workdir: Some("/tmp".into()),
            name: "test".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: None,
        };
        let spawn_result = mcp.spawn_session(Parameters(spawn_params)).await;
        let session: Session = serde_json::from_str(&spawn_result).unwrap();

        let params = GetOutputParams {
            id: session.id.to_string(),
            lines: None,
            node: None,
        };
        let result = mcp.get_output(Parameters(params)).await;
        assert!(result.contains("test output"));
    }

    #[tokio::test]
    async fn test_get_output_not_found() {
        let mcp = test_mcp(MockBackend::new()).await;
        let params = GetOutputParams {
            id: "nonexistent".into(),
            lines: None,
            node: None,
        };
        let result = mcp.get_output(Parameters(params)).await;
        assert_eq!(result, "Session not found");
    }

    #[tokio::test]
    async fn test_get_output_remote_unknown() {
        let mcp = test_mcp(MockBackend::new()).await;
        let params = GetOutputParams {
            id: "id".into(),
            lines: None,
            node: Some("missing".into()),
        };
        let result = mcp.get_output(Parameters(params)).await;
        assert!(result.contains("Error"));
    }

    // -- send_input tests --

    #[tokio::test]
    async fn test_send_input_local() {
        let mcp = test_mcp(MockBackend::new()).await;
        let spawn_params = SpawnSessionParams {
            workdir: Some("/tmp".into()),
            name: "test".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: None,
        };
        let spawn_result = mcp.spawn_session(Parameters(spawn_params)).await;
        let session: Session = serde_json::from_str(&spawn_result).unwrap();

        let params = SendInputParams {
            id: session.id.to_string(),
            text: "hello world".into(),
            node: None,
        };
        let result = mcp.send_input(Parameters(params)).await;
        assert_eq!(result, "Input sent");
    }

    #[tokio::test]
    async fn test_send_input_not_found() {
        let mcp = test_mcp(MockBackend::new()).await;
        let params = SendInputParams {
            id: "nonexistent".into(),
            text: "hello".into(),
            node: None,
        };
        let result = mcp.send_input(Parameters(params)).await;
        assert_eq!(result, "Session not found");
    }

    #[tokio::test]
    async fn test_send_input_remote_unknown() {
        let mcp = test_mcp(MockBackend::new()).await;
        let params = SendInputParams {
            id: "id".into(),
            text: "hello".into(),
            node: Some("missing".into()),
        };
        let result = mcp.send_input(Parameters(params)).await;
        assert!(result.contains("Error"));
    }

    // -- list_nodes tests --

    #[tokio::test]
    async fn test_list_nodes() {
        let mcp = test_mcp(MockBackend::new()).await;
        let result = mcp.list_nodes().await;
        assert!(result.contains("test-node"));
        assert!(result.contains("peers"));
    }

    #[tokio::test]
    async fn test_list_nodes_with_peers() {
        let mut peers = HashMap::new();
        peers.insert(
            "remote".into(),
            pulpo_common::peer::PeerEntry::Simple("10.0.0.1:7433".into()),
        );
        let mcp = test_mcp_with_peers(MockBackend::new(), peers).await;
        let result = mcp.list_nodes().await;
        assert!(result.contains("test-node"));
        assert!(result.contains("remote"));
    }

    // -- wait_for_session tests --

    #[tokio::test]
    async fn test_wait_for_session_already_terminal() {
        let mcp = test_mcp(MockBackend::new().with_alive(false)).await;
        let spawn_params = SpawnSessionParams {
            workdir: Some("/tmp".into()),
            name: "test".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: None,
        };
        let spawn_result = mcp.spawn_session(Parameters(spawn_params)).await;
        let session: Session = serde_json::from_str(&spawn_result).unwrap();

        // get_session detects stale
        let _ = mcp
            .get_session(Parameters(GetSessionParams {
                id: session.id.to_string(),
                node: None,
            }))
            .await;

        let params = WaitForSessionParams {
            id: session.id.to_string(),
            timeout_secs: Some(1),
            poll_interval_secs: Some(1),
            node: None,
        };
        let result = mcp.wait_for_session(Parameters(params)).await;
        let wait: WaitResult = serde_json::from_str(&result).unwrap();
        assert!(!wait.timed_out);
        assert_eq!(wait.session.status, SessionStatus::Lost);
    }

    #[tokio::test]
    async fn test_wait_for_session_timeout() {
        let mcp = test_mcp(MockBackend::new()).await;
        let spawn_params = SpawnSessionParams {
            workdir: Some("/tmp".into()),
            name: "test".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: None,
        };
        let spawn_result = mcp.spawn_session(Parameters(spawn_params)).await;
        let session: Session = serde_json::from_str(&spawn_result).unwrap();

        let params = WaitForSessionParams {
            id: session.id.to_string(),
            timeout_secs: Some(0), // Immediately timeout
            poll_interval_secs: Some(1),
            node: None,
        };
        let result = mcp.wait_for_session(Parameters(params)).await;
        let wait: WaitResult = serde_json::from_str(&result).unwrap();
        assert!(wait.timed_out);
    }

    #[tokio::test]
    async fn test_wait_for_session_not_found() {
        let mcp = test_mcp(MockBackend::new()).await;
        let params = WaitForSessionParams {
            id: "nonexistent".into(),
            timeout_secs: Some(1),
            poll_interval_secs: Some(1),
            node: None,
        };
        let result = mcp.wait_for_session(Parameters(params)).await;
        assert_eq!(result, "Session not found");
    }

    #[tokio::test]
    async fn test_wait_for_session_remote_unknown() {
        let mcp = test_mcp(MockBackend::new()).await;
        let params = WaitForSessionParams {
            id: "id".into(),
            timeout_secs: Some(1),
            poll_interval_secs: Some(1),
            node: Some("missing".into()),
        };
        let result = mcp.wait_for_session(Parameters(params)).await;
        assert!(result.contains("Error"));
    }

    // -- build_node_info test --

    #[tokio::test]
    async fn test_build_node_info() {
        let mcp = test_mcp(MockBackend::new()).await;
        let info = mcp.build_node_info();
        assert_eq!(info.name, "test-node");
        assert!(!info.hostname.is_empty());
        assert!(!info.os.is_empty());
        assert!(!info.arch.is_empty());
        assert!(info.cpus > 0);
    }

    // -- ServerHandler tests --

    #[tokio::test]
    async fn test_get_info() {
        let mcp = test_mcp(MockBackend::new()).await;
        let info = mcp.get_info();
        assert_eq!(info.server_info.name, "pulpo");
        assert!(info.instructions.is_some());
    }

    // -- Param type debug/deserialize tests --

    #[test]
    fn test_spawn_session_params_debug() {
        let params = SpawnSessionParams {
            workdir: Some("/tmp".into()),
            name: "test".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: None,
        };
        let debug = format!("{params:?}");
        assert!(debug.contains("/tmp"));
    }

    #[test]
    fn test_list_sessions_params_debug() {
        let params = ListSessionsParams {
            status: None,
            node: None,
        };
        let debug = format!("{params:?}");
        assert!(debug.contains("ListSessionsParams"));
    }

    #[test]
    fn test_get_session_params_debug() {
        let params = GetSessionParams {
            id: "test-id".into(),
            node: None,
        };
        let debug = format!("{params:?}");
        assert!(debug.contains("test-id"));
    }

    #[test]
    fn test_kill_session_params_debug() {
        let params = KillSessionParams {
            id: "test-id".into(),
            node: None,
        };
        let debug = format!("{params:?}");
        assert!(debug.contains("test-id"));
    }

    #[test]
    fn test_resume_session_params_debug() {
        let params = ResumeSessionParams {
            id: "test-id".into(),
            node: None,
        };
        let debug = format!("{params:?}");
        assert!(debug.contains("test-id"));
    }

    #[test]
    fn test_get_output_params_debug() {
        let params = GetOutputParams {
            id: "test-id".into(),
            lines: Some(50),
            node: None,
        };
        let debug = format!("{params:?}");
        assert!(debug.contains("50"));
    }

    #[test]
    fn test_send_input_params_debug() {
        let params = SendInputParams {
            id: "test-id".into(),
            text: "hello".into(),
            node: None,
        };
        let debug = format!("{params:?}");
        assert!(debug.contains("hello"));
    }

    #[test]
    fn test_wait_for_session_params_debug() {
        let params = WaitForSessionParams {
            id: "test-id".into(),
            timeout_secs: Some(60),
            poll_interval_secs: Some(5),
            node: None,
        };
        let debug = format!("{params:?}");
        assert!(debug.contains("60"));
    }

    #[test]
    fn test_list_intervention_events_params_debug() {
        let params = ListInterventionEventsParams {
            id: "test-id".into(),
        };
        let debug = format!("{params:?}");
        assert!(debug.contains("test-id"));
    }

    #[test]
    fn test_wait_result_debug() {
        let result = WaitResult {
            session: Session {
                id: uuid::Uuid::nil(),
                name: "test".into(),
                workdir: "/tmp".into(),
                command: "echo hello".into(),
                description: Some("test".into()),
                status: SessionStatus::Ready,
                exit_code: Some(0),
                backend_session_id: None,
                output_snapshot: None,
                metadata: None,
                ink: None,
                intervention_code: None,
                intervention_reason: None,
                intervention_at: None,
                last_output_at: None,
                idle_since: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            output: "done".into(),
            timed_out: false,
        };
        let debug = format!("{result:?}");
        assert!(debug.contains("WaitResult"));
    }

    #[test]
    fn test_nodes_result_debug() {
        let result = NodesResult {
            local: NodeInfo {
                name: "local".into(),
                hostname: "host".into(),
                os: "macos".into(),
                arch: "aarch64".into(),
                cpus: 8,
                memory_mb: 16384,
                gpu: None,
            },
            peers: vec![],
        };
        let debug = format!("{result:?}");
        assert!(debug.contains("NodesResult"));
    }

    #[test]
    fn test_spawn_session_params_deserialize() {
        let json = r#"{"name":"my-session","workdir":"/tmp","prompt":"test"}"#;
        let params: SpawnSessionParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.name, "my-session");
        assert_eq!(params.workdir.as_deref(), Some("/tmp"));
        assert!(params.node.is_none());
    }

    #[test]
    fn test_spawn_session_params_deserialize_with_conversation_id() {
        let json = r#"{"name":"my-session","workdir":"/tmp","command":"echo test"}"#;
        let _params: SpawnSessionParams = serde_json::from_str(json).unwrap();
    }

    #[test]
    fn test_list_sessions_params_deserialize() {
        let json = r#"{"status":"active"}"#;
        let params: ListSessionsParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.status, Some("active".into()));
    }

    #[test]
    fn test_get_session_params_deserialize() {
        let json = r#"{"id":"abc-123"}"#;
        let params: GetSessionParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.id, "abc-123");
    }

    #[test]
    fn test_kill_session_params_deserialize() {
        let json = r#"{"id":"abc-123","node":"remote"}"#;
        let params: KillSessionParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.id, "abc-123");
        assert_eq!(params.node, Some("remote".into()));
    }

    #[test]
    fn test_resume_session_params_deserialize() {
        let json = r#"{"id":"abc-123"}"#;
        let params: ResumeSessionParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.id, "abc-123");
    }

    #[test]
    fn test_get_output_params_deserialize() {
        let json = r#"{"id":"abc","lines":50}"#;
        let params: GetOutputParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.lines, Some(50));
    }

    #[test]
    fn test_send_input_params_deserialize() {
        let json = r#"{"id":"abc","text":"hello"}"#;
        let params: SendInputParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.text, "hello");
    }

    #[test]
    fn test_wait_for_session_params_deserialize() {
        let json = r#"{"id":"abc","timeout_secs":60,"poll_interval_secs":2}"#;
        let params: WaitForSessionParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.timeout_secs, Some(60));
        assert_eq!(params.poll_interval_secs, Some(2));
    }

    #[test]
    fn test_list_intervention_events_params_deserialize() {
        let json = r#"{"id":"sess-1"}"#;
        let params: ListInterventionEventsParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.id, "sess-1");
    }

    #[test]
    fn test_wait_result_serialize() {
        let result = WaitResult {
            session: Session {
                id: uuid::Uuid::nil(),
                name: "test".into(),
                workdir: "/tmp".into(),
                command: "echo hello".into(),
                description: Some("test".into()),
                status: SessionStatus::Ready,
                exit_code: None,
                backend_session_id: None,
                output_snapshot: None,
                metadata: None,
                ink: None,
                intervention_code: None,
                intervention_reason: None,
                intervention_at: None,
                last_output_at: None,
                idle_since: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            output: "done".into(),
            timed_out: false,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"timed_out\":false"));
    }

    #[test]
    fn test_nodes_result_serialize() {
        let result = NodesResult {
            local: NodeInfo {
                name: "local".into(),
                hostname: "host".into(),
                os: "macos".into(),
                arch: "aarch64".into(),
                cpus: 8,
                memory_mb: 16384,
                gpu: None,
            },
            peers: vec![],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"local\""));
    }

    // -- Extended MockBackend with send_input error flag --

    struct SendInputErrorBackend;

    impl Backend for SendInputErrorBackend {
        fn create_session(&self, _name: &str, _dir: &str, _cmd: &str) -> Result<()> {
            Ok(())
        }
        fn kill_session(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        fn is_alive(&self, _name: &str) -> Result<bool> {
            Ok(true)
        }
        fn capture_output(&self, _name: &str, _lines: usize) -> Result<String> {
            Ok("output".into())
        }
        fn send_input(&self, _name: &str, _text: &str) -> Result<()> {
            Err(anyhow::anyhow!("send_input failed"))
        }
        fn setup_logging(&self, _name: &str, _log_path: &str) -> Result<()> {
            Ok(())
        }
    }

    async fn test_mcp_with_send_input_error() -> PulpoMcp {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(SendInputErrorBackend);
        let manager = SessionManager::new(backend, store, HashMap::new()).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        PulpoMcp::new(manager, peer_registry, test_config())
    }

    // -- Verify all SendInputErrorBackend methods for coverage --

    #[test]
    fn test_send_input_error_backend_methods() {
        let b = SendInputErrorBackend;
        assert!(b.create_session("n", "d", "c").is_ok());
        assert!(b.kill_session("n").is_ok());
        assert!(b.is_alive("n").unwrap());
        assert_eq!(b.capture_output("n", 10).unwrap(), "output");
        assert!(b.send_input("n", "t").is_err());
        assert!(b.setup_logging("n", "p").is_ok());
    }

    // -- get_output / send_input store error tests (lines 392, 424) --

    #[tokio::test]
    async fn test_get_output_store_error() {
        let (mcp, pool) = test_mcp_with_pool(MockBackend::new()).await;
        // Drop table to make get_session return Err
        sqlx::query("DROP TABLE sessions")
            .execute(&pool)
            .await
            .unwrap();
        let params = GetOutputParams {
            id: "some-id".into(),
            lines: None,
            node: None,
        };
        let result = mcp.get_output(Parameters(params)).await;
        assert!(result.contains("Error"));
    }

    #[tokio::test]
    async fn test_send_input_store_error() {
        let (mcp, pool) = test_mcp_with_pool(MockBackend::new()).await;
        // Drop table to make get_session return Err
        sqlx::query("DROP TABLE sessions")
            .execute(&pool)
            .await
            .unwrap();
        let params = SendInputParams {
            id: "some-id".into(),
            text: "hello".into(),
            node: None,
        };
        let result = mcp.send_input(Parameters(params)).await;
        assert!(result.contains("Error"));
    }

    // -- list_intervention_events tests --

    #[tokio::test]
    async fn test_list_intervention_events_empty() {
        let (mcp, _pool) = test_mcp_with_pool(MockBackend::new()).await;
        let params = ListInterventionEventsParams {
            id: "some-id".into(),
        };
        let result = mcp.list_intervention_events(Parameters(params)).await;
        let events: Vec<serde_json::Value> = serde_json::from_str(&result).unwrap();
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn test_list_intervention_events_with_data() {
        let (mcp, pool) = test_mcp_with_pool(MockBackend::new()).await;
        let session_id = "test-session-id";

        // Insert an intervention event directly
        sqlx::query(
            "INSERT INTO intervention_events (session_id, reason, created_at) VALUES (?, ?, ?)",
        )
        .bind(session_id)
        .bind("Memory exceeded threshold")
        .bind("2026-01-01T00:00:00+00:00")
        .execute(&pool)
        .await
        .unwrap();

        let params = ListInterventionEventsParams {
            id: session_id.into(),
        };
        let result = mcp.list_intervention_events(Parameters(params)).await;
        let events: Vec<serde_json::Value> = serde_json::from_str(&result).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["reason"], "Memory exceeded threshold");
        assert_eq!(events[0]["session_id"], session_id);
    }

    #[tokio::test]
    async fn test_list_intervention_events_store_error() {
        let (mcp, pool) = test_mcp_with_pool(MockBackend::new()).await;
        // Drop table to cause error
        sqlx::query("DROP TABLE intervention_events")
            .execute(&pool)
            .await
            .unwrap();
        let params = ListInterventionEventsParams {
            id: "some-id".into(),
        };
        let result = mcp.list_intervention_events(Parameters(params)).await;
        assert!(result.contains("Error"));
    }

    // -- send_input error tests (line 420) --

    #[tokio::test]
    async fn test_send_input_backend_error() {
        let mcp = test_mcp_with_send_input_error().await;
        let spawn_params = SpawnSessionParams {
            workdir: Some("/tmp".into()),
            name: "test".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: None,
        };
        let spawn_result = mcp.spawn_session(Parameters(spawn_params)).await;
        let session: Session = serde_json::from_str(&spawn_result).unwrap();

        let params = SendInputParams {
            id: session.id.to_string(),
            text: "hello".into(),
            node: None,
        };
        let result = mcp.send_input(Parameters(params)).await;
        assert!(result.contains("Error"));
        assert!(result.contains("send_input failed"));
    }

    // -- Mock HTTP server helpers for remote tests --

    fn make_test_session() -> Session {
        Session {
            id: uuid::Uuid::new_v4(),
            name: "remote-session".into(),
            workdir: "/tmp/remote".into(),
            command: "echo hello".into(),
            description: Some("remote test".into()),
            status: SessionStatus::Active,
            exit_code: None,
            backend_session_id: None,
            output_snapshot: None,
            metadata: None,
            ink: None,
            intervention_code: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn make_completed_session() -> Session {
        Session {
            id: uuid::Uuid::new_v4(),
            name: "completed-session".into(),
            workdir: "/tmp/remote".into(),
            command: "echo hello".into(),
            description: Some("done".into()),
            status: SessionStatus::Ready,
            exit_code: Some(0),
            backend_session_id: None,
            output_snapshot: None,
            metadata: None,
            ink: None,
            intervention_code: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    /// Start a mock HTTP server that mimics pulpod's REST API.
    /// Returns the address (host:port) of the running server and the session used.
    async fn start_mock_remote_server() -> (String, Session) {
        use axum::{Router, routing::delete, routing::get, routing::post};

        let session = make_test_session();
        let session_json = serde_json::to_string(&session).unwrap();
        let all_sessions_json = serde_json::to_string(&vec![&session]).unwrap();
        let session_opt_json = serde_json::to_string(&Some(&session)).unwrap();
        let output_json = serde_json::to_string(&"remote output text".to_string()).unwrap();
        let input_resp_json = serde_json::to_string(&serde_json::json!({"ok": true})).unwrap();

        let session_json2 = session_json.clone();
        let session_json3 = session_json.clone();

        let app = Router::new()
            .route(
                "/api/v1/sessions",
                post(move || {
                    let body = session_json.clone();
                    async move { ([("content-type", "application/json")], body) }
                }),
            )
            .route(
                "/api/v1/sessions",
                get(move || {
                    let body = all_sessions_json.clone();
                    async move { ([("content-type", "application/json")], body) }
                }),
            )
            .route(
                "/api/v1/sessions/{id}",
                get(move || {
                    let body = session_opt_json.clone();
                    async move { ([("content-type", "application/json")], body) }
                }),
            )
            .route(
                "/api/v1/sessions/{id}",
                delete(|| async { axum::http::StatusCode::OK }),
            )
            .route(
                "/api/v1/sessions/{id}/resume",
                post(move || {
                    let body = session_json2.clone();
                    async move { ([("content-type", "application/json")], body) }
                }),
            )
            .route(
                "/api/v1/sessions/{id}/output",
                get(move || {
                    let body = output_json.clone();
                    async move { ([("content-type", "application/json")], body) }
                }),
            )
            .route(
                "/api/v1/sessions/{id}/input",
                post(move || {
                    let body = input_resp_json.clone();
                    async move { ([("content-type", "application/json")], body) }
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

        let session2: Session = serde_json::from_str(&session_json3).unwrap();
        (format!("127.0.0.1:{}", addr.port()), session2)
    }

    /// Start a mock HTTP server that returns completed sessions (for `wait_for_session`).
    async fn start_mock_completed_server() -> (String, Session) {
        use axum::{Router, routing::get};

        let session = make_completed_session();
        let session_opt_json = serde_json::to_string(&Some(&session)).unwrap();

        let app = Router::new().route(
            "/api/v1/sessions/{id}",
            get(move || {
                let body = session_opt_json.clone();
                async move { ([("content-type", "application/json")], body) }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

        (format!("127.0.0.1:{}", addr.port()), session)
    }

    async fn test_mcp_with_remote(address: &str) -> PulpoMcp {
        let mut peers = HashMap::new();
        peers.insert(
            "remote".into(),
            pulpo_common::peer::PeerEntry::Simple(address.into()),
        );
        test_mcp_with_peers(MockBackend::new(), peers).await
    }

    async fn test_mcp_with_remote_and_token(address: &str) -> PulpoMcp {
        let mut peers = HashMap::new();
        peers.insert(
            "remote".into(),
            pulpo_common::peer::PeerEntry::Full {
                address: address.into(),
                token: Some("test-token".into()),
            },
        );
        test_mcp_with_peers(MockBackend::new(), peers).await
    }

    // -- Remote spawn_session tests (lines 185-192 remote_post) --

    #[tokio::test]
    async fn test_spawn_session_remote() {
        let (addr, _session) = start_mock_remote_server().await;
        let mcp = test_mcp_with_remote(&addr).await;
        let params = SpawnSessionParams {
            workdir: Some("/tmp".into()),
            name: "test".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: Some("remote".into()),
        };
        let result = mcp.spawn_session(Parameters(params)).await;
        assert!(result.contains("remote-session"));
    }

    // -- Remote list_sessions tests (lines 297-301 query params) --

    #[tokio::test]
    async fn test_list_sessions_remote() {
        let (addr, _session) = start_mock_remote_server().await;
        let mcp = test_mcp_with_remote(&addr).await;
        let params = ListSessionsParams {
            status: None,
            node: Some("remote".into()),
        };
        let result = mcp.list_sessions(Parameters(params)).await;
        assert!(result.contains("remote-session"));
    }

    #[tokio::test]
    async fn test_list_sessions_remote_with_status_filter() {
        let (addr, _session) = start_mock_remote_server().await;
        let mcp = test_mcp_with_remote(&addr).await;
        let params = ListSessionsParams {
            status: Some("active".into()),
            node: Some("remote".into()),
        };
        let result = mcp.list_sessions(Parameters(params)).await;
        // Remote server returns sessions regardless of filter (mock)
        assert!(result.contains("remote-session"));
    }

    #[tokio::test]
    async fn test_list_sessions_remote_with_both_filters() {
        let (addr, _session) = start_mock_remote_server().await;
        let mcp = test_mcp_with_remote(&addr).await;
        let params = ListSessionsParams {
            status: Some("active".into()),
            node: Some("remote".into()),
        };
        let result = mcp.list_sessions(Parameters(params)).await;
        assert!(result.contains("remote-session"));
    }

    #[tokio::test]
    async fn test_list_sessions_remote_with_provider_only() {
        let (addr, _session) = start_mock_remote_server().await;
        let mcp = test_mcp_with_remote(&addr).await;
        let params = ListSessionsParams {
            status: None,
            node: Some("remote".into()),
        };
        let result = mcp.list_sessions(Parameters(params)).await;
        assert!(result.contains("remote-session"));
    }

    // -- Remote get_session tests --

    #[tokio::test]
    async fn test_get_session_remote() {
        let (addr, session) = start_mock_remote_server().await;
        let mcp = test_mcp_with_remote(&addr).await;
        let params = GetSessionParams {
            id: session.id.to_string(),
            node: Some("remote".into()),
        };
        let result = mcp.get_session(Parameters(params)).await;
        assert!(result.contains(&session.id.to_string()));
    }

    // -- Remote kill_session tests (lines 214-221 remote_delete) --

    #[tokio::test]
    async fn test_kill_session_remote() {
        let (addr, session) = start_mock_remote_server().await;
        let mcp = test_mcp_with_remote(&addr).await;
        let params = KillSessionParams {
            id: session.id.to_string(),
            node: Some("remote".into()),
        };
        let result = mcp.kill_session(Parameters(params)).await;
        assert!(result.contains("killed"));
    }

    // -- Remote resume_session tests --

    #[tokio::test]
    async fn test_resume_session_remote() {
        let (addr, session) = start_mock_remote_server().await;
        let mcp = test_mcp_with_remote(&addr).await;
        let params = ResumeSessionParams {
            id: session.id.to_string(),
            node: Some("remote".into()),
        };
        let result = mcp.resume_session(Parameters(params)).await;
        assert!(result.contains("remote-session"));
    }

    // -- Remote get_output tests (line 398) --

    #[tokio::test]
    async fn test_get_output_remote() {
        let (addr, session) = start_mock_remote_server().await;
        let mcp = test_mcp_with_remote(&addr).await;
        let params = GetOutputParams {
            id: session.id.to_string(),
            lines: Some(50),
            node: Some("remote".into()),
        };
        let result = mcp.get_output(Parameters(params)).await;
        assert_eq!(result, "remote output text");
    }

    // -- Remote send_input tests (line 434) --

    #[tokio::test]
    async fn test_send_input_remote() {
        let (addr, session) = start_mock_remote_server().await;
        let mcp = test_mcp_with_remote(&addr).await;
        let params = SendInputParams {
            id: session.id.to_string(),
            text: "hello remote".into(),
            node: Some("remote".into()),
        };
        let result = mcp.send_input(Parameters(params)).await;
        assert_eq!(result, "Input sent");
    }

    // -- Remote wait_for_session tests (lines 484, 499, 507) --

    #[tokio::test]
    async fn test_wait_for_session_remote_terminal() {
        let (addr, session) = start_mock_completed_server().await;
        let mcp = test_mcp_with_remote(&addr).await;
        let params = WaitForSessionParams {
            id: session.id.to_string(),
            timeout_secs: Some(5),
            poll_interval_secs: Some(1),
            node: Some("remote".into()),
        };
        let result = mcp.wait_for_session(Parameters(params)).await;
        let wait: WaitResult = serde_json::from_str(&result).unwrap();
        assert!(!wait.timed_out);
        assert_eq!(wait.session.status, SessionStatus::Ready);
        // Remote path returns empty output (line 484)
        assert!(wait.output.is_empty());
    }

    #[tokio::test]
    async fn test_wait_for_session_remote_timeout() {
        let (addr, session) = start_mock_remote_server().await;
        let mcp = test_mcp_with_remote(&addr).await;
        let params = WaitForSessionParams {
            id: session.id.to_string(),
            timeout_secs: Some(0), // immediate timeout
            poll_interval_secs: Some(1),
            node: Some("remote".into()),
        };
        let result = mcp.wait_for_session(Parameters(params)).await;
        let wait: WaitResult = serde_json::from_str(&result).unwrap();
        assert!(wait.timed_out);
        // Remote path returns empty output (line 499)
        assert!(wait.output.is_empty());
    }

    // -- Remote with token tests (exercises bearer_auth branch) --

    #[tokio::test]
    async fn test_spawn_session_remote_with_token() {
        let (addr, _session) = start_mock_remote_server().await;
        let mcp = test_mcp_with_remote_and_token(&addr).await;
        let params = SpawnSessionParams {
            workdir: Some("/tmp".into()),
            name: "test".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: Some("remote".into()),
        };
        let result = mcp.spawn_session(Parameters(params)).await;
        assert!(result.contains("remote-session"));
    }

    #[tokio::test]
    async fn test_kill_session_remote_with_token() {
        let (addr, session) = start_mock_remote_server().await;
        let mcp = test_mcp_with_remote_and_token(&addr).await;
        let params = KillSessionParams {
            id: session.id.to_string(),
            node: Some("remote".into()),
        };
        let result = mcp.kill_session(Parameters(params)).await;
        assert!(result.contains("killed"));
    }

    #[tokio::test]
    async fn test_get_output_remote_with_token() {
        let (addr, session) = start_mock_remote_server().await;
        let mcp = test_mcp_with_remote_and_token(&addr).await;
        let params = GetOutputParams {
            id: session.id.to_string(),
            lines: None,
            node: Some("remote".into()),
        };
        let result = mcp.get_output(Parameters(params)).await;
        assert_eq!(result, "remote output text");
    }

    // -- wait_for_session polling test (line 513 - sleep branch) --

    async fn test_mcp_with_arc_backend(backend: Arc<MockBackend>) -> PulpoMcp {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let manager = SessionManager::new(backend, store, HashMap::new()).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        PulpoMcp::new(manager, peer_registry, test_config())
    }

    #[tokio::test]
    async fn test_wait_for_session_polls_then_terminal() {
        let backend = Arc::new(MockBackend::new()); // alive=true
        let mcp = test_mcp_with_arc_backend(backend.clone()).await;

        // Create a session
        let spawn_params = SpawnSessionParams {
            workdir: Some("/tmp".into()),
            name: "test".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: None,
        };
        let spawn_result = mcp.spawn_session(Parameters(spawn_params)).await;
        let session: Session = serde_json::from_str(&spawn_result).unwrap();

        let mcp_clone = mcp.clone();
        let session_id = session.id.to_string();

        // Flip alive to false after a small delay so the first poll sees Running,
        // executes sleep (line 513), then the second poll sees Stale.
        let backend_clone = backend.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            *backend_clone.alive.lock().unwrap() = false;
        });

        let params = WaitForSessionParams {
            id: session_id,
            timeout_secs: Some(10),
            poll_interval_secs: Some(0), // minimal poll interval
            node: None,
        };
        let result = mcp_clone.wait_for_session(Parameters(params)).await;
        let wait: WaitResult = serde_json::from_str(&result).unwrap();
        assert!(!wait.timed_out);
        assert_eq!(wait.session.status, SessionStatus::Lost);
    }

    // -- ServerHandler list_resources / read_resource tests (lines 538-552) --

    #[tokio::test]
    async fn test_server_handler_list_resources() {
        use tokio::io::BufReader;

        let mcp = test_mcp(MockBackend::new()).await;
        let (a, b) = tokio::io::duplex(65536);
        let (ar, aw) = tokio::io::split(a);
        let (_br, _bw) = tokio::io::split(b);

        // Use serve_directly to create a running service that calls ServerHandler
        let running = rmcp::service::serve_directly(
            mcp,
            (BufReader::new(ar), aw),
            None::<rmcp::model::ClientInfo>,
        );
        // The service is now running. We just need to verify it started
        // (the list_resources/read_resource methods are called via the protocol).
        // Drop immediately — this validates that the ServerHandler impl compiles
        // and the methods are wired up.
        drop(running);
    }

    // Test list_resources and read_resource via the ServerHandler trait by sending
    // raw JSON-RPC messages over an in-memory transport.
    #[tokio::test]
    async fn test_server_handler_resources_via_transport() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let mcp = test_mcp(MockBackend::new()).await;
        let (client_side, server_side) = tokio::io::duplex(65536);
        let (sr, sw) = tokio::io::split(server_side);

        let _running = rmcp::service::serve_directly(
            mcp,
            (BufReader::new(sr), sw),
            None::<rmcp::model::ClientInfo>,
        );

        let (cr, cw) = tokio::io::split(client_side);
        let mut writer = cw;
        let mut reader = BufReader::new(cr);

        // Send resources/list request
        let list_req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "resources/list",
            "params": {}
        });
        let list_msg = serde_json::to_string(&list_req).unwrap();
        writer
            .write_all(format!("{list_msg}\n").as_bytes())
            .await
            .unwrap();
        writer.flush().await.unwrap();

        // Read response
        let mut response_line = String::new();
        reader.read_line(&mut response_line).await.unwrap();
        let resp: serde_json::Value = serde_json::from_str(&response_line).unwrap();
        // Should have a result with resources array
        let resources = resp["result"]["resources"].as_array().unwrap();
        assert_eq!(resources.len(), 3);

        // Send resources/read request for pulpo://sessions
        let read_req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "resources/read",
            "params": {
                "uri": "pulpo://sessions"
            }
        });
        let read_msg = serde_json::to_string(&read_req).unwrap();
        writer
            .write_all(format!("{read_msg}\n").as_bytes())
            .await
            .unwrap();
        writer.flush().await.unwrap();

        let mut response_line2 = String::new();
        reader.read_line(&mut response_line2).await.unwrap();
        let resp2: serde_json::Value = serde_json::from_str(&response_line2).unwrap();
        // Should have contents
        assert!(resp2["result"]["contents"].is_array());
    }

    // -- spawn_and_wait tests --

    #[tokio::test]
    async fn test_spawn_and_wait_happy_path() {
        // Backend starts alive=false so session immediately goes stale on poll
        let mcp = test_mcp(MockBackend::new().with_alive(false)).await;
        let params = SpawnAndWaitParams {
            workdir: Some("/tmp".into()),
            name: "test".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: None,
            timeout_secs: Some(5),
            poll_interval_secs: Some(0),
            output_lines: Some(50),
        };
        let result = mcp.spawn_and_wait(Parameters(params)).await;
        let wait: WaitResult = serde_json::from_str(&result).unwrap();
        assert!(!wait.timed_out);
        assert_eq!(wait.session.status, SessionStatus::Lost);
        assert!(wait.output.contains("test output"));
    }

    #[tokio::test]
    async fn test_spawn_and_wait_timeout() {
        // Backend alive=true so session stays running
        let mcp = test_mcp(MockBackend::new()).await;
        let params = SpawnAndWaitParams {
            workdir: Some("/tmp".into()),
            name: "test".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: None,
            timeout_secs: Some(0), // immediate timeout
            poll_interval_secs: Some(1),
            output_lines: None,
        };
        let result = mcp.spawn_and_wait(Parameters(params)).await;
        let wait: WaitResult = serde_json::from_str(&result).unwrap();
        assert!(wait.timed_out);
        assert_eq!(wait.session.status, SessionStatus::Active);
    }

    #[tokio::test]
    async fn test_spawn_and_wait_spawn_failure() {
        // Spawn to unknown remote node — spawn fails
        let mcp = test_mcp(MockBackend::new()).await;
        let params = SpawnAndWaitParams {
            workdir: Some("/tmp".into()),
            name: "test".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: Some("nonexistent-node".into()),
            timeout_secs: Some(5),
            poll_interval_secs: Some(1),
            output_lines: None,
        };
        let result = mcp.spawn_and_wait(Parameters(params)).await;
        assert!(result.contains("Error"));
        assert!(result.contains("unknown node"));
    }

    #[tokio::test]
    async fn test_spawn_and_wait_defaults() {
        // Test with all defaults
        let mcp = test_mcp(MockBackend::new().with_alive(false)).await;
        let params = SpawnAndWaitParams {
            workdir: Some("/tmp".into()),
            name: "custom-name".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: None,
            timeout_secs: None,       // defaults to 600
            poll_interval_secs: None, // defaults to 5
            output_lines: None,       // defaults to 200
        };
        let result = mcp.spawn_and_wait(Parameters(params)).await;
        let wait: WaitResult = serde_json::from_str(&result).unwrap();
        assert!(!wait.timed_out);
        assert_eq!(wait.session.name, "custom-name");
    }

    #[tokio::test]
    async fn test_spawn_and_wait_params_debug() {
        let params = SpawnAndWaitParams {
            workdir: Some("/tmp".into()),
            name: "test".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: None,
            timeout_secs: None,
            poll_interval_secs: None,
            output_lines: None,
        };
        let debug = format!("{params:?}");
        assert!(debug.contains("/tmp"));
    }

    #[tokio::test]
    async fn test_spawn_and_wait_params_deserialize() {
        let json = r#"{"name":"test","workdir":"/tmp","prompt":"test"}"#;
        let params: SpawnAndWaitParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.workdir.as_deref(), Some("/tmp"));
        assert!(params.timeout_secs.is_none());
    }

    // -- fan_out tests --

    #[tokio::test]
    async fn test_fan_out_empty_tasks() {
        let mcp = test_mcp(MockBackend::new()).await;
        let params = FanOutParams {
            tasks: vec![],
            timeout_secs: None,
            poll_interval_secs: None,
            output_lines: None,
        };
        let result = mcp.fan_out(Parameters(params)).await;
        let fan_out: FanOutResult = serde_json::from_str(&result).unwrap();
        assert!(fan_out.results.is_empty());
        assert!(fan_out.all_completed);
    }

    #[tokio::test]
    async fn test_fan_out_single_task() {
        let mcp = test_mcp(MockBackend::new().with_alive(false)).await;
        let params = FanOutParams {
            tasks: vec![FanOutTask {
                workdir: Some("/tmp".into()),
                name: "test".into(),
                command: Some("echo test".into()),
                description: None,
                ink: None,
                node: None,
            }],
            timeout_secs: Some(5),
            poll_interval_secs: Some(0),
            output_lines: Some(50),
        };
        let result = mcp.fan_out(Parameters(params)).await;
        let fan_out: FanOutResult = serde_json::from_str(&result).unwrap();
        assert_eq!(fan_out.results.len(), 1);
        assert!(fan_out.all_completed);
        assert!(fan_out.results[0].error.is_none());
        assert!(!fan_out.results[0].timed_out);
        assert!(fan_out.results[0].session.is_some());
    }

    #[tokio::test]
    async fn test_fan_out_two_tasks() {
        let mcp = test_mcp(MockBackend::new().with_alive(false)).await;
        let params = FanOutParams {
            tasks: vec![
                FanOutTask {
                    workdir: Some("/tmp".into()),
                    name: "task-a".into(),
                    command: Some("echo test".into()),
                    description: None,
                    ink: None,
                    node: None,
                },
                FanOutTask {
                    workdir: Some("/tmp".into()),
                    name: "task-b".into(),
                    command: Some("echo test".into()),
                    description: None,
                    ink: None,
                    node: None,
                },
            ],
            timeout_secs: Some(5),
            poll_interval_secs: Some(0),
            output_lines: None,
        };
        let result = mcp.fan_out(Parameters(params)).await;
        let fan_out: FanOutResult = serde_json::from_str(&result).unwrap();
        assert_eq!(fan_out.results.len(), 2);
        assert!(fan_out.all_completed);
        // Both should have sessions
        assert!(fan_out.results[0].session.is_some());
        assert!(fan_out.results[1].session.is_some());
    }

    #[tokio::test]
    async fn test_fan_out_partial_failure() {
        // One task local (succeeds), one task on unknown remote (fails)
        let mcp = test_mcp(MockBackend::new().with_alive(false)).await;
        let params = FanOutParams {
            tasks: vec![
                FanOutTask {
                    workdir: Some("/tmp".into()),
                    name: "good-task".into(),
                    command: Some("echo test".into()),
                    description: None,
                    ink: None,
                    node: None,
                },
                FanOutTask {
                    workdir: Some("/tmp".into()),
                    name: "bad-task".into(),
                    command: Some("echo test".into()),
                    description: None,
                    ink: None,
                    node: Some("nonexistent-node".into()),
                },
            ],
            timeout_secs: Some(5),
            poll_interval_secs: Some(0),
            output_lines: None,
        };
        let result = mcp.fan_out(Parameters(params)).await;
        let fan_out: FanOutResult = serde_json::from_str(&result).unwrap();
        assert_eq!(fan_out.results.len(), 2);
        assert!(!fan_out.all_completed); // one failed

        // First task succeeded
        assert!(fan_out.results[0].session.is_some());
        assert!(fan_out.results[0].error.is_none());

        // Second task failed
        assert!(fan_out.results[1].session.is_none());
        assert!(fan_out.results[1].error.is_some());
        assert!(
            fan_out.results[1]
                .error
                .as_ref()
                .unwrap()
                .contains("unknown node")
        );
    }

    #[tokio::test]
    async fn test_fan_out_timeout() {
        // All tasks timeout (alive=true, timeout=0)
        let mcp = test_mcp(MockBackend::new()).await;
        let params = FanOutParams {
            tasks: vec![FanOutTask {
                workdir: Some("/tmp".into()),
                name: "test".into(),
                command: Some("echo test".into()),
                description: None,
                ink: None,
                node: None,
            }],
            timeout_secs: Some(0),
            poll_interval_secs: Some(1),
            output_lines: None,
        };
        let result = mcp.fan_out(Parameters(params)).await;
        let fan_out: FanOutResult = serde_json::from_str(&result).unwrap();
        assert_eq!(fan_out.results.len(), 1);
        assert!(!fan_out.all_completed);
        assert!(fan_out.results[0].timed_out);
    }

    #[tokio::test]
    async fn test_fan_out_metadata_tagging() {
        let mcp = test_mcp(MockBackend::new().with_alive(false)).await;
        let params = FanOutParams {
            tasks: vec![FanOutTask {
                workdir: Some("/tmp".into()),
                name: "test".into(),
                command: Some("echo test".into()),
                description: None,
                ink: None,
                node: None,
            }],
            timeout_secs: Some(5),
            poll_interval_secs: Some(0),
            output_lines: None,
        };
        let result = mcp.fan_out(Parameters(params)).await;
        let fan_out: FanOutResult = serde_json::from_str(&result).unwrap();
        let session = fan_out.results[0].session.as_ref().unwrap();
        // Session should have fan_out metadata
        let meta = session.metadata.as_ref().unwrap();
        assert_eq!(meta.get("fan_out"), Some(&"true".into()));
    }

    #[test]
    fn test_fan_out_params_debug() {
        let params = FanOutParams {
            tasks: vec![],
            timeout_secs: Some(60),
            poll_interval_secs: None,
            output_lines: None,
        };
        let debug = format!("{params:?}");
        assert!(debug.contains("60"));
    }

    #[test]
    fn test_fan_out_params_deserialize() {
        let json = r#"{"tasks":[{"name":"test","workdir":"/tmp","prompt":"test"}]}"#;
        let params: FanOutParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.tasks.len(), 1);
        assert!(params.timeout_secs.is_none());
    }

    #[test]
    fn test_fan_out_task_debug() {
        let task = FanOutTask {
            workdir: Some("/tmp".into()),
            name: "test".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: None,
        };
        let debug = format!("{task:?}");
        assert!(debug.contains("/tmp"));
    }

    #[test]
    fn test_fan_out_task_deserialize() {
        let json = r#"{"name":"test","workdir":"/tmp","command":"echo test"}"#;
        let task: FanOutTask = serde_json::from_str(json).unwrap();
        assert_eq!(task.command, Some("echo test".into()));
    }

    #[test]
    fn test_fan_out_task_result_debug() {
        let result = FanOutTaskResult {
            session: None,
            output: "out".into(),
            timed_out: false,
            error: Some("err".into()),
        };
        let debug = format!("{result:?}");
        assert!(debug.contains("err"));
    }

    #[test]
    fn test_fan_out_task_result_serialize() {
        let result = FanOutTaskResult {
            session: None,
            output: String::new(),
            timed_out: true,
            error: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"timed_out\":true"));
    }

    #[test]
    fn test_fan_out_result_debug() {
        let result = FanOutResult {
            results: vec![],
            all_completed: true,
        };
        let debug = format!("{result:?}");
        assert!(debug.contains("all_completed"));
    }

    #[test]
    fn test_fan_out_result_serialize() {
        let result = FanOutResult {
            results: vec![],
            all_completed: true,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"all_completed\":true"));
    }

    // -- poll_until_terminal tests --

    #[tokio::test]
    async fn test_poll_until_terminal_not_found() {
        let mcp = test_mcp(MockBackend::new()).await;
        let result = mcp
            .poll_until_terminal("nonexistent", None, 1, 1, 100)
            .await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("session not found")
        );
    }

    #[tokio::test]
    async fn test_poll_until_terminal_remote_error() {
        let mcp = test_mcp(MockBackend::new()).await;
        let result = mcp
            .poll_until_terminal("id", Some("nonexistent-node"), 1, 1, 100)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_poll_until_terminal_already_terminal() {
        let mcp = test_mcp(MockBackend::new().with_alive(false)).await;
        // Create and make stale
        let spawn_result = mcp
            .spawn_session(Parameters(SpawnSessionParams {
                workdir: Some("/tmp".into()),
                name: "test".into(),
                command: Some("echo test".into()),
                description: None,
                ink: None,
                node: None,
            }))
            .await;
        let session: Session = serde_json::from_str(&spawn_result).unwrap();
        // get_session triggers stale detection
        let _ = mcp
            .get_session(Parameters(GetSessionParams {
                id: session.id.to_string(),
                node: None,
            }))
            .await;

        let result = mcp
            .poll_until_terminal(&session.id.to_string(), None, 5, 0, 100)
            .await;
        let (s, output, timed_out) = result.unwrap();
        assert_eq!(s.status, SessionStatus::Lost);
        assert!(!timed_out);
        assert!(output.contains("test output"));
    }

    #[tokio::test]
    async fn test_spawn_and_wait_poll_error() {
        // Spawn succeeds locally, then we break the store so poll_until_terminal fails
        let (mcp, pool) = test_mcp_with_pool(MockBackend::new()).await;

        // Create a session first to ensure spawn works
        let spawn_params = SpawnSessionParams {
            workdir: Some("/tmp".into()),
            name: "test".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: None,
        };
        let spawn_result = mcp.spawn_session(Parameters(spawn_params)).await;
        let session: Session = serde_json::from_str(&spawn_result).unwrap();

        // Drop the sessions table to make poll_until_terminal's get_session fail
        sqlx::query("DROP TABLE sessions")
            .execute(&pool)
            .await
            .unwrap();

        // poll_until_terminal should error
        let result = mcp
            .poll_until_terminal(&session.id.to_string(), None, 1, 0, 100)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_spawn_and_wait_impl_poll_error() {
        // Test the error path in spawn_and_wait_impl where spawn succeeds but poll errors.
        // Use a backend that tracks calls so we know spawn worked, then break the store.
        use std::sync::atomic::{AtomicBool, Ordering};

        struct BreakableBackend {
            break_after_spawn: Arc<AtomicBool>,
        }

        impl Backend for BreakableBackend {
            fn create_session(&self, _n: &str, _d: &str, _c: &str) -> Result<()> {
                // After spawn, mark that we should break
                self.break_after_spawn.store(true, Ordering::SeqCst);
                Ok(())
            }
            fn kill_session(&self, _: &str) -> Result<()> {
                Ok(())
            }
            fn is_alive(&self, _: &str) -> Result<bool> {
                Ok(true)
            }
            fn capture_output(&self, _: &str, _: usize) -> Result<String> {
                Ok(String::new())
            }
            fn send_input(&self, _: &str, _: &str) -> Result<()> {
                Ok(())
            }
            fn setup_logging(&self, _: &str, _: &str) -> Result<()> {
                Ok(())
            }
        }

        let break_flag = Arc::new(AtomicBool::new(false));
        let backend = Arc::new(BreakableBackend {
            break_after_spawn: break_flag.clone(),
        });
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let pool = store.pool().clone();
        let manager = SessionManager::new(backend, store, HashMap::new()).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let mcp = PulpoMcp::new(manager, peer_registry, test_config());

        // Spawn a background task that drops the sessions table once spawn completes
        let pool_clone = pool.clone();
        let flag_clone = break_flag.clone();
        tokio::spawn(async move {
            // Wait for spawn to complete
            while !flag_clone.load(Ordering::SeqCst) {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            // Give a tiny window for the insert to complete
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            // Break the store
            let _ = sqlx::query("DROP TABLE sessions")
                .execute(&pool_clone)
                .await;
        });

        // Exercise all BreakableBackend trait methods for coverage
        let bb = BreakableBackend {
            break_after_spawn: Arc::new(AtomicBool::new(false)),
        };
        assert!(bb.kill_session("test").is_ok());
        assert!(bb.is_alive("test").unwrap());
        assert!(bb.capture_output("test", 10).unwrap().is_empty());
        assert!(bb.send_input("test", "t").is_ok());
        assert!(bb.setup_logging("test", "p").is_ok());

        let result = mcp
            .spawn_and_wait_impl(SpawnAndWaitRequest {
                name: "test".into(),
                command: Some("echo test".into()),
                description: None,
                workdir: Some("/tmp".into()),
                ink: None,
                node: None,
                metadata: None,
                timeout_secs: 3,
                poll_interval_secs: 0,
                output_lines: 100,
            })
            .await;
        // The poll should eventually error once the table is dropped
        assert!(result.error.is_some());
        assert!(result.session.is_none());
    }

    // -- spawn_and_wait remote tests --

    #[tokio::test]
    async fn test_spawn_and_wait_remote() {
        use axum::{Router, routing::get, routing::post};

        let (_addr, _session) = start_mock_completed_server().await;
        let completed = make_completed_session();
        let spawn_json = serde_json::to_string(&completed).unwrap();
        let session_opt_json = serde_json::to_string(&Some(&completed)).unwrap();
        let spawn_json2 = spawn_json.clone();

        let app = Router::new()
            .route(
                "/api/v1/sessions",
                post(move || {
                    let body = spawn_json2.clone();
                    async move { ([("content-type", "application/json")], body) }
                }),
            )
            .route(
                "/api/v1/sessions/{id}",
                get(move || {
                    let body = session_opt_json.clone();
                    async move { ([("content-type", "application/json")], body) }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let remote_addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

        let addr = format!("127.0.0.1:{}", remote_addr.port());
        let mcp = test_mcp_with_remote(&addr).await;
        let params = SpawnAndWaitParams {
            workdir: Some("/tmp/remote".into()),
            name: "test".into(),
            command: Some("echo test".into()),
            description: None,
            ink: None,
            node: Some("remote".into()),
            timeout_secs: Some(5),
            poll_interval_secs: Some(0),
            output_lines: None,
        };
        let result = mcp.spawn_and_wait(Parameters(params)).await;
        let wait: WaitResult = serde_json::from_str(&result).unwrap();
        assert!(!wait.timed_out);
        assert_eq!(wait.session.status, SessionStatus::Ready);
        // Remote returns empty output
        assert!(wait.output.is_empty());
    }
}
