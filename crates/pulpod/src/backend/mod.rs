pub mod tmux;

use anyhow::Result;

/// Backend trait — abstracts the terminal session runtime (tmux, Docker, etc.).
///
/// All methods that operate on a session take `backend_id` — the backend-specific
/// session identifier (e.g. `my-session` for tmux). This ID is created once
/// via `session_id()` at session creation time, stored in the database, and passed
/// to all subsequent backend calls. Callers must never pass the human-friendly
/// session name directly.
#[allow(dead_code)]
pub trait Backend: Send + Sync {
    /// Create the backend-specific session identifier from a human-friendly session name.
    /// Called once at session creation; the result is stored as `backend_session_id`.
    /// For tmux this returns the session name as-is; other backends may use different schemes.
    /// Default returns the name as-is (useful for test stubs).
    fn session_id(&self, name: &str) -> String {
        name.to_owned()
    }

    /// Check the backend runtime version. Returns a human-readable version string.
    fn check_version(&self) -> Result<String> {
        Ok("unknown".into())
    }

    /// Create a new terminal session running the given command.
    fn create_session(&self, backend_id: &str, working_dir: &str, command: &str) -> Result<()>;

    /// Kill a terminal session.
    fn kill_session(&self, backend_id: &str) -> Result<()>;

    /// Check if a session is still alive.
    fn is_alive(&self, backend_id: &str) -> Result<bool>;

    /// Capture the current terminal output (last N lines).
    fn capture_output(&self, backend_id: &str, lines: usize) -> Result<String>;

    /// Send input text to the session.
    fn send_input(&self, backend_id: &str, text: &str) -> Result<()>;

    /// Set up output logging via pipe-pane to the given log file path.
    fn setup_logging(&self, backend_id: &str, log_path: &str) -> Result<()>;

    /// Resize the session's terminal to the given dimensions.
    /// Default implementation is a no-op.
    fn resize(&self, _backend_id: &str, _cols: u16, _rows: u16) -> Result<()> {
        Ok(())
    }

    /// Spawn a child process that attaches to the session's terminal for PTY bridging.
    /// Returns a tokio child process with piped stdin/stdout.
    /// Default implementation returns an error — only real backends (tmux) override this.
    fn spawn_attach(&self, _backend_id: &str) -> Result<tokio::process::Child> {
        anyhow::bail!("spawn_attach not supported by this backend")
    }

    /// Query the backend-specific internal session ID from a human-friendly name.
    /// For tmux, returns the `$N` session ID. Default returns `name.to_owned()`.
    fn query_backend_id(&self, name: &str) -> Result<String> {
        Ok(name.to_owned())
    }

    /// Get the full command line of the process running in the session's pane.
    /// Default returns an error — only real backends (tmux) override this.
    fn pane_command_line(&self, _backend_id: &str) -> Result<String> {
        anyhow::bail!("pane_command_line not supported by this backend")
    }

    /// List all sessions known to the backend runtime.
    /// Returns `(backend_id, name)` pairs.
    /// Default returns empty — only real backends (tmux) override this.
    fn list_sessions(&self) -> Result<Vec<(String, String)>> {
        Ok(Vec::new())
    }

    /// Get the current pane process and working directory for a backend session.
    /// Returns `(process_name, working_dir)`.
    /// Default returns an error — only real backends (tmux) override this.
    fn pane_info(&self, _backend_id: &str) -> Result<(String, String)> {
        anyhow::bail!("pane_info not supported by this backend")
    }

    /// Set an environment variable in a backend session.
    /// Default is a no-op — only real backends (tmux) override this.
    fn set_env(&self, _backend_id: &str, _key: &str, _value: &str) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MinimalBackend;

    impl Backend for MinimalBackend {
        fn create_session(&self, _: &str, _: &str, _: &str) -> Result<()> {
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

    #[test]
    fn test_default_session_id() {
        let b = MinimalBackend;
        assert_eq!(b.session_id("my-session"), "my-session");
    }

    #[test]
    fn test_default_check_version() {
        let b = MinimalBackend;
        assert_eq!(b.check_version().unwrap(), "unknown");
    }

    #[test]
    fn test_default_resize() {
        let b = MinimalBackend;
        assert!(b.resize("s", 120, 40).is_ok());
    }

    #[test]
    fn test_default_spawn_attach() {
        let b = MinimalBackend;
        let err = b.spawn_attach("x").unwrap_err();
        assert!(err.to_string().contains("spawn_attach not supported"));
    }

    #[test]
    fn test_default_query_backend_id() {
        let b = MinimalBackend;
        assert_eq!(b.query_backend_id("my-session").unwrap(), "my-session");
    }

    #[test]
    fn test_default_pane_command_line() {
        let b = MinimalBackend;
        let err = b.pane_command_line("x").unwrap_err();
        assert!(err.to_string().contains("pane_command_line not supported"));
    }

    #[test]
    fn test_default_list_sessions() {
        let b = MinimalBackend;
        assert!(b.list_sessions().unwrap().is_empty());
    }

    #[test]
    fn test_default_pane_info() {
        let b = MinimalBackend;
        let err = b.pane_info("x").unwrap_err();
        assert!(err.to_string().contains("pane_info not supported"));
    }

    #[test]
    fn test_default_set_env() {
        let b = MinimalBackend;
        assert!(b.set_env("s", "KEY", "VALUE").is_ok());
    }

    #[test]
    fn test_minimal_backend_required_methods() {
        let b = MinimalBackend;
        assert!(b.create_session("s", "/tmp", "echo hi").is_ok());
        assert!(b.kill_session("s").is_ok());
        assert!(b.is_alive("s").unwrap());
        assert_eq!(b.capture_output("s", 10).unwrap(), "");
        assert!(b.send_input("s", "hello").is_ok());
        assert!(b.setup_logging("s", "/tmp/log").is_ok());
    }
}
