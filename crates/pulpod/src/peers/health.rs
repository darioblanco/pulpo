use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use pulpo_common::node::NodeInfo;
use pulpo_common::peer::PeerStatus;
use pulpo_common::session::Session;
use tokio::sync::RwLock;
use tracing::{debug, warn};

use super::PeerRegistry;

/// Build a base URL from a peer address. Addresses that already include a scheme
/// (e.g., `https://machine.tailnet.ts.net`) are used as-is; bare `host:port`
/// addresses get `http://` prepended.
pub fn base_url(address: &str) -> String {
    if address.starts_with("http://") || address.starts_with("https://") {
        address.to_owned()
    } else {
        format!("http://{address}")
    }
}

/// Result of probing a peer node.
#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub node_info: NodeInfo,
    pub session_count: usize,
}

/// Trait for probing peer health. Enables testing with mocks.
pub trait PeerProber: Send + Sync {
    fn probe(
        &self,
        address: &str,
        token: Option<&str>,
    ) -> impl std::future::Future<Output = Result<ProbeResult>> + Send;
}

/// Production prober that uses HTTP requests to peer `/api/v1/node` and `/api/v1/sessions`.
pub struct HttpPeerProber {
    client: reqwest::Client,
}

impl Default for HttpPeerProber {
    fn default() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap_or_default(),
        }
    }
}

impl HttpPeerProber {
    pub fn new() -> Self {
        Self::default()
    }
}

impl HttpPeerProber {
    /// Build a GET request, attaching `Authorization: Bearer` when a token is present.
    fn authed_get(&self, url: String, token: Option<&str>) -> reqwest::RequestBuilder {
        let req = self.client.get(url);
        if let Some(t) = token {
            req.bearer_auth(t)
        } else {
            req
        }
    }
}

impl PeerProber for HttpPeerProber {
    async fn probe(&self, address: &str, token: Option<&str>) -> Result<ProbeResult> {
        let base = base_url(address);
        let node_info: NodeInfo = self
            .authed_get(format!("{base}/api/v1/node"), token)
            .send()
            .await?
            .json()
            .await?;

        let sessions: Vec<Session> = self
            .authed_get(format!("{base}/api/v1/sessions"), token)
            .send()
            .await?
            .json()
            .await?;

        Ok(ProbeResult {
            node_info,
            session_count: sessions.len(),
        })
    }
}

/// Cached result from a single peer probe.
struct CachedProbeResult {
    result: ProbeResult,
    fetched_at: Instant,
}

/// On-demand peer prober with a per-peer TTL cache.
///
/// Instead of running a background polling loop, `CachedProber` probes peers
/// lazily when `probe_all()` or `probe_peer()` is called and caches results
/// for a configurable TTL (default 60 s). Subsequent calls within the TTL
/// window return the cached value without hitting the network.
pub struct CachedProber<P: PeerProber> {
    prober: P,
    cache: Arc<RwLock<HashMap<String, CachedProbeResult>>>,
    ttl: Duration,
}

impl<P: PeerProber> CachedProber<P> {
    pub fn new(prober: P, ttl: Duration) -> Self {
        Self {
            prober,
            cache: Arc::new(RwLock::new(HashMap::new())),
            ttl,
        }
    }

    /// Probe a single peer, returning a cached result if still fresh.
    pub async fn probe_peer(
        &self,
        name: &str,
        address: &str,
        token: Option<&str>,
    ) -> Option<ProbeResult> {
        // Check cache
        if let Some(cached) = self.cache.read().await.get(name)
            && cached.fetched_at.elapsed() < self.ttl
        {
            return Some(cached.result.clone());
        }
        // Probe and cache
        match self.prober.probe(address, token).await {
            Ok(result) => {
                self.cache.write().await.insert(
                    name.to_owned(),
                    CachedProbeResult {
                        result: result.clone(),
                        fetched_at: Instant::now(),
                    },
                );
                Some(result)
            }
            Err(e) => {
                warn!("Peer {name} at {address} is offline: {e}");
                None
            }
        }
    }

