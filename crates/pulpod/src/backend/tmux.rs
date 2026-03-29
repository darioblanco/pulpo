#[cfg(not(coverage))]
use anyhow::{Context, Result, bail};
use std::process::Command;
#[cfg(not(coverage))]
use std::process::{Output, Stdio};

#[cfg(not(coverage))]
use super::Backend;

/// Well-known tmux binary locations for macOS/Linux. Checked in order when
/// `tmux` is not on `PATH` (common in launchd/systemd contexts after reboot).
const TMUX_SEARCH_PATHS: &[&str] = &[
    "/opt/homebrew/bin/tmux",              // macOS Apple Silicon (Homebrew)
    "/usr/local/bin/tmux",                 // macOS Intel (Homebrew) / Linux manual install
    "/usr/bin/tmux",                       // Linux distro package
    "/home/linuxbrew/.linuxbrew/bin/tmux", // Linux Homebrew
];

/// Resolve the absolute path to the `tmux` binary.
///
/// Tries `PATH` first (via `Command::new("tmux")`), then falls back to
/// well-known locations. This ensures `pulpod` can find tmux even when
/// launched by launchd/systemd before the full user `PATH` is available.
#[cfg_attr(coverage, allow(dead_code))]
fn resolve_tmux_path() -> String {
    // Try PATH first
    if let Ok(output) = Command::new("which").arg("tmux").output()
        && output.status.success()
    {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !path.is_empty() {
            return path;
        }
    }

    // Fall back to well-known locations
    for path in TMUX_SEARCH_PATHS {
        if std::path::Path::new(path).exists() {
            return (*path).to_owned();
        }
    }

    // Last resort: hope it's on PATH at runtime
    "tmux".to_owned()
}

