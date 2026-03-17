use std::net::SocketAddr;
use std::sync::Arc;

use axum::Json;
use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use pulpo_common::api::{AuthTokenResponse, PairingUrlResponse};
use pulpo_common::auth::BindMode;

use super::AppState;

/// Extract token from `Authorization: Bearer <token>` header.
fn extract_bearer_token(req: &Request<Body>) -> Option<&str> {
    req.headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
}

/// Extract token from `?token=<token>` query parameter.
fn extract_query_token(req: &Request<Body>) -> Option<String> {
    req.uri().query().and_then(|q| {
        q.split('&')
            .find_map(|pair| pair.strip_prefix("token=").map(String::from))
    })
}

/// Check if a `SocketAddr` is a loopback address.
const fn is_loopback(addr: &SocketAddr) -> bool {
    addr.ip().is_loopback()
}

/// Auth middleware: enforces Bearer token when `bind = "public"`.
///
/// Exempt paths:
/// - `GET /api/v1/health` — monitoring, peer probing
/// - `/api/v1/auth/*` from loopback — local token retrieval
/// - Static files (fallback) — web UI assets must load
pub async fn require_auth(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let config = state.config.read().await;
    let bind_mode = config.node.bind;
    let expected_token = config.auth.token.clone();
    drop(config);

    // Local/container/tailscale bind → no auth needed (network isolation is the guard)
    if matches!(
        bind_mode,
        BindMode::Local | BindMode::Container | BindMode::Tailscale
    ) {
        return next.run(req).await;
    }

    let path = req.uri().path();

    // Health endpoint is always exempt
    if path == "/api/v1/health" {
        return next.run(req).await;
    }

    // Auth endpoints are exempt when called from loopback
    if path.starts_with("/api/v1/auth/") {
        let client_addr = req.extensions().get::<ConnectInfo<SocketAddr>>();
        let is_local = client_addr.is_none_or(|ConnectInfo(addr)| is_loopback(addr));
        if is_local {
            return next.run(req).await;
        }
    }

    // Non-API paths (static files) are exempt
    if !path.starts_with("/api/") {
        return next.run(req).await;
    }

    // Check Bearer token from header or query param
    let token = extract_bearer_token(&req)
        .map(String::from)
        .or_else(|| extract_query_token(&req));

    match token {
        Some(t) if t == expected_token => next.run(req).await,
        _ => StatusCode::UNAUTHORIZED.into_response(),
    }
}

/// `GET /api/v1/auth/token` — returns the auth token.
///
/// The auth middleware already exempts `/api/v1/auth/*` for loopback clients
/// when `bind = "public"`, so this endpoint is effectively localhost-only.
pub async fn get_token(State(state): State<Arc<AppState>>) -> Json<AuthTokenResponse> {
    let config = state.config.read().await;
    let token = config.auth.token.clone();
    drop(config);
    Json(AuthTokenResponse { token })
}

/// Resolve hostname, falling back to `"localhost"` on error.
fn resolve_hostname(result: std::io::Result<std::ffi::OsString>) -> String {
    result.map_or_else(|_| "localhost".into(), |h| h.to_string_lossy().into_owned())
}

