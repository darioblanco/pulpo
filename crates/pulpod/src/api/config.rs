use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode};
use pulpo_common::api::{
    AuthConfigResponse, ConfigResponse, DiscordWebhookConfigResponse, ErrorResponse,
    GuardDefaultConfigResponse, NodeConfigResponse, NotificationsConfigResponse,
    PersonaConfigResponse, UpdateConfigRequest, UpdateConfigResponse, WatchdogConfigResponse,
    WebhookEndpointConfigResponse,
};

type ApiError = (StatusCode, Json<ErrorResponse>);

fn internal_error(msg: &str) -> ApiError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: msg.to_owned(),
        }),
    )
}

fn config_to_response(config: &crate::config::Config) -> ConfigResponse {
    ConfigResponse {
        node: NodeConfigResponse {
            name: config.node.name.clone(),
            port: config.node.port,
            data_dir: config.node.data_dir.clone(),
            bind: config.node.bind,
            tag: config.node.tag.clone(),
            seed: config.node.seed.clone(),
            discovery_interval_secs: config.node.discovery_interval_secs,
        },
        auth: AuthConfigResponse {},
        peers: config.peers.clone(),
        guards: GuardDefaultConfigResponse {
            preset: config.guards.preset,
            max_turns: config.guards.max_turns,
            max_budget_usd: config.guards.max_budget_usd,
            output_format: config.guards.output_format.clone(),
        },
        watchdog: WatchdogConfigResponse {
            enabled: config.watchdog.enabled,
            memory_threshold: config.watchdog.memory_threshold,
            check_interval_secs: config.watchdog.check_interval_secs,
            breach_count: config.watchdog.breach_count,
            idle_timeout_secs: config.watchdog.idle_timeout_secs,
            idle_action: config.watchdog.idle_action.clone(),
        },
        notifications: NotificationsConfigResponse {
            discord: config
                .notifications
                .discord
                .as_ref()
                .map(|d| DiscordWebhookConfigResponse {
                    webhook_url: d.webhook_url.clone(),
                    events: d.events.clone(),
                }),
            webhooks: config
                .notifications
                .webhooks
                .iter()
                .map(|w| WebhookEndpointConfigResponse {
                    name: w.name.clone(),
                    url: w.url.clone(),
                    events: w.events.clone(),
                    has_secret: w.secret.is_some(),
                })
                .collect(),
        },
        personas: config
            .personas
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    PersonaConfigResponse {
                        provider: v.provider.clone(),
                        model: v.model.clone(),
                        mode: v.mode.clone(),
                        guard_preset: v.guard_preset.clone(),
                        allowed_tools: v.allowed_tools.clone(),
                        system_prompt: v.system_prompt.clone(),
                        max_turns: v.max_turns,
                        max_budget_usd: v.max_budget_usd,
                        output_format: v.output_format.clone(),
                    },
                )
            })
            .collect(),
    }
}

pub async fn get_config(
    State(state): State<Arc<super::AppState>>,
) -> Result<Json<ConfigResponse>, ApiError> {
    let config = state.config.read().await;
    let response = config_to_response(&config);
    drop(config);
    Ok(Json(response))
}

