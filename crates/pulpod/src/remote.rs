use std::time::Duration;

use anyhow::Result;

use crate::peers::PeerRegistry;

#[derive(Debug, Clone)]
pub struct RemoteNodeTarget {
    pub node_name: String,
    pub base_url: String,
    pub token: Option<String>,
}

pub fn normalize_http_base(address: &str) -> String {
    if address.contains("://") {
        address.to_owned()
    } else {
        format!("http://{address}")
    }
}

pub fn apply_remote_auth(
    request: reqwest::RequestBuilder,
    token: Option<&str>,
) -> reqwest::RequestBuilder {
    if let Some(token) = token {
        request.bearer_auth(token)
    } else {
        request
    }
}

pub fn remote_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?)
}

pub async fn resolve_peer_target(
    peer_registry: &PeerRegistry,
    target_node: &str,
) -> Option<RemoteNodeTarget> {
    let peer = peer_registry.get(target_node).await?;
    let token = peer_registry.get_token(target_node).await;
    Some(RemoteNodeTarget {
        node_name: target_node.to_owned(),
        base_url: normalize_http_base(&peer.address),
        token,
    })
}
