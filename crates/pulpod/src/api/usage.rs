use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
};
use pulpo_common::api::{UsageScanResponse, UsageSessionsResponse};
use pulpo_common::session::{Session, SessionStatus, meta};
use serde::Deserialize;

use crate::api::error::{ApiError, internal_error};
use crate::store::Store;
use crate::usage::rollup::{build_repo_rollups, session_usage};

/// For a `Done`/`Lost` session with no `session_cost_usd` metadata yet, compute
/// its exact usage on demand (and persist it) instead of leaving it to a
/// watchdog tick that will never revisit a terminal session —
/// `check_idle_sessions` only ever visits `Working`/`Waiting` sessions, so a
/// session that reached a terminal status before
/// `watchdog::metadata::refresh_exact_usage` ran for it (an old session from
/// before that fix, or a race the watchdog missed) would otherwise report no
/// cost forever. `Lost` is included alongside `Done` (#127 follow-up): a
/// session that crashed rather than exiting cleanly still has a real
/// transcript worth reading.
///
/// Skips the refresh entirely once `usage_source` is already recorded as
/// `"none"` (see `watchdog::metadata::mark_usage_source_none`) — a prior
/// refresh already established there is nothing to read for this session (no
/// structured reader matched its command, or its transcript has nothing
/// usable), so recomputing from disk on every single call to this endpoint
/// would be pure waste. Best-effort: a session still missing cost afterward is
/// returned unchanged either way.
async fn session_with_on_demand_usage(store: &Store, session: Session) -> Session {
    if !matches!(session.status, SessionStatus::Done | SessionStatus::Lost)
        || session.meta_str(meta::SESSION_COST_USD).is_some()
        || session.meta_str(meta::USAGE_SOURCE) == Some("none")
    {
        return session;
    }
    crate::watchdog::refresh_exact_usage(store, &session).await;
    store
        .get_session(&session.id.to_string())
        .await
        .ok()
        .flatten()
        .unwrap_or(session)
}

/// `GET /api/v1/usage/sessions` — exact per-session usage plus per-repo rollups, for
/// pulpo-managed sessions on this node.
///
/// Computed from the exact-usage metadata the watchdog keeps fresh via the
/// structured usage readers (Claude/Codex/pi); a `Stopped` session with no cost
/// recorded yet is refreshed on demand (see [`session_with_on_demand_usage`]).
/// Sessions run with an unsupported harness simply show no usage — there is no
/// output-scraping fallback.
pub async fn sessions(
    State(state): State<Arc<super::AppState>>,
) -> Result<Json<UsageSessionsResponse>, ApiError> {
    let now = chrono::Utc::now();
    let sessions = state
        .store
        .list_sessions()
        .await
        .map_err(|e| internal_error(&e.to_string()))?;

    let mut refreshed = Vec::with_capacity(sessions.len());
    for session in sessions {
        refreshed.push(session_with_on_demand_usage(&state.store, session).await);
    }

    let node_name = state.config.read().await.node.name.clone();
    let usages: Vec<_> = refreshed.iter().map(session_usage).collect();
    let repos = build_repo_rollups(&usages);
    Ok(Json(UsageSessionsResponse {
        node_name,
        generated_at: now.to_rfc3339(),
        sessions: usages,
        repos,
    }))
}

/// Query parameters for [`scan`].
#[derive(Debug, Default, Deserialize)]
pub struct ScanParams {
    /// Keep every directory distinct instead of collapsing worktrees/subdirectories onto
    /// their origin repository (the default).
    #[serde(default)]
    pub by_worktree: bool,
    /// Limit the scan to the last N days (`None` = all-time).
    #[serde(default)]
    pub since_days: Option<u32>,
}

/// `GET /api/v1/usage/scan` — read-only sweep of all local Claude/Codex history.
///
/// Reports total spend by agent and by repo from the agents' own on-disk session files —
/// no pulpo-managed sessions required. The low-friction "what did my agents cost?" view.
/// By default worktrees and subdirectories roll up to their origin repo; `?by_worktree=true`
/// keeps each checkout separate.
pub async fn scan(
    State(state): State<Arc<super::AppState>>,
    Query(params): Query<ScanParams>,
) -> Result<Json<UsageScanResponse>, ApiError> {
    let (node_name, data_dir) = {
        let config = state.config.read().await;
        (config.node.name.clone(), config.data_dir())
    };
    let resp = crate::usage::scan_local_usage(
        &node_name,
        params.by_worktree,
        params.since_days,
        std::path::Path::new(&data_dir),
    )
    .unwrap_or_else(|| UsageScanResponse {
        node_name,
        generated_at: chrono::Utc::now().to_rfc3339(),
        window_days: params.since_days,
        total_tokens: 0,
        total_cost_usd: None,
        by_agent: Vec::new(),
        by_model: Vec::new(),
        by_repo: Vec::new(),
    });
    Ok(Json(resp))
}

