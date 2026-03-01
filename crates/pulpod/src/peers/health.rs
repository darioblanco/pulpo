use std::time::Duration;

use anyhow::Result;
use pulpo_common::node::NodeInfo;
use pulpo_common::peer::PeerStatus;
use pulpo_common::session::Session;
use tracing::{debug, warn};

use super::PeerRegistry;

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
                .timeout(Duration::from_secs(10))
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
        let base = format!("http://{address}");
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

/// Run a background health check loop that periodically probes all peers.
pub async fn run_health_check_loop<P: PeerProber>(
    registry: PeerRegistry,
    prober: P,
    interval: Duration,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    let mut tick = tokio::time::interval(interval);
    tick.tick().await; // first tick completes immediately

    loop {
        tokio::select! {
            _ = tick.tick() => {
                check_all_peers(&registry, &prober).await;
            }
            _ = shutdown_rx.changed() => {
                debug!("Peer health check loop shutting down");
                break;
            }
        }
    }
}

async fn check_all_peers<P: PeerProber>(registry: &PeerRegistry, prober: &P) {
    let peers = registry.get_all().await;
    for peer in peers {
        let token = registry.get_token(&peer.name).await;
        match prober.probe(&peer.address, token.as_deref()).await {
            Ok(result) => {
                debug!(
                    "Peer {} at {} is online ({} sessions)",
                    peer.name, peer.address, result.session_count
                );
                registry
                    .update_status(
                        &peer.name,
                        PeerStatus::Online,
                        Some(result.node_info),
                        Some(result.session_count),
                    )
                    .await;
            }
            Err(e) => {
                warn!("Peer {} at {} is offline: {e}", peer.name, peer.address);
                registry
                    .update_status(&peer.name, PeerStatus::Offline, None, None)
                    .await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

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

    #[tokio::test]
    async fn test_check_all_peers_online() {
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

        check_all_peers(&registry, &prober).await;

        let peer = registry.get("node-a").await.unwrap();
        assert_eq!(peer.status, PeerStatus::Online);
        assert_eq!(peer.session_count, Some(3));
        assert!(peer.node_info.is_some());
    }

    #[tokio::test]
    async fn test_check_all_peers_offline() {
        let mut configured = HashMap::new();
        configured.insert("node-b".into(), PeerEntry::Simple("10.0.0.2:7433".into()));
        let registry = PeerRegistry::new(&configured);

        let mut results = HashMap::new();
        results.insert("10.0.0.2:7433".into(), Err("connection refused".into()));
        let prober = MockProber { results };

        check_all_peers(&registry, &prober).await;

        let peer = registry.get("node-b").await.unwrap();
        assert_eq!(peer.status, PeerStatus::Offline);
        assert!(peer.node_info.is_none());
        assert!(peer.session_count.is_none());
    }

    #[tokio::test]
    async fn test_check_all_peers_mixed() {
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

        check_all_peers(&registry, &prober).await;

        let online = registry.get("online").await.unwrap();
        assert_eq!(online.status, PeerStatus::Online);
        assert_eq!(online.session_count, Some(1));

        let offline = registry.get("offline").await.unwrap();
        assert_eq!(offline.status, PeerStatus::Offline);
    }

    #[tokio::test]
    async fn test_check_all_peers_empty() {
        let registry = PeerRegistry::new(&HashMap::new());
        let prober = MockProber {
            results: HashMap::new(),
        };
        // Should not panic
        check_all_peers(&registry, &prober).await;
    }

    struct ArcCountingProber {
        count: Arc<AtomicUsize>,
    }

    impl PeerProber for ArcCountingProber {
        async fn probe(&self, _address: &str, _token: Option<&str>) -> Result<ProbeResult> {
            self.count.fetch_add(1, Ordering::SeqCst);
            Ok(ProbeResult {
                node_info: make_node_info("counter"),
                session_count: 0,
            })
        }
    }

    #[tokio::test]
    async fn test_check_all_peers_unknown_address() {
        // Exercise MockProber's None arm (unknown address)
        let mut configured = HashMap::new();
        configured.insert(
            "mystery".into(),
            PeerEntry::Simple("10.99.99.99:7433".into()),
        );
        let registry = PeerRegistry::new(&configured);

        let prober = MockProber {
            results: HashMap::new(), // no results for this address
        };
        check_all_peers(&registry, &prober).await;

        let peer = registry.get("mystery").await.unwrap();
        assert_eq!(peer.status, PeerStatus::Offline);
    }

    #[tokio::test]
    async fn test_health_check_loop_shutdown() {
        let mut configured = HashMap::new();
        configured.insert("node".into(), PeerEntry::Simple("10.0.0.1:7433".into()));
        let registry = PeerRegistry::new(&configured);

        let count = Arc::new(AtomicUsize::new(0));
        let prober = ArcCountingProber {
            count: count.clone(),
        };

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        tokio::time::pause();

        let handle = tokio::spawn(run_health_check_loop(
            registry.clone(),
            prober,
            Duration::from_secs(30),
            shutdown_rx,
        ));

        // Advance past the first tick interval to trigger a health check
        tokio::time::advance(Duration::from_secs(31)).await;
        tokio::task::yield_now().await;

        // Shutdown
        shutdown_tx.send(true).unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_health_check_loop_multiple_ticks() {
        let mut configured = HashMap::new();
        configured.insert("node".into(), PeerEntry::Simple("10.0.0.1:7433".into()));
        let registry = PeerRegistry::new(&configured);

        let count = Arc::new(AtomicUsize::new(0));
        let prober = ArcCountingProber {
            count: count.clone(),
        };
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        tokio::time::pause();

        let handle = tokio::spawn(run_health_check_loop(
            registry,
            prober,
            Duration::from_secs(30),
            shutdown_rx,
        ));

        // Advance through multiple intervals, yielding generously
        for _ in 0..3 {
            tokio::time::advance(Duration::from_secs(31)).await;
            // Multiple yields to let the spawned task process
            for _ in 0..5 {
                tokio::task::yield_now().await;
            }
        }

        shutdown_tx.send(true).unwrap();
        handle.await.unwrap();

        // At least 2 ticks should have fired (the exact count depends on
        // scheduling, but we should get several with generous yielding)
        assert!(count.load(Ordering::SeqCst) >= 2);
    }

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
        use chrono::Utc;

        let node_json = serde_json::to_string(&make_node_info("test-peer")).unwrap();
        let sessions = vec![
            Session {
                id: uuid::Uuid::new_v4(),
                name: "s1".into(),
                workdir: "/tmp".into(),
                provider: pulpo_common::session::Provider::Claude,
                prompt: "test".into(),
                status: pulpo_common::session::SessionStatus::Running,
                mode: pulpo_common::session::SessionMode::Interactive,
                conversation_id: None,
                exit_code: None,
                tmux_session: None,

                output_snapshot: None,
                git_branch: None,
                git_sha: None,
                guard_config: None,
                model: None,
                allowed_tools: None,
                system_prompt: None,
                metadata: None,
                persona: None,
                max_turns: None,
                max_budget_usd: None,
                output_format: None,
                intervention_reason: None,
                intervention_at: None,
                recovery_count: 0,
                last_output_at: None,
                idle_since: None,
                waiting_for_input: false,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
            Session {
                id: uuid::Uuid::new_v4(),
                name: "s2".into(),
                workdir: "/tmp".into(),
                provider: pulpo_common::session::Provider::Claude,
                prompt: "test".into(),
                status: pulpo_common::session::SessionStatus::Running,
                mode: pulpo_common::session::SessionMode::Interactive,
                conversation_id: None,
                exit_code: None,
                tmux_session: None,

                output_snapshot: None,
                git_branch: None,
                git_sha: None,
                guard_config: None,
                model: None,
                allowed_tools: None,
                system_prompt: None,
                metadata: None,
                persona: None,
                max_turns: None,
                max_budget_usd: None,
                output_format: None,
                intervention_reason: None,
                intervention_at: None,
                recovery_count: 0,
                last_output_at: None,
                idle_since: None,
                waiting_for_input: false,
                created_at: Utc::now(),
                updated_at: Utc::now(),
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
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

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
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

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
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

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
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

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
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

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
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

        let prober = HttpPeerProber::new();
        let _ = prober
            .probe(&format!("127.0.0.1:{}", addr.port()), None)
            .await
            .unwrap();

        assert!(!*has_auth.lock().unwrap());
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
}