/// Apply an update request to the config, returning whether a restart is required.
fn apply_update(config: &mut crate::config::Config, req: UpdateConfigRequest) -> bool {
    let original_port = config.node.port;
    let original_bind = config.node.bind;
    let original_tag = config.node.tag.clone();
    let original_seed = config.node.seed.clone();

    // Node settings
    if let Some(name) = &req.node_name {
        config.node.name.clone_from(name);
    }
    if let Some(port) = req.port {
        config.node.port = port;
    }
    if let Some(data_dir) = &req.data_dir {
        config.node.data_dir.clone_from(data_dir);
    }
    if let Some(bind) = req.bind {
        config.node.bind = bind;
    }
    if let Some(tag) = req.tag {
        config.node.tag = if tag.is_empty() { None } else { Some(tag) };
    }
    if let Some(seed) = req.seed {
        config.node.seed = if seed.is_empty() { None } else { Some(seed) };
    }
    if let Some(interval) = req.discovery_interval_secs {
        config.node.discovery_interval_secs = interval;
    }

    // Guard defaults
    if let Some(preset) = req.guard_preset {
        config.guards.preset = preset;
    }
    if let Some(turns) = req.guard_max_turns {
        config.guards.max_turns = Some(turns);
    }
    if let Some(budget) = req.guard_max_budget_usd {
        config.guards.max_budget_usd = Some(budget);
    }
    if let Some(fmt) = req.guard_output_format {
        config.guards.output_format = if fmt.is_empty() { None } else { Some(fmt) };
    }

    // Watchdog
    if let Some(enabled) = req.watchdog_enabled {
        config.watchdog.enabled = enabled;
    }
    if let Some(threshold) = req.watchdog_memory_threshold {
        config.watchdog.memory_threshold = threshold;
    }
    if let Some(interval) = req.watchdog_check_interval_secs {
        config.watchdog.check_interval_secs = interval;
    }
    if let Some(count) = req.watchdog_breach_count {
        config.watchdog.breach_count = count;
    }
    if let Some(timeout) = req.watchdog_idle_timeout_secs {
        config.watchdog.idle_timeout_secs = timeout;
    }
    if let Some(action) = req.watchdog_idle_action {
        config.watchdog.idle_action = action;
    }

    // Notifications
    if let Some(url) = req.discord_webhook_url {
        if url.is_empty() {
            config.notifications.discord = None;
        } else {
            let events = req.discord_events.unwrap_or_default();
            config.notifications.discord = Some(crate::config::DiscordWebhookConfig {
                webhook_url: url,
                events,
            });
        }
    } else if let Some(events) = req.discord_events
        && let Some(discord) = &mut config.notifications.discord
    {
        discord.events = events;
    }

    // Generic webhooks (full replace when provided)
    if let Some(webhooks) = req.webhooks {
        config.notifications.webhooks = webhooks
            .into_iter()
            .map(|w| crate::config::WebhookEndpointConfig {
                name: w.name,
                url: w.url,
                events: w.events,
                secret: w.secret,
            })
            .collect();
    }

    // Personas (full replace when provided)
    if let Some(personas) = req.personas {
        config.personas = personas
            .into_iter()
            .map(|(k, v)| {
                (
                    k,
                    crate::config::PersonaConfig {
                        provider: v.provider,
                        model: v.model,
                        mode: v.mode,
                        guard_preset: v.guard_preset,
                        allowed_tools: v.allowed_tools,
                        system_prompt: v.system_prompt,
                        max_turns: v.max_turns,
                        max_budget_usd: v.max_budget_usd,
                        output_format: v.output_format,
                    },
                )
            })
            .collect();
    }

    // Peers
    if let Some(peers) = req.peers {
        config.peers = peers;
    }

    // Restart required for port, bind, tag, or seed changes (affects network/discovery loops)
    config.node.port != original_port
        || config.node.bind != original_bind
        || config.node.tag != original_tag
        || config.node.seed != original_seed
}

