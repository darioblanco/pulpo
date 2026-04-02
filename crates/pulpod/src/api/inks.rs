use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use pulpo_common::api::{ErrorResponse, InkConfigResponse};
use serde::Serialize;

use crate::config::InkConfig;

type ApiError = (StatusCode, Json<ErrorResponse>);

fn bad_request(msg: &str) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: msg.to_owned(),
        }),
    )
}

fn not_found_error(msg: &str) -> ApiError {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: msg.to_owned(),
        }),
    )
}

#[cfg_attr(coverage, allow(dead_code))]
fn internal_error(msg: &str) -> ApiError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: msg.to_owned(),
        }),
    )
}

#[derive(Serialize)]
pub struct InksResponse {
    pub inks: HashMap<String, InkConfig>,
}

pub async fn list(State(state): State<Arc<super::AppState>>) -> Json<InksResponse> {
    let inks = state.config.read().await.inks.clone();
    Json(InksResponse { inks })
}

pub async fn get(
    State(state): State<Arc<super::AppState>>,
    Path(name): Path<String>,
) -> Result<Json<InkConfigResponse>, ApiError> {
    let config = state.config.read().await;
    config.inks.get(&name).map_or_else(
        || Err(not_found_error(&format!("ink not found: {name}"))),
        |ink| Ok(Json(InkConfigResponse::from(ink))),
    )
}

pub async fn create(
    State(state): State<Arc<super::AppState>>,
    Path(name): Path<String>,
    Json(req): Json<InkConfigResponse>,
) -> Result<(StatusCode, Json<InkConfigResponse>), ApiError> {
    if name.is_empty() {
        return Err(bad_request("ink name must not be empty"));
    }
    let config_snapshot = {
        let mut config = state.config.write().await;
        if config.inks.contains_key(&name) {
            return Err((
                StatusCode::CONFLICT,
                Json(ErrorResponse {
                    error: format!("ink '{name}' already exists"),
                }),
            ));
        }
        config.inks.insert(name, InkConfig::from(&req));
        state.session_manager.set_inks(config.inks.clone());
        config.clone()
    };
    save_config(&state, &config_snapshot)?;
    Ok((StatusCode::CREATED, Json(req)))
}

pub async fn update(
    State(state): State<Arc<super::AppState>>,
    Path(name): Path<String>,
    Json(req): Json<InkConfigResponse>,
) -> Result<Json<InkConfigResponse>, ApiError> {
    let config_snapshot = {
        let mut config = state.config.write().await;
        if !config.inks.contains_key(&name) {
            return Err(not_found_error(&format!("ink not found: {name}")));
        }
        config.inks.insert(name, InkConfig::from(&req));
        state.session_manager.set_inks(config.inks.clone());
        config.clone()
    };
    save_config(&state, &config_snapshot)?;
    Ok(Json(req))
}

pub async fn delete(
    State(state): State<Arc<super::AppState>>,
    Path(name): Path<String>,
) -> Result<StatusCode, ApiError> {
    let config_snapshot = {
        let mut config = state.config.write().await;
        if config.inks.remove(&name).is_none() {
            return Err(not_found_error(&format!("ink not found: {name}")));
        }
        state.session_manager.set_inks(config.inks.clone());
        config.clone()
    };
    save_config(&state, &config_snapshot)?;
    Ok(StatusCode::NO_CONTENT)
}

impl From<&InkConfig> for InkConfigResponse {
    fn from(ink: &InkConfig) -> Self {
        Self {
            description: ink.description.clone(),
            command: ink.command.clone(),
            secrets: ink.secrets.clone(),
            runtime: ink.runtime.clone(),
        }
    }
}

impl From<&InkConfigResponse> for InkConfig {
    fn from(resp: &InkConfigResponse) -> Self {
        Self {
            description: resp.description.clone(),
            command: resp.command.clone(),
            secrets: resp.secrets.clone(),
            runtime: resp.runtime.clone(),
        }
    }
}

