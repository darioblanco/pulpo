use pulpo_common::event::SessionEvent;
use pulpo_common::session::{Session, SessionStatus, meta, status_reason};
use tracing::info;

use super::output_patterns;
use crate::harness::HarnessSignals;
use crate::store::Store;
use crate::usage::ExactUsage;

/// Detect PR URL, branch name, rate limits, errors, and token usage from session output.
/// PR and branch are only written if not already present. Transient signals (rate limits,
/// errors) are always updated and cleared when no longer detected.
///
/// `exact_usage`, when present (read from the agent's own session files by a structured
/// reader — Claude, Codex, or pi), is the only source of token/cost metadata. A session
/// run with an agent that has no structured reader records no usage at all — there is no
/// output-scraping fallback.
///
/// `signals` (see `harness::HarnessSignals`, resolved by `watchdog::owned_signals`)
/// says which of rate-limit and error-status scraping stay skipped: each is skipped
/// only when the session's own harness adapter says it owns that signal — a
/// harness's own `StopFailure`-shaped event owns it instead (see `watchdog::idle`
/// and `harness::transition_for_event`). An adapter with hooks for lifecycle but not
/// errors/rate-limits (Codex has neither hook) leaves the corresponding heuristic
/// running even while its other events flow. PR/branch detection and usage tracking
/// are unaffected; they aren't part of the scrollback-heuristic state machine hooks
/// replace.
#[allow(clippy::too_many_lines)]
pub(super) async fn detect_and_store_output_metadata(
    store: &Store,
    session: &Session,
    output: &str,
    exact_usage: Option<ExactUsage>,
    signals: HarnessSignals,
) {
    let has_pr = session.meta_str(meta::PR_URL).is_some();
    if !has_pr && let Some(pr_url) = output_patterns::extract_pr_url(output) {
        #[allow(unused_variables)]
        if let Err(error) = store
            .update_session_metadata_field(&session.id.to_string(), meta::PR_URL, &pr_url)
            .await
        {
            coverage_warn!(
                session_name = %session.name,
                "Failed to store pr_url metadata: {error}"
            );
        } else {
            info!(
                session_name = %session.name,
                pr_url = %pr_url,
                "Detected PR URL from session output"
            );
        }
    }

    let has_branch = session.meta_str(meta::BRANCH).is_some();
    if !has_branch && let Some(branch) = output_patterns::extract_branch(output) {
        #[allow(unused_variables)]
        if let Err(error) = store
            .update_session_metadata_field(&session.id.to_string(), meta::BRANCH, &branch)
            .await
        {
            coverage_warn!(
                session_name = %session.name,
                "Failed to store branch metadata: {error}"
            );
        } else {
            info!(
                session_name = %session.name,
                branch = %branch,
                "Detected branch from session output"
            );
        }
    }

    // Rate-limit scraping: skipped only once the session's own harness adapter owns
    // that signal — its own `Failed{rate_limited: true}`-shaped event is
    // authoritative then, and scrollback heuristics would otherwise fight with it.
    // An adapter with no rate-limit hook (Codex today) leaves this running.
    if !signals.rate_limit
        && let Some(rate_msg) = output_patterns::detect_rate_limit(output)
    {
        let timestamp = chrono::Utc::now().to_rfc3339();
        #[allow(unused_variables)]
        if let Err(error) = store
            .update_session_metadata_field(&session.id.to_string(), meta::RATE_LIMIT, &rate_msg)
            .await
        {
            coverage_warn!(
                session_name = %session.name,
                "Failed to store rate_limit metadata: {error}"
            );
        }
        #[allow(unused_variables)]
        if let Err(error) = store
            .update_session_metadata_field(&session.id.to_string(), meta::RATE_LIMIT_AT, &timestamp)
            .await
        {
            coverage_warn!(
                session_name = %session.name,
                "Failed to store rate_limit_at metadata: {error}"
            );
        } else {
            info!(
                session_name = %session.name,
                rate_limit = %rate_msg,
                "Detected rate limit from session output"
            );
        }
    }

    // Error-status scraping: same per-signal gate, independent of rate-limit above —
    // an adapter could in principle own one signal without the other.
    if !signals.error {
        let current_error = output_patterns::detect_error(output);
        let stored_error = session.meta_str(meta::ERROR_STATUS);
        match (&current_error, stored_error) {
            (Some(error_status), _) => {
                let timestamp = chrono::Utc::now().to_rfc3339();
                #[allow(unused_variables)]
                if let Err(error) = store
                    .update_session_metadata_field(
                        &session.id.to_string(),
                        meta::ERROR_STATUS,
                        error_status,
                    )
                    .await
                {
                    coverage_warn!(
                        session_name = %session.name,
                        "Failed to store error_status metadata: {error}"
                    );
                }
                let _ = store
                    .update_session_metadata_field(
                        &session.id.to_string(),
                        meta::ERROR_STATUS_AT,
                        &timestamp,
                    )
                    .await;
            }
            (None, Some(_)) => {
                #[allow(unused_variables)]
                if let Err(error) = store
                    .remove_session_metadata_field(&session.id.to_string(), meta::ERROR_STATUS)
                    .await
                {
                    coverage_warn!(
                        session_name = %session.name,
                        "Failed to clear error_status metadata: {error}"
                    );
                }
                let _ = store
                    .remove_session_metadata_field(&session.id.to_string(), meta::ERROR_STATUS_AT)
                    .await;
            }
            (None, None) => {}
        }
    }

    if let Some(exact) = exact_usage {
        store_exact_usage(store, session, &exact).await;
    }
}

