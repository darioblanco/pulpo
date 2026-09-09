//! `pulpo hook <harness>` — invoked by a harness's own hook config (e.g. Claude
//! Code's `--settings`-injected `SessionStart`/`Stop`/`Notification`/... hooks) to
//! report a lifecycle event to the daemon.
//!
//! Contract: a hook must never block or break the agent it's wired into. Every
//! function here is built to always succeed from the caller's point of view — no
//! error is ever surfaced, and `execute_hook` always returns (nothing to print on
//! success, matching every other silent-success path in this CLI).

use std::time::Duration;

use crate::Cli;
use crate::http::{authed_post, base_url, resolve_node, resolve_token};

/// Environment variable the command wrapper (`session/manager.rs::wrap_command`)
/// exports into every pulpo session — how a hook finds out which session it's
/// running in.
pub const SESSION_ID_ENV: &str = "PULPO_SESSION_ID";

/// Parse a hook's raw stdin payload into a JSON value. Empty/whitespace-only input
/// (and anything that fails to parse as JSON) becomes an empty object — a hook must
/// never fail the hosting agent's turn just because pulpo couldn't make sense of what
/// it was given.
pub fn parse_hook_event_json(raw: &str) -> serde_json::Value {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return serde_json::json!({});
    }
    serde_json::from_str(trimmed).unwrap_or_else(|_| serde_json::json!({}))
}

/// Build the `POST /api/v1/sessions/{id}/harness-events` body: `{"harness":
/// <harness>, "event": <raw event, with hook_event_name filled in from `--event`
/// when the payload doesn't already carry one>}`.
pub fn build_hook_body(
    harness: &str,
    event_name: Option<&str>,
    raw_stdin: &str,
) -> serde_json::Value {
    let mut event = parse_hook_event_json(raw_stdin);
    if let (Some(name), Some(obj)) = (event_name, event.as_object_mut()) {
        obj.entry("hook_event_name")
            .or_insert_with(|| serde_json::Value::String(name.to_owned()));
    }
    serde_json::json!({ "harness": harness, "event": event })
}

/// POST the hook event to the daemon. Every failure (network, non-2xx response) is
/// swallowed — a hook reporting a lifecycle event must never fail the agent's turn.
pub async fn post_hook_event(
    client: &reqwest::Client,
    base: &str,
    token: Option<&str>,
    session_id: &str,
    harness: &str,
    event_name: Option<&str>,
    raw_stdin: &str,
) {
    let body = build_hook_body(harness, event_name, raw_stdin);
    let url = format!("{base}/api/v1/sessions/{session_id}/harness-events");
    let request = authed_post(client, url, token)
        .json(&body)
        .timeout(Duration::from_secs(2));
    let _ = request.send().await;
}

/// Real stdin read for a hook invocation — command hooks receive the harness's JSON
/// payload on stdin. Run on a blocking thread so a stuck/interactive stdin (the hook
/// run outside its normal harness-piped context) can never hang the async runtime;
/// if it never completes the thread is abandoned (harmless — the process exits at the
/// end of `execute_hook` regardless).
#[cfg(not(coverage))]
async fn read_hook_stdin() -> String {
    tokio::task::spawn_blocking(|| {
        use std::io::Read;
        let mut buf = String::new();
        let _ = std::io::stdin().read_to_string(&mut buf);
        buf
    })
    .await
    .unwrap_or_default()
}

/// No real stdin under coverage builds (nothing meaningful to read from the test
/// binary's own stdin).
#[cfg(coverage)]
async fn read_hook_stdin() -> String {
    String::new()
}

