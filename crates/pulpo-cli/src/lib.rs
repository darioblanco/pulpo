use anyhow::Result;
use clap::{Parser, Subcommand};
#[cfg_attr(coverage, allow(unused_imports))]
use pulpo_common::api::{
    AuthTokenResponse, ConfigResponse, CreateSessionResponse, InterventionEventResponse,
    PeersResponse,
};
use pulpo_common::session::{Runtime, Session};

#[derive(Parser, Debug)]
#[command(
    name = "pulpo",
    about = "Manage agent sessions across your machines",
    version = env!("PULPO_VERSION"),
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    /// Target node (default: localhost)
    #[arg(long, default_value = "localhost:7433")]
    pub node: String,

    /// Auth token (auto-discovered from local daemon if omitted)
    #[arg(long)]
    pub token: Option<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Quick spawn: `pulpo <path>` spawns a session in that directory
    #[arg(value_name = "PATH")]
    pub path: Option<String>,
}

#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Commands {
    /// Attach to a session's terminal
    #[command(visible_alias = "a")]
    Attach {
        /// Session name or ID
        name: String,
    },

    /// Send input to a session
    #[command(visible_alias = "i", visible_alias = "send")]
    Input {
        /// Session name or ID
        name: String,
        /// Text to send (sends Enter if omitted)
        text: Option<String>,
    },

    /// Spawn a new agent session
    #[command(visible_alias = "s")]
    Spawn {
        /// Session name (auto-generated from workdir if omitted)
        name: Option<String>,

        /// Working directory (defaults to current directory)
        #[arg(long)]
        workdir: Option<String>,

        /// Ink name (from config)
        #[arg(long)]
        ink: Option<String>,

        /// Human-readable description of the task
        #[arg(long)]
        description: Option<String>,

        /// Don't attach to the session after spawning
        #[arg(short, long)]
        detach: bool,

        /// Idle threshold in seconds (0 = never idle)
        #[arg(long)]
        idle_threshold: Option<u32>,

        /// Auto-select the least loaded node
        #[arg(long)]
        auto: bool,

        /// Create an isolated git worktree for the session
        #[arg(long)]
        worktree: bool,

        /// Base branch to fork the worktree from (implies --worktree)
        #[arg(long = "worktree-base")]
        worktree_base: Option<String>,

        /// Runtime environment: tmux (default) or docker
        #[arg(long)]
        runtime: Option<String>,

        /// Secrets to inject as environment variables (by name)
        #[arg(long)]
        secret: Vec<String>,

        /// Command to run (everything after --)
        #[arg(last = true)]
        command: Vec<String>,
    },

    /// List sessions (live only by default)
    #[command(visible_alias = "ls")]
    List {
        /// Show all sessions including stopped and lost
        #[arg(short, long)]
        all: bool,
    },

    /// Show session logs/output
    #[command(visible_alias = "l")]
    Logs {
        /// Session name or ID
        name: String,

        /// Number of lines to fetch
        #[arg(long, default_value = "100")]
        lines: usize,

        /// Follow output (like `tail -f`)
        #[arg(short, long)]
        follow: bool,
    },

    /// Stop one or more sessions
    #[command(visible_alias = "k", alias = "kill")]
    Stop {
        /// Session names or IDs
        #[arg(required = true)]
        names: Vec<String>,

        /// Also purge the session from history
        #[arg(long, short = 'p')]
        purge: bool,
    },

    /// Remove all stopped and lost sessions
    Cleanup,

    /// Resume a lost session
    #[command(visible_alias = "r")]
    Resume {
        /// Session name or ID
        name: String,
    },

    /// List all known nodes
    #[command(visible_alias = "n")]
    Nodes,

    /// Show intervention history for a session
    #[command(visible_alias = "iv")]
    Interventions {
        /// Session name or ID
        name: String,
    },

    /// Open the web dashboard in your browser
    Ui,

    /// Manage scheduled agent runs
    #[command(visible_alias = "sched")]
    Schedule {
        #[command(subcommand)]
        action: ScheduleAction,
    },

    /// Manage secrets (environment variables injected into sessions)
    #[command(visible_alias = "sec")]
    Secret {
        #[command(subcommand)]
        action: SecretAction,
    },

    /// Manage git worktrees for sessions
    #[command(visible_alias = "wt")]
    Worktree {
        #[command(subcommand)]
        action: WorktreeAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum SecretAction {
    /// Set a secret
    Set {
        /// Secret name (will be the env var name, uppercase + underscores)
        name: String,
        /// Secret value
        value: String,
        /// Environment variable name (defaults to secret name)
        #[arg(long)]
        env: Option<String>,
    },
    /// List secret names
    #[command(visible_alias = "ls")]
    List,
    /// Delete a secret
    #[command(visible_alias = "rm")]
    Delete {
        /// Secret name
        name: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum WorktreeAction {
    /// List sessions that use git worktrees
    #[command(visible_alias = "ls")]
    List,
}

#[derive(Subcommand, Debug)]
pub enum ScheduleAction {
    /// Add a new schedule
    #[command(alias = "install")]
    Add {
        /// Schedule name
        name: String,
        /// Cron expression (e.g. "0 3 * * *")
        cron: String,
        /// Working directory
        #[arg(long)]
        workdir: Option<String>,
        /// Target node (omit = local, "auto" = least-loaded)
        #[arg(long)]
        node: Option<String>,
        /// Ink preset
        #[arg(long)]
        ink: Option<String>,
        /// Description
        #[arg(long)]
        description: Option<String>,
        /// Command to run (everything after --)
        #[arg(last = true)]
        command: Vec<String>,
    },
    /// List all schedules
    #[command(alias = "ls")]
    List,
    /// Remove a schedule
    #[command(alias = "rm")]
    Remove {
        /// Schedule name or ID
        name: String,
    },
    /// Pause a schedule
    Pause {
        /// Schedule name or ID
        name: String,
    },
    /// Resume a paused schedule
    Resume {
        /// Schedule name or ID
        name: String,
    },
}

/// The marker emitted by the agent wrapper when the agent process exits.
const AGENT_EXIT_MARKER: &str = "[pulpo] Agent exited";

/// Resolve a path to an absolute path string.
fn resolve_path(path: &str) -> String {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir().map_or_else(
            |_| path.to_owned(),
            |cwd| cwd.join(p).to_string_lossy().into_owned(),
        )
    }
}

/// Derive a session name from a directory path (basename, kebab-cased).
fn derive_session_name(path: &str) -> String {
    let basename = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("session");
    // Convert to kebab-case: lowercase, replace non-alphanumeric with hyphens, collapse
    let kebab: String = basename
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    // Collapse consecutive hyphens and trim leading/trailing hyphens
    let mut result = String::new();
    for c in kebab.chars() {
        if c == '-' && result.ends_with('-') {
            continue;
        }
        result.push(c);
    }
    let result = result.trim_matches('-').to_owned();
    if result.is_empty() {
        "session".to_owned()
    } else {
        result
    }
}

/// Deduplicate a session name by appending `-2`, `-3`, etc. if the base name is active.
async fn deduplicate_session_name(
    client: &reqwest::Client,
    base: &str,
    name: &str,
    token: Option<&str>,
) -> String {
    // Check if the name is already taken by fetching the session
    let resp = authed_get(client, format!("{base}/api/v1/sessions/{name}"), token)
        .send()
        .await;
    match resp {
        Ok(r) if r.status().is_success() => {
            // Session exists — try suffixed names
            for i in 2..=99 {
                let candidate = format!("{name}-{i}");
                let resp = authed_get(client, format!("{base}/api/v1/sessions/{candidate}"), token)
                    .send()
                    .await;
                match resp {
                    Ok(r) if r.status().is_success() => {}
                    _ => return candidate,
                }
            }
            format!("{name}-100")
        }
        _ => name.to_owned(),
    }
}

/// Format the base URL from the node address.
pub fn base_url(node: &str) -> String {
    format!("http://{node}")
}

/// Response shape for the output endpoint.
#[derive(serde::Deserialize)]
struct OutputResponse {
    output: String,
}

/// Format a list of sessions as a table.
const fn session_runtime(session: &Session) -> &'static str {
    match session.runtime {
        Runtime::Tmux => "tmux",
        Runtime::Docker => "docker",
    }
}

fn format_sessions(sessions: &[Session]) -> String {
    if sessions.is_empty() {
        return "No sessions.".into();
    }
    let mut lines = vec![format!(
        "{:<10} {:<24} {:<12} {:<8} {}",
        "ID", "NAME", "STATUS", "RUNTIME", "COMMAND"
    )];
    for s in sessions {
        let cmd_display = if s.command.len() > 50 {
            let truncated: String = s.command.chars().take(47).collect();
            format!("{truncated}...")
        } else {
            s.command.clone()
        };
        let short_id = &s.id.to_string()[..8];
        let has_pr = s
            .metadata
            .as_ref()
            .is_some_and(|m| m.contains_key("pr_url"));
        let name_display = match (s.worktree_path.is_some(), has_pr) {
            (true, true) => format!("{} [wt] [PR]", s.name),
            (true, false) => format!("{} [wt]", s.name),
            (false, true) => format!("{} [PR]", s.name),
            (false, false) => s.name.clone(),
        };
        lines.push(format!(
            "{:<10} {:<24} {:<12} {:<8} {}",
            short_id,
            name_display,
            s.status,
            session_runtime(s),
            cmd_display
        ));
    }
    lines.join("\n")
}

/// Format the peers response as a table.
fn format_nodes(resp: &PeersResponse) -> String {
    let mut lines = vec![format!(
        "{:<20} {:<25} {:<10} {}",
        "NAME", "ADDRESS", "STATUS", "SESSIONS"
    )];
    lines.push(format!(
        "{:<20} {:<25} {:<10} {}",
        resp.local.name, "(local)", "online", "-"
    ));
    for p in &resp.peers {
        let sessions = p
            .session_count
            .map_or_else(|| "-".into(), |c| c.to_string());
        lines.push(format!(
            "{:<20} {:<25} {:<10} {}",
            p.name, p.address, p.status, sessions
        ));
    }
    lines.join("\n")
}

/// Format intervention events as a table.
fn format_interventions(events: &[InterventionEventResponse]) -> String {
    if events.is_empty() {
        return "No intervention events.".into();
    }
    let mut lines = vec![format!("{:<8} {:<20} {}", "ID", "TIMESTAMP", "REASON")];
    for e in events {
        lines.push(format!("{:<8} {:<20} {}", e.id, e.created_at, e.reason));
    }
    lines.join("\n")
}

/// Build the command to open a URL in the default browser.
#[cfg_attr(coverage, allow(dead_code))]
fn build_open_command(url: &str) -> std::process::Command {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = std::process::Command::new("open");
        cmd.arg(url);
        cmd
    }
    #[cfg(target_os = "linux")]
    {
        let mut cmd = std::process::Command::new("xdg-open");
        cmd.arg(url);
        cmd
    }
    #[cfg(target_os = "windows")]
    {
        let mut cmd = std::process::Command::new("cmd");
        cmd.args(["/C", "start", url]);
        cmd
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        // Fallback: try xdg-open
        let mut cmd = std::process::Command::new("xdg-open");
        cmd.arg(url);
        cmd
    }
}

/// Open a URL in the default browser.
#[cfg(not(coverage))]
fn open_browser(url: &str) -> Result<()> {
    build_open_command(url).status()?;
    Ok(())
}

/// Stub for coverage builds — avoids opening a browser during tests.
#[cfg(coverage)]
fn open_browser(_url: &str) -> Result<()> {
    Ok(())
}

/// Build the command to attach to a session's terminal.
/// Detects Docker sessions by the `docker:` prefix in the backend session ID.
#[cfg_attr(coverage, allow(dead_code))]
fn build_attach_command(backend_session_id: &str) -> std::process::Command {
    // Docker sessions: exec into the container
    if let Some(container) = backend_session_id.strip_prefix("docker:") {
        let mut cmd = std::process::Command::new("docker");
        cmd.args(["exec", "-it", container, "/bin/sh"]);
        return cmd;
    }
    // tmux sessions
    #[cfg(not(target_os = "windows"))]
    {
        let mut cmd = std::process::Command::new("tmux");
        cmd.args(["attach-session", "-t", backend_session_id]);
        cmd
    }
    #[cfg(target_os = "windows")]
    {
        // tmux attach not available on Windows — inform the user
        let mut cmd = std::process::Command::new("cmd");
        cmd.args([
            "/C",
            "echo",
            "Attach not available on Windows. Use the web UI or --runtime docker.",
        ]);
        cmd
    }
}

/// Attach to a session's terminal.
#[cfg(not(any(test, coverage, target_os = "windows")))]
fn attach_session(backend_session_id: &str) -> Result<()> {
    let status = build_attach_command(backend_session_id).status()?;
    if !status.success() {
        anyhow::bail!("attach failed with {status}");
    }
    Ok(())
}

/// Stub for Windows — tmux attach is not available.
#[cfg(all(target_os = "windows", not(test), not(coverage)))]
fn attach_session(_backend_session_id: &str) -> Result<()> {
    eprintln!("tmux attach is not available on Windows. Use the web UI or --runtime docker.");
    Ok(())
}

/// Stub for test and coverage builds — avoids spawning real terminals during tests.
#[cfg(any(test, coverage))]
#[allow(clippy::unnecessary_wraps, clippy::missing_const_for_fn)]
fn attach_session(_backend_session_id: &str) -> Result<()> {
    Ok(())
}

/// Extract a clean error message from an API JSON response (or fall back to raw text).
fn api_error(text: &str) -> anyhow::Error {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| v["error"].as_str().map(String::from))
        .map_or_else(|| anyhow::anyhow!("{text}"), |msg| anyhow::anyhow!("{msg}"))
}

/// Return the response body text, or a clean error if the response was non-success.
async fn ok_or_api_error(resp: reqwest::Response) -> Result<String> {
    if resp.status().is_success() {
        Ok(resp.text().await?)
    } else {
        let text = resp.text().await?;
        Err(api_error(&text))
    }
}

/// Map a reqwest error to a user-friendly message.
fn friendly_error(err: &reqwest::Error, node: &str) -> anyhow::Error {
    if err.is_connect() {
        anyhow::anyhow!(
            "Could not connect to pulpod at {node}. Is the daemon running?\nStart it with: brew services start pulpo"
        )
    } else {
        anyhow::anyhow!("Network error connecting to {node}: {err}")
    }
}

/// Check if the node address points to localhost.
fn is_localhost(node: &str) -> bool {
    let host = node.split(':').next().unwrap_or(node);
    host == "localhost" || host == "127.0.0.1" || node.starts_with("[::1]") || node == "::1"
}

/// Try to auto-discover the auth token from a local daemon.
async fn discover_token(client: &reqwest::Client, base: &str) -> Option<String> {
    let resp = client
        .get(format!("{base}/api/v1/auth/token"))
        .send()
        .await
        .ok()?;
    let body: AuthTokenResponse = resp.json().await.ok()?;
    if body.token.is_empty() {
        None
    } else {
        Some(body.token)
    }
}

/// Resolve the auth token: use explicit `--token`, auto-discover from localhost, or `None`.
async fn resolve_token(
    client: &reqwest::Client,
    base: &str,
    node: &str,
    explicit: Option<&str>,
) -> Option<String> {
    if let Some(t) = explicit {
        return Some(t.to_owned());
    }
    if is_localhost(node) {
        return discover_token(client, base).await;
    }
    None
}

/// Check if a node string needs resolution (no port specified).
fn node_needs_resolution(node: &str) -> bool {
    !node.contains(':')
}

