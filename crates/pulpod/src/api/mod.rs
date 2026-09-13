pub mod auth;
pub mod config;
mod embed;
pub mod error;
pub mod events;
pub mod health;
pub mod node;
pub mod notifications;

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

const EVENT_CHANNEL_CAPACITY: usize = 256;

pub struct AppState {
    pub config: Arc<RwLock<Config>>,
    pub config_path: PathBuf,
    pub session_manager: SessionManager,
    pub store: Store,
    pub event_tx: broadcast::Sender<PulpoEvent>,
}

impl AppState {
    /// The single construction core every public constructor funnels through.
    fn build(
        config: Config,
        config_path: PathBuf,
        session_manager: SessionManager,
        event_tx: broadcast::Sender<PulpoEvent>,
        store: Store,
    ) -> Arc<Self> {
        Arc::new(Self {
            config: Arc::new(RwLock::new(config)),
            config_path,
            session_manager,
            store,
            event_tx,
        })
    }

    /// Minimal constructor (tests): empty config path, own event channel.
    pub fn new(config: Config, session_manager: SessionManager, store: Store) -> Arc<Self> {
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self::build(config, PathBuf::new(), session_manager, event_tx, store)
    }

    /// Full constructor with an explicit config path and event channel.
    pub fn with_event_tx(
        config: Config,
        config_path: PathBuf,
        session_manager: SessionManager,
        event_tx: broadcast::Sender<PulpoEvent>,
        store: Store,
    ) -> Arc<Self> {
        Self::build(config, config_path, session_manager, event_tx, store)
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
    async fn test_router_builds() {
        let state = test_support::test_state().await;
        let _router = router(state);
    }
}
