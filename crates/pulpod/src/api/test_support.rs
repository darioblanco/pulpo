//! Shared test scaffolding for `crate::api` handler unit tests.
//!
//! Every handler test module used to hand-roll the same tmpdir → `Store` →
//! `StubBackend` → `SessionManager` → `PeerRegistry` → `AppState` builder. This
//! module centralizes it: [`test_state`] for the common case, [`test_state_with`]
//! for per-test `Config` tweaks (via a mutator closure, applied *before* the
//! `SessionManager`/`PeerRegistry` are built so ink/peer config takes effect),
//! and [`test_server`] / [`test_server_with`] for full-router `TestServer`
//! integration tests.
//!
//! Test-only: gated `#[cfg(test)]` in `api/mod.rs`.

use std::collections::HashMap;
use std::sync::Arc;

use axum_test::TestServer;

use crate::api::AppState;
use crate::backend::StubBackend;
use crate::config::{Config, NodeConfig};
use crate::peers::PeerRegistry;
use crate::session::manager::SessionManager;
use crate::store::Store;

/// The raw ingredients behind [`test_state`]/[`test_state_with`], for tests that
/// need to call an `AppState` constructor directly (e.g. exercising `AppState::new`
/// vs `AppState::with_event_tx` vs `AppState::with_all` themselves).
pub async fn test_parts() -> (Config, SessionManager, PeerRegistry, Store) {
    test_parts_with(|_| {}).await
}

/// Like [`test_parts`], but runs `mutate` on the default `Config` before the
/// `SessionManager` (ink map) and `PeerRegistry` (peers) are built from it, so a
/// mutator that sets `config.inks` or `config.peers` is reflected consistently
/// across all three.
pub async fn test_parts_with(
    mutate: impl FnOnce(&mut Config),
) -> (Config, SessionManager, PeerRegistry, Store) {
    let tmpdir = tempfile::tempdir().unwrap();
    let tmpdir = Box::leak(Box::new(tmpdir));
    let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
    store.migrate().await.unwrap();

    let mut config = Config {
        node: NodeConfig {
            name: "test-node".into(),
            port: 7433,
            data_dir: tmpdir.path().to_str().unwrap().into(),
            ..NodeConfig::default()
        },
        ..Default::default()
    };
    mutate(&mut config);

    let backend = Arc::new(StubBackend);
    let manager = SessionManager::new(backend, store.clone(), config.inks.clone(), None)
        .with_no_stale_grace();
    let peer_registry = PeerRegistry::new(&config.peers);
    (config, manager, peer_registry, store)
}

/// The canonical `AppState` test builder: a tempdir-backed `Store`, a
/// `StubBackend`-driven `SessionManager` with no stale grace period, an empty
/// `PeerRegistry`, and a minimal `test-node` `Config`.
pub async fn test_state() -> Arc<AppState> {
    test_state_with(|_| {}).await
}

/// [`test_state`] with a mutator applied to the `Config` before the
/// `SessionManager`/`PeerRegistry` are built (see [`test_parts_with`]).
pub async fn test_state_with(mutate: impl FnOnce(&mut Config)) -> Arc<AppState> {
    let (config, manager, peer_registry, store) = test_parts_with(mutate).await;
    AppState::new(config, manager, peer_registry, store)
}

/// [`test_state`], but built with `AppState::with_event_tx` and a real
/// `{tmpdir}/config.toml` path — for handlers that persist config to disk.
pub async fn test_state_with_config_path() -> Arc<AppState> {
    let tmpdir = tempfile::tempdir().unwrap();
    let tmpdir = Box::leak(Box::new(tmpdir));
    let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
    store.migrate().await.unwrap();
    let backend = Arc::new(StubBackend);
    let manager =
        SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
    let peer_registry = PeerRegistry::new(&HashMap::new());
    let config_path = tmpdir.path().join("config.toml");
    let (event_tx, _) = tokio::sync::broadcast::channel(16);
    AppState::with_event_tx(
        Config {
            node: NodeConfig {
                name: "test-node".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                ..NodeConfig::default()
            },
            ..Default::default()
        },
        config_path,
        manager,
        peer_registry,
        event_tx,
        store,
    )
}

/// A [`test_state`] backed by a caller-supplied backend, for tests that need
/// non-default `is_alive`/`kill_session`/etc behavior (dead sessions, failing
/// backends, ...).
pub async fn test_state_with_backend(backend: Arc<dyn crate::backend::Backend>) -> Arc<AppState> {
    let tmpdir = tempfile::tempdir().unwrap();
    let tmpdir = Box::leak(Box::new(tmpdir));
    let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
    store.migrate().await.unwrap();
    let manager =
        SessionManager::new(backend, store.clone(), HashMap::new(), None).with_no_stale_grace();
    let peer_registry = PeerRegistry::new(&HashMap::new());
    AppState::new(
        Config {
            node: NodeConfig {
                name: "test-node".into(),
                port: 7433,
                data_dir: tmpdir.path().to_str().unwrap().into(),
                ..NodeConfig::default()
            },
            ..Default::default()
        },
        manager,
        peer_registry,
        store,
    )
}

/// A full-router `TestServer` over [`test_state`].
pub async fn test_server() -> TestServer {
    TestServer::new(crate::api::routes::build(test_state().await)).unwrap()
}

/// A full-router `TestServer` over [`test_state_with`].
pub async fn test_server_with(mutate: impl FnOnce(&mut Config)) -> TestServer {
    TestServer::new(crate::api::routes::build(test_state_with(mutate).await)).unwrap()
}
