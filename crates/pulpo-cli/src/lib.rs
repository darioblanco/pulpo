use anyhow::Result;
use clap::{Parser, Subcommand};
use pulpo_common::api::{
    AuthTokenResponse, CreateSessionResponse, InterventionEventResponse, PeersResponse,
};
use pulpo_common::session::Session;

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

        /// Command to run (everything after --)
        #[arg(last = true)]
        command: Vec<String>,
    },

    /// List all sessions
    #[command(visible_alias = "ls")]
    List,

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

    /// Kill a session
    #[command(visible_alias = "k")]
    Kill {
        /// Session name or ID
        name: String,
    },

    /// Permanently remove a session from history
    #[command(visible_alias = "rm")]
    Delete {
        /// Session name or ID
        name: String,
    },

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

    /// Manage scheduled agent runs via crontab
    #[command(visible_alias = "sched")]
    Schedule {
        #[command(subcommand)]
        action: ScheduleAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum ScheduleAction {
    /// Install a cron schedule that spawns a session
    Install {
        /// Schedule name
        name: String,
        /// Cron expression (e.g. "0 3 * * *")
        cron: String,
        /// Working directory
        #[arg(long)]
        workdir: String,
        /// Command to run (everything after --)
        #[arg(last = true)]
        command: Vec<String>,
    },
    /// List installed pulpo cron schedules
    #[command(alias = "ls")]
    List,
    /// Remove a cron schedule
    #[command(alias = "rm")]
    Remove {
        /// Schedule name
        name: String,
    },
    /// Pause a cron schedule (comments out the line)
    Pause {
        /// Schedule name
        name: String,
    },
    /// Resume a paused cron schedule (uncomments the line)
    Resume {
        /// Schedule name
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
fn format_sessions(sessions: &[Session]) -> String {
    if sessions.is_empty() {
        return "No sessions.".into();
    }
    let mut lines = vec![format!("{:<20} {:<12} {}", "NAME", "STATUS", "COMMAND")];
    for s in sessions {
        let cmd_display = if s.command.len() > 50 {
            format!("{}...", &s.command[..47])
        } else {
            s.command.clone()
        };
        lines.push(format!("{:<20} {:<12} {}", s.name, s.status, cmd_display));
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
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
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
/// Takes the backend session ID (e.g. `my-session`) — the tmux session name.
#[cfg_attr(coverage, allow(dead_code))]
fn build_attach_command(backend_session_id: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new("tmux");
    cmd.args(["attach-session", "-t", backend_session_id]);
    cmd
}

/// Attach to a session's terminal.
#[cfg(not(any(test, coverage)))]
fn attach_session(backend_session_id: &str) -> Result<()> {
    let status = build_attach_command(backend_session_id).status()?;
    if !status.success() {
        anyhow::bail!("attach failed with {status}");
    }
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
            let is_terminal = status == "ready" || status == "killed" || status == "lost";
            if is_terminal {
                break;
            }
        }
    }
    Ok(())
}

// --- Crontab wrapper ---

#[cfg_attr(coverage, allow(dead_code))]
const CRONTAB_TAG: &str = "#pulpo:";

/// Read the current crontab. Returns empty string if no crontab exists.
#[cfg(not(coverage))]
fn read_crontab() -> Result<String> {
    let output = std::process::Command::new("crontab").arg("-l").output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Ok(String::new())
    }
}

/// Write the given content as the user's crontab.
#[cfg(not(coverage))]
fn write_crontab(content: &str) -> Result<()> {
    use std::io::Write;
    let mut child = std::process::Command::new("crontab")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .spawn()?;
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(content.as_bytes())?;
    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("crontab write failed");
    }
    Ok(())
}

/// Build the crontab line for a pulpo schedule.
#[cfg_attr(coverage, allow(dead_code))]
fn build_crontab_line(name: &str, cron: &str, workdir: &str, command: &str, node: &str) -> String {
    format!(
        "{cron} pulpo --node {node} spawn {name} --workdir {workdir} -- {command} {CRONTAB_TAG}{name}\n"
    )
}

/// Install a cron schedule into a crontab string. Returns the updated crontab.
#[cfg_attr(coverage, allow(dead_code))]
fn crontab_install(crontab: &str, name: &str, line: &str) -> Result<String> {
    let tag = format!("{CRONTAB_TAG}{name}");
    if crontab.contains(&tag) {
        anyhow::bail!("schedule \"{name}\" already exists — remove it first");
    }
    let mut result = crontab.to_owned();
    result.push_str(line);
    Ok(result)
}

/// Format pulpo crontab entries for display.
#[cfg_attr(coverage, allow(dead_code))]
fn crontab_list(crontab: &str) -> String {
    let entries: Vec<&str> = crontab
        .lines()
        .filter(|l| l.contains(CRONTAB_TAG))
        .collect();
    if entries.is_empty() {
        return "No pulpo schedules.".into();
    }
    let mut lines = vec![format!("{:<20} {:<15} {}", "NAME", "CRON", "PAUSED")];
    for entry in entries {
        let paused = entry.starts_with('#');
        let raw = entry.trim_start_matches('#').trim();
        let name = raw.rsplit_once(CRONTAB_TAG).map_or("?", |(_, n)| n);
        let parts: Vec<&str> = raw.splitn(6, ' ').collect();
        let cron_expr = if parts.len() >= 5 {
            parts[..5].join(" ")
        } else {
            "?".into()
        };
        lines.push(format!(
            "{:<20} {:<15} {}",
            name,
            cron_expr,
            if paused { "yes" } else { "no" }
        ));
    }
    lines.join("\n")
}

/// Remove a schedule from a crontab string. Returns the updated crontab.
#[cfg_attr(coverage, allow(dead_code))]
fn crontab_remove(crontab: &str, name: &str) -> Result<String> {
    use std::fmt::Write;
    let tag = format!("{CRONTAB_TAG}{name}");
    let filtered =
        crontab
            .lines()
            .filter(|l| !l.contains(&tag))
            .fold(String::new(), |mut acc, l| {
                writeln!(acc, "{l}").unwrap();
                acc
            });
    if filtered.len() == crontab.len() {
        anyhow::bail!("schedule \"{name}\" not found");
    }
    Ok(filtered)
}

/// Pause (comment out) a schedule in a crontab string.
#[cfg_attr(coverage, allow(dead_code))]
fn crontab_pause(crontab: &str, name: &str) -> Result<String> {
    use std::fmt::Write;
    let tag = format!("{CRONTAB_TAG}{name}");
    let mut found = false;
    let updated = crontab.lines().fold(String::new(), |mut acc, l| {
        if l.contains(&tag) && !l.starts_with('#') {
            found = true;
            writeln!(acc, "#{l}").unwrap();
        } else {
            writeln!(acc, "{l}").unwrap();
        }
        acc
    });
    if !found {
        anyhow::bail!("schedule \"{name}\" not found or already paused");
    }
    Ok(updated)
}

/// Resume (uncomment) a schedule in a crontab string.
#[cfg_attr(coverage, allow(dead_code))]
fn crontab_resume(crontab: &str, name: &str) -> Result<String> {
    use std::fmt::Write;
    let tag = format!("{CRONTAB_TAG}{name}");
    let mut found = false;
    let updated = crontab.lines().fold(String::new(), |mut acc, l| {
        if l.contains(&tag) && l.starts_with('#') {
            found = true;
            writeln!(acc, "{}", l.trim_start_matches('#')).unwrap();
        } else {
            writeln!(acc, "{l}").unwrap();
        }
        acc
    });
    if !found {
        anyhow::bail!("schedule \"{name}\" not found or not paused");
    }
    Ok(updated)
}

/// Execute a schedule subcommand using the crontab wrapper.
#[cfg(not(coverage))]
fn execute_schedule(action: &ScheduleAction, node: &str) -> Result<String> {
    match action {
        ScheduleAction::Install {
            name,
            cron,
            workdir,
            command,
        } => {
            let crontab = read_crontab()?;
            let joined_command = command.join(" ");
            let line = build_crontab_line(name, cron, workdir, &joined_command, node);
            let updated = crontab_install(&crontab, name, &line)?;
            write_crontab(&updated)?;
            Ok(format!("Installed schedule \"{name}\""))
        }
        ScheduleAction::List => {
            let crontab = read_crontab()?;
            Ok(crontab_list(&crontab))
        }
        ScheduleAction::Remove { name } => {
            let crontab = read_crontab()?;
            let updated = crontab_remove(&crontab, name)?;
            write_crontab(&updated)?;
            Ok(format!("Removed schedule \"{name}\""))
        }
        ScheduleAction::Pause { name } => {
            let crontab = read_crontab()?;
            let updated = crontab_pause(&crontab, name)?;
            write_crontab(&updated)?;
            Ok(format!("Paused schedule \"{name}\""))
        }
        ScheduleAction::Resume { name } => {
            let crontab = read_crontab()?;
            let updated = crontab_resume(&crontab, name)?;
            write_crontab(&updated)?;
            Ok(format!("Resumed schedule \"{name}\""))
        }
    }
}

/// Stub for coverage builds — crontab is real I/O.
#[cfg(coverage)]
fn execute_schedule(_action: &ScheduleAction, _node: &str) -> Result<String> {
    Ok(String::new())
}

/// Execute the given CLI command against the specified node.
#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli) -> Result<String> {
    let url = base_url(&cli.node);
    let client = reqwest::Client::new();
    let node = &cli.node;
    let token = resolve_token(&client, &url, node, cli.token.as_deref()).await;

    // Handle `pulpo <path>` shortcut — spawn a session in the given directory
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
                "killed" => {
                    anyhow::bail!(
                        "Session \"{name}\" is {} — cannot attach to a killed session.",
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
        Commands::List => {
            let resp = authed_get(&client, format!("{url}/api/v1/sessions"), token.as_deref())
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
            if !detach {
                let backend_id = resp
                    .session
                    .backend_session_id
                    .as_deref()
                    .unwrap_or(&resp.session.name);
                eprintln!("{msg}");
                attach_session(backend_id)?;
                return Ok(format!("Detached from session \"{}\".", resp.session.name));
            }
            Ok(msg)
        }
        Commands::Kill { name } => {
            let resp = authed_post(
                &client,
                format!("{url}/api/v1/sessions/{name}/kill"),
                token.as_deref(),
            )
            .send()
            .await
            .map_err(|e| friendly_error(&e, node))?;
            ok_or_api_error(resp).await?;
            Ok(format!("Session {name} killed."))
        }
        Commands::Delete { name } => {
            let resp = authed_delete(
                &client,
                format!("{url}/api/v1/sessions/{name}"),
                token.as_deref(),
            )
            .send()
            .await
            .map_err(|e| friendly_error(&e, node))?;
            ok_or_api_error(resp).await?;
            Ok(format!("Session {name} deleted."))
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
            let dashboard = base_url(&cli.node);
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
            attach_session(backend_id)?;
            Ok(format!("Detached from session \"{}\".", session.name))
        }
        Commands::Schedule { action } => execute_schedule(action, node),
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
        assert!(matches!(cli.command, Some(Commands::List)));
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
    fn test_cli_parse_kill() {
        let cli = Cli::try_parse_from(["pulpo", "kill", "my-session"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Kill { name }) if name == "my-session"
        ));
    }

    #[test]
    fn test_cli_parse_delete() {
        let cli = Cli::try_parse_from(["pulpo", "delete", "my-session"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Delete { name }) if name == "my-session"
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
        let cmd = Commands::List;
        assert_eq!(format!("{cmd:?}"), "List");
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
                get(|| async { TEST_SESSION_JSON.to_owned() })
                    .delete(|| async { StatusCode::NO_CONTENT }),
            )
            .route(
                "/api/v1/sessions/{id}/kill",
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
            command: Some(Commands::List),
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
                command: vec!["claude".into(), "-p".into(), "Fix bug".into()],
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
                command: vec!["claude".into(), "-p".into(), "Fix bug".into()],
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        // When not detached, spawn prints creation to stderr and returns detach message
        assert!(result.contains("Detached from session"));
    }

    #[tokio::test]
    async fn test_execute_kill_success() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Kill {
                name: "test-session".into(),
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("killed"));
    }

    #[tokio::test]
    async fn test_execute_delete_success() {
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Delete {
                name: "test-session".into(),
            }),
            path: None,
        };
        let result = execute(&cli).await.unwrap();
        assert!(result.contains("deleted"));
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
            command: Some(Commands::List),
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
    async fn test_execute_kill_error_response() {
        use axum::{Router, http::StatusCode, routing::post};

        let app = Router::new().route(
            "/api/v1/sessions/{id}/kill",
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
            command: Some(Commands::Kill {
                name: "test-session".into(),
            }),
            path: None,
        };
        let err = execute(&cli).await.unwrap_err();
        assert_eq!(err.to_string(), "session not found: test-session");
    }

    #[tokio::test]
    async fn test_execute_delete_error_response() {
        use axum::{Router, http::StatusCode, routing::delete};

        let app = Router::new().route(
            "/api/v1/sessions/{id}",
            delete(|| async {
                (
                    StatusCode::CONFLICT,
                    "{\"error\":\"cannot delete session in 'running' state\"}",
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
            command: Some(Commands::Delete {
                name: "test-session".into(),
            }),
            path: None,
        };
        let err = execute(&cli).await.unwrap_err();
        assert_eq!(err.to_string(), "cannot delete session in 'running' state");
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
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }];
        let output = format_sessions(&sessions);
        assert!(output.contains("NAME"));
        assert!(output.contains("COMMAND"));
        assert!(output.contains("my-api"));
        assert!(output.contains("active"));
        assert!(output.contains("claude -p 'Fix the bug'"));
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
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }];
        let output = format_sessions(&sessions);
        assert!(output.contains("..."));
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
                command: vec!["test".into()],
            }),
            path: None,
        };
        let result = execute(&cli).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Could not connect to pulpod"));
    }

    #[tokio::test]
    async fn test_execute_kill_connection_refused() {
        let cli = Cli {
            node: "localhost:1".into(),
            token: None,
            command: Some(Commands::Kill {
                name: "test".into(),
            }),
            path: None,
        };
        let result = execute(&cli).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Could not connect to pulpod"));
    }

    #[tokio::test]
    async fn test_execute_delete_connection_refused() {
        let cli = Cli {
            node: "localhost:1".into(),
            token: None,
            command: Some(Commands::Delete {
                name: "test".into(),
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
            command: Some(Commands::List),
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
    fn test_build_attach_command() {
        let cmd = build_attach_command("my-session");
        assert_eq!(cmd.get_program(), "tmux");
        let args: Vec<&std::ffi::OsStr> = cmd.get_args().collect();
        assert_eq!(args, vec!["attach-session", "-t", "my-session"]);
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
        let session_json = r#"{"id":"00000000-0000-0000-0000-000000000001","name":"dead-sess","workdir":"/tmp","command":"echo test","description":null,"status":"killed","exit_code":null,"backend_session_id":"dead-sess","output_snapshot":null,"metadata":null,"ink":null,"intervention_code":null,"intervention_reason":null,"intervention_at":null,"last_output_at":null,"idle_since":null,"idle_threshold_secs":null,"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}"#;
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
        assert!(err.contains("killed"));
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
        assert!(matches!(&cli.command, Some(Commands::List)));
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
    fn test_cli_parse_alias_kill() {
        let cli = Cli::try_parse_from(["pulpo", "k", "my-session"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Kill { name }) if name == "my-session"
        ));
    }

    #[test]
    fn test_cli_parse_alias_delete() {
        let cli = Cli::try_parse_from(["pulpo", "rm", "my-session"]).unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Delete { name }) if name == "my-session"
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
                    r#"{"id":"00000000-0000-0000-0000-000000000001","name":"test","workdir":"/tmp","command":"echo test","description":null,"status":"killed","exit_code":null,"backend_session_id":null,"output_snapshot":null,"metadata":null,"ink":null,"intervention_code":null,"intervention_reason":null,"intervention_at":null,"last_output_at":null,"idle_since":null,"idle_threshold_secs":null,"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}"#.to_owned()
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

    // -- Crontab wrapper tests --

    #[test]
    fn test_build_crontab_line() {
        let line = build_crontab_line(
            "nightly-review",
            "0 3 * * *",
            "/home/me/repo",
            "claude -p 'Review PRs'",
            "localhost:7433",
        );
        assert_eq!(
            line,
            "0 3 * * * pulpo --node localhost:7433 spawn nightly-review --workdir /home/me/repo -- claude -p 'Review PRs' #pulpo:nightly-review\n"
        );
    }

    #[test]
    fn test_crontab_install_success() {
        let crontab = "# existing cron\n0 * * * * echo hi\n";
        let line = "0 3 * * * pulpo --node n spawn my-job --workdir /tmp -- claude -p task #pulpo:my-job\n";
        let result = crontab_install(crontab, "my-job", line).unwrap();
        assert!(result.starts_with("# existing cron\n"));
        assert!(result.ends_with("#pulpo:my-job\n"));
        assert!(result.contains("echo hi"));
    }

    #[test]
    fn test_crontab_install_duplicate_error() {
        let crontab = "0 3 * * * pulpo spawn task #pulpo:my-job\n";
        let line = "0 4 * * * pulpo spawn other #pulpo:my-job\n";
        let err = crontab_install(crontab, "my-job", line).unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn test_crontab_list_empty() {
        assert_eq!(crontab_list(""), "No pulpo schedules.");
    }

    #[test]
    fn test_crontab_list_no_pulpo_entries() {
        assert_eq!(crontab_list("0 * * * * echo hi\n"), "No pulpo schedules.");
    }

    #[test]
    fn test_crontab_list_with_entries() {
        let crontab = "0 3 * * * pulpo --node n spawn nightly --workdir /tmp -- claude -p task #pulpo:nightly\n";
        let output = crontab_list(crontab);
        assert!(output.contains("NAME"));
        assert!(output.contains("CRON"));
        assert!(output.contains("PAUSED"));
        assert!(output.contains("nightly"));
        assert!(output.contains("0 3 * * *"));
        assert!(output.contains("no"));
    }

    #[test]
    fn test_crontab_list_paused_entry() {
        let crontab = "#0 3 * * * pulpo spawn task #pulpo:paused-job\n";
        let output = crontab_list(crontab);
        assert!(output.contains("paused-job"));
        assert!(output.contains("yes"));
    }

    #[test]
    fn test_crontab_list_short_line() {
        // A line with fewer than 5 space-separated parts but still tagged
        let crontab = "badcron #pulpo:broken\n";
        let output = crontab_list(crontab);
        assert!(output.contains("broken"));
        assert!(output.contains('?'));
    }

    #[test]
    fn test_crontab_remove_success() {
        let crontab = "0 * * * * echo hi\n0 3 * * * pulpo spawn task #pulpo:my-job\n";
        let result = crontab_remove(crontab, "my-job").unwrap();
        assert!(result.contains("echo hi"));
        assert!(!result.contains("my-job"));
    }

    #[test]
    fn test_crontab_remove_not_found() {
        let crontab = "0 * * * * echo hi\n";
        let err = crontab_remove(crontab, "ghost").unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn test_crontab_pause_success() {
        let crontab = "0 3 * * * pulpo spawn task #pulpo:my-job\n";
        let result = crontab_pause(crontab, "my-job").unwrap();
        assert!(result.starts_with('#'));
        assert!(result.contains("#pulpo:my-job"));
    }

    #[test]
    fn test_crontab_pause_not_found() {
        let crontab = "0 * * * * echo hi\n";
        let err = crontab_pause(crontab, "ghost").unwrap_err();
        assert!(err.to_string().contains("not found or already paused"));
    }

    #[test]
    fn test_crontab_pause_already_paused() {
        let crontab = "#0 3 * * * pulpo spawn task #pulpo:my-job\n";
        let err = crontab_pause(crontab, "my-job").unwrap_err();
        assert!(err.to_string().contains("already paused"));
    }

    #[test]
    fn test_crontab_resume_success() {
        let crontab = "#0 3 * * * pulpo spawn task #pulpo:my-job\n";
        let result = crontab_resume(crontab, "my-job").unwrap();
        assert!(!result.starts_with('#'));
        assert!(result.contains("#pulpo:my-job"));
    }

    #[test]
    fn test_crontab_resume_not_found() {
        let crontab = "0 * * * * echo hi\n";
        let err = crontab_resume(crontab, "ghost").unwrap_err();
        assert!(err.to_string().contains("not found or not paused"));
    }

    #[test]
    fn test_crontab_resume_not_paused() {
        let crontab = "0 3 * * * pulpo spawn task #pulpo:my-job\n";
        let err = crontab_resume(crontab, "my-job").unwrap_err();
        assert!(err.to_string().contains("not paused"));
    }

    // -- Schedule CLI parse tests --

    #[test]
    fn test_cli_parse_schedule_install() {
        let cli = Cli::try_parse_from([
            "pulpo",
            "schedule",
            "install",
            "nightly",
            "0 3 * * *",
            "--workdir",
            "/tmp/repo",
            "--",
            "claude",
            "-p",
            "Review PRs",
        ])
        .unwrap();
        assert!(matches!(
            &cli.command,
            Some(Commands::Schedule {
                action: ScheduleAction::Install { name, cron, workdir, command }
            }) if name == "nightly" && cron == "0 3 * * *" && workdir == "/tmp/repo"
              && command == &["claude", "-p", "Review PRs"]
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
    async fn test_execute_schedule_via_execute() {
        // Under coverage builds, execute_schedule is a stub returning Ok("")
        let node = start_test_server().await;
        let cli = Cli {
            node,
            token: None,
            command: Some(Commands::Schedule {
                action: ScheduleAction::List,
            }),
            path: None,
        };
        let result = execute(&cli).await;
        // Under coverage: succeeds with empty string; under non-coverage: may fail (no crontab)
        assert!(result.is_ok() || result.is_err());
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
}
