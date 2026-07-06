use std::sync::Arc;

use pulpo_common::event::{PulpoEvent, UsageAlertEvent};
use pulpo_common::session::{InterventionCode, SessionStatus, meta};

use super::{BurnAction, BurnConfig, ReadyContext, resolve_backend_id};
use crate::backend::Backend;
use crate::store::Store;
use crate::usage::projection::per_hour;

/// Sum of every token dimension we track for a session (matches the projection helper).
fn total_tokens(session: &pulpo_common::session::Session) -> u64 {
    [
        meta::TOTAL_INPUT_TOKENS,
        meta::TOTAL_OUTPUT_TOKENS,
        meta::CACHE_WRITE_TOKENS,
        meta::CACHE_READ_TOKENS,
    ]
    .iter()
    .filter_map(|key| session.meta_parsed::<u64>(key))
    .sum()
}

/// Enforce the burn-velocity governor.
///
/// For each active session, the burn rate is the **lifetime average** (cumulative
/// usage ÷ session age) — deliberately not a noisy per-tick delta (see the
/// `usage::projection` module docs). A session is over-ceiling when its cost rate
/// exceeds `ceiling_usd_per_hour` (when set) **or** its token rate exceeds
/// `ceiling_tokens_per_hour` (when set). Sessions too young to have a rate are
/// ignored (`per_hour` returns `None` for non-positive elapsed time).
///
/// On the first crossing (deduped via `burn_alerted_at`) it emits a
/// `usage_alert.burn_ceiling` event. When `action` is [`BurnAction::Stop`] it
/// additionally stops the session via the standard intervention path. The default
/// is alert-only: catching a runaway/loop ("$90 at 2am") that a flat per-session
/// budget misses because the budget only trips at the total.
pub(super) async fn enforce_burn_ceiling(
    backend: &Arc<dyn Backend>,
    store: &Store,
    ready_ctx: &ReadyContext,
    cfg: &BurnConfig,
) {
    // Skip entirely when no ceiling is configured.
    if cfg.ceiling_usd_per_hour.is_none() && cfg.ceiling_tokens_per_hour.is_none() {
        return;
    }

    let sessions = match store.list_sessions().await {
        Ok(s) => s,
        #[allow(unused_variables)]
        Err(error) => {
            coverage_warn!("Burn check: failed to list sessions: {error}");
            return;
        }
    };

    let now = chrono::Utc::now();

    for session in sessions
        .into_iter()
        .filter(|s| s.status == SessionStatus::Active)
    {
        let elapsed = (now - session.created_at).num_seconds();
        let cost = session.meta_parsed::<f64>(meta::SESSION_COST_USD);
        let tokens = total_tokens(&session);

        let cost_per_hour = cost.and_then(|c| per_hour(c, elapsed));
        #[allow(clippy::cast_precision_loss)]
        let tokens_per_hour = per_hour(tokens as f64, elapsed);

        let over_cost = matches!(
            (cfg.ceiling_usd_per_hour, cost_per_hour),
            (Some(ceiling), Some(rate)) if rate > ceiling
        );
        #[allow(clippy::cast_precision_loss)]
        let over_tokens = matches!(
            (cfg.ceiling_tokens_per_hour, tokens_per_hour),
            (Some(ceiling), Some(rate)) if rate > ceiling as f64
        );

        if !(over_cost || over_tokens) {
            continue;
        }

        // One-shot: only act on the first crossing.
        if session.meta_str(meta::BURN_ALERTED_AT).is_some() {
            continue;
        }

        let timestamp = now.to_rfc3339();
        #[allow(unused_variables)]
        if let Err(error) = store
            .update_session_metadata_field(
                &session.id.to_string(),
                meta::BURN_ALERTED_AT,
                &timestamp,
            )
            .await
        {
            coverage_warn!(
                session_name = %session.name,
                "Failed to record burn alert: {error}"
            );
            continue;
        }

        let message = burn_message(
            cfg.ceiling_usd_per_hour,
            cost_per_hour,
            over_cost,
            cfg.ceiling_tokens_per_hour,
            tokens_per_hour,
            over_tokens,
        );

        coverage_warn!(
            session_name = %session.name,
            "Session exceeded burn ceiling: {message}"
        );

        if let Some(tx) = &ready_ctx.event_tx {
            let _ = tx.send(PulpoEvent::UsageAlert(UsageAlertEvent {
                session_id: session.id.to_string(),
                session_name: session.name.clone(),
                node_name: ready_ctx.node_name.clone(),
                alert_kind: "burn_ceiling".to_owned(),
                message: message.clone(),
                cost_usd: cost,
                budget_usd: None,
                timestamp: timestamp.clone(),
            }));
        }

        if cfg.action == BurnAction::Stop {
            stop_over_ceiling(backend, store, &session, &message, ready_ctx).await;
        }
    }
}