/// `GET /api/v1/auth/pairing-url` — returns the URL for QR code pairing.
///
/// Protected by the same middleware exemption as `get_token`.
pub async fn get_pairing_url(State(state): State<Arc<AppState>>) -> Json<PairingUrlResponse> {
    let config = state.config.read().await;
    let token = config.auth.token.clone();
    let port = config.node.port;
    drop(config);

    let hostname = resolve_hostname(hostname::get());
    let url = format!("http://{hostname}:{port}/?token={token}");
    Json(PairingUrlResponse { url, token })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::http::Uri;
    use tower::ServiceExt;

    #[test]
    fn test_extract_bearer_token_valid() {
        let req = Request::builder()
            .header("authorization", "Bearer my-token")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_bearer_token(&req), Some("my-token"));
    }

    #[test]
    fn test_extract_bearer_token_missing() {
        let req = Request::builder().body(Body::empty()).unwrap();
        assert_eq!(extract_bearer_token(&req), None);
    }

    #[test]
    fn test_extract_bearer_token_wrong_scheme() {
        let req = Request::builder()
            .header("authorization", "Basic abc")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_bearer_token(&req), None);
    }

    #[test]
    fn test_extract_query_token_present() {
        let req = Request::builder()
            .uri("http://localhost/api?token=abc123")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_query_token(&req), Some("abc123".into()));
    }

    #[test]
    fn test_extract_query_token_with_other_params() {
        let req = Request::builder()
            .uri("http://localhost/api?foo=bar&token=secret&baz=1")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_query_token(&req), Some("secret".into()));
    }

    #[test]
    fn test_extract_query_token_missing() {
        let req = Request::builder()
            .uri("http://localhost/api?foo=bar")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_query_token(&req), None);
    }

    #[test]
    fn test_extract_query_token_no_query() {
        let req = Request::builder()
            .uri("http://localhost/api")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_query_token(&req), None);
    }

    #[test]
    fn test_is_loopback_ipv4() {
        let addr: SocketAddr = "127.0.0.1:7433".parse().unwrap();
        assert!(is_loopback(&addr));
    }

    #[test]
    fn test_is_loopback_ipv6() {
        let addr: SocketAddr = "[::1]:7433".parse().unwrap();
        assert!(is_loopback(&addr));
    }

    #[test]
    fn test_is_not_loopback() {
        let addr: SocketAddr = "192.168.1.100:7433".parse().unwrap();
        assert!(!is_loopback(&addr));
    }

    #[test]
    fn test_extract_bearer_token_empty_value() {
        let req = Request::builder()
            .header("authorization", "Bearer ")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_bearer_token(&req), Some(""));
    }

    #[test]
    fn test_extract_query_token_empty_value() {
        let req = Request::builder()
            .uri(Uri::from_static("http://localhost/api?token="))
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_query_token(&req), Some(String::new()));
    }

    #[test]
    fn test_resolve_hostname_ok() {
        let result = Ok(std::ffi::OsString::from("myhost"));
        assert_eq!(resolve_hostname(result), "myhost");
    }

    #[test]
    fn test_resolve_hostname_err() {
        let result: std::io::Result<std::ffi::OsString> = Err(std::io::Error::other("fail"));
        assert_eq!(resolve_hostname(result), "localhost");
    }

    // -- Middleware integration tests --

    use crate::config::{AuthConfig, Config, NodeConfig};
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;
    use std::collections::HashMap;

    struct StubBackend;

    impl crate::backend::Backend for StubBackend {
        fn create_session(&self, _: &str, _: &str, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn kill_session(&self, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn is_alive(&self, _: &str) -> anyhow::Result<bool> {
            Ok(true)
        }
        fn capture_output(&self, _: &str, _: usize) -> anyhow::Result<String> {
            Ok(String::new())
        }
        fn send_input(&self, _: &str, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn setup_logging(&self, _: &str, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    async fn make_state(bind: BindMode, token: &str) -> Arc<AppState> {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                bind,
                ..NodeConfig::default()
            },
            auth: AuthConfig {
                token: token.into(),
            },
            peers: HashMap::new(),
            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new()).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        AppState::new(config, manager, peer_registry, store)
    }

    /// Build a minimal router with the auth middleware and a pass-through handler.
    fn auth_router(state: Arc<AppState>) -> Router {
        Router::new()
            .fallback(|| async { StatusCode::OK })
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                require_auth,
            ))
            .with_state(state)
    }

    /// Call the middleware with a crafted request, optionally injecting `ConnectInfo`.
    async fn call_middleware(
        state: Arc<AppState>,
        mut req: Request<Body>,
        remote_addr: Option<SocketAddr>,
    ) -> Response {
        if let Some(addr) = remote_addr {
            req.extensions_mut().insert(ConnectInfo(addr));
        }
        let app = auth_router(state);
        app.oneshot(req).await.unwrap()
    }

    #[tokio::test]
    async fn test_middleware_local_bind_skips_auth() {
        let state = make_state(BindMode::Local, "tok").await;
        let req = Request::builder()
            .uri("/api/v1/sessions")
            .body(Body::empty())
            .unwrap();
        let resp = call_middleware(state, req, None).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_middleware_container_bind_skips_auth() {
        let state = make_state(BindMode::Container, "tok").await;
        let req = Request::builder()
            .uri("/api/v1/sessions")
            .body(Body::empty())
            .unwrap();
        let resp = call_middleware(state, req, None).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_middleware_public_health_exempt() {
        let state = make_state(BindMode::Public, "tok").await;
        let req = Request::builder()
            .uri("/api/v1/health")
            .body(Body::empty())
            .unwrap();
        let resp = call_middleware(state, req, None).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_middleware_public_auth_path_loopback() {
        let state = make_state(BindMode::Public, "tok").await;
        let req = Request::builder()
            .uri("/api/v1/auth/token")
            .body(Body::empty())
            .unwrap();
        let addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        let resp = call_middleware(state, req, Some(addr)).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_middleware_public_auth_path_remote_denied() {
        let state = make_state(BindMode::Public, "tok").await;
        let req = Request::builder()
            .uri("/api/v1/auth/token")
            .body(Body::empty())
            .unwrap();
        let addr: SocketAddr = "192.168.1.100:12345".parse().unwrap();
        let resp = call_middleware(state, req, Some(addr)).await;
        // Remote client hitting /api/v1/auth/* without token → 401
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_middleware_public_non_api_exempt() {
        let state = make_state(BindMode::Public, "tok").await;
        let req = Request::builder()
            .uri("/index.html")
            .body(Body::empty())
            .unwrap();
        let resp = call_middleware(state, req, None).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_middleware_public_api_no_token() {
        let state = make_state(BindMode::Public, "tok").await;
        let req = Request::builder()
            .uri("/api/v1/sessions")
            .body(Body::empty())
            .unwrap();
        let resp = call_middleware(state, req, None).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_middleware_public_api_correct_header() {
        let state = make_state(BindMode::Public, "my-secret").await;
        let req = Request::builder()
            .uri("/api/v1/sessions")
            .header("authorization", "Bearer my-secret")
            .body(Body::empty())
            .unwrap();
        let resp = call_middleware(state, req, None).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_middleware_public_api_correct_query_param() {
        let state = make_state(BindMode::Public, "qs-token").await;
        let req = Request::builder()
            .uri("/api/v1/sessions?token=qs-token")
            .body(Body::empty())
            .unwrap();
        let resp = call_middleware(state, req, None).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_middleware_public_api_wrong_token() {
        let state = make_state(BindMode::Public, "correct").await;
        let req = Request::builder()
            .uri("/api/v1/sessions")
            .header("authorization", "Bearer wrong")
            .body(Body::empty())
            .unwrap();
        let resp = call_middleware(state, req, None).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn test_stub_backend_methods() {
        use crate::backend::Backend;
        let b = StubBackend;
        assert!(b.create_session("n", "d", "c").is_ok());
        assert!(b.kill_session("n").is_ok());
        assert!(b.is_alive("n").unwrap());
        assert!(b.capture_output("n", 10).unwrap().is_empty());
        assert!(b.send_input("n", "t").is_ok());
        assert!(b.setup_logging("n", "p").is_ok());
    }
}