#[cfg(test)]
mod tests {
    use crate::api::AppState;
    use crate::api::test_support::test_state;
    use axum::extract::State;
    use pulpo_common::session::{Runtime, Session, SessionStatus};
    use std::collections::HashMap;
    use uuid::Uuid;

    async fn insert(state: &AppState, name: &str, workdir: &str, meta_pairs: &[(&str, &str)]) {
        let mut metadata = HashMap::new();
        for (k, v) in meta_pairs {
            metadata.insert((*k).to_owned(), (*v).to_owned());
        }
        let session = Session {
            id: Uuid::new_v4(),
            name: name.into(),
            workdir: workdir.into(),
            command: "claude -p x".into(),
            status: SessionStatus::Working,
            runtime: Runtime::Tmux,
            metadata: Some(metadata),
            ..Default::default()
        };
        state.store.insert_session(&session).await.unwrap();
    }

    /// Insert a `Stopped` session with a non-agent command — deterministic input
    /// for the on-demand-refresh tests below: no structured usage reader matches
    /// `"cargo build"`, so `session_with_on_demand_usage`'s call into
    /// `watchdog::refresh_exact_usage` is guaranteed to find nothing, regardless of
    /// what's on the machine actually running the test.
    async fn insert_stopped(state: &AppState, name: &str, meta_pairs: &[(&str, &str)]) {
        insert_terminal(state, name, SessionStatus::Done, meta_pairs).await;
    }

    /// Like [`insert_stopped`], but with a caller-chosen terminal status — used
    /// to prove the on-demand gate covers `Lost` too (#127 follow-up), not just
    /// `Done`.
    async fn insert_terminal(
        state: &AppState,
        name: &str,
        status: SessionStatus,
        meta_pairs: &[(&str, &str)],
    ) {
        let mut metadata = HashMap::new();
        for (k, v) in meta_pairs {
            metadata.insert((*k).to_owned(), (*v).to_owned());
        }
        let session = Session {
            id: Uuid::new_v4(),
            name: name.into(),
            workdir: "/tmp/repo".into(),
            command: "cargo build".into(),
            status,
            runtime: Runtime::Tmux,
            metadata: Some(metadata),
            ..Default::default()
        };
        state.store.insert_session(&session).await.unwrap();
    }

    #[tokio::test]
    async fn test_sessions_computes_usage_on_demand_for_stopped_session_without_cost() {
        // A `Stopped` session with no `session_cost_usd` yet (e.g. one that reached
        // `Stopped` before `watchdog::refresh_exact_usage` existed, or a race the
        // watchdog missed) triggers an on-demand refresh instead of reporting no
        // cost forever. The command here matches no structured reader, so the
        // refresh finds nothing — this proves the code path runs without error and
        // still reports "no usage" rather than crashing or fabricating a value.
        let state = test_state().await;
        insert_stopped(&state, "old-stopped", &[]).await;

        let resp = super::sessions(State(state)).await.unwrap();
        assert_eq!(resp.sessions.len(), 1);
        assert_eq!(resp.sessions[0].cost_usd, None);
    }

    #[tokio::test]
    async fn test_sessions_skips_on_demand_refresh_when_stopped_session_already_has_cost() {
        use pulpo_common::session::meta;
        // A `Stopped` session that already has cost metadata must not be touched by
        // the on-demand path — it's returned as-is.
        let state = test_state().await;
        insert_stopped(
            &state,
            "already-priced",
            &[(meta::SESSION_COST_USD, "2.500000")],
        )
        .await;

        let resp = super::sessions(State(state)).await.unwrap();
        assert_eq!(resp.sessions.len(), 1);
        assert_eq!(resp.sessions[0].cost_usd, Some(2.5));
    }

    #[tokio::test]
    async fn test_sessions_computes_usage_on_demand_for_lost_session_without_cost() {
        // #127 follow-up: a `Lost` session (crashed rather than exiting
        // cleanly) still has a real transcript worth reading — the on-demand
        // gate must cover it exactly like `Done`, not just the clean-exit case.
        let state = test_state().await;
        insert_terminal(&state, "old-lost", SessionStatus::Lost, &[]).await;

        let resp = super::sessions(State(state)).await.unwrap();
        assert_eq!(resp.sessions.len(), 1);
        assert_eq!(resp.sessions[0].cost_usd, None);
    }

