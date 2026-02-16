use anyhow::{Context, Result};
use std::process::Command;

use super::Backend;

#[allow(dead_code)]
pub struct TmuxBackend;

#[allow(dead_code)]
impl TmuxBackend {
    pub const fn new() -> Self {
        Self
    }
}

impl Backend for TmuxBackend {
    fn create_session(&self, name: &str, working_dir: &str, command: &str) -> Result<()> {
        let session_name = format!("norn-{name}");
        Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s",
                &session_name,
                "-c",
                working_dir,
                command,
            ])
            .status()
            .context("Failed to create tmux session")?;
        Ok(())
    }

    fn kill_session(&self, name: &str) -> Result<()> {
        let session_name = format!("norn-{name}");
        Command::new("tmux")
            .args(["kill-session", "-t", &session_name])
            .status()
            .context("Failed to kill tmux session")?;
        Ok(())
    }

    fn is_alive(&self, name: &str) -> Result<bool> {
        let session_name = format!("norn-{name}");
        let output = Command::new("tmux")
            .args(["has-session", "-t", &session_name])
            .status()
            .context("Failed to check tmux session")?;
        Ok(output.success())
    }

    fn capture_output(&self, name: &str, lines: usize) -> Result<String> {
        let session_name = format!("norn-{name}");
        let start = -i64::try_from(lines).unwrap_or(i64::MAX);
        let output = Command::new("tmux")
            .args([
                "capture-pane",
                "-t",
                &session_name,
                "-p",
                "-S",
                &start.to_string(),
            ])
            .output()
            .context("Failed to capture tmux pane")?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn send_input(&self, name: &str, text: &str) -> Result<()> {
        let session_name = format!("norn-{name}");
        Command::new("tmux")
            .args(["send-keys", "-t", &session_name, text, "Enter"])
            .status()
            .context("Failed to send input to tmux session")?;
        Ok(())
    }
}
