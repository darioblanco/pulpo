#[cfg(not(coverage))]
use anyhow::{Context, Result, bail};
use std::process::Command;
#[cfg(not(coverage))]
use std::process::{Output, Stdio};

#[cfg(not(coverage))]
use super::Backend;

#[cfg_attr(coverage, allow(dead_code))]
fn tmux_session_name(name: &str) -> String {
    format!("pulpo-{name}")
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

/// Check that the provider binary (e.g. `claude`, `codex`) is on `$PATH`.
#[cfg(not(coverage))]
pub fn check_provider_binary(provider: &str) -> anyhow::Result<()> {
    let output = Command::new("which").arg(provider).output();
    match output {
        Ok(o) if o.status.success() => Ok(()),
        _ => anyhow::bail!(
            "{provider} is not installed. Install it before spawning sessions.\n\
             Claude: npm install -g @anthropic-ai/claude-code\n\
             Codex: npm install -g @openai/codex"
        ),
    }
}

#[cfg(coverage)]
pub fn check_provider_binary(_provider: &str) -> anyhow::Result<()> {
    Ok(())
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
    fn create_session(&self, name: &str, working_dir: &str, command: &str) -> Result<()> {
        let session_name = tmux_session_name(name);
        run_tmux(
            build_create_command(&session_name, working_dir, command),
            "create tmux session",
        )?;
        Ok(())
    }

    fn kill_session(&self, name: &str) -> Result<()> {
        let session_name = tmux_session_name(name);
        run_tmux(build_kill_command(&session_name), "kill tmux session")?;
        Ok(())
    }

    fn is_alive(&self, name: &str) -> Result<bool> {
        let session_name = tmux_session_name(name);
        let output = build_has_session_command(&session_name)
            .stderr(Stdio::piped())
            .output()
            .context("Failed to check tmux session")?;
        Ok(output.status.success())
    }

    fn capture_output(&self, name: &str, lines: usize) -> Result<String> {
        let session_name = tmux_session_name(name);
        let output: Output = build_capture_command(&session_name, lines)
            .stderr(Stdio::piped())
            .output()
            .context("Failed to capture tmux pane")?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn send_input(&self, name: &str, text: &str) -> Result<()> {
        let session_name = tmux_session_name(name);
        run_tmux(
            build_send_keys_command(&session_name, text),
            "send input to tmux session",
        )?;
        Ok(())
    }

    fn setup_logging(&self, name: &str, log_path: &str) -> Result<()> {
        let session_name = tmux_session_name(name);
        run_tmux(
            build_pipe_pane_command(&session_name, log_path),
            "setup pipe-pane logging",
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn test_tmux_session_name() {
        assert_eq!(tmux_session_name("my-api"), "pulpo-my-api");
        assert_eq!(tmux_session_name("test"), "pulpo-test");
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

    #[cfg(not(coverage))]
    #[test]
    fn test_check_provider_binary_missing() {
        let result = check_provider_binary("nonexistent-binary-xyz-12345");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("is not installed"), "got: {err}");
    }

    #[test]
    fn test_check_provider_binary_coverage_noop() {
        // Under non-coverage builds this calls real `which`, so use a binary that exists
        let result = check_provider_binary("ls");
        assert!(result.is_ok());
    }
}
