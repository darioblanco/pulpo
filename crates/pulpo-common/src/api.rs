use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::auth::BindMode;
use crate::knowledge::{Knowledge, KnowledgeKind};
use crate::node::NodeInfo;
use crate::peer::{PeerEntry, PeerInfo};
use crate::session::{Provider, Session, SessionMode};

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub name: Option<String>,
    pub workdir: Option<String>,
    pub provider: Option<Provider>,
    pub prompt: Option<String>,
    pub mode: Option<SessionMode>,
    pub unrestricted: Option<bool>,
    pub model: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub system_prompt: Option<String>,
    pub metadata: Option<HashMap<String, String>>,
    pub ink: Option<String>,
    pub max_turns: Option<u32>,
    pub max_budget_usd: Option<f64>,
    pub output_format: Option<String>,
    pub worktree: Option<bool>,
    pub conversation_id: Option<String>,
}

/// Response from session creation, includes the session and any capability warnings.
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateSessionResponse {
    pub session: Session,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct SendInputRequest {
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct OutputQuery {
    pub lines: Option<usize>,
}

#[derive(Debug, Serialize)]
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
    pub guards: GuardDefaultConfigResponse,
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
    pub seed: Option<String>,
    pub discovery_interval_secs: u64,
    pub default_provider: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GuardDefaultConfigResponse {
    pub unrestricted: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WatchdogConfigResponse {
    pub enabled: bool,
    pub memory_threshold: u8,
    pub check_interval_secs: u64,
    pub breach_count: u32,
    pub idle_timeout_secs: u64,
    pub idle_action: String,
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
    pub provider: Option<String>,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub unrestricted: Option<bool>,
    pub instructions: Option<String>,
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
    pub seed: Option<String>,
    pub discovery_interval_secs: Option<u64>,
    // Guard defaults
    pub unrestricted: Option<bool>,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct AddPeerRequest {
    pub name: String,
    pub address: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InterventionEventResponse {
    pub id: i64,
    pub session_id: String,
    pub reason: String,
    pub created_at: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct ListSessionsQuery {
    pub status: Option<String>,
    pub provider: Option<String>,
    pub search: Option<String>,
    pub sort: Option<String>,
    pub order: Option<String>,
}

// -- Knowledge types --

#[derive(Debug, Serialize, Deserialize)]
pub struct KnowledgeResponse {
    pub knowledge: Vec<Knowledge>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ListKnowledgeQuery {
    pub repo: Option<String>,
    pub ink: Option<String>,
    pub kind: Option<KnowledgeKind>,
    pub session_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
pub struct KnowledgeContextQuery {
    pub workdir: Option<String>,
    pub ink: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateKnowledgeRequest {
    pub title: Option<String>,
    pub body: Option<String>,
    pub tags: Option<Vec<String>>,
    pub relevance: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KnowledgeItemResponse {
    pub knowledge: Knowledge,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KnowledgeDeleteResponse {
    pub deleted: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KnowledgePushResponse {
    pub pushed: bool,
    pub message: String,
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
                seed: None,
                discovery_interval_secs: 30,
                default_provider: None,
            },
            auth: AuthConfigResponse {},
            peers: HashMap::new(),
            guards: GuardDefaultConfigResponse {
                unrestricted: false,
            },
            watchdog: WatchdogConfigResponse {
                enabled: true,
                memory_threshold: 90,
                check_interval_secs: 10,
                breach_count: 3,
                idle_timeout_secs: 600,
                idle_action: "alert".into(),
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
        let resp = ConfigResponse {
            node: NodeConfigResponse {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp".into(),
                bind: BindMode::Local,
                tag: None,
                seed: None,
                discovery_interval_secs: 30,
                default_provider: None,
            },
            auth: AuthConfigResponse {},
            peers: HashMap::new(),
            guards: GuardDefaultConfigResponse {
                unrestricted: false,
            },
            watchdog: WatchdogConfigResponse {
                enabled: true,
                memory_threshold: 90,
                check_interval_secs: 10,
                breach_count: 3,
                idle_timeout_secs: 600,
                idle_action: "alert".into(),
            },
            notifications: NotificationsConfigResponse {
                discord: None,
                webhooks: vec![],
            },
            inks: HashMap::new(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"test\""));
        assert!(json.contains("7433"));
    }

    #[test]
    fn test_config_response_deserialize() {
        let json = r#"{"node":{"name":"n","port":1234,"data_dir":"/d","bind":"local","tag":null,"seed":null,"discovery_interval_secs":30},"auth":{},"peers":{},"guards":{"unrestricted":true},"watchdog":{"enabled":true,"memory_threshold":90,"check_interval_secs":10,"breach_count":3,"idle_timeout_secs":600,"idle_action":"alert"},"notifications":{"discord":null,"webhooks":[]},"inks":{}}"#;
        let resp: ConfigResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.node.name, "n");
        assert_eq!(resp.node.port, 1234);
        assert_eq!(resp.node.bind, BindMode::Local);
        assert!(resp.guards.unrestricted);
        assert!(resp.watchdog.enabled);
        assert!(resp.notifications.discord.is_none());
        assert!(resp.inks.is_empty());
    }

    #[test]
    fn test_config_response_debug() {
        let resp = ConfigResponse {
            node: NodeConfigResponse {
                name: "debug".into(),
                port: 7433,
                data_dir: "/tmp".into(),
                bind: BindMode::Local,
                tag: None,
                seed: None,
                discovery_interval_secs: 30,
                default_provider: None,
            },
            auth: AuthConfigResponse {},
            peers: HashMap::new(),
            guards: GuardDefaultConfigResponse {
                unrestricted: false,
            },
            watchdog: WatchdogConfigResponse {
                enabled: true,
                memory_threshold: 90,
                check_interval_secs: 10,
                breach_count: 3,
                idle_timeout_secs: 600,
                idle_action: "alert".into(),
            },
            notifications: NotificationsConfigResponse {
                discord: None,
                webhooks: vec![],
            },
            inks: HashMap::new(),
        };
        let debug = format!("{resp:?}");
        assert!(debug.contains("debug"));
    }

    #[test]
    fn test_node_config_response_debug() {
        let resp = NodeConfigResponse {
            name: "test".into(),
            port: 7433,
            data_dir: "/tmp".into(),
            bind: BindMode::Local,
            tag: None,
            seed: None,
            discovery_interval_secs: 30,
            default_provider: None,
        };
        let debug = format!("{resp:?}");
        assert!(debug.contains("test"));
    }

    #[test]
    fn test_guard_default_config_response_debug() {
        let resp = GuardDefaultConfigResponse { unrestricted: true };
        let debug = format!("{resp:?}");
        assert!(debug.contains("true"));
    }

    #[test]
    fn test_update_config_request_deserialize() {
        let json = r#"{"node_name":"new","port":9999}"#;
        let req: UpdateConfigRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.node_name, Some("new".into()));
        assert_eq!(req.port, Some(9999));
        assert!(req.data_dir.is_none());
        assert!(req.bind.is_none());
        assert!(req.unrestricted.is_none());
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
        let resp = ConfigResponse {
            node: NodeConfigResponse {
                name: "n".into(),
                port: 7433,
                data_dir: "/d".into(),
                bind: BindMode::Local,
                tag: None,
                seed: None,
                discovery_interval_secs: 30,
                default_provider: None,
            },
            auth: AuthConfigResponse {},
            peers,
            guards: GuardDefaultConfigResponse {
                unrestricted: false,
            },
            watchdog: WatchdogConfigResponse {
                enabled: true,
                memory_threshold: 90,
                check_interval_secs: 10,
                breach_count: 3,
                idle_timeout_secs: 600,
                idle_action: "alert".into(),
            },
            notifications: NotificationsConfigResponse {
                discord: None,
                webhooks: vec![],
            },
            inks: HashMap::new(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("remote"));
    }

    #[test]
    fn test_update_config_request_with_all_fields() {
        let json = r#"{"node_name":"new","port":9999,"data_dir":"/d","bind":"public","unrestricted":true,"peers":{"remote":"10.0.0.1:7433"}}"#;
        let req: UpdateConfigRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.node_name, Some("new".into()));
        assert_eq!(req.port, Some(9999));
        assert_eq!(req.data_dir, Some("/d".into()));
        assert_eq!(req.bind, Some(BindMode::Public));
        assert_eq!(req.unrestricted, Some(true));
        assert!(req.peers.is_some());
    }

    #[test]
    fn test_create_session_request_deserialize() {
        let json = r#"{"workdir":"/tmp/repo","prompt":"Fix bug"}"#;
        let req: CreateSessionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.workdir.as_deref(), Some("/tmp/repo"));
        assert_eq!(req.prompt.as_deref(), Some("Fix bug"));
        assert!(req.name.is_none());
        assert!(req.provider.is_none());
        assert!(req.mode.is_none());
        assert!(req.model.is_none());
        assert!(req.allowed_tools.is_none());
        assert!(req.system_prompt.is_none());
        assert!(req.metadata.is_none());
        assert!(req.ink.is_none());
    }

    #[test]
    fn test_create_session_request_with_all_fields() {
        let json = r#"{"name":"my-session","workdir":"/repo","provider":"claude","prompt":"Do it","mode":"autonomous","model":"opus","allowed_tools":["Read","Grep"],"system_prompt":"Be concise","metadata":{"discord_channel":"123"},"ink":"coder"}"#;
        let req: CreateSessionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, Some("my-session".into()));
        assert_eq!(req.provider, Some(crate::session::Provider::Claude));
        assert_eq!(req.mode, Some(SessionMode::Autonomous));
        assert_eq!(req.model, Some("opus".into()));
        assert_eq!(req.allowed_tools, Some(vec!["Read".into(), "Grep".into()]));
        assert_eq!(req.system_prompt, Some("Be concise".into()));
        assert_eq!(
            req.metadata.as_ref().unwrap().get("discord_channel"),
            Some(&"123".into())
        );
        assert_eq!(req.ink, Some("coder".into()));
    }

    #[test]
    fn test_create_session_request_all_optional() {
        let json = r#"{"workdir":"/tmp"}"#;
        let req: CreateSessionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.workdir.as_deref(), Some("/tmp"));
        assert!(req.prompt.is_none());
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
            name: None,
            workdir: Some("/tmp".into()),
            provider: None,
            prompt: Some("test".into()),
            mode: None,
            unrestricted: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            worktree: None,
            conversation_id: None,
        };
        let debug = format!("{req:?}");
        assert!(debug.contains("/tmp"));
    }

    #[test]
    fn test_create_session_request_with_conversation_id() {
        let json = r#"{"workdir":"/repo","prompt":"test","conversation_id":"conv-abc"}"#;
        let req: CreateSessionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.conversation_id.as_deref(), Some("conv-abc"));
    }

    #[test]
    fn test_create_session_request_without_conversation_id() {
        let json = r#"{"workdir":"/repo","prompt":"test"}"#;
        let req: CreateSessionRequest = serde_json::from_str(json).unwrap();
        assert!(req.conversation_id.is_none());
    }

    #[test]
    fn test_create_session_request_with_interactive_mode() {
        let json = r#"{"workdir":"/repo","prompt":"test","mode":"interactive"}"#;
        let req: CreateSessionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.mode, Some(SessionMode::Interactive));
    }

    #[test]
    fn test_create_session_request_with_unrestricted() {
        let json = r#"{"workdir":"/repo","prompt":"test","unrestricted":true}"#;
        let req: CreateSessionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.unrestricted, Some(true));
    }

    #[test]
    fn test_create_session_request_without_unrestricted() {
        let json = r#"{"workdir":"/repo","prompt":"test"}"#;
        let req: CreateSessionRequest = serde_json::from_str(json).unwrap();
        assert!(req.unrestricted.is_none());
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
            reason: "r".into(),
            created_at: "c".into(),
        };
        #[allow(clippy::redundant_clone)]
        let cloned = event.clone();
        assert_eq!(cloned.id, 1);
        assert_eq!(cloned.session_id, "s");
    }

    #[test]
    fn test_list_sessions_query_default() {
        let q = ListSessionsQuery::default();
        assert!(q.status.is_none());
        assert!(q.provider.is_none());
        assert!(q.search.is_none());
        assert!(q.sort.is_none());
        assert!(q.order.is_none());
    }

    #[test]
    fn test_list_sessions_query_deserialize() {
        let json = r#"{"status":"running,completed","provider":"claude","search":"bug","sort":"created_at","order":"asc"}"#;
        let q: ListSessionsQuery = serde_json::from_str(json).unwrap();
        assert_eq!(q.status, Some("running,completed".into()));
        assert_eq!(q.provider, Some("claude".into()));
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
            status: Some("running".into()),
            provider: None,
            search: None,
            sort: None,
            order: None,
        };
        let debug = format!("{q:?}");
        assert!(debug.contains("running"));
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
            seed: None,
            discovery_interval_secs: 30,
            default_provider: None,
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
        let json = r#"{"token":"secret-token"}"#;
        let resp: AuthTokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.token, "secret-token");
    }

    #[test]
    fn test_auth_token_response_roundtrip() {
        let original = AuthTokenResponse {
            token: "roundtrip-token".into(),
        };
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: AuthTokenResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.token, "roundtrip-token");
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
            url: "http://10.0.0.1:7433/pair".into(),
            token: "pair-token".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"url\":\"http://10.0.0.1:7433/pair\""));
        assert!(json.contains("\"token\":\"pair-token\""));
    }

    #[test]
    fn test_pairing_url_response_deserialize() {
        let json = r#"{"url":"http://host:7433/pair","token":"t123"}"#;
        let resp: PairingUrlResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.url, "http://host:7433/pair");
        assert_eq!(resp.token, "t123");
    }

    #[test]
    fn test_pairing_url_response_roundtrip() {
        let original = PairingUrlResponse {
            url: "http://example.com/pair".into(),
            token: "rt-token".into(),
        };
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: PairingUrlResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.url, "http://example.com/pair");
        assert_eq!(deserialized.token, "rt-token");
    }

    #[test]
    fn test_pairing_url_response_debug() {
        let resp = PairingUrlResponse {
            url: "http://debug.local/pair".into(),
            token: "dbg".into(),
        };
        let debug = format!("{resp:?}");
        assert!(debug.contains("debug.local"));
        assert!(debug.contains("dbg"));
    }

    #[test]
    fn test_webhook_endpoint_config_response_serialize() {
        let resp = WebhookEndpointConfigResponse {
            name: "ci-hook".into(),
            url: "https://example.com/hook".into(),
            events: vec!["completed".into()],
            has_secret: true,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"ci-hook\""));
        assert!(json.contains("\"has_secret\":true"));
    }

    #[test]
    fn test_webhook_endpoint_config_response_roundtrip() {
        let original = WebhookEndpointConfigResponse {
            name: "hook".into(),
            url: "https://example.com".into(),
            events: vec!["dead".into(), "running".into()],
            has_secret: false,
        };
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: WebhookEndpointConfigResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "hook");
        assert_eq!(deserialized.events.len(), 2);
        assert!(!deserialized.has_secret);
    }

    #[test]
    fn test_webhook_endpoint_config_response_debug_clone() {
        let resp = WebhookEndpointConfigResponse {
            name: "test".into(),
            url: "https://test.com".into(),
            events: vec![],
            has_secret: false,
        };
        #[allow(clippy::redundant_clone)]
        let cloned = resp.clone();
        let debug = format!("{cloned:?}");
        assert!(debug.contains("test"));
    }

    #[test]
    fn test_webhook_endpoint_update_request_deserialize() {
        let json =
            r#"{"name":"hook","url":"https://example.com","events":["dead"],"secret":"key"}"#;
        let req: WebhookEndpointUpdateRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "hook");
        assert_eq!(req.url, "https://example.com");
        assert_eq!(req.events, vec!["dead"]);
        assert_eq!(req.secret, Some("key".into()));
    }

    #[test]
    fn test_webhook_endpoint_update_request_no_secret() {
        let json = r#"{"name":"hook","url":"https://example.com"}"#;
        let req: WebhookEndpointUpdateRequest = serde_json::from_str(json).unwrap();
        assert!(req.secret.is_none());
        assert!(req.events.is_empty());
    }

    #[test]
    fn test_webhook_endpoint_update_request_debug_clone() {
        let req = WebhookEndpointUpdateRequest {
            name: "test".into(),
            url: "https://test.com".into(),
            events: vec![],
            secret: None,
        };
        #[allow(clippy::redundant_clone)]
        let cloned = req.clone();
        let debug = format!("{cloned:?}");
        assert!(debug.contains("test"));
    }

    #[test]
    fn test_update_config_request_with_webhooks() {
        let json =
            r#"{"webhooks":[{"name":"hook","url":"https://a.com","events":[],"secret":null}]}"#;
        let req: UpdateConfigRequest = serde_json::from_str(json).unwrap();
        assert!(req.webhooks.is_some());
        assert_eq!(req.webhooks.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_create_session_response_serialize_no_warnings() {
        use crate::session::{Session, SessionStatus};
        use chrono::Utc;
        use uuid::Uuid;
        let session = Session {
            id: Uuid::nil(),
            name: "test".into(),
            workdir: "/tmp".into(),
            provider: crate::session::Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            backend_session_id: None,
            output_snapshot: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let resp = CreateSessionResponse {
            session,
            warnings: vec![],
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"session\""));
        assert!(!json.contains("\"warnings\""));
    }

    #[test]
    fn test_create_session_response_serialize_with_warnings() {
        use crate::session::{Session, SessionStatus};
        use chrono::Utc;
        use uuid::Uuid;
        let session = Session {
            id: Uuid::nil(),
            name: "test".into(),
            workdir: "/tmp".into(),
            provider: crate::session::Provider::OpenCode,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            backend_session_id: None,
            output_snapshot: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let resp = CreateSessionResponse {
            session,
            warnings: vec!["opencode does not support --model; value ignored".into()],
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"warnings\""));
        assert!(json.contains("--model"));
    }

    #[test]
    fn test_create_session_response_debug() {
        use crate::session::{Session, SessionStatus};
        use chrono::Utc;
        use uuid::Uuid;
        let session = Session {
            id: Uuid::nil(),
            name: "test".into(),
            workdir: "/tmp".into(),
            provider: crate::session::Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            backend_session_id: None,
            output_snapshot: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            ink: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let resp = CreateSessionResponse {
            session,
            warnings: vec![],
        };
        let debug = format!("{resp:?}");
        assert!(debug.contains("CreateSessionResponse"));
    }

    #[test]
    fn test_notifications_config_response_with_webhooks() {
        let resp = NotificationsConfigResponse {
            discord: None,
            webhooks: vec![WebhookEndpointConfigResponse {
                name: "hook".into(),
                url: "https://example.com".into(),
                events: vec![],
                has_secret: false,
            }],
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"webhooks\""));
        assert!(json.contains("\"hook\""));
    }

    #[test]
    fn test_knowledge_response_serialize() {
        let resp = KnowledgeResponse { knowledge: vec![] };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"knowledge\":[]"));
    }

    #[test]
    fn test_knowledge_response_debug() {
        let resp = KnowledgeResponse { knowledge: vec![] };
        let debug = format!("{resp:?}");
        assert!(debug.contains("KnowledgeResponse"));
    }

    #[test]
    fn test_list_knowledge_query_default() {
        let q = ListKnowledgeQuery::default();
        assert!(q.repo.is_none());
        assert!(q.ink.is_none());
        assert!(q.kind.is_none());
        assert!(q.session_id.is_none());
        assert!(q.limit.is_none());
    }

    #[test]
    fn test_list_knowledge_query_deserialize() {
        let json = r#"{"repo":"/tmp/repo","ink":"coder","kind":"failure","limit":10}"#;
        let q: ListKnowledgeQuery = serde_json::from_str(json).unwrap();
        assert_eq!(q.repo, Some("/tmp/repo".into()));
        assert_eq!(q.ink, Some("coder".into()));
        assert_eq!(q.kind, Some(KnowledgeKind::Failure));
        assert_eq!(q.limit, Some(10));
    }

    #[test]
    fn test_list_knowledge_query_debug() {
        let q = ListKnowledgeQuery {
            repo: Some("/repo".into()),
            ..Default::default()
        };
        let debug = format!("{q:?}");
        assert!(debug.contains("/repo"));
    }

    #[test]
    fn test_knowledge_context_query_default() {
        let q = KnowledgeContextQuery::default();
        assert!(q.workdir.is_none());
        assert!(q.ink.is_none());
        assert!(q.limit.is_none());
    }

    #[test]
    fn test_knowledge_context_query_deserialize() {
        let json = r#"{"workdir":"/repo","ink":"reviewer","limit":5}"#;
        let q: KnowledgeContextQuery = serde_json::from_str(json).unwrap();
        assert_eq!(q.workdir, Some("/repo".into()));
        assert_eq!(q.ink, Some("reviewer".into()));
        assert_eq!(q.limit, Some(5));
    }

    #[test]
    fn test_knowledge_context_query_debug() {
        let q = KnowledgeContextQuery {
            workdir: Some("/repo".into()),
            ..Default::default()
        };
        let debug = format!("{q:?}");
        assert!(debug.contains("/repo"));
    }

    #[test]
    fn test_update_knowledge_request_deserialize() {
        let json = r#"{"title":"new","body":"updated","tags":["a","b"],"relevance":0.8}"#;
        let req: UpdateKnowledgeRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.title, Some("new".into()));
        assert_eq!(req.body, Some("updated".into()));
        assert_eq!(req.tags, Some(vec!["a".into(), "b".into()]));
        assert!((req.relevance.unwrap() - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn test_update_knowledge_request_partial() {
        let json = r#"{"title":"only-title"}"#;
        let req: UpdateKnowledgeRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.title, Some("only-title".into()));
        assert!(req.body.is_none());
        assert!(req.tags.is_none());
        assert!(req.relevance.is_none());
    }

    #[test]
    fn test_update_knowledge_request_debug() {
        let req = UpdateKnowledgeRequest {
            title: Some("test".into()),
            body: None,
            tags: None,
            relevance: None,
        };
        let debug = format!("{req:?}");
        assert!(debug.contains("test"));
    }

    #[test]
    fn test_knowledge_item_response_roundtrip() {
        use chrono::Utc;
        use uuid::Uuid;

        let resp = KnowledgeItemResponse {
            knowledge: Knowledge {
                id: Uuid::new_v4(),
                session_id: Uuid::new_v4(),
                kind: KnowledgeKind::Summary,
                scope_repo: Some("/repo".into()),
                scope_ink: Some("coder".into()),
                title: "test".into(),
                body: "body".into(),
                tags: vec!["tag".into()],
                relevance: 0.5,
                created_at: Utc::now(),
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: KnowledgeItemResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.knowledge.title, "test");
    }

    #[test]
    fn test_knowledge_delete_response_roundtrip() {
        let resp = KnowledgeDeleteResponse { deleted: true };
        let json = serde_json::to_string(&resp).unwrap();
        let back: KnowledgeDeleteResponse = serde_json::from_str(&json).unwrap();
        assert!(back.deleted);
    }

    #[test]
    fn test_knowledge_push_response_roundtrip() {
        let resp = KnowledgePushResponse {
            pushed: true,
            message: "pushed to remote".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: KnowledgePushResponse = serde_json::from_str(&json).unwrap();
        assert!(back.pushed);
        assert_eq!(back.message, "pushed to remote");
    }
}
