pub mod auth;
pub mod config;
mod embed;
pub mod error;
pub mod events;
pub mod health;
pub mod metrics;
pub mod node;
pub mod notifications;
pub mod push;

pub mod routes;
pub mod schedules;
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
use crate::session::manager::SessionManager;
use crate::store::Store;
use crate::watchdog::WatchdogRuntimeConfig;

const EVENT_CHANNEL_CAPACITY: usize = 256;

pub struct AppState {
    pub config: Arc<RwLock<Config>>,
    pub config_path: PathBuf,
    pub session_manager: SessionManager,
    pub store: Store,
    pub event_tx: broadcast::Sender<PulpoEvent>,
    /// Watch channel sender for pushing watchdog config changes to the running loop.
    pub watchdog_config_tx: Option<tokio::sync::watch::Sender<WatchdogRuntimeConfig>>,
}

impl AppState {
    /// The single construction core every public constructor funnels through.
    fn build(
        config: Config,
        config_path: PathBuf,
        session_manager: SessionManager,
        event_tx: broadcast::Sender<PulpoEvent>,
        watchdog_config_tx: Option<tokio::sync::watch::Sender<WatchdogRuntimeConfig>>,
        store: Store,
    ) -> Arc<Self> {
        Arc::new(Self {
            config: Arc::new(RwLock::new(config)),
            config_path,
            session_manager,
            store,
            event_tx,
            watchdog_config_tx,
        })
    }

    /// Minimal constructor (tests): empty config path, own event channel, no watchdog channel.
    pub fn new(config: Config, session_manager: SessionManager, store: Store) -> Arc<Self> {
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self::build(
            config,
            PathBuf::new(),
            session_manager,
            event_tx,
            None,
            store,
        )
    }

    /// [`AppState::with_all`] without a watchdog config channel.
    pub fn with_event_tx(
        config: Config,
        config_path: PathBuf,
        session_manager: SessionManager,
        event_tx: broadcast::Sender<PulpoEvent>,
        store: Store,
    ) -> Arc<Self> {
        Self::with_all(config, config_path, session_manager, event_tx, None, store)
    }

    /// Full constructor with all optional fields (watchdog).
    pub fn with_all(
        config: Config,
        config_path: PathBuf,
        session_manager: SessionManager,
        event_tx: broadcast::Sender<PulpoEvent>,
        watchdog_config_tx: Option<tokio::sync::watch::Sender<WatchdogRuntimeConfig>>,
        store: Store,
    ) -> Arc<Self> {
        Self::build(
            config,
            config_path,
            session_manager,
            event_tx,
            watchdog_config_tx,
            store,
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
        let (config, manager, store) = test_support::test_parts().await;
        let state = AppState::new(config, manager, store);
        assert_eq!(state.config.read().await.node.name, "test-node");
        assert!(state.config_path.as_os_str().is_empty());
    }

    #[tokio::test]
    async fn test_app_state_with_event_tx() {
        let (config, manager, store) = test_support::test_parts().await;
        let config_path = std::path::PathBuf::from("/nonexistent/config.toml");
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let state = AppState::with_event_tx(config, config_path.clone(), manager, event_tx, store);
        assert_eq!(state.config.read().await.node.name, "test-node");
        assert_eq!(state.config_path, config_path);
    }

    #[tokio::test]
    async fn test_app_state_with_all_watchdog_tx() {
        let (config, manager, store) = test_support::test_parts().await;
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let initial = crate::watchdog::WatchdogRuntimeConfig {
            threshold: 90,
            interval: std::time::Duration::from_secs(10),
            breach_count: 3,
            idle: crate::watchdog::IdleConfig::default(),
            ready_ttl_secs: 0,
            extra_waiting_patterns: Vec::new(),
            burn: crate::watchdog::BurnConfig::default(),
        };
        let (config_tx, _config_rx) = tokio::sync::watch::channel(initial);
        let state = AppState::with_all(
            config,
            std::path::PathBuf::from("/nonexistent/config.toml"),
            manager,
            event_tx,
            Some(config_tx),
            store,
        );
        assert!(state.watchdog_config_tx.is_some());
    }

    #[tokio::test]
    async fn test_app_state_with_all_watchdog_tx_none() {
        let (config, manager, store) = test_support::test_parts().await;
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let state = AppState::with_all(
            config,
            std::path::PathBuf::from("/nonexistent/config.toml"),
            manager,
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
