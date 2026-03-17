pub mod auth;
pub mod config;
mod embed;
pub mod events;
pub mod health;
pub mod inks;
pub mod node;
pub mod notifications;
pub mod peers;
pub mod push;

pub mod routes;
pub mod sessions;
pub mod static_files;
pub mod watchdog;
pub mod ws;

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use pulpo_common::event::PulpoEvent;
use tokio::sync::{RwLock, broadcast};

use crate::config::Config;
use crate::peers::PeerRegistry;
use crate::session::manager::SessionManager;
use crate::store::Store;
use crate::watchdog::WatchdogRuntimeConfig;

const EVENT_CHANNEL_CAPACITY: usize = 256;

pub struct AppState {
    pub config: Arc<RwLock<Config>>,
    pub config_path: PathBuf,
    pub session_manager: SessionManager,
    pub peer_registry: PeerRegistry,
    pub store: Store,
    /// On-demand peer prober with TTL cache. Only present in production builds;
    /// excluded under coverage because the monomorphized
    /// `CachedProber<HttpPeerProber>` methods are never exercised (the handler
    /// call site is also gated) and would produce uncoverable lines.
    #[cfg(not(coverage))]
    pub cached_prober:
        Option<crate::peers::health::CachedProber<crate::peers::health::HttpPeerProber>>,
    pub event_tx: broadcast::Sender<PulpoEvent>,
    /// Watch channel sender for pushing watchdog config changes to the running loop.
    pub watchdog_config_tx: Option<tokio::sync::watch::Sender<WatchdogRuntimeConfig>>,
}

impl AppState {
    pub fn new(
        config: Config,
        session_manager: SessionManager,
        peer_registry: PeerRegistry,
        store: Store,
    ) -> Arc<Self> {
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Arc::new(Self {
            config: Arc::new(RwLock::new(config)),
            config_path: PathBuf::new(),
            session_manager,
            peer_registry,
            store,
            #[cfg(not(coverage))]
            cached_prober: None,
            event_tx,
            watchdog_config_tx: None,
        })
    }

    pub fn with_event_tx(
        config: Config,
        config_path: PathBuf,
        session_manager: SessionManager,
        peer_registry: PeerRegistry,
        event_tx: broadcast::Sender<PulpoEvent>,
        store: Store,
    ) -> Arc<Self> {
        Arc::new(Self {
            config: Arc::new(RwLock::new(config)),
            config_path,
            session_manager,
            peer_registry,
            store,
            #[cfg(not(coverage))]
            cached_prober: Some(crate::peers::health::CachedProber::new(
                crate::peers::health::HttpPeerProber::new(),
                std::time::Duration::from_secs(60),
            )),
            event_tx,
            watchdog_config_tx: None,
        })
    }

    pub fn with_watchdog_tx(
        config: Config,
        config_path: PathBuf,
        session_manager: SessionManager,
        peer_registry: PeerRegistry,
        event_tx: broadcast::Sender<PulpoEvent>,
        watchdog_config_tx: Option<tokio::sync::watch::Sender<WatchdogRuntimeConfig>>,
        store: Store,
    ) -> Arc<Self> {
        Arc::new(Self {
            config: Arc::new(RwLock::new(config)),
            config_path,
            session_manager,
            peer_registry,
            store,
            #[cfg(not(coverage))]
            cached_prober: Some(crate::peers::health::CachedProber::new(
                crate::peers::health::HttpPeerProber::new(),
                std::time::Duration::from_secs(60),
            )),
            event_tx,
            watchdog_config_tx,
        })
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    routes::build(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use crate::backend::Backend;
    use crate::config::{Config, NodeConfig};
    use crate::peers::PeerRegistry;
    use crate::store::Store;
    use anyhow::Result;

    struct StubBackend;

    impl Backend for StubBackend {
        fn create_session(&self, _: &str, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn kill_session(&self, _: &str) -> Result<()> {
            Ok(())
        }
        fn is_alive(&self, _: &str) -> Result<bool> {
            Ok(true)
        }
        fn capture_output(&self, _: &str, _: usize) -> Result<String> {
            Ok(String::new())
        }
        fn send_input(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn setup_logging(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_app_state_new() {
        let tmpdir = tempfile::tempdir().unwrap();
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig::default(),
            peers: HashMap::new(),

            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new()).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(config, manager, peer_registry, store);
        assert_eq!(state.config.read().await.node.name, "test");
        assert!(state.config_path.as_os_str().is_empty());
    }

    #[tokio::test]
    async fn test_app_state_with_event_tx() {
        let tmpdir = tempfile::tempdir().unwrap();
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig::default(),
            peers: HashMap::new(),

            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
        };
        let config_path = tmpdir.path().join("config.toml");
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new()).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let state = AppState::with_event_tx(
            config,
            config_path.clone(),
            manager,
            peer_registry,
            event_tx,
            store,
        );
        assert_eq!(state.config.read().await.node.name, "test");
        assert_eq!(state.config_path, config_path);
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

    #[tokio::test]
    async fn test_app_state_with_watchdog_tx() {
        let tmpdir = tempfile::tempdir().unwrap();
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig::default(),
            peers: HashMap::new(),

            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new()).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let initial = crate::watchdog::WatchdogRuntimeConfig {
            threshold: 90,
            interval: std::time::Duration::from_secs(10),
            breach_count: 3,
            idle: crate::watchdog::IdleConfig::default(),
            ready_ttl_secs: 0,
            adopt_tmux: true,
        };
        let (config_tx, _config_rx) = tokio::sync::watch::channel(initial);
        let state = AppState::with_watchdog_tx(
            config,
            tmpdir.path().join("config.toml"),
            manager,
            peer_registry,
            event_tx,
            Some(config_tx),
            store,
        );
        assert!(state.watchdog_config_tx.is_some());
    }

    #[tokio::test]
    async fn test_app_state_with_watchdog_tx_none() {
        let tmpdir = tempfile::tempdir().unwrap();
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig::default(),
            peers: HashMap::new(),

            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new()).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let state = AppState::with_watchdog_tx(
            config,
            tmpdir.path().join("config.toml"),
            manager,
            peer_registry,
            event_tx,
            None,
            store,
        );
        assert!(state.watchdog_config_tx.is_none());
    }

    #[tokio::test]
    async fn test_router_builds() {
        let tmpdir = tempfile::tempdir().unwrap();
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig::default(),
            peers: HashMap::new(),

            watchdog: crate::config::WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
        };
        let backend = Arc::new(StubBackend);
        let manager =
            SessionManager::new(backend, store.clone(), HashMap::new()).with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let state = AppState::new(config, manager, peer_registry, store);
        let _router = router(state);
    }
}
