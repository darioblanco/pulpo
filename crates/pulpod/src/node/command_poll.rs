use pulpo_common::api::NodeCommandsResponse;
#[cfg(not(coverage))]
use pulpo_common::api::{CreateSessionRequest, NodeCommand};
#[cfg(not(coverage))]
use std::time::Duration;
#[cfg(not(coverage))]
use tracing::{debug, info, warn};

/// Execute a single node command against the local session manager.
#[cfg(not(coverage))]
async fn execute_command(
    cmd: NodeCommand,
    session_manager: &crate::session::manager::SessionManager,
) {
    match cmd {
        NodeCommand::CreateSession {
            command_id,
            name,
            workdir,
            command,
            ink,
            description,
        } => {
            debug!(command_id = %command_id, name = %name, "Executing create session command");
            let req = CreateSessionRequest {
                name: name.clone(),
                workdir,
                command,
                ink,
                description,
                metadata: None,
                idle_threshold_secs: None,
                worktree: None,
                worktree_base: None,
                runtime: None,
                secrets: None,
                target_node: None,
                term_program: None,
            };
            match session_manager.create_session(req).await {
                Ok(session) => {
                    info!(command_id = %command_id, session_id = %session.id, name = %name,
                        "Created session from controller command");
                }
                Err(e) => {
                    warn!(command_id = %command_id, name = %name, error = %e,
                        "Failed to create session from controller command");
                }
            }
        }
        NodeCommand::StopSession {
            command_id,
            session_id,
        } => {
            debug!(command_id = %command_id, session_id = %session_id, "Executing stop session command");
            match session_manager.stop_session(&session_id, false).await {
                Ok(()) => {
                    info!(command_id = %command_id, session_id = %session_id,
                        "Stopped session from controller command");
                }
                Err(e) => {
                    warn!(command_id = %command_id, session_id = %session_id, error = %e,
                        "Failed to stop session from controller command");
                }
            }
        }
    }
}

/// Run the command poll loop — periodically GETs pending commands from the
/// controller and executes them locally.
#[cfg(not(coverage))]
pub async fn run_command_poll_loop(
    controller_url: String,
    controller_token: String,
    _node_name: String,
    session_manager: crate::session::manager::SessionManager,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("failed to build reqwest client");

    let poll_url = format!("{controller_url}/api/v1/node/commands");
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    info!("Node command poll loop started (controller={controller_url})");

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let request = client.get(&poll_url).bearer_auth(&controller_token);

                let response = match request.send().await {
                    Ok(resp) if resp.status().is_success() => resp,
                    Ok(resp) => {
                        warn!(status = %resp.status(), "Controller rejected command poll");
                        continue;
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to poll commands from controller");
                        continue;
                    }
                };

                let commands_resp = match response.json::<NodeCommandsResponse>().await {
                    Ok(r) => r,
                    Err(e) => {
                        warn!(error = %e, "Failed to parse command poll response");
                        continue;
                    }
                };

                for cmd in commands_resp.commands {
                    execute_command(cmd, &session_manager).await;
                }
            }
            _ = shutdown_rx.changed() => {
                info!("Command poll loop shutting down");
                break;
            }
        }
    }
}

/// Stub for coverage builds — the real loop performs network I/O.
#[cfg(coverage)]
pub async fn run_command_poll_loop(
    _controller_url: String,
    _controller_token: String,
    _node_name: String,
    _session_manager: crate::session::manager::SessionManager,
    _shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
}

/// Parse a command poll response from JSON bytes.
#[cfg_attr(coverage, allow(dead_code))]
pub fn parse_commands_response(body: &[u8]) -> Result<NodeCommandsResponse, serde_json::Error> {
    serde_json::from_slice(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulpo_common::api::NodeCommand;
    #[cfg(coverage)]
    #[tokio::test]
    async fn test_coverage_stub_returns_immediately() {
        use crate::backend::StubBackend;
        use crate::config::InkConfig;
        use crate::session::manager::SessionManager;
        use crate::store::Store;
        use std::collections::HashMap;
        use std::sync::Arc;

        let store = Store::new(":memory:").await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(StubBackend);
        let inks: HashMap<String, InkConfig> = HashMap::new();
        let manager = SessionManager::new(backend, store, inks, None);

        let (_shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            run_command_poll_loop(
                "http://localhost:9999".into(),
                "node-token".into(),
                "test-node".into(),
                manager,
                shutdown_rx,
            ),
        )
        .await
        .expect("coverage stub should return immediately");
    }

    #[test]
    fn test_parse_commands_response_empty() {
        let json = br#"{"commands":[]}"#;
        let resp = parse_commands_response(json).unwrap();
        assert!(resp.commands.is_empty());
    }

    #[test]
    fn test_parse_commands_response_with_commands() {
        let json = br#"{"commands":[{"type":"create_session","command_id":"c1","name":"test","workdir":null,"command":null,"ink":null,"description":null},{"type":"stop_session","command_id":"c2","session_id":"s1"}]}"#;
        let resp = parse_commands_response(json).unwrap();
        assert_eq!(resp.commands.len(), 2);
        match &resp.commands[0] {
            NodeCommand::CreateSession { name, .. } => assert_eq!(name, "test"),
            NodeCommand::StopSession { .. } => panic!("expected CreateSession"),
        }
        match &resp.commands[1] {
            NodeCommand::StopSession { session_id, .. } => assert_eq!(session_id, "s1"),
            NodeCommand::CreateSession { .. } => panic!("expected StopSession"),
        }
    }

    #[test]
    fn test_parse_commands_response_invalid_json() {
        let json = b"not json";
        assert!(parse_commands_response(json).is_err());
    }
}
