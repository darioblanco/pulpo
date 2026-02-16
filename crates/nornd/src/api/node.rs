use std::sync::Arc;

use axum::{Json, extract::State};
use norn_common::node::NodeInfo;

pub async fn get_info(State(state): State<Arc<super::AppState>>) -> Json<NodeInfo> {
    Json(NodeInfo {
        name: state.config.node.name.clone(),
        hostname: hostname::get()
            .map_or_else(|_| "unknown".into(), |h| h.to_string_lossy().into_owned()),
        os: crate::platform::os_name().into(),
        arch: std::env::consts::ARCH.into(),
        cpus: num_cpus::get(),
        memory_mb: 0, // TODO: get system memory
        gpu: None,    // TODO: detect GPU
    })
}
