use std::sync::Arc;

use pulpo_common::event::PulpoEvent;
use pulpo_common::session::{InterventionCode, Session, SessionStatus};
use tracing::{debug, info};

use super::{
    IdleAction, IdleConfig, ReadyContext, build_session_event, detect_agent_exited,
    detect_and_store_output_metadata, detect_waiting_for_input, resolve_backend_id,
};
use crate::backend::Backend;
use crate::store::Store;

pub(super) async fn check_idle_sessions(
    backend: &Arc<dyn Backend>,
    store: &Store,
    idle_config: &IdleConfig,
    ready_ctx: &ReadyContext,
    extra_waiting_patterns: &[String],
) {
    let sessions = match store.list_sessions().await {
        Ok(sessions) => sessions,
        #[allow(unused_variables)]
        Err(error) => {
            coverage_warn!("Idle check: failed to list sessions: {error}");
            return;
        }
    };

    let live: Vec<_> = sessions
        .into_iter()
        .filter(|session| {
            session.status == SessionStatus::Active || session.status == SessionStatus::Idle
        })
        .collect();

    let now = chrono::Utc::now();
    let timeout =
        chrono::Duration::seconds(idle_config.timeout_secs.try_into().unwrap_or(i64::MAX));

    for session in &live {
        check_session_idle(
            backend,
            store,
            idle_config,
            session,
            now,
            timeout,
            ready_ctx,
            extra_waiting_patterns,
        )
        .await;
    }
}

pub(super) async fn check_session_idle(
    backend: &Arc<dyn Backend>,
    store: &Store,
    idle_config: &IdleConfig,
    session: &Session,
    now: chrono::DateTime<chrono::Utc>,
    timeout: chrono::Duration,
    ready_ctx: &ReadyContext,
    extra_waiting_patterns: &[String],
) {
    let backend_id = resolve_backend_id(session, backend.as_ref());
    let current_output = match backend.capture_output(&backend_id, 500) {
        Ok(output) => output,
        #[allow(unused_variables)]
        Err(error) => {
            debug!(
                "Idle check: failed to capture output for {}: {error}",
                session.name
            );
            return;
        }
    };

    if detect_agent_exited(&current_output) {
        handle_session_ready(store, session, ready_ctx).await;
        return;
    }

    #[allow(unused_variables)]
    if let Err(error) = store
        .update_session_output_snapshot(&session.id.to_string(), &current_output)
        .await
    {
        coverage_warn!(
            "Idle check: failed to update output snapshot for {}: {error}",
            session.name
        );
        return;
    }

    detect_and_store_output_metadata(store, session, &current_output).await;

    let output_changed = session.output_snapshot.as_deref() != Some(current_output.as_str());
    if output_changed {
        handle_active_session(store, session, ready_ctx).await;
        return;
    }

    if session.status == SessionStatus::Active {
        let immediate = detect_waiting_for_input(&current_output, extra_waiting_patterns);
        let last_change = session.last_output_at.unwrap_or(session.created_at);
        let sustained = (now - last_change).num_seconds()
            >= i64::try_from(idle_config.threshold_secs).unwrap_or(i64::MAX);
        if immediate || sustained {
            info!(
                "Session {} idle ({}), transitioning to idle",
                session.name,
                if immediate {
                    "waiting pattern"
                } else {
                    "output unchanged"
                }
            );
            #[allow(unused_variables)]
            if let Err(error) = store
                .update_session_status(&session.id.to_string(), SessionStatus::Idle)
                .await
            {
                coverage_warn!(
                    "Idle check: failed to transition {} to idle: {error}",
                    session.name
                );
            } else if let Some(tx) = &ready_ctx.event_tx {
                let event = build_session_event(
                    session,
                    SessionStatus::Idle,
                    Some(SessionStatus::Active),
                    &ready_ctx.node_name,
                    Some(current_output.clone()),
                );
                let _ = tx.send(PulpoEvent::Session(event));
            }
            return;
        }
    }

    handle_idle_session(
        backend,
        store,
        idle_config,
        session,
        &backend_id,
        now,
        timeout,
    )
    .await;
}

pub(super) async fn handle_session_ready(store: &Store, session: &Session, ctx: &ReadyContext) {
    let previous = session.status;
    info!(
        session_name = %session.name,
        "Agent exited, transitioning to ready"
    );
    #[allow(unused_variables)]
    if let Err(error) = store
        .update_session_status(&session.id.to_string(), SessionStatus::Ready)
        .await
    {
        coverage_warn!(
            session_name = %session.name,
            "Failed to transition to ready: {error}"
        );
        return;
    }

    if let Some(tx) = &ctx.event_tx {
        let event = build_session_event(
            session,
            SessionStatus::Ready,
            Some(previous),
            &ctx.node_name,
            session.output_snapshot.clone(),
        );
        let _ = tx.send(PulpoEvent::Session(event));
    }
}

