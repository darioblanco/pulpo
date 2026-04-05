use std::collections::HashSet;
use std::sync::Arc;

use pulpo_common::event::{PulpoEvent, SessionEvent};
use pulpo_common::session::{Session, SessionStatus};
use tracing::{debug, info};

use super::ReadyContext;
use crate::backend::Backend;
use crate::store::Store;

/// Known agent process names — adopted as Active.
const AGENT_PROCESSES: &[&str] = &["claude", "codex", "gemini", "opencode"];

/// Known shell process names — adopted as Ready.
const SHELL_PROCESSES: &[&str] = &["bash", "zsh", "sh", "fish", "nu"];

/// Determine the status for an adopted tmux session based on its running process.
pub(super) fn classify_adopted_process(process: &str) -> SessionStatus {
    let lower = process.to_lowercase();
    if AGENT_PROCESSES.iter().any(|agent| lower.contains(agent)) {
        SessionStatus::Active
    } else if SHELL_PROCESSES.iter().any(|shell| lower == *shell) {
        SessionStatus::Ready
    } else {
        // Unknown process — conservatively treat as Active.
        SessionStatus::Active
    }
}

/// Auto-discover tmux sessions not tracked by pulpo and adopt them.
#[allow(clippy::too_many_lines)]
pub(super) async fn adopt_tmux_sessions(
    backend: &Arc<dyn Backend>,
    store: &Store,
    ctx: &ReadyContext,
) {
    let tmux_sessions = match backend.list_sessions() {
        Ok(sessions) => sessions,
        Err(error) => {
            debug!("Adopt: failed to list tmux sessions: {error}");
            return;
        }
    };

    if tmux_sessions.is_empty() {
        return;
    }

    let pulpo_sessions = match store.list_sessions().await {
        Ok(sessions) => sessions,
        #[allow(unused_variables)]
        Err(error) => {
            coverage_warn!("Adopt: failed to list pulpo sessions: {error}");
            return;
        }
    };

    let live_statuses = [
        SessionStatus::Creating,
        SessionStatus::Active,
        SessionStatus::Idle,
        SessionStatus::Ready,
    ];
    let known_ids: HashSet<String> = pulpo_sessions
        .iter()
        .filter(|session| live_statuses.contains(&session.status))
        .filter_map(|session| session.backend_session_id.clone())
        .collect();
    let known_names: HashSet<&str> = pulpo_sessions
        .iter()
        .filter(|session| live_statuses.contains(&session.status))
        .map(|session| session.name.as_str())
        .collect();

    for (tmux_id, tmux_name) in &tmux_sessions {
        if known_ids.contains(tmux_id)
            || known_ids.contains(tmux_name)
            || known_names.contains(tmux_name.as_str())
        {
            continue;
        }

        if tmux_name.starts_with("claude-") && tmux_name.len() > 10 {
            debug!("Adopt: skipping Claude teammate session {tmux_name}");
            continue;
        }

        let (process, workdir) = match backend.pane_info(tmux_name) {
            Ok(info) => info,
            Err(error) => {
                debug!("Adopt: failed to get pane info for {tmux_name}: {error}");
                continue;
            }
        };

        let status = classify_adopted_process(&process);
        let command = backend
            .pane_command_line(tmux_id)
            .unwrap_or_else(|_| process.clone());

        let session = Session {
            id: uuid::Uuid::new_v4(),
            name: tmux_name.clone(),
            workdir,
            command,
            description: Some("Adopted from tmux".into()),
            status,
            backend_session_id: Some(tmux_id.clone()),
            ..Default::default()
        };

        #[allow(unused_variables)]
        if let Err(error) = store.insert_session(&session).await {
            coverage_warn!("Adopt: failed to insert session {tmux_name}: {error}");
            continue;
        }

        if let Err(error) = backend.set_env(tmux_name, "PULPO_SESSION_ID", &session.id.to_string())
        {
            debug!("Adopt: failed to set PULPO_SESSION_ID for {tmux_name}: {error}");
        }
        if let Err(error) = backend.set_env(tmux_name, "PULPO_SESSION_NAME", tmux_name) {
            debug!("Adopt: failed to set PULPO_SESSION_NAME for {tmux_name}: {error}");
        }

        info!(
            session_name = %tmux_name,
            process = %process,
            status = %status,
            "Adopted external tmux session"
        );

        if let Some(tx) = &ctx.event_tx {
            let event = SessionEvent {
                session_id: session.id.to_string(),
                session_name: tmux_name.clone(),
                status: status.to_string(),
                previous_status: None,
                node_name: ctx.node_name.clone(),
                output_snippet: None,
                timestamp: chrono::Utc::now().to_rfc3339(),
                ..Default::default()
            };
            let _ = tx.send(PulpoEvent::Session(event));
        }
    }
}
