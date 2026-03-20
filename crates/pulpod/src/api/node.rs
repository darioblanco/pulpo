use std::sync::Arc;

use axum::{Json, extract::State};
use pulpo_common::node::NodeInfo;

pub fn get_hostname() -> String {
    let fallback = String::from("unknown");
    hostname::get().map_or(fallback, |h| h.to_string_lossy().into_owned())
}

/// Get system memory in megabytes.
///
/// On macOS, uses `sysctl hw.memsize`. On Linux, reads `/proc/meminfo`.
/// Returns 0 on error or unsupported platforms.
pub fn get_memory_mb() -> u64 {
    get_memory_mb_impl()
}

#[cfg(target_os = "macos")]
fn get_memory_mb_impl() -> u64 {
    std::process::Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()
        .and_then(|out| {
            String::from_utf8_lossy(&out.stdout)
                .trim()
                .parse::<u64>()
                .ok()
        })
        .map_or(0, |bytes| bytes / 1_048_576)
}

#[cfg(target_os = "linux")]
fn get_memory_mb_impl() -> u64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|content| {
            content.lines().find_map(|line| {
                line.strip_prefix("MemTotal:")
                    .and_then(|rest| rest.trim().strip_suffix("kB"))
                    .and_then(|kb_str| kb_str.trim().parse::<u64>().ok())
                    .map(|kb| kb / 1024)
            })
        })
        .unwrap_or(0)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
const fn get_memory_mb_impl() -> u64 {
    0
}

pub async fn get_info(State(state): State<Arc<super::AppState>>) -> Json<NodeInfo> {
    let config = state.config.read().await;
    Json(NodeInfo {
        name: config.node.name.clone(),
        hostname: get_hostname(),
        os: crate::platform::os_name().into(),
        arch: std::env::consts::ARCH.into(),
        cpus: num_cpus::get(),
        memory_mb: get_memory_mb(),
        gpu: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::AppState;
    use crate::backend::StubBackend;
    use std::collections::HashMap;

    use crate::config::{Config, NodeConfig};
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;

    async fn test_state() -> Arc<AppState> {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(StubBackend);
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
                auth: crate::config::AuthConfig::default(),
                peers: HashMap::new(),
                watchdog: crate::config::WatchdogConfig::default(),
                inks: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
                docker: crate::config::DockerConfig::default(),
            },
            manager,
            peer_registry,
            store,
        )
    }

    #[test]
    fn test_get_hostname() {
        let hostname = get_hostname();
        assert!(!hostname.is_empty());
    }

    #[test]
    fn test_get_memory_mb() {
        let mem = get_memory_mb();
        // On any real macOS or Linux machine, should be > 0
        assert!(mem > 0, "Expected positive memory, got: {mem}");
    }

    #[tokio::test]
    async fn test_get_info_returns_node_info() {
        let state = test_state().await;
        let Json(info) = get_info(State(state)).await;
        assert_eq!(info.name, "test-node");
        assert!(!info.hostname.is_empty());
        assert!(!info.os.is_empty());
        assert!(!info.arch.is_empty());
        assert!(info.cpus > 0);
        assert!(info.memory_mb > 0);
    }
}