    #[tokio::test]
    async fn test_sessions_skips_on_demand_refresh_when_usage_source_already_marked_none() {
        use pulpo_common::session::meta;
        // A session already marked `usage_source = "none"` (a previous refresh
        // already established nothing can be read for it) must not be
        // recomputed from disk again on every call — the whole point of the
        // negative marker (`watchdog::metadata::mark_usage_source_none`).
        let state = test_state().await;
        insert_stopped(&state, "known-no-usage", &[(meta::USAGE_SOURCE, "none")]).await;

        let resp = super::sessions(State(state)).await.unwrap();
        assert_eq!(resp.sessions.len(), 1);
        assert_eq!(resp.sessions[0].cost_usd, None);
    }

    #[tokio::test]
    async fn test_sessions_on_demand_refresh_persists_negative_marker() {
        use pulpo_common::session::meta;
        // The first call for a session with nothing to read persists the
        // negative marker so a *second* call doesn't need to touch disk again
        // — proven here by observing the marker lands in the store, not just
        // that the response looks the same.
        //
        // `refresh_exact_usage` (which this exercises transitively through
        // `session_with_on_demand_usage`) has a `cfg(coverage)` no-op stub —
        // untestable real filesystem I/O, same as `read_exact_usage_for_session`
        // itself — so the marker is only ever actually written under a normal
        // (non-coverage) build.
        let state = test_state().await;
        insert_stopped(&state, "first-call-marks-none", &[]).await;
        let id = {
            let sessions = state.store.list_sessions().await.unwrap();
            sessions[0].id.to_string()
        };

        let _ = super::sessions(State(state.clone())).await.unwrap();

        let fetched = state.store.get_session(&id).await.unwrap().unwrap();
        #[cfg(not(coverage))]
        assert_eq!(fetched.meta_str(meta::USAGE_SOURCE), Some("none"));
    }

    #[tokio::test]
    async fn test_sessions_empty() {
        let state = test_state().await;
        let resp = super::sessions(State(state)).await.unwrap();
        assert!(resp.sessions.is_empty());
        assert!(resp.repos.is_empty());
        assert!(!resp.generated_at.is_empty());
    }

    #[tokio::test]
    async fn test_sessions_returns_usage_and_repo_rollup() {
        use pulpo_common::session::meta;
        let state = test_state().await;
        insert(
            &state,
            "claude-one",
            "/tmp/repo",
            &[
                (meta::USAGE_SOURCE, "claude-jsonl"),
                (meta::TOTAL_INPUT_TOKENS, "1000"),
                (meta::SESSION_COST_USD, "0.5"),
            ],
        )
        .await;

        let resp = super::sessions(State(state)).await.unwrap();
        assert_eq!(resp.sessions.len(), 1);
        assert_eq!(resp.sessions[0].total_tokens, 1000);
        assert_eq!(resp.sessions[0].cost_usd, Some(0.5));
        assert_eq!(resp.repos.len(), 1);
        assert_eq!(resp.repos[0].label, "/tmp/repo");
        assert_eq!(resp.repos[0].total_cost_usd, Some(0.5));
    }

    #[tokio::test]
    async fn test_sessions_skips_sessions_without_workdir_in_rollup() {
        let state = test_state().await;
        insert(&state, "no-workdir", "", &[]).await;

        let resp = super::sessions(State(state)).await.unwrap();
        assert_eq!(resp.sessions.len(), 1);
        assert!(resp.repos.is_empty());
    }

    #[tokio::test]
    async fn test_scan_endpoint_returns_node_name() {
        // Under coverage the scan is a no-op stub → empty report; under normal builds it
        // reads the (likely-absent in CI) real home dirs. Either way the node name is set
        // and the call succeeds, which exercises the handler wiring.
        let state = test_state().await;
        let resp = super::scan(
            State(state),
            axum::extract::Query(super::ScanParams::default()),
        )
        .await
        .unwrap();
        assert_eq!(resp.node_name, "test-node");
    }

    #[tokio::test]
    async fn test_scan_endpoint_accepts_by_worktree() {
        let state = test_state().await;
        let resp = super::scan(
            State(state),
            axum::extract::Query(super::ScanParams {
                by_worktree: true,
                since_days: Some(7),
            }),
        )
        .await
        .unwrap();
        assert_eq!(resp.node_name, "test-node");
    }
}
