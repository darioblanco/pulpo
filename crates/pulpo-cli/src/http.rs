//! HTTP plumbing for the `pulpo` CLI: authenticated requests, node/token
//! resolution, and friendly error mapping.
//!
//! Pure move from `lib.rs`, plus the shared [`request_text`] /
//! [`request_json`] / [`get_json`] helpers every subcommand funnels through
//! (so a stopped daemon always produces the friendly "is pulpod running?"
//! hint instead of a raw reqwest error).

use anyhow::Result;
use pulpo_common::api::AuthTokenResponse;
#[cfg(not(coverage))]
use pulpo_common::api::{ConfigResponse, PeersResponse};

/// Format the base URL from the node address.
pub fn base_url(node: &str) -> String {
    if node.starts_with("http://") || node.starts_with("https://") {
        node.to_string()
    } else {
        format!("http://{node}")
    }
}

/// Extract a clean error message from an API JSON response (or fall back to raw text).
fn api_error(text: &str) -> anyhow::Error {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| v["error"].as_str().map(String::from))
        .map_or_else(|| anyhow::anyhow!("{text}"), |msg| anyhow::anyhow!("{msg}"))
}

/// Return the response body text, or a clean error if the response was non-success.
pub async fn ok_or_api_error(resp: reqwest::Response) -> Result<String> {
    if resp.status().is_success() {
        Ok(resp.text().await?)
    } else {
        let text = resp.text().await?;
        Err(api_error(&text))
    }
}

/// Map a reqwest error to a user-friendly message.
pub fn friendly_error(err: &reqwest::Error, node: &str) -> anyhow::Error {
    if err.is_connect() {
        anyhow::anyhow!(
            "Could not connect to pulpod at {node}. Is the daemon running?\nStart it with: brew services start pulpo"
        )
    } else {
        anyhow::anyhow!("Network error connecting to {node}: {err}")
    }
}

/// Check if the node address points to localhost.
pub fn is_localhost(node: &str) -> bool {
    let host = node.split(':').next().unwrap_or(node);
    host == "localhost" || host == "127.0.0.1" || node.starts_with("[::1]") || node == "::1"
}

/// Try to auto-discover the auth token from a local daemon.
async fn discover_token(client: &reqwest::Client, base: &str) -> Option<String> {
    let resp = client
        .get(format!("{base}/api/v1/auth/token"))
        .send()
        .await
        .ok()?;
    let body: AuthTokenResponse = resp.json().await.ok()?;
    if body.token.is_empty() {
        None
    } else {
        Some(body.token)
    }
}

/// Resolve the auth token: use explicit `--token`, auto-discover from localhost, or `None`.
pub async fn resolve_token(
    client: &reqwest::Client,
    base: &str,
    node: &str,
    explicit: Option<&str>,
) -> Option<String> {
    if let Some(t) = explicit {
        return Some(t.to_owned());
    }
    if is_localhost(node) {
        return discover_token(client, base).await;
    }
    None
}

/// Check if a node string needs resolution (no port specified).
fn node_needs_resolution(node: &str) -> bool {
    !node.contains(':')
}

/// Resolve a node reference to a `host:port` address.
///
/// If `node` looks like `host:port` (contains `:`), return as-is with no peer token.
/// Otherwise, query the local daemon's peer registry for a matching name. If a matching
/// online peer is found, return its address and optionally its configured auth token
/// (from the config endpoint). Falls back to appending `:7433` if the peer is not found.
#[cfg(not(coverage))]
pub async fn resolve_node(client: &reqwest::Client, node: &str) -> (String, Option<String>) {
    // Already has port — use as-is
    if !node_needs_resolution(node) {
        return (node.to_owned(), None);
    }

    // Try to resolve via local daemon's peer registry
    let local_base = "http://localhost:7433";
    let mut resolved_address: Option<String> = None;

    if let Ok(resp) = client
        .get(format!("{local_base}/api/v1/peers"))
        .send()
        .await
        && let Ok(peers_resp) = resp.json::<PeersResponse>().await
    {
        for peer in &peers_resp.peers {
            if peer.name == node {
                resolved_address = Some(peer.address.clone());
                break;
            }
        }
    }

    let address = resolved_address.unwrap_or_else(|| format!("{node}:7433"));

    // Try to get the peer's auth token from the config endpoint
    let peer_token = if let Ok(resp) = client
        .get(format!("{local_base}/api/v1/config"))
        .send()
        .await
        && let Ok(config) = resp.json::<ConfigResponse>().await
        && let Some(entry) = config.peers.get(node)
    {
        entry.token().map(String::from)
    } else {
        None
    };

    (address, peer_token)
}

