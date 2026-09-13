use std::sync::Arc;

use axum::{Json, extract::State};
use pulpo_common::api::WatchdogConfigResponse;

use crate::api::error::ApiError;

/// Read-only view of pulpod's effective watchdog configuration. The config file
/// (`~/.pulpo/config.toml`) is the source of truth — editing happens by hand-editing
/// the file and restarting pulpod.
pub async fn get_watchdog(
    State(state): State<Arc<super::AppState>>,
) -> Result<Json<WatchdogConfigResponse>, ApiError> {
    let config = state.config.read().await;
    let resp = WatchdogConfigResponse {
        enabled: config.watchdog.enabled,
        check_interval_secs: config.watchdog.check_interval_secs,
        idle_timeout_secs: config.watchdog.idle_timeout_secs,
        idle_action: config.watchdog.idle_action.clone(),
        idle_threshold_secs: config.watchdog.idle_threshold_secs,
        extra_waiting_patterns: config.watchdog.waiting_patterns.clone(),
    };
    drop(config);
    Ok(Json(resp))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::test_support::test_state;

    #[tokio::test]
    async fn test_get_watchdog_returns_defaults() {
        let state = test_state().await;
        let Json(resp) = get_watchdog(State(state)).await.unwrap();
        assert!(resp.enabled);
        assert_eq!(resp.check_interval_secs, 10);
        assert_eq!(resp.idle_timeout_secs, 600);
        assert_eq!(resp.idle_action, "alert");
    }
}
