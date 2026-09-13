use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
};
use pulpo_common::api::{UsageScanResponse, UsageSessionsResponse};
use serde::Deserialize;

use crate::api::error::{ApiError, internal_error};
use crate::usage::rollup::{build_repo_rollups, session_usage};

/// `GET /api/v1/usage/sessions` — exact per-session usage plus per-repo rollups, for
/// pulpo-managed sessions on this node.
///
/// Read-only; computed from the exact-usage metadata the watchdog keeps fresh via the
/// structured usage readers (Claude/Codex/pi). Sessions run with an unsupported harness
/// simply show no usage — there is no output-scraping fallback.
pub async fn sessions(
    State(state): State<Arc<super::AppState>>,
) -> Result<Json<UsageSessionsResponse>, ApiError> {
    let now = chrono::Utc::now();
    let sessions = state
        .store
        .list_sessions()
        .await
        .map_err(|e| internal_error(&e.to_string()))?;

    let node_name = state.config.read().await.node.name.clone();
    let usages: Vec<_> = sessions.iter().map(session_usage).collect();
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
            status: SessionStatus::Active,
            runtime: Runtime::Tmux,
            metadata: Some(metadata),
            ..Default::default()
        };
        state.store.insert_session(&session).await.unwrap();
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