/// Resolve a node reference to a `host:port` address.
///
/// If `node` looks like `host:port` (contains `:`), return as-is with no peer token.
/// Otherwise, query the local daemon's peer registry for a matching name. If a matching
/// online peer is found, return its address and optionally its configured auth token
/// (from the config endpoint). Falls back to appending `:7433` if the peer is not found.
#[cfg(not(coverage))]
async fn resolve_node(client: &reqwest::Client, node: &str) -> (String, Option<String>) {
    // Already has port — use as-is
    if !node_needs_resolution(node) {
        return (node.to_owned(), None);
    }

    // Try to resolve via local daemon's peer registry
    let local_base = "http://localhost:7433";
    let mut resolved_address: Option<String> = None;

    if let Ok(resp) = client
        .get(format!("{local_base}/api/v1/peers"))
        .send()
        .await
        && let Ok(peers_resp) = resp.json::<PeersResponse>().await
    {
        for peer in &peers_resp.peers {
            if peer.name == node {
                resolved_address = Some(peer.address.clone());
                break;
            }
        }
    }

    let address = resolved_address.unwrap_or_else(|| format!("{node}:7433"));

    // Try to get the peer's auth token from the config endpoint
    let peer_token = if let Ok(resp) = client
        .get(format!("{local_base}/api/v1/config"))
        .send()
        .await
        && let Ok(config) = resp.json::<ConfigResponse>().await
        && let Some(entry) = config.peers.get(node)
    {
        entry.token().map(String::from)
    } else {
        None
    };

    (address, peer_token)
}

/// Coverage stub — no real HTTP resolution during coverage builds.
#[cfg(coverage)]
async fn resolve_node(_client: &reqwest::Client, node: &str) -> (String, Option<String>) {
    if node_needs_resolution(node) {
        (format!("{node}:7433"), None)
    } else {
        (node.to_owned(), None)
    }
}

/// Build an authenticated GET request.
fn authed_get(
    client: &reqwest::Client,
    url: String,
    token: Option<&str>,
) -> reqwest::RequestBuilder {
    let req = client.get(url);
    if let Some(t) = token {
        req.bearer_auth(t)
    } else {
        req
    }
}

/// Build an authenticated POST request.
fn authed_post(
    client: &reqwest::Client,
    url: String,
    token: Option<&str>,
) -> reqwest::RequestBuilder {
    let req = client.post(url);
    if let Some(t) = token {
        req.bearer_auth(t)
    } else {
        req
    }
}

/// Build an authenticated DELETE request.
fn authed_delete(
    client: &reqwest::Client,
    url: String,
    token: Option<&str>,
) -> reqwest::RequestBuilder {
    let req = client.delete(url);
    if let Some(t) = token {
        req.bearer_auth(t)
    } else {
        req
    }
}

/// Build an authenticated PUT request.
#[cfg(not(coverage))]
fn authed_put(
    client: &reqwest::Client,
    url: String,
    token: Option<&str>,
) -> reqwest::RequestBuilder {
    let req = client.put(url);
    if let Some(t) = token {
        req.bearer_auth(t)
    } else {
        req
    }
}

/// Fetch session output from the API.
async fn fetch_output(
    client: &reqwest::Client,
    base: &str,
    name: &str,
    lines: usize,
    token: Option<&str>,
) -> Result<String> {
    let resp = authed_get(
        client,
        format!("{base}/api/v1/sessions/{name}/output?lines={lines}"),
        token,
    )
    .send()
    .await?;
    let text = ok_or_api_error(resp).await?;
    let output: OutputResponse = serde_json::from_str(&text)?;
    Ok(output.output)
}

/// Fetch session status from the API.
async fn fetch_session_status(
    client: &reqwest::Client,
    base: &str,
    name: &str,
    token: Option<&str>,
) -> Result<String> {
    let resp = authed_get(client, format!("{base}/api/v1/sessions/{name}"), token)
        .send()
        .await?;
    let text = ok_or_api_error(resp).await?;
    let session: Session = serde_json::from_str(&text)?;
    Ok(session.status.to_string())
}

/// Wait for the session to leave "creating" state, then check if it died instantly.
/// Uses the session ID (not name) to avoid matching old stopped sessions with the same name.
/// Returns an error with a helpful message if the session is lost/stopped.
async fn check_session_alive(
    client: &reqwest::Client,
    base: &str,
    session_id: &str,
    token: Option<&str>,
) -> Result<()> {
    // Poll up to 3 times at 500ms intervals — handles slow daemons and Docker pull delays
    for _ in 0..3 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        // Fetch by ID to avoid name collisions with old sessions
        let resp = authed_get(
            client,
            format!("{base}/api/v1/sessions/{session_id}"),
            token,
        )
        .send()
        .await;
        if let Ok(resp) = resp
            && let Ok(text) = ok_or_api_error(resp).await
            && let Ok(session) = serde_json::from_str::<Session>(&text)
        {
            match session.status.to_string().as_str() {
                "creating" => continue,
                "lost" | "stopped" => {
                    anyhow::bail!(
                        "Session \"{}\" exited immediately — the command may have failed.\n  Check logs: pulpo logs {}",
                        session.name,
                        session.name
                    );
                }
                _ => return Ok(()),
            }
        }
        // fetch failed — don't block, proceed to attach
        break;
    }
    Ok(())
}

/// Compute the new trailing lines that differ from the previous output.
///
/// The output endpoint returns the last N lines from the terminal pane. As new lines
/// appear, old lines at the top scroll off. We find the overlap between the end
/// of `prev` and the beginning-to-middle of `new`, then return only the truly new
/// trailing lines.
fn diff_output<'a>(prev: &str, new: &'a str) -> &'a str {
    if prev.is_empty() {
        return new;
    }

    let prev_lines: Vec<&str> = prev.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    if new_lines.is_empty() {
        return "";
    }

    // prev is non-empty (early return above), so last() always succeeds
    let last_prev = prev_lines[prev_lines.len() - 1];

    // Find the last line of prev in new to determine the overlap boundary
    for i in (0..new_lines.len()).rev() {
        if new_lines[i] == last_prev {
            // Verify contiguous overlap: check that lines before this match too
            let overlap_len = prev_lines.len().min(i + 1);
            let prev_tail = &prev_lines[prev_lines.len() - overlap_len..];
            let new_overlap = &new_lines[i + 1 - overlap_len..=i];
            if prev_tail == new_overlap {
                if i + 1 < new_lines.len() {
                    // Return the slice of `new` after the overlap
                    let consumed: usize = new_lines[..=i].iter().map(|l| l.len() + 1).sum();
                    return new.get(consumed.min(new.len())..).unwrap_or("");
                }
                return "";
            }
        }
    }

    // No overlap found — output changed completely, print it all
    new
}

/// Follow logs by polling, printing only new output. Returns when the session ends.
async fn follow_logs(
    client: &reqwest::Client,
    base: &str,
    name: &str,
    lines: usize,
    token: Option<&str>,
    writer: &mut (dyn std::io::Write + Send),
) -> Result<()> {
    let mut prev_output = fetch_output(client, base, name, lines, token).await?;
    write!(writer, "{prev_output}")?;

    let mut unchanged_ticks: u32 = 0;

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // Fetch latest output
        let new_output = fetch_output(client, base, name, lines, token).await?;

        let diff = diff_output(&prev_output, &new_output);
        if diff.is_empty() {
            unchanged_ticks += 1;
        } else {
            write!(writer, "{diff}")?;
            unchanged_ticks = 0;
        }

        // Check for agent exit marker in output
        if new_output.contains(AGENT_EXIT_MARKER) {
            break;
        }

        prev_output = new_output;

        // Only check session status when output has been unchanged for 3+ ticks
        if unchanged_ticks >= 3 {
            let status = fetch_session_status(client, base, name, token).await?;
            let is_terminal = status == "ready" || status == "stopped" || status == "lost";
            if is_terminal {
                break;
            }
        }
    }
    Ok(())
}

// --- Schedule API ---

/// Execute a schedule subcommand via the scheduler API.
#[cfg(not(coverage))]
async fn execute_schedule(
    client: &reqwest::Client,
    action: &ScheduleAction,
    base: &str,
    token: Option<&str>,
) -> Result<String> {
    match action {
        ScheduleAction::Add {
            name,
            cron,
            workdir,
            node,
            ink,
            description,
            command,
        } => {
            let cmd = if command.is_empty() {
                None
            } else {
                Some(command.join(" "))
            };
            let resolved_workdir = workdir.clone().unwrap_or_else(|| {
                std::env::current_dir()
                    .map_or_else(|_| ".".into(), |p| p.to_string_lossy().into_owned())
            });
            let mut body = serde_json::json!({
                "name": name,
                "cron": cron,
                "workdir": resolved_workdir,
            });
            if let Some(c) = &cmd {
                body["command"] = serde_json::json!(c);
            }
            if let Some(n) = node {
                body["target_node"] = serde_json::json!(n);
            }
            if let Some(i) = ink {
                body["ink"] = serde_json::json!(i);
            }
            if let Some(d) = description {
                body["description"] = serde_json::json!(d);
            }
            let resp = authed_post(client, format!("{base}/api/v1/schedules"), token)
                .json(&body)
                .send()
                .await?;
            ok_or_api_error(resp).await?;
            Ok(format!("Created schedule \"{name}\""))
        }
        ScheduleAction::List => {
            let resp = authed_get(client, format!("{base}/api/v1/schedules"), token)
                .send()
                .await?;
            let text = ok_or_api_error(resp).await?;
            let schedules: Vec<serde_json::Value> = serde_json::from_str(&text)?;
            Ok(format_schedules(&schedules))
        }
        ScheduleAction::Remove { name } => {
            let resp = authed_delete(client, format!("{base}/api/v1/schedules/{name}"), token)
                .send()
                .await?;
            ok_or_api_error(resp).await?;
            Ok(format!("Removed schedule \"{name}\""))
        }
        ScheduleAction::Pause { name } => {
            let body = serde_json::json!({ "enabled": false });
            let resp = authed_put(client, format!("{base}/api/v1/schedules/{name}"), token)
                .json(&body)
                .send()
                .await?;
            ok_or_api_error(resp).await?;
            Ok(format!("Paused schedule \"{name}\""))
        }
        ScheduleAction::Resume { name } => {
            let body = serde_json::json!({ "enabled": true });
            let resp = authed_put(client, format!("{base}/api/v1/schedules/{name}"), token)
                .json(&body)
                .send()
                .await?;
            ok_or_api_error(resp).await?;
            Ok(format!("Resumed schedule \"{name}\""))
        }
    }
}

/// Coverage stub for schedule execution.
#[cfg(coverage)]
#[allow(clippy::unnecessary_wraps)]
async fn execute_schedule(
    _client: &reqwest::Client,
    _action: &ScheduleAction,
    _base: &str,
    _token: Option<&str>,
) -> Result<String> {
    Ok(String::new())
}

// --- Secret API ---

/// Format secret entries as a table.
#[cfg_attr(coverage, allow(dead_code))]
fn format_secrets(secrets: &[serde_json::Value]) -> String {
    if secrets.is_empty() {
        return "No secrets configured.".into();
    }
    let mut lines = vec![format!("{:<24} {:<24} {}", "NAME", "ENV", "CREATED")];
    for s in secrets {
        let name = s["name"].as_str().unwrap_or("?");
        let env_display = s["env"]
            .as_str()
            .map_or_else(|| name.to_owned(), String::from);
        let created = s["created_at"]
            .as_str()
            .map_or("-", |t| if t.len() >= 16 { &t[..16] } else { t });
        lines.push(format!("{name:<24} {env_display:<24} {created}"));
    }
    lines.join("\n")
}

/// Execute a secret subcommand via the secrets API.
#[cfg(not(coverage))]
async fn execute_secret(
    client: &reqwest::Client,
    action: &SecretAction,
    base: &str,
    token: Option<&str>,
) -> Result<String> {
    match action {
        SecretAction::Set { name, value, env } => {
            let mut body = serde_json::json!({ "value": value });
            if let Some(e) = env {
                body["env"] = serde_json::json!(e);
            }
            let resp = authed_put(client, format!("{base}/api/v1/secrets/{name}"), token)
                .json(&body)
                .send()
                .await?;
            ok_or_api_error(resp).await?;
            Ok(format!("Secret \"{name}\" set."))
        }
        SecretAction::List => {
            let resp = authed_get(client, format!("{base}/api/v1/secrets"), token)
                .send()
                .await?;
            let text = ok_or_api_error(resp).await?;
            let parsed: serde_json::Value = serde_json::from_str(&text)?;
            let secrets = parsed["secrets"].as_array().map_or(&[][..], Vec::as_slice);
            Ok(format_secrets(secrets))
        }
        SecretAction::Delete { name } => {
            let resp = authed_delete(client, format!("{base}/api/v1/secrets/{name}"), token)
                .send()
                .await?;
            ok_or_api_error(resp).await?;
            Ok(format!("Secret \"{name}\" deleted."))
        }
    }
}

/// Coverage stub for secret execution.
#[cfg(coverage)]
#[allow(clippy::unnecessary_wraps)]
async fn execute_secret(
    _client: &reqwest::Client,
    _action: &SecretAction,
    _base: &str,
    _token: Option<&str>,
) -> Result<String> {
    Ok(String::new())
}

/// Execute a worktree subcommand.
#[cfg(not(coverage))]
async fn execute_worktree(
    client: &reqwest::Client,
    action: &WorktreeAction,
    base: &str,
    token: Option<&str>,
) -> Result<String> {
    match action {
        WorktreeAction::List => {
            let resp = authed_get(client, format!("{base}/api/v1/sessions"), token)
                .send()
                .await?;
            let text = ok_or_api_error(resp).await?;
            let sessions: Vec<Session> = serde_json::from_str(&text)?;
            let wt_sessions: Vec<&Session> = sessions
                .iter()
                .filter(|s| s.worktree_path.is_some())
                .collect();
            Ok(format_worktree_sessions(&wt_sessions))
        }
    }
}

/// Coverage stub for worktree execution.
#[cfg(coverage)]
#[allow(clippy::unnecessary_wraps)]
async fn execute_worktree(
    _client: &reqwest::Client,
    _action: &WorktreeAction,
    _base: &str,
    _token: Option<&str>,
) -> Result<String> {
    Ok(String::new())
}

/// Format worktree sessions as a table.
fn format_worktree_sessions(sessions: &[&Session]) -> String {
    if sessions.is_empty() {
        return "No worktree sessions.".into();
    }
    let mut lines = vec![format!(
        "{:<20} {:<20} {:<10} {}",
        "NAME", "BRANCH", "STATUS", "PATH"
    )];
    for s in sessions {
        let branch = s.worktree_branch.as_deref().unwrap_or("-");
        let path = s.worktree_path.as_deref().unwrap_or("-");
        lines.push(format!(
            "{:<20} {:<20} {:<10} {}",
            s.name, branch, s.status, path
        ));
    }
    lines.join("\n")
}

/// Format a list of schedules as a table.
#[cfg_attr(coverage, allow(dead_code))]
fn format_schedules(schedules: &[serde_json::Value]) -> String {
    if schedules.is_empty() {
        return "No schedules.".into();
    }
    let mut lines = vec![format!(
        "{:<20} {:<18} {:<8} {:<12} {}",
        "NAME", "CRON", "ENABLED", "LAST RUN", "NODE"
    )];
    for s in schedules {
        let name = s["name"].as_str().unwrap_or("?");
        let cron = s["cron"].as_str().unwrap_or("?");
        let enabled = if s["enabled"].as_bool().unwrap_or(true) {
            "yes"
        } else {
            "no"
        };
        let last_run = s["last_run_at"]
            .as_str()
            .map_or("-", |t| if t.len() >= 16 { &t[..16] } else { t });
        let node = s["target_node"].as_str().unwrap_or("local");
        lines.push(format!(
            "{name:<20} {cron:<18} {enabled:<8} {last_run:<12} {node}"
        ));
    }
    lines.join("\n")
}

