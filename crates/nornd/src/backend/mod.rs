pub mod tmux;

#[cfg(target_os = "linux")]
pub mod docker;

use anyhow::Result;

/// Backend trait — abstracts tmux (direct) vs Docker+tmux.
#[allow(dead_code)]
pub trait Backend: Send + Sync {
    /// Create a new terminal session running the given command.
    fn create_session(&self, name: &str, working_dir: &str, command: &str) -> Result<()>;

    /// Kill a terminal session.
    fn kill_session(&self, name: &str) -> Result<()>;

    /// Check if a session is still alive.
    fn is_alive(&self, name: &str) -> Result<bool>;

    /// Capture the current terminal output (last N lines).
    fn capture_output(&self, name: &str, lines: usize) -> Result<String>;

    /// Send input text to the session.
    fn send_input(&self, name: &str, text: &str) -> Result<()>;
}
