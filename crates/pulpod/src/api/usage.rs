use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
};
use pulpo_common::api::{UsageProjectionResponse, UsageScanResponse};
use pulpo_common::session::meta;
use serde::Deserialize;

use crate::api::error::{ApiError, internal_error};
use crate::usage::projection::{
    build_ink_rollups, build_repo_rollups, build_rollups, project_session,
};

/// `GET /api/v1/usage/projection` — per-session burn-rate projections plus per-account
/// rollups for this node.
///
/// Read-only; computed from the exact-usage metadata the watchdog keeps fresh. Claude
/// %-of-cap is included only for plans with a configured
/// `[plans.<plan>] weekly_token_allowance`.
pub async fn projection(
    State(state): State<Arc<super::AppState>>,
) -> Result<Json<UsageProjectionResponse>, ApiError> {
    let now = chrono::Utc::now();
    let sessions = state
        .store
        .list_sessions()
        .await
        .map_err(|e| internal_error(&e.to_string()))?;

    let config = state.config.read().await;
    let node_name = config.node.name.clone();
    let projections: Vec<_> = sessions
        .iter()
        .map(|session| {
            let allowance = session
                .meta_str(meta::AUTH_PLAN)
                .and_then(|plan| config.plans.get(plan))
                .and_then(|plan| plan.weekly_token_allowance);
            project_session(session, now, allowance)
        })
        .collect();
    drop(config);

    let accounts = build_rollups(&projections);
    let inks = build_ink_rollups(&projections);
    let repos = build_repo_rollups(&projections);
    Ok(Json(UsageProjectionResponse {
        node_name,
        generated_at: now.to_rfc3339(),
        sessions: projections,
        accounts,
        inks,
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
    let node_name = state.config.read().await.node.name.clone();
    let resp = crate::usage::scan_local_usage(&node_name, params.by_worktree, params.since_days)
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

    async fn insert(state: &AppState, name: &str, meta_pairs: &[(&str, &str)]) {
        let mut metadata = HashMap::new();
        for (k, v) in meta_pairs {
            metadata.insert((*k).to_owned(), (*v).to_owned());
        }
        let session = Session {
            id: Uuid::new_v4(),
            name: name.into(),
            workdir: "/tmp/repo".into(),
            command: "claude -p x".into(),
            status: SessionStatus::Active,
            runtime: Runtime::Tmux,
            metadata: Some(metadata),
            ..Default::default()
        };
        state.store.insert_session(&session).await.unwrap();
    }

    #[tokio::test]
    async fn test_projection_empty() {
        let state = test_state().await;
        let resp = super::projection(State(state)).await.unwrap();
        assert!(resp.sessions.is_empty());
        assert!(resp.accounts.is_empty());
        assert!(!resp.generated_at.is_empty());
    }

    #[tokio::test]
    async fn test_projection_returns_sessions_and_rollups() {
        use pulpo_common::session::meta;
        let state = test_state().await;
        insert(
            &state,
            "claude-one",
            &[
                (meta::USAGE_SOURCE, "claude-jsonl"),
                (meta::TOTAL_INPUT_TOKENS, "1000"),
                (meta::SESSION_COST_USD, "0.5"),
                (meta::AUTH_PROVIDER, "claude.ai"),
                (meta::AUTH_PLAN, "max"),
                (meta::AUTH_EMAIL, "a@x.com"),
            ],
        )
        .await;

        let resp = super::projection(State(state)).await.unwrap();
        assert_eq!(resp.sessions.len(), 1);
        assert_eq!(resp.sessions[0].total_tokens, 1000);
        assert_eq!(resp.accounts.len(), 1);
        assert_eq!(resp.accounts[0].email.as_deref(), Some("a@x.com"));
        assert_eq!(resp.accounts[0].session_count, 1);
        // Per-repo rollup wired (session has workdir /tmp/repo); no ink → empty ink rollup.
        assert_eq!(resp.repos.len(), 1);
        assert_eq!(resp.repos[0].label, "/tmp/repo");
        assert!(resp.inks.is_empty());
    }

    #[tokio::test]
    async fn test_projection_ink_rollup_wired() {
        use pulpo_common::session::meta;
        let state = test_state().await;
        // Insert a session spawned from an ink, with exact cost.
        let mut metadata = HashMap::new();
        metadata.insert(meta::USAGE_SOURCE.to_owned(), "claude-jsonl".to_owned());
        metadata.insert(meta::SESSION_COST_USD.to_owned(), "11.0".to_owned());
        let session = Session {
            id: Uuid::new_v4(),
            name: "nightly-run".into(),
            workdir: "/repos/api".into(),
            command: "claude -p review".into(),
            ink: Some("nightly".into()),
            status: SessionStatus::Active,
            runtime: Runtime::Tmux,
            metadata: Some(metadata),
            ..Default::default()
        };
        state.store.insert_session(&session).await.unwrap();

        let resp = super::projection(State(state)).await.unwrap();
        assert_eq!(resp.inks.len(), 1);
        assert_eq!(resp.inks[0].label, "nightly");
        assert!(resp.inks[0].cost_is_exact);
        assert_eq!(resp.repos.len(), 1);
        assert_eq!(resp.repos[0].label, "/repos/api");
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

    #[tokio::test]
    async fn test_projection_claude_allowance_from_config() {
        use pulpo_common::session::meta;
        let state = test_state().await;
        {
            let mut cfg = state.config.write().await;
            cfg.plans.insert(
                "max".into(),
                crate::config::PlanConfig {
                    weekly_token_allowance: Some(10_000),
                },
            );
        }
        insert(
            &state,
            "claude-alloc",
            &[(meta::TOTAL_INPUT_TOKENS, "1000"), (meta::AUTH_PLAN, "max")],
        )
        .await;

        let resp = super::projection(State(state)).await.unwrap();
        let p = &resp.sessions[0];
        assert_eq!(p.allowance_tokens, Some(10_000));
        assert!((p.allowance_used_percent.unwrap() - 10.0).abs() < 1e-9);
    }
}
