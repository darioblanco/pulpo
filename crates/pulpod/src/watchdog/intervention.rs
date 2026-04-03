use std::sync::Arc;

use pulpo_common::session::{InterventionCode, SessionStatus};
use tracing::warn;

use super::{memory::MemorySnapshot, resolve_backend_id};
use crate::backend::Backend;
use crate::store::Store;

pub(super) async fn intervene(
    backend: &Arc<dyn Backend>,
    store: &Store,
    snapshot: &MemorySnapshot,
) {
    let sessions = match store.list_sessions().await {
        Ok(s) => s,
        Err(e) => {
            warn!("Watchdog: failed to list sessions: {e}");
            return;
        }
    };

    let running: Vec<_> = sessions
        .into_iter()
        .filter(|s| s.status == SessionStatus::Active)
        .collect();

    if running.is_empty() {
        let usage = snapshot.usage_percent();
        warn!(
            usage,
            "Memory pressure but no running sessions to intervene on"
        );
        return;
    }

    for session in &running {
        let bid = resolve_backend_id(session, backend.as_ref());
        match backend.capture_output(&bid, 500) {
            Ok(output) => {
                if let Err(e) = store
                    .update_session_output_snapshot(&session.id.to_string(), &output)
                    .await
                {
                    warn!(
                        session_id = %session.id,
                        session_name = %session.name,
                        "Failed to save output snapshot: {e}"
                    );
                }
            }
            Err(e) => {
                warn!(
                    session_id = %session.id,
                    session_name = %session.name,
                    "Failed to capture output before intervention: {e}"
                );
            }
        }

        if let Err(e) = backend.kill_session(&bid) {
            warn!(
                session_id = %session.id,
                session_name = %session.name,
                "Failed to kill session during intervention (session still alive): {e}"
            );
            continue;
        }

        let reason = format!(
            "Memory usage {}% ({}/{}MB available)",
            snapshot.usage_percent(),
            snapshot.available_mb,
            snapshot.total_mb
        );
        if let Err(e) = store
            .update_session_intervention(
                &session.id.to_string(),
                InterventionCode::MemoryPressure,
                &reason,
            )
            .await
        {
            warn!(
                session_id = %session.id,
                session_name = %session.name,
                "Failed to record intervention: {e}"
            );
        }
        if let Some(ref wt_path) = session.worktree_path {
            crate::session::manager::cleanup_worktree(wt_path, &session.workdir);
        }
        let usage = snapshot.usage_percent();
        warn!(
            session_id = %session.id,
            session_name = %session.name,
            usage,
            available_mb = snapshot.available_mb,
            total_mb = snapshot.total_mb,
            "Watchdog intervention: stopped session due to memory pressure"
        );
    }
}
