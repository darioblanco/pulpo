use pulpo_common::event::SessionEvent;
use pulpo_common::session::{Session, SessionStatus, meta};
use tracing::{info, warn};

use super::output_patterns;
use crate::store::Store;

/// Detect PR URL, branch name, rate limits, errors, and token usage from session output.
/// PR and branch are only written if not already present. Transient signals (rate limits,
/// errors) are always updated and cleared when no longer detected.
#[allow(clippy::too_many_lines)]
pub(super) async fn detect_and_store_output_metadata(
    store: &Store,
    session: &Session,
    output: &str,
) {
    let has_pr = session.meta_str(meta::PR_URL).is_some();
    if !has_pr && let Some(pr_url) = output_patterns::extract_pr_url(output) {
        if let Err(error) = store
            .update_session_metadata_field(&session.id.to_string(), meta::PR_URL, &pr_url)
            .await
        {
            warn!(
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
        if let Err(error) = store
            .update_session_metadata_field(&session.id.to_string(), meta::BRANCH, &branch)
            .await
        {
            warn!(
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

    if let Some(rate_msg) = output_patterns::detect_rate_limit(output) {
        let timestamp = chrono::Utc::now().to_rfc3339();
        if let Err(error) = store
            .update_session_metadata_field(&session.id.to_string(), meta::RATE_LIMIT, &rate_msg)
            .await
        {
            warn!(
                session_name = %session.name,
                "Failed to store rate_limit metadata: {error}"
            );
        }
        if let Err(error) = store
            .update_session_metadata_field(&session.id.to_string(), meta::RATE_LIMIT_AT, &timestamp)
            .await
        {
            warn!(
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

    let current_error = output_patterns::detect_error(output);
    let stored_error = session.meta_str(meta::ERROR_STATUS);
    match (&current_error, stored_error) {
        (Some(error_status), _) => {
            let timestamp = chrono::Utc::now().to_rfc3339();
            if let Err(error) = store
                .update_session_metadata_field(
                    &session.id.to_string(),
                    meta::ERROR_STATUS,
                    error_status,
                )
                .await
            {
                warn!(
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
            if let Err(error) = store
                .remove_session_metadata_field(&session.id.to_string(), meta::ERROR_STATUS)
                .await
            {
                warn!(
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

    if let Some(usage) = output_patterns::extract_agent_usage(output) {
        store_agent_usage(store, session, &usage).await;
    }
}

/// Build a `SessionEvent` from a session, populating token/cost enrichment from metadata.
pub(super) fn build_session_event(
    session: &Session,
    status: SessionStatus,
    previous: Option<SessionStatus>,
    node_name: &str,
    output: Option<String>,
) -> SessionEvent {
    SessionEvent {
        session_id: session.id.to_string(),
        session_name: session.name.clone(),
        status: status.to_string(),
        previous_status: previous.map(|previous_status| previous_status.to_string()),
        node_name: node_name.to_owned(),
        output_snippet: output,
        timestamp: chrono::Utc::now().to_rfc3339(),
        total_input_tokens: session.meta_parsed(meta::TOTAL_INPUT_TOKENS),
        total_output_tokens: session.meta_parsed(meta::TOTAL_OUTPUT_TOKENS),
        session_cost_usd: session.meta_parsed(meta::SESSION_COST_USD),
        ..Default::default()
    }
}

/// Resolve a token field value with accumulation for agent restarts.
/// If new value < stored, the agent was restarted — accumulate.
/// Returns `None` if the value is unchanged.
pub(super) fn accumulate_token_value(new_val: u64, stored: Option<&str>) -> Option<u64> {
    let previous = stored.and_then(|value| value.parse::<u64>().ok());
    match previous {
        Some(stored_value) if new_val == stored_value => None,
        Some(stored_value) if new_val < stored_value => Some(stored_value + new_val),
        _ => Some(new_val),
    }
}

/// Store agent usage data as metadata fields in a single DB round-trip.
///
/// When new token counts are lower than stored values, the agent was restarted —
/// previous totals are added to new values instead of overwriting.
async fn store_agent_usage(store: &Store, session: &Session, usage: &output_patterns::AgentUsage) {
    let session_id = session.id.to_string();
    let mut updates: Vec<(&str, String)> = Vec::new();

    let input = usage
        .input_tokens
        .or_else(|| usage.total_tokens.filter(|_| usage.output_tokens.is_none()));
    if let Some(value) = input
        && let Some(final_value) =
            accumulate_token_value(value, session.meta_str(meta::TOTAL_INPUT_TOKENS))
    {
        updates.push((meta::TOTAL_INPUT_TOKENS, final_value.to_string()));
    }
    if let Some(value) = usage.output_tokens
        && let Some(final_value) =
            accumulate_token_value(value, session.meta_str(meta::TOTAL_OUTPUT_TOKENS))
    {
        updates.push((meta::TOTAL_OUTPUT_TOKENS, final_value.to_string()));
    }
    if let Some(value) = usage.cache_write_tokens
        && let Some(final_value) =
            accumulate_token_value(value, session.meta_str(meta::CACHE_WRITE_TOKENS))
    {
        updates.push((meta::CACHE_WRITE_TOKENS, final_value.to_string()));
    }
    if let Some(value) = usage.cache_read_tokens
        && let Some(final_value) =
            accumulate_token_value(value, session.meta_str(meta::CACHE_READ_TOKENS))
    {
        updates.push((meta::CACHE_READ_TOKENS, final_value.to_string()));
    }
    if let Some(cost) = usage.session_cost_usd {
        let stored_cost = session
            .meta_str(meta::SESSION_COST_USD)
            .and_then(|value| value.parse::<f64>().ok());
        let final_cost = match stored_cost {
            Some(previous) if (cost - previous).abs() < 1e-7 => None,
            Some(previous) if cost < previous => Some(previous + cost),
            _ => Some(cost),
        };
        if let Some(final_cost) = final_cost {
            updates.push((meta::SESSION_COST_USD, format!("{final_cost:.6}")));
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

#[cfg(test)]
mod tests {
    use super::*;
    use pulpo_common::session::Runtime;
    use std::collections::HashMap;
    use uuid::Uuid;

    async fn test_store() -> Store {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        store
    }

    async fn insert_session(store: &Store, name: &str) -> Session {
        let session = Session {
            id: Uuid::new_v4(),
            name: name.into(),
            workdir: "/tmp/repo".into(),
            command: "echo test".into(),
            status: SessionStatus::Active,
            runtime: Runtime::Tmux,
            metadata: Some(HashMap::new()),
            ..Default::default()
        };
        store.insert_session(&session).await.unwrap();
        session
    }

    #[test]
    fn test_accumulate_token_value_initial_set() {
        assert_eq!(accumulate_token_value(42, None), Some(42));
    }

    #[test]
    fn test_accumulate_token_value_unchanged_returns_none() {
        assert_eq!(accumulate_token_value(42, Some("42")), None);
    }

    #[test]
    fn test_accumulate_token_value_restart_accumulates() {
        assert_eq!(accumulate_token_value(10, Some("100")), Some(110));
    }

    #[test]
    fn test_accumulate_token_value_invalid_previous_replaces() {
        assert_eq!(accumulate_token_value(7, Some("invalid")), Some(7));
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
            status: SessionStatus::Active,
            metadata: Some(metadata),
            ..Default::default()
        };

        let event = build_session_event(
            &session,
            SessionStatus::Idle,
            Some(SessionStatus::Active),
            "node-a",
            Some("snippet".into()),
        );

        assert_eq!(event.session_name, "event-test");
        assert_eq!(event.status, "idle");
        assert_eq!(event.previous_status.as_deref(), Some("active"));
        assert_eq!(event.node_name, "node-a");
        assert_eq!(event.output_snippet.as_deref(), Some("snippet"));
        assert_eq!(event.total_input_tokens, Some(123));
        assert_eq!(event.total_output_tokens, Some(456));
        assert_eq!(event.session_cost_usd, Some(1.25));
    }

    #[tokio::test]
    async fn test_store_agent_usage_writes_usage_fields() {
        let store = test_store().await;
        let session = insert_session(&store, "usage-write").await;
        let usage = output_patterns::AgentUsage {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_write_tokens: Some(10),
            cache_read_tokens: Some(20),
            total_tokens: None,
            session_cost_usd: Some(0.75),
        };

        store_agent_usage(&store, &session, &usage).await;

        let fetched = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.meta_str(meta::TOTAL_INPUT_TOKENS), Some("100"));
        assert_eq!(fetched.meta_str(meta::TOTAL_OUTPUT_TOKENS), Some("50"));
        assert_eq!(fetched.meta_str(meta::CACHE_WRITE_TOKENS), Some("10"));
        assert_eq!(fetched.meta_str(meta::CACHE_READ_TOKENS), Some("20"));
        assert_eq!(fetched.meta_str(meta::SESSION_COST_USD), Some("0.750000"));
    }

    #[tokio::test]
    async fn test_store_agent_usage_accumulates_restart_values() {
        let store = test_store().await;
        let session = insert_session(&store, "usage-restart").await;
        store
            .batch_update_session_metadata(
                &session.id.to_string(),
                &[
                    (meta::TOTAL_INPUT_TOKENS, "100"),
                    (meta::TOTAL_OUTPUT_TOKENS, "80"),
                    (meta::CACHE_WRITE_TOKENS, "30"),
                    (meta::CACHE_READ_TOKENS, "20"),
                    (meta::SESSION_COST_USD, "2.500000"),
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
        let usage = output_patterns::AgentUsage {
            input_tokens: Some(10),
            output_tokens: Some(5),
            cache_write_tokens: Some(3),
            cache_read_tokens: Some(2),
            total_tokens: None,
            session_cost_usd: Some(0.5),
        };

        store_agent_usage(&store, &fetched, &usage).await;

        let updated = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.meta_str(meta::TOTAL_INPUT_TOKENS), Some("110"));
        assert_eq!(updated.meta_str(meta::TOTAL_OUTPUT_TOKENS), Some("85"));
        assert_eq!(updated.meta_str(meta::CACHE_WRITE_TOKENS), Some("33"));
        assert_eq!(updated.meta_str(meta::CACHE_READ_TOKENS), Some("22"));
        assert_eq!(updated.meta_str(meta::SESSION_COST_USD), Some("3.000000"));
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
        detect_and_store_output_metadata(&store, &fetched, "all green now").await;

        let updated = store
            .get_session(&session.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.meta_str(meta::ERROR_STATUS), None);
        assert_eq!(updated.meta_str(meta::ERROR_STATUS_AT), None);
    }
}
