use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use pulpo_common::api::{ErrorResponse, SecretEntry, SecretListResponse, SetSecretRequest};

type ApiError = (StatusCode, Json<ErrorResponse>);

fn internal_error(msg: &str) -> ApiError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: msg.to_owned(),
        }),
    )
}

/// Validate that a secret name is a valid env var name: uppercase alphanumeric + underscores.
fn is_valid_secret_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit())
}

pub async fn list_secrets(
    State(state): State<Arc<super::AppState>>,
) -> Result<Json<SecretListResponse>, ApiError> {
    let names = state
        .store
        .list_secret_names()
        .await
        .map_err(|e| internal_error(&e.to_string()))?;
    let secrets = names
        .into_iter()
        .map(|(name, created_at)| SecretEntry { name, created_at })
        .collect();
    Ok(Json(SecretListResponse { secrets }))
}

pub async fn set_secret(
    State(state): State<Arc<super::AppState>>,
    Path(name): Path<String>,
    Json(req): Json<SetSecretRequest>,
) -> Result<StatusCode, ApiError> {
    if !is_valid_secret_name(&name) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid secret name: must be uppercase alphanumeric and underscores (env var format)".to_owned(),
            }),
        ));
    }
    let value = req.value.trim().to_owned();
    state
        .store
        .set_secret(&name, &value)
        .await
        .map_err(|e| internal_error(&e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_secret(
    State(state): State<Arc<super::AppState>>,
    Path(name): Path<String>,
) -> Result<StatusCode, ApiError> {
    let deleted = state
        .store
        .delete_secret(&name)
        .await
        .map_err(|e| internal_error(&e.to_string()))?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("secret not found: {name}"),
            }),
        ))
    }
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

    #[tokio::test]
    async fn test_list_secrets_empty() {
        let state = test_state().await;
        let Json(resp) = list_secrets(State(state)).await.unwrap();
        assert!(resp.secrets.is_empty());
    }

    #[tokio::test]
    async fn test_set_and_list_secrets() {
        let state = test_state().await;
        let req = SetSecretRequest {
            value: "my-token".into(),
        };
        let status = set_secret(State(state.clone()), Path("GITHUB_TOKEN".into()), Json(req))
            .await
            .unwrap();
        assert_eq!(status, StatusCode::NO_CONTENT);

        let Json(resp) = list_secrets(State(state)).await.unwrap();
        assert_eq!(resp.secrets.len(), 1);
        assert_eq!(resp.secrets[0].name, "GITHUB_TOKEN");
        assert!(!resp.secrets[0].created_at.is_empty());
    }

    #[tokio::test]
    async fn test_set_secret_trims_value() {
        let state = test_state().await;
        let req = SetSecretRequest {
            value: "  my-token  ".into(),
        };
        set_secret(State(state.clone()), Path("TOKEN".into()), Json(req))
            .await
            .unwrap();
        let value = state.store.get_secret("TOKEN").await.unwrap().unwrap();
        assert_eq!(value, "my-token");
    }

    #[tokio::test]
    async fn test_set_secret_invalid_name() {
        let state = test_state().await;
        let req = SetSecretRequest {
            value: "val".into(),
        };
        let result = set_secret(State(state.clone()), Path("invalid-name".into()), Json(req)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_set_secret_invalid_name_empty() {
        let state = test_state().await;
        let req = SetSecretRequest {
            value: "val".into(),
        };
        let result = set_secret(State(state), Path(String::new()), Json(req)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_set_secret_invalid_name_starts_with_digit() {
        let state = test_state().await;
        let req = SetSecretRequest {
            value: "val".into(),
        };
        let result = set_secret(State(state), Path("1TOKEN".into()), Json(req)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_delete_secret_found() {
        let state = test_state().await;
        state.store.set_secret("MY_KEY", "val").await.unwrap();
        let status = delete_secret(State(state), Path("MY_KEY".into()))
            .await
            .unwrap();
        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_delete_secret_not_found() {
        let state = test_state().await;
        let result = delete_secret(State(state), Path("NONEXISTENT".into())).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_internal_error_helper() {
        let (status, Json(err)) = internal_error("boom");
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(err.error, "boom");
    }

    #[test]
    fn test_is_valid_secret_name() {
        assert!(is_valid_secret_name("MY_TOKEN"));
        assert!(is_valid_secret_name("GITHUB_TOKEN"));
        assert!(is_valid_secret_name("A"));
        assert!(is_valid_secret_name("A1"));
        assert!(is_valid_secret_name("MY_TOKEN_2"));
        assert!(!is_valid_secret_name(""));
        assert!(!is_valid_secret_name("my_token"));
        assert!(!is_valid_secret_name("MY-TOKEN"));
        assert!(!is_valid_secret_name("1TOKEN"));
        assert!(!is_valid_secret_name("MY TOKEN"));
    }
}
