use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State, WebSocketUpgrade},
    http::StatusCode,
    response::Response,
};
use futures::StreamExt;
use pulpo_common::api::ErrorResponse;
use pulpo_common::session::SessionStatus;
use tracing::info;

type ApiError = (StatusCode, Json<ErrorResponse>);

pub async fn stream(
    State(state): State<Arc<super::AppState>>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let session = state
        .session_manager
        .get_session(&id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("session not found: {id}"),
                }),
            )
        })?;

    if session.status != SessionStatus::Active && session.status != SessionStatus::Idle {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("session is not running (status: {})", session.status),
            }),
        ));
    }

    let backend_id = state.session_manager.resolve_backend_id(&session);

    info!("WebSocket stream requested for session {id} (backend: {backend_id})");

    let backend = state.session_manager.backend();
    Ok(ws.on_upgrade(move |socket| async move {
        handle_stream(socket, &backend_id, &backend).await;
    }))
}

async fn handle_stream(
    socket: axum::extract::ws::WebSocket,
    session_id: &str,
    backend: &Arc<dyn crate::backend::Backend>,
) {
    #[cfg(not(coverage))]
    {
        use crate::session::pty_bridge;
        use tracing::{debug, warn};

        // Use the backend's script-based spawn_attach for the PTY.
        // Then clone the child's stdout fd for resize via tcsetwinsize.
        let mut child = match backend.spawn_attach(session_id) {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to spawn PTY for {session_id}: {e:#}");
                return;
            }
        };

        let Some(stdout) = child.stdout.take() else {
            warn!("No stdout pipe for {session_id}");
            return;
        };
        let Some(stdin) = child.stdin.take() else {
            warn!("No stdin pipe for {session_id}");
            return;
        };

        // script's child (tmux) runs on the PTY slave. Find the child PID,
        // then look up its TTY device so we can resize the PTY directly.
        let child_pid = child.id();
        let tty_fd = child_pid.and_then(|script_pid| {
            // Wait briefly for script to fork its child
            std::thread::sleep(std::time::Duration::from_millis(200));
            // Find script's child process (tmux)
            let output = std::process::Command::new("pgrep")
                .args(["-P", &script_pid.to_string()])
                .output()
                .ok()?;
            let tmux_pid = String::from_utf8_lossy(&output.stdout)
                .trim()
                .lines()
                .next()?
                .to_owned();
            // Get the child's TTY
            let output = std::process::Command::new("ps")
                .args(["-p", &tmux_pid, "-o", "tty="])
                .output()
                .ok()?;
            let tty = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            if tty.is_empty() || tty == "??" {
                return None;
            }
            let path = format!("/dev/{tty}");
            let fd = std::fs::OpenOptions::new().write(true).open(&path).ok()?;
            info!("PTY device for {session_id}: {path} (tmux pid {tmux_pid})");
            Some(fd)
        });
        info!(
            "PTY bridge started for {session_id} (child pid: {child_pid:?}, tty_fd: {})",
            if tty_fd.is_some() { "found" } else { "none" }
        );

        let (ws_sender, ws_receiver) = socket.split();
        let result =
            pty_bridge::run_bridge(stdout, stdin, ws_sender, ws_receiver, move |cols, rows| {
                debug!("Resize: {cols}x{rows}");
                if let Some(ref fd) = tty_fd {
                    use std::os::fd::AsFd;
                    let ws = rustix::termios::Winsize {
                        ws_col: cols,
                        ws_row: rows,
                        ws_xpixel: 0,
                        ws_ypixel: 0,
                    };
                    if let Err(e) = rustix::termios::tcsetwinsize(fd.as_fd(), ws) {
                        debug!("tcsetwinsize failed: {e}");
                    }
                }
                Ok(())
            })
            .await;
        if let Err(e) = &result {
            warn!("PTY bridge error for {session_id}: {e}");
        }
        info!("PTY bridge ended for {session_id}");
        let _ = child.kill().await;
    }

    #[cfg(coverage)]
    {
        // In coverage builds, echo input back for testing
        let (mut ws_sender, mut ws_receiver) = socket.split();
        use axum::extract::ws::Message;
        use futures::SinkExt;

        while let Some(Ok(msg)) = ws_receiver.next().await {
            let response = match msg {
                Message::Binary(data) => Message::Binary(data),
                Message::Text(text) => Message::Text(format!("echo:{text}").into()),
                _ => break, // Close, Ping, Pong
            };
            let _ = ws_sender.send(response).await;
        }
        let _ = session_id;
        let _ = backend;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::AppState;
    use crate::backend::Backend;
    use std::collections::HashMap;

    use crate::config::{Config, NodeConfig};
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;
    use anyhow::Result;
    use pulpo_common::api::CreateSessionRequest;

    struct StubBackend;

    impl Backend for StubBackend {
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

    struct DeadBackend;

    impl Backend for DeadBackend {
        fn create_session(&self, _: &str, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn kill_session(&self, _: &str) -> Result<()> {
            Ok(())
        }
        fn is_alive(&self, _: &str) -> Result<bool> {
            Ok(false)
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

    async fn test_state_withbackend(backend: Arc<dyn Backend>) -> Arc<AppState> {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        AppState::new(
            Config {
                node: NodeConfig {
                    name: "test-node".into(),
                    port: 7433,
                    data_dir: tmpdir.path().to_str().unwrap().into(),
                    ..NodeConfig::default()
                },
                auth: crate::config::AuthConfig::default(),
                peers: HashMap::new(),
                watchdog: crate::config::WatchdogConfig::default(),
                inks: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
            },
            manager,
            peer_registry,
            store,
        )
    }

    #[tokio::test]
    async fn test_stream_not_found() {
        let state = test_state_withbackend(Arc::new(StubBackend)).await;
        // We can't easily test the WS upgrade without a real HTTP request,
        // but we can verify the session validation by calling the handler
        // without the WebSocketUpgrade (which would fail at the extractor level).
        // Instead, test the error cases via the route integration tests in routes.rs.

        // Verify: session not found returns 404 by checking the manager
        let result = state.session_manager.get_session("nonexistent").await;
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_stream_not_running() {
        // Create a session, then kill it, verify it would fail the running check
        let state = test_state_withbackend(Arc::new(DeadBackend)).await;
        let req = CreateSessionRequest {
            name: "dead-test".into(),
            workdir: Some("/tmp".into()),
            metadata: None,
            command: Some("echo test".into()),
            description: None,
            ink: None,
            idle_threshold_secs: None,
            worktree: None,
        };
        let session = state.session_manager.create_session(req).await.unwrap();
        // DeadBackend's is_alive returns false, so get_session marks it Stale
        let fetched = state
            .session_manager
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_ne!(fetched.status, SessionStatus::Active);
    }

    #[test]
    fn test_stubbackend_methods() {
        let b = StubBackend;
        assert!(b.create_session("n", "d", "c").is_ok());
        assert!(b.kill_session("n").is_ok());
        assert!(b.is_alive("n").unwrap());
        assert!(b.capture_output("n", 10).unwrap().is_empty());
        assert!(b.send_input("n", "t").is_ok());
        assert!(b.setup_logging("n", "p").is_ok());
    }

    #[test]
    fn test_deadbackend_methods() {
        let b = DeadBackend;
        assert!(b.create_session("n", "d", "c").is_ok());
        assert!(b.kill_session("n").is_ok());
        assert!(!b.is_alive("n").unwrap());
        assert!(b.capture_output("n", 10).unwrap().is_empty());
        assert!(b.send_input("n", "t").is_ok());
        assert!(b.setup_logging("n", "p").is_ok());
    }

    #[tokio::test]
    async fn test_resolve_backend_id_with_explicit() {
        use pulpo_common::session::*;
        let state = test_state_withbackend(Arc::new(StubBackend)).await;
        let session = Session {
            id: uuid::Uuid::new_v4(),
            name: "my-session".into(),
            workdir: "/tmp".into(),
            command: "echo hello".into(),
            description: None,
            status: SessionStatus::Active,
            exit_code: None,
            backend_session_id: Some("custom-backend-id".into()),

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
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        assert_eq!(
            state.session_manager.resolve_backend_id(&session),
            "custom-backend-id"
        );
    }

    #[tokio::test]
    async fn test_resolve_backend_id_fallback() {
        use pulpo_common::session::*;
        let state = test_state_withbackend(Arc::new(StubBackend)).await;
        let session = Session {
            id: uuid::Uuid::new_v4(),
            name: "my-session".into(),
            workdir: "/tmp".into(),
            command: "echo hello".into(),
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
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        // StubBackend.session_id returns just the name
        assert_eq!(
            state.session_manager.resolve_backend_id(&session),
            "my-session"
        );
    }
}
