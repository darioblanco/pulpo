use std::sync::Arc;

use pulpo_common::event::{PulpoEvent, UsageAlertEvent};
use pulpo_common::session::{InterventionCode, SessionStatus, meta};

use super::ReadyContext;
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
pub(super) async fn enforce_budgets(
    backend: &Arc<dyn Backend>,
    store: &Store,
    ready_ctx: &ReadyContext,
) {
    let sessions = super::list_sessions_or_warn(store, "Budget check").await;

    for session in sessions
        .into_iter()
        .filter(|s| s.status == SessionStatus::Working)
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
            stop_over_budget(backend, store, &session, cost, budget, ready_ctx).await;
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
                if let Some(tx) = &ready_ctx.event_tx {
                    let _ = tx.send(PulpoEvent::UsageAlert(UsageAlertEvent {
                        session_id: session.id.to_string(),
                        session_name: session.name.clone(),
                        node_name: ready_ctx.node_name.clone(),
                        alert_kind: "budget_threshold".to_owned(),
                        message: format!("Cost ${cost:.2} reached 80% of ${budget:.2} budget"),
                        cost_usd: Some(cost),
                        budget_usd: Some(budget),
                        timestamp: timestamp.clone(),
                    }));
                }
            }
        }
    }
}

/// Stop a session that has reached its cost budget, recording a `BudgetExceeded`
/// intervention via the shared [`super::intervention::stop_and_record`] path.
async fn stop_over_budget(
    backend: &Arc<dyn Backend>,
    store: &Store,
    session: &pulpo_common::session::Session,
    cost: f64,
    budget: f64,
    ready_ctx: &ReadyContext,
) {
    let reason = format!("Cost ${cost:.2} reached budget ${budget:.2}");
    if !super::intervention::stop_and_record(
        backend,
        store,
        session,
        InterventionCode::BudgetExceeded,
        &reason,
        ready_ctx,
        "Failed to kill session over budget (still alive)",
        "Failed to record budget intervention",
    )
    .await
    {
        return;
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
    use crate::store::test_store;
    use pulpo_common::session::{Runtime, Session};
    use std::collections::HashMap;
    use uuid::Uuid;

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
            status: SessionStatus::Working,
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

    fn ready_ctx() -> ReadyContext {
        ReadyContext {
            event_tx: None,
            node_name: "test-node".into(),
        }
    }

    #[tokio::test]
    async fn test_stops_session_at_budget() {
        let store = test_store().await;
        let s = insert(&store, "over", Some("1.0"), Some("1.5")).await;
        enforce_budgets(&backend(), &store, &ready_ctx()).await;
        let updated = store.get_session(&s.id.to_string()).await.unwrap().unwrap();
        assert_eq!(updated.status, SessionStatus::Done);
        assert_eq!(
            updated.intervention_code,
            Some(InterventionCode::BudgetExceeded)
        );
    }

    #[tokio::test]
    async fn test_alerts_once_at_eighty_percent() {
        let store = test_store().await;
        let s = insert(&store, "warn", Some("1.0"), Some("0.85")).await;
        enforce_budgets(&backend(), &store, &ready_ctx()).await;
        let after = store.get_session(&s.id.to_string()).await.unwrap().unwrap();
        // alerted, not stopped
        assert_eq!(after.status, SessionStatus::Working);
        let alerted_at = after.meta_str(meta::BUDGET_ALERTED_AT).map(str::to_owned);
        assert!(alerted_at.is_some());

        // second pass must not change the alert timestamp (one-shot)
        enforce_budgets(&backend(), &store, &ready_ctx()).await;
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
        enforce_budgets(&backend(), &store, &ready_ctx()).await;
        let after = store.get_session(&s.id.to_string()).await.unwrap().unwrap();
        assert_eq!(after.status, SessionStatus::Working);
        assert!(after.meta_str(meta::BUDGET_ALERTED_AT).is_none());
    }

    #[tokio::test]
    async fn test_no_budget_is_ignored() {
        let store = test_store().await;
        let s = insert(&store, "nob", None, Some("99.0")).await;
        enforce_budgets(&backend(), &store, &ready_ctx()).await;
        let after = store.get_session(&s.id.to_string()).await.unwrap().unwrap();
        assert_eq!(after.status, SessionStatus::Working);
    }

    #[tokio::test]
    async fn test_zero_budget_is_ignored() {
        let store = test_store().await;
        let s = insert(&store, "zero", Some("0"), Some("5.0")).await;
        enforce_budgets(&backend(), &store, &ready_ctx()).await;
        let after = store.get_session(&s.id.to_string()).await.unwrap().unwrap();
        assert_eq!(after.status, SessionStatus::Working);
    }

    #[tokio::test]
    async fn test_budget_without_cost_is_ignored() {
        let store = test_store().await;
        let s = insert(&store, "nocost", Some("1.0"), None).await;
        enforce_budgets(&backend(), &store, &ready_ctx()).await;
        let after = store.get_session(&s.id.to_string()).await.unwrap().unwrap();
        assert_eq!(after.status, SessionStatus::Working);
    }

    #[tokio::test]
    async fn test_emits_usage_alert_event_at_eighty_percent() {
        let store = test_store().await;
        insert(&store, "alert-evt", Some("1.0"), Some("0.9")).await;
        let (tx, mut rx) = tokio::sync::broadcast::channel(8);
        let ctx = ReadyContext {
            event_tx: Some(tx),
            node_name: "node-a".into(),
        };

        enforce_budgets(&backend(), &store, &ctx).await;

        let event = rx.try_recv().expect("expected a usage alert event");
        match event {
            pulpo_common::event::PulpoEvent::UsageAlert(a) => {
                assert_eq!(a.alert_kind, "budget_threshold");
                assert_eq!(a.node_name, "node-a");
                assert_eq!(a.budget_usd, Some(1.0));
                assert!(a.message.contains("80%"));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    // The former `test_enforce_budgets_kills_real_tmux_session_over_budget` (a
    // real-tmux, DB-seeded proof that an over-budget session gets killed) was
    // migrated to the end-to-end scenario suite's S6
    // (`crates/pulpo-e2e/tests/scenarios.rs::s6_budget_breaker_stops_session_and_delivers_webhook`)
    // and deleted here — S6 drives the same breaker through a real spawn, a real
    // Claude transcript file (via the fake harness), and the real watchdog loop,
    // which is a stronger proof than seeding `session_cost_usd` directly into the
    // DB ever was, and also covers the webhook delivery this test never touched.
}
