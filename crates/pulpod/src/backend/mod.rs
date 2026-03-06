pub mod tmux;

use anyhow::Result;

/// Backend trait — abstracts the terminal session runtime (tmux, Docker, etc.).
#[allow(dead_code)]
pub trait Backend: Send + Sync {
    /// Return the backend-specific session identifier for a given session name.
    /// For tmux this is `pulpo-{name}`; other backends may use different schemes.
    fn session_id(&self, name: &str) -> String;

    /// Check the backend runtime version. Returns a human-readable version string.
    fn check_version(&self) -> Result<String> {
        Ok("unknown".into())
    }

    /// Check that the provider binary is available.
    /// Default is no-op (for mocks). Real backends override this.
    fn check_provider(&self, _provider: &str) -> Result<()> {
        Ok(())
    }

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

    /// Set up output logging via pipe-pane to the given log file path.
    fn setup_logging(&self, name: &str, log_path: &str) -> Result<()>;

    /// Spawn a child process that attaches to the session's terminal for PTY bridging.
    /// Returns a tokio child process with piped stdin/stdout.
    fn spawn_attach(&self, name: &str) -> Result<tokio::process::Child>;
}