pub(super) async fn handle_active_session(
    store: &Store,
    session: &Session,
    ready_ctx: &ReadyContext,
) {
    if session.status == SessionStatus::Idle {
        info!(
            "Session {} has new output, transitioning back to active",
            session.name
        );
        #[allow(unused_variables)]
        if let Err(error) = store
            .update_session_status(&session.id.to_string(), SessionStatus::Active)
            .await
        {
            coverage_warn!(
                "Idle check: failed to transition {} back to active: {error}",
                session.name
            );
        } else if let Some(tx) = &ready_ctx.event_tx {
            let event = build_session_event(
                session,
                SessionStatus::Active,
                Some(SessionStatus::Idle),
                &ready_ctx.node_name,
                session.output_snapshot.clone(),
            );
            let _ = tx.send(PulpoEvent::Session(event));
        }
    }

    if session.idle_since.is_none() {
        return;
    }

    info!(
        "Idle check: session {} active again, clearing idle status",
        session.name
    );
    #[allow(unused_variables)]
    if let Err(error) = store
        .clear_session_idle_since(&session.id.to_string())
        .await
    {
        coverage_warn!(
            "Idle check: failed to clear idle_since for {}: {error}",
            session.name
        );
    }
}

pub(super) async fn handle_idle_session(
    backend: &Arc<dyn Backend>,
    store: &Store,
    idle_config: &IdleConfig,
    session: &Session,
    backend_id: &str,
    now: chrono::DateTime<chrono::Utc>,
    timeout: chrono::Duration,
) {
    let last_activity = session.last_output_at.unwrap_or(session.created_at);
    let idle_duration = now - last_activity;

    if idle_duration <= timeout {
        return;
    }

    let minutes = idle_duration.num_minutes();
    match idle_config.action {
        IdleAction::Alert => {
            if session.idle_since.is_none() {
                coverage_warn!(
                    "Idle check: session {} idle for {minutes} minutes, marking as idle",
                    session.name
                );
                #[allow(unused_variables)]
                if let Err(error) = store
                    .update_session_idle_since(&session.id.to_string())
                    .await
                {
                    coverage_warn!(
                        "Idle check: failed to set idle_since for {}: {error}",
                        session.name
                    );
                }
            }
        }
        IdleAction::Kill => {
            let reason = format!("Idle for {minutes} minutes");

            #[allow(unused_variables)]
            if let Err(error) = backend.kill_session(backend_id) {
                coverage_warn!(
                    "Idle check: failed to kill idle session {}: {error}",
                    session.name
                );
                return;
            }

            #[allow(unused_variables)]
            if let Err(error) = store
                .update_session_intervention(
                    &session.id.to_string(),
                    InterventionCode::IdleTimeout,
                    &reason,
                )
                .await
            {
                coverage_warn!(
                    "Idle check: failed to record intervention for {}: {error}",
                    session.name
                );
            }
            if let Some(worktree_path) = &session.worktree_path {
                crate::session::manager::cleanup_worktree(worktree_path, &session.workdir);
            }
            coverage_warn!(
                "Idle check: stopped idle session {} after {minutes} minutes",
                session.name
            );
        }
    }
}

pub(super) async fn cleanup_ready_sessions(
    backend: &Arc<dyn Backend>,
    store: &Store,
    ready_ttl_secs: u64,
) {
    let sessions = match store.list_sessions().await {
        Ok(sessions) => sessions,
        #[allow(unused_variables)]
        Err(error) => {
            coverage_warn!("Ready cleanup: failed to list sessions: {error}");
            return;
        }
    };

    let now = chrono::Utc::now();
    let ttl = chrono::Duration::seconds(ready_ttl_secs.try_into().unwrap_or(i64::MAX));

    for session in sessions
        .iter()
        .filter(|session| session.status == SessionStatus::Ready)
    {
        let age = now - session.updated_at;
        if age <= ttl {
            continue;
        }

        let backend_id = resolve_backend_id(session, backend.as_ref());
        #[allow(unused_variables)]
        if let Err(error) = backend.kill_session(&backend_id) {
            debug!(
                session_name = %session.name,
                "Ready cleanup: tmux already gone: {error}"
            );
        }
        #[allow(unused_variables)]
        if let Err(error) = store
            .update_session_status(&session.id.to_string(), SessionStatus::Stopped)
            .await
        {
            coverage_warn!(
                session_name = %session.name,
                "Ready cleanup: failed to mark stopped: {error}"
            );
        } else {
            info!(
                session_name = %session.name,
                age_secs = age.num_seconds(),
                "Ready cleanup: stopped tmux shell after TTL"
            );
        }
    }
}