/// Coverage stub — no real HTTP resolution during coverage builds.
#[cfg(coverage)]
pub async fn resolve_node(_client: &reqwest::Client, node: &str) -> (String, Option<String>) {
    if node_needs_resolution(node) {
        (format!("{node}:7433"), None)
    } else {
        (node.to_owned(), None)
    }
}

/// Build an authenticated request for any HTTP method.
fn authed(
    client: &reqwest::Client,
    method: reqwest::Method,
    url: String,
    token: Option<&str>,
) -> reqwest::RequestBuilder {
    let req = client.request(method, url);
    if let Some(t) = token {
        req.bearer_auth(t)
    } else {
        req
    }
}

/// Build an authenticated GET request.
pub fn authed_get(
    client: &reqwest::Client,
    url: String,
    token: Option<&str>,
) -> reqwest::RequestBuilder {
    authed(client, reqwest::Method::GET, url, token)
}

/// Build an authenticated POST request.
pub fn authed_post(
    client: &reqwest::Client,
    url: String,
    token: Option<&str>,
) -> reqwest::RequestBuilder {
    authed(client, reqwest::Method::POST, url, token)
}

/// Send an authenticated request (optionally with a JSON body), mapping
/// connection failures to the friendly "is pulpod running?" hint and non-2xx
/// responses to clean API errors. Returns the response body text.
pub async fn request_text(
    client: &reqwest::Client,
    method: reqwest::Method,
    url: String,
    token: Option<&str>,
    node: &str,
    body: Option<&serde_json::Value>,
) -> Result<String> {
    let mut req = authed(client, method, url, token);
    if let Some(b) = body {
        req = req.json(b);
    }
    let resp = req.send().await.map_err(|e| friendly_error(&e, node))?;
    ok_or_api_error(resp).await
}

/// [`request_text`] + parse the response body as JSON into `T`.
pub async fn request_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    method: reqwest::Method,
    url: String,
    token: Option<&str>,
    node: &str,
    body: Option<&serde_json::Value>,
) -> Result<T> {
    let text = request_text(client, method, url, token, node, body).await?;
    Ok(serde_json::from_str(&text)?)
}