/// Append `(key, value)` to `updates` when it differs from the stored metadata value.
fn push_if_changed(
    updates: &mut Vec<(&'static str, String)>,
    session: &Session,
    key: &'static str,
    value: String,
) {
    if session.meta_str(key) != Some(value.as_str()) {
        updates.push((key, value));
    }
}

/// Append quota-window fields for one rate-limit window.
fn push_quota_window(
    updates: &mut Vec<(&'static str, String)>,
    session: &Session,
    window: &crate::usage::QuotaWindow,
    used_key: &'static str,
    minutes_key: &'static str,
    resets_key: &'static str,
) {
    push_if_changed(
        updates,
        session,
        used_key,
        format!("{}", window.used_percent),
    );
    if let Some(minutes) = window.window_minutes {
        push_if_changed(updates, session, minutes_key, minutes.to_string());
    }
    if let Some(resets_at) = window.resets_at {
        push_if_changed(updates, session, resets_key, resets_at.to_string());
    }
}

/// Read a session's exact usage from its own on-disk agent files and persist it
/// (`session_cost_usd` + token counts) via [`store_exact_usage`] — the same
/// persistence [`detect_and_store_output_metadata`] already runs on every watchdog
/// tick for `Active`/`Idle` sessions.
///
/// Call this at every transition into `Ready` or `Stopped` too: the idle-sweep
/// loop (`watchdog::idle::check_idle_sessions`) only ever visits `Active`/`Idle`
/// sessions, so a session that reaches a terminal status (the exit-marker path in
/// `watchdog::idle`, a harness's own `SessionEnded` event in
/// `session::manager::apply_harness_event`, or an explicit `pulpo stop`) before the
/// next tick would otherwise never get its final cost recorded — the bug this
/// fixes (a real run ending with `session_cost_usd` still unset even though its
/// transcript had the data all along).
///
/// Gated like [`crate::usage::read_exact_usage_for_session`] (the actual
/// filesystem read this wraps) — untestable I/O under coverage, so this delegates
/// to a no-op stub there. [`store_exact_usage`], the persistence half, stays
/// coverage-included and is unit-tested directly with a synthetic `ExactUsage`.
///
/// The filesystem read itself runs in [`tokio::task::spawn_blocking`]: this is
/// called from request-handler contexts too (`api::usage::session_with_on_demand_usage`),
/// where synchronous file I/O on the async worker thread would otherwise briefly
/// stall every other request that worker is serving. When nothing is found,
/// persists a negative `usage_source = "none"` marker via
/// [`mark_usage_source_none`] so a session with no exact usage to read (an
/// unsupported harness, or a transcript with nothing usable) isn't re-read from
/// disk on every subsequent on-demand call.
#[cfg(not(coverage))]
pub(crate) async fn refresh_exact_usage(store: &Store, session: &Session) {
    let session_owned = session.clone();
    let data_dir = store.data_dir().to_owned();
    let exact_usage = tokio::task::spawn_blocking(move || {
        crate::usage::read_exact_usage_for_session(&session_owned, std::path::Path::new(&data_dir))
    })
    .await
    .unwrap_or(None);
    match exact_usage {
        Some(exact) => store_exact_usage(store, session, &exact).await,
        None => mark_usage_source_none(store, session).await,
    }
}

/// No-op stub under coverage builds (no real filesystem access) — see
/// [`crate::usage::read_exact_usage_for_session`]'s matching stub.
#[cfg(coverage)]
pub(crate) async fn refresh_exact_usage(_store: &Store, _session: &Session) {}

/// Persist a negative usage marker (`usage_source = "none"`) so a session whose
/// exact usage genuinely can't be read (no structured reader matched its
/// command, or its agent transcript has nothing usable) isn't re-read from disk
/// on every subsequent on-demand call (`api::usage::session_with_on_demand_usage`).
/// Only fills in an absent value — never overwrites a `usage_source` a previous,
/// successful read already recorded (e.g. before a session was resumed and its
/// transcript still has the earlier run's data).
async fn mark_usage_source_none(store: &Store, session: &Session) {
    if session.meta_str(meta::USAGE_SOURCE).is_some() {
        return;
    }
    let _ = store
        .batch_update_session_metadata(
            &session.id.to_string(),
            &[(meta::USAGE_SOURCE, "none")],
            &[],
        )
        .await;
}

/// Store exact usage read from the agent's own session files.
///
/// Unlike scraped usage, these values are session-lifetime totals computed fresh on
/// every tick (restarts produce new files that are summed), so they overwrite rather
/// than accumulate. Only changed values are written.
async fn store_exact_usage(store: &Store, session: &Session, exact: &ExactUsage) {
    let session_id = session.id.to_string();
    let mut updates: Vec<(&'static str, String)> = Vec::new();

    push_if_changed(
        &mut updates,
        session,
        meta::USAGE_SOURCE,
        exact.source.to_owned(),
    );
    push_if_changed(
        &mut updates,
        session,
        meta::TOTAL_INPUT_TOKENS,
        exact.input_tokens.to_string(),
    );
    push_if_changed(
        &mut updates,
        session,
        meta::TOTAL_OUTPUT_TOKENS,
        exact.output_tokens.to_string(),
    );
    push_if_changed(
        &mut updates,
        session,
        meta::CACHE_WRITE_TOKENS,
        exact.cache_write_tokens.to_string(),
    );
    push_if_changed(
        &mut updates,
        session,
        meta::CACHE_READ_TOKENS,
        exact.cache_read_tokens.to_string(),
    );
    if let Some(cost) = exact.cost_usd {
        push_if_changed(
            &mut updates,
            session,
            meta::SESSION_COST_USD,
            format!("{cost:.6}"),
        );
    }
    if let Some(quota) = &exact.quota {
        if let Some(primary) = &quota.primary {
            push_quota_window(
                &mut updates,
                session,
                primary,
                meta::QUOTA_PRIMARY_USED_PERCENT,
                meta::QUOTA_PRIMARY_WINDOW_MINUTES,
                meta::QUOTA_PRIMARY_RESETS_AT,
            );
        }
        if let Some(secondary) = &quota.secondary {
            push_quota_window(
                &mut updates,
                session,
                secondary,
                meta::QUOTA_SECONDARY_USED_PERCENT,
                meta::QUOTA_SECONDARY_WINDOW_MINUTES,
                meta::QUOTA_SECONDARY_RESETS_AT,
            );
        }
        if let Some(plan) = &quota.plan {
            push_if_changed(&mut updates, session, meta::QUOTA_PLAN, plan.clone());
        }
    }

    if updates.is_empty() {
        return;
    }

    let refs: Vec<(&str, &str)> = updates
        .iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect();
    let _ = store
        .batch_update_session_metadata(&session_id, &refs, &[])
        .await;
}

/// Build a `SessionEvent` from a session, populating token/cost enrichment from
/// metadata. `reason` is the *new* `status_reason` to pair with `status` — `session`
/// itself still carries the pre-transition status/reason, so callers pass the target
/// status/reason explicitly rather than reading `session.status`/`status_reason`.
/// `session.exit_code` is used as-is — callers resolving a dead backend update it on
/// `session` themselves before calling this, so it always reflects the transition
/// being reported here rather than a stale pre-transition value.
pub(super) fn build_session_event(
    session: &Session,
    status: SessionStatus,
    reason: Option<&str>,
    previous: Option<SessionStatus>,
    node_name: &str,
    output: Option<String>,
) -> SessionEvent {
    // Deprecated-for-one-release compatibility field (ADR 0009): derived from
    // `reason` rather than a separately-written `needs_input` metadata key.
    let needs_input = reason
        .and_then(status_reason::needs_input_reason)
        .map(str::to_owned);
    SessionEvent {
        session_id: session.id.to_string(),
        session_name: session.name.clone(),
        status: status.to_string(),
        status_reason: reason.map(str::to_owned),
        previous_status: previous.map(|previous_status| previous_status.to_string()),
        node_name: node_name.to_owned(),
        output_snippet: output,
        timestamp: chrono::Utc::now().to_rfc3339(),
        needs_input,
        total_input_tokens: session.meta_parsed(meta::TOTAL_INPUT_TOKENS),
        total_output_tokens: session.meta_parsed(meta::TOTAL_OUTPUT_TOKENS),
        session_cost_usd: session.meta_parsed(meta::SESSION_COST_USD),
        exit_code: session.exit_code,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_store;
    use pulpo_common::session::Runtime;
    use std::collections::HashMap;
    use uuid::Uuid;

    async fn insert_session(store: &Store, name: &str) -> Session {
        let session = Session {
            id: Uuid::new_v4(),
            name: name.into(),
            workdir: "/tmp/repo".into(),
            command: "echo test".into(),
            status: SessionStatus::Working,
            runtime: Runtime::Tmux,
            metadata: Some(HashMap::new()),
            ..Default::default()
        };
        store.insert_session(&session).await.unwrap();
        session
    }

    /// `refresh_exact_usage` — the shared helper `watchdog::idle`'s exit-marker path,
    /// `session::manager::apply_harness_event`'s `SessionEnded` handling, and
    /// `stop_session` all call at every transition into `Ready`/`Stopped` (see its
    /// doc comment for why: the idle-sweep loop alone only ever revisits
    /// `Active`/`Idle` sessions). A session whose command isn't agent-shaped (no
    /// structured usage reader matches) is the deterministic, environment-independent
    /// case: `session_cost_usd`/token metadata stays unset either way (real reader
    /// or the `cfg(coverage)` stub), but under a normal (non-coverage) build the
    /// real reader still persists the negative `usage_source = "none"` marker (see
    /// `mark_usage_source_none`) so `api::usage::session_with_on_demand_usage`
    /// doesn't re-read this session's disk files on every future call.
    #[tokio::test]
    async fn test_refresh_exact_usage_noop_for_non_agent_command() {
        let store = test_store().await;
        let session = insert_session(&store, "generic-cmd").await;

        refresh_exact_usage(&store, &session).await;

        let refreshed = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(refreshed.meta_str(meta::SESSION_COST_USD).is_none());
        assert!(refreshed.meta_str(meta::TOTAL_INPUT_TOKENS).is_none());
        #[cfg(not(coverage))]
        assert_eq!(refreshed.meta_str(meta::USAGE_SOURCE), Some("none"));
    }

    #[tokio::test]
    async fn test_refresh_exact_usage_does_not_overwrite_existing_usage_source() {
        // A session that already has a real `usage_source` recorded (from an
        // earlier successful read) must not have it clobbered with the
        // negative marker just because a later refresh call finds nothing new
        // — `mark_usage_source_none` only ever fills in an absent value.
        let store = test_store().await;
        let session = insert_session(&store, "generic-cmd-2").await;
        store
            .batch_update_session_metadata(
                &session.id.to_string(),
                &[(meta::USAGE_SOURCE, "claude-jsonl")],
                &[],
            )
            .await
            .unwrap();
        let session = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();

        refresh_exact_usage(&store, &session).await;

        let refreshed = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            refreshed.meta_str(meta::USAGE_SOURCE),
            Some("claude-jsonl")
        );
    }

    #[test]
    fn test_build_session_event_enriches_usage_from_metadata() {
        let mut metadata = HashMap::new();
        metadata.insert(meta::TOTAL_INPUT_TOKENS.into(), "123".into());
        metadata.insert(meta::TOTAL_OUTPUT_TOKENS.into(), "456".into());
        metadata.insert(meta::SESSION_COST_USD.into(), "1.250000".into());
        let session = Session {
            id: Uuid::new_v4(),
            name: "event-test".into(),
            status: SessionStatus::Working,
            metadata: Some(metadata),
            ..Default::default()
        };

        let event = build_session_event(
            &session,
            SessionStatus::Waiting,
            Some(status_reason::IDLE),
            Some(SessionStatus::Working),
            "node-a",
            Some("snippet".into()),
        );

        assert_eq!(event.session_name, "event-test");
        assert_eq!(event.status, "waiting");
        assert_eq!(event.status_reason.as_deref(), Some("idle"));
        assert_eq!(event.previous_status.as_deref(), Some("working"));
        assert_eq!(event.node_name, "node-a");
        assert_eq!(event.output_snippet.as_deref(), Some("snippet"));
        assert_eq!(event.total_input_tokens, Some(123));
        assert_eq!(event.total_output_tokens, Some(456));
        assert_eq!(event.session_cost_usd, Some(1.25));
    }

    #[test]
    fn test_build_session_event_needs_input_populates_compat_field() {
        let session = Session {
            id: Uuid::new_v4(),
            name: "needs-input-test".into(),
            status: SessionStatus::Waiting,
            ..Default::default()
        };
        let event = build_session_event(
            &session,
            SessionStatus::Waiting,
            Some(&status_reason::needs_input("permission")),
            None,
            "node-a",
            None,
        );
        assert_eq!(
            event.status_reason.as_deref(),
            Some("needs_input:permission")
        );
        assert_eq!(event.needs_input.as_deref(), Some("permission"));
    }

    #[tokio::test]
    async fn test_detect_and_store_output_metadata_clears_error_status_on_recovery() {
        let store = test_store().await;
        let session = insert_session(&store, "error-recovery").await;
        store
            .batch_update_session_metadata(
                &session.id.to_string(),
                &[
                    (meta::ERROR_STATUS, "Compile error"),
                    (meta::ERROR_STATUS_AT, "2026-04-01T00:00:00Z"),
                ],
                &[],
            )
            .await
            .unwrap();

        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        detect_and_store_output_metadata(
            &store,
            &fetched,
            "all green now",
            None,
            HarnessSignals::none(),
        )
        .await;

        let updated = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.meta_str(meta::ERROR_STATUS), None);
        assert_eq!(updated.meta_str(meta::ERROR_STATUS_AT), None);
    }

    fn exact_usage_fixture() -> crate::usage::ExactUsage {
        crate::usage::ExactUsage {
            source: crate::usage::SOURCE_CLAUDE,
            input_tokens: 1000,
            output_tokens: 500,
            cache_write_tokens: 300,
            cache_read_tokens: 2000,
            cost_usd: Some(0.123_456),
            quota: None,
        }
    }

    #[tokio::test]
    async fn test_exact_usage_writes_fields_and_source() {
        let store = test_store().await;
        let session = insert_session(&store, "exact-write").await;

        detect_and_store_output_metadata(
            &store,
            &session,
            "",
            Some(exact_usage_fixture()),
            HarnessSignals::none(),
        )
        .await;

        let updated = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated.meta_str(meta::USAGE_SOURCE),
            Some(crate::usage::SOURCE_CLAUDE)
        );
        assert_eq!(updated.meta_str(meta::TOTAL_INPUT_TOKENS), Some("1000"));
        assert_eq!(updated.meta_str(meta::TOTAL_OUTPUT_TOKENS), Some("500"));
        assert_eq!(updated.meta_str(meta::CACHE_WRITE_TOKENS), Some("300"));
        assert_eq!(updated.meta_str(meta::CACHE_READ_TOKENS), Some("2000"));
        assert_eq!(updated.meta_str(meta::SESSION_COST_USD), Some("0.123456"));
    }

    #[tokio::test]
    async fn test_exact_usage_overwrites_instead_of_accumulating() {
        let store = test_store().await;
        let session = insert_session(&store, "exact-overwrite").await;
        store
            .batch_update_session_metadata(
                &session.id.to_string(),
                &[(meta::TOTAL_INPUT_TOKENS, "999999")],
                &[],
            )
            .await
            .unwrap();
        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();

        detect_and_store_output_metadata(
            &store,
            &fetched,
            "",
            Some(exact_usage_fixture()),
            HarnessSignals::none(),
        )
        .await;

        let updated = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.meta_str(meta::TOTAL_INPUT_TOKENS), Some("1000"));
    }

    #[tokio::test]
    async fn test_exact_usage_writes_quota_snapshot() {
        let store = test_store().await;
        let session = insert_session(&store, "exact-quota").await;
        let exact = crate::usage::ExactUsage {
            source: crate::usage::SOURCE_CODEX,
            cost_usd: None,
            quota: Some(crate::usage::QuotaSnapshot {
                primary: Some(crate::usage::QuotaWindow {
                    used_percent: 12.5,
                    window_minutes: Some(300),
                    resets_at: Some(1_775_073_678),
                }),
                secondary: Some(crate::usage::QuotaWindow {
                    used_percent: 3.0,
                    window_minutes: Some(10_080),
                    resets_at: Some(1_775_660_478),
                }),
                plan: Some("plus".into()),
            }),
            ..exact_usage_fixture()
        };

        detect_and_store_output_metadata(&store, &session, "", Some(exact), HarnessSignals::none())
            .await;

        let updated = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated.meta_str(meta::QUOTA_PRIMARY_USED_PERCENT),
            Some("12.5")
        );
        assert_eq!(
            updated.meta_str(meta::QUOTA_PRIMARY_WINDOW_MINUTES),
            Some("300")
        );
        assert_eq!(
            updated.meta_str(meta::QUOTA_PRIMARY_RESETS_AT),
            Some("1775073678")
        );
        assert_eq!(
            updated.meta_str(meta::QUOTA_SECONDARY_USED_PERCENT),
            Some("3")
        );
        assert_eq!(
            updated.meta_str(meta::QUOTA_SECONDARY_WINDOW_MINUTES),
            Some("10080")
        );
        assert_eq!(updated.meta_str(meta::QUOTA_PLAN), Some("plus"));
        // No cost was provided; the field must stay unset.
        assert_eq!(updated.meta_str(meta::SESSION_COST_USD), None);
    }

    #[tokio::test]
    async fn test_exact_usage_noop_when_values_unchanged() {
        let store = test_store().await;
        let session = insert_session(&store, "exact-noop").await;
        detect_and_store_output_metadata(
            &store,
            &session,
            "",
            Some(exact_usage_fixture()),
            HarnessSignals::none(),
        )
        .await;
        let after_first = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();

        // Same values again — must not error and must leave values identical.
        detect_and_store_output_metadata(
            &store,
            &after_first,
            "",
            Some(exact_usage_fixture()),
            HarnessSignals::none(),
        )
        .await;

        let after_second = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after_second.metadata, after_first.metadata);
    }

    #[tokio::test]
    async fn test_no_usage_metadata_written_without_exact_source() {
        // No structured reader matched this tick (unsupported harness, or nothing to
        // read yet) — there is no output-scraping fallback, so usage stays unset.
        let store = test_store().await;
        let session = insert_session(&store, "no-exact-usage").await;

        let output = "Tokens: 1,234 sent, 567 received. Cost: $0.03 message, $0.06 session.\n";
        detect_and_store_output_metadata(&store, &session, output, None, HarnessSignals::none())
            .await;

        let updated = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.meta_str(meta::TOTAL_INPUT_TOKENS), None);
        assert_eq!(updated.meta_str(meta::USAGE_SOURCE), None);
    }
}
