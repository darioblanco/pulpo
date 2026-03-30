use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::auth::BindMode;
use crate::node::NodeInfo;
use crate::peer::{PeerEntry, PeerInfo};
use crate::session::Session;

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub name: String,
    pub workdir: Option<String>,
    pub command: Option<String>,
    pub ink: Option<String>,
    pub description: Option<String>,
    pub metadata: Option<HashMap<String, String>>,
    pub idle_threshold_secs: Option<u32>,
    /// Create in an isolated git worktree.
    pub worktree: Option<bool>,
    /// Base branch to fork the worktree from (defaults to current HEAD).
    pub worktree_base: Option<String>,
    /// Runtime environment (tmux or docker). Defaults to tmux.
    pub runtime: Option<crate::session::Runtime>,
    /// Secret names to inject as environment variables.
    #[serde(default)]
    pub secrets: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateSessionResponse {
    pub session: Session,
}

#[derive(Debug, Deserialize)]
pub struct SendInputRequest {
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct OutputQuery {
    pub lines: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PeersResponse {
    pub local: NodeInfo,
    pub peers: Vec<PeerInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

/// WebSocket control messages (sent as JSON text frames).
/// Binary frames carry raw terminal I/O data.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsControl {
    Resize { cols: u16, rows: u16 },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigResponse {
    pub node: NodeConfigResponse,
    pub auth: AuthConfigResponse,
    pub peers: HashMap<String, PeerEntry>,
    pub watchdog: WatchdogConfigResponse,
    pub notifications: NotificationsConfigResponse,
    pub inks: HashMap<String, InkConfigResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthConfigResponse {}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthTokenResponse {
    pub token: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PairingUrlResponse {
    pub url: String,
    pub token: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NodeConfigResponse {
    pub name: String,
    pub port: u16,
    pub data_dir: String,
    pub bind: BindMode,
    pub tag: Option<String>,
    pub discovery_interval_secs: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WatchdogConfigResponse {
    pub enabled: bool,
    pub memory_threshold: u8,
    pub check_interval_secs: u64,
    pub breach_count: u32,
    pub idle_timeout_secs: u64,
    pub idle_action: String,
    #[serde(default)]
    pub ready_ttl_secs: u64,
    #[serde(default = "default_adopt_tmux")]
    pub adopt_tmux: bool,
    pub idle_threshold_secs: u64,
    #[serde(default)]
    pub extra_waiting_patterns: Vec<String>,
}

const fn default_adopt_tmux() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DiscordWebhookConfigResponse {
    pub webhook_url: String,
    pub events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookEndpointConfigResponse {
    pub name: String,
    pub url: String,
    pub events: Vec<String>,
    pub has_secret: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NotificationsConfigResponse {
    pub discord: Option<DiscordWebhookConfigResponse>,
    pub webhooks: Vec<WebhookEndpointConfigResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InkConfigResponse {
    pub description: Option<String>,
    pub command: Option<String>,
    /// Secret names to inject as environment variables.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secrets: Vec<String>,
    /// Runtime to use (tmux or docker).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebhookEndpointUpdateRequest {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub events: Vec<String>,
    pub secret: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct UpdateConfigRequest {
    // Node settings
    pub node_name: Option<String>,
    pub port: Option<u16>,
    pub data_dir: Option<String>,
    pub bind: Option<BindMode>,
    pub tag: Option<String>,
    pub discovery_interval_secs: Option<u64>,
    // Watchdog
    pub watchdog_enabled: Option<bool>,
    pub watchdog_memory_threshold: Option<u8>,
    pub watchdog_check_interval_secs: Option<u64>,
    pub watchdog_breach_count: Option<u32>,
    pub watchdog_idle_timeout_secs: Option<u64>,
    pub watchdog_idle_action: Option<String>,
    // Notifications — Discord
    pub discord_webhook_url: Option<String>,
    pub discord_events: Option<Vec<String>>,
    // Notifications — Generic webhooks (full replace when provided)
    pub webhooks: Option<Vec<WebhookEndpointUpdateRequest>>,
    // Inks (full replace)
    pub inks: Option<HashMap<String, InkConfigResponse>>,
    // Peers
    pub peers: Option<HashMap<String, PeerEntry>>,
}

#[derive(Debug, Serialize)]
pub struct UpdateConfigResponse {
    pub config: ConfigResponse,
    pub restart_required: bool,
}

/// Update request for watchdog settings. All fields optional — only provided fields change.
#[derive(Debug, Default, Deserialize)]
pub struct UpdateWatchdogRequest {
    pub enabled: Option<bool>,
    pub memory_threshold: Option<u8>,
    pub check_interval_secs: Option<u64>,
    pub breach_count: Option<u32>,
    pub idle_timeout_secs: Option<u64>,
    pub idle_action: Option<String>,
    pub ready_ttl_secs: Option<u64>,
    pub adopt_tmux: Option<bool>,
    pub idle_threshold_secs: Option<u64>,
    pub extra_waiting_patterns: Option<Vec<String>>,
}

/// Update request for notification settings.
#[derive(Debug, Default, Deserialize)]
pub struct UpdateNotificationsRequest {
    pub discord: Option<DiscordWebhookUpdateRequest>,
    pub webhooks: Option<Vec<WebhookEndpointUpdateRequest>>,
}

/// Discord webhook update — set url to empty string to remove.
#[derive(Debug, Clone, Deserialize)]
pub struct DiscordWebhookUpdateRequest {
    pub webhook_url: String,
    #[serde(default)]
    pub events: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AddPeerRequest {
    pub name: String,
    pub address: String,
}

// -- Web Push types --

#[derive(Debug, Serialize, Deserialize)]
pub struct VapidPublicKeyResponse {
    pub public_key: String,
}

#[derive(Debug, Deserialize)]
pub struct PushSubscriptionRequest {
    pub endpoint: String,
    pub keys: PushSubscriptionKeys,
}

#[derive(Debug, Deserialize)]
pub struct PushSubscriptionKeys {
    pub p256dh: String,
    pub auth: String,
}

#[derive(Debug, Deserialize)]
pub struct PushUnsubscribeRequest {
    pub endpoint: String,
}

use crate::session::InterventionCode;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InterventionEventResponse {
    pub id: i64,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<InterventionCode>,
    pub reason: String,
    pub created_at: String,
}

/// A session annotated with its source node for fleet-wide views.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetSession {
    /// The node this session belongs to.
    pub node_name: String,
    /// The node's address (empty for local).
    pub node_address: String,
    #[serde(flatten)]
    pub session: Session,
}

/// Response from the fleet sessions endpoint.
#[derive(Debug, Serialize, Deserialize)]
pub struct FleetSessionsResponse {
    pub sessions: Vec<FleetSession>,
}

/// A scheduled session spawn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub id: String,
    pub name: String,
    pub cron: String,
    pub command: String,
    pub workdir: String,
    /// Target node: None = local, Some("auto") = least-loaded, Some("name") = specific node.
    pub target_node: Option<String>,
    pub ink: Option<String>,
    pub description: Option<String>,
    /// Runtime environment (tmux or docker).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    /// Secret names to inject as environment variables.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secrets: Vec<String>,
    /// Create in an isolated git worktree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<bool>,
    /// Base branch to fork the worktree from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_base: Option<String>,
    pub enabled: bool,
    pub last_run_at: Option<String>,
    pub last_session_id: Option<String>,
    pub created_at: String,
}

/// Request to create or update a schedule.
#[derive(Debug, Deserialize)]
pub struct CreateScheduleRequest {
    pub name: String,
    pub cron: String,
    pub command: Option<String>,
    pub workdir: String,
    /// Target node: omit = local, "auto" = least-loaded, "name" = specific node.
    pub target_node: Option<String>,
    pub ink: Option<String>,
    pub description: Option<String>,
    /// Runtime environment (tmux or docker).
    pub runtime: Option<String>,
    /// Secret names to inject as environment variables.
    #[serde(default)]
    pub secrets: Option<Vec<String>>,
    /// Create in an isolated git worktree.
    pub worktree: Option<bool>,
    /// Base branch to fork the worktree from.
    pub worktree_base: Option<String>,
}

/// Request to update a schedule.
#[derive(Debug, Default, Deserialize)]
pub struct UpdateScheduleRequest {
    pub cron: Option<String>,
    pub command: Option<String>,
    pub workdir: Option<String>,
    pub target_node: Option<Option<String>>,
    pub ink: Option<Option<String>>,
    pub description: Option<Option<String>>,
    pub enabled: Option<bool>,
    /// Runtime environment (tmux or docker). Use `Some(None)` to clear.
    pub runtime: Option<Option<String>>,
    /// Secret names. Use `Some(vec![])` to clear.
    pub secrets: Option<Vec<String>>,
    /// Create in an isolated git worktree. Use `Some(None)` to clear.
    pub worktree: Option<Option<bool>>,
    /// Base branch to fork the worktree from. Use `Some(None)` to clear.
    pub worktree_base: Option<Option<String>>,
}

// -- Secret types --

/// Request body for PUT /api/v1/secrets/{name}
#[derive(Debug, Deserialize)]
pub struct SetSecretRequest {
    pub value: String,
    /// Optional env var name override. If set, the secret is exported as this
    /// environment variable instead of using the secret name.
    pub env: Option<String>,
}

/// Response for GET /api/v1/secrets
#[derive(Debug, Serialize, Deserialize)]
pub struct SecretListResponse {
    pub secrets: Vec<SecretEntry>,
}

/// A single secret entry (name + `created_at`, never includes the value).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretEntry {
    pub name: String,
    /// The env var name this secret maps to. `None` means the secret name is
    /// used directly as the env var.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct ListSessionsQuery {
    pub status: Option<String>,
    pub search: Option<String>,
    pub sort: Option<String>,
    pub order: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config_response() -> ConfigResponse {
        ConfigResponse {
            node: NodeConfigResponse {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp".into(),
                bind: BindMode::Local,
                tag: None,
                discovery_interval_secs: 30,
            },
            auth: AuthConfigResponse {},
            peers: HashMap::new(),
            watchdog: WatchdogConfigResponse {
                enabled: true,
                memory_threshold: 90,
                check_interval_secs: 10,
                breach_count: 3,
                idle_timeout_secs: 600,
                idle_action: "alert".into(),
                ready_ttl_secs: 0,
                adopt_tmux: true,
                idle_threshold_secs: 60,
                extra_waiting_patterns: vec![],
            },
            notifications: NotificationsConfigResponse {
                discord: None,
                webhooks: vec![],
            },
            inks: HashMap::new(),
        }
    }

    #[test]
    fn test_config_response_serialize() {
        let resp = test_config_response();
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"test\""));
        assert!(json.contains("7433"));
    }

    #[test]
    fn test_config_response_deserialize() {
        let json = r#"{"node":{"name":"n","port":1234,"data_dir":"/d","bind":"local","tag":null,"discovery_interval_secs":30},"auth":{},"peers":{},"watchdog":{"enabled":true,"memory_threshold":90,"check_interval_secs":10,"breach_count":3,"idle_timeout_secs":600,"idle_action":"alert","idle_threshold_secs":60},"notifications":{"discord":null,"webhooks":[]},"inks":{}}"#;
        let resp: ConfigResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.node.name, "n");
        assert_eq!(resp.node.port, 1234);
        assert_eq!(resp.node.bind, BindMode::Local);
        assert!(resp.watchdog.enabled);
        assert!(resp.notifications.discord.is_none());
        assert!(resp.inks.is_empty());
    }

    #[test]
    fn test_config_response_debug() {
        let resp = test_config_response();
        let debug = format!("{resp:?}");
        assert!(debug.contains("test"));
    }

    #[test]
    fn test_node_config_response_debug() {
        let resp = NodeConfigResponse {
            name: "test".into(),
            port: 7433,
            data_dir: "/tmp".into(),
            bind: BindMode::Local,
            tag: None,
            discovery_interval_secs: 30,
        };
        let debug = format!("{resp:?}");
        assert!(debug.contains("test"));
    }

    #[test]
    fn test_update_config_request_deserialize() {
        let json = r#"{"node_name":"new","port":9999}"#;
        let req: UpdateConfigRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.node_name, Some("new".into()));
        assert_eq!(req.port, Some(9999));
        assert!(req.data_dir.is_none());
        assert!(req.bind.is_none());
        assert!(req.peers.is_none());
    }

    #[test]
    fn test_update_config_request_empty() {
        let json = "{}";
        let req: UpdateConfigRequest = serde_json::from_str(json).unwrap();
        assert!(req.node_name.is_none());
        assert!(req.port.is_none());
    }

    #[test]
    fn test_update_config_request_debug() {
        let req = UpdateConfigRequest {
            node_name: Some("test".into()),
            ..Default::default()
        };
        let debug = format!("{req:?}");
        assert!(debug.contains("test"));
    }

    #[test]
    fn test_update_config_response_serialize() {
        let resp = UpdateConfigResponse {
            config: test_config_response(),
            restart_required: true,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"restart_required\":true"));
    }

    #[test]
    fn test_update_config_response_debug() {
        let resp = UpdateConfigResponse {
            config: test_config_response(),
            restart_required: false,
        };
        let debug = format!("{resp:?}");
        assert!(debug.contains("restart_required"));
    }

    #[test]
    fn test_config_response_with_peers() {
        let mut peers: HashMap<String, PeerEntry> = HashMap::new();
        peers.insert("remote".into(), PeerEntry::Simple("10.0.0.1:7433".into()));
        let mut resp = test_config_response();
        resp.peers = peers;
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("remote"));
    }

    #[test]
    fn test_update_config_request_with_all_fields() {
        let json = r#"{"node_name":"new","port":9999,"data_dir":"/d","bind":"public","peers":{"remote":"10.0.0.1:7433"}}"#;
        let req: UpdateConfigRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.node_name, Some("new".into()));
        assert_eq!(req.port, Some(9999));
        assert_eq!(req.data_dir, Some("/d".into()));
        assert_eq!(req.bind, Some(BindMode::Public));
        assert!(req.peers.is_some());
    }

    #[test]
    fn test_create_session_request_deserialize() {
        let json = r#"{"name":"my-task","workdir":"/tmp/repo","description":"Fix bug"}"#;
        let req: CreateSessionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "my-task");
        assert_eq!(req.workdir.as_deref(), Some("/tmp/repo"));
        assert_eq!(req.description.as_deref(), Some("Fix bug"));
        assert!(req.command.is_none());
        assert!(req.metadata.is_none());
        assert!(req.ink.is_none());
    }

    #[test]
    fn test_create_session_request_with_command() {
        let json = r#"{"name":"my-session","workdir":"/repo","command":"claude -p 'Do it'","description":"test","metadata":{"discord_channel":"123"},"ink":"coder"}"#;
        let req: CreateSessionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "my-session");
        assert_eq!(req.command, Some("claude -p 'Do it'".into()));
        assert_eq!(req.description, Some("test".into()));
        assert_eq!(
            req.metadata.as_ref().unwrap().get("discord_channel"),
            Some(&"123".into())
        );
        assert_eq!(req.ink, Some("coder".into()));
    }