/// Select the best node from the peer registry based on load.
/// Returns the node address and name of the least loaded online peer.
/// Scoring: lower memory usage + fewer active sessions = better.
#[cfg(not(coverage))]
async fn select_best_node(
    client: &reqwest::Client,
    base: &str,
    token: Option<&str>,
) -> Result<(String, String)> {
    let resp = authed_get(client, format!("{base}/api/v1/peers"), token)
        .send()
        .await?;
    let text = ok_or_api_error(resp).await?;
    let peers_resp: PeersResponse = serde_json::from_str(&text)?;

    // Score: fewer active sessions is better, more memory is better
    let mut best: Option<(String, String, f64)> = None; // (address, name, score)

    for peer in &peers_resp.peers {
        if peer.status != pulpo_common::peer::PeerStatus::Online {
            continue;
        }
        let sessions = peer.session_count.unwrap_or(0);
        let mem = peer.node_info.as_ref().map_or(0, |n| n.memory_mb);
        // Lower score = better (fewer sessions, more memory)
        #[allow(clippy::cast_precision_loss)]
        let score = sessions as f64 - (mem as f64 / 1024.0);
        if best.as_ref().is_none_or(|(_, _, s)| score < *s) {
            best = Some((peer.address.clone(), peer.name.clone(), score));
        }
    }

    // Fall back to local if no online peers
    match best {
        Some((addr, name, _)) => Ok((addr, name)),
        None => Ok(("localhost:7433".into(), peers_resp.local.name)),
    }
}

/// Coverage stub — auto-select always falls back to local.
#[cfg(coverage)]
#[allow(clippy::unnecessary_wraps)]
async fn select_best_node(
    _client: &reqwest::Client,
    _base: &str,
    _token: Option<&str>,
) -> Result<(String, String)> {
    Ok(("localhost:7433".into(), "local".into()))
}

