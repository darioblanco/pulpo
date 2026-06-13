use std::sync::Arc;

use pulpo_common::session::{InterventionCode, SessionStatus, meta};

use super::resolve_backend_id;
use crate::backend::Backend;
use crate::store::Store;

/// Fraction of the budget at which a one-shot alert fires.
const ALERT_FRACTION: f64 = 0.8;

/// Enforce per-session cost budgets.
///
/// For each active session with a resolved `budget_cost_usd`: alert once at 80% (recorded
/// via `budget_alerted_at` so it fires only once), and stop the session at 100% via the
/// standard intervention path (capture output → kill → record `BudgetExceeded` → clean up
/// the worktree). On a subscription this allocates the shared pool away from a runaway
/// session; on prepaid credits / API keys it caps real dollars.
pub(super) async fn enforce_budgets(backend: &Arc<dyn Backend>, store: &Store) {
    let sessions = match store.list_sessions().await {
        Ok(s) => s,
        #[allow(unused_variables)]
        Err(error) => {
            coverage_warn!("Budget check: failed to list sessions: {error}");
            return;
        }
    };

    for session in sessions
        .into_iter()
        .filter(|s| s.status == SessionStatus::Active)
    {
        let Some(budget) = session.meta_parsed::<f64>(meta::BUDGET_COST_USD) else {
            continue;
        };
        if budget <= 0.0 {
            continue;
        }
        let Some(cost) = session.meta_parsed::<f64>(meta::SESSION_COST_USD) else {
            continue;
        };

        if cost >= budget {
            stop_over_budget(backend, store, &session, cost, budget).await;
        } else if cost >= budget * ALERT_FRACTION
            && session.meta_str(meta::BUDGET_ALERTED_AT).is_none()
        {
            let timestamp = chrono::Utc::now().to_rfc3339();
            #[allow(unused_variables)]
            if let Err(error) = store
                .update_session_metadata_field(
                    &session.id.to_string(),
                    meta::BUDGET_ALERTED_AT,
                    &timestamp,
                )
                .await
            {
                coverage_warn!(
                    session_name = %session.name,
                    "Failed to record budget alert: {error}"
                );
            } else {
                coverage_warn!(
                    session_name = %session.name,
                    cost,
                    budget,
                    "Session reached 80% of its cost budget"
                );
            }
        }
    }
}

