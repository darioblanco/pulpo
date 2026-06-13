//! Prometheus `/metrics` endpoint — pull-based, stateless.
//!
//! Every gauge is computed from the current store state on each scrape; nothing
//! is persisted. The endpoint is off by default and enabled via `[metrics]
//! enabled = true`. When disabled, the handler returns `404` so the route table
//! stays static while the endpoint is effectively absent.
//!
//! This complements webhooks (push of discrete events) with pull-based,
//! continuous dashboard state. Pulpo emits the numbers; it is not a TSDB —
//! Prometheus scrapes and stores them.

use std::sync::Arc;

use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use pulpo_common::session::{Session, SessionStatus, meta};

/// Prometheus text exposition format content type (version 0.0.4).
const CONTENT_TYPE: &str = "text/plain; version=0.0.4";

/// All session statuses, so every series is always present (zero when empty).
/// A stable label set keeps Prometheus graphs continuous across scrapes.
const ALL_STATUSES: [SessionStatus; 6] = [
    SessionStatus::Active,
    SessionStatus::Idle,
    SessionStatus::Ready,
    SessionStatus::Stopped,
    SessionStatus::Lost,
    SessionStatus::Creating,
];

/// Statuses considered terminal (the session is no longer doing work).
/// Cost is summed only across non-terminal sessions to reflect live spend.
const fn is_terminal(status: SessionStatus) -> bool {
    matches!(status, SessionStatus::Stopped | SessionStatus::Lost)
}

/// Escape a Prometheus label value per the text exposition spec:
/// backslash, double-quote, and newline are escaped. Returns owned only when
/// an escape was needed; otherwise borrows the input.
fn escape_label_value(value: &str) -> std::borrow::Cow<'_, str> {
    if value.contains(['\\', '"', '\n']) {
        let mut out = String::with_capacity(value.len() + 8);
        for ch in value.chars() {
            match ch {
                '\\' => out.push_str("\\\\"),
                '"' => out.push_str("\\\""),
                '\n' => out.push_str("\\n"),
                other => out.push(other),
            }
        }
        std::borrow::Cow::Owned(out)
    } else {
        std::borrow::Cow::Borrowed(value)
    }
}

/// Render the Prometheus text exposition for the given sessions.
///
/// Pure and total: no I/O, no panics. Computes every gauge from the snapshot so
/// the result is fully unit-testable. Output ends with a trailing newline and
/// parses as valid Prometheus text.
#[allow(clippy::cast_precision_loss)]
pub fn render_metrics(sessions: &[Session]) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();

    // pulpo_sessions{status="..."} — count per status (every status present).
    out.push_str("# HELP pulpo_sessions Number of sessions by status.\n");
    out.push_str("# TYPE pulpo_sessions gauge\n");
    for status in ALL_STATUSES {
        let count = sessions.iter().filter(|s| s.status == status).count();
        // Status labels are a fixed enum (no escaping needed) but route through
        // the escaper so the format stays correct if the enum ever grows.
        let status_str = status.to_string();
        let label = escape_label_value(&status_str);
        // Writing to a String is infallible; discard the always-Ok Result.
        let _ = writeln!(out, "pulpo_sessions{{status=\"{label}\"}} {count}");
    }

    // pulpo_session_cost_usd — summed cost across non-terminal sessions.
    // `+ 0.0` collapses a possible `-0.0` (from summing negatives/empties) to
    // `0.0` so the rendered value is never the surprising `-0`.
    let cost_usd: f64 = sessions
        .iter()
        .filter(|s| !is_terminal(s.status))
        .filter_map(|s| s.meta_parsed::<f64>(meta::SESSION_COST_USD))
        .sum::<f64>()
        + 0.0;
    out.push_str(
        "# HELP pulpo_session_cost_usd Summed cost in USD across non-terminal sessions.\n",
    );
    out.push_str("# TYPE pulpo_session_cost_usd gauge\n");
    let _ = writeln!(out, "pulpo_session_cost_usd {cost_usd}");

    // pulpo_session_tokens — summed input+output tokens across all sessions.
    let tokens: u64 = sessions
        .iter()
        .map(|s| {
            s.meta_parsed::<u64>(meta::TOTAL_INPUT_TOKENS).unwrap_or(0)
                + s.meta_parsed::<u64>(meta::TOTAL_OUTPUT_TOKENS).unwrap_or(0)
        })
        .sum();
    out.push_str("# HELP pulpo_session_tokens Summed input and output tokens across sessions.\n");
    out.push_str("# TYPE pulpo_session_tokens gauge\n");
    let _ = writeln!(out, "pulpo_session_tokens {tokens}");

    // pulpo_sessions_with_budget — count of sessions with a cost budget set.
    let with_budget = sessions
        .iter()
        .filter(|s| s.meta_str(meta::BUDGET_COST_USD).is_some())
        .count();
    out.push_str("# HELP pulpo_sessions_with_budget Number of sessions with a cost budget set.\n");
    out.push_str("# TYPE pulpo_sessions_with_budget gauge\n");
    let _ = writeln!(out, "pulpo_sessions_with_budget {with_budget}");

    // pulpo_build_info{version="..."} — always 1, carries the version label.
    let version = escape_label_value(env!("CARGO_PKG_VERSION"));
    out.push_str("# HELP pulpo_build_info Build information; value is always 1.\n");
    out.push_str("# TYPE pulpo_build_info gauge\n");
    let _ = writeln!(out, "pulpo_build_info{{version=\"{version}\"}} 1");

    out
}