/// Execute the given CLI command against the specified node.
#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli) -> Result<String> {
    let client = reqwest::Client::new();
    let (resolved_node, peer_token) = resolve_node(&client, &cli.node).await;
    let url = base_url(&resolved_node);
    let node = &resolved_node;
    let token = resolve_token(&client, &url, node, cli.token.as_deref())
        .await
        .or(peer_token);

    // Handle `pulpo <path>` shortcut — spawn a session in the given directory
    if cli.command.is_none() && cli.path.is_none() {
        // No subcommand and no path: print help
        use clap::CommandFactory;
        let mut cmd = Cli::command();
        cmd.print_help()?;
        println!();
        return Ok(String::new());
    }
    if cli.command.is_none() {
        let path = cli.path.as_deref().unwrap_or(".");
        let resolved_workdir = resolve_path(path);
        let base_name = derive_session_name(&resolved_workdir);
        let name = deduplicate_session_name(&client, &url, &base_name, token.as_deref()).await;
        let body = serde_json::json!({
            "name": name,
            "workdir": resolved_workdir,
        });
        let resp = authed_post(&client, format!("{url}/api/v1/sessions"), token.as_deref())
            .json(&body)
            .send()
            .await
            .map_err(|e| friendly_error(&e, node))?;
        let text = ok_or_api_error(resp).await?;
        let resp: CreateSessionResponse = serde_json::from_str(&text)?;
        let msg = format!(
            "Created session \"{}\" ({})",
            resp.session.name, resp.session.id
        );
        let backend_id = resp
            .session
            .backend_session_id
            .as_deref()
            .unwrap_or(&resp.session.name);
        eprintln!("{msg}");
        // Path shortcut spawns a shell (no command) — skip liveness check
        // since shell sessions are immediately detected as idle by the watchdog
        attach_session(backend_id)?;
        return Ok(format!("Detached from session \"{}\".", resp.session.name));
    }

    match cli.command.as_ref().unwrap() {
        Commands::Attach { name } => {
            // Fetch session to get status and backend_session_id
            let resp = authed_get(
                &client,
                format!("{url}/api/v1/sessions/{name}"),
                token.as_deref(),
            )
            .send()
            .await
            .map_err(|e| friendly_error(&e, node))?;
            let text = ok_or_api_error(resp).await?;
            let session: Session = serde_json::from_str(&text)?;
            match session.status.to_string().as_str() {
                "lost" => {
                    anyhow::bail!(
                        "Session \"{name}\" is lost (agent process died). Resume it first:\n  pulpo resume {name}"
                    );
                }
                "stopped" => {
                    anyhow::bail!(
                        "Session \"{name}\" is {} — cannot attach to a stopped session.",
                        session.status
                    );
                }
                _ => {}
            }
            let backend_id = session.backend_session_id.unwrap_or_else(|| name.clone());
            attach_session(&backend_id)?;
            Ok(format!("Detached from session {name}."))
        }
        Commands::Input { name, text } => {
            let input_text = text.as_deref().unwrap_or("\n");
            let body = serde_json::json!({ "text": input_text });
            let resp = authed_post(
                &client,
                format!("{url}/api/v1/sessions/{name}/input"),
                token.as_deref(),
            )
            .json(&body)
            .send()
            .await
            .map_err(|e| friendly_error(&e, node))?;
            ok_or_api_error(resp).await?;
            Ok(format!("Sent input to session {name}."))
        }
        Commands::List { all } => {
            let list_url = if *all {
                format!("{url}/api/v1/sessions")
            } else {
                format!("{url}/api/v1/sessions?status=creating,active,idle,ready")
            };
            let resp = authed_get(&client, list_url, token.as_deref())
                .send()
                .await
                .map_err(|e| friendly_error(&e, node))?;
            let text = ok_or_api_error(resp).await?;
            let sessions: Vec<Session> = serde_json::from_str(&text)?;
            Ok(format_sessions(&sessions))
        }
        Commands::Nodes => {
            let resp = authed_get(&client, format!("{url}/api/v1/peers"), token.as_deref())
                .send()
                .await
                .map_err(|e| friendly_error(&e, node))?;
            let text = ok_or_api_error(resp).await?;
            let resp: PeersResponse = serde_json::from_str(&text)?;
            Ok(format_nodes(&resp))
        }
        Commands::Spawn {
            workdir,
            name,
            ink,
            description,
            detach,
            idle_threshold,
            auto,
            worktree,
            worktree_base,
            runtime,
            secret,
            command,
        } => {
            let cmd = if command.is_empty() {
                None
            } else {
                Some(command.join(" "))
            };
            // Resolve workdir: --workdir flag > current directory
            let resolved_workdir = workdir.clone().unwrap_or_else(|| {
                std::env::current_dir()
                    .map_or_else(|_| ".".into(), |p| p.to_string_lossy().into_owned())
            });
            // Resolve name: explicit > derived from workdir (with dedup)
            let resolved_name = if let Some(n) = name {
                n.clone()
            } else {
                let base_name = derive_session_name(&resolved_workdir);
                deduplicate_session_name(&client, &url, &base_name, token.as_deref()).await
            };
            let mut body = serde_json::json!({
                "name": resolved_name,
                "workdir": resolved_workdir,
            });
            if let Some(c) = &cmd {
                body["command"] = serde_json::json!(c);
            }
            if let Some(i) = ink {
                body["ink"] = serde_json::json!(i);
            }
            if let Some(d) = description {
                body["description"] = serde_json::json!(d);
            }
            if let Some(t) = idle_threshold {
                body["idle_threshold_secs"] = serde_json::json!(t);
            }
            // --base-branch implies --worktree
            if *worktree || worktree_base.is_some() {
                body["worktree"] = serde_json::json!(true);
                if let Some(base) = worktree_base {
                    body["worktree_base"] = serde_json::json!(base);
                    eprintln!(
                        "Worktree: branch {resolved_name} (from {base}) in ~/.pulpo/worktrees/{resolved_name}/"
                    );
                } else {
                    eprintln!(
                        "Worktree: branch {resolved_name} in ~/.pulpo/worktrees/{resolved_name}/"
                    );
                }
            }
            if let Some(rt) = runtime {
                body["runtime"] = serde_json::json!(rt);
            }
            if !secret.is_empty() {
                body["secrets"] = serde_json::json!(secret);
            }
            let spawn_url = if *auto {
                let (auto_addr, auto_name) =
                    select_best_node(&client, &url, token.as_deref()).await?;
                eprintln!("Auto-selected node: {auto_name} ({auto_addr})");
                base_url(&auto_addr)
            } else {
                url.clone()
            };
            let resp = authed_post(
                &client,
                format!("{spawn_url}/api/v1/sessions"),
                token.as_deref(),
            )
            .json(&body)
            .send()
            .await
            .map_err(|e| friendly_error(&e, node))?;
            let text = ok_or_api_error(resp).await?;
            let resp: CreateSessionResponse = serde_json::from_str(&text)?;
            let msg = format!(
                "Created session \"{}\" ({})",
                resp.session.name, resp.session.id
            );
            if !detach {
                let backend_id = resp
                    .session
                    .backend_session_id
                    .as_deref()
                    .unwrap_or(&resp.session.name);
                eprintln!("{msg}");
                // Only check liveness for explicit commands — shell sessions (no command)
                // may be immediately marked idle/stopped by the watchdog, which is expected
                if cmd.is_some() {
                    let sid = resp.session.id.to_string();
                    check_session_alive(&client, &url, &sid, token.as_deref()).await?;
                }
                attach_session(backend_id)?;
                return Ok(format!("Detached from session \"{}\".", resp.session.name));
            }
            Ok(msg)
        }
        Commands::Stop { names, purge } => {
            let mut results = Vec::new();
            for name in names {
                let query = if *purge { "?purge=true" } else { "" };
                let resp = authed_post(
                    &client,
                    format!("{url}/api/v1/sessions/{name}/stop{query}"),
                    token.as_deref(),
                )
                .send()
                .await
                .map_err(|e| friendly_error(&e, node))?;
                let action = if *purge {
                    "stopped and purged"
                } else {
                    "stopped"
                };
                match ok_or_api_error(resp).await {
                    Ok(_) => results.push(format!("Session {name} {action}.")),
                    Err(e) => results.push(format!("Error stopping {name}: {e}")),
                }
            }
            Ok(results.join("\n"))
        }
        Commands::Cleanup => {
            let resp = authed_post(
                &client,
                format!("{url}/api/v1/sessions/cleanup"),
                token.as_deref(),
            )
            .send()
            .await
            .map_err(|e| friendly_error(&e, node))?;
            let text = ok_or_api_error(resp).await?;
            let result: serde_json::Value = serde_json::from_str(&text)?;
            let count = result["deleted"].as_u64().unwrap_or(0);
            if count == 0 {
                Ok("No stopped or lost sessions to clean up.".into())
            } else {
                Ok(format!("Cleaned up {count} session(s)."))
            }
        }
        Commands::Logs {
            name,
            lines,
            follow,
        } => {
            if *follow {
                let mut stdout = std::io::stdout();
                follow_logs(&client, &url, name, *lines, token.as_deref(), &mut stdout)
                    .await
                    .map_err(|e| {
                        // Unwrap reqwest errors to friendly messages
                        match e.downcast::<reqwest::Error>() {
                            Ok(re) => friendly_error(&re, node),
                            Err(other) => other,
                        }
                    })?;
                Ok(String::new())
            } else {
                let output = fetch_output(&client, &url, name, *lines, token.as_deref())
                    .await
                    .map_err(|e| match e.downcast::<reqwest::Error>() {
                        Ok(re) => friendly_error(&re, node),
                        Err(other) => other,
                    })?;
                Ok(output)
            }
        }
        Commands::Interventions { name } => {
            let resp = authed_get(
                &client,
                format!("{url}/api/v1/sessions/{name}/interventions"),
                token.as_deref(),
            )
            .send()
            .await
            .map_err(|e| friendly_error(&e, node))?;
            let text = ok_or_api_error(resp).await?;
            let events: Vec<InterventionEventResponse> = serde_json::from_str(&text)?;
            Ok(format_interventions(&events))
        }
        Commands::Ui => {
            let dashboard = base_url(node);
            open_browser(&dashboard)?;
            Ok(format!("Opening {dashboard}"))
        }
        Commands::Resume { name } => {
            let resp = authed_post(
                &client,
                format!("{url}/api/v1/sessions/{name}/resume"),
                token.as_deref(),
            )
            .send()
            .await
            .map_err(|e| friendly_error(&e, node))?;
            let text = ok_or_api_error(resp).await?;
            let session: Session = serde_json::from_str(&text)?;
            let backend_id = session
                .backend_session_id
                .as_deref()
                .unwrap_or(&session.name);
            eprintln!("Resumed session \"{}\"", session.name);
            let sid = session.id.to_string();
            check_session_alive(&client, &url, &sid, token.as_deref()).await?;
            attach_session(backend_id)?;
            Ok(format!("Detached from session \"{}\".", session.name))
        }
        Commands::Schedule { action } => execute_schedule(&client, action, &url, token.as_deref())
            .await
            .map_err(|e| match e.downcast::<reqwest::Error>() {
                Ok(re) => friendly_error(&re, node),
                Err(other) => other,
            }),
        Commands::Secret { action } => execute_secret(&client, action, &url, token.as_deref())
            .await
            .map_err(|e| match e.downcast::<reqwest::Error>() {
                Ok(re) => friendly_error(&re, node),
                Err(other) => other,
            }),
        Commands::Worktree { action } => execute_worktree(&client, action, &url, token.as_deref())
            .await
            .map_err(|e| match e.downcast::<reqwest::Error>() {
                Ok(re) => friendly_error(&re, node),
                Err(other) => other,
            }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_url() {
        assert_eq!(base_url("localhost:7433"), "http://localhost:7433");
        assert_eq!(base_url("my-machine:9999"), "http://my-machine:9999");
    }

    #[test]
    fn test_cli_parse_list() {
        let cli = Cli::try_parse_from(["pulpo", "list"]).unwrap();
        assert_eq!(cli.node, "localhost:7433");
        assert!(matches!(cli.command, Some(Commands::List { .. })));
    }

    #[test]
    fn test_cli_parse_nodes() {
        let cli = Cli::try_parse_from(["pulpo", "nodes"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Nodes)));
    }

    #[test]
    fn test_cli_parse_ui() {
        let cli = Cli::try_parse_from(["pulpo", "ui"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Ui)));
    }

    #[test]
    fn test_cli_parse_ui_custom_node() {
        let cli = Cli::try_parse_from(["pulpo", "--node", "mac-mini:7433", "ui"]).unwrap();
        // With args_conflicts_with_subcommands, "ui" is parsed as path when --node is explicit
        assert_eq!(cli.node, "mac-mini:7433");
        assert_eq!(cli.path.as_deref(), Some("ui"));
    }

    #[test]
    fn test_build_open_command() {
        let cmd = build_open_command("http://localhost:7433");
        let args: Vec<&std::ffi::OsStr> = cmd.get_args().collect();
        assert_eq!(args, vec!["http://localhost:7433"]);
        #[cfg(target_os = "macos")]
        assert_eq!(cmd.get_program(), "open");
        #[cfg(target_os = "linux")]
        assert_eq!(cmd.get_program(), "xdg-open");
    }

    #[test]
    fn test_cli_parse_spawn() {
        let cli = Cli::try_parse_from([
            "pulpo",
            "spawn",
            "my-task",
            "--workdir",
            "/tmp/repo",
            "--",
            "claude",
            "-p",
            "Fix the bug",
        ])
        .unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Spawn { name, workdir, command, .. })
                if name.as_deref() == Some("my-task") && workdir.as_deref() == Some("/tmp/repo")
                && command == &["claude", "-p", "Fix the bug"]
        ));
    }

    #[test]
    fn test_cli_parse_spawn_with_ink() {
        let cli = Cli::try_parse_from(["pulpo", "spawn", "my-task", "--ink", "coder"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Spawn { ink, .. }) if ink.as_deref() == Some("coder")
        ));
    }

    #[test]
    fn test_cli_parse_spawn_with_description() {
        let cli =
            Cli::try_parse_from(["pulpo", "spawn", "my-task", "--description", "Fix the bug"])
                .unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Spawn { description, .. }) if description.as_deref() == Some("Fix the bug")
        ));
    }

    #[test]
    fn test_cli_parse_spawn_name_positional() {
        let cli = Cli::try_parse_from(["pulpo", "spawn", "portal", "--", "echo", "hello"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Spawn { name, command, .. })
                if name.as_deref() == Some("portal") && command == &["echo", "hello"]
        ));
    }

    #[test]
    fn test_cli_parse_spawn_no_command() {
        let cli = Cli::try_parse_from(["pulpo", "spawn", "my-task"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Spawn { command, .. }) if command.is_empty()
        ));
    }

    #[test]
    fn test_cli_parse_spawn_idle_threshold() {
        let cli =
            Cli::try_parse_from(["pulpo", "spawn", "my-task", "--idle-threshold", "0"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Spawn { idle_threshold, .. }) if *idle_threshold == Some(0)
        ));
    }

    #[test]
    fn test_cli_parse_spawn_idle_threshold_60() {
        let cli =
            Cli::try_parse_from(["pulpo", "spawn", "my-task", "--idle-threshold", "60"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Spawn { idle_threshold, .. }) if *idle_threshold == Some(60)
        ));
    }

    #[test]
    fn test_cli_parse_spawn_secrets() {
        let cli = Cli::try_parse_from([
            "pulpo",
            "spawn",
            "my-task",
            "--secret",
            "GITHUB_TOKEN",
            "--secret",
            "NPM_TOKEN",
        ])
        .unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Spawn { secret, .. }) if secret == &["GITHUB_TOKEN", "NPM_TOKEN"]
        ));
    }

    #[test]
    fn test_cli_parse_spawn_no_secrets() {
        let cli = Cli::try_parse_from(["pulpo", "spawn", "my-task"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Spawn { secret, .. }) if secret.is_empty()
        ));
    }

    #[test]
    fn test_cli_parse_secret_set_with_env() {
        let cli = Cli::try_parse_from([
            "pulpo",
            "secret",
            "set",
            "GH_WORK",
            "token123",
            "--env",
            "GITHUB_TOKEN",
        ])
        .unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Secret { action: SecretAction::Set { name, value, env } })
                if name == "GH_WORK" && value == "token123" && env.as_deref() == Some("GITHUB_TOKEN")
        ));
    }

    #[test]
    fn test_cli_parse_secret_set_without_env() {
        let cli = Cli::try_parse_from(["pulpo", "secret", "set", "MY_KEY", "val"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Secret { action: SecretAction::Set { name, value, env } })
                if name == "MY_KEY" && value == "val" && env.is_none()
        ));
    }

    #[test]
    fn test_cli_parse_spawn_worktree() {
        let cli = Cli::try_parse_from(["pulpo", "spawn", "my-task", "--worktree"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Spawn { worktree, .. }) if *worktree
        ));
    }

    #[test]
    fn test_cli_parse_spawn_worktree_base() {
        let cli = Cli::try_parse_from([
            "pulpo",
            "spawn",
            "my-task",
            "--worktree",
            "--worktree-base",
            "main",
        ])
        .unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Spawn { worktree, worktree_base, .. })
                if *worktree && worktree_base.as_deref() == Some("main")
        ));
    }

    #[test]
    fn test_cli_parse_spawn_worktree_base_implies_worktree() {
        // --worktree-base without --worktree should still parse (implied at execute time)
        let cli = Cli::try_parse_from(["pulpo", "spawn", "my-task", "--worktree-base", "develop"])
            .unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Spawn { worktree_base, .. })
                if worktree_base.as_deref() == Some("develop")
        ));
    }

    #[test]
    fn test_cli_parse_worktree_list() {
        let cli = Cli::try_parse_from(["pulpo", "worktree", "list"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Worktree {
                action: WorktreeAction::List
            })
        ));
    }

    #[test]
    fn test_cli_parse_wt_alias() {
        let cli = Cli::try_parse_from(["pulpo", "wt", "list"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Worktree {
                action: WorktreeAction::List
            })
        ));
    }

    #[test]
    fn test_cli_parse_worktree_list_ls_alias() {
        let cli = Cli::try_parse_from(["pulpo", "wt", "ls"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Worktree {
                action: WorktreeAction::List
            })
        ));
    }

    #[test]
    fn test_format_worktree_sessions_empty() {
        let output = format_worktree_sessions(&[]);
        assert_eq!(output, "No worktree sessions.");
    }

    #[test]
    fn test_format_worktree_sessions_with_data() {
        use chrono::Utc;
        use pulpo_common::session::SessionStatus;
        use uuid::Uuid;

        let session = Session {
            id: Uuid::nil(),
            name: "fix-auth".into(),
            workdir: "/tmp/repo".into(),
            command: "claude -p 'fix auth'".into(),
            description: None,
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
            idle_threshold_secs: None,
            worktree_path: Some("/home/user/.pulpo/worktrees/fix-auth".into()),
            worktree_branch: Some("fix-auth".into()),
            runtime: Runtime::Tmux,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let sessions = vec![&session];
        let output = format_worktree_sessions(&sessions);
        assert!(output.contains("fix-auth"), "should show name: {output}");
        assert!(output.contains("active"), "should show status: {output}");
        assert!(
            output.contains("/home/user/.pulpo/worktrees/fix-auth"),
            "should show path: {output}"
        );
        assert!(output.contains("BRANCH"), "should have header: {output}");
    }

    #[test]
    fn test_format_worktree_sessions_no_branch() {
        use chrono::Utc;
        use pulpo_common::session::SessionStatus;
        use uuid::Uuid;

        let session = Session {
            id: Uuid::nil(),
            name: "old-session".into(),
            workdir: "/tmp".into(),
            command: "echo".into(),
            description: None,
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
            idle_threshold_secs: None,
            worktree_path: Some("/home/user/.pulpo/worktrees/old-session".into()),
            worktree_branch: None,
            runtime: Runtime::Tmux,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let sessions = vec![&session];
        let output = format_worktree_sessions(&sessions);
        assert!(
            output.contains('-'),
            "branch should show dash when None: {output}"
        );
    }

    #[test]
    fn test_cli_parse_spawn_detach() {
        let cli = Cli::try_parse_from(["pulpo", "spawn", "my-task", "--detach"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Spawn { detach, .. }) if *detach
        ));
    }

    #[test]
    fn test_cli_parse_spawn_detach_short() {
        let cli = Cli::try_parse_from(["pulpo", "spawn", "my-task", "-d"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Spawn { detach, .. }) if *detach
        ));
    }

    #[test]
    fn test_cli_parse_spawn_detach_default() {
        let cli = Cli::try_parse_from(["pulpo", "spawn", "my-task"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Spawn { detach, .. }) if !detach
        ));
    }

    #[test]
    fn test_cli_parse_logs() {
        let cli = Cli::try_parse_from(["pulpo", "logs", "my-session"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Logs { name, lines, follow }) if name == "my-session" && *lines == 100 && !follow
        ));
    }

    #[test]
    fn test_cli_parse_logs_with_lines() {
        let cli = Cli::try_parse_from(["pulpo", "logs", "my-session", "--lines", "50"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Logs { name, lines, follow }) if name == "my-session" && *lines == 50 && !follow
        ));
    }

    #[test]
    fn test_cli_parse_logs_follow() {
        let cli = Cli::try_parse_from(["pulpo", "logs", "my-session", "--follow"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Logs { name, follow, .. }) if name == "my-session" && *follow
        ));
    }

    #[test]
    fn test_cli_parse_logs_follow_short() {
        let cli = Cli::try_parse_from(["pulpo", "logs", "my-session", "-f"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Logs { name, follow, .. }) if name == "my-session" && *follow
        ));
    }

    #[test]
    fn test_cli_parse_stop() {
        let cli = Cli::try_parse_from(["pulpo", "stop", "my-session"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Stop { names, purge }) if names == &["my-session"] && !purge
        ));
    }

    #[test]
    fn test_cli_parse_stop_purge() {
        let cli = Cli::try_parse_from(["pulpo", "stop", "my-session", "--purge"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Stop { names, purge }) if names == &["my-session"] && *purge
        ));

        let cli = Cli::try_parse_from(["pulpo", "stop", "my-session", "-p"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Stop { names, purge }) if names == &["my-session"] && *purge
        ));
    }

    #[test]
    fn test_cli_parse_kill_alias() {
        let cli = Cli::try_parse_from(["pulpo", "kill", "my-session"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Stop { names, purge }) if names == &["my-session"] && !purge
        ));
    }

    #[test]
    fn test_cli_parse_resume() {
        let cli = Cli::try_parse_from(["pulpo", "resume", "my-session"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Resume { name }) if name == "my-session"
        ));
    }

    #[test]
    fn test_cli_parse_input() {
        let cli = Cli::try_parse_from(["pulpo", "input", "my-session", "yes"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Input { name, text }) if name == "my-session" && text.as_deref() == Some("yes")
        ));
    }

    #[test]
    fn test_cli_parse_input_no_text() {
        let cli = Cli::try_parse_from(["pulpo", "input", "my-session"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Input { name, text }) if name == "my-session" && text.is_none()
        ));
    }

    #[test]
    fn test_cli_parse_input_alias() {
        let cli = Cli::try_parse_from(["pulpo", "i", "my-session", "y"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Input { name, text }) if name == "my-session" && text.as_deref() == Some("y")
        ));
    }

    #[test]
    fn test_cli_parse_custom_node() {
        let cli = Cli::try_parse_from(["pulpo", "--node", "win-pc:8080", "list"]).unwrap();
        assert_eq!(cli.node, "win-pc:8080");
        // With args_conflicts_with_subcommands, "list" is parsed as path when --node is explicit
        assert_eq!(cli.path.as_deref(), Some("list"));
    }

    #[test]
    fn test_cli_version() {
        let result = Cli::try_parse_from(["pulpo", "--version"]);
        // clap exits with an error (kind DisplayVersion) when --version is used
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn test_cli_parse_no_subcommand_succeeds() {
        let cli = Cli::try_parse_from(["pulpo"]).unwrap();
        assert!(cli.command.is_none());
        assert!(cli.path.is_none());
    }

    #[test]
    fn test_cli_debug() {
        let cli = Cli::try_parse_from(["pulpo", "list"]).unwrap();
        let debug = format!("{cli:?}");
        assert!(debug.contains("List"));
    }

    #[test]
    fn test_commands_debug() {
        let cmd = Commands::List { all: false };
        assert_eq!(format!("{cmd:?}"), "List { all: false }");
    }

    /// A valid Session JSON for test responses.
    const TEST_SESSION_JSON: &str = r#"{"id":"00000000-0000-0000-0000-000000000001","name":"repo","workdir":"/tmp/repo","command":"claude -p 'Fix bug'","description":null,"status":"active","exit_code":null,"backend_session_id":null,"output_snapshot":null,"metadata":null,"ink":null,"intervention_code":null,"intervention_reason":null,"intervention_at":null,"last_output_at":null,"idle_since":null,"idle_threshold_secs":null,"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}"#;

    /// A valid `CreateSessionResponse` JSON wrapping the session.
    fn test_create_response_json() -> String {
        format!(r#"{{"session":{TEST_SESSION_JSON}}}"#)
    }

    /// Start a lightweight test HTTP server and return its address.
    async fn start_test_server() -> String {
        use axum::http::StatusCode;
        use axum::{
            Json, Router,
            routing::{get, post},
        };

        let create_json = test_create_response_json();

        let app = Router::new()
            .route(
                "/api/v1/sessions",
                get(|| async { Json::<Vec<()>>(vec![]) }).post(move || async move {
                    (StatusCode::CREATED, create_json.clone())
                }),
            )
            .route(
                "/api/v1/sessions/{id}",
                get(|| async { TEST_SESSION_JSON.to_owned() }),
            )
            .route(
                "/api/v1/sessions/{id}/stop",
                post(|| async { StatusCode::NO_CONTENT }),
            )
            .route(
                "/api/v1/sessions/{id}/output",
                get(|| async { r#"{"output":"test output"}"#.to_owned() }),
            )
            .route(
                "/api/v1/peers",
                get(|| async {
                    r#"{"local":{"name":"test","hostname":"h","os":"macos","arch":"arm64","cpus":8,"memory_mb":0,"gpu":null},"peers":[]}"#.to_owned()
                }),
            )
            .route(
                "/api/v1/sessions/{id}/resume",
                axum::routing::post(|| async { TEST_SESSION_JSON.to_owned() }),
            )
            .route(
                "/api/v1/sessions/{id}/interventions",
                get(|| async { "[]".to_owned() }),
            )
            .route(
                "/api/v1/sessions/{id}/input",
                post(|| async { StatusCode::NO_CONTENT }),
            )
            .route(
                "/api/v1/schedules",
                get(|| async { Json::<Vec<()>>(vec![]) })
                    .post(|| async { StatusCode::CREATED }),
            )
            .route(
                "/api/v1/schedules/{id}",
                axum::routing::put(|| async { StatusCode::OK })
                    .delete(|| async { StatusCode::NO_CONTENT }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        format!("127.0.0.1:{}", addr.port())
    }

    #[tokio::test]
    async fn test_execute_list_success() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::List { all: false }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert_eq!(result, "No sessions.");
    }

    #[tokio::test]
    async fn test_execute_nodes_success() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Nodes),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("test"));
        assert!(result.contains("(local)"));
        assert!(result.contains("NAME"));
    }

    #[tokio::test]
    async fn test_execute_spawn_success() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Spawn {
                name: Some("test".into()),
                workdir: Some("/tmp/repo".into()),
                ink: None,
                description: None,
                detach: true,
                idle_threshold: None,
                auto: false,
                worktree: false,
                worktree_base: None,
                runtime: None,
                secret: vec![],
                command: vec!["claude".into(), "-p".into(), "Fix bug".into()],
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("Created session"));
        assert!(result.contains("repo"));
    }

    #[tokio::test]
    async fn test_execute_spawn_with_all_flags() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Spawn {
                name: Some("test".into()),
                workdir: Some("/tmp/repo".into()),
                ink: Some("coder".into()),
                description: Some("Fix the bug".into()),
                detach: true,
                idle_threshold: None,
                auto: false,
                worktree: false,
                worktree_base: None,
                runtime: None,
                secret: vec![],
                command: vec!["claude".into(), "-p".into(), "Fix bug".into()],
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("Created session"));
    }

    #[tokio::test]
    async fn test_execute_spawn_with_idle_threshold_and_worktree_and_docker_runtime() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Spawn {
                name: Some("full-opts".into()),
                workdir: Some("/tmp/repo".into()),
                ink: Some("coder".into()),
                description: Some("Full options".into()),
                detach: true,
                idle_threshold: Some(120),
                auto: false,
                worktree: true,
                worktree_base: None,
                runtime: Some("docker".into()),
                secret: vec![],
                command: vec!["claude".into()],
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("Created session"));
    }

    #[tokio::test]
    async fn test_execute_spawn_no_name_derives_from_workdir() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Spawn {
                name: None,
                workdir: Some("/tmp/my-project".into()),
                ink: None,
                description: None,
                detach: true,
                idle_threshold: None,
                auto: false,
                worktree: false,
                worktree_base: None,
                runtime: None,
                secret: vec![],
                command: vec!["echo".into(), "hello".into()],
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("Created session"));
    }

    #[tokio::test]
    async fn test_execute_spawn_no_command() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Spawn {
                name: Some("test".into()),
                workdir: Some("/tmp/repo".into()),
                ink: None,
                description: None,
                detach: true,
                idle_threshold: None,
                auto: false,
                worktree: false,
                worktree_base: None,
                runtime: None,
                secret: vec![],
                command: vec![],
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("Created session"));
    }

    #[tokio::test]
    async fn test_execute_spawn_with_name() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Spawn {
                name: Some("my-task".into()),
                workdir: Some("/tmp/repo".into()),
                ink: None,
                description: None,
                detach: true,
                idle_threshold: None,
                auto: false,
                worktree: false,
                worktree_base: None,
                runtime: None,
                secret: vec![],
                command: vec!["claude".into(), "-p".into(), "Fix bug".into()],
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("Created session"));
    }

    #[tokio::test]
    async fn test_execute_spawn_auto_attach() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Spawn {
                name: Some("test".into()),
                workdir: Some("/tmp/repo".into()),
                ink: None,
                description: None,
                detach: false,
                idle_threshold: None,
                auto: false,
                worktree: false,
                worktree_base: None,
                runtime: None,
                secret: vec![],
                command: vec!["claude".into(), "-p".into(), "Fix bug".into()],
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        // When not detached, spawn prints creation to stderr and returns detach message
        assert!(result.contains("Detached from session"));
    }

    #[tokio::test]
    async fn test_execute_stop_success() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Stop {
                names: vec!["test-session".into()],
                purge: false,
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("stopped"));
        assert!(!result.contains("purged"));
    }

    #[tokio::test]
    async fn test_execute_stop_with_purge() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Stop {
                names: vec!["test-session".into()],
                purge: true,
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("stopped and purged"));
    }

    #[tokio::test]
    async fn test_execute_logs_success() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Logs {
                name: "test-session".into(),
                lines: 50,
                follow: false,
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("test output"));
    }

    #[tokio::test]
    async fn test_execute_list_connection_refused() {
        let cli = Cli {
            node: "localhost:1".into(),
            token: None,
            command: Some(Commands::List { all: false }),
            path: None,
        };
        let result = execute(&cli).await;
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Could not connect to pulpod"),
            "Expected friendly error, got: {err}"
        );
        assert!(err.contains("localhost:1"));
    }

    #[tokio::test]
    async fn test_execute_nodes_connection_refused() {
        let cli = Cli {
            node: "localhost:1".into(),
            token: None,
            command: Some(Commands::Nodes),
            path: None,
        };
        let result = execute(&cli).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Could not connect to pulpod"));
    }

    #[tokio::test]
    async fn test_execute_stop_error_response() {
        use axum::{Router, http::StatusCode, routing::post};

        let app = Router::new().route(
            "/api/v1/sessions/{id}/stop",
            post(|| async {
                (
                    StatusCode::NOT_FOUND,
                    "{\"error\":\"session not found: test-session\"}",
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let node = format!("127.0.0.1:{}", addr.port());

        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Stop {
                names: vec!["test-session".into()],
                purge: false,
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("Error stopping test-session"), "{result}");
    }

    #[tokio::test]
    async fn test_execute_logs_error_response() {
        use axum::{Router, http::StatusCode, routing::get};

        let app = Router::new().route(
            "/api/v1/sessions/{id}/output",
            get(|| async {
                (
                    StatusCode::NOT_FOUND,
                    "{\"error\":\"session not found: ghost\"}",
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let node = format!("127.0.0.1:{}", addr.port());

        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Logs {
                name: "ghost".into(),
                lines: 50,
                follow: false,
            }),
            path: None,
        };
        let err = execute(&cli).await.unwrap_err();
        assert_eq!(err.to_string(), "session not found: ghost");
    }

    #[tokio::test]
    async fn test_execute_resume_error_response() {
        use axum::{Router, http::StatusCode, routing::post};

        let app = Router::new().route(
            "/api/v1/sessions/{id}/resume",
            post(|| async {
                (
                    StatusCode::BAD_REQUEST,
                    "{\"error\":\"session is not lost (status: active)\"}",
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let node = format!("127.0.0.1:{}", addr.port());

        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Resume {
                name: "test-session".into(),
            }),
            path: None,
        };
        let err = execute(&cli).await.unwrap_err();
        assert_eq!(err.to_string(), "session is not lost (status: active)");
    }

    #[tokio::test]
    async fn test_execute_spawn_error_response() {
        use axum::{Router, http::StatusCode, routing::post};

        let app = Router::new().route(
            "/api/v1/sessions",
            post(|| async {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "{\"error\":\"failed to spawn session\"}",
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let node = format!("127.0.0.1:{}", addr.port());

        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Spawn {
                name: Some("test".into()),
                workdir: Some("/tmp/repo".into()),
                ink: None,
                description: None,
                detach: true,
                idle_threshold: None,
                auto: false,
                worktree: false,
                worktree_base: None,
                runtime: None,
                secret: vec![],
                command: vec!["test".into()],
            }),
            path: None,
        };
        let err = execute(&cli).await.unwrap_err();
        assert_eq!(err.to_string(), "failed to spawn session");
    }

    #[tokio::test]
    async fn test_execute_interventions_error_response() {
        use axum::{Router, http::StatusCode, routing::get};

        let app = Router::new().route(
            "/api/v1/sessions/{id}/interventions",
            get(|| async {
                (
                    StatusCode::NOT_FOUND,
                    "{\"error\":\"session not found: ghost\"}",
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let node = format!("127.0.0.1:{}", addr.port());

        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Interventions {
                name: "ghost".into(),
            }),
            path: None,
        };
        let err = execute(&cli).await.unwrap_err();
        assert_eq!(err.to_string(), "session not found: ghost");
    }

    #[tokio::test]
    async fn test_execute_resume_success() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Resume {
                name: "test-session".into(),
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("Detached from session"));
    }

    #[tokio::test]
    async fn test_execute_input_success() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Input {
                name: "test-session".into(),
                text: Some("yes".into()),
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("Sent input to session test-session"));
    }

    #[tokio::test]
    async fn test_execute_input_no_text() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Input {
                name: "test-session".into(),
                text: None,
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("Sent input to session test-session"));
    }

    #[tokio::test]
    async fn test_execute_input_connection_refused() {
        let cli = Cli {
            node: "localhost:1".into(),
            token: None,
            command: Some(Commands::Input {
                name: "test".into(),
                text: Some("y".into()),
            }),
            path: None,
        };
        let result = execute(&cli).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Could not connect to pulpod"));
    }

    #[tokio::test]
    async fn test_execute_input_error_response() {
        use axum::{Router, http::StatusCode, routing::post};

        let app = Router::new().route(
            "/api/v1/sessions/{id}/input",
            post(|| async {
                (
                    StatusCode::NOT_FOUND,
                    "{\"error\":\"session not found: ghost\"}",
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let node = format!("127.0.0.1:{}", addr.port());

        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Input {
                name: "ghost".into(),
                text: Some("y".into()),
            }),
            path: None,
        };
        let err = execute(&cli).await.unwrap_err();
        assert_eq!(err.to_string(), "session not found: ghost");
    }

    #[tokio::test]
    async fn test_execute_ui() {
        let cli = Cli {
            node: "localhost:7433".into(),
            token: None,
            command: Some(Commands::Ui),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("Opening"));
        assert!(result.contains("http://localhost:7433"));
    }

    #[tokio::test]
    async fn test_execute_ui_custom_node() {
        let cli = Cli {
            node: "mac-mini:7433".into(),
            token: None,
            command: Some(Commands::Ui),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("http://mac-mini:7433"));
    }

    #[test]
    fn test_format_sessions_empty() {
        assert_eq!(format_sessions(&[]), "No sessions.");
    }

    #[test]
    fn test_format_sessions_with_data() {
        use chrono::Utc;
        use pulpo_common::session::SessionStatus;
        use uuid::Uuid;

        let sessions = vec![Session {
            id: Uuid::nil(),
            name: "my-api".into(),
            workdir: "/tmp/repo".into(),
            command: "claude -p 'Fix the bug'".into(),
            description: Some("Fix the bug".into()),
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
            idle_threshold_secs: None,
            worktree_path: None,
            worktree_branch: None,
            runtime: Runtime::Tmux,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }];
        let output = format_sessions(&sessions);
        assert!(output.contains("ID"));
        assert!(output.contains("NAME"));
        assert!(output.contains("RUNTIME"));
        assert!(output.contains("COMMAND"));
        assert!(output.contains("00000000"));
        assert!(output.contains("my-api"));
        assert!(output.contains("active"));
        assert!(output.contains("tmux"));
        assert!(output.contains("claude -p 'Fix the bug'"));
    }

    #[test]
    fn test_format_sessions_docker_runtime() {
        use chrono::Utc;
        use pulpo_common::session::SessionStatus;
        use uuid::Uuid;

        let sessions = vec![Session {
            id: Uuid::nil(),
            name: "sandbox-test".into(),
            workdir: "/tmp".into(),
            command: "claude".into(),
            description: None,
            status: SessionStatus::Active,
            exit_code: None,
            backend_session_id: Some("docker:pulpo-sandbox-test".into()),
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
            runtime: Runtime::Docker,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }];
        let output = format_sessions(&sessions);
        assert!(output.contains("docker"));
    }

    #[test]
    fn test_format_sessions_long_command_truncated() {
        use chrono::Utc;
        use pulpo_common::session::SessionStatus;
        use uuid::Uuid;

        let sessions = vec![Session {
            id: Uuid::nil(),
            name: "test".into(),
            workdir: "/tmp".into(),
            command:
                "claude -p 'A very long command that exceeds fifty characters in total length here'"
                    .into(),
            description: None,
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
            idle_threshold_secs: None,
            worktree_path: None,
            worktree_branch: None,
            runtime: Runtime::Tmux,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }];
        let output = format_sessions(&sessions);
        assert!(output.contains("..."));
    }

    #[test]
    fn test_format_sessions_worktree_indicator() {
        use chrono::Utc;
        use pulpo_common::session::SessionStatus;
        use uuid::Uuid;

        let sessions = vec![Session {
            id: Uuid::nil(),
            name: "wt-task".into(),
            workdir: "/repo".into(),
            command: "claude".into(),
            description: None,
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
            idle_threshold_secs: None,
            worktree_path: Some("/home/user/.pulpo/worktrees/wt-task".into()),
            worktree_branch: Some("wt-task".into()),
            runtime: Runtime::Tmux,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }];
        let output = format_sessions(&sessions);
        assert!(
            output.contains("[wt]"),
            "should show worktree indicator: {output}"
        );
        assert!(output.contains("wt-task [wt]"));
    }

    #[test]
    fn test_format_sessions_pr_indicator() {
        use chrono::Utc;
        use pulpo_common::session::SessionStatus;
        use std::collections::HashMap;
        use uuid::Uuid;

        let mut meta = HashMap::new();
        meta.insert("pr_url".into(), "https://github.com/a/b/pull/1".into());
        let sessions = vec![Session {
            id: Uuid::nil(),
            name: "pr-task".into(),
            workdir: "/tmp".into(),
            command: "claude".into(),
            description: None,
            status: SessionStatus::Active,
            exit_code: None,
            backend_session_id: None,
            output_snapshot: None,
            metadata: Some(meta),
            ink: None,
            intervention_code: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            idle_threshold_secs: None,
            worktree_path: None,
            worktree_branch: None,
            runtime: Runtime::Tmux,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }];
        let output = format_sessions(&sessions);
        assert!(
            output.contains("[PR]"),
            "should show PR indicator: {output}"
        );
        assert!(output.contains("pr-task [PR]"));
    }

    #[test]
    fn test_format_sessions_worktree_and_pr_indicator() {
        use chrono::Utc;
        use pulpo_common::session::SessionStatus;
        use std::collections::HashMap;
        use uuid::Uuid;

        let mut meta = HashMap::new();
        meta.insert("pr_url".into(), "https://github.com/a/b/pull/1".into());
        let sessions = vec![Session {
            id: Uuid::nil(),
            name: "both-task".into(),
            workdir: "/tmp".into(),
            command: "claude".into(),
            description: None,
            status: SessionStatus::Active,
            exit_code: None,
            backend_session_id: None,
            output_snapshot: None,
            metadata: Some(meta),
            ink: None,
            intervention_code: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            idle_threshold_secs: None,
            worktree_path: Some("/home/user/.pulpo/worktrees/both-task".into()),
            worktree_branch: Some("both-task".into()),
            runtime: Runtime::Tmux,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }];
        let output = format_sessions(&sessions);
        assert!(
            output.contains("[wt] [PR]"),
            "should show both indicators: {output}"
        );
    }

    #[test]
    fn test_format_sessions_no_pr_without_metadata() {
        use chrono::Utc;
        use pulpo_common::session::SessionStatus;
        use uuid::Uuid;

        let sessions = vec![Session {
            id: Uuid::nil(),
            name: "no-pr".into(),
            workdir: "/tmp".into(),
            command: "claude".into(),
            description: None,
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
            idle_threshold_secs: None,
            worktree_path: None,
            worktree_branch: None,
            runtime: Runtime::Tmux,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }];
        let output = format_sessions(&sessions);
        assert!(
            !output.contains("[PR]"),
            "should not show PR indicator: {output}"
        );
    }

    #[test]
    fn test_format_nodes() {
        use pulpo_common::node::NodeInfo;
        use pulpo_common::peer::{PeerInfo, PeerSource, PeerStatus};

        let resp = PeersResponse {
            local: NodeInfo {
                name: "mac-mini".into(),
                hostname: "h".into(),
                os: "macos".into(),
                arch: "arm64".into(),
                cpus: 8,
                memory_mb: 16384,
                gpu: None,
            },
            peers: vec![PeerInfo {
                name: "win-pc".into(),
                address: "win-pc:7433".into(),
                status: PeerStatus::Online,
                node_info: None,
                session_count: Some(3),
                source: PeerSource::Configured,
            }],
        };
        let output = format_nodes(&resp);
        assert!(output.contains("mac-mini"));
        assert!(output.contains("(local)"));
        assert!(output.contains("win-pc"));
        assert!(output.contains('3'));
    }

    #[test]
    fn test_format_nodes_no_session_count() {
        use pulpo_common::node::NodeInfo;
        use pulpo_common::peer::{PeerInfo, PeerSource, PeerStatus};

        let resp = PeersResponse {
            local: NodeInfo {
                name: "local".into(),
                hostname: "h".into(),
                os: "linux".into(),
                arch: "x86_64".into(),
                cpus: 4,
                memory_mb: 8192,
                gpu: None,
            },
            peers: vec![PeerInfo {
                name: "peer".into(),
                address: "peer:7433".into(),
                status: PeerStatus::Offline,
                node_info: None,
                session_count: None,
                source: PeerSource::Configured,
            }],
        };
        let output = format_nodes(&resp);
        assert!(output.contains("offline"));
        // No session count → shows "-"
        let lines: Vec<&str> = output.lines().collect();
        assert!(lines[2].contains('-'));
    }

    #[tokio::test]
    async fn test_execute_resume_connection_refused() {
        let cli = Cli {
            node: "localhost:1".into(),
            token: None,
            command: Some(Commands::Resume {
                name: "test".into(),
            }),
            path: None,
        };
        let result = execute(&cli).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Could not connect to pulpod"));
    }

    #[tokio::test]
    async fn test_execute_spawn_connection_refused() {
        let cli = Cli {
            node: "localhost:1".into(),
            token: None,
            command: Some(Commands::Spawn {
                name: Some("test".into()),
                workdir: Some("/tmp".into()),
                ink: None,
                description: None,
                detach: true,
                idle_threshold: None,
                auto: false,
                worktree: false,
                worktree_base: None,
                runtime: None,
                secret: vec![],
                command: vec!["test".into()],
            }),
            path: None,
        };
        let result = execute(&cli).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Could not connect to pulpod"));
    }

    #[tokio::test]
    async fn test_execute_stop_connection_refused() {
        let cli = Cli {
            node: "localhost:1".into(),
            token: None,
            command: Some(Commands::Stop {
                names: vec!["test".into()],
                purge: false,
            }),
            path: None,
        };
        let result = execute(&cli).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Could not connect to pulpod"));
    }

    #[tokio::test]
    async fn test_execute_logs_connection_refused() {
        let cli = Cli {
            node: "localhost:1".into(),
            token: None,
            command: Some(Commands::Logs {
                name: "test".into(),
                lines: 50,
                follow: false,
            }),
            path: None,
        };
        let result = execute(&cli).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Could not connect to pulpod"));
    }

    #[tokio::test]
    async fn test_friendly_error_connect() {
        // Make a request to a closed port to get a connect error
        let err = reqwest::Client::new()
            .get("http://127.0.0.1:1")
            .send()
            .await
            .unwrap_err();
        let friendly = friendly_error(&err, "test-node:1");
        let msg = friendly.to_string();
        assert!(
            msg.contains("Could not connect"),
            "Expected connect message, got: {msg}"
        );
    }

    #[tokio::test]
    async fn test_friendly_error_other() {
        // A request to an invalid URL creates a builder error, not a connect error
        let err = reqwest::Client::new()
            .get("http://[::invalid::url")
            .send()
            .await
            .unwrap_err();
        let friendly = friendly_error(&err, "bad-host");
        let msg = friendly.to_string();
        assert!(
            msg.contains("Network error"),
            "Expected network error message, got: {msg}"
        );
        assert!(msg.contains("bad-host"));
    }

    // -- Auth helper tests --

    #[test]
    fn test_is_localhost_variants() {
        assert!(is_localhost("localhost:7433"));
        assert!(is_localhost("127.0.0.1:7433"));
        assert!(is_localhost("[::1]:7433"));
        assert!(is_localhost("::1"));
        assert!(is_localhost("localhost"));
        assert!(!is_localhost("mac-mini:7433"));
        assert!(!is_localhost("192.168.1.100:7433"));
    }

    #[test]
    fn test_authed_get_with_token() {
        let client = reqwest::Client::new();
        let req = authed_get(&client, "http://h:1/api".into(), Some("tok"))
            .build()
            .unwrap();
        let auth = req
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(auth, "Bearer tok");
    }

    #[test]
    fn test_authed_get_without_token() {
        let client = reqwest::Client::new();
        let req = authed_get(&client, "http://h:1/api".into(), None)
            .build()
            .unwrap();
        assert!(req.headers().get("authorization").is_none());
    }

    #[test]
    fn test_authed_post_with_token() {
        let client = reqwest::Client::new();
        let req = authed_post(&client, "http://h:1/api".into(), Some("secret"))
            .build()
            .unwrap();
        let auth = req
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(auth, "Bearer secret");
    }

    #[test]
    fn test_authed_post_without_token() {
        let client = reqwest::Client::new();
        let req = authed_post(&client, "http://h:1/api".into(), None)
            .build()
            .unwrap();
        assert!(req.headers().get("authorization").is_none());
    }

    #[test]
    fn test_authed_delete_with_token() {
        let client = reqwest::Client::new();
        let req = authed_delete(&client, "http://h:1/api".into(), Some("del-tok"))
            .build()
            .unwrap();
        let auth = req
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(auth, "Bearer del-tok");
    }

    #[test]
    fn test_authed_delete_without_token() {
        let client = reqwest::Client::new();
        let req = authed_delete(&client, "http://h:1/api".into(), None)
            .build()
            .unwrap();
        assert!(req.headers().get("authorization").is_none());
    }

    #[tokio::test]
    async fn test_resolve_token_explicit() {
        let client = reqwest::Client::new();
        let token =
            resolve_token(&client, "http://localhost:1", "localhost:1", Some("my-tok")).await;
        assert_eq!(token, Some("my-tok".into()));
    }

    #[tokio::test]
    async fn test_resolve_token_remote_no_explicit() {
        let client = reqwest::Client::new();
        let token = resolve_token(&client, "http://remote:7433", "remote:7433", None).await;
        assert_eq!(token, None);
    }

    #[tokio::test]
    async fn test_resolve_token_localhost_auto_discover() {
        use axum::{Json, Router, routing::get};

        let app = Router::new().route(
            "/api/v1/auth/token",
            get(|| async {
                Json(AuthTokenResponse {
                    token: "discovered".into(),
                })
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

        let node = format!("localhost:{}", addr.port());
        let base = base_url(&node);
        let client = reqwest::Client::new();
        let token = resolve_token(&client, &base, &node, None).await;
        assert_eq!(token, Some("discovered".into()));
    }

    #[tokio::test]
    async fn test_discover_token_empty_returns_none() {
        use axum::{Json, Router, routing::get};

        let app = Router::new().route(
            "/api/v1/auth/token",
            get(|| async {
                Json(AuthTokenResponse {
                    token: String::new(),
                })
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

        let base = format!("http://127.0.0.1:{}", addr.port());
        let client = reqwest::Client::new();
        assert_eq!(discover_token(&client, &base).await, None);
    }

    #[tokio::test]
    async fn test_discover_token_unreachable_returns_none() {
        let client = reqwest::Client::new();
        assert_eq!(discover_token(&client, "http://127.0.0.1:1").await, None);
    }

    #[test]
    fn test_cli_parse_with_token() {
        let cli = Cli::try_parse_from(["pulpo", "--token", "my-secret", "list"]).unwrap();
        assert_eq!(cli.token, Some("my-secret".into()));
    }

    #[test]
    fn test_cli_parse_without_token() {
        let cli = Cli::try_parse_from(["pulpo", "list"]).unwrap();
        assert_eq!(cli.token, None);
    }

    #[tokio::test]
    async fn test_execute_with_explicit_token_sends_header() {
        use axum::{Router, extract::Request, http::StatusCode, routing::get};

        let app = Router::new().route(
            "/api/v1/sessions",
            get(|req: Request| async move {
                let auth = req
                    .headers()
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                assert_eq!(auth, "Bearer test-token");
                (StatusCode::OK, "[]".to_owned())
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let node = format!("127.0.0.1:{}", addr.port());

        let cli = Cli {
            node,
            token: Some("test-token".into()),
            command: Some(Commands::List { all: false }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert_eq!(result, "No sessions.");
    }

    // -- Interventions tests --

    #[test]
    fn test_cli_parse_interventions() {
        let cli = Cli::try_parse_from(["pulpo", "interventions", "my-session"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Interventions { name }) if name == "my-session"
        ));
    }

    #[test]
    fn test_format_interventions_empty() {
        assert_eq!(format_interventions(&[]), "No intervention events.");
    }

    #[test]
    fn test_format_interventions_with_data() {
        let events = vec![
            InterventionEventResponse {
                id: 1,
                session_id: "sess-1".into(),
                code: None,
                reason: "Memory exceeded threshold".into(),
                created_at: "2026-01-01T00:00:00Z".into(),
            },
            InterventionEventResponse {
                id: 2,
                session_id: "sess-1".into(),
                code: None,
                reason: "Idle for 10 minutes".into(),
                created_at: "2026-01-02T00:00:00Z".into(),
            },
        ];
        let output = format_interventions(&events);
        assert!(output.contains("ID"));
        assert!(output.contains("TIMESTAMP"));
        assert!(output.contains("REASON"));
        assert!(output.contains("Memory exceeded threshold"));
        assert!(output.contains("Idle for 10 minutes"));
        assert!(output.contains("2026-01-01T00:00:00Z"));
    }

    #[tokio::test]
    async fn test_execute_interventions_empty() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Interventions {
                name: "my-session".into(),
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert_eq!(result, "No intervention events.");
    }

    #[tokio::test]
    async fn test_execute_interventions_with_data() {
        use axum::{Router, routing::get};

        let app = Router::new().route(
            "/api/v1/sessions/{id}/interventions",
            get(|| async {
                r#"[{"id":1,"session_id":"s","reason":"OOM","created_at":"2026-01-01T00:00:00Z"}]"#
                    .to_owned()
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let node = format!("127.0.0.1:{}", addr.port());

        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Interventions {
                name: "test".into(),
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("OOM"));
        assert!(result.contains("2026-01-01T00:00:00Z"));
    }

    #[tokio::test]
    async fn test_execute_interventions_connection_refused() {
        let cli = Cli {
            node: "localhost:1".into(),
            token: None,
            command: Some(Commands::Interventions {
                name: "test".into(),
            }),
            path: None,
        };
        let result = execute(&cli).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Could not connect to pulpod"));
    }

    // -- Attach command tests --

    #[test]
    fn test_build_attach_command_tmux() {
        let cmd = build_attach_command("my-session");
        assert_eq!(cmd.get_program(), "tmux");
        let args: Vec<&std::ffi::OsStr> = cmd.get_args().collect();
        assert_eq!(args, vec!["attach-session", "-t", "my-session"]);
    }

    #[test]
    fn test_build_attach_command_docker() {
        let cmd = build_attach_command("docker:pulpo-my-task");
        assert_eq!(cmd.get_program(), "docker");
        let args: Vec<&std::ffi::OsStr> = cmd.get_args().collect();
        assert_eq!(args, vec!["exec", "-it", "pulpo-my-task", "/bin/sh"]);
    }

    #[test]
    fn test_cli_parse_attach() {
        let cli = Cli::try_parse_from(["pulpo", "attach", "my-session"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Attach { name }) if name == "my-session"
        ));
    }

    #[test]
    fn test_cli_parse_attach_alias() {
        let cli = Cli::try_parse_from(["pulpo", "a", "my-session"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Attach { name }) if name == "my-session"
        ));
    }

    #[tokio::test]
    async fn test_execute_attach_success() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Attach {
                name: "test-session".into(),
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("Detached from session test-session"));
    }

    #[tokio::test]
    async fn test_execute_attach_with_backend_session_id() {
        use axum::{Router, routing::get};
        let session_json = r#"{"id":"00000000-0000-0000-0000-000000000002","name":"my-session","workdir":"/tmp","command":"echo test","description":null,"status":"active","exit_code":null,"backend_session_id":"my-session","output_snapshot":null,"metadata":null,"ink":null,"intervention_code":null,"intervention_reason":null,"intervention_at":null,"last_output_at":null,"idle_since":null,"idle_threshold_secs":null,"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}"#;
        let app = Router::new().route(
            "/api/v1/sessions/{id}",
            get(move || async move { session_json.to_owned() }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

        let cli = Cli {
            node: format!("127.0.0.1:{}", addr.port()),
            token: None,
            command: Some(Commands::Attach {
                name: "my-session".into(),
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("Detached from session my-session"));
    }

    #[tokio::test]
    async fn test_execute_attach_connection_refused() {
        let cli = Cli {
            node: "localhost:1".into(),
            token: None,
            command: Some(Commands::Attach {
                name: "test-session".into(),
            }),
            path: None,
        };
        let result = execute(&cli).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Could not connect to pulpod"));
    }

    #[tokio::test]
    async fn test_execute_attach_error_response() {
        use axum::{Router, http::StatusCode, routing::get};
        let app = Router::new().route(
            "/api/v1/sessions/{id}",
            get(|| async {
                (
                    StatusCode::NOT_FOUND,
                    r#"{"error":"session not found"}"#.to_owned(),
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

        let cli = Cli {
            node: format!("127.0.0.1:{}", addr.port()),
            token: None,
            command: Some(Commands::Attach {
                name: "nonexistent".into(),
            }),
            path: None,
        };
        let result = execute(&cli).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("session not found"));
    }

    #[tokio::test]
    async fn test_execute_attach_stale_session() {
        use axum::{Router, routing::get};
        let session_json = r#"{"id":"00000000-0000-0000-0000-000000000001","name":"stale-sess","workdir":"/tmp","command":"echo test","description":null,"status":"lost","exit_code":null,"backend_session_id":"stale-sess","output_snapshot":null,"metadata":null,"ink":null,"intervention_code":null,"intervention_reason":null,"intervention_at":null,"last_output_at":null,"idle_since":null,"idle_threshold_secs":null,"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}"#;
        let app = Router::new().route(
            "/api/v1/sessions/{id}",
            get(move || async move { session_json.to_owned() }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

        let cli = Cli {
            node: format!("127.0.0.1:{}", addr.port()),
            token: None,
            command: Some(Commands::Attach {
                name: "stale-sess".into(),
            }),
            path: None,
        };
        let result = execute(&cli).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("lost"));
        assert!(err.contains("pulpo resume"));
    }

    #[tokio::test]
    async fn test_execute_attach_dead_session() {
        use axum::{Router, routing::get};
        let session_json = r#"{"id":"00000000-0000-0000-0000-000000000001","name":"dead-sess","workdir":"/tmp","command":"echo test","description":null,"status":"stopped","exit_code":null,"backend_session_id":"dead-sess","output_snapshot":null,"metadata":null,"ink":null,"intervention_code":null,"intervention_reason":null,"intervention_at":null,"last_output_at":null,"idle_since":null,"idle_threshold_secs":null,"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}"#;
        let app = Router::new().route(
            "/api/v1/sessions/{id}",
            get(move || async move { session_json.to_owned() }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

        let cli = Cli {
            node: format!("127.0.0.1:{}", addr.port()),
            token: None,
            command: Some(Commands::Attach {
                name: "dead-sess".into(),
            }),
            path: None,
        };
        let result = execute(&cli).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("stopped"));
        assert!(err.contains("cannot attach"));
    }

    // -- Alias parse tests --

    #[test]
    fn test_cli_parse_alias_spawn() {
        let cli = Cli::try_parse_from(["pulpo", "s", "my-task", "--", "echo", "hello"]).unwrap();
        assert!(matches!(&cli.command, Some(Commands::Spawn { .. })));
    }

    #[test]
    fn test_cli_parse_alias_list() {
        let cli = Cli::try_parse_from(["pulpo", "ls"]).unwrap();
        assert!(matches!(&cli.command, Some(Commands::List { all: false })));
    }

    #[test]
    fn test_cli_parse_list_all() {
        let cli = Cli::try_parse_from(["pulpo", "ls", "-a"]).unwrap();
        assert!(matches!(&cli.command, Some(Commands::List { all: true })));

        let cli = Cli::try_parse_from(["pulpo", "list", "--all"]).unwrap();
        assert!(matches!(&cli.command, Some(Commands::List { all: true })));
    }

    #[test]
    fn test_cli_parse_alias_logs() {
        let cli = Cli::try_parse_from(["pulpo", "l", "my-session"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Logs { name, .. }) if name == "my-session"
        ));
    }

    #[test]
    fn test_cli_parse_alias_stop() {
        let cli = Cli::try_parse_from(["pulpo", "k", "my-session"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Stop { names, purge }) if names == &["my-session"] && !purge
        ));
    }

    #[test]
    fn test_cli_parse_alias_resume() {
        let cli = Cli::try_parse_from(["pulpo", "r", "my-session"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Resume { name }) if name == "my-session"
        ));
    }

    #[test]
    fn test_cli_parse_alias_nodes() {
        let cli = Cli::try_parse_from(["pulpo", "n"]).unwrap();
        assert!(matches!(&cli.command, Some(Commands::Nodes)));
    }

    #[test]
    fn test_cli_parse_alias_interventions() {
        let cli = Cli::try_parse_from(["pulpo", "iv", "my-session"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Interventions { name }) if name == "my-session"
        ));
    }

    #[test]
    fn test_api_error_json() {
        let err = api_error("{\"error\":\"session not found: foo\"}");
        assert_eq!(err.to_string(), "session not found: foo");
    }

    #[test]
    fn test_api_error_plain_text() {
        let err = api_error("plain text error");
        assert_eq!(err.to_string(), "plain text error");
    }

    // -- diff_output tests --

    #[test]
    fn test_diff_output_empty_prev() {
        assert_eq!(diff_output("", "line1\nline2\n"), "line1\nline2\n");
    }

    #[test]
    fn test_diff_output_identical() {
        assert_eq!(diff_output("line1\nline2", "line1\nline2"), "");
    }

    #[test]
    fn test_diff_output_new_lines_appended() {
        let prev = "line1\nline2";
        let new = "line1\nline2\nline3\nline4";
        assert_eq!(diff_output(prev, new), "line3\nline4");
    }

    #[test]
    fn test_diff_output_scrolled_window() {
        // Window of 3 lines: old lines scroll off top, new appear at bottom
        let prev = "line1\nline2\nline3";
        let new = "line2\nline3\nline4";
        assert_eq!(diff_output(prev, new), "line4");
    }

    #[test]
    fn test_diff_output_completely_different() {
        let prev = "aaa\nbbb";
        let new = "xxx\nyyy";
        assert_eq!(diff_output(prev, new), "xxx\nyyy");
    }

    #[test]
    fn test_diff_output_last_line_matches_but_overlap_fails() {
        // Last line of prev appears in new but preceding lines don't match
        let prev = "aaa\ncommon";
        let new = "zzz\ncommon\nnew_line";
        // "common" matches at index 1 of new, overlap_len = min(2, 2) = 2
        // prev_tail = ["aaa", "common"], new_overlap = ["zzz", "common"] — mismatch
        // Falls through, no verified overlap, so returns everything
        assert_eq!(diff_output(prev, new), "zzz\ncommon\nnew_line");
    }

    #[test]
    fn test_diff_output_new_empty() {
        assert_eq!(diff_output("line1", ""), "");
    }

    // -- follow_logs tests --

    /// Start a test server that simulates evolving output and session status transitions.
    /// Start a test server that simulates evolving output with agent exit marker.
    async fn start_follow_test_server() -> String {
        use axum::{Router, extract::Path, extract::Query, routing::get};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let call_count = Arc::new(AtomicUsize::new(0));
        let output_count = call_count.clone();

        let app = Router::new()
            .route(
                "/api/v1/sessions/{id}/output",
                get(
                    move |_path: Path<String>,
                          _query: Query<std::collections::HashMap<String, String>>| {
                        let count = output_count.clone();
                        async move {
                            let n = count.fetch_add(1, Ordering::SeqCst);
                            let output = match n {
                                0 => "line1\nline2".to_owned(),
                                1 => "line1\nline2\nline3".to_owned(),
                                _ => "line2\nline3\nline4\n[pulpo] Agent exited (session: test). Run: pulpo resume test".to_owned(),
                            };
                            format!(r#"{{"output":{}}}"#, serde_json::json!(output))
                        }
                    },
                ),
            )
            .route(
                "/api/v1/sessions/{id}",
                get(|_path: Path<String>| async {
                    r#"{"id":"00000000-0000-0000-0000-000000000001","name":"test","workdir":"/tmp","command":"echo test","description":null,"status":"active","exit_code":null,"backend_session_id":null,"output_snapshot":null,"metadata":null,"ink":null,"intervention_code":null,"intervention_reason":null,"intervention_at":null,"last_output_at":null,"idle_since":null,"idle_threshold_secs":null,"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}"#.to_owned()
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        format!("http://127.0.0.1:{}", addr.port())
    }

    #[tokio::test]
    async fn test_follow_logs_polls_and_exits_on_agent_exit_marker() {
        let base = start_follow_test_server().await;
        let client = reqwest::Client::new();
        let mut buf = Vec::new();

        follow_logs(&client, &base, "test", 100, None, &mut buf)
            .await
            .unwrap();

        let output = String::from_utf8(buf).unwrap();
        // Should contain initial output + new lines + agent exit marker
        assert!(output.contains("line1"));
        assert!(output.contains("line2"));
        assert!(output.contains("line3"));
        assert!(output.contains("line4"));
        assert!(output.contains("[pulpo] Agent exited"));
    }

    #[tokio::test]
    async fn test_execute_logs_follow_success() {
        let base = start_follow_test_server().await;
        // Extract host:port from http://127.0.0.1:PORT
        let node = base.strip_prefix("http://").unwrap().to_owned();

        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Logs {
                name: "test".into(),
                lines: 100,
                follow: true,
            }),
            path: None,
        };
        // execute() with follow writes to stdout and returns empty string
        let result = execute(&cli).await.unwrap();
        assert_eq!(result, "");
    }

    #[tokio::test]
    async fn test_execute_logs_follow_connection_refused() {
        let cli = Cli {
            node: "localhost:1".into(),
            token: None,
            command: Some(Commands::Logs {
                name: "test".into(),
                lines: 50,
                follow: true,
            }),
            path: None,
        };
        let result = execute(&cli).await;
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Could not connect to pulpod"),
            "Expected friendly error, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_follow_logs_exits_on_dead() {
        use axum::{Router, extract::Path, extract::Query, routing::get};

        let app = Router::new()
            .route(
                "/api/v1/sessions/{id}/output",
                get(
                    |_path: Path<String>,
                     _query: Query<std::collections::HashMap<String, String>>| async {
                        r#"{"output":"some output"}"#.to_owned()
                    },
                ),
            )
            .route(
                "/api/v1/sessions/{id}",
                get(|_path: Path<String>| async {
                    r#"{"id":"00000000-0000-0000-0000-000000000001","name":"test","workdir":"/tmp","command":"echo test","description":null,"status":"stopped","exit_code":null,"backend_session_id":null,"output_snapshot":null,"metadata":null,"ink":null,"intervention_code":null,"intervention_reason":null,"intervention_at":null,"last_output_at":null,"idle_since":null,"idle_threshold_secs":null,"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}"#.to_owned()
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let base = format!("http://127.0.0.1:{}", addr.port());

        let client = reqwest::Client::new();
        let mut buf = Vec::new();
        follow_logs(&client, &base, "test", 100, None, &mut buf)
            .await
            .unwrap();

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("some output"));
    }

    #[tokio::test]
    async fn test_follow_logs_exits_on_stale() {
        use axum::{Router, extract::Path, extract::Query, routing::get};

        let app = Router::new()
            .route(
                "/api/v1/sessions/{id}/output",
                get(
                    |_path: Path<String>,
                     _query: Query<std::collections::HashMap<String, String>>| async {
                        r#"{"output":"stale output"}"#.to_owned()
                    },
                ),
            )
            .route(
                "/api/v1/sessions/{id}",
                get(|_path: Path<String>| async {
                    r#"{"id":"00000000-0000-0000-0000-000000000001","name":"test","workdir":"/tmp","command":"echo test","description":null,"status":"lost","exit_code":null,"backend_session_id":null,"output_snapshot":null,"metadata":null,"ink":null,"intervention_code":null,"intervention_reason":null,"intervention_at":null,"last_output_at":null,"idle_since":null,"idle_threshold_secs":null,"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}"#.to_owned()
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let base = format!("http://127.0.0.1:{}", addr.port());

        let client = reqwest::Client::new();
        let mut buf = Vec::new();
        follow_logs(&client, &base, "test", 100, None, &mut buf)
            .await
            .unwrap();

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("stale output"));
    }

    #[tokio::test]
    async fn test_execute_logs_follow_non_reqwest_error() {
        use axum::{Router, extract::Path, extract::Query, routing::get};

        // Session status endpoint returns invalid JSON to trigger a serde error
        let app = Router::new()
            .route(
                "/api/v1/sessions/{id}/output",
                get(
                    |_path: Path<String>,
                     _query: Query<std::collections::HashMap<String, String>>| async {
                        r#"{"output":"initial"}"#.to_owned()
                    },
                ),
            )
            .route(
                "/api/v1/sessions/{id}",
                get(|_path: Path<String>| async { "not valid json".to_owned() }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let node = format!("127.0.0.1:{}", addr.port());

        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Logs {
                name: "test".into(),
                lines: 100,
                follow: true,
            }),
            path: None,
        };
        let err = execute(&cli).await.unwrap_err();
        // serde_json error, not a reqwest error — hits the Err(other) branch
        let msg = err.to_string();
        assert!(
            msg.contains("expected ident"),
            "Expected serde parse error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn test_fetch_session_status_connection_error() {
        let client = reqwest::Client::new();
        let result = fetch_session_status(&client, "http://127.0.0.1:1", "test", None).await;
        assert!(result.is_err());
    }

    // -- Schedule tests --

    #[test]
    fn test_format_schedules_empty() {
        assert_eq!(format_schedules(&[]), "No schedules.");
    }

    #[test]
    fn test_format_schedules_with_entries() {
        let schedules = vec![serde_json::json!({
            "name": "nightly",
            "cron": "0 3 * * *",
            "enabled": true,
            "last_run_at": null,
            "target_node": null
        })];
        let output = format_schedules(&schedules);
        assert!(output.contains("nightly"));
        assert!(output.contains("0 3 * * *"));
        assert!(output.contains("local"));
        assert!(output.contains("yes"));
        assert!(output.contains('-'));
    }

    #[test]
    fn test_format_schedules_disabled_entry() {
        let schedules = vec![serde_json::json!({
            "name": "weekly",
            "cron": "0 0 * * 0",
            "enabled": false,
            "last_run_at": "2026-03-18T03:00:00Z",
            "target_node": "gpu-box"
        })];
        let output = format_schedules(&schedules);
        assert!(output.contains("weekly"));
        assert!(output.contains("no"));
        assert!(output.contains("gpu-box"));
        assert!(output.contains("2026-03-18T03:00"));
    }

    #[test]
    fn test_format_schedules_header() {
        let schedules = vec![serde_json::json!({
            "name": "test",
            "cron": "* * * * *",
            "enabled": true,
            "last_run_at": null,
            "target_node": null
        })];
        let output = format_schedules(&schedules);
        assert!(output.contains("NAME"));
        assert!(output.contains("CRON"));
        assert!(output.contains("ENABLED"));
        assert!(output.contains("LAST RUN"));
        assert!(output.contains("NODE"));
    }

    // -- Schedule CLI parse tests --

    #[test]
    fn test_cli_parse_schedule_add() {
        let cli = Cli::try_parse_from([
            "pulpo",
            "schedule",
            "add",
            "nightly",
            "0 3 * * *",
            "--workdir",
            "/repo",
            "--",
            "claude",
            "-p",
            "review",
        ])
        .unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Schedule {
                action: ScheduleAction::Add { name, cron, .. }
            }) if name == "nightly" && cron == "0 3 * * *"
        ));
    }

    #[test]
    fn test_cli_parse_schedule_add_with_node() {
        let cli = Cli::try_parse_from([
            "pulpo",
            "schedule",
            "add",
            "nightly",
            "0 3 * * *",
            "--workdir",
            "/repo",
            "--node",
            "gpu-box",
            "--",
            "claude",
        ])
        .unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Schedule {
                action: ScheduleAction::Add { node, .. }
            }) if node.as_deref() == Some("gpu-box")
        ));
    }

    #[test]
    fn test_cli_parse_schedule_add_install_alias() {
        let cli =
            Cli::try_parse_from(["pulpo", "schedule", "install", "nightly", "0 3 * * *"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Schedule {
                action: ScheduleAction::Add { name, .. }
            }) if name == "nightly"
        ));
    }

    #[test]
    fn test_cli_parse_schedule_list() {
        let cli = Cli::try_parse_from(["pulpo", "schedule", "list"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Schedule {
                action: ScheduleAction::List
            })
        ));
    }

    #[test]
    fn test_cli_parse_schedule_remove() {
        let cli = Cli::try_parse_from(["pulpo", "schedule", "remove", "nightly"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Schedule {
                action: ScheduleAction::Remove { name }
            }) if name == "nightly"
        ));
    }

    #[test]
    fn test_cli_parse_schedule_pause() {
        let cli = Cli::try_parse_from(["pulpo", "schedule", "pause", "nightly"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Schedule {
                action: ScheduleAction::Pause { name }
            }) if name == "nightly"
        ));
    }

    #[test]
    fn test_cli_parse_schedule_resume() {
        let cli = Cli::try_parse_from(["pulpo", "schedule", "resume", "nightly"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Schedule {
                action: ScheduleAction::Resume { name }
            }) if name == "nightly"
        ));
    }

    #[test]
    fn test_cli_parse_schedule_alias() {
        let cli = Cli::try_parse_from(["pulpo", "sched", "list"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Schedule {
                action: ScheduleAction::List
            })
        ));
    }

    #[test]
    fn test_cli_parse_schedule_list_alias() {
        let cli = Cli::try_parse_from(["pulpo", "schedule", "ls"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Schedule {
                action: ScheduleAction::List
            })
        ));
    }

    #[test]
    fn test_cli_parse_schedule_remove_alias() {
        let cli = Cli::try_parse_from(["pulpo", "schedule", "rm", "nightly"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Schedule {
                action: ScheduleAction::Remove { name }
            }) if name == "nightly"
        ));
    }

    #[tokio::test]
    async fn test_execute_schedule_list_via_execute() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Schedule {
                action: ScheduleAction::List,
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        // Under coverage, execute_schedule is a stub that returns empty string
        #[cfg(coverage)]
        assert!(result.is_empty());
        #[cfg(not(coverage))]
        assert_eq!(result, "No schedules.");
    }

    #[test]
    fn test_schedule_action_debug() {
        let action = ScheduleAction::List;
        assert_eq!(format!("{action:?}"), "List");
    }

    #[test]
    fn test_cli_parse_send_alias() {
        let cli = Cli::try_parse_from(["pulpo", "send", "my-session", "y"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Input { name, text }) if name == "my-session" && text.as_deref() == Some("y")
        ));
    }

    #[test]
    fn test_cli_parse_spawn_no_name() {
        let cli = Cli::try_parse_from(["pulpo", "spawn"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Spawn { name, command, .. }) if name.is_none() && command.is_empty()
        ));
    }

    #[test]
    fn test_cli_parse_spawn_optional_name_with_command() {
        let cli = Cli::try_parse_from(["pulpo", "spawn", "--", "echo", "hello"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Spawn { name, command, .. })
                if name.is_none() && command == &["echo", "hello"]
        ));
    }

    #[test]
    fn test_cli_parse_path_shortcut() {
        let cli = Cli::try_parse_from(["pulpo", "/tmp/my-repo"]).unwrap();
        assert!(cli.command.is_none());
        assert_eq!(cli.path.as_deref(), Some("/tmp/my-repo"));
    }

    #[test]
    fn test_cli_parse_no_args() {
        let cli = Cli::try_parse_from(["pulpo"]).unwrap();
        assert!(cli.command.is_none());
        assert!(cli.path.is_none());
    }

    #[test]
    fn test_derive_session_name_simple() {
        assert_eq!(derive_session_name("/home/user/my-repo"), "my-repo");
    }

    #[test]
    fn test_derive_session_name_with_special_chars() {
        assert_eq!(derive_session_name("/home/user/My Repo_v2"), "my-repo-v2");
    }

    #[test]
    fn test_derive_session_name_root() {
        assert_eq!(derive_session_name("/"), "session");
    }

    #[test]
    fn test_derive_session_name_dots() {
        assert_eq!(derive_session_name("/home/user/.hidden"), "hidden");
    }

    #[test]
    fn test_resolve_path_absolute() {
        assert_eq!(resolve_path("/tmp/repo"), "/tmp/repo");
    }

    #[test]
    fn test_resolve_path_relative() {
        let resolved = resolve_path("my-repo");
        assert!(resolved.ends_with("my-repo"));
        assert!(resolved.starts_with('/'));
    }

    #[tokio::test]
    async fn test_execute_no_args_shows_help() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            path: None,
            command: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(
            result.is_empty(),
            "no-args should return empty string after printing help"
        );
    }

    #[tokio::test]
    async fn test_execute_path_shortcut() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            path: Some("/tmp".into()),
            command: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("Detached from session"));
    }

    #[tokio::test]
    async fn test_deduplicate_session_name_no_conflict() {
        // Connection refused → falls through to "name not taken" path
        let base = "http://127.0.0.1:1";
        let client = reqwest::Client::new();
        let name = deduplicate_session_name(&client, base, "fresh", None).await;
        assert_eq!(name, "fresh");
    }

    #[tokio::test]
    async fn test_deduplicate_session_name_with_conflict() {
        use axum::{Router, routing::get};
        use std::sync::atomic::{AtomicU32, Ordering};

        let call_count = std::sync::Arc::new(AtomicU32::new(0));
        let counter = call_count.clone();
        let app = Router::new()
            .route(
                "/api/v1/sessions/{id}",
                get(move || {
                    let c = counter.clone();
                    async move {
                        let n = c.fetch_add(1, Ordering::SeqCst);
                        if n == 0 {
                            // First call (base name) → exists
                            (axum::http::StatusCode::OK, TEST_SESSION_JSON.to_owned())
                        } else {
                            // Suffixed name → not found
                            (axum::http::StatusCode::NOT_FOUND, "not found".to_owned())
                        }
                    }
                }),
            )
            .route(
                "/api/v1/peers",
                get(|| async {
                    r#"{"local":{"name":"test","hostname":"h","os":"macos","arch":"arm64","cpus":8,"memory_mb":0,"gpu":null},"peers":[]}"#.to_owned()
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let base = format!("http://127.0.0.1:{}", addr.port());
        let client = reqwest::Client::new();
        let name = deduplicate_session_name(&client, &base, "repo", None).await;
        assert_eq!(name, "repo-2");
    }

    // -- Node resolution tests --

    #[test]
    fn test_node_needs_resolution() {
        assert!(!node_needs_resolution("localhost:7433"));
        assert!(!node_needs_resolution("mac-mini:7433"));
        assert!(!node_needs_resolution("10.0.0.1:7433"));
        assert!(!node_needs_resolution("[::1]:7433"));
        assert!(node_needs_resolution("mac-mini"));
        assert!(node_needs_resolution("linux-server"));
        assert!(node_needs_resolution("localhost"));
    }

    #[tokio::test]
    async fn test_resolve_node_with_port() {
        let client = reqwest::Client::new();
        let (addr, token) = resolve_node(&client, "mac-mini:7433").await;
        assert_eq!(addr, "mac-mini:7433");
        assert!(token.is_none());
    }

    #[tokio::test]
    async fn test_resolve_node_fallback_appends_port() {
        // No local daemon running on localhost:7433, so peer lookup fails
        // and it falls back to appending :7433
        let client = reqwest::Client::new();
        let (addr, token) = resolve_node(&client, "unknown-host").await;
        assert_eq!(addr, "unknown-host:7433");
        assert!(token.is_none());
    }

    #[cfg(not(coverage))]
    #[tokio::test]
    async fn test_resolve_node_finds_peer() {
        use axum::{Router, routing::get};

        let app = Router::new()
            .route(
                "/api/v1/peers",
                get(|| async {
                    r#"{"local":{"name":"local","hostname":"h","os":"macos","arch":"arm64","cpus":8,"memory_mb":0,"gpu":null},"peers":[{"name":"mac-mini","address":"10.0.0.5:7433","status":"online","node_info":null,"session_count":2,"source":"configured"}]}"#.to_owned()
                }),
            )
            .route(
                "/api/v1/config",
                get(|| async {
                    r#"{"node":{"name":"local","port":7433,"data_dir":"/tmp","bind":"local","tag":null,"seed":null,"discovery_interval_secs":30},"auth":{},"peers":{"mac-mini":{"address":"10.0.0.5:7433","token":"peer-secret"}},"watchdog":{"enabled":true,"memory_threshold":90,"check_interval_secs":10,"breach_count":3,"idle_timeout_secs":600,"idle_action":"alert","idle_threshold_secs":60},"notifications":{"discord":null,"webhooks":[]},"inks":{}}"#.to_owned()
                }),
            );

        // Port 7433 may be in use; skip test if so
        let Ok(listener) = tokio::net::TcpListener::bind("127.0.0.1:7433").await else {
            return;
        };
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

        let client = reqwest::Client::new();
        let (addr, token) = resolve_node(&client, "mac-mini").await;
        assert_eq!(addr, "10.0.0.5:7433");
        assert_eq!(token, Some("peer-secret".into()));
    }

    #[cfg(not(coverage))]
    #[tokio::test]
    async fn test_resolve_node_peer_no_token() {
        use axum::{Router, routing::get};

        let app = Router::new()
            .route(
                "/api/v1/peers",
                get(|| async {
                    r#"{"local":{"name":"local","hostname":"h","os":"macos","arch":"arm64","cpus":8,"memory_mb":0,"gpu":null},"peers":[{"name":"test-peer","address":"10.0.0.9:7433","status":"online","node_info":null,"session_count":null,"source":"configured"}]}"#.to_owned()
                }),
            )
            .route(
                "/api/v1/config",
                get(|| async {
                    r#"{"node":{"name":"local","port":7433,"data_dir":"/tmp","bind":"local","tag":null,"seed":null,"discovery_interval_secs":30},"auth":{},"peers":{"test-peer":"10.0.0.9:7433"},"watchdog":{"enabled":true,"memory_threshold":90,"check_interval_secs":10,"breach_count":3,"idle_timeout_secs":600,"idle_action":"alert","idle_threshold_secs":60},"notifications":{"discord":null,"webhooks":[]},"inks":{}}"#.to_owned()
                }),
            );

        let Ok(listener) = tokio::net::TcpListener::bind("127.0.0.1:7433").await else {
            return; // Port in use, skip
        };
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

        let client = reqwest::Client::new();
        let (addr, token) = resolve_node(&client, "test-peer").await;
        assert_eq!(addr, "10.0.0.9:7433");
        assert!(token.is_none()); // Simple peer entry has no token
    }

    #[tokio::test]
    async fn test_execute_with_peer_name_resolution() {
        // When node doesn't contain ':', resolve_node is called.
        // Since there's no local daemon on port 7433, it falls back to appending :7433.
        // The connection to the fallback address will fail, giving us a connection error.
        let cli = Cli {
            node: "nonexistent-peer".into(),
            token: None,
            command: Some(Commands::List { all: false }),
            path: None,
        };
        let result = execute(&cli).await;
        // Should try to connect to nonexistent-peer:7433 and fail
        assert!(result.is_err());
    }

    // -- Auto node selection tests --

    #[test]
    fn test_cli_parse_spawn_auto() {
        let cli = Cli::try_parse_from(["pulpo", "spawn", "my-task", "--auto"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Spawn { auto, .. }) if *auto
        ));
    }

    #[test]
    fn test_cli_parse_spawn_auto_default() {
        let cli = Cli::try_parse_from(["pulpo", "spawn", "my-task"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Spawn { auto, .. }) if !auto
        ));
    }

    #[tokio::test]
    async fn test_select_best_node_coverage_stub() {
        // Exercise the coverage stub (or real function in non-coverage builds)
        let client = reqwest::Client::new();
        // In coverage builds, the stub returns ("localhost:7433", "local")
        // In non-coverage builds, this fails because no server is running — that's OK
        let _result = select_best_node(&client, "http://127.0.0.1:19999", None).await;
    }

    #[cfg(not(coverage))]
    #[tokio::test]
    async fn test_select_best_node_picks_least_loaded() {
        use axum::{Router, routing::get};

        let app = Router::new().route(
            "/api/v1/peers",
            get(|| async {
                r#"{"local":{"name":"local","hostname":"h","os":"macos","arch":"arm64","cpus":8,"memory_mb":16384,"gpu":null},"peers":[{"name":"busy","address":"busy:7433","status":"online","node_info":{"name":"busy","hostname":"h","os":"linux","arch":"x86_64","cpus":4,"memory_mb":8192,"gpu":null},"session_count":5,"source":"configured"},{"name":"idle","address":"idle:7433","status":"online","node_info":{"name":"idle","hostname":"h","os":"linux","arch":"x86_64","cpus":8,"memory_mb":16384,"gpu":null},"session_count":1,"source":"configured"}]}"#.to_owned()
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let base = format!("http://127.0.0.1:{}", addr.port());

        let client = reqwest::Client::new();
        let (addr, name) = select_best_node(&client, &base, None).await.unwrap();
        // "idle" has 1 session + 16384 MB → score = 1 - 16 = -15
        // "busy" has 5 sessions + 8192 MB → score = 5 - 8 = -3
        // idle wins (lower score)
        assert_eq!(name, "idle");
        assert_eq!(addr, "idle:7433");
    }

    #[cfg(not(coverage))]
    #[tokio::test]
    async fn test_select_best_node_no_online_peers_falls_back_to_local() {
        use axum::{Router, routing::get};

        let app = Router::new().route(
            "/api/v1/peers",
            get(|| async {
                r#"{"local":{"name":"my-mac","hostname":"h","os":"macos","arch":"arm64","cpus":8,"memory_mb":16384,"gpu":null},"peers":[{"name":"offline-peer","address":"offline:7433","status":"offline","node_info":null,"session_count":null,"source":"configured"}]}"#.to_owned()
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let base = format!("http://127.0.0.1:{}", addr.port());

        let client = reqwest::Client::new();
        let (addr, name) = select_best_node(&client, &base, None).await.unwrap();
        assert_eq!(name, "my-mac");
        assert_eq!(addr, "localhost:7433");
    }

    #[cfg(not(coverage))]
    #[tokio::test]
    async fn test_select_best_node_empty_peers_falls_back_to_local() {
        use axum::{Router, routing::get};

        let app = Router::new().route(
            "/api/v1/peers",
            get(|| async {
                r#"{"local":{"name":"solo","hostname":"h","os":"macos","arch":"arm64","cpus":8,"memory_mb":16384,"gpu":null},"peers":[]}"#.to_owned()
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let base = format!("http://127.0.0.1:{}", addr.port());

        let client = reqwest::Client::new();
        let (addr, name) = select_best_node(&client, &base, None).await.unwrap();
        assert_eq!(name, "solo");
        assert_eq!(addr, "localhost:7433");
    }

    #[cfg(not(coverage))]
    #[tokio::test]
    async fn test_execute_spawn_auto_selects_node() {
        use axum::{
            Router,
            http::StatusCode,
            routing::{get, post},
        };

        let create_json = test_create_response_json();

        // Bind early so we know the address to embed in the peers response
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let node = format!("127.0.0.1:{}", addr.port());
        let peer_addr = node.clone();

        let app = Router::new()
            .route(
                "/api/v1/peers",
                get(move || {
                    let peer_addr = peer_addr.clone();
                    async move {
                        format!(
                            r#"{{"local":{{"name":"local","hostname":"h","os":"macos","arch":"arm64","cpus":8,"memory_mb":16384,"gpu":null}},"peers":[{{"name":"remote","address":"{peer_addr}","status":"online","node_info":{{"name":"remote","hostname":"h","os":"linux","arch":"x86_64","cpus":8,"memory_mb":32768,"gpu":null}},"session_count":0,"source":"configured"}}]}}"#
                        )
                    }
                }),
            )
            .route(
                "/api/v1/sessions",
                post(move || async move {
                    (StatusCode::CREATED, create_json.clone())
                }),
            );

        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Spawn {
                name: Some("test".into()),
                workdir: Some("/tmp/repo".into()),
                ink: None,
                description: None,
                detach: true,
                idle_threshold: None,
                auto: true,
                worktree: false,
                worktree_base: None,
                runtime: None,
                secret: vec![],
                command: vec!["echo".into(), "hello".into()],
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("Created session"));
    }

    #[cfg(not(coverage))]
    #[tokio::test]
    async fn test_select_best_node_peer_no_session_count() {
        use axum::{Router, routing::get};

        let app = Router::new().route(
            "/api/v1/peers",
            get(|| async {
                r#"{"local":{"name":"local","hostname":"h","os":"macos","arch":"arm64","cpus":8,"memory_mb":16384,"gpu":null},"peers":[{"name":"fresh","address":"fresh:7433","status":"online","node_info":null,"session_count":null,"source":"configured"}]}"#.to_owned()
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let base = format!("http://127.0.0.1:{}", addr.port());

        let client = reqwest::Client::new();
        let (addr, name) = select_best_node(&client, &base, None).await.unwrap();
        // Online peer with no session_count (0) and no node_info (0 mem) → score = 0
        assert_eq!(name, "fresh");
        assert_eq!(addr, "fresh:7433");
    }

    // -- Secret CLI parse tests --

    #[test]
    fn test_cli_parse_secret_set() {
        let cli = Cli::try_parse_from(["pulpo", "secret", "set", "MY_TOKEN", "abc123"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Secret { action: SecretAction::Set { name, value, env } })
                if name == "MY_TOKEN" && value == "abc123" && env.is_none()
        ));
    }

    #[test]
    fn test_cli_parse_secret_list() {
        let cli = Cli::try_parse_from(["pulpo", "secret", "list"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Secret {
                action: SecretAction::List
            })
        ));
    }

    #[test]
    fn test_cli_parse_secret_list_alias() {
        let cli = Cli::try_parse_from(["pulpo", "secret", "ls"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Secret {
                action: SecretAction::List
            })
        ));
    }

    #[test]
    fn test_cli_parse_secret_delete() {
        let cli = Cli::try_parse_from(["pulpo", "secret", "delete", "MY_TOKEN"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Secret { action: SecretAction::Delete { name } })
                if name == "MY_TOKEN"
        ));
    }

    #[test]
    fn test_cli_parse_secret_delete_alias() {
        let cli = Cli::try_parse_from(["pulpo", "secret", "rm", "MY_TOKEN"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Secret { action: SecretAction::Delete { name } })
                if name == "MY_TOKEN"
        ));
    }

    #[test]
    fn test_cli_parse_secret_alias() {
        let cli = Cli::try_parse_from(["pulpo", "sec", "list"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Secret {
                action: SecretAction::List
            })
        ));
    }

    #[test]
    fn test_format_secrets_empty() {
        let secrets: Vec<serde_json::Value> = vec![];
        assert_eq!(format_secrets(&secrets), "No secrets configured.");
    }

    #[test]
    fn test_format_secrets_with_entries() {
        let secrets = vec![
            serde_json::json!({"name": "GITHUB_TOKEN", "created_at": "2026-03-21T12:00:00Z"}),
            serde_json::json!({"name": "NPM_TOKEN", "created_at": "2026-03-20T10:30:00Z"}),
        ];
        let output = format_secrets(&secrets);
        assert!(output.contains("GITHUB_TOKEN"));
        assert!(output.contains("NPM_TOKEN"));
        assert!(output.contains("NAME"));
        assert!(output.contains("ENV"));
        assert!(output.contains("CREATED"));
    }

    #[test]
    fn test_format_secrets_with_env() {
        let secrets = vec![
            serde_json::json!({"name": "GH_WORK", "env": "GITHUB_TOKEN", "created_at": "2026-03-21T12:00:00Z"}),
            serde_json::json!({"name": "NPM_TOKEN", "created_at": "2026-03-20T10:30:00Z"}),
        ];
        let output = format_secrets(&secrets);
        assert!(output.contains("GH_WORK"));
        assert!(output.contains("GITHUB_TOKEN"));
        assert!(output.contains("NPM_TOKEN"));
    }

    #[test]
    fn test_format_secrets_short_timestamp() {
        let secrets = vec![serde_json::json!({"name": "KEY", "created_at": "now"})];
        let output = format_secrets(&secrets);
        assert!(output.contains("now"));
    }

    #[test]
    fn test_format_schedules_short_last_run_at() {
        // Regression: last_run_at shorter than 16 chars must not panic
        let schedules = vec![serde_json::json!({
            "name": "test",
            "cron": "* * * * *",
            "enabled": true,
            "last_run_at": "short",
            "target_node": null
        })];
        let output = format_schedules(&schedules);
        assert!(output.contains("short"));
    }

    #[test]
    fn test_format_sessions_multibyte_command_truncation() {
        use chrono::Utc;
        use pulpo_common::session::SessionStatus;
        use uuid::Uuid;

        // Command with multi-byte chars exceeding 50 bytes; must not panic
        let sessions = vec![Session {
            id: Uuid::nil(),
            name: "test".into(),
            workdir: "/tmp".into(),
            command: "echo '\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}'".into(),
            description: None,
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
            idle_threshold_secs: None,
            worktree_path: None,
            worktree_branch: None,
            runtime: Runtime::Tmux,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }];
        let output = format_sessions(&sessions);
        assert!(output.contains("..."));
    }
}