pub async fn update_config(
    State(state): State<Arc<super::AppState>>,
    Json(req): Json<UpdateConfigRequest>,
) -> Result<Json<UpdateConfigResponse>, ApiError> {
    let mut config = state.config.write().await;
    let restart_required = apply_update(&mut config, req);

    // Save to disk if config_path is set
    if !state.config_path.as_os_str().is_empty() {
        crate::config::save(&config, &state.config_path)
            .map_err(|e| internal_error(&e.to_string()))?;
    }

    let response = config_to_response(&config);
    drop(config);
    Ok(Json(UpdateConfigResponse {
        config: response,
        restart_required,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::AppState;
    use crate::backend::Backend;
    use std::collections::HashMap;

    use crate::config::{Config, GuardDefaultConfig, NodeConfig};
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;
    use anyhow::Result;
    use axum::extract::State;
    use pulpo_common::peer::PeerEntry;

    struct StubBackend;

    impl Backend for StubBackend {
        fn session_id(&self, name: &str) -> String {
            name.to_owned()
        }
        fn spawn_attach(&self, _: &str) -> anyhow::Result<tokio::process::Child> {
            anyhow::bail!("not supported in mock")
        }
        fn create_session(&self, _: &str, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn kill_session(&self, _: &str) -> Result<()> {
            Ok(())
        }
        fn is_alive(&self, _: &str) -> Result<bool> {
            Ok(true)
        }
        fn capture_output(&self, _: &str, _: usize) -> Result<String> {
            Ok(String::new())
        }
        fn send_input(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn setup_logging(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
    }

    async fn test_state() -> Arc<AppState> {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
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
                guards: GuardDefaultConfig::default(),
                watchdog: crate::config::WatchdogConfig::default(),
                personas: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
            },
            manager,
            peer_registry,
        )
    }

    async fn test_state_with_config_path() -> Arc<AppState> {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
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
                auth: crate::config::AuthConfig::default(),
                peers: HashMap::new(),
                guards: GuardDefaultConfig::default(),
                watchdog: crate::config::WatchdogConfig::default(),
                personas: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
            },
            config_path,
            manager,
            peer_registry,
            event_tx,
        )
    }

    #[test]
    fn test_stub_backend_methods() {
        use crate::backend::Backend;
        let b = StubBackend;
        assert!(b.create_session("n", "d", "c").is_ok());
        assert!(b.kill_session("n").is_ok());
        assert!(b.is_alive("n").unwrap());
        assert!(b.capture_output("n", 10).unwrap().is_empty());
        assert!(b.send_input("n", "t").is_ok());
        assert!(b.setup_logging("n", "p").is_ok());
    }

    #[tokio::test]
    async fn test_get_config_returns_current() {
        let state = test_state().await;
        let Json(resp) = get_config(State(state)).await.unwrap();
        assert_eq!(resp.node.name, "test-node");
        assert_eq!(resp.node.port, 7433);
        assert_eq!(
            resp.guards.preset,
            pulpo_common::guard::GuardPreset::Standard
        );
        assert!(resp.peers.is_empty());
    }

    #[tokio::test]
    async fn test_update_config_node_name() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            node_name: Some("new-name".into()),
            port: None,
            data_dir: None,
            bind: None,
            guard_preset: None,

            ..Default::default()
        };
        let Json(resp) = update_config(State(state.clone()), Json(req))
            .await
            .unwrap();
        assert_eq!(resp.config.node.name, "new-name");
        assert!(!resp.restart_required);

        // Verify persisted
        let Json(current) = get_config(State(state)).await.unwrap();
        assert_eq!(current.node.name, "new-name");
    }

    #[tokio::test]
    async fn test_update_config_port_requires_restart() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            node_name: None,
            port: Some(9999),
            data_dir: None,
            bind: None,
            guard_preset: None,

            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.config.node.port, 9999);
        assert!(resp.restart_required);
    }

    #[tokio::test]
    async fn test_update_config_same_port_no_restart() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            node_name: None,
            port: Some(7433),
            data_dir: None,
            bind: None,
            guard_preset: None,

            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert!(!resp.restart_required);
    }

    #[tokio::test]
    async fn test_update_config_guard_preset() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            node_name: None,
            port: None,
            data_dir: None,
            bind: None,
            guard_preset: Some(pulpo_common::guard::GuardPreset::Strict),

            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert_eq!(
            resp.config.guards.preset,
            pulpo_common::guard::GuardPreset::Strict
        );
    }

    #[tokio::test]
    async fn test_update_config_peers() {
        let state = test_state().await;
        let mut peers = HashMap::new();
        peers.insert("remote".into(), PeerEntry::Simple("10.0.0.1:7433".into()));
        let req = UpdateConfigRequest {
            peers: Some(peers),
            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.config.peers.len(), 1);
        assert_eq!(
            resp.config.peers["remote"],
            PeerEntry::Simple("10.0.0.1:7433".into())
        );
    }

    #[tokio::test]
    async fn test_update_config_data_dir() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            node_name: None,
            port: None,
            data_dir: Some("/new/data/dir".into()),
            bind: None,
            guard_preset: None,

            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.config.node.data_dir, "/new/data/dir");
    }

    #[tokio::test]
    async fn test_update_config_multiple_fields() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            node_name: Some("multi".into()),
            port: Some(8888),
            data_dir: None,
            bind: None,
            guard_preset: Some(pulpo_common::guard::GuardPreset::Unrestricted),

            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.config.node.name, "multi");
        assert_eq!(resp.config.node.port, 8888);
        assert_eq!(
            resp.config.guards.preset,
            pulpo_common::guard::GuardPreset::Unrestricted
        );
        assert!(resp.restart_required);
    }

    #[tokio::test]
    async fn test_update_config_saves_to_disk() {
        let state = test_state_with_config_path().await;
        let req = UpdateConfigRequest {
            node_name: Some("saved-node".into()),
            port: None,
            data_dir: None,
            bind: None,
            guard_preset: None,

            ..Default::default()
        };
        let Json(resp) = update_config(State(state.clone()), Json(req))
            .await
            .unwrap();
        assert_eq!(resp.config.node.name, "saved-node");

        // Verify file was written
        let content = std::fs::read_to_string(&state.config_path).unwrap();
        assert!(content.contains("saved-node"));
    }

    #[tokio::test]
    async fn test_update_config_save_roundtrip() {
        let state = test_state_with_config_path().await;
        let req = UpdateConfigRequest {
            node_name: Some("roundtrip".into()),
            port: Some(9000),
            data_dir: None,
            bind: None,
            guard_preset: Some(pulpo_common::guard::GuardPreset::Strict),

            ..Default::default()
        };
        let _ = update_config(State(state.clone()), Json(req))
            .await
            .unwrap();

        // Load back from disk
        let loaded = crate::config::load(state.config_path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.node.name, "roundtrip");
        assert_eq!(loaded.node.port, 9000);
        assert_eq!(
            loaded.guards.preset,
            pulpo_common::guard::GuardPreset::Strict
        );
    }

    #[tokio::test]
    async fn test_update_config_empty_request() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            node_name: None,
            port: None,
            data_dir: None,
            bind: None,
            guard_preset: None,

            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        // Nothing changed
        assert_eq!(resp.config.node.name, "test-node");
        assert_eq!(resp.config.node.port, 7433);
        assert!(!resp.restart_required);
    }

    #[test]
    fn test_config_to_response() {
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp".into(),
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig::default(),
            peers: HashMap::new(),
            guards: GuardDefaultConfig::default(),
            watchdog: crate::config::WatchdogConfig::default(),
            personas: HashMap::new(),
            notifications: crate::config::NotificationsConfig::default(),
        };
        let resp = config_to_response(&config);
        assert_eq!(resp.node.name, "test");
        assert_eq!(resp.node.port, 7433);
        assert_eq!(resp.node.bind, pulpo_common::auth::BindMode::Local);
    }

    #[tokio::test]
    async fn test_update_config_bind_requires_restart() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            node_name: None,
            port: None,
            data_dir: None,
            bind: Some(pulpo_common::auth::BindMode::Public),
            guard_preset: None,
            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.config.node.bind, pulpo_common::auth::BindMode::Public);
        assert!(resp.restart_required);
    }

    #[tokio::test]
    async fn test_update_config_same_bind_no_restart() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            node_name: None,
            port: None,
            data_dir: None,
            bind: Some(pulpo_common::auth::BindMode::Local),
            guard_preset: None,
            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert!(!resp.restart_required);
    }

    #[tokio::test]
    async fn test_get_config_returns_bind() {
        let state = test_state().await;
        let Json(resp) = get_config(State(state)).await.unwrap();
        assert_eq!(resp.node.bind, pulpo_common::auth::BindMode::Local);
    }

    #[test]
    fn test_internal_error() {
        let (status, Json(err)) = internal_error("boom");
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(err.error, "boom");
    }

    #[tokio::test]
    async fn test_config_response_debug() {
        let state = test_state().await;
        let Json(resp) = get_config(State(state)).await.unwrap();
        let debug = format!("{resp:?}");
        assert!(debug.contains("test-node"));
    }

    #[tokio::test]
    async fn test_update_config_tag_requires_restart() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            tag: Some("gpu".into()),
            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.config.node.tag, Some("gpu".into()));
        assert!(resp.restart_required);
    }

    #[tokio::test]
    async fn test_update_config_tag_empty_clears() {
        let state = test_state().await;
        // Set tag first
        let req = UpdateConfigRequest {
            tag: Some("gpu".into()),
            ..Default::default()
        };
        let _ = update_config(State(state.clone()), Json(req))
            .await
            .unwrap();
        // Clear it with empty string
        let req = UpdateConfigRequest {
            tag: Some(String::new()),
            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.config.node.tag, None);
    }

    #[tokio::test]
    async fn test_update_config_seed_requires_restart() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            seed: Some("10.0.0.1:7433".into()),
            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.config.node.seed, Some("10.0.0.1:7433".into()));
        assert!(resp.restart_required);
    }

    #[tokio::test]
    async fn test_update_config_seed_empty_clears() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            seed: Some("10.0.0.1:7433".into()),
            ..Default::default()
        };
        let _ = update_config(State(state.clone()), Json(req))
            .await
            .unwrap();
        let req = UpdateConfigRequest {
            seed: Some(String::new()),
            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.config.node.seed, None);
    }

    #[tokio::test]
    async fn test_update_config_discovery_interval() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            discovery_interval_secs: Some(120),
            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.config.node.discovery_interval_secs, 120);
        assert!(!resp.restart_required);
    }

    #[tokio::test]
    async fn test_update_config_guard_max_turns() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            guard_max_turns: Some(50),
            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.config.guards.max_turns, Some(50));
    }

    #[tokio::test]
    async fn test_update_config_guard_max_budget() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            guard_max_budget_usd: Some(10.0),
            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.config.guards.max_budget_usd, Some(10.0));
    }

    #[tokio::test]
    async fn test_update_config_guard_output_format() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            guard_output_format: Some("json".into()),
            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.config.guards.output_format, Some("json".into()));
    }

    #[tokio::test]
    async fn test_update_config_guard_output_format_empty_clears() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            guard_output_format: Some("json".into()),
            ..Default::default()
        };
        let _ = update_config(State(state.clone()), Json(req))
            .await
            .unwrap();
        let req = UpdateConfigRequest {
            guard_output_format: Some(String::new()),
            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.config.guards.output_format, None);
    }

    #[tokio::test]
    async fn test_update_config_watchdog() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            watchdog_enabled: Some(false),
            watchdog_memory_threshold: Some(90),
            watchdog_check_interval_secs: Some(120),
            watchdog_breach_count: Some(5),
            watchdog_idle_timeout_secs: Some(600),
            watchdog_idle_action: Some("kill".into()),
            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert!(!resp.config.watchdog.enabled);
        assert_eq!(resp.config.watchdog.memory_threshold, 90);
        assert_eq!(resp.config.watchdog.check_interval_secs, 120);
        assert_eq!(resp.config.watchdog.breach_count, 5);
        assert_eq!(resp.config.watchdog.idle_timeout_secs, 600);
        assert_eq!(resp.config.watchdog.idle_action, "kill");
        assert!(!resp.restart_required);
    }

    #[tokio::test]
    async fn test_update_config_discord_notifications() {
        let state = test_state().await;
        let req = UpdateConfigRequest {
            discord_webhook_url: Some("https://discord.com/api/webhooks/test".into()),
            discord_events: Some(vec!["session.created".into(), "session.completed".into()]),
            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        let discord = resp.config.notifications.discord.as_ref().unwrap();
        assert_eq!(discord.webhook_url, "https://discord.com/api/webhooks/test");
        assert_eq!(discord.events.len(), 2);
    }

    #[tokio::test]
    async fn test_update_config_discord_empty_url_clears() {
        let state = test_state().await;
        // Set discord first
        let req = UpdateConfigRequest {
            discord_webhook_url: Some("https://discord.com/api/webhooks/test".into()),
            ..Default::default()
        };
        let _ = update_config(State(state.clone()), Json(req))
            .await
            .unwrap();
        // Clear with empty URL
        let req = UpdateConfigRequest {
            discord_webhook_url: Some(String::new()),
            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert!(resp.config.notifications.discord.is_none());
    }

    #[tokio::test]
    async fn test_update_config_discord_events_only() {
        let state = test_state().await;
        // Set discord first
        let req = UpdateConfigRequest {
            discord_webhook_url: Some("https://discord.com/api/webhooks/test".into()),
            discord_events: Some(vec!["session.created".into()]),
            ..Default::default()
        };
        let _ = update_config(State(state.clone()), Json(req))
            .await
            .unwrap();
        // Update events only (no webhook_url)
        let req = UpdateConfigRequest {
            discord_events: Some(vec!["session.completed".into()]),
            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        let discord = resp.config.notifications.discord.as_ref().unwrap();
        assert_eq!(discord.events, vec!["session.completed"]);
        // URL unchanged
        assert_eq!(discord.webhook_url, "https://discord.com/api/webhooks/test");
    }

    #[tokio::test]
    async fn test_update_config_discord_events_no_existing_discord() {
        let state = test_state().await;
        // Update events only with no existing discord config — should be ignored
        let req = UpdateConfigRequest {
            discord_events: Some(vec!["session.completed".into()]),
            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert!(resp.config.notifications.discord.is_none());
    }

    #[tokio::test]
    async fn test_update_config_personas() {
        use pulpo_common::api::PersonaConfigResponse;
        let state = test_state().await;
        let mut personas = HashMap::new();
        personas.insert(
            "reviewer".into(),
            PersonaConfigResponse {
                provider: Some("claude".into()),
                model: Some("opus".into()),
                mode: Some("interactive".into()),
                guard_preset: Some("strict".into()),
                allowed_tools: Some(vec!["read".into()]),
                system_prompt: Some("You are a reviewer.".into()),
                max_turns: Some(10),
                max_budget_usd: Some(5.0),
                output_format: Some("json".into()),
            },
        );
        let req = UpdateConfigRequest {
            personas: Some(personas),
            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.config.personas.len(), 1);
        let p = &resp.config.personas["reviewer"];
        assert_eq!(p.provider, Some("claude".into()));
        assert_eq!(p.model, Some("opus".into()));
        assert_eq!(p.max_turns, Some(10));
    }

    #[test]
    fn test_config_to_response_with_notifications_and_personas() {
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp".into(),
                tag: Some("gpu".into()),
                seed: Some("10.0.0.1:7433".into()),
                discovery_interval_secs: 120,
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig::default(),
            peers: HashMap::new(),
            guards: GuardDefaultConfig {
                max_turns: Some(50),
                max_budget_usd: Some(10.0),
                output_format: Some("json".into()),
                ..GuardDefaultConfig::default()
            },
            watchdog: crate::config::WatchdogConfig {
                enabled: true,
                memory_threshold: 85,
                check_interval_secs: 30,
                breach_count: 3,
                idle_timeout_secs: 300,
                idle_action: "pause".into(),
            },
            personas: {
                let mut m = HashMap::new();
                m.insert(
                    "coder".into(),
                    crate::config::PersonaConfig {
                        provider: Some("claude".into()),
                        model: Some("sonnet".into()),
                        mode: None,
                        guard_preset: None,
                        allowed_tools: None,
                        system_prompt: None,
                        max_turns: None,
                        max_budget_usd: None,
                        output_format: None,
                    },
                );
                m
            },
            notifications: crate::config::NotificationsConfig {
                discord: Some(crate::config::DiscordWebhookConfig {
                    webhook_url: "https://discord.com/test".into(),
                    events: vec!["session.created".into()],
                }),
                webhooks: vec![],
            },
        };
        let resp = config_to_response(&config);
        // Node fields
        assert_eq!(resp.node.tag, Some("gpu".into()));
        assert_eq!(resp.node.seed, Some("10.0.0.1:7433".into()));
        assert_eq!(resp.node.discovery_interval_secs, 120);
        // Guard fields
        assert_eq!(resp.guards.max_turns, Some(50));
        assert_eq!(resp.guards.max_budget_usd, Some(10.0));
        assert_eq!(resp.guards.output_format, Some("json".into()));
        // Watchdog
        assert!(resp.watchdog.enabled);
        assert_eq!(resp.watchdog.memory_threshold, 85);
        assert_eq!(resp.watchdog.check_interval_secs, 30);
        assert_eq!(resp.watchdog.breach_count, 3);
        assert_eq!(resp.watchdog.idle_timeout_secs, 300);
        assert_eq!(resp.watchdog.idle_action, "pause");
        // Notifications
        let discord = resp.notifications.discord.as_ref().unwrap();
        assert_eq!(discord.webhook_url, "https://discord.com/test");
        assert_eq!(discord.events, vec!["session.created"]);
        // Personas
        assert_eq!(resp.personas.len(), 1);
        let p = &resp.personas["coder"];
        assert_eq!(p.provider, Some("claude".into()));
        assert_eq!(p.model, Some("sonnet".into()));
    }

    #[test]
    fn test_config_to_response_with_webhooks() {
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp".into(),
                ..NodeConfig::default()
            },
            auth: crate::config::AuthConfig::default(),
            peers: HashMap::new(),
            guards: GuardDefaultConfig::default(),
            watchdog: crate::config::WatchdogConfig::default(),
            personas: HashMap::new(),
            notifications: crate::config::NotificationsConfig {
                discord: None,
                webhooks: vec![
                    crate::config::WebhookEndpointConfig {
                        name: "ci-hook".into(),
                        url: "https://example.com/hook".into(),
                        events: vec!["completed".into(), "dead".into()],
                        secret: Some("s3cret".into()),
                    },
                    crate::config::WebhookEndpointConfig {
                        name: "logs-hook".into(),
                        url: "https://logs.example.com".into(),
                        events: vec![],
                        secret: None,
                    },
                ],
            },
        };
        let resp = config_to_response(&config);
        assert_eq!(resp.notifications.webhooks.len(), 2);
        let w0 = &resp.notifications.webhooks[0];
        assert_eq!(w0.name, "ci-hook");
        assert_eq!(w0.url, "https://example.com/hook");
        assert_eq!(w0.events, vec!["completed", "dead"]);
        assert!(w0.has_secret);
        let w1 = &resp.notifications.webhooks[1];
        assert_eq!(w1.name, "logs-hook");
        assert!(!w1.has_secret);
        assert!(w1.events.is_empty());
    }

    #[tokio::test]
    async fn test_update_config_webhooks() {
        use pulpo_common::api::WebhookEndpointUpdateRequest;
        let state = test_state().await;
        let req = UpdateConfigRequest {
            webhooks: Some(vec![WebhookEndpointUpdateRequest {
                name: "my-hook".into(),
                url: "https://example.com/webhook".into(),
                events: vec!["running".into()],
                secret: Some("key".into()),
            }]),
            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.config.notifications.webhooks.len(), 1);
        assert_eq!(resp.config.notifications.webhooks[0].name, "my-hook");
        assert_eq!(
            resp.config.notifications.webhooks[0].url,
            "https://example.com/webhook"
        );
        assert!(resp.config.notifications.webhooks[0].has_secret);
    }

    #[tokio::test]
    async fn test_update_config_webhooks_replaces_all() {
        use pulpo_common::api::WebhookEndpointUpdateRequest;
        let state = test_state().await;
        // Set initial webhooks
        let req = UpdateConfigRequest {
            webhooks: Some(vec![
                WebhookEndpointUpdateRequest {
                    name: "hook-1".into(),
                    url: "https://a.com".into(),
                    events: vec![],
                    secret: None,
                },
                WebhookEndpointUpdateRequest {
                    name: "hook-2".into(),
                    url: "https://b.com".into(),
                    events: vec![],
                    secret: None,
                },
            ]),
            ..Default::default()
        };
        let _ = update_config(State(state.clone()), Json(req))
            .await
            .unwrap();
        // Replace with single webhook
        let req = UpdateConfigRequest {
            webhooks: Some(vec![WebhookEndpointUpdateRequest {
                name: "hook-3".into(),
                url: "https://c.com".into(),
                events: vec!["dead".into()],
                secret: None,
            }]),
            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert_eq!(resp.config.notifications.webhooks.len(), 1);
        assert_eq!(resp.config.notifications.webhooks[0].name, "hook-3");
    }

    #[tokio::test]
    async fn test_update_config_webhooks_empty_clears() {
        use pulpo_common::api::WebhookEndpointUpdateRequest;
        let state = test_state().await;
        let req = UpdateConfigRequest {
            webhooks: Some(vec![WebhookEndpointUpdateRequest {
                name: "hook".into(),
                url: "https://a.com".into(),
                events: vec![],
                secret: None,
            }]),
            ..Default::default()
        };
        let _ = update_config(State(state.clone()), Json(req))
            .await
            .unwrap();
        // Clear
        let req = UpdateConfigRequest {
            webhooks: Some(vec![]),
            ..Default::default()
        };
        let Json(resp) = update_config(State(state), Json(req)).await.unwrap();
        assert!(resp.config.notifications.webhooks.is_empty());
    }

    #[tokio::test]
    async fn test_update_config_save_error() {
        // Use an invalid path that can't be written
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(StubBackend);
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
        let peer_registry = PeerRegistry::new(&HashMap::new());

        // Use /dev/null/impossible as config path (can't create dirs under /dev/null)
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let state = AppState::with_event_tx(
            Config {
                node: NodeConfig {
                    name: "test".into(),
                    port: 7433,
                    data_dir: tmpdir.path().to_str().unwrap().into(),
                    ..NodeConfig::default()
                },
                auth: crate::config::AuthConfig::default(),
                peers: HashMap::new(),
                guards: GuardDefaultConfig::default(),
                watchdog: crate::config::WatchdogConfig::default(),
                personas: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
            },
            std::path::PathBuf::from("/dev/null/impossible/config.toml"),
            manager,
            peer_registry,
            event_tx,
        );

        let req = UpdateConfigRequest {
            node_name: Some("fail".into()),
            port: None,
            data_dir: None,
            bind: None,
            guard_preset: None,

            ..Default::default()
        };
        let result = update_config(State(state), Json(req)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }
}
