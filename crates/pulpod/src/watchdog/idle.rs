use std::path::Path;
use std::sync::Arc;

use pulpo_common::event::PulpoEvent;
use pulpo_common::session::{InterventionCode, Session, SessionStatus, status_reason};
use tracing::{debug, info};

use super::{
    HarnessSignals, IdleAction, IdleConfig, ReadyContext, build_session_event,
    detect_and_store_output_metadata, detect_waiting_for_input, owned_signals, resolve_backend_id,
};
use crate::backend::Backend;
use crate::store::Store;

/// Eagerly detect and resolve a dead backend for every `Working`/`Waiting`
/// session, entirely independent of whether idle-timeout detection/action is
/// enabled — a session's backend dying is a fact about the process itself, not
/// something an operator disabling the idle-timeout ALERT/KILL breaker
/// (`idle_timeout_secs = 0`) should silently also disable detecting. Called
/// unconditionally, every watchdog tick, from `run_watchdog_tick` — *before*
/// the `if cfg.idle.enabled` gate that guards `check_idle_sessions` (PR
/// #129/#118 follow-up: the sweep used to live entirely inside
/// `check_session_idle`, itself only ever reached when idle detection was
/// enabled).
///
/// Harmless overlap with `check_session_idle`'s own identical `is_alive()`
/// pre-check when idle detection *is* enabled: this sweep runs first and
/// re-fetches nothing afterward, so a session it just resolved to
/// `Done`/`Lost` no longer matches `check_idle_sessions`' own fresh
/// `Working`/`Waiting` fetch a moment later.
pub(super) async fn sweep_dead_backends(
    backend: &Arc<dyn Backend>,
    store: &Store,
    ready_ctx: &ReadyContext,
) {
    let sessions = super::list_sessions_or_warn(store, "Dead-backend sweep").await;
    for session in &sessions {
        if !matches!(
            session.status,
            SessionStatus::Working | SessionStatus::Waiting
        ) {
            continue;
        }
        let backend_id = resolve_backend_id(session, backend.as_ref());
        match backend.is_alive(&backend_id) {
            Ok(true) => {}
            Ok(false) => {
                resolve_and_report_dead_session(store, backend, &backend_id, session, ready_ctx)
                    .await;
            }
            #[allow(unused_variables)]
            Err(error) => {
                debug!(
                    "Dead-backend sweep: failed to check liveness for {}: {error}",
                    session.name
                );
            }
        }
    }
}

