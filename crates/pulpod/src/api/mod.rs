pub mod auth;
pub mod config;
mod embed;
pub mod error;
pub mod events;
pub mod health;
pub mod inks;
pub mod metrics;
pub mod node;
pub mod notifications;
pub mod peers;
pub mod push;

pub mod routes;
pub mod schedules;
pub mod secrets;
pub mod sessions;
pub mod static_files;
#[cfg(test)]
pub(crate) mod test_support;
pub mod usage;
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
    /// The single construction core every public constructor funnels through.
    ///
    /// `enable_prober` controls the on-demand peer prober: `false` for the
    /// bare test constructor ([`AppState::new`]) so tests never make real HTTP
    /// probes, `true` for the production constructors. The field itself only
    /// exists in non-coverage builds.
    fn build(
        config: Config,
        config_path: PathBuf,
        session_manager: SessionManager,
        peer_registry: PeerRegistry,
        event_tx: broadcast::Sender<PulpoEvent>,
        watchdog_config_tx: Option<tokio::sync::watch::Sender<WatchdogRuntimeConfig>>,
        store: Store,
        enable_prober: bool,
    ) -> Arc<Self> {
        #[cfg(coverage)]
        let _ = enable_prober;
        Arc::new(Self {
            config: Arc::new(RwLock::new(config)),
            config_path,
            session_manager,
            peer_registry,
            store,
            #[cfg(not(coverage))]
            cached_prober: enable_prober.then(|| {
                crate::peers::health::CachedProber::new(
                    crate::peers::health::HttpPeerProber::new(),
                    std::time::Duration::from_secs(60),
                )
            }),
            event_tx,
            watchdog_config_tx,
        })
    }

    /// Minimal constructor (tests): empty config path, own event channel, no
    /// peer prober, no watchdog channel.
    pub fn new(
        config: Config,
        session_manager: SessionManager,
        peer_registry: PeerRegistry,
        store: Store,
    ) -> Arc<Self> {
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self::build(
            config,
            PathBuf::new(),
            session_manager,
            peer_registry,
            event_tx,
            None,
            store,
            false,
        )
    }

    /// [`AppState::with_all`] without a watchdog config channel.
    pub fn with_event_tx(
        config: Config,
        config_path: PathBuf,
        session_manager: SessionManager,
        peer_registry: PeerRegistry,
        event_tx: broadcast::Sender<PulpoEvent>,
        store: Store,
    ) -> Arc<Self> {
        Self::with_all(
            config,
            config_path,
            session_manager,
            peer_registry,
            event_tx,
            None,
            store,
        )
    }

    /// Full constructor with all optional fields (watchdog).
    pub fn with_all(
        config: Config,
        config_path: PathBuf,
        session_manager: SessionManager,
        peer_registry: PeerRegistry,
        event_tx: broadcast::Sender<PulpoEvent>,
        watchdog_config_tx: Option<tokio::sync::watch::Sender<WatchdogRuntimeConfig>>,
        store: Store,
    ) -> Arc<Self> {
        Self::build(
            config,
            config_path,
            session_manager,
            peer_registry,
            event_tx,
            watchdog_config_tx,
            store,
            true,
        )
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    routes::build(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::test_support;

    #[tokio::test]
    async fn test_app_state_new() {
        let (config, manager, peer_registry, store) = test_support::test_parts().await;
        let state = AppState::new(config, manager, peer_registry, store);
        assert_eq!(state.config.read().await.node.name, "test-node");
        assert!(state.config_path.as_os_str().is_empty());
    }

    #[tokio::test]
    async fn test_app_state_with_event_tx() {
        let (config, manager, peer_registry, store) = test_support::test_parts().await;
        let config_path = std::path::PathBuf::from("/nonexistent/config.toml");
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let state = AppState::with_event_tx(
            config,
            config_path.clone(),
            manager,
            peer_registry,
            event_tx,
            store,
        );
        assert_eq!(state.config.read().await.node.name, "test-node");
        assert_eq!(state.config_path, config_path);
    }

    #[tokio::test]
    async fn test_app_state_with_all_watchdog_tx() {
        let (config, manager, peer_registry, store) = test_support::test_parts().await;
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let initial = crate::watchdog::WatchdogRuntimeConfig {
            threshold: 90,
            interval: std::time::Duration::from_secs(10),
            breach_count: 3,
            idle: crate::watchdog::IdleConfig::default(),
            ready_ttl_secs: 0,
            adopt_tmux: true,
            extra_waiting_patterns: Vec::new(),
            burn: crate::watchdog::BurnConfig::default(),
        };
        let (config_tx, _config_rx) = tokio::sync::watch::channel(initial);
        let state = AppState::with_all(
            config,
            std::path::PathBuf::from("/nonexistent/config.toml"),
            manager,
            peer_registry,
            event_tx,
            Some(config_tx),
            store,
        );
        assert!(state.watchdog_config_tx.is_some());
    }

    #[tokio::test]
    async fn test_app_state_with_all_watchdog_tx_none() {
        let (config, manager, peer_registry, store) = test_support::test_parts().await;
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let state = AppState::with_all(
            config,
            std::path::PathBuf::from("/nonexistent/config.toml"),
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
        let state = test_support::test_state().await;
        let _router = router(state);
    }
}
