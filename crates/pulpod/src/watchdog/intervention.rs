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

/// Stop a session via the standard intervention path shared by every breaker
/// (memory pressure, budget, burn ceiling, idle timeout):
///
/// 1. capture a final output snapshot (best-effort, warn on failure),
/// 2. kill the backend session (warn + return `false` on failure so the caller
///    can retry on the next tick — nothing is recorded for a still-alive session),
/// 3. record the intervention in the store (warn on failure),
/// 4. emit the `PulpoEvent::Intervention` event,
/// 5. clean up the session's worktree, if any.
///
/// `kill_fail_msg`/`record_fail_msg` preserve each call site's log wording.
/// Returns `true` when the session was killed (callers log their own success line).
#[cfg_attr(coverage, allow(unused_variables))]
pub(super) async fn stop_and_record(
    backend: &Arc<dyn Backend>,
    store: &Store,
    session: &Session,
    code: InterventionCode,
    reason: &str,
    ready_ctx: &ReadyContext,
    kill_fail_msg: &str,
    record_fail_msg: &str,
) -> bool {
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
            "{kill_fail_msg}: {error}"
        );
        return false;
    }

    #[allow(unused_variables)]
    if let Err(error) = store
        .update_session_intervention(&session.id.to_string(), code, reason)
        .await
    {
        coverage_warn!(
            session_id = %session.id,
            session_name = %session.name,
            "{record_fail_msg}: {error}"
        );
    }
    emit_intervention(ready_ctx, session, code, reason);
    if let Some(ref wt_path) = session.worktree_path {
        // Mirror `session::manager`'s own guard on the normal stop/purge/cleanup
        // paths: a worktree `pulpo handoff` made two sessions share must survive a
        // forced intervention (budget/burn/idle/memory stop) on just one of them —
        // it's only reclaimed once every referencing session is dead.
        let in_use = store
            .worktree_in_use_elsewhere(wt_path, &session.id.to_string())
            .await
            .unwrap_or_else(|error| {
                coverage_warn!(
                    session_id = %session.id,
                    session_name = %session.name,
                    "Failed to check worktree usage before intervention cleanup, leaving worktree in place: {error}"
                );
                true
            });
        if in_use {
            tracing::debug!(
                session_id = %session.id,
                session_name = %session.name,
                path = %wt_path,
                "Skipping worktree cleanup on intervention — still referenced by another session"
            );
        } else {
            crate::session::manager::cleanup_worktree(wt_path, &session.workdir);
        }
    }
    true
}

pub(super) async fn intervene(
    backend: &Arc<dyn Backend>,
    store: &Store,
    snapshot: &MemorySnapshot,
    ready_ctx: &ReadyContext,
) {
    let sessions = super::list_sessions_or_warn(store, "Watchdog").await;

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
        let reason = format!(
            "Memory usage {}% ({}/{}MB available)",
            snapshot.usage_percent(),
            snapshot.available_mb,
            snapshot.total_mb
        );
        if !stop_and_record(
            backend,
            store,
            session,
            InterventionCode::MemoryPressure,
            &reason,
            ready_ctx,
            "Failed to kill session during intervention (session still alive)",
            "Failed to record intervention",
        )
        .await
        {
            continue;
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

    // -- stop_and_record: worktree cleanup must respect sharing --

    fn ready_ctx() -> ReadyContext {
        ReadyContext {
            event_tx: None,
            node_name: "test-node".into(),
        }
    }

    fn session_with_worktree(name: &str, worktree_path: &str) -> Session {
        Session {
            id: uuid::Uuid::new_v4(),
            name: name.into(),
            workdir: "/tmp/repo".into(),
            command: "claude".into(),
            status: SessionStatus::Active,
            backend_session_id: Some(name.into()),
            worktree_path: Some(worktree_path.into()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_stop_and_record_preserves_worktree_shared_with_another_live_session() {
        // Two sessions (as `pulpo handoff` produces) sharing one worktree path — an
        // intervention (budget/burn/idle/memory stop) on one must not delete the
        // worktree (or its branch) out from under the other, still-live session.
        let store = crate::store::test_store().await;
        let tmp = tempfile::tempdir().unwrap();
        let wt_path = tmp.path().to_str().unwrap().to_owned();

        let session_a = session_with_worktree("shared-a", &wt_path);
        let session_b = session_with_worktree("shared-b", &wt_path);
        store.insert_session(&session_a).await.unwrap();
        store.insert_session(&session_b).await.unwrap();

        let backend: Arc<dyn Backend> = Arc::new(crate::backend::StubBackend);
        let stopped = stop_and_record(
            &backend,
            &store,
            &session_a,
            InterventionCode::IdleTimeout,
            "idle for too long",
            &ready_ctx(),
            "kill failed",
            "record failed",
        )
        .await;

        assert!(stopped);
        assert!(
            std::path::Path::new(&wt_path).exists(),
            "worktree shared with a still-live session must survive the intervention"
        );
    }

    #[tokio::test]
    async fn test_stop_and_record_cleans_up_worktree_when_not_shared() {
        // Sanity check for the guard above: a worktree with no other live session
        // referencing it is still cleaned up as before.
        let store = crate::store::test_store().await;
        let tmp = tempfile::tempdir().unwrap();
        let wt_path = tmp.path().to_str().unwrap().to_owned();

        let session = session_with_worktree("solo", &wt_path);
        store.insert_session(&session).await.unwrap();

        let backend: Arc<dyn Backend> = Arc::new(crate::backend::StubBackend);
        let stopped = stop_and_record(
            &backend,
            &store,
            &session,
            InterventionCode::IdleTimeout,
            "idle for too long",
            &ready_ctx(),
            "kill failed",
            "record failed",
        )
        .await;

        assert!(stopped);
        assert!(
            !std::path::Path::new(&wt_path).exists(),
            "an unshared worktree should still be cleaned up on intervention"
        );
    }
}