    /// Probe all peers in the registry concurrently, updating their statuses.
    pub async fn probe_all(&self, registry: &PeerRegistry) {
        let peers = registry.get_all().await;

        // Collect (name, address, token) tuples first, then probe concurrently.
        let mut tasks = Vec::with_capacity(peers.len());
        for peer in &peers {
            let token = registry.get_token(&peer.name).await;
            tasks.push((peer.name.clone(), peer.address.clone(), token));
        }

        let futures: Vec<_> = tasks
            .iter()
            .map(|(name, address, token)| self.probe_peer(name, address, token.as_deref()))
            .collect();

        let results = futures::future::join_all(futures).await;

        for ((name, address, _), result) in tasks.iter().zip(results) {
            match result {
                Some(probe) => {
                    debug!(
                        "Peer {} at {} is online ({} sessions)",
                        name, address, probe.session_count
                    );
                    registry
                        .update_status(
                            name,
                            PeerStatus::Online,
                            Some(probe.node_info),
                            Some(probe.session_count),
                        )
                        .await;
                }
                None => {
                    registry
                        .update_status(name, PeerStatus::Offline, None, None)
                        .await;
                }
            }
        }
    }
}

#[cfg(all(test, not(coverage)))]
mod tests {
    use super::*;
    use pulpo_common::peer::PeerEntry;
    struct MockProber {
        results: HashMap<String, Result<ProbeResult, String>>,
    }

    impl PeerProber for MockProber {
        async fn probe(&self, address: &str, _token: Option<&str>) -> Result<ProbeResult> {
            match self.results.get(address) {
                Some(Ok(result)) => Ok(result.clone()),
                Some(Err(msg)) => Err(anyhow::anyhow!("{msg}")),
                None => Err(anyhow::anyhow!("unknown address: {address}")),
            }
        }
    }

    fn make_node_info(name: &str) -> NodeInfo {
        NodeInfo {
            name: name.into(),
            hostname: "host".into(),
            os: "linux".into(),
            arch: "x86_64".into(),
            cpus: 4,
            memory_mb: 8192,
            gpu: None,
        }
    }

    // ---- CachedProber tests ----
    //
    // All CachedProber tests use MockProber exclusively so that there is a
    // single monomorphisation of `CachedProber<MockProber>`. This avoids
    // LLVM coverage instrumentation artifacts where cross-instantiation
    // region merging creates phantom "uncovered" lines.

    #[tokio::test]
    async fn test_cached_prober_caches_result() {
        let mut results = HashMap::new();
        results.insert(
            "10.0.0.1:7433".into(),
            Ok(ProbeResult {
                node_info: make_node_info("node-a"),
                session_count: 1,
            }),
        );
        let prober = MockProber { results };
        let cached = CachedProber::new(prober, Duration::from_secs(60));

        // First probe — hits the prober
        let result = cached.probe_peer("node-a", "10.0.0.1:7433", None).await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().session_count, 1);

