use std::sync::Arc;

use pulpo_common::session::{InterventionCode, SessionStatus};

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
        #[allow(unused_variables)]
        Err(error) => {
            coverage_warn!("Watchdog: failed to list sessions: {error}");
            return;
        }
    };

    let running: Vec<_> = sessions
        .into_iter()
        .filter(|s| s.status == SessionStatus::Active)
        .collect();

    if running.is_empty() {
        let _usage = snapshot.usage_percent();
        coverage_warn!(
            _usage,
            "Memory pressure but no running sessions to intervene on"
        );
        return;
    }

    for session in &running {
        let bid = resolve_backend_id(session, backend.as_ref());
        match backend.capture_output(&bid, 500) {
            Ok(output) => {
                #[allow(unused_variables)]
                if let Err(error) = store
                    .update_session_output_snapshot(&session.id.to_string(), &output)
                    .await
                {
                    coverage_warn!(
                        session_id = %session.id,
                        session_name = %session.name,
                        "Failed to save output snapshot: {error}"
                    );
                }
            }
            #[allow(unused_variables)]
            Err(error) => {
                coverage_warn!(
                    session_id = %session.id,
                    session_name = %session.name,
                    "Failed to capture output before intervention: {error}"
                );
            }
        }

        #[allow(unused_variables)]
        if let Err(error) = backend.kill_session(&bid) {
            coverage_warn!(
                session_id = %session.id,
                session_name = %session.name,
                "Failed to kill session during intervention (session still alive): {error}"
            );
            continue;
        }

        let reason = format!(
            "Memory usage {}% ({}/{}MB available)",
            snapshot.usage_percent(),
            snapshot.available_mb,
            snapshot.total_mb
        );
        #[allow(unused_variables)]
        if let Err(error) = store
            .update_session_intervention(
                &session.id.to_string(),
                InterventionCode::MemoryPressure,
                &reason,
            )
            .await
        {
            coverage_warn!(
                session_id = %session.id,
                session_name = %session.name,
                "Failed to record intervention: {error}"
            );
        }
        if let Some(ref wt_path) = session.worktree_path {
            crate::session::manager::cleanup_worktree(wt_path, &session.workdir);
        }
        let _usage = snapshot.usage_percent();
        coverage_warn!(
            session_id = %session.id,
            session_name = %session.name,
            _usage,
            available_mb = snapshot.available_mb,
            total_mb = snapshot.total_mb,
            "Watchdog intervention: stopped session due to memory pressure"
        );
    }
}
