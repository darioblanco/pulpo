use std::sync::Arc;

use pulpo_common::event::{PulpoEvent, SessionInterventionEvent};
use pulpo_common::session::{InterventionCode, Session, SessionStatus};

use super::{ReadyContext, memory::MemorySnapshot, resolve_backend_id};
use crate::backend::Backend;
use crate::store::Store;

/// Emit a `PulpoEvent::Intervention` after a session was forcibly stopped (best-effort).
///
/// Mirrors the DB record written by `update_session_intervention`, so a forced stop shows
/// up on the event plane (SSE + webhooks), not only in the interventions table.
pub(super) fn emit_intervention(
    ready_ctx: &ReadyContext,
    session: &Session,
    code: InterventionCode,
    reason: &str,
) {
    if let Some(tx) = &ready_ctx.event_tx {
        let _ = tx.send(PulpoEvent::Intervention(SessionInterventionEvent {
            session_id: session.id.to_string(),
            session_name: session.name.clone(),
            node_name: ready_ctx.node_name.clone(),
            code: code.to_string(),
            reason: reason.to_owned(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        }));
    }
}

pub(super) async fn intervene(
    backend: &Arc<dyn Backend>,
    store: &Store,
    snapshot: &MemorySnapshot,
    ready_ctx: &ReadyContext,
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
        emit_intervention(
            ready_ctx,
            session,
            InterventionCode::MemoryPressure,
            &reason,
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast;

    #[test]
    fn test_emit_intervention_sends_event_when_tx_present() {
        let (tx, mut rx) = broadcast::channel(8);
        let ctx = ReadyContext {
            event_tx: Some(tx),
            node_name: "node-x".into(),
        };
        let session = Session {
            name: "iv".into(),
            ..Default::default()
        };
        emit_intervention(
            &ctx,
            &session,
            InterventionCode::BudgetExceeded,
            "over budget",
        );
        match rx.try_recv().expect("intervention event") {
            PulpoEvent::Intervention(iv) => {
                assert_eq!(iv.code, "budget_exceeded");
                assert_eq!(iv.node_name, "node-x");
                assert_eq!(iv.reason, "over budget");
                assert_eq!(iv.session_name, "iv");
            }
            other => panic!("expected intervention, got {other:?}"),
        }
    }

    #[test]
    fn test_emit_intervention_is_noop_without_tx() {
        let ctx = ReadyContext {
            event_tx: None,
            node_name: "n".into(),
        };
        // No subscribers / no tx — must not panic.
        emit_intervention(
            &ctx,
            &Session::default(),
            InterventionCode::IdleTimeout,
            "idle",
        );
    }
}