pub(super) async fn check_idle_sessions(
    backend: &Arc<dyn Backend>,
    store: &Store,
    idle_config: &IdleConfig,
    ready_ctx: &ReadyContext,
    extra_waiting_patterns: &[String],
) {
    let sessions = super::list_sessions_or_warn(store, "Idle check").await;

    let live: Vec<_> = sessions
        .iter()
        .filter(|session| {
            session.status == SessionStatus::Working || session.status == SessionStatus::Waiting
        })
        .collect();

    let now = chrono::Utc::now();
    let timeout =
        chrono::Duration::seconds(idle_config.timeout_secs.try_into().unwrap_or(i64::MAX));

    for session in live {
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

    // ADR 0009 removed the `Ready` sweep that used to live here: `wrap_command` no
    // longer keeps a fallback shell alive after the agent exits, so there is no more
    // persistent "done but still alive" backend state to sweep for a lagging exit
    // marker. A `Working`/`Waiting` session's dead backend (and its exit marker) is
    // resolved to `Done`/`Lost` by the shared `session::manager::resolve_dead_backend_session`
    // — called eagerly, right here, by `check_session_idle`'s own `is_alive()`
    // pre-check on every tick (so the `lifecycle` event/webhook fires within one
    // tick even with no API traffic at all), and also lazily by the next
    // `get_session`/`list_sessions` call or by `resume_lost_sessions` on startup,
    // for whichever of those happens to notice first. `Done` is a true terminal
    // status: nothing here revisits it. See `docs/operations/session-lifecycle.md`.
}

/// Resolve the effective Active→Idle threshold (seconds) for a session: the
/// session's own `idle_threshold_secs` overrides `idle_config.threshold_secs`
/// when set. `Some(0)` on the session disables the time-based transition for
/// that session entirely (never idle on elapsed time alone); `None` on the
/// session falls back to the global default. Returns `None` when the
/// time-based transition must never fire, `Some(secs)` otherwise.
pub(super) fn effective_idle_threshold_secs(
    session: &Session,
    idle_config: &IdleConfig,
) -> Option<u64> {
    match session.idle_threshold_secs {
        Some(0) => None,
        Some(secs) => Some(u64::from(secs)),
        None => Some(idle_config.threshold_secs),
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

    // Eager dead-backend resolution (ADR 0009 follow-up): a session's backend can
    // die between ticks — the agent exits, and `wrap_command` no longer keeps a
    // fallback shell open, so tmux tears the pane down in the same instant. Without
    // this pre-check, a `capture_output` failure below was the only signal, and
    // nothing on this path ever turned that into a `Done`/`Lost` transition — the
    // session sat in `Working`/`Waiting` until something else happened to call
    // `GET /sessions/{id}` (see `session::manager::resolve_dead_backend_session`'s
    // doc comment for the full story). Checking liveness explicitly, before trying
    // to capture output at all, means this tick itself resolves it and fires the
    // `lifecycle` event/webhook — not some future, possibly-never-arriving poll.
    match backend.is_alive(&backend_id) {
        Ok(true) => {}
        Ok(false) => {
            resolve_and_report_dead_session(store, backend, &backend_id, session, ready_ctx).await;
            return;
        }
        #[allow(unused_variables)]
        Err(error) => {
            debug!(
                "Idle check: failed to check liveness for {}: {error}",
                session.name
            );
            return;
        }
    }

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

    let signals = owned_signals(session);
    let exact_usage =
        crate::usage::read_exact_usage_for_session(session, Path::new(store.data_dir()));
    detect_and_store_output_metadata(store, session, &current_output, exact_usage, signals).await;

    let output_changed = session.output_snapshot.as_deref() != Some(current_output.as_str());
    if output_changed {
        handle_active_session(store, session, ready_ctx, signals).await;
        return;
    }

    if !signals.lifecycle && session.status == SessionStatus::Working {
        let immediate = detect_waiting_for_input(&current_output, extra_waiting_patterns);
        let effective_threshold_secs = effective_idle_threshold_secs(session, idle_config);
        let last_change = session.last_output_at.unwrap_or(session.created_at);
        let sustained = effective_threshold_secs.is_some_and(|threshold_secs| {
            (now - last_change).num_seconds() >= i64::try_from(threshold_secs).unwrap_or(i64::MAX)
        });
        if immediate || sustained {
            // A matched waiting-for-input pattern (a `(y/n)` prompt, `sudo
            // password:`, "I trust this folder", ...) really is "blocked on the
            // user" — reason `needs_input:permission` (these built-in patterns are
            // overwhelmingly approval/confirmation prompts). Sustained silence with
            // no such pattern is just a plain idle prompt, reason `idle`. See ADR
            // 0009: this is the scrollback-heuristic counterpart to a harness's own
            // `NeedsInput`/`TurnFinished` events.
            let reason = if immediate {
                status_reason::needs_input("permission")
            } else {
                status_reason::IDLE.to_owned()
            };
            info!(
                "Session {} idle ({}), transitioning to waiting",
                session.name,
                if immediate {
                    "waiting pattern"
                } else {
                    "output unchanged"
                }
            );
            #[allow(unused_variables)]
            if let Err(error) = store
                .update_session_status(
                    &session.id.to_string(),
                    SessionStatus::Waiting,
                    Some(&reason),
                )
                .await
            {
                coverage_warn!(
                    "Idle check: failed to transition {} to waiting: {error}",
                    session.name
                );
            } else if let Some(tx) = &ready_ctx.event_tx {
                let event = build_session_event(
                    session,
                    SessionStatus::Waiting,
                    Some(&reason),
                    Some(SessionStatus::Working),
                    &ready_ctx.node_name,
                    Some(current_output.clone()),
                );
                let _ = tx.send(PulpoEvent::Session(event));
            }
            return;
        }
    }

    // A session parked on `waiting:needs_input:<reason>` is blocked on a real
    // decision from the operator (a permission/approval prompt) — that's not
    // "idle" in the sense `idle_timeout_secs` means, it's waiting on a person who
    // might come back in five minutes or five hours. Only a session genuinely
    // stuck with no activity (`waiting:idle`, or a harness-owned `working`
    // producing no output at all) is subject to the idle-timeout alert/kill
    // action — exempt `needs_input` from it entirely rather than have the
    // breaker forcibly stop a session that's correctly doing exactly what it
    // should: waiting for you.
    let blocked_on_operator = session.status == SessionStatus::Waiting
        && session
            .status_reason
            .as_deref()
            .and_then(status_reason::needs_input_reason)
            .is_some();
    if blocked_on_operator {
        return;
    }

    handle_idle_session(
        backend,
        store,
        idle_config,
        session,
        now,
        timeout,
        ready_ctx,
    )
    .await;
}

/// A session's backend just failed `is_alive()` — resolve it to `Done`/`Lost` via
/// the shared `session::manager::resolve_dead_backend_session` and, on success,
/// emit the `lifecycle` event/webhook with the session's *real* previous status
/// (never a hardcoded guess). Best-effort: a failure here is logged and simply
/// leaves the session for the next tick (or the lazy `get_session`/`list_sessions`
/// path) to retry — never a hard error that would abort the whole watchdog tick.
///
/// `resolve_dead_backend_session` is itself a compare-and-set: `Ok(false)` means
/// a concurrent caller (another watchdog tick, or a `GET`/`list_sessions` racing
/// this one) already resolved the session first. Neither the "resolved" info log
/// nor the `lifecycle` event may fire in that case — the session's real status is
/// whatever that concurrent winner set, not `updated`'s stale, unmodified copy of
/// `session`, and logging/emitting here would be a duplicate for a transition
/// this call didn't actually make (see the doc comment on
/// `session::manager::resolve_dead_backend_session` itself).
#[cfg_attr(coverage, allow(unused_variables))]
pub(super) async fn resolve_and_report_dead_session(
    store: &Store,
    backend: &Arc<dyn Backend>,
    backend_id: &str,
    session: &Session,
    ready_ctx: &ReadyContext,
) {
    let previous = session.status;
    let mut updated = session.clone();
    let transitioned = match crate::session::manager::resolve_dead_backend_session(
        store,
        backend.as_ref(),
        backend_id,
        &mut updated,
    )
    .await
    {
        Ok(transitioned) => transitioned,
        Err(error) => {
            coverage_warn!(
                "Idle check: failed to resolve dead backend for {}: {error}",
                session.name
            );
            return;
        }
    };
    if !transitioned {
        debug!(
            session = %session.name,
            "Idle check: dead backend already resolved concurrently, skipping duplicate log/event"
        );
        return;
    }
    info!(
        "Session {} backend is gone, resolved {previous} -> {}",
        session.name, updated.status
    );
    if let Some(tx) = &ready_ctx.event_tx {
        let event = build_session_event(
            &updated,
            updated.status,
            updated.status_reason.as_deref(),
            Some(previous),
            &ready_ctx.node_name,
            updated.output_snapshot.clone(),
        );
        let _ = tx.send(PulpoEvent::Session(event));
    }
}

/// React to fresh output on a session: revert Idle→Active and clear `idle_since`.
///
/// `signals.lifecycle` gates this entirely: once a harness adapter owns lifecycle
/// signals (hook events are flowing), only its own events may decide status — a
/// mere TUI repaint (the output snapshot changing) must not revert a hook-driven
/// `Idle`/`needs_input` back to `Active`, since the very next watchdog tick would
/// otherwise undo a real `NeedsInput`/`TurnFinished` transition just because the
/// terminal redrew itself. Non-owned sessions (no harness, or its events aren't
/// flowing yet) keep the original scrollback-driven behavior unchanged.
pub(super) async fn handle_active_session(
    store: &Store,
    session: &Session,
    ready_ctx: &ReadyContext,
    signals: HarnessSignals,
) {
    if signals.lifecycle {
        return;
    }

    if session.status == SessionStatus::Waiting {
        info!(
            "Session {} has new output, transitioning back to active",
            session.name
        );
        #[allow(unused_variables)]
        if let Err(error) = store
            .update_session_status(&session.id.to_string(), SessionStatus::Working, None)
            .await
        {
            coverage_warn!(
                "Idle check: failed to transition {} back to active: {error}",
                session.name
            );
        } else if let Some(tx) = &ready_ctx.event_tx {
            let event = build_session_event(
                session,
                SessionStatus::Working,
                None,
                Some(SessionStatus::Waiting),
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
    now: chrono::DateTime<chrono::Utc>,
    timeout: chrono::Duration,
    ready_ctx: &ReadyContext,
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

            // Shared breaker path: captures a final output snapshot before the
            // kill, so idle-killed sessions keep their last output.
            if !super::intervention::stop_and_record(
                backend,
                store,
                session,
                InterventionCode::IdleTimeout,
                &reason,
                ready_ctx,
                "Idle check: failed to kill idle session",
                "Idle check: failed to record intervention",
            )
            .await
            {
                return;
            }
            coverage_warn!(
                "Idle check: stopped idle session {} after {minutes} minutes",
                session.name
            );
        }
    }
}