#[cfg_attr(coverage, allow(dead_code))]
fn tmux_session_name(name: &str) -> String {
    name.to_owned()
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_create_command(
    tmux: &str,
    session_name: &str,
    working_dir: &str,
    command: &str,
    user_path: Option<&str>,
) -> Command {
    let mut cmd = Command::new(tmux);
    // Set the user's full PATH on the tmux process so the tmux server (and all
    // sessions it spawns) inherit it. This avoids quoting issues with embedding
    // PATH inside the shell command string, and works regardless of shell type.
    if let Some(path) = user_path {
        cmd.env("PATH", path);
    }
    cmd.args([
        "new-session",
        "-d",
        "-s",
        session_name,
        "-c",
        working_dir,
        command,
    ]);
    cmd
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_set_mouse_command(tmux: &str, session_name: &str) -> Command {
    let mut cmd = Command::new(tmux);
    cmd.args(["set-option", "-t", session_name, "mouse", "on"]);
    cmd
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_set_clipboard_command(tmux: &str, session_name: &str) -> Command {
    let mut cmd = Command::new(tmux);
    cmd.args(["set-option", "-t", session_name, "set-clipboard", "on"]);
    cmd
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_allow_passthrough_command(tmux: &str, session_name: &str) -> Command {
    let mut cmd = Command::new(tmux);
    cmd.args(["set-option", "-t", session_name, "allow-passthrough", "on"]);
    cmd
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_kill_command(tmux: &str, session_name: &str) -> Command {
    let mut cmd = Command::new(tmux);
    cmd.args(["kill-session", "-t", session_name]);
    cmd
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_has_session_command(tmux: &str, session_name: &str) -> Command {
    let mut cmd = Command::new(tmux);
    cmd.args(["has-session", "-t", session_name]);
    cmd
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_capture_command(tmux: &str, session_name: &str, lines: usize) -> Command {
    let start = -i64::try_from(lines).unwrap_or(i64::MAX);
    let mut cmd = Command::new(tmux);
    cmd.args([
        "capture-pane",
        "-t",
        session_name,
        "-p",
        "-S",
        &start.to_string(),
    ]);
    cmd
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_send_keys_command(tmux: &str, session_name: &str, text: &str) -> Command {
    let mut cmd = Command::new(tmux);
    cmd.args(["send-keys", "-t", session_name, text, "Enter"]);
    cmd
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_pipe_pane_command(tmux: &str, session_name: &str, log_path: &str) -> Command {
    let mut cmd = Command::new(tmux);
    cmd.args([
        "pipe-pane",
        "-t",
        session_name,
        "-o",
        &format!("cat >> {log_path}"),
    ]);
    cmd
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_resize_command(tmux: &str, session_name: &str, cols: u16, rows: u16) -> Command {
    let mut cmd = Command::new(tmux);
    cmd.args([
        "resize-window",
        "-t",
        session_name,
        "-x",
        &cols.to_string(),
        "-y",
        &rows.to_string(),
    ]);
    cmd
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_list_sessions_command(tmux: &str) -> Command {
    let mut cmd = Command::new(tmux);
    cmd.args(["list-sessions", "-F", "#{session_id}\t#{session_name}"]);
    cmd
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_pane_info_command(tmux: &str, session_name: &str) -> Command {
    let mut cmd = Command::new(tmux);
    cmd.args([
        "list-panes",
        "-t",
        session_name,
        "-F",
        "#{pane_current_command}\t#{pane_current_path}",
    ]);
    cmd
}

/// Parse `list-sessions -F '#{session_id}\t#{session_name}'` output into (id, name) pairs.
pub fn parse_list_sessions(output: &str) -> Vec<(String, String)> {
    output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|l| {
            let (id, name) = l.split_once('\t')?;
            Some((id.to_owned(), name.to_owned()))
        })
        .collect()
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_query_session_id_command(tmux: &str, name: &str) -> Command {
    let mut cmd = Command::new(tmux);
    cmd.args(["display-message", "-t", name, "-p", "#{session_id}"]);
    cmd
}

/// Parse `display-message -p '#{session_id}'` output to `$N`.
pub fn parse_session_id(output: &str) -> Option<String> {
    let trimmed = output.trim();
    if trimmed.starts_with('$') {
        Some(trimmed.to_owned())
    } else {
        None
    }
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_pane_pid_command(tmux: &str, backend_id: &str) -> Command {
    let mut cmd = Command::new(tmux);
    cmd.args(["list-panes", "-t", backend_id, "-F", "#{pane_pid}"]);
    cmd
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_set_env_command(tmux: &str, session_name: &str, key: &str, value: &str) -> Command {
    let mut cmd = Command::new(tmux);
    cmd.args(["set-environment", "-t", session_name, key, value]);
    cmd
}

/// Parse `list-panes -F '#{pane_current_command}\t#{pane_current_path}'` output.
/// Returns `(process_name, working_dir)` from the first pane.
pub fn parse_pane_info(output: &str) -> Option<(String, String)> {
    let line = output.lines().next()?;
    let (process, path) = line.split_once('\t')?;
    Some((process.to_owned(), path.to_owned()))
}

/// Parse tmux version from `tmux -V` output (e.g., "tmux 3.4" -> `Some((3, 4))`).
///
/// Handles formats like "tmux 3.4", "tmux 3.2a", "tmux next-3.4".
pub fn parse_tmux_version(output: &str) -> Option<(u32, u32)> {
    let version_str = output.strip_prefix("tmux ")?;
    let numeric = version_str.trim_start_matches("next-");
    let mut parts = numeric.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor_str = parts.next().unwrap_or("0");
    let minor = minor_str
        .trim_end_matches(|c: char| c.is_alphabetic())
        .parse::<u32>()
        .ok()?;
    Some((major, minor))
}

/// Check tmux is installed and >= 3.2. Returns `Ok(version string)` or `Err`.
#[cfg(not(coverage))]
fn check_tmux_version(tmux_path: &str) -> anyhow::Result<String> {
    let output = Command::new(tmux_path)
        .arg("-V")
        .output()
        .context("tmux not found — install tmux 3.2+ to use pulpo")?;
    let version_string = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    match parse_tmux_version(&version_string) {
        Some((major, minor)) if major > 3 || (major == 3 && minor >= 2) => Ok(version_string),
        Some((major, minor)) => {
            anyhow::bail!("tmux {major}.{minor} is too old — pulpo requires tmux 3.2+")
        }
        None => {
            anyhow::bail!("Could not parse tmux version from: {version_string}")
        }
    }
}

#[allow(dead_code)]
pub struct TmuxBackend {
    /// Absolute path to the tmux binary, resolved at construction time.
    tmux_path: String,
    /// The user's full login-shell PATH, probed at construction time.
    /// When pulpod runs as a launchd/systemd service the inherited PATH is
    /// minimal and may not include directories like `~/.local/bin` or
    /// `~/.cargo/bin`. We resolve the real PATH once and set it on every
    /// tmux command so sessions can find tools like `claude`.
    user_path: Option<String>,
}

#[allow(dead_code)]
impl Default for TmuxBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
impl TmuxBackend {
    pub fn new() -> Self {
        Self {
            tmux_path: resolve_tmux_path(),
            user_path: resolve_user_path(),
        }
    }
}

/// Probe the user's full PATH by spawning their login shell.
///
/// Runs `$SHELL -lic 'printf %s "$PATH"'` which sources both login profiles
/// (`.zprofile`/`.bash_profile`) and interactive rc files (`.zshrc`/`.bashrc`).
/// This is shell-agnostic: works with zsh, bash, fish, nu, etc.
/// Returns `None` if the probe fails (e.g. `$SHELL` is unset or broken).
#[cfg_attr(coverage, allow(dead_code))]
fn resolve_user_path() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_owned());
    let output = Command::new(&shell)
        .args(["-lic", r#"printf '%s' "$PATH""#])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if path.is_empty() { None } else { Some(path) }
}

#[cfg(not(coverage))]
/// Run a tmux command, capturing output and suppressing stderr from the daemon log.
/// Returns an error with the tmux stderr message if the command exits non-zero.
fn run_tmux(mut cmd: Command, context: &str) -> Result<Output> {
    let output = cmd
        .stderr(Stdio::piped())
        .output()
        .context(format!("Failed to spawn tmux for {context}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{context}: {}", stderr.trim());
    }
    Ok(output)
}

#[cfg(not(coverage))]
impl Backend for TmuxBackend {
    fn session_id(&self, name: &str) -> String {
        tmux_session_name(name)
    }

    fn check_version(&self) -> Result<String> {
        check_tmux_version(&self.tmux_path)
    }

    fn create_session(&self, backend_id: &str, working_dir: &str, command: &str) -> Result<()> {
        run_tmux(
            build_create_command(
                &self.tmux_path,
                backend_id,
                working_dir,
                command,
                self.user_path.as_deref(),
            ),
            "create tmux session",
        )?;
        // Best-effort session options — if the command exits instantly and kills
        // the tmux server, these will fail but the session was already dead anyway.
        let _ = run_tmux(
            build_set_mouse_command(&self.tmux_path, backend_id),
            "enable tmux mouse mode",
        );
        let _ = run_tmux(
            build_set_clipboard_command(&self.tmux_path, backend_id),
            "enable tmux clipboard",
        );
        let _ = run_tmux(
            build_allow_passthrough_command(&self.tmux_path, backend_id),
            "enable tmux passthrough",
        );
        Ok(())
    }

    fn kill_session(&self, backend_id: &str) -> Result<()> {
        run_tmux(
            build_kill_command(&self.tmux_path, backend_id),
            "kill tmux session",
        )?;
        Ok(())
    }

    fn is_alive(&self, backend_id: &str) -> Result<bool> {
        let output = build_has_session_command(&self.tmux_path, backend_id)
            .stderr(Stdio::piped())
            .output()
            .context("Failed to check tmux session")?;
        Ok(output.status.success())
    }

    fn capture_output(&self, backend_id: &str, lines: usize) -> Result<String> {
        let output: Output = build_capture_command(&self.tmux_path, backend_id, lines)
            .stderr(Stdio::piped())
            .output()
            .context("Failed to capture tmux pane")?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn send_input(&self, backend_id: &str, text: &str) -> Result<()> {
        run_tmux(
            build_send_keys_command(&self.tmux_path, backend_id, text),
            "send input to tmux session",
        )?;
        Ok(())
    }

    fn setup_logging(&self, backend_id: &str, log_path: &str) -> Result<()> {
        run_tmux(
            build_pipe_pane_command(&self.tmux_path, backend_id, log_path),
            "setup pipe-pane logging",
        )?;
        Ok(())
    }

    fn resize(&self, backend_id: &str, cols: u16, rows: u16) -> Result<()> {
        run_tmux(
            build_resize_command(&self.tmux_path, backend_id, cols, rows),
            "resize tmux window",
        )?;
        Ok(())
    }

    fn spawn_attach(&self, backend_id: &str) -> Result<tokio::process::Child> {
        let mut cmd = tokio::process::Command::new("script");
        let tmux = &self.tmux_path;

        #[cfg(target_os = "macos")]
        cmd.args(["-q", "/dev/null", tmux, "attach-session", "-t", backend_id]);

        #[cfg(not(target_os = "macos"))]
        cmd.args([
            "-q",
            "-c",
            &format!("{tmux} attach-session -t {backend_id}"),
            "/dev/null",
        ]);

        cmd.env_remove("TMUX");
        cmd.env("TERM", "xterm-256color");
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());

        cmd.spawn().context("spawn script+tmux attach")
    }

    fn query_backend_id(&self, name: &str) -> Result<String> {
        let output = run_tmux(
            build_query_session_id_command(&self.tmux_path, name),
            "query tmux session ID",
        )?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_session_id(&stdout).context("Failed to parse tmux session ID")
    }

    fn pane_command_line(&self, backend_id: &str) -> Result<String> {
        let output = run_tmux(
            build_pane_pid_command(&self.tmux_path, backend_id),
            "get pane PID",
        )?;
        let pid = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if pid.is_empty() {
            anyhow::bail!("no pane PID for {backend_id}");
        }
        let ps_output = std::process::Command::new("ps")
            .args(["-o", "args=", "-p", &pid])
            .output()
            .context("failed to run ps")?;
        let cmd_line = String::from_utf8_lossy(&ps_output.stdout).trim().to_owned();
        if cmd_line.is_empty() {
            anyhow::bail!("no command line for PID {pid}");
        }
        Ok(cmd_line)
    }

    fn list_sessions(&self) -> Result<Vec<(String, String)>> {
        let output = build_list_sessions_command(&self.tmux_path)
            .stderr(Stdio::piped())
            .output()
            .context("Failed to list tmux sessions")?;
        if !output.status.success() {
            return Ok(Vec::new());
        }
        Ok(parse_list_sessions(&String::from_utf8_lossy(
            &output.stdout,
        )))
    }

    fn pane_info(&self, backend_id: &str) -> Result<(String, String)> {
        let output = run_tmux(
            build_pane_info_command(&self.tmux_path, backend_id),
            "get pane info",
        )?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_pane_info(&stdout).context("Failed to parse pane info")
    }

    fn set_env(&self, backend_id: &str, key: &str, value: &str) -> Result<()> {
        run_tmux(
            build_set_env_command(&self.tmux_path, backend_id, key, value),
            "set tmux environment variable",
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    const T: &str = "tmux";

    #[test]
    fn test_tmux_session_name() {
        assert_eq!(tmux_session_name("my-api"), "my-api");
        assert_eq!(tmux_session_name("indigo-wave"), "indigo-wave");
    }

    #[test]
    fn test_build_create_command() {
        let cmd = build_create_command(T, "pulpo-test", "/tmp/repo", "claude", None);
        assert_eq!(cmd.get_program(), T);
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(
            args,
            vec![
                "new-session",
                "-d",
                "-s",
                "pulpo-test",
                "-c",
                "/tmp/repo",
                "claude"
            ]
        );
    }

    #[test]
    fn test_build_create_command_with_user_path() {
        let cmd = build_create_command(
            T,
            "test",
            "/tmp",
            "claude",
            Some("/usr/local/bin:/usr/bin:/home/user/.local/bin"),
        );
        let envs: Vec<_> = cmd.get_envs().collect();
        let path_env = envs
            .iter()
            .find(|(k, _)| *k == "PATH")
            .expect("PATH should be set");
        assert_eq!(
            path_env.1.unwrap().to_str().unwrap(),
            "/usr/local/bin:/usr/bin:/home/user/.local/bin"
        );
    }

    #[test]
    fn test_build_kill_command() {
        let cmd = build_kill_command(T, "pulpo-test");
        assert_eq!(cmd.get_program(), T);
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(args, vec!["kill-session", "-t", "pulpo-test"]);
    }

    #[test]
    fn test_build_has_session_command() {
        let cmd = build_has_session_command(T, "pulpo-test");
        assert_eq!(cmd.get_program(), T);
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(args, vec!["has-session", "-t", "pulpo-test"]);
    }

    #[test]
    fn test_build_capture_command() {
        let cmd = build_capture_command(T, "pulpo-test", 100);
        assert_eq!(cmd.get_program(), T);
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(
            args,
            vec!["capture-pane", "-t", "pulpo-test", "-p", "-S", "-100"]
        );
    }

    #[test]
    fn test_build_capture_command_large_lines() {
        let cmd = build_capture_command(T, "pulpo-test", usize::MAX);
        let args: Vec<&OsStr> = cmd.get_args().collect();
        // Should handle overflow gracefully
        let start_str = args[5].to_str().unwrap();
        assert!(start_str.starts_with('-'));
    }

    #[test]
    fn test_build_set_mouse_command() {
        let cmd = build_set_mouse_command(T, "pulpo-test");
        assert_eq!(cmd.get_program(), T);
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(args, vec!["set-option", "-t", "pulpo-test", "mouse", "on"]);
    }

    #[test]
    fn test_build_set_clipboard_command() {
        let cmd = build_set_clipboard_command(T, "pulpo-test");
        assert_eq!(cmd.get_program(), T);
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(
            args,
            vec!["set-option", "-t", "pulpo-test", "set-clipboard", "on"]
        );
    }

    #[test]
    fn test_build_allow_passthrough_command() {
        let cmd = build_allow_passthrough_command(T, "pulpo-test");
        assert_eq!(cmd.get_program(), T);
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(
            args,
            vec!["set-option", "-t", "pulpo-test", "allow-passthrough", "on"]
        );
    }

    #[test]
    fn test_build_send_keys_command() {
        let cmd = build_send_keys_command(T, "pulpo-test", "hello world");
        assert_eq!(cmd.get_program(), T);
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(
            args,
            vec!["send-keys", "-t", "pulpo-test", "hello world", "Enter"]
        );
    }

    #[test]
    fn test_build_pipe_pane_command() {
        let cmd = build_pipe_pane_command(T, "pulpo-test", "/tmp/logs/session.log");
        assert_eq!(cmd.get_program(), T);
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(
            args,
            vec![
                "pipe-pane",
                "-t",
                "pulpo-test",
                "-o",
                "cat >> /tmp/logs/session.log"
            ]
        );
    }

    #[test]
    fn test_build_resize_command() {
        let cmd = build_resize_command(T, "pulpo-test", 120, 40);
        assert_eq!(cmd.get_program(), T);
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(
            args,
            vec!["resize-window", "-t", "pulpo-test", "-x", "120", "-y", "40"]
        );
    }

    #[test]
    fn test_build_list_sessions_command() {
        let cmd = build_list_sessions_command(T);
        assert_eq!(cmd.get_program(), T);
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(
            args,
            vec!["list-sessions", "-F", "#{session_id}\t#{session_name}"]
        );
    }

    #[test]
    fn test_build_set_env_command() {
        let cmd = build_set_env_command(T, "my-session", "PULPO_SESSION_ID", "abc-123");
        assert_eq!(cmd.get_program(), T);
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(
            args,
            vec![
                "set-environment",
                "-t",
                "my-session",
                "PULPO_SESSION_ID",
                "abc-123"
            ]
        );
    }

    #[test]
    fn test_build_pane_info_command() {
        let cmd = build_pane_info_command(T, "my-session");
        assert_eq!(cmd.get_program(), T);
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(
            args,
            vec![
                "list-panes",
                "-t",
                "my-session",
                "-F",
                "#{pane_current_command}\t#{pane_current_path}"
            ]
        );
    }

    #[test]
    fn test_build_query_session_id_command() {
        let cmd = build_query_session_id_command(T, "my-session");
        assert_eq!(cmd.get_program(), T);
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(
            args,
            vec!["display-message", "-t", "my-session", "-p", "#{session_id}"]
        );
    }

    #[test]
    fn test_parse_session_id_valid() {
        assert_eq!(parse_session_id("$0\n"), Some("$0".into()));
        assert_eq!(parse_session_id("$42\n"), Some("$42".into()));
        assert_eq!(parse_session_id("  $5  \n"), Some("$5".into()));
    }

    #[test]
    fn test_parse_session_id_invalid() {
        assert_eq!(parse_session_id(""), None);
        assert_eq!(parse_session_id("not-a-session-id"), None);
        assert_eq!(parse_session_id("my-session\n"), None);
    }

    #[test]
    fn test_build_pane_pid_command() {
        let cmd = build_pane_pid_command(T, "$5");
        assert_eq!(cmd.get_program(), T);
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(args, vec!["list-panes", "-t", "$5", "-F", "#{pane_pid}"]);
    }

    #[test]
    fn test_build_create_command_with_absolute_path() {
        let cmd = build_create_command("/opt/homebrew/bin/tmux", "sess", "/tmp", "echo hi", None);
        assert_eq!(cmd.get_program(), "/opt/homebrew/bin/tmux");
    }

    #[test]
    fn test_parse_list_sessions() {
        let output = "$0\tsession-1\n$1\tsession-2\n$5\tmy-work\n";
        let sessions = parse_list_sessions(output);
        assert_eq!(
            sessions,
            vec![
                ("$0".into(), "session-1".into()),
                ("$1".into(), "session-2".into()),
                ("$5".into(), "my-work".into()),
            ]
        );
    }

    #[test]
    fn test_parse_list_sessions_empty() {
        assert!(parse_list_sessions("").is_empty());
        assert!(parse_list_sessions("\n").is_empty());
    }

    #[test]
    fn test_parse_list_sessions_whitespace() {
        let output = "  $0\tsession-1  \n  $1\tsession-2  \n";
        let sessions = parse_list_sessions(output);
        assert_eq!(
            sessions,
            vec![
                ("$0".into(), "session-1".into()),
                ("$1".into(), "session-2".into()),
            ]
        );
    }

    #[test]
    fn test_parse_list_sessions_no_tab() {
        // Lines without tab separator are skipped
        let output = "no-tab-here\n$0\tvalid-session\n";
        let sessions = parse_list_sessions(output);
        assert_eq!(sessions, vec![("$0".into(), "valid-session".into())]);
    }

    #[test]
    fn test_parse_pane_info() {
        let output = "claude\t/home/user/repo\n";
        let (process, path) = parse_pane_info(output).unwrap();
        assert_eq!(process, "claude");
        assert_eq!(path, "/home/user/repo");
    }

    #[test]
    fn test_parse_pane_info_bash() {
        let output = "bash\t/tmp\n";
        let (process, path) = parse_pane_info(output).unwrap();
        assert_eq!(process, "bash");
        assert_eq!(path, "/tmp");
    }

    #[test]
    fn test_parse_pane_info_empty() {
        assert!(parse_pane_info("").is_none());
    }

    #[test]
    fn test_parse_pane_info_no_tab() {
        assert!(parse_pane_info("no-tab-here").is_none());
    }

    #[test]
    fn test_tmux_backend_new() {
        let backend = TmuxBackend::new();
        assert!(!backend.tmux_path.is_empty());
    }

    #[test]
    fn test_tmux_backend_default() {
        let backend = TmuxBackend::default();
        assert!(!backend.tmux_path.is_empty());
    }

    #[test]
    fn test_resolve_tmux_path_finds_tmux() {
        let path = resolve_tmux_path();
        // Should find tmux via which or fallback paths
        assert!(!path.is_empty());
    }

    #[test]
    fn test_tmux_search_paths_are_absolute() {
        for path in TMUX_SEARCH_PATHS {
            assert!(
                path.starts_with('/'),
                "Search path should be absolute: {path}"
            );
        }
    }

    #[test]
    fn test_parse_tmux_version_standard() {
        assert_eq!(parse_tmux_version("tmux 3.4"), Some((3, 4)));
    }

    #[test]
    fn test_parse_tmux_version_with_letter_suffix() {
        assert_eq!(parse_tmux_version("tmux 3.2a"), Some((3, 2)));
    }

    #[test]
    fn test_parse_tmux_version_next_prefix() {
        assert_eq!(parse_tmux_version("tmux next-3.4"), Some((3, 4)));
    }

    #[test]
    fn test_parse_tmux_version_old() {
        assert_eq!(parse_tmux_version("tmux 2.9"), Some((2, 9)));
    }

    #[test]
    fn test_parse_tmux_version_major_4() {
        assert_eq!(parse_tmux_version("tmux 4.0"), Some((4, 0)));
    }

    #[test]
    fn test_parse_tmux_version_empty() {
        assert_eq!(parse_tmux_version(""), None);
    }

    #[test]
    fn test_parse_tmux_version_invalid_prefix() {
        assert_eq!(parse_tmux_version("not-tmux"), None);
    }

    #[test]
    fn test_parse_tmux_version_no_version_number() {
        assert_eq!(parse_tmux_version("tmux"), None);
    }

    #[test]
    fn test_parse_tmux_version_major_only() {
        assert_eq!(parse_tmux_version("tmux 3"), Some((3, 0)));
    }

    #[test]
    fn test_parse_tmux_version_non_numeric() {
        assert_eq!(parse_tmux_version("tmux abc"), None);
    }

    // -- Integration tests: actually spawn tmux sessions --
    // These tests create real tmux sessions, run commands, and verify
    // the output. They catch bugs that unit tests miss (quoting issues,
    // PATH inheritance, tmux server crashes). Excluded from coverage
    // because they require a real tmux installation.

    /// Helper: create a tmux session, wait for the command to produce output,
    /// capture it, and kill the session. Returns the captured output.
    #[cfg(not(coverage))]
    fn tmux_run_and_capture(session_name: &str, command: &str, user_path: Option<&str>) -> String {
        let tmux = resolve_tmux_path();

        // Kill any leftover session with this name (best-effort)
        let _ = run_tmux(build_kill_command(&tmux, session_name), "cleanup");

        // Create the session
        run_tmux(
            build_create_command(&tmux, session_name, "/tmp", command, user_path),
            "create test session",
        )
        .unwrap_or_else(|e| panic!("failed to create tmux session '{session_name}': {e}"));

        // Wait for the command to produce output (up to 10 seconds — CI can be slow)
        let mut output = String::new();
        for _ in 0..100 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if let Ok(o) = run_tmux(
                build_capture_command(&tmux, session_name, 50),
                "capture output",
            ) {
                output = String::from_utf8_lossy(&o.stdout).trim().to_owned();
                if !output.is_empty() {
                    break;
                }
            }
        }

        // Kill the session
        let _ = run_tmux(build_kill_command(&tmux, session_name), "cleanup");

        output
    }

    #[cfg(not(coverage))]
    #[test]
    fn test_tmux_session_runs_simple_command() {
        // Use `sh -c` with sleep so the session stays alive long enough to capture
        let output =
            tmux_run_and_capture("pulpo-integ-simple", "sh -c 'echo PULPO_OK; sleep 5'", None);
        assert!(
            output.contains("PULPO_OK"),
            "tmux session should run the command: {output}"
        );
    }

    #[cfg(not(coverage))]
    #[test]
    fn test_tmux_session_inherits_user_path() {
        // Set a custom PATH and verify the session can see it
        let custom_path = format!(
            "/tmp/pulpo-test-fake-bin:{}",
            std::env::var("PATH").unwrap_or_default()
        );
        let output = tmux_run_and_capture(
            "pulpo-integ-path",
            "sh -c 'echo PATH_CHECK=$PATH; sleep 5'",
            Some(&custom_path),
        );
        assert!(
            output.contains("PATH_CHECK=/tmp/pulpo-test-fake-bin"),
            "tmux session should inherit user_path: {output}"
        );
    }

    #[cfg(not(coverage))]
    #[test]
    fn test_tmux_session_runs_wrapped_command() {
        // Test the full wrap_command flow: create a wrapped command and run it
        // through tmux, just like pulpod does in production.
        let id = uuid::Uuid::new_v4();
        let wrapped = crate::session::manager::wrap_command_for_test(
            "echo WRAPPED_OK",
            &id,
            "integ-wrapped",
            None,
        );
        // wrap_command adds `exec $SHELL -l` fallback, so the session stays alive
        let output = tmux_run_and_capture("pulpo-integ-wrapped", &wrapped, None);
        assert!(
            output.contains("WRAPPED_OK"),
            "tmux should execute wrapped command: {output}"
        );
    }

    #[cfg(not(coverage))]
    #[test]
    fn test_tmux_session_wrapped_command_with_single_quotes() {
        // Single-quoted arguments (e.g. claude -p 'fix the bug') are the most
        // common source of quoting bugs. Verify they survive the wrapping.
        let id = uuid::Uuid::new_v4();
        let wrapped = crate::session::manager::wrap_command_for_test(
            "echo 'QUOTED_OK'",
            &id,
            "integ-quotes",
            None,
        );
        let output = tmux_run_and_capture("pulpo-integ-quotes", &wrapped, None);
        assert!(
            output.contains("QUOTED_OK"),
            "tmux should handle single-quoted args: {output}"
        );
    }

    #[cfg(not(coverage))]
    #[test]
    fn test_check_tmux_version_succeeds_if_installed() {
        let backend = TmuxBackend::new();
        let result = check_tmux_version(&backend.tmux_path);
        assert!(result.is_ok(), "tmux should be installed: {result:?}");
        let version = result.unwrap();
        assert!(
            version.starts_with("tmux "),
            "Expected 'tmux ...' got '{version}'"
        );
    }
}