/// `GET /api/v1/metrics` — Prometheus scrape endpoint.
///
/// Returns `404` when `[metrics] enabled` is false (endpoint disabled). When
/// enabled, lists sessions from the store and renders the text exposition.
pub async fn metrics(State(state): State<Arc<super::AppState>>) -> Response {
    let enabled = state.config.read().await.metrics.enabled;
    if !enabled {
        return StatusCode::NOT_FOUND.into_response();
    }

    match state.store.list_sessions().await {
        Ok(sessions) => {
            let body = render_metrics(&sessions);
            ([(header::CONTENT_TYPE, CONTENT_TYPE)], body).into_response()
        }
        Err(e) => {
            tracing::error!("metrics: failed to list sessions: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use uuid::Uuid;

    fn session(status: SessionStatus, meta: &[(&str, &str)]) -> Session {
        let metadata = if meta.is_empty() {
            None
        } else {
            Some(
                meta.iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                    .collect::<HashMap<_, _>>(),
            )
        };
        Session {
            id: Uuid::new_v4(),
            name: "s".into(),
            workdir: "/tmp".into(),
            command: "claude".into(),
            status,
            metadata,
            ..Default::default()
        }
    }

    /// Extract the value of a single (label-less) metric line.
    fn metric_value(text: &str, name: &str) -> Option<String> {
        text.lines().find_map(|line| {
            line.strip_prefix(name)
                .and_then(|rest| rest.strip_prefix(' '))
                .map(str::to_owned)
        })
    }

    #[test]
    fn test_escape_label_value_plain() {
        assert_eq!(escape_label_value("plain"), "plain");
        // Borrowed (no allocation) when nothing to escape.
        assert!(matches!(
            escape_label_value("plain"),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    #[test]
    fn test_escape_label_value_special_chars() {
        assert_eq!(
            escape_label_value(r#"a\b"c"#),
            r#"a\\b\"c"#,
            "backslash and quote must be escaped"
        );
        assert_eq!(escape_label_value("line1\nline2"), "line1\\nline2");
    }

    #[test]
    fn test_is_terminal() {
        assert!(is_terminal(SessionStatus::Stopped));
        assert!(is_terminal(SessionStatus::Lost));
        assert!(!is_terminal(SessionStatus::Active));
        assert!(!is_terminal(SessionStatus::Idle));
        assert!(!is_terminal(SessionStatus::Ready));
        assert!(!is_terminal(SessionStatus::Creating));
    }

    #[test]
    fn test_render_empty_has_all_series_and_help() {
        let text = render_metrics(&[]);
        // Every status series present and zero.
        for status in ["active", "idle", "ready", "stopped", "lost", "creating"] {
            assert!(
                text.contains(&format!("pulpo_sessions{{status=\"{status}\"}} 0")),
                "missing zero series for {status}:\n{text}"
            );
        }
        // HELP and TYPE lines for each metric.
        for metric in [
            "pulpo_sessions",
            "pulpo_session_cost_usd",
            "pulpo_session_tokens",
            "pulpo_sessions_with_budget",
            "pulpo_build_info",
        ] {
            assert!(
                text.contains(&format!("# HELP {metric} ")),
                "no HELP {metric}"
            );
            assert!(
                text.contains(&format!("# TYPE {metric} gauge")),
                "no TYPE {metric}"
            );
        }
        // Trailing newline.
        assert!(text.ends_with('\n'));
        // build_info carries the crate version and is 1.
        assert!(text.contains(&format!(
            "pulpo_build_info{{version=\"{}\"}} 1",
            env!("CARGO_PKG_VERSION")
        )));
        // Empty snapshot → zero aggregates.
        assert_eq!(
            metric_value(&text, "pulpo_session_cost_usd").as_deref(),
            Some("0")
        );
        assert_eq!(
            metric_value(&text, "pulpo_session_tokens").as_deref(),
            Some("0")
        );
        assert_eq!(
            metric_value(&text, "pulpo_sessions_with_budget").as_deref(),
            Some("0")
        );
    }

    #[test]
    fn test_render_per_status_counts() {
        let sessions = vec![
            session(SessionStatus::Active, &[]),
            session(SessionStatus::Active, &[]),
            session(SessionStatus::Idle, &[]),
            session(SessionStatus::Stopped, &[]),
            session(SessionStatus::Lost, &[]),
            session(SessionStatus::Ready, &[]),
            session(SessionStatus::Creating, &[]),
        ];
        let text = render_metrics(&sessions);
        assert!(text.contains("pulpo_sessions{status=\"active\"} 2"));
        assert!(text.contains("pulpo_sessions{status=\"idle\"} 1"));
        assert!(text.contains("pulpo_sessions{status=\"ready\"} 1"));
        assert!(text.contains("pulpo_sessions{status=\"stopped\"} 1"));
        assert!(text.contains("pulpo_sessions{status=\"lost\"} 1"));
        assert!(text.contains("pulpo_sessions{status=\"creating\"} 1"));
    }

    #[test]
    fn test_render_cost_sum_excludes_terminal() {
        let sessions = vec![
            session(SessionStatus::Active, &[(meta::SESSION_COST_USD, "1.5")]),
            session(SessionStatus::Idle, &[(meta::SESSION_COST_USD, "2.25")]),
            // Terminal sessions must NOT count toward live cost.
            session(SessionStatus::Stopped, &[(meta::SESSION_COST_USD, "100")]),
            session(SessionStatus::Lost, &[(meta::SESSION_COST_USD, "100")]),
            // No cost metadata → contributes nothing.
            session(SessionStatus::Active, &[]),
        ];
        let text = render_metrics(&sessions);
        assert_eq!(
            metric_value(&text, "pulpo_session_cost_usd").as_deref(),
            Some("3.75")
        );
    }

    #[test]
    fn test_render_token_sum() {
        let sessions = vec![
            session(
                SessionStatus::Active,
                &[
                    (meta::TOTAL_INPUT_TOKENS, "100"),
                    (meta::TOTAL_OUTPUT_TOKENS, "50"),
                ],
            ),
            // Only input tokens present (output absent → treated as 0).
            session(SessionStatus::Idle, &[(meta::TOTAL_INPUT_TOKENS, "10")]),
            // Even terminal sessions count toward cumulative token totals.
            session(SessionStatus::Stopped, &[(meta::TOTAL_OUTPUT_TOKENS, "5")]),
        ];
        let text = render_metrics(&sessions);
        assert_eq!(
            metric_value(&text, "pulpo_session_tokens").as_deref(),
            Some("165")
        );
    }

    #[test]
    fn test_render_with_budget_count() {
        let sessions = vec![
            session(SessionStatus::Active, &[(meta::BUDGET_COST_USD, "10")]),
            session(SessionStatus::Idle, &[(meta::BUDGET_COST_USD, "5")]),
            session(SessionStatus::Active, &[]),
        ];
        let text = render_metrics(&sessions);
        assert_eq!(
            metric_value(&text, "pulpo_sessions_with_budget").as_deref(),
            Some("2")
        );
    }

    #[test]
    fn test_render_ignores_malformed_cost_metadata() {
        // A non-numeric cost value must be ignored, not panic or poison the sum.
        let sessions = vec![
            session(
                SessionStatus::Active,
                &[(meta::SESSION_COST_USD, "not-a-number")],
            ),
            session(SessionStatus::Active, &[(meta::SESSION_COST_USD, "2.0")]),
        ];
        let text = render_metrics(&sessions);
        assert_eq!(
            metric_value(&text, "pulpo_session_cost_usd").as_deref(),
            Some("2")
        );
    }

    // -- Handler tests --

    use crate::api::AppState;
    use crate::backend::StubBackend;
    use crate::config::{Config, MetricsConfig, NodeConfig};
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;
    use axum::body::to_bytes;
    use axum::http::StatusCode;

    async fn state_with_metrics(enabled: bool) -> Arc<AppState> {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "test-node".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig::default(),
            peers: HashMap::new(),
            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            plans: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
            webhooks: Vec::new(),
            docker: None,
            controller: crate::config::ControllerConfig::default(),
            metrics: MetricsConfig { enabled },
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        AppState::new(config, manager, peer_registry, store)
    }

    #[tokio::test]
    async fn test_handler_disabled_returns_404() {
        let state = state_with_metrics(false).await;
        let resp = metrics(State(state)).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_handler_enabled_returns_text() {
        let state = state_with_metrics(true).await;
        let resp = metrics(State(state)).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            CONTENT_TYPE
        );
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("# TYPE pulpo_sessions gauge"));
        assert!(text.contains("pulpo_build_info{version="));
    }

    #[tokio::test]
    async fn test_handler_store_error_returns_500() {
        let state = state_with_metrics(true).await;
        // Drop the sessions table so list_sessions errors → 500.
        sqlx::query("DROP TABLE sessions")
            .execute(state.store.pool())
            .await
            .unwrap();
        let resp = metrics(State(state)).await;
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
