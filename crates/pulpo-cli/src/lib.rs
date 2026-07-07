use anyhow::Result;
use clap::{Parser, Subcommand};
#[cfg_attr(coverage, allow(unused_imports))]
use pulpo_common::api::{
    CleanupResponse, CreateSessionResponse, InterventionEventResponse, PeersResponse,
    UsageProjectionResponse, UsageScanResponse,
};
use pulpo_common::session::{Session, SessionStatus};

mod format;
mod http;

#[cfg_attr(coverage, allow(unused_imports))]
use format::{
    format_cleanup_message, format_ink_detail, format_inks, format_interventions, format_nodes,
    format_schedules, format_secrets, format_sessions, format_usage_projection, format_usage_scan,
    format_worktree_sessions,
};
#[cfg_attr(coverage, allow(unused_imports))]
use http::{
    authed_get, authed_post, base_url, friendly_error, get_json, is_localhost, ok_or_api_error,
    request_json, request_text, resolve_node, resolve_token,
};

#[derive(Parser, Debug)]
#[command(
    name = "pulpo",
    about = "Manage agent sessions across your machines",
    version = env!("PULPO_VERSION")
)]
pub struct Cli {
    /// Target node (default: localhost)
    ///
    /// `global = true` so it parses before or after a subcommand
    /// (`pulpo --node X ui` and `pulpo ui --node X` both work) without
    /// `args_conflicts_with_subcommands` mistaking the subcommand for the quick-spawn path.
    #[arg(long, global = true, default_value = "localhost:7433")]
    pub node: String,

    /// Auth token (auto-discovered from local daemon if omitted)
    #[arg(long, global = true)]
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

        /// Create an isolated git worktree for the session
        #[arg(short, long)]
        worktree: bool,

        /// Base branch to fork the worktree from (implies --worktree)
        #[arg(long = "worktree-base")]
        worktree_base: Option<String>,

        /// Secrets to inject as environment variables (by name)
        #[arg(long)]
        secret: Vec<String>,

        /// Cost budget in USD (watchdog alerts at 80%, stops at 100%)
        #[arg(long = "budget-cost")]
        budget_cost: Option<f64>,