fn save_config(state: &super::AppState, config: &crate::config::Config) -> Result<(), ApiError> {
    if !state.config_path.as_os_str().is_empty() {
        crate::config::save(config, &state.config_path)
            .map_err(|e| internal_error(&e.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::AppState;
    use crate::backend::StubBackend;
    use crate::config::{Config, NodeConfig};
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;

    async fn test_state(inks: HashMap<String, InkConfig>) -> Arc<AppState> {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let manager = SessionManager::new(Arc::new(StubBackend), store.clone(), inks.clone(), None)
            .with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        AppState::new(
            Config {
                node: NodeConfig {
                    name: "test".into(),
                    port: 7433,
                    data_dir: tmpdir.path().to_str().unwrap().into(),
                    ..NodeConfig::default()
                },
                auth: crate::config::AuthConfig::default(),
                peers: HashMap::new(),
                watchdog: crate::config::WatchdogConfig::default(),
                inks,
                notifications: crate::config::NotificationsConfig::default(),
                docker: crate::config::DockerConfig::default(),
                controller: crate::config::ControllerConfig::default(),
            },
            manager,
            peer_registry,
            store,
        )
    }

    #[tokio::test]
    async fn test_list_inks_empty() {
        let state = test_state(HashMap::new()).await;
        let Json(response) = list(State(state)).await;
        assert!(response.inks.is_empty());
    }

    #[tokio::test]
    async fn test_list_inks_with_entries() {
        let mut inks = HashMap::new();
        inks.insert(
            "reviewer".into(),
            InkConfig {
                description: None,
                command: Some("Review code".into()),
                ..InkConfig::default()
            },
        );
        let state = test_state(inks).await;
        let Json(response) = list(State(state)).await;
        assert_eq!(response.inks.len(), 1);
        let reviewer = &response.inks["reviewer"];
        assert_eq!(reviewer.command, Some("Review code".into()));
    }

    #[test]
    fn test_inks_response_serialize() {
        let mut inks = HashMap::new();
        inks.insert(
            "coder".into(),
            InkConfig {
                description: None,
                command: None,
                ..InkConfig::default()
            },
        );
        let resp = InksResponse { inks };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("coder"));
    }

    #[tokio::test]
    async fn test_get_ink_found() {
        let mut inks = HashMap::new();
        inks.insert(
            "coder".into(),
            InkConfig {
                description: Some("A coder".into()),
                command: Some("claude -p 'code'".into()),
                secrets: vec!["GH_TOKEN".into()],
                runtime: Some("docker".into()),
            },
        );
        let state = test_state(inks).await;
        let result = get(State(state), Path("coder".into())).await;
        assert!(result.is_ok());
        let Json(resp) = result.unwrap();
        assert_eq!(resp.description, Some("A coder".into()));
        assert_eq!(resp.command, Some("claude -p 'code'".into()));
        assert_eq!(resp.secrets, vec!["GH_TOKEN".to_owned()]);
        assert_eq!(resp.runtime, Some("docker".into()));
    }

    #[tokio::test]
    async fn test_get_ink_not_found() {
        let state = test_state(HashMap::new()).await;
        let result = get(State(state), Path("nonexistent".into())).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_create_ink() {
        let state = test_state(HashMap::new()).await;
        let req = InkConfigResponse {
            description: Some("Test ink".into()),
            command: Some("echo hello".into()),
            secrets: vec![],
            runtime: None,
        };
        let result = create(State(state.clone()), Path("test-ink".into()), Json(req)).await;
        assert!(result.is_ok());
        let (status, _) = result.unwrap();
        assert_eq!(status, StatusCode::CREATED);

        // Verify it's in config
        assert!(state.config.read().await.inks.contains_key("test-ink"));
    }

    #[tokio::test]
    async fn test_create_ink_duplicate() {
        let mut inks = HashMap::new();
        inks.insert(
            "existing".into(),
            InkConfig {
                command: Some("echo".into()),
                ..InkConfig::default()
            },
        );
        let state = test_state(inks).await;
        let req = InkConfigResponse {
            description: None,
            command: Some("echo".into()),
            secrets: vec![],
            runtime: None,
        };
        let result = create(State(state), Path("existing".into()), Json(req)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_create_ink_empty_name() {
        let state = test_state(HashMap::new()).await;
        let req = InkConfigResponse {
            description: None,
            command: None,
            secrets: vec![],
            runtime: None,
        };
        let result = create(State(state), Path(String::new()), Json(req)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_update_ink() {
        let mut inks = HashMap::new();
        inks.insert(
            "coder".into(),
            InkConfig {
                command: Some("old cmd".into()),
                ..InkConfig::default()
            },
        );
        let state = test_state(inks).await;
        let req = InkConfigResponse {
            description: Some("Updated".into()),
            command: Some("new cmd".into()),
            secrets: vec!["SECRET_A".into()],
            runtime: Some("docker".into()),
        };
        let result = update(State(state.clone()), Path("coder".into()), Json(req)).await;
        assert!(result.is_ok());
        let Json(resp) = result.unwrap();
        assert_eq!(resp.command, Some("new cmd".into()));
        assert_eq!(resp.secrets, vec!["SECRET_A".to_owned()]);

        // Verify config updated
        let ink = state.config.read().await.inks["coder"].clone();
        assert_eq!(ink.command, Some("new cmd".into()));
        assert_eq!(ink.runtime, Some("docker".into()));
    }

    #[tokio::test]
    async fn test_update_ink_not_found() {
        let state = test_state(HashMap::new()).await;
        let req = InkConfigResponse {
            description: None,
            command: None,
            secrets: vec![],
            runtime: None,
        };
        let result = update(State(state), Path("nonexistent".into()), Json(req)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_delete_ink() {
        let mut inks = HashMap::new();
        inks.insert(
            "to-delete".into(),
            InkConfig {
                command: Some("echo".into()),
                ..InkConfig::default()
            },
        );
        let state = test_state(inks).await;
        let result = delete(State(state.clone()), Path("to-delete".into())).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), StatusCode::NO_CONTENT);

        // Verify removed from config
        assert!(!state.config.read().await.inks.contains_key("to-delete"));
    }

    #[tokio::test]
    async fn test_delete_ink_not_found() {
        let state = test_state(HashMap::new()).await;
        let result = delete(State(state), Path("nonexistent".into())).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_ink_to_response_roundtrip() {
        let ink = InkConfig {
            description: Some("desc".into()),
            command: Some("cmd".into()),
            secrets: vec!["S1".into()],
            runtime: Some("docker".into()),
        };
        let resp = InkConfigResponse::from(&ink);
        let back = InkConfig::from(&resp);
        assert_eq!(back.description, ink.description);
        assert_eq!(back.command, ink.command);
        assert_eq!(back.secrets, ink.secrets);
        assert_eq!(back.runtime, ink.runtime);
    }

    #[test]
    fn test_error_helpers() {
        let (status, Json(body)) = bad_request("bad");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body.error, "bad");

        let (status, Json(body)) = not_found_error("missing");
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body.error, "missing");

        let (status, Json(body)) = internal_error("boom");
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body.error, "boom");
    }

    #[tokio::test]
    async fn test_save_config_writes_to_disk() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let config_path = tmpdir.path().join("config.toml");
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
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
            controller: crate::config::ControllerConfig::default(),
        };
        let manager =
            SessionManager::new(Arc::new(StubBackend), store.clone(), HashMap::new(), None)
                .with_no_stale_grace();
        let peer_registry = PeerRegistry::new(&HashMap::new());
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let state = AppState::with_event_tx(
            config,
            config_path.clone(),
            manager,
            peer_registry,
            event_tx,
            store,
        );
        let config = state.config.read().await;
        let result = save_config(&state, &config);
        drop(config);
        assert!(result.is_ok());
        assert!(config_path.exists());
    }
}