/// The `pulpo hook <harness>` entry point, called from `execute()`. Reads the real
/// hook payload off stdin (the one genuinely untestable step here — see
/// [`read_hook_stdin`]) and delegates everything else to [`execute_hook_with_stdin`],
/// which is what tests exercise directly.
pub async fn execute_hook(
    cli: &Cli,
    session_id: Option<&str>,
    harness: &str,
    event_name: Option<&str>,
) -> String {
    // Skip the (potentially blocking) stdin read entirely when there's no session to
    // report against — matches `execute_hook_with_stdin`'s own early return, but
    // avoids reading stdin for nothing first.
    if session_id.is_none() {
        return String::new();
    }
    let raw_stdin = read_hook_stdin().await;
    execute_hook_with_stdin(cli, session_id, harness, event_name, &raw_stdin).await
}

/// The testable core of [`execute_hook`]: everything past reading stdin. `session_id`
/// is `None` when `PULPO_SESSION_ID` is unset (the harness is running outside pulpo)
/// — in that case this returns immediately without making any network call. Always
/// returns from the caller's perspective (`execute` never propagates an error for
/// this command) and always exits 0.
pub async fn execute_hook_with_stdin(
    cli: &Cli,
    session_id: Option<&str>,
    harness: &str,
    event_name: Option<&str>,
    raw_stdin: &str,
) -> String {
    let Some(session_id) = session_id else {
        return String::new();
    };

    let client = reqwest::Client::new();
    let (resolved_node, peer_token) = resolve_node(&client, &cli.node).await;
    let base = base_url(&resolved_node);
    let token = resolve_token(&client, &base, &resolved_node, cli.token.as_deref())
        .await
        .or(peer_token);

    post_hook_event(
        &client,
        &base,
        token.as_deref(),
        session_id,
        harness,
        event_name,
        raw_stdin,
    )
    .await;

    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Commands;

    fn test_cli(node: String) -> Cli {
        Cli {
            node,
            token: None,
            command: Some(Commands::Hook {
                harness: "claude".into(),
                event: None,
            }),
            path: None,
        }
    }

    // -- parse_hook_event_json --

    #[test]
    fn test_parse_hook_event_json_empty_stdin_is_empty_object() {
        assert_eq!(parse_hook_event_json(""), serde_json::json!({}));
        assert_eq!(parse_hook_event_json("   \n  "), serde_json::json!({}));
    }

    #[test]
    fn test_parse_hook_event_json_valid_payload() {
        let raw = r#"{"hook_event_name":"Stop","session_id":"abc"}"#;
        let value = parse_hook_event_json(raw);
        assert_eq!(value["hook_event_name"], "Stop");
        assert_eq!(value["session_id"], "abc");
    }

    #[test]
    fn test_parse_hook_event_json_malformed_falls_back_to_empty_object() {
        assert_eq!(parse_hook_event_json("{not json"), serde_json::json!({}));
    }

    // -- build_hook_body --

    #[test]
    fn test_build_hook_body_wraps_harness_and_event() {
        let body = build_hook_body("claude", None, r#"{"hook_event_name":"Stop"}"#);
        assert_eq!(body["harness"], "claude");
        assert_eq!(body["event"]["hook_event_name"], "Stop");
    }

    #[test]
    fn test_build_hook_body_fills_event_name_when_missing() {
        let body = build_hook_body("claude", Some("SessionStart"), "{}");
        assert_eq!(body["event"]["hook_event_name"], "SessionStart");
    }

    #[test]
    fn test_build_hook_body_does_not_override_existing_event_name() {
        let body = build_hook_body(
            "claude",
            Some("SessionStart"),
            r#"{"hook_event_name":"Stop"}"#,
        );
        assert_eq!(body["event"]["hook_event_name"], "Stop");
    }

    #[test]
    fn test_build_hook_body_empty_stdin_and_no_event_name() {
        let body = build_hook_body("claude", None, "");
        assert_eq!(body["harness"], "claude");
        assert_eq!(body["event"], serde_json::json!({}));
    }

    // -- post_hook_event: never fails, regardless of what the server does --

    #[tokio::test]
    async fn test_post_hook_event_success() {
        use axum::{Router, http::StatusCode, routing::post};

        let received = std::sync::Arc::new(std::sync::Mutex::new(None::<serde_json::Value>));
        let received_clone = received.clone();
        let app = Router::new().route(
            "/api/v1/sessions/{id}/harness-events",
            post(move |body: axum::extract::Json<serde_json::Value>| {
                let received = received_clone.clone();
                async move {
                    *received.lock().unwrap() = Some(body.0);
                    StatusCode::NO_CONTENT
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let base = format!("http://127.0.0.1:{}", addr.port());
        let client = reqwest::Client::new();

        post_hook_event(
            &client,
            &base,
            None,
            "sess-1",
            "claude",
            None,
            r#"{"hook_event_name":"Stop"}"#,
        )
        .await;

        let body = received.lock().unwrap().clone().unwrap();
        assert_eq!(body["harness"], "claude");
        assert_eq!(body["event"]["hook_event_name"], "Stop");
    }

    #[tokio::test]
    async fn test_post_hook_event_server_error_is_swallowed() {
        use axum::{Router, http::StatusCode, routing::post};
        let app = Router::new().route(
            "/api/v1/sessions/{id}/harness-events",
            post(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let base = format!("http://127.0.0.1:{}", addr.port());
        let client = reqwest::Client::new();

        // Must not panic — errors are swallowed.
        post_hook_event(&client, &base, None, "sess-1", "claude", None, "{}").await;
    }

    #[tokio::test]
    async fn test_post_hook_event_unreachable_server_is_swallowed() {
        let client = reqwest::Client::new();
        post_hook_event(
            &client,
            "http://127.0.0.1:1",
            None,
            "sess-1",
            "claude",
            None,
            "{}",
        )
        .await;
    }

    // -- execute_hook_with_stdin (the testable core — never touches real stdin) --

    #[tokio::test]
    async fn test_execute_hook_no_session_id_is_noop() {
        // No network call is even attempted — an unreachable node would otherwise
        // hang/error, proving this short-circuits before touching the network.
        let cli = test_cli("127.0.0.1:1".into());
        let result = execute_hook_with_stdin(&cli, None, "claude", None, "{}").await;
        assert_eq!(result, "");
    }

    #[tokio::test]
    async fn test_execute_hook_posts_event_when_session_id_present() {
        use axum::{Router, http::StatusCode, routing::post};
        let received = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
        let received_clone = received.clone();
        let app = Router::new()
            .route(
                "/api/v1/sessions/{id}/harness-events",
                post(
                    move |axum::extract::Path(id): axum::extract::Path<String>| {
                        let received = received_clone.clone();
                        async move {
                            *received.lock().unwrap() = Some(id);
                            StatusCode::NO_CONTENT
                        }
                    },
                ),
            )
            .route(
                "/api/v1/auth/token",
                axum::routing::get(|| async { r#"{"token":""}"# }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let node = format!("127.0.0.1:{}", addr.port());
        let cli = test_cli(node);

        let result = execute_hook_with_stdin(&cli, Some("sess-42"), "claude", None, "{}").await;
        assert_eq!(result, "");
        assert_eq!(received.lock().unwrap().clone(), Some("sess-42".into()));
    }

    #[tokio::test]
    async fn test_execute_hook_daemon_unreachable_still_returns_empty() {
        let cli = test_cli("127.0.0.1:1".into());
        let result = execute_hook_with_stdin(&cli, Some("sess-1"), "claude", None, "{}").await;
        assert_eq!(result, "");
    }

    #[tokio::test]
    async fn test_execute_hook_short_circuits_before_reading_stdin() {
        // `execute_hook` (not `..._with_stdin`) is the one that would read real
        // stdin — assert the None-session_id path returns without doing so (this
        // would otherwise risk hanging if the test process's stdin is a live tty).
        let cli = test_cli("127.0.0.1:1".into());
        let result = execute_hook(&cli, None, "claude", None).await;
        assert_eq!(result, "");
    }
}
