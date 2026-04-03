use std::sync::Arc;

use axum::{Json, http::StatusCode};
use pulpo_common::api::ErrorResponse;
use pulpo_common::peer::PeerInfo;
use serde::de::DeserializeOwned;

use crate::remote::{
    RemoteNodeTarget, apply_remote_auth, normalize_http_base, resolve_peer_target,
};

use super::AppState;

pub(super) type ApiError = (StatusCode, Json<ErrorResponse>);

#[derive(Debug, Clone)]
pub(super) struct RemoteSessionNodeTarget {
    pub session_id: String,
    pub node_name: String,
    pub base_url: String,
    pub token: Option<String>,
}

pub(super) fn internal_error(msg: &str) -> ApiError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: msg.to_owned(),
        }),
    )
}

pub(super) fn bad_gateway(msg: &str) -> ApiError {
    (
        StatusCode::BAD_GATEWAY,
        Json(ErrorResponse {
            error: msg.to_owned(),
        }),
    )
}

fn reqwest_status_to_axum(status: reqwest::StatusCode) -> StatusCode {
    StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY)
}

pub(super) async fn reqwest_error_response(resp: reqwest::Response, fallback: &str) -> ApiError {
    let status = reqwest_status_to_axum(resp.status());
    let error = match resp.json::<ErrorResponse>().await {
        Ok(body) => body.error,
        Err(_) => fallback.to_owned(),
    };
    (status, Json(ErrorResponse { error }))
}

pub(super) async fn send_remote_request(
    request: reqwest::RequestBuilder,
    failure: String,
) -> Result<reqwest::Response, ApiError> {
    request
        .send()
        .await
        .map_err(|e| bad_gateway(&format!("{failure}: {e}")))
}

pub(super) fn remote_json_request(
    target: &RemoteSessionNodeTarget,
    request: reqwest::RequestBuilder,
) -> reqwest::RequestBuilder {
    apply_remote_auth(request, target.token.as_deref())
}

pub(super) async fn parse_remote_json<T: DeserializeOwned>(
    resp: reqwest::Response,
    fallback: &str,
    parse_error: &str,
) -> Result<T, ApiError> {
    if !resp.status().is_success() {
        return Err(reqwest_error_response(resp, fallback).await);
    }

    resp.json::<T>()
        .await
        .map_err(|e| internal_error(&format!("{parse_error}: {e}")))
}

pub(super) async fn expect_remote_no_content(
    resp: reqwest::Response,
    fallback: &str,
) -> Result<StatusCode, ApiError> {
    if !resp.status().is_success() {
        return Err(reqwest_error_response(resp, fallback).await);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn resolve_remote_session_node_target(
    state: &Arc<AppState>,
    id: &str,
) -> Result<Option<RemoteSessionNodeTarget>, ApiError> {
    let Some(session_index) = &state.session_index else {
        return Ok(None);
    };
    let Some(entry) = session_index.get(id).await else {
        return Ok(None);
    };

    let peer: Option<PeerInfo> = state.peer_registry.get(&entry.node_name).await;
    let address = peer
        .as_ref()
        .map(|p| p.address.clone())
        .or_else(|| entry.node_address.clone());

    let Some(address) = address else {
        return Err(bad_gateway(&format!(
            "node address unknown for remote session {id} on node {}",
            entry.node_name
        )));
    };

    let token = state.peer_registry.get_token(&entry.node_name).await;
    Ok(Some(RemoteSessionNodeTarget {
        session_id: entry.session_id,
        node_name: entry.node_name,
        base_url: normalize_http_base(&address),
        token,
    }))
}

pub(super) async fn resolve_remote_node_target(
    state: &Arc<AppState>,
    target_node: &str,
) -> Result<Option<RemoteNodeTarget>, ApiError> {
    let local_name = state.config.read().await.node.name.clone();
    if target_node == local_name {
        return Ok(None);
    }

    let Some(target) = resolve_peer_target(&state.peer_registry, target_node).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("target node not found: {target_node}"),
            }),
        ));
    };
    Ok(Some(target))
}