/// Stop a session that has reached its cost budget, recording a `BudgetExceeded`
/// intervention. Mirrors the memory-pressure intervention path.
async fn stop_over_budget(
    backend: &Arc<dyn Backend>,
    store: &Store,
    session: &pulpo_common::session::Session,
    cost: f64,
    budget: f64,
) {
    let bid = resolve_backend_id(session, backend.as_ref());
    if let Ok(output) = backend.capture_output(&bid, 500) {
        let _ = store
            .update_session_output_snapshot(&session.id.to_string(), &output)
            .await;
    }
    #[allow(unused_variables)]
    if let Err(error) = backend.kill_session(&bid) {
        coverage_warn!(
            session_name = %session.name,
            "Failed to kill session over budget (still alive): {error}"
        );
        return;
    }
    let reason = format!("Cost ${cost:.2} reached budget ${budget:.2}");
    #[allow(unused_variables)]
    if let Err(error) = store
        .update_session_intervention(
            &session.id.to_string(),
            InterventionCode::BudgetExceeded,
            &reason,
        )
        .await
    {
        coverage_warn!(
            session_name = %session.name,
            "Failed to record budget intervention: {error}"
        );
    }
    if let Some(ref wt_path) = session.worktree_path {
        crate::session::manager::cleanup_worktree(wt_path, &session.workdir);
    }
    coverage_warn!(
        session_name = %session.name,
        cost,
        budget,
        "Watchdog intervention: stopped session over cost budget"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::StubBackend;
    use pulpo_common::session::{Runtime, Session};
    use std::collections::HashMap;
    use uuid::Uuid;

    async fn test_store() -> Store {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        store
    }

    async fn insert(
        store: &Store,
        name: &str,
        budget: Option<&str>,
        cost: Option<&str>,
    ) -> Session {
        let mut metadata = HashMap::new();
        if let Some(b) = budget {
            metadata.insert(meta::BUDGET_COST_USD.to_owned(), b.to_owned());
        }
        if let Some(c) = cost {
            metadata.insert(meta::SESSION_COST_USD.to_owned(), c.to_owned());
        }
        let session = Session {
            id: Uuid::new_v4(),
            name: name.into(),
            workdir: "/tmp/repo".into(),
            command: "claude".into(),
            status: SessionStatus::Active,
            runtime: Runtime::Tmux,
            backend_session_id: Some("$1".into()),
            metadata: Some(metadata),
            ..Default::default()
        };
        store.insert_session(&session).await.unwrap();
        session
    }

    fn backend() -> Arc<dyn Backend> {
        Arc::new(StubBackend)
    }

    #[tokio::test]
    async fn test_stops_session_at_budget() {
        let store = test_store().await;
        let s = insert(&store, "over", Some("1.0"), Some("1.5")).await;
        enforce_budgets(&backend(), &store).await;
        let updated = store.get_session(&s.id.to_string()).await.unwrap().unwrap();
        assert_eq!(updated.status, SessionStatus::Stopped);
        assert_eq!(
            updated.intervention_code,
            Some(InterventionCode::BudgetExceeded)
        );
    }

    #[tokio::test]
    async fn test_alerts_once_at_eighty_percent() {
        let store = test_store().await;
        let s = insert(&store, "warn", Some("1.0"), Some("0.85")).await;
        enforce_budgets(&backend(), &store).await;
        let after = store.get_session(&s.id.to_string()).await.unwrap().unwrap();
        // alerted, not stopped
        assert_eq!(after.status, SessionStatus::Active);
        let alerted_at = after.meta_str(meta::BUDGET_ALERTED_AT).map(str::to_owned);
        assert!(alerted_at.is_some());

        // second pass must not change the alert timestamp (one-shot)
        enforce_budgets(&backend(), &store).await;
        let again = store.get_session(&s.id.to_string()).await.unwrap().unwrap();
        assert_eq!(
            again.meta_str(meta::BUDGET_ALERTED_AT).map(str::to_owned),
            alerted_at
        );
    }

    #[tokio::test]
    async fn test_under_threshold_does_nothing() {
        let store = test_store().await;
        let s = insert(&store, "ok", Some("10.0"), Some("1.0")).await;
        enforce_budgets(&backend(), &store).await;
        let after = store.get_session(&s.id.to_string()).await.unwrap().unwrap();
        assert_eq!(after.status, SessionStatus::Active);
        assert!(after.meta_str(meta::BUDGET_ALERTED_AT).is_none());
    }

    #[tokio::test]
    async fn test_no_budget_is_ignored() {
        let store = test_store().await;
        let s = insert(&store, "nob", None, Some("99.0")).await;
        enforce_budgets(&backend(), &store).await;
        let after = store.get_session(&s.id.to_string()).await.unwrap().unwrap();
        assert_eq!(after.status, SessionStatus::Active);
    }

    #[tokio::test]
    async fn test_zero_budget_is_ignored() {
        let store = test_store().await;
        let s = insert(&store, "zero", Some("0"), Some("5.0")).await;
        enforce_budgets(&backend(), &store).await;
        let after = store.get_session(&s.id.to_string()).await.unwrap().unwrap();
        assert_eq!(after.status, SessionStatus::Active);
    }

    #[tokio::test]
    async fn test_budget_without_cost_is_ignored() {
        let store = test_store().await;
        let s = insert(&store, "nocost", Some("1.0"), None).await;
        enforce_budgets(&backend(), &store).await;
        let after = store.get_session(&s.id.to_string()).await.unwrap().unwrap();
        assert_eq!(after.status, SessionStatus::Active);
    }
}