        // Second probe within TTL — returns cached
        let result = cached.probe_peer("node-a", "10.0.0.1:7433", None).await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().session_count, 1);
    }

    #[tokio::test]
    async fn test_cached_prober_expires_cache() {
        let mut results = HashMap::new();
        results.insert(
            "10.0.0.1:7433".into(),
            Ok(ProbeResult {
                node_info: make_node_info("node-a"),
                session_count: 1,
            }),
        );
        let prober = MockProber { results };
        // Use a zero-duration TTL so cache expires immediately
        let cached = CachedProber::new(prober, Duration::ZERO);

        let result = cached.probe_peer("node-a", "10.0.0.1:7433", None).await;
        assert!(result.is_some());

        // TTL is zero, so cache is already stale — prober called again
        let result = cached.probe_peer("node-a", "10.0.0.1:7433", None).await;
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn test_cached_prober_probe_all_updates_registry() {
        let mut configured = HashMap::new();
        configured.insert("node-a".into(), PeerEntry::Simple("10.0.0.1:7433".into()));
        let registry = PeerRegistry::new(&configured);

        let mut results = HashMap::new();
        results.insert(
            "10.0.0.1:7433".into(),
            Ok(ProbeResult {
                node_info: make_node_info("node-a"),
                session_count: 3,
            }),
        );
        let prober = MockProber { results };
        let cached = CachedProber::new(prober, Duration::from_secs(60));

        cached.probe_all(&registry).await;

        let peer = registry.get("node-a").await.unwrap();
        assert_eq!(peer.status, PeerStatus::Online);
        assert_eq!(peer.session_count, Some(3));
        assert!(peer.node_info.is_some());
    }

    #[tokio::test]
    async fn test_cached_prober_probe_all_offline() {
        let mut configured = HashMap::new();
        configured.insert("node-b".into(), PeerEntry::Simple("10.0.0.2:7433".into()));
        let registry = PeerRegistry::new(&configured);

        let mut results = HashMap::new();
        results.insert("10.0.0.2:7433".into(), Err("connection refused".into()));
        let prober = MockProber { results };
        let cached = CachedProber::new(prober, Duration::from_secs(60));

        cached.probe_all(&registry).await;

        let peer = registry.get("node-b").await.unwrap();
        assert_eq!(peer.status, PeerStatus::Offline);
        assert!(peer.node_info.is_none());
        assert!(peer.session_count.is_none());
    }

    #[tokio::test]
    async fn test_cached_prober_probe_all_mixed() {
        let mut configured = HashMap::new();
        configured.insert("online".into(), PeerEntry::Simple("10.0.0.1:7433".into()));
        configured.insert("offline".into(), PeerEntry::Simple("10.0.0.2:7433".into()));
        let registry = PeerRegistry::new(&configured);

        let mut results = HashMap::new();
        results.insert(
            "10.0.0.1:7433".into(),
            Ok(ProbeResult {
                node_info: make_node_info("online"),
                session_count: 1,
            }),
        );
        results.insert("10.0.0.2:7433".into(), Err("timeout".into()));
        let prober = MockProber { results };
        let cached = CachedProber::new(prober, Duration::from_secs(60));

        cached.probe_all(&registry).await;

        let online = registry.get("online").await.unwrap();
        assert_eq!(online.status, PeerStatus::Online);
        assert_eq!(online.session_count, Some(1));

        let offline = registry.get("offline").await.unwrap();
        assert_eq!(offline.status, PeerStatus::Offline);
    }

    #[tokio::test]
    async fn test_cached_prober_probe_all_empty() {
        let registry = PeerRegistry::new(&HashMap::new());
        let prober = MockProber {
            results: HashMap::new(),
        };
        let cached = CachedProber::new(prober, Duration::from_secs(60));
        // Should not panic
        cached.probe_all(&registry).await;
    }

    #[tokio::test]
    async fn test_cached_prober_probe_peer_unknown_address() {
        let prober = MockProber {
            results: HashMap::new(), // no results for any address
        };
        let cached = CachedProber::new(prober, Duration::from_secs(60));

        let result = cached.probe_peer("mystery", "10.99.99.99:7433", None).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cached_prober_different_peers_independent() {
        let mut results = HashMap::new();
        results.insert(
            "10.0.0.1:7433".into(),
            Ok(ProbeResult {
                node_info: make_node_info("node-a"),
                session_count: 1,
            }),
        );
        results.insert(
            "10.0.0.2:7433".into(),
            Ok(ProbeResult {
                node_info: make_node_info("node-b"),
                session_count: 2,
            }),
        );
        let prober = MockProber { results };
        let cached = CachedProber::new(prober, Duration::from_secs(60));

        // Probe two different peers
        let r1 = cached.probe_peer("node-a", "10.0.0.1:7433", None).await;
        let r2 = cached.probe_peer("node-b", "10.0.0.2:7433", None).await;
        assert_eq!(r1.unwrap().session_count, 1);
        assert_eq!(r2.unwrap().session_count, 2);

        // Re-probe both — returns cached results
        let r1 = cached.probe_peer("node-a", "10.0.0.1:7433", None).await;
        let r2 = cached.probe_peer("node-b", "10.0.0.2:7433", None).await;
        assert_eq!(r1.unwrap().session_count, 1);
        assert_eq!(r2.unwrap().session_count, 2);
    }

    // ---- HttpPeerProber tests (kept from original) ----

    #[tokio::test]
    async fn test_http_peer_prober_new() {
        let prober = HttpPeerProber::new();
        // Verify construction doesn't panic
        let debug = format!("{:?}", prober.client);
        assert!(!debug.is_empty());
    }

    #[tokio::test]
    async fn test_http_peer_prober_default() {
        let prober = HttpPeerProber::default();
        let debug = format!("{:?}", prober.client);
        assert!(!debug.is_empty());
    }

    #[tokio::test]
    async fn test_http_peer_prober_probe_connection_refused() {
        let prober = HttpPeerProber::new();
        // Probe a non-existent address — should fail
        let result = prober.probe("127.0.0.1:1", None).await;
        assert!(result.is_err());
    }

    /// Integration test: start a real axum server and probe it.
    #[tokio::test]
    async fn test_http_peer_prober_probe_real_server() {
        use axum::{Router, routing::get};
        let node_json = serde_json::to_string(&make_node_info("test-peer")).unwrap();
        let sessions = vec![
            Session {
                id: uuid::Uuid::new_v4(),
                name: "s1".into(),
                workdir: "/tmp".into(),
                command: "echo test".into(),
                status: pulpo_common::session::SessionStatus::Active,
                ..Default::default()
            },
            Session {
                id: uuid::Uuid::new_v4(),
                name: "s2".into(),
                workdir: "/tmp".into(),
                command: "echo test".into(),
                status: pulpo_common::session::SessionStatus::Active,
                ..Default::default()
            },
        ];
        let sessions_json = serde_json::to_string(&sessions).unwrap();

        let app = Router::new()
            .route(
                "/api/v1/node",
                get(move || {
                    let body = node_json.clone();
                    async move { ([("content-type", "application/json")], body) }
                }),
            )
            .route(
                "/api/v1/sessions",
                get(move || {
                    let body = sessions_json.clone();
                    async move { ([("content-type", "application/json")], body) }
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(axum::serve(listener, app).into_future());

        let prober = HttpPeerProber::new();
        let result = prober
            .probe(&format!("127.0.0.1:{}", addr.port()), None)
            .await
            .unwrap();
        assert_eq!(result.node_info.name, "test-peer");
        assert_eq!(result.session_count, 2);
    }

    /// Test probe fails when node endpoint returns invalid JSON.
    #[tokio::test]
    async fn test_http_peer_prober_probe_invalid_node_json() {
        use axum::{Router, routing::get};

        let app = Router::new().route(
            "/api/v1/node",
            get(|| async { ([("content-type", "application/json")], "not json") }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(axum::serve(listener, app).into_future());

        let prober = HttpPeerProber::new();
        let result = prober
            .probe(&format!("127.0.0.1:{}", addr.port()), None)
            .await;
        assert!(result.is_err());
    }

    /// Test probe fails when sessions endpoint returns invalid JSON.
    #[tokio::test]
    async fn test_http_peer_prober_probe_invalid_sessions_json() {
        use axum::{Router, routing::get};

        let node_json = serde_json::to_string(&make_node_info("test-peer")).unwrap();
        let app = Router::new()
            .route(
                "/api/v1/node",
                get(move || {
                    let body = node_json.clone();
                    async move { ([("content-type", "application/json")], body) }
                }),
            )
            .route(
                "/api/v1/sessions",
                get(|| async { ([("content-type", "application/json")], "not json") }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(axum::serve(listener, app).into_future());

        let prober = HttpPeerProber::new();
        let result = prober
            .probe(&format!("127.0.0.1:{}", addr.port()), None)
            .await;
        assert!(result.is_err());
    }

    /// Test probe fails when sessions endpoint is missing (node works but sessions 404s).
    #[tokio::test]
    async fn test_http_peer_prober_probe_missing_sessions_endpoint() {
        use axum::{Router, routing::get};

        let node_json = serde_json::to_string(&make_node_info("test-peer")).unwrap();
        let app = Router::new().route(
            "/api/v1/node",
            get(move || {
                let body = node_json.clone();
                async move { ([("content-type", "application/json")], body) }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(axum::serve(listener, app).into_future());

        let prober = HttpPeerProber::new();
        let result = prober
            .probe(&format!("127.0.0.1:{}", addr.port()), None)
            .await;
        // 404 response body won't parse as Vec<Session>
        assert!(result.is_err());
    }

    /// Test that probe sends Authorization header when token is provided.
    #[tokio::test]
    async fn test_http_peer_prober_probe_with_token() {
        use axum::extract::Request;
        use axum::{Router, routing::get};
        use std::sync::Mutex;

        let captured_auth = Arc::new(Mutex::new(None::<String>));
        let auth_clone = captured_auth.clone();

        let node_json = serde_json::to_string(&make_node_info("tok-peer")).unwrap();
        let sessions: Vec<pulpo_common::session::Session> = vec![];
        let sessions_json = serde_json::to_string(&sessions).unwrap();

        let app = Router::new()
            .route(
                "/api/v1/node",
                get(move |req: Request| {
                    let auth = req
                        .headers()
                        .get("authorization")
                        .map(|v| v.to_str().unwrap().to_owned());
                    *auth_clone.lock().unwrap() = auth;
                    let body = node_json.clone();
                    async move { ([("content-type", "application/json")], body) }
                }),
            )
            .route(
                "/api/v1/sessions",
                get(move || {
                    let body = sessions_json.clone();
                    async move { ([("content-type", "application/json")], body) }
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(axum::serve(listener, app).into_future());

        let prober = HttpPeerProber::new();
        let result = prober
            .probe(&format!("127.0.0.1:{}", addr.port()), Some("my-token"))
            .await
            .unwrap();
        assert_eq!(result.node_info.name, "tok-peer");

        let auth = captured_auth.lock().unwrap().clone();
        assert_eq!(auth, Some("Bearer my-token".into()));
    }

    /// Test that probe does NOT send Authorization header when no token.
    #[tokio::test]
    async fn test_http_peer_prober_probe_without_token_no_header() {
        use axum::extract::Request;
        use axum::{Router, routing::get};
        use std::sync::Mutex;

        let has_auth = Arc::new(Mutex::new(true));
        let flag = has_auth.clone();

        let node_json = serde_json::to_string(&make_node_info("no-tok")).unwrap();
        let sessions: Vec<pulpo_common::session::Session> = vec![];
        let sessions_json = serde_json::to_string(&sessions).unwrap();

        let app = Router::new()
            .route(
                "/api/v1/node",
                get(move |req: Request| {
                    *flag.lock().unwrap() = req.headers().contains_key("authorization");
                    let body = node_json.clone();
                    async move { ([("content-type", "application/json")], body) }
                }),
            )
            .route(
                "/api/v1/sessions",
                get(move || {
                    let body = sessions_json.clone();
                    async move { ([("content-type", "application/json")], body) }
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(axum::serve(listener, app).into_future());

        let prober = HttpPeerProber::new();
        let _ = prober
            .probe(&format!("127.0.0.1:{}", addr.port()), None)
            .await
            .unwrap();

        assert!(!*has_auth.lock().unwrap());
    }

    /// Test that probe returns an error when the server shuts down between
    /// the node and sessions requests (exercises the `.send().await?` error
    /// path on the sessions request).
    #[tokio::test]
    async fn test_http_peer_prober_probe_sessions_send_error() {
        use axum::{Router, routing::get};
        use tokio::sync::oneshot;

        let node_json = serde_json::to_string(&make_node_info("gone-peer")).unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let shutdown_tx = Arc::new(std::sync::Mutex::new(Some(shutdown_tx)));

        let app = Router::new().route(
            "/api/v1/node",
            get(move || {
                // After serving the node response, trigger server shutdown
                // so the sessions request will find no server.
                let _ = shutdown_tx.lock().unwrap().take().map(|tx| tx.send(()));
                let body = node_json.clone();
                async move { ([("content-type", "application/json")], body) }
            }),
        );
        // No /api/v1/sessions route — server shuts down before it's needed

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    shutdown_rx.await.ok();
                })
                .await
                .unwrap();
        });

        let prober = HttpPeerProber::new();
        let result = prober
            .probe(&format!("127.0.0.1:{}", addr.port()), None)
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_probe_result_debug() {
        let result = ProbeResult {
            node_info: make_node_info("debug-test"),
            session_count: 5,
        };
        let debug = format!("{result:?}");
        assert!(debug.contains("debug-test"));
        assert!(debug.contains('5'));
    }

    #[test]
    fn test_probe_result_clone() {
        let result = ProbeResult {
            node_info: make_node_info("clone-test"),
            session_count: 2,
        };
        let cloned = result.clone();
        assert_eq!(cloned.session_count, result.session_count);
        assert_eq!(cloned.node_info.name, "clone-test");
    }

    // ---- base_url tests ----

    #[test]
    fn test_base_url_bare_host_port() {
        assert_eq!(base_url("10.0.0.1:7433"), "http://10.0.0.1:7433");
    }

    #[test]
    fn test_base_url_https_passthrough() {
        assert_eq!(
            base_url("https://machine.tailnet.ts.net"),
            "https://machine.tailnet.ts.net"
        );
    }

    #[test]
    fn test_base_url_http_passthrough() {
        assert_eq!(base_url("http://localhost:7433"), "http://localhost:7433");
    }

    #[test]
    fn test_base_url_hostname_no_scheme() {
        assert_eq!(base_url("myhost:8080"), "http://myhost:8080");
    }
}