    #[test]
    fn test_create_session_request_all_optional() {
        let json = r#"{"name":"my-task","workdir":"/tmp"}"#;
        let req: CreateSessionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.workdir.as_deref(), Some("/tmp"));
        assert!(req.command.is_none());
    }

    #[test]
    fn test_send_input_request_deserialize() {
        let json = r#"{"text":"hello world"}"#;
        let req: SendInputRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.text, "hello world");
    }

    #[test]
    fn test_send_input_request_missing_text() {
        let json = r"{}";
        let result = serde_json::from_str::<SendInputRequest>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_send_input_request_debug() {
        let req = SendInputRequest {
            text: "test".into(),
        };
        let debug = format!("{req:?}");
        assert!(debug.contains("test"));
    }

    #[test]
    fn test_output_query_deserialize() {
        let json = r#"{"lines":100}"#;
        let query: OutputQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.lines, Some(100));
    }

    #[test]
    fn test_output_query_empty() {
        let json = r"{}";
        let query: OutputQuery = serde_json::from_str(json).unwrap();
        assert!(query.lines.is_none());
    }

    #[test]
    fn test_output_query_debug() {
        let query = OutputQuery { lines: Some(50) };
        let debug = format!("{query:?}");
        assert!(debug.contains("50"));
    }

    #[test]
    fn test_error_response_serialize() {
        let err = ErrorResponse {
            error: "something failed".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        assert_eq!(json, r#"{"error":"something failed"}"#);
    }

    #[test]
    fn test_error_response_debug() {
        let err = ErrorResponse {
            error: "test".into(),
        };
        let debug = format!("{err:?}");
        assert!(debug.contains("test"));
    }

    #[test]
    fn test_create_session_request_debug() {
        let req = CreateSessionRequest {
            name: "test".into(),
            workdir: Some("/tmp".into()),
            command: Some("echo hello".into()),
            ink: None,
            description: None,
            metadata: None,
            idle_threshold_secs: None,
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
        };
        let debug = format!("{req:?}");
        assert!(debug.contains("/tmp"));
    }

    #[test]
    fn test_create_session_request_with_worktree_base() {
        let json = r#"{"name":"test","worktree":true,"worktree_base":"main"}"#;
        let req: CreateSessionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.worktree_base, Some("main".into()));
        assert_eq!(req.worktree, Some(true));
    }

    #[test]
    fn test_create_session_request_without_worktree_base() {
        let json = r#"{"name":"test"}"#;
        let req: CreateSessionRequest = serde_json::from_str(json).unwrap();
        assert!(req.worktree_base.is_none());
    }

    #[test]
    fn test_create_session_request_with_secrets() {
        let json = r#"{"name":"test","secrets":["GITHUB_TOKEN","NPM_TOKEN"]}"#;
        let req: CreateSessionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(
            req.secrets,
            Some(vec!["GITHUB_TOKEN".into(), "NPM_TOKEN".into()])
        );
    }

    #[test]
    fn test_create_session_request_without_secrets() {
        let json = r#"{"name":"test"}"#;
        let req: CreateSessionRequest = serde_json::from_str(json).unwrap();
        assert!(req.secrets.is_none());
    }

    #[test]
    fn test_ws_control_resize_serialize() {
        let msg = WsControl::Resize {
            cols: 120,
            rows: 40,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"resize\""));
        assert!(json.contains("\"cols\":120"));
        assert!(json.contains("\"rows\":40"));
    }

    #[test]
    fn test_ws_control_resize_deserialize() {
        let json = r#"{"type":"resize","cols":80,"rows":24}"#;
        let msg: WsControl = serde_json::from_str(json).unwrap();
        match msg {
            WsControl::Resize { cols, rows } => {
                assert_eq!(cols, 80);
                assert_eq!(rows, 24);
            }
        }
    }

    #[test]
    fn test_ws_control_roundtrip() {
        let original = WsControl::Resize {
            cols: 200,
            rows: 50,
        };
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: WsControl = serde_json::from_str(&json).unwrap();
        match deserialized {
            WsControl::Resize { cols, rows } => {
                assert_eq!(cols, 200);
                assert_eq!(rows, 50);
            }
        }
    }

    #[test]
    fn test_ws_control_invalid_type() {
        let json = r#"{"type":"unknown","data":123}"#;
        let result = serde_json::from_str::<WsControl>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_ws_control_debug() {
        let msg = WsControl::Resize { cols: 80, rows: 24 };
        let debug = format!("{msg:?}");
        assert!(debug.contains("Resize"));
        assert!(debug.contains("80"));
        assert!(debug.contains("24"));
    }

    #[test]
    fn test_health_response_serialize() {
        let resp = HealthResponse {
            status: "ok".into(),
            version: "0.0.1".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"status\":\"ok\""));
        assert!(json.contains("\"version\":\"0.0.1\""));
    }

    #[test]
    fn test_health_response_deserialize() {
        let json = r#"{"status":"ok","version":"1.2.3"}"#;
        let resp: HealthResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.status, "ok");
        assert_eq!(resp.version, "1.2.3");
    }

    #[test]
    fn test_health_response_debug() {
        let resp = HealthResponse {
            status: "ok".into(),
            version: "0.0.1".into(),
        };
        let debug = format!("{resp:?}");
        assert!(debug.contains("ok"));
    }

    #[test]
    fn test_peers_response_serialize() {
        use crate::node::NodeInfo;
        use crate::peer::{PeerInfo, PeerSource, PeerStatus};

        let resp = PeersResponse {
            local: NodeInfo {
                name: "local".into(),
                hostname: "host".into(),
                os: "macos".into(),
                arch: "aarch64".into(),
                cpus: 8,
                memory_mb: 16384,
                gpu: None,
            },
            peers: vec![PeerInfo {
                name: "remote".into(),
                address: "10.0.0.2:7433".into(),
                status: PeerStatus::Online,
                node_info: None,
                session_count: Some(2),
                source: PeerSource::Configured,
            }],
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"local\""));
        assert!(json.contains("\"peers\""));
        assert!(json.contains("\"remote\""));
    }

    #[test]
    fn test_peers_response_deserialize() {
        let json = r#"{"local":{"name":"n","hostname":"h","os":"linux","arch":"x86_64","cpus":4,"memory_mb":8192,"gpu":null},"peers":[]}"#;
        let resp: PeersResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.local.name, "n");
        assert!(resp.peers.is_empty());
    }

    #[test]
    fn test_peers_response_roundtrip() {
        use crate::node::NodeInfo;

        let resp = PeersResponse {
            local: NodeInfo {
                name: "roundtrip".into(),
                hostname: "h".into(),
                os: "macos".into(),
                arch: "arm64".into(),
                cpus: 10,
                memory_mb: 32768,
                gpu: Some("M4".into()),
            },
            peers: vec![],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: PeersResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.local.name, "roundtrip");
        assert!(deserialized.peers.is_empty());
    }

    #[test]
    fn test_peers_response_debug() {
        use crate::node::NodeInfo;

        let resp = PeersResponse {
            local: NodeInfo {
                name: "debug".into(),
                hostname: "h".into(),
                os: "macos".into(),
                arch: "arm64".into(),
                cpus: 1,
                memory_mb: 0,
                gpu: None,
            },
            peers: vec![],
        };
        let debug = format!("{resp:?}");
        assert!(debug.contains("debug"));
    }

    #[test]
    fn test_add_peer_request_deserialize() {
        let json = r#"{"name":"remote","address":"10.0.0.1:7433"}"#;
        let req: AddPeerRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "remote");
        assert_eq!(req.address, "10.0.0.1:7433");
    }

    #[test]
    fn test_add_peer_request_debug() {
        let req = AddPeerRequest {
            name: "test".into(),
            address: "host:7433".into(),
        };
        let debug = format!("{req:?}");
        assert!(debug.contains("test"));
    }

    #[test]
    fn test_add_peer_request_serialize() {
        let req = AddPeerRequest {
            name: "node".into(),
            address: "10.0.0.1:7433".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"name\":\"node\""));
        assert!(json.contains("\"address\":\"10.0.0.1:7433\""));
    }

    #[test]
    fn test_add_peer_request_roundtrip() {
        let req = AddPeerRequest {
            name: "roundtrip".into(),
            address: "h:1".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: AddPeerRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "roundtrip");
        assert_eq!(parsed.address, "h:1");
    }

    #[test]
    fn test_add_peer_request_missing_fields() {
        let json = r#"{"name":"only-name"}"#;
        let result = serde_json::from_str::<AddPeerRequest>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_intervention_event_response_serde() {
        let event = InterventionEventResponse {
            id: 1,
            session_id: "abc-123".into(),
            code: None,
            reason: "Memory usage 95%".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("abc-123"));
        assert!(json.contains("Memory usage 95%"));
        let deserialized: InterventionEventResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, 1);
        assert_eq!(deserialized.session_id, "abc-123");
        assert_eq!(deserialized.reason, "Memory usage 95%");
        assert_eq!(deserialized.created_at, "2026-01-01T00:00:00Z");
    }

    #[test]
    fn test_intervention_event_response_debug() {
        let event = InterventionEventResponse {
            id: 42,
            session_id: "test".into(),
            code: None,
            reason: "reason".into(),
            created_at: "now".into(),
        };
        let debug = format!("{event:?}");
        assert!(debug.contains("42"));
    }

    #[test]
    fn test_intervention_event_response_clone() {
        let event = InterventionEventResponse {
            id: 1,
            session_id: "s".into(),
            code: None,
            reason: "r".into(),
            created_at: "c".into(),
        };
        #[allow(clippy::redundant_clone)]
        let cloned = event.clone();
        assert_eq!(cloned.id, 1);
        assert_eq!(cloned.session_id, "s");
    }

    #[test]
    fn test_intervention_event_response_with_code() {
        let event = InterventionEventResponse {
            id: 1,
            session_id: "abc".into(),
            code: Some(InterventionCode::MemoryPressure),
            reason: "Memory 95%".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("memory_pressure"));
        let deserialized: InterventionEventResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.code, Some(InterventionCode::MemoryPressure));
    }

    #[test]
    fn test_intervention_event_response_code_none_skipped() {
        let event = InterventionEventResponse {
            id: 1,
            session_id: "abc".into(),
            code: None,
            reason: "reason".into(),
            created_at: "now".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("code"));
    }

    #[test]
    fn test_list_sessions_query_default() {
        let q = ListSessionsQuery::default();
        assert!(q.status.is_none());
        assert!(q.search.is_none());
        assert!(q.sort.is_none());
        assert!(q.order.is_none());
    }

    #[test]
    fn test_list_sessions_query_deserialize() {
        let json =
            r#"{"status":"running,completed","search":"bug","sort":"created_at","order":"asc"}"#;
        let q: ListSessionsQuery = serde_json::from_str(json).unwrap();
        assert_eq!(q.status, Some("running,completed".into()));
        assert_eq!(q.search, Some("bug".into()));
        assert_eq!(q.sort, Some("created_at".into()));
        assert_eq!(q.order, Some("asc".into()));
    }

    #[test]
    fn test_list_sessions_query_empty() {
        let json = "{}";
        let q: ListSessionsQuery = serde_json::from_str(json).unwrap();
        assert!(q.status.is_none());
    }

    #[test]
    fn test_list_sessions_query_debug() {
        let q = ListSessionsQuery {
            status: Some("active".into()),
            search: None,
            sort: None,
            order: None,
        };
        let debug = format!("{q:?}");
        assert!(debug.contains("active"));
    }

    // -- Web Push type tests --

    #[test]
    fn test_vapid_public_key_response_serialize() {
        let resp = VapidPublicKeyResponse {
            public_key: "BPXYZ123".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("BPXYZ123"));
    }

    #[test]
    fn test_vapid_public_key_response_deserialize() {
        let json = r#"{"public_key":"BPXYZ123"}"#;
        let resp: VapidPublicKeyResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.public_key, "BPXYZ123");
    }

    #[test]
    fn test_vapid_public_key_response_debug() {
        let resp = VapidPublicKeyResponse {
            public_key: "key".into(),
        };
        let debug = format!("{resp:?}");
        assert!(debug.contains("VapidPublicKeyResponse"));
    }

    #[test]
    fn test_push_subscription_request_deserialize() {
        let json = r#"{"endpoint":"https://push.example.com","keys":{"p256dh":"p256dh-val","auth":"auth-val"}}"#;
        let req: PushSubscriptionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.endpoint, "https://push.example.com");
        assert_eq!(req.keys.p256dh, "p256dh-val");
        assert_eq!(req.keys.auth, "auth-val");
    }

    #[test]
    fn test_push_subscription_request_missing_fields() {
        let json = r#"{"endpoint":"https://push.example.com"}"#;
        let result = serde_json::from_str::<PushSubscriptionRequest>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_push_subscription_request_debug() {
        let req = PushSubscriptionRequest {
            endpoint: "https://push.example.com".into(),
            keys: PushSubscriptionKeys {
                p256dh: "p".into(),
                auth: "a".into(),
            },
        };
        let debug = format!("{req:?}");
        assert!(debug.contains("PushSubscriptionRequest"));
    }

    #[test]
    fn test_push_subscription_keys_debug() {
        let keys = PushSubscriptionKeys {
            p256dh: "p".into(),
            auth: "a".into(),
        };
        let debug = format!("{keys:?}");
        assert!(debug.contains("PushSubscriptionKeys"));
    }

    #[test]
    fn test_push_unsubscribe_request_deserialize() {
        let json = r#"{"endpoint":"https://push.example.com"}"#;
        let req: PushUnsubscribeRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.endpoint, "https://push.example.com");
    }

    #[test]
    fn test_push_unsubscribe_request_missing_endpoint() {
        let json = r"{}";
        let result = serde_json::from_str::<PushUnsubscribeRequest>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_push_unsubscribe_request_debug() {
        let req = PushUnsubscribeRequest {
            endpoint: "ep".into(),
        };
        let debug = format!("{req:?}");
        assert!(debug.contains("PushUnsubscribeRequest"));
    }

    #[test]
    fn test_auth_config_response_serialize() {
        let resp = AuthConfigResponse {};
        let json = serde_json::to_string(&resp).unwrap();
        assert_eq!(json, "{}");
    }

    #[test]
    fn test_auth_config_response_deserialize() {
        let json = "{}";
        let _resp: AuthConfigResponse = serde_json::from_str(json).unwrap();
    }

    #[test]
    fn test_auth_config_response_debug() {
        let resp = AuthConfigResponse {};
        let debug = format!("{resp:?}");
        assert!(debug.contains("AuthConfigResponse"));
    }

    #[test]
    fn test_node_config_response_bind() {
        let resp = NodeConfigResponse {
            name: "test".into(),
            port: 7433,
            data_dir: "/tmp".into(),
            bind: BindMode::Tailscale,
            tag: None,
            discovery_interval_secs: 30,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"bind\":\"tailscale\""));
    }

    #[test]
    fn test_auth_token_response_serialize() {
        let resp = AuthTokenResponse {
            token: "abc123".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert_eq!(json, r#"{"token":"abc123"}"#);
    }

    #[test]
    fn test_auth_token_response_deserialize() {
        let json = r#"{"token":"abc123"}"#;
        let resp: AuthTokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.token, "abc123");
    }

    #[test]
    fn test_auth_token_response_debug() {
        let resp = AuthTokenResponse {
            token: "tok".into(),
        };
        let debug = format!("{resp:?}");
        assert!(debug.contains("tok"));
    }

    #[test]
    fn test_pairing_url_response_serialize() {
        let resp = PairingUrlResponse {
            url: "http://example.com/pair?token=abc".into(),
            token: "abc".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("http://example.com/pair?token=abc"));
    }

    #[test]
    fn test_pairing_url_response_deserialize() {
        let json = r#"{"url":"http://pair.test","token":"tok"}"#;
        let resp: PairingUrlResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.url, "http://pair.test");
        assert_eq!(resp.token, "tok");
    }

    #[test]
    fn test_pairing_url_response_debug() {
        let resp = PairingUrlResponse {
            url: "u".into(),
            token: "t".into(),
        };
        let debug = format!("{resp:?}");
        assert!(debug.contains("PairingUrlResponse"));
    }

    #[test]
    fn test_create_session_response_serialize() {
        use crate::session::{Runtime, Session, SessionStatus};
        use chrono::Utc;
        use uuid::Uuid;
        let session = Session {
            id: Uuid::nil(),
            name: "test".into(),
            workdir: "/tmp".into(),
            command: "echo hi".into(),
            description: None,
            status: SessionStatus::Active,
            exit_code: None,
            backend_session_id: None,
            output_snapshot: None,
            metadata: None,
            ink: None,
            intervention_code: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            idle_threshold_secs: None,
            worktree_path: None,
            worktree_branch: None,
            git_branch: None,
            git_commit: None,
            git_files_changed: None,
            git_insertions: None,
            git_deletions: None,
            git_ahead: None,
            runtime: Runtime::Tmux,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let resp = CreateSessionResponse { session };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"name\":\"test\""));
    }

    #[test]
    fn test_ink_config_response_serialize() {
        let resp = InkConfigResponse {
            description: Some("Code review".into()),
            command: Some("claude -p 'review'".into()),
            secrets: vec![],
            runtime: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("Code review"));
        assert!(json.contains("claude -p 'review'"));
    }

    #[test]
    fn test_ink_config_response_clone() {
        let resp = InkConfigResponse {
            description: None,
            command: None,
            secrets: vec![],
            runtime: None,
        };
        #[allow(clippy::redundant_clone)]
        let cloned = resp.clone();
        assert!(cloned.description.is_none());
    }

    #[test]
    fn test_update_watchdog_request_default() {
        let req = UpdateWatchdogRequest::default();
        assert!(req.enabled.is_none());
        assert!(req.memory_threshold.is_none());
    }

    #[test]
    fn test_update_watchdog_request_deserialize() {
        let json = r#"{"enabled":false,"memory_threshold":80}"#;
        let req: UpdateWatchdogRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.enabled, Some(false));
        assert_eq!(req.memory_threshold, Some(80));
    }

    #[test]
    fn test_update_notifications_request_default() {
        let req = UpdateNotificationsRequest::default();
        assert!(req.discord.is_none());
        assert!(req.webhooks.is_none());
    }

    #[test]
    fn test_update_notifications_request_deserialize() {
        let json = r#"{"discord":{"webhook_url":"http://hook","events":["killed"]}}"#;
        let req: UpdateNotificationsRequest = serde_json::from_str(json).unwrap();
        assert!(req.discord.is_some());
        let d = req.discord.unwrap();
        assert_eq!(d.webhook_url, "http://hook");
        assert_eq!(d.events, vec!["killed"]);
    }

    #[test]
    fn test_webhook_endpoint_update_request() {
        let json = r#"{"name":"test","url":"http://hook","events":["killed"],"secret":"s"}"#;
        let req: WebhookEndpointUpdateRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "test");
        assert_eq!(req.secret, Some("s".into()));
    }

    // -- Fleet type tests --

    fn make_fleet_session() -> FleetSession {
        use crate::session::{Runtime, Session, SessionStatus};
        use chrono::Utc;
        use uuid::Uuid;
        FleetSession {
            node_name: "node-a".into(),
            node_address: "10.0.0.1:7433".into(),
            session: Session {
                id: Uuid::nil(),
                name: "my-session".into(),
                workdir: "/tmp".into(),
                command: "echo hi".into(),
                description: None,
                status: SessionStatus::Active,
                exit_code: None,
                backend_session_id: None,
                output_snapshot: None,
                metadata: None,
                ink: None,
                intervention_code: None,
                intervention_reason: None,
                intervention_at: None,
                last_output_at: None,
                idle_since: None,
                idle_threshold_secs: None,
                worktree_path: None,
                worktree_branch: None,
                git_branch: None,
                git_commit: None,
                git_files_changed: None,
                git_insertions: None,
                git_deletions: None,
                git_ahead: None,
                runtime: Runtime::Tmux,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
        }
    }

    #[test]
    fn test_fleet_session_serialize() {
        let fs = make_fleet_session();
        let json = serde_json::to_string(&fs).unwrap();
        assert!(json.contains("\"node_name\":\"node-a\""));
        assert!(json.contains("\"node_address\":\"10.0.0.1:7433\""));
        // Flattened session fields should appear at top level
        assert!(json.contains("\"name\":\"my-session\""));
        assert!(json.contains("\"command\":\"echo hi\""));
    }

    #[test]
    fn test_fleet_session_deserialize() {
        let fs = make_fleet_session();
        let json = serde_json::to_string(&fs).unwrap();
        let deserialized: FleetSession = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.node_name, "node-a");
        assert_eq!(deserialized.node_address, "10.0.0.1:7433");
        assert_eq!(deserialized.session.name, "my-session");
    }

    #[test]
    fn test_fleet_session_debug() {
        let fs = make_fleet_session();
        let debug = format!("{fs:?}");
        assert!(debug.contains("node-a"));
    }

    #[test]
    fn test_fleet_session_clone() {
        let fs = make_fleet_session();
        #[allow(clippy::redundant_clone)]
        let cloned = fs.clone();
        assert_eq!(cloned.node_name, "node-a");
        assert_eq!(cloned.session.name, "my-session");
    }

    #[test]
    fn test_fleet_session_local_empty_address() {
        let mut fs = make_fleet_session();
        fs.node_address = String::new();
        let json = serde_json::to_string(&fs).unwrap();
        assert!(json.contains("\"node_address\":\"\""));
        let deserialized: FleetSession = serde_json::from_str(&json).unwrap();
        assert!(deserialized.node_address.is_empty());
    }

    #[test]
    fn test_fleet_sessions_response_serialize() {
        let resp = FleetSessionsResponse {
            sessions: vec![make_fleet_session()],
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"sessions\""));
        assert!(json.contains("\"node_name\":\"node-a\""));
    }

    #[test]
    fn test_fleet_sessions_response_deserialize() {
        let resp = FleetSessionsResponse {
            sessions: vec![make_fleet_session()],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: FleetSessionsResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.sessions.len(), 1);
        assert_eq!(deserialized.sessions[0].node_name, "node-a");
    }

    #[test]
    fn test_fleet_sessions_response_empty() {
        let resp = FleetSessionsResponse { sessions: vec![] };
        let json = serde_json::to_string(&resp).unwrap();
        assert_eq!(json, r#"{"sessions":[]}"#);
    }

    #[test]
    fn test_fleet_sessions_response_debug() {
        let resp = FleetSessionsResponse { sessions: vec![] };
        let debug = format!("{resp:?}");
        assert!(debug.contains("FleetSessionsResponse"));
    }

    #[test]
    fn test_webhook_endpoint_config_response_clone() {
        let resp = WebhookEndpointConfigResponse {
            name: "test".into(),
            url: "http://hook".into(),
            events: vec!["killed".into()],
            has_secret: false,
        };
        #[allow(clippy::redundant_clone)]
        let cloned = resp.clone();
        assert_eq!(cloned.name, "test");
    }

    // -- Schedule type tests --

    fn make_schedule() -> Schedule {
        Schedule {
            id: "sched-1".into(),
            name: "nightly-review".into(),
            cron: "0 3 * * *".into(),
            command: "claude -p 'review'".into(),
            workdir: "/tmp".into(),
            target_node: None,
            ink: None,
            description: Some("Nightly review".into()),
            runtime: None,
            secrets: vec![],
            worktree: None,
            worktree_base: None,
            enabled: true,
            last_run_at: None,
            last_session_id: None,
            created_at: "2026-03-18T00:00:00Z".into(),
        }
    }

    #[test]
    fn test_schedule_serialize() {
        let schedule = make_schedule();
        let json = serde_json::to_string(&schedule).unwrap();
        assert!(json.contains("\"name\":\"nightly-review\""));
        assert!(json.contains("\"cron\":\"0 3 * * *\""));
        assert!(json.contains("\"enabled\":true"));
    }

    #[test]
    fn test_schedule_deserialize() {
        let json = r#"{"id":"s1","name":"test","cron":"* * * * *","command":"echo","workdir":"/tmp","target_node":null,"ink":null,"description":null,"enabled":true,"last_run_at":null,"last_session_id":null,"created_at":"2026-01-01T00:00:00Z"}"#;
        let schedule: Schedule = serde_json::from_str(json).unwrap();
        assert_eq!(schedule.id, "s1");
        assert_eq!(schedule.name, "test");
        assert!(schedule.enabled);
    }

    #[test]
    fn test_schedule_debug() {
        let schedule = make_schedule();
        let debug = format!("{schedule:?}");
        assert!(debug.contains("nightly-review"));
    }

    #[test]
    fn test_schedule_clone() {
        let schedule = make_schedule();
        #[allow(clippy::redundant_clone)]
        let cloned = schedule.clone();
        assert_eq!(cloned.id, "sched-1");
        assert_eq!(cloned.name, "nightly-review");
    }

    #[test]
    fn test_schedule_roundtrip() {
        let schedule = make_schedule();
        let json = serde_json::to_string(&schedule).unwrap();
        let deserialized: Schedule = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, schedule.id);
        assert_eq!(deserialized.name, schedule.name);
        assert_eq!(deserialized.cron, schedule.cron);
        assert_eq!(deserialized.enabled, schedule.enabled);
    }

    #[test]
    fn test_schedule_with_target_node() {
        let mut schedule = make_schedule();
        schedule.target_node = Some("auto".into());
        let json = serde_json::to_string(&schedule).unwrap();
        assert!(json.contains("\"target_node\":\"auto\""));
        let deserialized: Schedule = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.target_node, Some("auto".into()));
    }

    #[test]
    fn test_create_schedule_request_deserialize() {
        let json = r#"{"name":"daily","cron":"0 9 * * *","workdir":"/repo","command":"echo hi","ink":"coder","description":"Daily task"}"#;
        let req: CreateScheduleRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "daily");
        assert_eq!(req.cron, "0 9 * * *");
        assert_eq!(req.command, Some("echo hi".into()));
        assert_eq!(req.ink, Some("coder".into()));
        assert_eq!(req.description, Some("Daily task".into()));
    }

    #[test]
    fn test_create_schedule_request_minimal() {
        let json = r#"{"name":"min","cron":"* * * * *","workdir":"/tmp"}"#;
        let req: CreateScheduleRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "min");
        assert!(req.command.is_none());
        assert!(req.target_node.is_none());
        assert!(req.ink.is_none());
        assert!(req.description.is_none());
    }

    #[test]
    fn test_create_schedule_request_debug() {
        let json = r#"{"name":"dbg","cron":"* * * * *","workdir":"/tmp"}"#;
        let req: CreateScheduleRequest = serde_json::from_str(json).unwrap();
        let debug = format!("{req:?}");
        assert!(debug.contains("dbg"));
    }

    #[test]
    fn test_update_schedule_request_default() {
        let req = UpdateScheduleRequest::default();
        assert!(req.cron.is_none());
        assert!(req.command.is_none());
        assert!(req.workdir.is_none());
        assert!(req.enabled.is_none());
    }

    #[test]
    fn test_update_schedule_request_deserialize() {
        let json = r#"{"cron":"0 5 * * *","enabled":false}"#;
        let req: UpdateScheduleRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.cron, Some("0 5 * * *".into()));
        assert_eq!(req.enabled, Some(false));
        assert!(req.command.is_none());
    }

    #[test]
    fn test_update_schedule_request_debug() {
        let req = UpdateScheduleRequest {
            enabled: Some(true),
            ..Default::default()
        };
        let debug = format!("{req:?}");
        assert!(debug.contains("true"));
    }

    // -- Secret type tests --

    #[test]
    fn test_set_secret_request_deserialize() {
        let json = r#"{"value":"super-secret"}"#;
        let req: SetSecretRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.value, "super-secret");
        assert!(req.env.is_none());
    }

    #[test]
    fn test_set_secret_request_with_env() {
        let json = r#"{"value":"super-secret","env":"CUSTOM_VAR"}"#;
        let req: SetSecretRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.value, "super-secret");
        assert_eq!(req.env.as_deref(), Some("CUSTOM_VAR"));
    }

    #[test]
    fn test_set_secret_request_missing_value() {
        let json = r"{}";
        let result = serde_json::from_str::<SetSecretRequest>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_set_secret_request_debug() {
        let req = SetSecretRequest {
            value: "secret".into(),
            env: None,
        };
        let debug = format!("{req:?}");
        assert!(debug.contains("SetSecretRequest"));
    }

    #[test]
    fn test_secret_list_response_serialize() {
        let resp = SecretListResponse {
            secrets: vec![SecretEntry {
                name: "MY_TOKEN".into(),
                env: None,
                created_at: "2026-01-01T00:00:00Z".into(),
            }],
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("MY_TOKEN"));
        assert!(json.contains("2026-01-01"));
        // env is None, so it should be skipped
        assert!(!json.contains("\"env\""));
    }

    #[test]
    fn test_secret_list_response_serialize_with_env() {
        let resp = SecretListResponse {
            secrets: vec![SecretEntry {
                name: "GH_WORK".into(),
                env: Some("GITHUB_TOKEN".into()),
                created_at: "2026-01-01T00:00:00Z".into(),
            }],
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("GH_WORK"));
        assert!(json.contains("GITHUB_TOKEN"));
    }

    #[test]
    fn test_secret_list_response_deserialize() {
        let json = r#"{"secrets":[{"name":"KEY","created_at":"2026-01-01T00:00:00Z"}]}"#;
        let resp: SecretListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.secrets.len(), 1);
        assert_eq!(resp.secrets[0].name, "KEY");
        assert!(resp.secrets[0].env.is_none());
    }

    #[test]
    fn test_secret_list_response_deserialize_with_env() {
        let json = r#"{"secrets":[{"name":"KEY","env":"CUSTOM_ENV","created_at":"2026-01-01T00:00:00Z"}]}"#;
        let resp: SecretListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.secrets[0].env.as_deref(), Some("CUSTOM_ENV"));
    }

    #[test]
    fn test_secret_list_response_empty() {
        let resp = SecretListResponse { secrets: vec![] };
        let json = serde_json::to_string(&resp).unwrap();
        assert_eq!(json, r#"{"secrets":[]}"#);
    }

    #[test]
    fn test_secret_list_response_debug() {
        let resp = SecretListResponse { secrets: vec![] };
        let debug = format!("{resp:?}");
        assert!(debug.contains("SecretListResponse"));
    }

    #[test]
    fn test_secret_entry_clone() {
        let entry = SecretEntry {
            name: "KEY".into(),
            env: Some("CUSTOM".into()),
            created_at: "now".into(),
        };
        #[allow(clippy::redundant_clone)]
        let cloned = entry.clone();
        assert_eq!(cloned.name, "KEY");
        assert_eq!(cloned.env.as_deref(), Some("CUSTOM"));
    }

    #[test]
    fn test_secret_entry_debug() {
        let entry = SecretEntry {
            name: "KEY".into(),
            env: None,
            created_at: "now".into(),
        };
        let debug = format!("{entry:?}");
        assert!(debug.contains("SecretEntry"));
    }

    #[test]
    fn test_secret_entry_roundtrip() {
        let entry = SecretEntry {
            name: "MY_VAR".into(),
            env: None,
            created_at: "2026-03-21T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: SecretEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "MY_VAR");
        assert!(deserialized.env.is_none());
        assert_eq!(deserialized.created_at, "2026-03-21T00:00:00Z");
    }

    #[test]
    fn test_secret_entry_roundtrip_with_env() {
        let entry = SecretEntry {
            name: "GH_WORK".into(),
            env: Some("GITHUB_TOKEN".into()),
            created_at: "2026-03-21T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: SecretEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "GH_WORK");
        assert_eq!(deserialized.env.as_deref(), Some("GITHUB_TOKEN"));
    }

    #[test]
    fn test_update_schedule_request_nullable_fields() {
        // serde treats `"field":null` as None for Option<Option<T>> by default
        let json = r#"{"target_node":null,"ink":"coder","description":null}"#;
        let req: UpdateScheduleRequest = serde_json::from_str(json).unwrap();
        assert!(req.target_node.is_none());
        assert_eq!(req.ink, Some(Some("coder".into())));
        assert!(req.description.is_none());
    }
}
