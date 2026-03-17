#[cfg(not(coverage))]
use anyhow::{Context, Result, bail};
use std::process::Command;
#[cfg(not(coverage))]
use std::process::{Output, Stdio};

#[cfg(not(coverage))]
use super::Backend;

#[cfg_attr(coverage, allow(dead_code))]
fn tmux_session_name(name: &str) -> String {
    name.to_owned()
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_create_command(session_name: &str, working_dir: &str, command: &str) -> Command {
    let mut cmd = Command::new("tmux");
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
fn build_kill_command(session_name: &str) -> Command {
    let mut cmd = Command::new("tmux");
    cmd.args(["kill-session", "-t", session_name]);
    cmd
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_has_session_command(session_name: &str) -> Command {
    let mut cmd = Command::new("tmux");
    cmd.args(["has-session", "-t", session_name]);
    cmd
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_capture_command(session_name: &str, lines: usize) -> Command {
    let start = -i64::try_from(lines).unwrap_or(i64::MAX);
    let mut cmd = Command::new("tmux");
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
fn build_set_mouse_command(session_name: &str) -> Command {
    let mut cmd = Command::new("tmux");
    cmd.args(["set-option", "-t", session_name, "mouse", "on"]);
    cmd
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_set_clipboard_command(session_name: &str) -> Command {
    let mut cmd = Command::new("tmux");
    cmd.args(["set-option", "-t", session_name, "set-clipboard", "on"]);
    cmd
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_allow_passthrough_command(session_name: &str) -> Command {
    let mut cmd = Command::new("tmux");
    cmd.args(["set-option", "-t", session_name, "allow-passthrough", "on"]);
    cmd
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_send_keys_command(session_name: &str, text: &str) -> Command {
    let mut cmd = Command::new("tmux");
    cmd.args(["send-keys", "-t", session_name, text, "Enter"]);
    cmd
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_pipe_pane_command(session_name: &str, log_path: &str) -> Command {
    let mut cmd = Command::new("tmux");
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
fn build_resize_command(session_name: &str, cols: u16, rows: u16) -> Command {
    let mut cmd = Command::new("tmux");
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
fn build_list_sessions_command() -> Command {
    let mut cmd = Command::new("tmux");
    cmd.args(["list-sessions", "-F", "#{session_name}"]);
    cmd
}

#[cfg_attr(coverage, allow(dead_code))]
fn build_pane_info_command(session_name: &str) -> Command {
    let mut cmd = Command::new("tmux");
    cmd.args([
        "list-panes",
        "-t",
        session_name,
        "-F",
        "#{pane_current_command}\t#{pane_current_path}",
    ]);
    cmd
}

/// Parse `list-sessions -F '#{session_name}'` output into session names.
pub fn parse_list_sessions(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
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
pub fn check_tmux_version() -> anyhow::Result<String> {
    let output = Command::new("tmux")
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
pub struct TmuxBackend;

#[allow(dead_code)]
impl Default for TmuxBackend {
    fn default() -> Self {
        Self
    }
}

#[allow(dead_code)]
impl TmuxBackend {
    pub const fn new() -> Self {
        Self
    }
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
        check_tmux_version()
    }

    fn create_session(&self, backend_id: &str, working_dir: &str, command: &str) -> Result<()> {
        run_tmux(
            build_create_command(backend_id, working_dir, command),
            "create tmux session",
        )?;
        // Enable mouse mode so scrollback works via the web terminal
        run_tmux(
            build_set_mouse_command(backend_id),
            "enable tmux mouse mode",
        )?;
        // Enable clipboard forwarding for image paste support
        run_tmux(
            build_set_clipboard_command(backend_id),
            "enable tmux clipboard",
        )?;
        // Allow escape sequence passthrough (image paste, OSC sequences)
        run_tmux(
            build_allow_passthrough_command(backend_id),
            "enable tmux passthrough",
        )?;
        Ok(())
    }

    fn kill_session(&self, backend_id: &str) -> Result<()> {
        run_tmux(build_kill_command(backend_id), "kill tmux session")?;
        Ok(())
    }

    fn is_alive(&self, backend_id: &str) -> Result<bool> {
        let output = build_has_session_command(backend_id)
            .stderr(Stdio::piped())
            .output()
            .context("Failed to check tmux session")?;
        Ok(output.status.success())
    }

    fn capture_output(&self, backend_id: &str, lines: usize) -> Result<String> {
        let output: Output = build_capture_command(backend_id, lines)
            .stderr(Stdio::piped())
            .output()
            .context("Failed to capture tmux pane")?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn send_input(&self, backend_id: &str, text: &str) -> Result<()> {
        run_tmux(
            build_send_keys_command(backend_id, text),
            "send input to tmux session",
        )?;
        Ok(())
    }

    fn setup_logging(&self, backend_id: &str, log_path: &str) -> Result<()> {
        run_tmux(
            build_pipe_pane_command(backend_id, log_path),
            "setup pipe-pane logging",
        )?;
        Ok(())
    }

    fn resize(&self, backend_id: &str, cols: u16, rows: u16) -> Result<()> {
        run_tmux(
            build_resize_command(backend_id, cols, rows),
            "resize tmux window",
        )?;
        Ok(())
    }

    fn spawn_attach(&self, backend_id: &str) -> Result<tokio::process::Child> {
        let mut cmd = tokio::process::Command::new("script");

        #[cfg(target_os = "macos")]
        cmd.args([
            "-q",
            "/dev/null",
            "tmux",
            "attach-session",
            "-t",
            backend_id,
        ]);

        #[cfg(not(target_os = "macos"))]
        cmd.args([
            "-q",
            "-c",
            &format!("tmux attach-session -t {backend_id}"),
            "/dev/null",
        ]);

        cmd.env_remove("TMUX");
        cmd.env("TERM", "xterm-256color");
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());

        cmd.spawn().context("spawn script+tmux attach")
    }

    fn list_sessions(&self) -> Result<Vec<String>> {
        let output = build_list_sessions_command()
            .stderr(Stdio::piped())
            .output()
            .context("Failed to list tmux sessions")?;
        if !output.status.success() {
            // tmux returns non-zero when no server is running — not an error
            return Ok(Vec::new());
        }
        Ok(parse_list_sessions(&String::from_utf8_lossy(
            &output.stdout,
        )))
    }

    fn pane_info(&self, backend_id: &str) -> Result<(String, String)> {
        let output = run_tmux(build_pane_info_command(backend_id), "get pane info")?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_pane_info(&stdout).context("Failed to parse pane info")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn test_tmux_session_name() {
        assert_eq!(tmux_session_name("my-api"), "my-api");
        assert_eq!(tmux_session_name("indigo-wave"), "indigo-wave");
    }

    #[test]
    fn test_build_create_command() {
        let cmd = build_create_command("pulpo-test", "/tmp/repo", "claude");
        assert_eq!(cmd.get_program(), "tmux");
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
    fn test_build_kill_command() {
        let cmd = build_kill_command("pulpo-test");
        assert_eq!(cmd.get_program(), "tmux");
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(args, vec!["kill-session", "-t", "pulpo-test"]);
    }

    #[test]
    fn test_build_has_session_command() {
        let cmd = build_has_session_command("pulpo-test");
        assert_eq!(cmd.get_program(), "tmux");
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(args, vec!["has-session", "-t", "pulpo-test"]);
    }

    #[test]
    fn test_build_capture_command() {
        let cmd = build_capture_command("pulpo-test", 100);
        assert_eq!(cmd.get_program(), "tmux");
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(
            args,
            vec!["capture-pane", "-t", "pulpo-test", "-p", "-S", "-100"]
        );
    }

    #[test]
    fn test_build_capture_command_large_lines() {
        let cmd = build_capture_command("pulpo-test", usize::MAX);
        let args: Vec<&OsStr> = cmd.get_args().collect();
        // Should handle overflow gracefully
        let start_str = args[5].to_str().unwrap();
        assert!(start_str.starts_with('-'));
    }

    #[test]
    fn test_build_set_mouse_command() {
        let cmd = build_set_mouse_command("pulpo-test");
        assert_eq!(cmd.get_program(), "tmux");
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(args, vec!["set-option", "-t", "pulpo-test", "mouse", "on"]);
    }

    #[test]
    fn test_build_set_clipboard_command() {
        let cmd = build_set_clipboard_command("pulpo-test");
        assert_eq!(cmd.get_program(), "tmux");
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(
            args,
            vec!["set-option", "-t", "pulpo-test", "set-clipboard", "on"]
        );
    }

    #[test]
    fn test_build_allow_passthrough_command() {
        let cmd = build_allow_passthrough_command("pulpo-test");
        assert_eq!(cmd.get_program(), "tmux");
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(
            args,
            vec!["set-option", "-t", "pulpo-test", "allow-passthrough", "on"]
        );
    }

    #[test]
    fn test_build_send_keys_command() {
        let cmd = build_send_keys_command("pulpo-test", "hello world");
        assert_eq!(cmd.get_program(), "tmux");
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(
            args,
            vec!["send-keys", "-t", "pulpo-test", "hello world", "Enter"]
        );
    }

    #[test]
    fn test_build_pipe_pane_command() {
        let cmd = build_pipe_pane_command("pulpo-test", "/tmp/logs/session.log");
        assert_eq!(cmd.get_program(), "tmux");
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
        let cmd = build_resize_command("pulpo-test", 120, 40);
        assert_eq!(cmd.get_program(), "tmux");
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(
            args,
            vec!["resize-window", "-t", "pulpo-test", "-x", "120", "-y", "40"]
        );
    }

    #[test]
    fn test_build_list_sessions_command() {
        let cmd = build_list_sessions_command();
        assert_eq!(cmd.get_program(), "tmux");
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(args, vec!["list-sessions", "-F", "#{session_name}"]);
    }

    #[test]
    fn test_build_pane_info_command() {
        let cmd = build_pane_info_command("my-session");
        assert_eq!(cmd.get_program(), "tmux");
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
    fn test_parse_list_sessions() {
        let output = "session-1\nsession-2\nmy-work\n";
        let sessions = parse_list_sessions(output);
        assert_eq!(sessions, vec!["session-1", "session-2", "my-work"]);
    }

    #[test]
    fn test_parse_list_sessions_empty() {
        assert!(parse_list_sessions("").is_empty());
        assert!(parse_list_sessions("\n").is_empty());
    }

    #[test]
    fn test_parse_list_sessions_whitespace() {
        let output = "  session-1  \n  session-2  \n";
        let sessions = parse_list_sessions(output);
        assert_eq!(sessions, vec!["session-1", "session-2"]);
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
        let _backend = TmuxBackend::new();
    }

    #[test]
    fn test_tmux_backend_default() {
        #[allow(clippy::default_constructed_unit_structs)]
        let _backend = TmuxBackend::default();
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

    #[cfg(not(coverage))]
    #[test]
    fn test_check_tmux_version_succeeds_if_installed() {
        // This test only runs outside coverage builds, where tmux is expected to be installed
        let result = check_tmux_version();
        assert!(result.is_ok(), "tmux should be installed: {result:?}");
        let version = result.unwrap();
        assert!(
            version.starts_with("tmux "),
            "Expected 'tmux ...' got '{version}'"
        );
    }
}