        /// Command to run (everything after --)
        #[arg(last = true)]
        command: Vec<String>,
    },

    /// Hand off a finished session's working context (directory, worktree) to a new session
    #[command(visible_alias = "h")]
    Handoff {
        /// Source session name or ID
        source: String,

        /// New session name (auto-generated as `<source>-2`, `-3`, ... if omitted)
        name: Option<String>,

        /// Human-readable description of the task
        #[arg(long)]
        description: Option<String>,

        /// Secrets to inject as environment variables (by name)
        #[arg(long)]
        secret: Vec<String>,

        /// Cost budget in USD (watchdog alerts at 80%, stops at 100%)
        #[arg(long = "budget-cost")]
        budget_cost: Option<f64>,

        /// Idle threshold in seconds (0 = never idle)
        #[arg(long)]
        idle_threshold: Option<u32>,

        /// Don't attach to the new session after handoff
        #[arg(short, long)]
        detach: bool,

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

    /// List known nodes on the tailnet/peer registry
    #[command(visible_alias = "n")]
    Nodes,

    /// Show intervention history for a session
    #[command(visible_alias = "iv")]
    Interventions {
        /// Session name or ID
        name: String,
    },

    /// Show token/cost burn rate, time-to-cap, and quota for sessions on this node
    Usage {
        /// Scan ALL local agent history (Claude + Codex) instead of pulpo-managed
        /// sessions — total spend by agent, model, and repo, no sessions routed through pulpo.
        #[arg(long)]
        scan: bool,
        /// With --scan: keep each git worktree/subdirectory as its own row instead of
        /// collapsing them onto their origin repository.
        #[arg(long, requires = "scan")]
        by_worktree: bool,
        /// With --scan: limit to the last N days (default: all-time).
        #[arg(long, value_name = "DAYS", requires = "scan")]
        since: Option<u32>,
        /// Output raw JSON instead of the formatted report.
        #[arg(long)]
        json: bool,
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

    /// Manage ink presets (reusable command templates)
    Ink {
        #[command(subcommand)]
        action: InkAction,
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
pub enum InkAction {
    /// List all ink presets
    #[command(visible_alias = "ls")]
    List,
    /// Show details for a specific ink
    Get {
        /// Ink name
        name: String,
    },
    /// Add a new ink preset
    Add {
        /// Ink name
        name: String,
        /// Human-readable description
        #[arg(long)]
        description: Option<String>,
        /// Command template
        #[arg(long)]
        command: Option<String>,
        /// Secrets to inject (by name, repeatable)
        #[arg(long)]
        secret: Vec<String>,
    },
    /// Update an existing ink preset
    Update {
        /// Ink name
        name: String,
        /// Human-readable description
        #[arg(long)]
        description: Option<String>,
        /// Command template
        #[arg(long)]
        command: Option<String>,
        /// Secrets to inject (by name, repeatable). Replaces existing secrets.
        #[arg(long)]
        secret: Vec<String>,
    },
    /// Remove an ink preset
    #[command(visible_alias = "rm")]
    Remove {
        /// Ink name
        name: String,
    },
}

#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)]
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
        /// Ink preset
        #[arg(long)]
        ink: Option<String>,
        /// Description
        #[arg(long)]
        description: Option<String>,
        /// Secrets to inject as environment variables (by name, repeatable)
        #[arg(long)]
        secret: Vec<String>,
        /// Create an isolated git worktree for each run
        #[arg(long)]
        worktree: bool,
        /// Base branch to fork the worktree from (implies --worktree)
        #[arg(long = "worktree-base")]
        worktree_base: Option<String>,
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

/// Response shape for the output endpoint.
#[derive(serde::Deserialize)]
struct OutputResponse {
    output: String,
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

/// Check if a TERM value is widely available across systems.
/// Returns false for exotic terminal types (e.g. xterm-ghostty) that may not
/// have terminfo entries installed on remote or headless machines.
fn is_safe_term(term: &str) -> bool {
    matches!(
        term,
        "xterm"
            | "xterm-256color"
            | "screen"
            | "screen-256color"
            | "tmux"
            | "tmux-256color"
            | "linux"
            | "vt100"
            | "dumb"
    )
}

/// Build the command to attach to a session's terminal.
#[cfg_attr(coverage, allow(dead_code))]
fn build_attach_command(backend_session_id: &str) -> std::process::Command {
    // tmux sessions — force a safe TERM value so attach works even when the
    // local terminal uses an exotic terminfo (e.g. xterm-ghostty) that isn't
    // installed on the machine running tmux.
    #[cfg(not(target_os = "windows"))]
    {
        let mut cmd = std::process::Command::new("tmux");
        cmd.args(["attach-session", "-t", backend_session_id]);
        let term = std::env::var("TERM").unwrap_or_default();
        if !is_safe_term(&term) {
            cmd.env("TERM", "xterm-256color");
        }
        cmd
    }
    #[cfg(target_os = "windows")]
    {
        // tmux attach not available on Windows — inform the user
        let mut cmd = std::process::Command::new("cmd");
        cmd.args([
            "/C",
            "echo",
            "Attach not available on Windows. Use the web UI.",
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
    eprintln!("tmux attach is not available on Windows. Use the web UI.");
    Ok(())
}

/// Stub for test and coverage builds — avoids spawning real terminals during tests.
#[cfg(any(test, coverage))]
#[allow(clippy::unnecessary_wraps, clippy::missing_const_for_fn)]
fn attach_session(_backend_session_id: &str) -> Result<()> {
    Ok(())
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
    // Poll up to 3 times at 500ms intervals — handles slow daemons
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
            match session.status {
                SessionStatus::Creating => continue,
                SessionStatus::Lost | SessionStatus::Stopped => {
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

/// Shared tail for `spawn` and `handoff`: print the creation message, optionally
/// verify the new session survived past `creating` (only for explicit commands —
/// shell sessions may be immediately marked idle/stopped by the watchdog, which is
/// expected), then attach unless `detach` is set or the target node isn't local.
async fn attach_or_report(
    client: &reqwest::Client,
    url: &str,
    node: &str,
    token: Option<&str>,
    resp: &CreateSessionResponse,
    detach: bool,
    has_explicit_command: bool,
) -> Result<String> {
    let msg = format!(
        "Created session \"{}\" ({})",
        resp.session.name, resp.session.id
    );
    // Auto-detach for a remote node — can't attach to a remote tmux session.
    if !is_localhost(node) {
        return Ok(msg);
    }
    if !detach {
        let backend_id = resp
            .session
            .backend_session_id
            .as_deref()
            .unwrap_or(&resp.session.name);
        eprintln!("{msg}");
        if has_explicit_command {
            let sid = resp.session.id.to_string();
            check_session_alive(client, url, &sid, token).await?;
        }
        attach_session(backend_id)?;
        return Ok(format!("Detached from session \"{}\".", resp.session.name));
    }
    Ok(msg)
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
#[allow(clippy::too_many_lines)]
async fn execute_schedule(
    client: &reqwest::Client,
    action: &ScheduleAction,
    base: &str,
    token: Option<&str>,
    node: &str,
) -> Result<String> {
    match action {
        ScheduleAction::Add {
            name,
            cron,
            workdir,
            ink,
            description,
            secret,
            worktree,
            worktree_base,
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
            let use_worktree = *worktree || worktree_base.is_some();
            let mut body = serde_json::json!({
                "name": name,
                "cron": cron,
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
            if !secret.is_empty() {
                body["secrets"] = serde_json::json!(secret);
            }
            if use_worktree {
                body["worktree"] = serde_json::json!(true);
            }
            if let Some(wb) = worktree_base {
                body["worktree_base"] = serde_json::json!(wb);
            }
            request_text(
                client,
                reqwest::Method::POST,
                format!("{base}/api/v1/schedules"),
                token,
                node,
                Some(&body),
            )
            .await?;
            Ok(format!("Created schedule \"{name}\""))
        }
        ScheduleAction::List => {
            let schedules: Vec<serde_json::Value> =
                get_json(client, format!("{base}/api/v1/schedules"), token, node).await?;
            Ok(format_schedules(&schedules))
        }
        ScheduleAction::Remove { name } => {
            request_text(
                client,
                reqwest::Method::DELETE,
                format!("{base}/api/v1/schedules/{name}"),
                token,
                node,
                None,
            )
            .await?;
            Ok(format!("Removed schedule \"{name}\""))
        }
        ScheduleAction::Pause { name } => {
            let body = serde_json::json!({ "enabled": false });
            request_text(
                client,
                reqwest::Method::PUT,
                format!("{base}/api/v1/schedules/{name}"),
                token,
                node,
                Some(&body),
            )
            .await?;
            Ok(format!("Paused schedule \"{name}\""))
        }
        ScheduleAction::Resume { name } => {
            let body = serde_json::json!({ "enabled": true });
            request_text(
                client,
                reqwest::Method::PUT,
                format!("{base}/api/v1/schedules/{name}"),
                token,
                node,
                Some(&body),
            )
            .await?;
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
    _node: &str,
) -> Result<String> {
    Ok(String::new())
}

// --- Secret API ---

/// Execute a secret subcommand via the secrets API.
#[cfg(not(coverage))]
async fn execute_secret(
    client: &reqwest::Client,
    action: &SecretAction,
    base: &str,
    token: Option<&str>,
    node: &str,
) -> Result<String> {
    match action {
        SecretAction::Set { name, value, env } => {
            let mut body = serde_json::json!({ "value": value });
            if let Some(e) = env {
                body["env"] = serde_json::json!(e);
            }
            request_text(
                client,
                reqwest::Method::PUT,
                format!("{base}/api/v1/secrets/{name}"),
                token,
                node,
                Some(&body),
            )
            .await?;
            Ok(format!("Secret \"{name}\" set."))
        }
        SecretAction::List => {
            let parsed: serde_json::Value =
                get_json(client, format!("{base}/api/v1/secrets"), token, node).await?;
            let secrets = parsed["secrets"].as_array().map_or(&[][..], Vec::as_slice);
            Ok(format_secrets(secrets))
        }
        SecretAction::Delete { name } => {
            request_text(
                client,
                reqwest::Method::DELETE,
                format!("{base}/api/v1/secrets/{name}"),
                token,
                node,
                None,
            )
            .await?;
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
    _node: &str,
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
    node: &str,
) -> Result<String> {
    match action {
        WorktreeAction::List => {
            let sessions: Vec<Session> =
                get_json(client, format!("{base}/api/v1/sessions"), token, node).await?;
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
    _node: &str,
) -> Result<String> {
    Ok(String::new())
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_ink_body(
    base: &serde_json::Value,
    description: Option<&String>,
    command: Option<&String>,
    secret: &[String],
) -> serde_json::Value {
    let mut body = base.clone();
    if let Some(d) = description {
        body["description"] = serde_json::json!(d);
    }
    if let Some(c) = command {
        body["command"] = serde_json::json!(c);
    }
    if !secret.is_empty() {
        body["secrets"] = serde_json::json!(secret);
    }
    body
}

/// Execute an ink subcommand via the inks API.
#[cfg(not(coverage))]
async fn execute_ink(
    client: &reqwest::Client,
    action: &InkAction,
    base: &str,
    token: Option<&str>,
    node: &str,
) -> Result<String> {
    match action {
        InkAction::List => {
            let wrapper: serde_json::Value =
                get_json(client, format!("{base}/api/v1/inks"), token, node).await?;
            let inks = wrapper
                .get("inks")
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();
            Ok(format_inks(&inks))
        }
        InkAction::Get { name } => {
            let ink: serde_json::Value =
                get_json(client, format!("{base}/api/v1/inks/{name}"), token, node).await?;
            Ok(format_ink_detail(name, &ink))
        }
        InkAction::Add {
            name,
            description,
            command,
            secret,
        } => {
            let body = build_ink_body(
                &serde_json::json!({}),
                description.as_ref(),
                command.as_ref(),
                secret,
            );
            request_text(
                client,
                reqwest::Method::POST,
                format!("{base}/api/v1/inks/{name}"),
                token,
                node,
                Some(&body),
            )
            .await?;
            Ok(format!("Created ink \"{name}\""))
        }
        InkAction::Update {
            name,
            description,
            command,
            secret,
        } => {
            let existing: serde_json::Value =
                get_json(client, format!("{base}/api/v1/inks/{name}"), token, node).await?;
            let body = build_ink_body(&existing, description.as_ref(), command.as_ref(), secret);
            request_text(
                client,
                reqwest::Method::PUT,
                format!("{base}/api/v1/inks/{name}"),
                token,
                node,
                Some(&body),
            )
            .await?;
            Ok(format!("Updated ink \"{name}\""))
        }
        InkAction::Remove { name } => {
            request_text(
                client,
                reqwest::Method::DELETE,
                format!("{base}/api/v1/inks/{name}"),
                token,
                node,
                None,
            )
            .await?;
            Ok(format!("Removed ink \"{name}\""))
        }
    }
}

/// Coverage stub for ink execution.
#[cfg(coverage)]
#[allow(clippy::unnecessary_wraps)]
async fn execute_ink(
    _client: &reqwest::Client,
    _action: &InkAction,
    _base: &str,
    _token: Option<&str>,
    _node: &str,
) -> Result<String> {
    Ok(String::new())
}

/// Try to start pulpod if it's not reachable on localhost.
/// Returns true if the daemon was started (or was already running).
#[cfg(not(coverage))]
#[cfg(not(any(test, coverage)))]
async fn ensure_daemon_running(client: &reqwest::Client, url: &str, node: &str) -> bool {
    if !is_localhost(node) {
        return true; // Remote node — not our job to start
    }
    // Quick health check
    if client
        .get(format!("{url}/api/v1/health"))
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
        .is_ok()
    {
        return true; // Already running
    }

    eprintln!("pulpod is not running — starting it...");

    // Try brew services first (macOS), then systemd (Linux), then direct spawn
    let started = if cfg!(target_os = "macos") {
        std::process::Command::new("brew")
            .args(["services", "start", "pulpo"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    } else {
        std::process::Command::new("systemctl")
            .args(["--user", "start", "pulpo"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };

    if !started {
        // Fallback: spawn pulpod directly in background
        if std::process::Command::new("pulpod")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_err()
        {
            eprintln!(
                "Failed to start pulpod. Install it with: brew install darioblanco/tap/pulpo"
            );
            return false;
        }
    }

    // Wait for it to become reachable (up to 5 seconds)
    for _ in 0..10 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if client
            .get(format!("{url}/api/v1/health"))
            .timeout(std::time::Duration::from_secs(1))
            .send()
            .await
            .is_ok()
        {
            eprintln!("pulpod started.");
            return true;
        }
    }
    eprintln!("pulpod did not start in time.");
    false
}

/// Stub for tests and coverage builds — avoids starting services during runners.
#[cfg(any(test, coverage))]
async fn ensure_daemon_running(_client: &reqwest::Client, _url: &str, _node: &str) -> bool {
    tokio::task::yield_now().await;
    true
}

/// Execute the given CLI command against the specified node.
#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli) -> Result<String> {
    let client = reqwest::Client::new();
    let (resolved_node, peer_token) = resolve_node(&client, &cli.node).await;
    let url = base_url(&resolved_node);
    let node = &resolved_node;

    // Auto-start pulpod if it's not running on localhost
    ensure_daemon_running(&client, &url, node).await;

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
        let resp: CreateSessionResponse = request_json(
            &client,
            reqwest::Method::POST,
            format!("{url}/api/v1/sessions"),
            token.as_deref(),
            node,
            Some(&body),
        )
        .await?;
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
            let session: Session = get_json(
                &client,
                format!("{url}/api/v1/sessions/{name}"),
                token.as_deref(),
                node,
            )
            .await?;
            match session.status {
                SessionStatus::Lost => {
                    anyhow::bail!(
                        "Session \"{name}\" is lost (agent process died). Resume it first:\n  pulpo resume {name}"
                    );
                }
                SessionStatus::Stopped => {
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
            request_text(
                &client,
                reqwest::Method::POST,
                format!("{url}/api/v1/sessions/{name}/input"),
                token.as_deref(),
                node,
                Some(&body),
            )
            .await?;
            Ok(format!("Sent input to session {name}."))
        }
        Commands::List { all } => {
            let list_url = if *all {
                format!("{url}/api/v1/sessions")
            } else {
                format!("{url}/api/v1/sessions?status=creating,active,idle,ready")
            };
            let sessions: Vec<Session> =
                get_json(&client, list_url, token.as_deref(), node).await?;
            Ok(format_sessions(&sessions))
        }
        Commands::Nodes => {
            let resp: PeersResponse = get_json(
                &client,
                format!("{url}/api/v1/peers"),
                token.as_deref(),
                node,
            )
            .await?;
            Ok(format_nodes(&resp))
        }
        Commands::Spawn {
            workdir,
            name,
            ink,
            description,
            detach,
            idle_threshold,
            worktree,
            worktree_base,
            secret,
            budget_cost,
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
            if let Some(b) = budget_cost {
                body["budget_cost_usd"] = serde_json::json!(b);
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
            if !secret.is_empty() {
                body["secrets"] = serde_json::json!(secret);
            }
            if let Ok(tp) = std::env::var("TERM_PROGRAM") {
                body["term_program"] = serde_json::json!(tp);
            }
            let resp: CreateSessionResponse = request_json(
                &client,
                reqwest::Method::POST,
                format!("{url}/api/v1/sessions"),
                token.as_deref(),
                node,
                Some(&body),
            )
            .await?;
            attach_or_report(
                &client,
                &url,
                node,
                token.as_deref(),
                &resp,
                *detach,
                cmd.is_some(),
            )
            .await
        }
        Commands::Handoff {
            source,
            name,
            description,
            secret,
            budget_cost,
            idle_threshold,
            detach,
            command,
        } => {
            let cmd = if command.is_empty() {
                None
            } else {
                Some(command.join(" "))
            };
            let mut body = serde_json::json!({});
            if let Some(n) = name {
                body["name"] = serde_json::json!(n);
            }
            if let Some(c) = &cmd {
                body["command"] = serde_json::json!(c);
            }
            if let Some(d) = description {
                body["description"] = serde_json::json!(d);
            }
            if !secret.is_empty() {
                body["secrets"] = serde_json::json!(secret);
            }
            if let Some(b) = budget_cost {
                body["budget_cost_usd"] = serde_json::json!(b);
            }
            if let Some(t) = idle_threshold {
                body["idle_threshold_secs"] = serde_json::json!(t);
            }
            if let Ok(tp) = std::env::var("TERM_PROGRAM") {
                body["term_program"] = serde_json::json!(tp);
            }
            let resp: CreateSessionResponse = request_json(
                &client,
                reqwest::Method::POST,
                format!("{url}/api/v1/sessions/{source}/handoff"),
                token.as_deref(),
                node,
                Some(&body),
            )
            .await?;
            attach_or_report(
                &client,
                &url,
                node,
                token.as_deref(),
                &resp,
                *detach,
                cmd.is_some(),
            )
            .await
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
            let result: CleanupResponse = request_json(
                &client,
                reqwest::Method::POST,
                format!("{url}/api/v1/sessions/cleanup"),
                token.as_deref(),
                node,
                None,
            )
            .await?;
            Ok(format_cleanup_message(
                result.sessions_deleted,
                result.worktrees_cleaned,
                result.logs_cleaned,
            ))
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
            let events: Vec<InterventionEventResponse> = get_json(
                &client,
                format!("{url}/api/v1/sessions/{name}/interventions"),
                token.as_deref(),
                node,
            )
            .await?;
            Ok(format_interventions(&events))
        }
        Commands::Usage {
            scan,
            by_worktree,
            since,
            json,
        } => {
            if *scan {
                let mut params: Vec<String> = Vec::new();
                if *by_worktree {
                    params.push("by_worktree=true".to_owned());
                }
                if let Some(days) = since {
                    params.push(format!("since_days={days}"));
                }
                let query = if params.is_empty() {
                    String::new()
                } else {
                    format!("?{}", params.join("&"))
                };
                let report: UsageScanResponse = get_json(
                    &client,
                    format!("{url}/api/v1/usage/scan{query}"),
                    token.as_deref(),
                    node,
                )
                .await?;
                if *json {
                    Ok(serde_json::to_string_pretty(&report)?)
                } else {
                    Ok(format_usage_scan(&report))
                }
            } else {
                let projection: UsageProjectionResponse = get_json(
                    &client,
                    format!("{url}/api/v1/usage/projection"),
                    token.as_deref(),
                    node,
                )
                .await?;
                if *json {
                    Ok(serde_json::to_string_pretty(&projection)?)
                } else {
                    Ok(format_usage_projection(&projection))
                }
            }
        }
        Commands::Ui => {
            let dashboard = base_url(node);
            open_browser(&dashboard)?;
            Ok(format!("Opening {dashboard}"))
        }
        Commands::Resume { name } => {
            let session: Session = request_json(
                &client,
                reqwest::Method::POST,
                format!("{url}/api/v1/sessions/{name}/resume"),
                token.as_deref(),
                node,
                None,
            )
            .await?;
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
        Commands::Schedule { action } => {
            execute_schedule(&client, action, &url, token.as_deref(), node).await
        }
        Commands::Secret { action } => {
            execute_secret(&client, action, &url, token.as_deref(), node).await
        }
        Commands::Worktree { action } => {
            execute_worktree(&client, action, &url, token.as_deref(), node).await
        }
        Commands::Ink { action } => {
            execute_ink(&client, action, &url, token.as_deref(), node).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // `--node` is global, so a subcommand after it parses as the subcommand
        // (not swallowed as the quick-spawn path).
        let cli = Cli::try_parse_from(["pulpo", "--node", "mac-mini:7433", "ui"]).unwrap();
        assert_eq!(cli.node, "mac-mini:7433");
        assert!(cli.path.is_none());
        assert!(matches!(cli.command, Some(Commands::Ui)));
    }

    #[test]
    fn test_cli_parse_node_after_subcommand() {
        // Global `--node` also works *after* the subcommand.
        let cli = Cli::try_parse_from(["pulpo", "ui", "--node", "mac-mini:7433"]).unwrap();
        assert_eq!(cli.node, "mac-mini:7433");
        assert!(matches!(cli.command, Some(Commands::Ui)));
    }

    #[test]
    fn test_cli_parse_node_with_quick_spawn_path() {
        // A real path after `--node` is still the quick-spawn positional (not a subcommand).
        let cli = Cli::try_parse_from(["pulpo", "--node", "box:7433", "/tmp/repo"]).unwrap();
        assert_eq!(cli.node, "box:7433");
        assert!(cli.command.is_none());
        assert_eq!(cli.path.as_deref(), Some("/tmp/repo"));
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
    fn test_cli_parse_spawn_budget_cost() {
        let cli =
            Cli::try_parse_from(["pulpo", "spawn", "my-task", "--budget-cost", "5.50"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Spawn { budget_cost, .. }) if *budget_cost == Some(5.50)
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
    fn test_cli_parse_spawn_worktree_short() {
        let cli = Cli::try_parse_from(["pulpo", "spawn", "x", "-w"]).unwrap();
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
    fn test_cli_parse_handoff_basic() {
        let cli = Cli::try_parse_from(["pulpo", "handoff", "plan-auth"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Handoff { source, name, command, .. })
                if source == "plan-auth" && name.is_none() && command.is_empty()
        ));
    }

    #[test]
    fn test_cli_parse_handoff_alias_h() {
        let cli = Cli::try_parse_from(["pulpo", "h", "plan-auth"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Handoff { source, .. }) if source == "plan-auth"
        ));
    }

    #[test]
    fn test_cli_parse_handoff_with_name() {
        let cli = Cli::try_parse_from(["pulpo", "handoff", "plan-auth", "implement-auth"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Handoff { source, name, .. })
                if source == "plan-auth" && name.as_deref() == Some("implement-auth")
        ));
    }

    #[test]
    fn test_cli_parse_handoff_with_command() {
        let cli =
            Cli::try_parse_from(["pulpo", "handoff", "plan-auth", "--", "codex", "implement"])
                .unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Handoff { source, command, .. })
                if source == "plan-auth" && command == &["codex", "implement"]
        ));
    }

    #[test]
    fn test_cli_parse_handoff_flags() {
        let cli = Cli::try_parse_from([
            "pulpo",
            "handoff",
            "plan-auth",
            "implement-auth",
            "--description",
            "Implement the plan",
            "--secret",
            "GH_WORK",
            "--budget-cost",
            "2.5",
            "--idle-threshold",
            "30",
            "-d",
        ])
        .unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Handoff {
                source,
                name,
                description,
                secret,
                budget_cost,
                idle_threshold,
                detach,
                ..
            })
                if source == "plan-auth"
                    && name.as_deref() == Some("implement-auth")
                    && description.as_deref() == Some("Implement the plan")
                    && secret == &["GH_WORK"]
                    && *budget_cost == Some(2.5)
                    && *idle_threshold == Some(30)
                    && *detach
        ));
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
        // `--node` is global → `list` parses as the List subcommand, not the quick-spawn path.
        let cli = Cli::try_parse_from(["pulpo", "--node", "win-pc:8080", "list"]).unwrap();
        assert_eq!(cli.node, "win-pc:8080");
        assert!(cli.path.is_none());
        assert!(matches!(cli.command, Some(Commands::List { .. })));
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
        let handoff_json = test_create_response_json();

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
                "/api/v1/sessions/{id}/handoff",
                post(move || async move { (StatusCode::CREATED, handoff_json.clone()) }),
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

                worktree: false,
                worktree_base: None,
                secret: vec![],
                budget_cost: None,
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

                worktree: false,
                worktree_base: None,
                secret: vec![],
                budget_cost: None,
                command: vec!["claude".into(), "-p".into(), "Fix bug".into()],
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("Created session"));
    }

    #[tokio::test]
    async fn test_execute_spawn_with_idle_threshold_and_worktree() {
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

                worktree: true,
                worktree_base: None,
                secret: vec![],
                budget_cost: None,
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

                worktree: false,
                worktree_base: None,
                secret: vec![],
                budget_cost: None,
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

                worktree: false,
                worktree_base: None,
                secret: vec![],
                budget_cost: None,
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

                worktree: false,
                worktree_base: None,
                secret: vec![],
                budget_cost: None,
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

                worktree: false,
                worktree_base: None,
                secret: vec![],
                budget_cost: None,
                command: vec!["claude".into(), "-p".into(), "Fix bug".into()],
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        // When not detached, spawn prints creation to stderr and returns detach message
        assert!(result.contains("Detached from session"));
    }

    #[tokio::test]
    async fn test_execute_handoff_detached() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Handoff {
                source: "plan-auth".into(),
                name: None,
                description: None,
                secret: vec![],
                budget_cost: None,
                idle_threshold: None,
                detach: true,
                command: vec!["codex".into(), "implement".into()],
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("Created session"));
    }

    #[tokio::test]
    async fn test_execute_handoff_with_all_flags() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Handoff {
                source: "plan-auth".into(),
                name: Some("implement-auth".into()),
                description: Some("Implement the plan".into()),
                secret: vec!["GH_WORK".into()],
                budget_cost: Some(2.5),
                idle_threshold: Some(30),
                detach: true,
                command: vec![],
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("Created session"));
    }

    #[tokio::test]
    async fn test_execute_handoff_auto_attach() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Handoff {
                source: "plan-auth".into(),
                name: None,
                description: None,
                secret: vec![],
                budget_cost: None,
                idle_threshold: None,
                detach: false,
                command: vec!["codex".into(), "implement".into()],
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("Detached from session"));
    }

    #[tokio::test]
    async fn test_execute_handoff_connection_refused() {
        let cli = Cli {
            node: "localhost:1".into(),
            token: None,
            command: Some(Commands::Handoff {
                source: "plan-auth".into(),
                name: None,
                description: None,
                secret: vec![],
                budget_cost: None,
                idle_threshold: None,
                detach: true,
                command: vec![],
            }),
            path: None,
        };
        let result = execute(&cli).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Could not connect to pulpod"));
    }

    #[tokio::test]
    async fn test_execute_handoff_error_response() {
        use axum::{Router, http::StatusCode, routing::post};

        let app = Router::new().route(
            "/api/v1/sessions/{id}/handoff",
            post(|| async {
                (
                    StatusCode::NOT_FOUND,
                    "{\"error\":\"session not found: plan-auth\"}",
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
            command: Some(Commands::Handoff {
                source: "plan-auth".into(),
                name: None,
                description: None,
                secret: vec![],
                budget_cost: None,
                idle_threshold: None,
                detach: true,
                command: vec![],
            }),
            path: None,
        };
        let err = execute(&cli).await.unwrap_err();
        assert_eq!(err.to_string(), "session not found: plan-auth");
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

    /// Regression: `pulpo ink list` (and the other schedule/secret/worktree/ink
    /// subcommands) used bare `.send().await?` without `friendly_error`, so a
    /// stopped daemon printed a raw reqwest error. Routing them through the
    /// shared request helpers fixed that. Gated `not(coverage)` because the
    /// sub-executors are stubbed out under coverage builds.
    #[cfg(not(coverage))]
    #[tokio::test]
    async fn test_execute_ink_list_connection_refused() {
        let cli = Cli {
            node: "localhost:1".into(),
            token: None,
            command: Some(Commands::Ink {
                action: InkAction::List,
            }),
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

    /// Same regression coverage for `pulpo schedule list`.
    #[cfg(not(coverage))]
    #[tokio::test]
    async fn test_execute_schedule_list_connection_refused() {
        let cli = Cli {
            node: "localhost:1".into(),
            token: None,
            command: Some(Commands::Schedule {
                action: ScheduleAction::List,
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

                worktree: false,
                worktree_base: None,
                secret: vec![],
                budget_cost: None,
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

                worktree: false,
                worktree_base: None,
                secret: vec![],
                budget_cost: None,
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

    // -- Auth helper tests --

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
    fn test_cli_parse_usage() {
        let cli = Cli::try_parse_from(["pulpo", "usage"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Usage {
                scan: false,
                by_worktree: false,
                since: None,
                json: false
            })
        ));
    }

    #[test]
    fn test_cli_parse_usage_scan() {
        let cli = Cli::try_parse_from(["pulpo", "usage", "--scan"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Usage {
                scan: true,
                by_worktree: false,
                since: None,
                json: false
            })
        ));
    }

    #[test]
    fn test_cli_parse_usage_scan_by_worktree() {
        let cli = Cli::try_parse_from(["pulpo", "usage", "--scan", "--by-worktree"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Usage {
                scan: true,
                by_worktree: true,
                ..
            })
        ));
    }

    #[test]
    fn test_cli_parse_usage_scan_since_and_json() {
        let cli =
            Cli::try_parse_from(["pulpo", "usage", "--scan", "--since", "7", "--json"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Usage {
                scan: true,
                since: Some(7),
                json: true,
                ..
            })
        ));
    }

    #[test]
    fn test_cli_parse_usage_json_without_scan() {
        // --json is allowed on the plain (projection) usage command too.
        let cli = Cli::try_parse_from(["pulpo", "usage", "--json"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Usage {
                scan: false,
                json: true,
                ..
            })
        ));
    }

    #[test]
    fn test_cli_parse_usage_by_worktree_requires_scan() {
        // --by-worktree and --since without --scan are rejected by clap (requires = "scan").
        assert!(Cli::try_parse_from(["pulpo", "usage", "--by-worktree"]).is_err());
        assert!(Cli::try_parse_from(["pulpo", "usage", "--since", "7"]).is_err());
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
    fn test_is_safe_term() {
        assert!(is_safe_term("xterm-256color"));
        assert!(is_safe_term("xterm"));
        assert!(is_safe_term("screen"));
        assert!(is_safe_term("screen-256color"));
        assert!(is_safe_term("tmux"));
        assert!(is_safe_term("tmux-256color"));
        assert!(is_safe_term("linux"));
        assert!(is_safe_term("vt100"));
        assert!(is_safe_term("dumb"));
        assert!(!is_safe_term("xterm-ghostty"));
        assert!(!is_safe_term("alacritty"));
        assert!(!is_safe_term(""));
        assert!(!is_safe_term("rxvt-unicode-256color"));
        assert!(!is_safe_term("wezterm"));
    }

    #[test]
    fn test_build_attach_command_uses_tmux_for_historical_docker_ids() {
        // Historical docker backend IDs no longer get a docker exec — the
        // docker runtime was removed, so attach always goes through tmux.
        let cmd = build_attach_command("docker:pulpo-my-task");
        assert_eq!(cmd.get_program(), "tmux");
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
        // `--node` is the global connection flag: the schedule is created directly
        // on that node's pulpod and fires locally there.
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
        assert_eq!(cli.node, "gpu-box");
        assert!(matches!(
            &cli.command,
            Some(Commands::Schedule {
                action: ScheduleAction::Add { name, .. }
            }) if name == "nightly"
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

    // -- Ink CLI parsing tests --

    #[test]
    fn test_cli_parse_ink_list() {
        let cli = Cli::try_parse_from(["pulpo", "ink", "list"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Ink {
                action: InkAction::List
            })
        ));
    }

    #[test]
    fn test_cli_parse_ink_list_alias() {
        let cli = Cli::try_parse_from(["pulpo", "ink", "ls"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Ink {
                action: InkAction::List
            })
        ));
    }

    #[test]
    fn test_cli_parse_ink_get() {
        let cli = Cli::try_parse_from(["pulpo", "ink", "get", "coder"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Ink {
                action: InkAction::Get { name }
            }) if name == "coder"
        ));
    }

    #[test]
    fn test_cli_parse_ink_add() {
        let cli = Cli::try_parse_from([
            "pulpo",
            "ink",
            "add",
            "coder",
            "--description",
            "A coder ink",
            "--command",
            "claude -p 'code'",
            "--secret",
            "GH_TOKEN",
            "--secret",
            "NPM_TOKEN",
        ])
        .unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Ink {
                action: InkAction::Add { name, description, command, secret }
            }) if name == "coder"
                && description.as_deref() == Some("A coder ink")
                && command.as_deref() == Some("claude -p 'code'")
                && secret == &["GH_TOKEN", "NPM_TOKEN"]
        ));
    }

    #[test]
    fn test_cli_parse_ink_add_runtime_flag_removed() {
        // --runtime was removed along with the docker session runtime
        let result = Cli::try_parse_from(["pulpo", "ink", "add", "coder", "--runtime", "docker"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_parse_ink_add_minimal() {
        let cli = Cli::try_parse_from(["pulpo", "ink", "add", "bare"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Ink {
                action: InkAction::Add { name, command, secret, .. }
            }) if name == "bare" && command.is_none() && secret.is_empty()
        ));
    }

    #[test]
    fn test_cli_parse_ink_update() {
        let cli = Cli::try_parse_from(["pulpo", "ink", "update", "coder", "--command", "new cmd"])
            .unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Ink {
                action: InkAction::Update { name, command, .. }
            }) if name == "coder" && command.as_deref() == Some("new cmd")
        ));
    }

    #[test]
    fn test_cli_parse_ink_remove() {
        let cli = Cli::try_parse_from(["pulpo", "ink", "remove", "coder"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Ink {
                action: InkAction::Remove { name }
            }) if name == "coder"
        ));
    }

    #[test]
    fn test_cli_parse_ink_remove_alias() {
        let cli = Cli::try_parse_from(["pulpo", "ink", "rm", "coder"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Ink {
                action: InkAction::Remove { name }
            }) if name == "coder"
        ));
    }

    #[test]
    fn test_ink_action_debug() {
        let action = InkAction::List;
        let debug = format!("{action:?}");
        assert!(debug.contains("List"));
    }

    // -- Ink format tests --

    // -- Schedule CLI new flags tests --

    #[test]
    fn test_cli_parse_schedule_add_with_flags() {
        let cli = Cli::try_parse_from([
            "pulpo",
            "schedule",
            "add",
            "nightly",
            "0 3 * * *",
            "--secret",
            "GH_TOKEN",
            "--worktree",
            "--worktree-base",
            "main",
        ])
        .unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Schedule {
                action: ScheduleAction::Add { name, secret, worktree, worktree_base, .. }
            }) if name == "nightly"
                && secret == &["GH_TOKEN"]
                && *worktree
                && worktree_base.as_deref() == Some("main")
        ));
    }

    #[test]
    fn test_cli_parse_schedule_add_runtime_flag_removed() {
        // --runtime was removed along with the docker session runtime
        let result = Cli::try_parse_from([
            "pulpo",
            "schedule",
            "add",
            "nightly",
            "0 3 * * *",
            "--runtime",
            "docker",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_parse_schedule_add_worktree_base_implies_worktree() {
        let cli = Cli::try_parse_from([
            "pulpo",
            "schedule",
            "add",
            "nightly",
            "0 3 * * *",
            "--worktree-base",
            "develop",
        ])
        .unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Schedule {
                action: ScheduleAction::Add { worktree, worktree_base, .. }
            }) if !worktree && worktree_base.as_deref() == Some("develop")
        ));
    }

    // -- format_local_time tests --

    // -- build_ink_body tests --

    #[test]
    fn test_build_ink_body_empty() {
        let body = build_ink_body(&serde_json::json!({}), None, None, &[]);
        assert_eq!(body, serde_json::json!({}));
    }

    #[test]
    fn test_build_ink_body_all_fields() {
        let body = build_ink_body(
            &serde_json::json!({}),
            Some(&"desc".into()),
            Some(&"cmd".into()),
            &["S1".into(), "S2".into()],
        );
        assert_eq!(body["description"], "desc");
        assert_eq!(body["command"], "cmd");
        assert_eq!(body["secrets"], serde_json::json!(["S1", "S2"]));
    }

    #[test]
    fn test_build_ink_body_merges_with_base() {
        let base = serde_json::json!({"description": "old", "command": "old_cmd"});
        let body = build_ink_body(&base, None, Some(&"new_cmd".into()), &[]);
        // description preserved from base, command overridden
        assert_eq!(body["description"], "old");
        assert_eq!(body["command"], "new_cmd");
    }

    // -- format_inks multibyte test --

    // -- format_token_count tests --

    // -- format_usage tests --
}
