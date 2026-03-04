use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::auth::BindMode;
use crate::guard::{EnvFilter, GuardConfig, GuardPreset};
use crate::node::NodeInfo;
use crate::peer::{PeerEntry, PeerInfo};
use crate::session::{Provider, SessionMode};

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub name: Option<String>,
    pub workdir: String,
    pub provider: Option<Provider>,
    pub prompt: String,
    pub mode: Option<SessionMode>,
    pub guard_preset: Option<GuardPreset>,
    pub guard_config: Option<GuardConfig>,
    pub model: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub system_prompt: Option<String>,
    pub metadata: Option<HashMap<String, String>>,
    pub persona: Option<String>,
    pub max_turns: Option<u32>,
    pub max_budget_usd: Option<f64>,
    pub output_format: Option<String>,
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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthConfigResponse {
    pub bind: BindMode,
}

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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GuardDefaultConfigResponse {
    pub preset: GuardPreset,
    pub env: Option<EnvFilter>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateConfigRequest {
    pub node_name: Option<String>,
    pub port: Option<u16>,
    pub data_dir: Option<String>,
    pub bind: Option<BindMode>,
    pub guard_preset: Option<GuardPreset>,
    pub guard_env: Option<EnvFilter>,
    pub peers: Option<HashMap<String, PeerEntry>>,
}

#[derive(Debug, Serialize)]
pub struct UpdateConfigResponse {
    pub config: ConfigResponse,
    pub restart_required: bool,
}

#[derive(Debug, Deserialize)]
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_response_serialize() {
        let resp = ConfigResponse {
            node: NodeConfigResponse {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp".into(),
            },
            auth: AuthConfigResponse {
                bind: BindMode::Local,
            },
            peers: HashMap::new(),
            guards: GuardDefaultConfigResponse {
                preset: GuardPreset::Standard,
                env: None,
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"test\""));
        assert!(json.contains("7433"));
    }

    #[test]
    fn test_config_response_deserialize() {
        let json = r#"{"node":{"name":"n","port":1234,"data_dir":"/d"},"auth":{"bind":"local"},"peers":{},"guards":{"preset":"strict","env":null}}"#;
        let resp: ConfigResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.node.name, "n");
        assert_eq!(resp.node.port, 1234);
        assert_eq!(resp.auth.bind, BindMode::Local);
        assert_eq!(resp.guards.preset, GuardPreset::Strict);
    }

    #[test]
    fn test_config_response_debug() {
        let resp = ConfigResponse {
            node: NodeConfigResponse {
                name: "debug".into(),
                port: 7433,
                data_dir: "/tmp".into(),
            },
            auth: AuthConfigResponse {
                bind: BindMode::Local,
            },
            peers: HashMap::new(),
            guards: GuardDefaultConfigResponse {
                preset: GuardPreset::Standard,
                env: None,
            },
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
        };
        let debug = format!("{resp:?}");
        assert!(debug.contains("test"));
    }

    #[test]
    fn test_guard_default_config_response_debug() {
        let resp = GuardDefaultConfigResponse {
            preset: GuardPreset::Yolo,
            env: None,
        };
        let debug = format!("{resp:?}");
        assert!(debug.contains("Yolo"));
    }

    #[test]
    fn test_update_config_request_deserialize() {
        let json = r#"{"node_name":"new","port":9999}"#;
        let req: UpdateConfigRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.node_name, Some("new".into()));
        assert_eq!(req.port, Some(9999));
        assert!(req.data_dir.is_none());
        assert!(req.bind.is_none());
        assert!(req.guard_preset.is_none());
        assert!(req.guard_env.is_none());
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
            port: None,
            data_dir: None,
            bind: None,
            guard_preset: None,
            guard_env: None,
            peers: None,
        };
        let debug = format!("{req:?}");
        assert!(debug.contains("test"));
    }

    #[test]
    fn test_update_config_response_serialize() {
        let resp = UpdateConfigResponse {
            config: ConfigResponse {
                node: NodeConfigResponse {
                    name: "test".into(),
                    port: 7433,
                    data_dir: "/tmp".into(),
                },
                auth: AuthConfigResponse {
                    bind: BindMode::Local,
                },
                peers: HashMap::new(),
                guards: GuardDefaultConfigResponse {
                    preset: GuardPreset::Standard,
                    env: None,
                },
            },
            restart_required: true,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"restart_required\":true"));
    }

    #[test]
    fn test_update_config_response_debug() {
        let resp = UpdateConfigResponse {
            config: ConfigResponse {
                node: NodeConfigResponse {
                    name: "test".into(),
                    port: 7433,
                    data_dir: "/tmp".into(),
                },
                auth: AuthConfigResponse {
                    bind: BindMode::Local,
                },
                peers: HashMap::new(),
                guards: GuardDefaultConfigResponse {
                    preset: GuardPreset::Standard,
                    env: None,
                },
            },
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
            },
            auth: AuthConfigResponse {
                bind: BindMode::Local,
            },
            peers,
            guards: GuardDefaultConfigResponse {
                preset: GuardPreset::Standard,
                env: None,
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("remote"));
    }

    #[test]
    fn test_config_response_with_env_filter() {
        let resp = ConfigResponse {
            node: NodeConfigResponse {
                name: "n".into(),
                port: 7433,
                data_dir: "/d".into(),
            },
            auth: AuthConfigResponse {
                bind: BindMode::Local,
            },
            peers: HashMap::new(),
            guards: GuardDefaultConfigResponse {
                preset: GuardPreset::Standard,
                env: Some(EnvFilter {
                    allow: vec!["PATH".into()],
                    deny: vec!["SECRET".into()],
                }),
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("PATH"));
        assert!(json.contains("SECRET"));
    }

    #[test]
    fn test_update_config_request_with_all_fields() {
        let json = r#"{"node_name":"new","port":9999,"data_dir":"/d","bind":"lan","guard_preset":"strict","guard_env":{"allow":["PATH"],"deny":[]},"peers":{"remote":"10.0.0.1:7433"}}"#;
        let req: UpdateConfigRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.node_name, Some("new".into()));
        assert_eq!(req.port, Some(9999));
        assert_eq!(req.data_dir, Some("/d".into()));
        assert_eq!(req.bind, Some(BindMode::Lan));
        assert_eq!(req.guard_preset, Some(GuardPreset::Strict));
        assert!(req.guard_env.is_some());
        assert!(req.peers.is_some());
    }

    #[test]
    fn test_create_session_request_deserialize() {
        let json = r#"{"workdir":"/tmp/repo","prompt":"Fix bug"}"#;
        let req: CreateSessionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.workdir, "/tmp/repo");
        assert_eq!(req.prompt, "Fix bug");
        assert!(req.name.is_none());
        assert!(req.provider.is_none());
        assert!(req.mode.is_none());
        assert!(req.model.is_none());
        assert!(req.allowed_tools.is_none());
        assert!(req.system_prompt.is_none());
        assert!(req.metadata.is_none());
        assert!(req.persona.is_none());
    }

    #[test]
    fn test_create_session_request_with_all_fields() {
        let json = r#"{"name":"my-session","workdir":"/repo","provider":"claude","prompt":"Do it","mode":"autonomous","model":"opus","allowed_tools":["Read","Grep"],"system_prompt":"Be concise","metadata":{"discord_channel":"123"},"persona":"coder"}"#;
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
        assert_eq!(req.persona, Some("coder".into()));
    }

    #[test]
    fn test_create_session_request_missing_required() {
        let json = r#"{"workdir":"/tmp"}"#;
        let result = serde_json::from_str::<CreateSessionRequest>(json);
        assert!(result.is_err()); // missing "prompt"
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
            workdir: "/tmp".into(),
            provider: None,
            prompt: "test".into(),
            mode: None,
            guard_preset: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
        };
        let debug = format!("{req:?}");
        assert!(debug.contains("/tmp"));
    }

    #[test]
    fn test_create_session_request_with_interactive_mode() {
        let json = r#"{"workdir":"/repo","prompt":"test","mode":"interactive"}"#;
        let req: CreateSessionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.mode, Some(SessionMode::Interactive));
    }

    #[test]
    fn test_create_session_request_with_guard_preset() {
        let json = r#"{"workdir":"/repo","prompt":"test","guard_preset":"strict"}"#;
        let req: CreateSessionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.guard_preset, Some(crate::guard::GuardPreset::Strict));
        assert!(req.guard_config.is_none());
    }

    #[test]
    fn test_create_session_request_with_guard_config() {
        let json = r#"{"workdir":"/repo","prompt":"test","guard_config":{"preset":"standard"}}"#;
        let req: CreateSessionRequest = serde_json::from_str(json).unwrap();
        assert!(req.guard_config.is_some());
        assert!(req.guard_preset.is_none());
    }

    #[test]
    fn test_create_session_request_with_legacy_guard_config() {
        let json = r#"{"workdir":"/repo","prompt":"test","guard_config":{"file_write":"repo_only","file_read":"workspace","shell":"restricted","network":true,"install_packages":false,"git_push":false}}"#;
        let req: CreateSessionRequest = serde_json::from_str(json).unwrap();
        assert!(req.guard_config.is_some());
        let gc = req.guard_config.unwrap();
        assert_eq!(gc.preset, crate::guard::GuardPreset::Standard);
    }

    #[test]
    fn test_create_session_request_without_guard_fields() {
        let json = r#"{"workdir":"/repo","prompt":"test"}"#;
        let req: CreateSessionRequest = serde_json::from_str(json).unwrap();
        assert!(req.guard_preset.is_none());
        assert!(req.guard_config.is_none());
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
        let resp = AuthConfigResponse {
            bind: BindMode::Local,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert_eq!(json, r#"{"bind":"local"}"#);
    }

    #[test]
    fn test_auth_config_response_serialize_lan() {
        let resp = AuthConfigResponse {
            bind: BindMode::Lan,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert_eq!(json, r#"{"bind":"lan"}"#);
    }

    #[test]
    fn test_auth_config_response_deserialize() {
        let json = r#"{"bind":"local"}"#;
        let resp: AuthConfigResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.bind, BindMode::Local);
    }

    #[test]
    fn test_auth_config_response_deserialize_lan() {
        let json = r#"{"bind":"lan"}"#;
        let resp: AuthConfigResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.bind, BindMode::Lan);
    }

    #[test]
    fn test_auth_config_response_roundtrip() {
        let original = AuthConfigResponse {
            bind: BindMode::Lan,
        };
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: AuthConfigResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.bind, BindMode::Lan);
    }

    #[test]
    fn test_auth_config_response_debug() {
        let resp = AuthConfigResponse {
            bind: BindMode::Local,
        };
        let debug = format!("{resp:?}");
        assert!(debug.contains("Local"));
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

}