/// Build a human-readable summary of which ceiling(s) the session crossed.
fn burn_message(
    ceiling_cost: Option<f64>,
    cost_per_hour: Option<f64>,
    over_cost: bool,
    ceiling_tokens: Option<u64>,
    tokens_per_hour: Option<f64>,
    over_tokens: bool,
) -> String {
    let mut parts = Vec::new();
    if over_cost && let (Some(rate), Some(ceiling)) = (cost_per_hour, ceiling_cost) {
        parts.push(format!(
            "Burn rate ${rate:.2}/hr exceeds ceiling ${ceiling:.2}/hr"
        ));
    }
    if over_tokens && let (Some(rate), Some(ceiling)) = (tokens_per_hour, ceiling_tokens) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let rate = rate.round() as u64;
        parts.push(format!(
            "Burn rate {rate} tokens/hr exceeds ceiling {ceiling} tokens/hr"
        ));
    }
    parts.join("; ")
}

/// Stop a session that exceeded its burn ceiling, recording a `BurnRate`
/// intervention. Mirrors the budget intervention path.
async fn stop_over_ceiling(
    backend: &Arc<dyn Backend>,
    store: &Store,
    session: &pulpo_common::session::Session,
    reason: &str,
    ready_ctx: &ReadyContext,
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
            "Failed to kill session over burn ceiling (still alive): {error}"
        );
        return;
    }
    #[allow(unused_variables)]
    if let Err(error) = store
        .update_session_intervention(&session.id.to_string(), InterventionCode::BurnRate, reason)
        .await
    {
        coverage_warn!(
            session_name = %session.name,
            "Failed to record burn intervention: {error}"
        );
    }
    super::intervention::emit_intervention(ready_ctx, session, InterventionCode::BurnRate, reason);
    if let Some(ref wt_path) = session.worktree_path {
        crate::session::manager::cleanup_worktree(wt_path, &session.workdir);
    }
    coverage_warn!(
        session_name = %session.name,
        "Watchdog intervention: stopped session over burn ceiling"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::StubBackend;
    use crate::store::test_store;
    use chrono::{TimeDelta, Utc};
    use pulpo_common::session::{Runtime, Session};
    use std::collections::HashMap;
    use uuid::Uuid;

    /// Insert an active session aged `age` with the given cost/token metadata.
    async fn insert(
        store: &Store,
        name: &str,
        age: TimeDelta,
        cost: Option<&str>,
        input_tokens: Option<&str>,
    ) -> Session {
        let mut metadata = HashMap::new();
        if let Some(c) = cost {
            metadata.insert(meta::SESSION_COST_USD.to_owned(), c.to_owned());
        }
        if let Some(t) = input_tokens {
            metadata.insert(meta::TOTAL_INPUT_TOKENS.to_owned(), t.to_owned());
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
            created_at: Utc::now() - age,
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

    fn cost_cfg(ceiling: f64, action: BurnAction) -> BurnConfig {
        BurnConfig {
            ceiling_usd_per_hour: Some(ceiling),
            ceiling_tokens_per_hour: None,
            action,
        }
    }

    fn tokens_cfg(ceiling: u64, action: BurnAction) -> BurnConfig {
        BurnConfig {
            ceiling_usd_per_hour: None,
            ceiling_tokens_per_hour: Some(ceiling),
            action,
        }
    }

    #[tokio::test]
    async fn test_no_ceiling_configured_skips() {
        let store = test_store().await;
        // Session that would be way over any ceiling, but no ceilings set.
        let s = insert(&store, "no-ceil", TimeDelta::hours(1), Some("99.0"), None).await;
        enforce_burn_ceiling(&backend(), &store, &ready_ctx(), &BurnConfig::default()).await;
        let after = store.get_session(&s.id.to_string()).await.unwrap().unwrap();
        assert_eq!(after.status, SessionStatus::Active);
        assert!(after.meta_str(meta::BURN_ALERTED_AT).is_none());
    }

    #[tokio::test]
    async fn test_cost_rate_over_ceiling_alerts() {
        let store = test_store().await;
        // $5 over 1h = $5/hr, ceiling $2/hr → over.
        let s = insert(&store, "cost-over", TimeDelta::hours(1), Some("5.0"), None).await;
        enforce_burn_ceiling(
            &backend(),
            &store,
            &ready_ctx(),
            &cost_cfg(2.0, BurnAction::Alert),
        )
        .await;
        let after = store.get_session(&s.id.to_string()).await.unwrap().unwrap();
        // Alert-only default: not stopped, but flagged.
        assert_eq!(after.status, SessionStatus::Active);
        assert!(after.meta_str(meta::BURN_ALERTED_AT).is_some());
    }

    #[tokio::test]
    async fn test_cost_rate_under_ceiling_does_nothing() {
        let store = test_store().await;
        // $1 over 1h = $1/hr, ceiling $5/hr → under.
        let s = insert(&store, "cost-under", TimeDelta::hours(1), Some("1.0"), None).await;
        enforce_burn_ceiling(
            &backend(),
            &store,
            &ready_ctx(),
            &cost_cfg(5.0, BurnAction::Alert),
        )
        .await;
        let after = store.get_session(&s.id.to_string()).await.unwrap().unwrap();
        assert_eq!(after.status, SessionStatus::Active);
        assert!(after.meta_str(meta::BURN_ALERTED_AT).is_none());
    }

    #[tokio::test]
    async fn test_tokens_rate_over_ceiling_alerts() {
        let store = test_store().await;
        // 6000 tokens over 1h = 6000/hr, ceiling 3000/hr → over. No cost (Codex-like).
        let s = insert(&store, "tok-over", TimeDelta::hours(1), None, Some("6000")).await;
        enforce_burn_ceiling(
            &backend(),
            &store,
            &ready_ctx(),
            &tokens_cfg(3000, BurnAction::Alert),
        )
        .await;
        let after = store.get_session(&s.id.to_string()).await.unwrap().unwrap();
        assert_eq!(after.status, SessionStatus::Active);
        assert!(after.meta_str(meta::BURN_ALERTED_AT).is_some());
    }

    #[tokio::test]
    async fn test_tokens_rate_under_ceiling_does_nothing() {
        let store = test_store().await;
        // 1000 tokens over 1h = 1000/hr, ceiling 3000/hr → under.
        let s = insert(&store, "tok-under", TimeDelta::hours(1), None, Some("1000")).await;
        enforce_burn_ceiling(
            &backend(),
            &store,
            &ready_ctx(),
            &tokens_cfg(3000, BurnAction::Alert),
        )
        .await;
        let after = store.get_session(&s.id.to_string()).await.unwrap().unwrap();
        assert_eq!(after.status, SessionStatus::Active);
        assert!(after.meta_str(meta::BURN_ALERTED_AT).is_none());
    }

    #[tokio::test]
    async fn test_too_young_session_ignored() {
        let store = test_store().await;
        // Age 0 → per_hour returns None even with a huge cost.
        let s = insert(
            &store,
            "fresh",
            TimeDelta::zero(),
            Some("99.0"),
            Some("999999"),
        )
        .await;
        let cfg = BurnConfig {
            ceiling_usd_per_hour: Some(0.01),
            ceiling_tokens_per_hour: Some(1),
            action: BurnAction::Alert,
        };
        enforce_burn_ceiling(&backend(), &store, &ready_ctx(), &cfg).await;
        let after = store.get_session(&s.id.to_string()).await.unwrap().unwrap();
        assert_eq!(after.status, SessionStatus::Active);
        assert!(after.meta_str(meta::BURN_ALERTED_AT).is_none());
    }

    #[tokio::test]
    async fn test_either_ceiling_triggers() {
        let store = test_store().await;
        // Cost under its ceiling, but tokens over theirs → still triggers.
        // $1/hr (ceiling $5/hr, under), 6000 tok/hr (ceiling 3000/hr, over).
        let s = insert(
            &store,
            "mixed",
            TimeDelta::hours(1),
            Some("1.0"),
            Some("6000"),
        )
        .await;
        let cfg = BurnConfig {
            ceiling_usd_per_hour: Some(5.0),
            ceiling_tokens_per_hour: Some(3000),
            action: BurnAction::Alert,
        };
        enforce_burn_ceiling(&backend(), &store, &ready_ctx(), &cfg).await;
        let after = store.get_session(&s.id.to_string()).await.unwrap().unwrap();
        assert!(after.meta_str(meta::BURN_ALERTED_AT).is_some());
    }

    #[tokio::test]
    async fn test_one_shot_dedup() {
        let store = test_store().await;
        let s = insert(&store, "dedup", TimeDelta::hours(1), Some("5.0"), None).await;
        enforce_burn_ceiling(
            &backend(),
            &store,
            &ready_ctx(),
            &cost_cfg(2.0, BurnAction::Alert),
        )
        .await;
        let first = store.get_session(&s.id.to_string()).await.unwrap().unwrap();
        let alerted_at = first.meta_str(meta::BURN_ALERTED_AT).map(str::to_owned);
        assert!(alerted_at.is_some());

        // Second pass must not change the alert timestamp.
        enforce_burn_ceiling(
            &backend(),
            &store,
            &ready_ctx(),
            &cost_cfg(2.0, BurnAction::Alert),
        )
        .await;
        let second = store.get_session(&s.id.to_string()).await.unwrap().unwrap();
        assert_eq!(
            second.meta_str(meta::BURN_ALERTED_AT).map(str::to_owned),
            alerted_at
        );
    }

    #[tokio::test]
    async fn test_emits_usage_alert_event() {
        let store = test_store().await;
        insert(&store, "evt", TimeDelta::hours(1), Some("4.0"), None).await;
        let (tx, mut rx) = tokio::sync::broadcast::channel(8);
        let ctx = ReadyContext {
            event_tx: Some(tx),
            node_name: "node-a".into(),
        };

        enforce_burn_ceiling(&backend(), &store, &ctx, &cost_cfg(2.0, BurnAction::Alert)).await;

        let event = rx.try_recv().expect("expected a usage alert event");
        match event {
            PulpoEvent::UsageAlert(a) => {
                assert_eq!(a.alert_kind, "burn_ceiling");
                assert_eq!(a.node_name, "node-a");
                assert_eq!(a.budget_usd, None);
                assert_eq!(a.cost_usd, Some(4.0));
                assert!(a.message.contains("/hr"));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_emits_token_message_without_cost() {
        let store = test_store().await;
        insert(&store, "tok-evt", TimeDelta::hours(1), None, Some("6000")).await;
        let (tx, mut rx) = tokio::sync::broadcast::channel(8);
        let ctx = ReadyContext {
            event_tx: Some(tx),
            node_name: "node-b".into(),
        };

        enforce_burn_ceiling(
            &backend(),
            &store,
            &ctx,
            &tokens_cfg(3000, BurnAction::Alert),
        )
        .await;

        let event = rx.try_recv().expect("expected a usage alert event");
        match event {
            PulpoEvent::UsageAlert(a) => {
                assert_eq!(a.alert_kind, "burn_ceiling");
                assert_eq!(a.cost_usd, None);
                assert!(a.message.contains("tokens/hr"));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_stop_action_stops_session() {
        let store = test_store().await;
        let s = insert(&store, "stopper", TimeDelta::hours(1), Some("5.0"), None).await;
        enforce_burn_ceiling(
            &backend(),
            &store,
            &ready_ctx(),
            &cost_cfg(2.0, BurnAction::Stop),
        )
        .await;
        let after = store.get_session(&s.id.to_string()).await.unwrap().unwrap();
        assert_eq!(after.status, SessionStatus::Stopped);
        assert_eq!(after.intervention_code, Some(InterventionCode::BurnRate));
        assert!(after.meta_str(meta::BURN_ALERTED_AT).is_some());
    }

    #[tokio::test]
    async fn test_stop_action_cleans_up_worktree() {
        let store = test_store().await;
        // A real worktree dir so the best-effort cleanup branch runs and removes it.
        let tmpdir = tempfile::tempdir().unwrap();
        let wt_path = tmpdir
            .path()
            .join(".pulpo")
            .join("worktrees")
            .join("burn-wt");
        std::fs::create_dir_all(&wt_path).unwrap();
        let wt_str = wt_path.to_str().unwrap().to_owned();

        let mut metadata = HashMap::new();
        metadata.insert(meta::SESSION_COST_USD.to_owned(), "5.0".to_owned());
        let session = Session {
            id: Uuid::new_v4(),
            name: "burn-wt".into(),
            workdir: "/tmp/repo".into(),
            command: "claude".into(),
            status: SessionStatus::Active,
            runtime: Runtime::Tmux,
            backend_session_id: Some("$1".into()),
            metadata: Some(metadata),
            worktree_path: Some(wt_str),
            created_at: Utc::now() - TimeDelta::hours(1),
            ..Default::default()
        };
        store.insert_session(&session).await.unwrap();

        enforce_burn_ceiling(
            &backend(),
            &store,
            &ready_ctx(),
            &cost_cfg(2.0, BurnAction::Stop),
        )
        .await;

        let after = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after.status, SessionStatus::Stopped);
        assert_eq!(after.intervention_code, Some(InterventionCode::BurnRate));
        // Worktree directory cleaned up.
        assert!(!wt_path.exists());
    }

    #[tokio::test]
    async fn test_non_active_session_ignored() {
        let store = test_store().await;
        let mut session = insert(&store, "idle-one", TimeDelta::hours(1), Some("9.0"), None).await;
        // Flip to Idle: only Active sessions are checked.
        session.status = SessionStatus::Idle;
        store
            .update_session_status(&session.id.to_string(), SessionStatus::Idle)
            .await
            .unwrap();
        enforce_burn_ceiling(
            &backend(),
            &store,
            &ready_ctx(),
            &cost_cfg(1.0, BurnAction::Stop),
        )
        .await;
        let after = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after.status, SessionStatus::Idle);
        assert!(after.meta_str(meta::BURN_ALERTED_AT).is_none());
    }

    #[test]
    fn test_burn_message_cost_only() {
        let msg = burn_message(Some(2.0), Some(5.0), true, None, None, false);
        assert!(msg.contains("$5.00/hr"));
        assert!(msg.contains("$2.00/hr"));
        assert!(!msg.contains("tokens"));
    }

    #[test]
    fn test_burn_message_tokens_only() {
        let msg = burn_message(None, None, false, Some(3000), Some(6000.0), true);
        assert!(msg.contains("6000 tokens/hr"));
        assert!(msg.contains("3000 tokens/hr"));
    }

    #[test]
    fn test_burn_message_both() {
        let msg = burn_message(Some(2.0), Some(5.0), true, Some(3000), Some(6000.0), true);
        assert!(msg.contains("$5.00/hr"));
        assert!(msg.contains("6000 tokens/hr"));
        assert!(msg.contains("; "));
    }
}
