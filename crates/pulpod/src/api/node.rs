use std::sync::Arc;

use axum::{Json, extract::State};
use pulpo_common::node::NodeInfo;

use crate::watchdog::memory::{MemoryReader, SystemMemoryReader};

pub fn get_hostname() -> String {
    let fallback = String::from("unknown");
    hostname::get().map_or(fallback, |h| h.to_string_lossy().into_owned())
}

pub async fn get_info(State(state): State<Arc<super::AppState>>) -> Json<NodeInfo> {
    let config = state.config.read().await;
    let memory_mb = SystemMemoryReader
        .read_memory()
        .map(|s| s.total_mb)
        .unwrap_or(0);
    Json(NodeInfo {
        name: config.node.name.clone(),
        hostname: get_hostname(),
        os: crate::platform::os_name().into(),
        arch: std::env::consts::ARCH.into(),
        cpus: num_cpus::get(),
        memory_mb,
        gpu: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::test_support::test_state;

    #[test]
    fn test_get_hostname() {
        let hostname = get_hostname();
        assert!(!hostname.is_empty());
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
