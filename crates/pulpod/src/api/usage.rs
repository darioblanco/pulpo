use std::sync::Arc;

use axum::{Json, extract::State};
use pulpo_common::api::UsageProjectionResponse;
use pulpo_common::session::meta;

use crate::api::session_remote::{ApiError, internal_error};
use crate::usage::projection::{build_rollups, project_session};

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
    Ok(Json(UsageProjectionResponse {
        node_name,
        generated_at: now.to_rfc3339(),
        sessions: projections,
        accounts,
    }))
}

#[cfg(test)]
mod tests {
    use crate::api::AppState;
    use crate::backend::StubBackend;
    use crate::config::{Config, NodeConfig};
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;
    use axum::extract::State;
    use pulpo_common::session::{Runtime, Session, SessionStatus};
    use std::collections::HashMap;
    use std::sync::Arc;
    use uuid::Uuid;

    async fn test_state() -> Arc<AppState> {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        AppState::new(
            Config {
                node: NodeConfig {
                    name: "test-node".into(),
                    port: 7433,
                    data_dir: tmpdir.path().to_str().unwrap().into(),
                    ..NodeConfig::default()
                },
                ..Default::default()
            },
            manager,
            peer_registry,
            store,
        )
    }

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