/// Authenticated GET returning parsed JSON — the most common CLI call shape.
pub async fn get_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    url: String,
    token: Option<&str>,
    node: &str,
) -> Result<T> {
    request_json(client, reqwest::Method::GET, url, token, node, None).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_url() {
        assert_eq!(base_url("localhost:7433"), "http://localhost:7433");
        assert_eq!(base_url("my-machine:9999"), "http://my-machine:9999");
        // Already has scheme — pass through unchanged
        assert_eq!(base_url("http://localhost:7433"), "http://localhost:7433");
        assert_eq!(
            base_url("https://pulpo.example.com"),
            "https://pulpo.example.com"
        );
    }

    #[tokio::test]
    async fn test_friendly_error_connect() {
        // Make a request to a closed port to get a connect error
        let err = reqwest::Client::new()
            .get("http://127.0.0.1:1")
            .send()
            .await
            .unwrap_err();
        let friendly = friendly_error(&err, "test-node:1");
        let msg = friendly.to_string();
        assert!(
            msg.contains("Could not connect"),
            "Expected connect message, got: {msg}"
        );
    }

    #[tokio::test]
    async fn test_friendly_error_other() {
        // A request to an invalid URL creates a builder error, not a connect error
        let err = reqwest::Client::new()
            .get("http://[::invalid::url")
            .send()
            .await
            .unwrap_err();
        let friendly = friendly_error(&err, "bad-host");
        let msg = friendly.to_string();
        assert!(
            msg.contains("Network error"),
            "Expected network error message, got: {msg}"
        );
        assert!(msg.contains("bad-host"));
    }

    #[test]
    fn test_is_localhost_variants() {
        assert!(is_localhost("localhost:7433"));
        assert!(is_localhost("127.0.0.1:7433"));
        assert!(is_localhost("[::1]:7433"));
        assert!(is_localhost("::1"));
        assert!(is_localhost("localhost"));
        assert!(!is_localhost("mac-mini:7433"));
        assert!(!is_localhost("192.168.1.100:7433"));
    }

    #[test]
    fn test_authed_get_with_token() {
        let client = reqwest::Client::new();
        let req = authed_get(&client, "http://h:1/api".into(), Some("tok"))
            .build()
            .unwrap();
        let auth = req
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(auth, "Bearer tok");
    }

    #[test]
    fn test_authed_get_without_token() {
        let client = reqwest::Client::new();
        let req = authed_get(&client, "http://h:1/api".into(), None)
            .build()
            .unwrap();
        assert!(req.headers().get("authorization").is_none());
    }

    #[test]
    fn test_authed_post_with_token() {
        let client = reqwest::Client::new();
        let req = authed_post(&client, "http://h:1/api".into(), Some("secret"))
            .build()
            .unwrap();
        let auth = req
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(auth, "Bearer secret");
    }

    #[test]
    fn test_authed_post_without_token() {
        let client = reqwest::Client::new();
        let req = authed_post(&client, "http://h:1/api".into(), None)
            .build()
            .unwrap();
        assert!(req.headers().get("authorization").is_none());
    }

    #[test]
    fn test_authed_delete_with_token() {
        let client = reqwest::Client::new();
        let req = authed(
            &client,
            reqwest::Method::DELETE,
            "http://h:1/api".into(),
            Some("del-tok"),
        )
        .build()
        .unwrap();
        assert_eq!(req.method(), reqwest::Method::DELETE);
        let auth = req
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(auth, "Bearer del-tok");
    }

    #[test]
    fn test_authed_delete_without_token() {
        let client = reqwest::Client::new();
        let req = authed(
            &client,
            reqwest::Method::DELETE,
            "http://h:1/api".into(),
            None,
        )
        .build()
        .unwrap();
        assert!(req.headers().get("authorization").is_none());
    }

    #[test]
    fn test_authed_put_with_token() {
        let client = reqwest::Client::new();
        let req = authed(
            &client,
            reqwest::Method::PUT,
            "http://h:1/api".into(),
            Some("put-tok"),
        )
        .build()
        .unwrap();
        assert_eq!(req.method(), reqwest::Method::PUT);
        let auth = req
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(auth, "Bearer put-tok");
    }

    #[tokio::test]
    async fn test_resolve_token_explicit() {
        let client = reqwest::Client::new();
        let token =
            resolve_token(&client, "http://localhost:1", "localhost:1", Some("my-tok")).await;
        assert_eq!(token, Some("my-tok".into()));
    }

    #[tokio::test]
    async fn test_resolve_token_remote_no_explicit() {
        let client = reqwest::Client::new();
        let token = resolve_token(&client, "http://remote:7433", "remote:7433", None).await;
        assert_eq!(token, None);
    }

    #[tokio::test]
    async fn test_resolve_token_localhost_auto_discover() {
        use axum::{Json, Router, routing::get};

        let app = Router::new().route(
            "/api/v1/auth/token",
            get(|| async {
                Json(AuthTokenResponse {
                    token: "discovered".into(),
                })
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

        let node = format!("localhost:{}", addr.port());
        let base = base_url(&node);
        let client = reqwest::Client::new();
        let token = resolve_token(&client, &base, &node, None).await;
        assert_eq!(token, Some("discovered".into()));
    }

    #[tokio::test]
    async fn test_discover_token_empty_returns_none() {
        use axum::{Json, Router, routing::get};

        let app = Router::new().route(
            "/api/v1/auth/token",
            get(|| async {
                Json(AuthTokenResponse {
                    token: String::new(),
                })
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

        let base = format!("http://127.0.0.1:{}", addr.port());
        let client = reqwest::Client::new();
        assert_eq!(discover_token(&client, &base).await, None);
    }

    #[tokio::test]
    async fn test_discover_token_unreachable_returns_none() {
        let client = reqwest::Client::new();
        assert_eq!(discover_token(&client, "http://127.0.0.1:1").await, None);
    }

    #[test]
    fn test_api_error_json() {
        let err = api_error("{\"error\":\"session not found: foo\"}");
        assert_eq!(err.to_string(), "session not found: foo");
    }

    #[test]
    fn test_api_error_plain_text() {
        let err = api_error("plain text error");
        assert_eq!(err.to_string(), "plain text error");
    }

    #[test]
    fn test_node_needs_resolution() {
        assert!(!node_needs_resolution("localhost:7433"));
        assert!(!node_needs_resolution("mac-mini:7433"));
        assert!(!node_needs_resolution("10.0.0.1:7433"));
        assert!(!node_needs_resolution("[::1]:7433"));
        assert!(node_needs_resolution("mac-mini"));
        assert!(node_needs_resolution("linux-server"));
        assert!(node_needs_resolution("localhost"));
    }

    #[tokio::test]
    async fn test_resolve_node_with_port() {
        let client = reqwest::Client::new();
        let (addr, token) = resolve_node(&client, "mac-mini:7433").await;
        assert_eq!(addr, "mac-mini:7433");
        assert!(token.is_none());
    }

    #[tokio::test]
    async fn test_resolve_node_fallback_appends_port() {
        // No local daemon running on localhost:7433, so peer lookup fails
        // and it falls back to appending :7433
        let client = reqwest::Client::new();
        let (addr, token) = resolve_node(&client, "unknown-host").await;
        assert_eq!(addr, "unknown-host:7433");
        assert!(token.is_none());
    }

    #[cfg(not(coverage))]
    #[tokio::test]
    async fn test_resolve_node_finds_peer() {
        use axum::{Router, routing::get};

        let app = Router::new()
            .route(
                "/api/v1/peers",
                get(|| async {
                    r#"{"local":{"name":"local","hostname":"h","os":"macos","arch":"arm64","cpus":8,"memory_mb":0,"gpu":null},"peers":[{"name":"mac-mini","address":"10.0.0.5:7433","status":"online","node_info":null,"session_count":2,"source":"configured"}]}"#.to_owned()
                }),
            )
            .route(
                "/api/v1/config",
                get(|| async {
                    r#"{"node":{"name":"local","port":7433,"data_dir":"/tmp","bind":"local","tag":null,"seed":null,"discovery_interval_secs":30},"auth":{},"peers":{"mac-mini":{"address":"10.0.0.5:7433","token":"peer-secret"}},"watchdog":{"enabled":true,"memory_threshold":90,"check_interval_secs":10,"breach_count":3,"idle_timeout_secs":600,"idle_action":"alert","idle_threshold_secs":60},"notifications":{"webhooks":[]}}"#.to_owned()
                }),
            );

        // Port 7433 may be in use; skip test if so
        let Ok(listener) = tokio::net::TcpListener::bind("127.0.0.1:7433").await else {
            return;
        };
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

        let client = reqwest::Client::new();
        let (addr, token) = resolve_node(&client, "mac-mini").await;
        assert_eq!(addr, "10.0.0.5:7433");
        assert_eq!(token, Some("peer-secret".into()));
    }

    #[cfg(not(coverage))]
    #[tokio::test]
    async fn test_resolve_node_peer_no_token() {
        use axum::{Router, routing::get};

        let app = Router::new()
            .route(
                "/api/v1/peers",
                get(|| async {
                    r#"{"local":{"name":"local","hostname":"h","os":"macos","arch":"arm64","cpus":8,"memory_mb":0,"gpu":null},"peers":[{"name":"test-peer","address":"10.0.0.9:7433","status":"online","node_info":null,"session_count":null,"source":"configured"}]}"#.to_owned()
                }),
            )
            .route(
                "/api/v1/config",
                get(|| async {
                    r#"{"node":{"name":"local","port":7433,"data_dir":"/tmp","bind":"local","tag":null,"seed":null,"discovery_interval_secs":30},"auth":{},"peers":{"test-peer":"10.0.0.9:7433"},"watchdog":{"enabled":true,"memory_threshold":90,"check_interval_secs":10,"breach_count":3,"idle_timeout_secs":600,"idle_action":"alert","idle_threshold_secs":60},"notifications":{"webhooks":[]}}"#.to_owned()
                }),
            );

        let Ok(listener) = tokio::net::TcpListener::bind("127.0.0.1:7433").await else {
            return; // Port in use, skip
        };
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

        let client = reqwest::Client::new();
        let (addr, token) = resolve_node(&client, "test-peer").await;
        assert_eq!(addr, "10.0.0.9:7433");
        assert!(token.is_none()); // Simple peer entry has no token
    }
}
