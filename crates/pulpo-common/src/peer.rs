use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::node::NodeInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PeerStatus {
    Online,
    Offline,
    Unknown,
}

impl fmt::Display for PeerStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Online => write!(f, "online"),
            Self::Offline => write!(f, "offline"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

impl FromStr for PeerStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "online" => Ok(Self::Online),
            "offline" => Ok(Self::Offline),
            "unknown" => Ok(Self::Unknown),
            other => Err(format!("unknown peer status: {other}")),
        }
    }
}

/// Configuration entry for a peer — supports both simple `"host:port"` strings
/// and structured `{ address, token }` tables for backward compatibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PeerEntry {
    /// Simple `"host:port"` string (backward-compatible).
    Simple(String),
    /// Structured entry with optional auth token.
    Full {
        address: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token: Option<String>,
    },
}

impl PeerEntry {
    /// The peer's `host:port` address.
    pub fn address(&self) -> &str {
        match self {
            Self::Simple(addr) => addr,
            Self::Full { address, .. } => address,
        }
    }

    /// Optional authentication token for this peer.
    pub fn token(&self) -> Option<&str> {
        match self {
            Self::Simple(_) => None,
            Self::Full { token, .. } => token.as_deref(),
        }
    }
}

/// How a peer was discovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PeerSource {
    /// Peer was explicitly listed in config.
    #[default]
    Configured,
    /// Peer was discovered via mDNS or similar.
    Discovered,
}

impl fmt::Display for PeerSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configured => write!(f, "configured"),
            Self::Discovered => write!(f, "discovered"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PeerInfo {
    pub name: String,
    pub address: String,
    pub status: PeerStatus,
    pub node_info: Option<NodeInfo>,
    pub session_count: Option<usize>,
    #[serde(default)]
    pub source: PeerSource,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_status_serialize() {
        assert_eq!(
            serde_json::to_string(&PeerStatus::Online).unwrap(),
            "\"online\""
        );
        assert_eq!(
            serde_json::to_string(&PeerStatus::Offline).unwrap(),
            "\"offline\""
        );
        assert_eq!(
            serde_json::to_string(&PeerStatus::Unknown).unwrap(),
            "\"unknown\""
        );
    }

    #[test]
    fn test_peer_status_deserialize() {
        assert_eq!(
            serde_json::from_str::<PeerStatus>("\"online\"").unwrap(),
            PeerStatus::Online
        );
        assert_eq!(
            serde_json::from_str::<PeerStatus>("\"offline\"").unwrap(),
            PeerStatus::Offline
        );
        assert_eq!(
            serde_json::from_str::<PeerStatus>("\"unknown\"").unwrap(),
            PeerStatus::Unknown
        );
    }

    #[test]
    fn test_peer_status_invalid_deserialize() {
        assert!(serde_json::from_str::<PeerStatus>("\"invalid\"").is_err());
    }

    #[test]
    fn test_peer_status_display() {
        assert_eq!(PeerStatus::Online.to_string(), "online");
        assert_eq!(PeerStatus::Offline.to_string(), "offline");
        assert_eq!(PeerStatus::Unknown.to_string(), "unknown");
    }

    #[test]
    fn test_peer_status_from_str() {
        assert_eq!("online".parse::<PeerStatus>().unwrap(), PeerStatus::Online);
        assert_eq!(
            "offline".parse::<PeerStatus>().unwrap(),
            PeerStatus::Offline
        );
        assert_eq!(
            "unknown".parse::<PeerStatus>().unwrap(),
            PeerStatus::Unknown
        );
    }

    #[test]
    fn test_peer_status_from_str_invalid() {
        let err = "invalid".parse::<PeerStatus>().unwrap_err();
        assert!(err.contains("unknown peer status"));
    }

    #[test]
    fn test_peer_status_clone_and_copy() {
        let s = PeerStatus::Online;
        let s2 = s;
        #[allow(clippy::clone_on_copy)]
        let s3 = s.clone();
        assert_eq!(s, s2);
        assert_eq!(s, s3);
    }

    #[test]
    fn test_peer_status_debug() {
        assert_eq!(format!("{:?}", PeerStatus::Online), "Online");
        assert_eq!(format!("{:?}", PeerStatus::Offline), "Offline");
        assert_eq!(format!("{:?}", PeerStatus::Unknown), "Unknown");
    }

    fn make_peer_info() -> PeerInfo {
        PeerInfo {
            name: "win-pc".into(),
            address: "192.168.1.100:7433".into(),
            status: PeerStatus::Online,
            node_info: Some(NodeInfo {
                name: "win-pc".into(),
                hostname: "DESKTOP-ABC".into(),
                os: "wsl2".into(),
                arch: "x86_64".into(),
                cpus: 16,
                memory_mb: 32768,
                gpu: Some("RTX 5090".into()),
            }),
            session_count: Some(3),
            source: PeerSource::Configured,
        }
    }

    #[test]
    fn test_peer_info_serialize() {
        let info = make_peer_info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"name\":\"win-pc\""));
        assert!(json.contains("\"address\":\"192.168.1.100:7433\""));
        assert!(json.contains("\"status\":\"online\""));
        assert!(json.contains("\"session_count\":3"));
    }

    #[test]
    fn test_peer_info_deserialize() {
        let json = r#"{"name":"mac","address":"10.0.0.1:7433","status":"offline","node_info":null,"session_count":null}"#;
        let info: PeerInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.name, "mac");
        assert_eq!(info.address, "10.0.0.1:7433");
        assert_eq!(info.status, PeerStatus::Offline);
        assert!(info.node_info.is_none());
        assert!(info.session_count.is_none());
    }

    #[test]
    fn test_peer_info_roundtrip() {
        let info = make_peer_info();
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: PeerInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, info.name);
        assert_eq!(deserialized.address, info.address);
        assert_eq!(deserialized.status, info.status);
        assert_eq!(deserialized.session_count, info.session_count);
    }

    #[test]
    fn test_peer_info_debug() {
        let info = make_peer_info();
        let debug = format!("{info:?}");
        assert!(debug.contains("win-pc"));
        assert!(debug.contains("Online"));
    }

    #[test]
    fn test_peer_info_clone() {
        let info = make_peer_info();
        #[allow(clippy::redundant_clone)]
        let cloned = info.clone();
        assert_eq!(cloned.name, "win-pc");
        assert_eq!(cloned.status, PeerStatus::Online);
        assert_eq!(cloned.session_count, Some(3));
    }

    #[test]
    fn test_peer_info_with_no_optionals() {
        let info = PeerInfo {
            name: "bare".into(),
            address: "host:7433".into(),
            status: PeerStatus::Unknown,
            node_info: None,
            session_count: None,
            source: PeerSource::Configured,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"node_info\":null"));
        assert!(json.contains("\"session_count\":null"));
        assert!(json.contains("\"unknown\""));
    }

    // -- PeerEntry tests --

    #[test]
    fn test_peer_entry_simple_json() {
        let entry = PeerEntry::Simple("10.0.0.1:7433".into());
        let json = serde_json::to_string(&entry).unwrap();
        assert_eq!(json, "\"10.0.0.1:7433\"");
        let parsed: PeerEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, entry);
    }

    #[test]
    fn test_peer_entry_full_json() {
        let entry = PeerEntry::Full {
            address: "10.0.0.1:7433".into(),
            token: Some("secret".into()),
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"address\""));
        assert!(json.contains("\"token\""));
        let parsed: PeerEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, entry);
    }

    #[test]
    fn test_peer_entry_full_no_token_json() {
        let entry = PeerEntry::Full {
            address: "host:7433".into(),
            token: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        // token should be skipped
        assert!(!json.contains("token"));
        assert!(json.contains("\"address\""));
    }

    #[test]
    fn test_peer_entry_simple_toml() {
        let mut map = std::collections::HashMap::new();
        map.insert("mac".to_owned(), PeerEntry::Simple("mac:7433".into()));
        let toml_str = toml::to_string(&map).unwrap();
        assert!(toml_str.contains("mac = \"mac:7433\""));
    }

    #[test]
    fn test_peer_entry_full_toml() {
        #[derive(Serialize, Deserialize)]
        struct Wrap {
            peers: std::collections::HashMap<String, PeerEntry>,
        }
        let mut peers = std::collections::HashMap::new();
        peers.insert(
            "win".to_owned(),
            PeerEntry::Full {
                address: "win:7433".into(),
                token: Some("tok".into()),
            },
        );
        let w = Wrap { peers };
        let toml_str = toml::to_string(&w).unwrap();
        assert!(toml_str.contains("[peers.win]"));
        assert!(toml_str.contains("address = \"win:7433\""));
        assert!(toml_str.contains("token = \"tok\""));
    }

    #[test]
    fn test_peer_entry_mixed_toml_roundtrip() {
        #[derive(Deserialize)]
        struct Wrap {
            peers: std::collections::HashMap<String, PeerEntry>,
        }
        let toml_str = r#"
[peers]
mac = "mac:7433"

[peers.win]
address = "win:7433"
token = "secret"
"#;
        let w: Wrap = toml::from_str(toml_str).unwrap();
        assert_eq!(w.peers["mac"].address(), "mac:7433");
        assert_eq!(w.peers["mac"].token(), None);
        assert_eq!(w.peers["win"].address(), "win:7433");
        assert_eq!(w.peers["win"].token(), Some("secret"));
    }

    #[test]
    fn test_peer_entry_address() {
        assert_eq!(PeerEntry::Simple("h:1".into()).address(), "h:1");
        assert_eq!(
            PeerEntry::Full {
                address: "h:2".into(),
                token: None
            }
            .address(),
            "h:2"
        );
    }

    #[test]
    fn test_peer_entry_token() {
        assert_eq!(PeerEntry::Simple("h:1".into()).token(), None);
        assert_eq!(
            PeerEntry::Full {
                address: "h:2".into(),
                token: Some("t".into())
            }
            .token(),
            Some("t")
        );
        assert_eq!(
            PeerEntry::Full {
                address: "h:3".into(),
                token: None
            }
            .token(),
            None
        );
    }

    #[test]
    fn test_peer_entry_debug() {
        let entry = PeerEntry::Simple("x:1".into());
        let dbg = format!("{entry:?}");
        assert!(dbg.contains("Simple"));
    }

    #[test]
    fn test_peer_entry_clone() {
        let entry = PeerEntry::Full {
            address: "a:1".into(),
            token: Some("t".into()),
        };
        #[allow(clippy::redundant_clone)]
        let cloned = entry.clone();
        assert_eq!(cloned, entry);
    }

    #[test]
    fn test_peer_entry_eq() {
        let a = PeerEntry::Simple("h:1".into());
        let b = PeerEntry::Simple("h:1".into());
        let c = PeerEntry::Simple("h:2".into());
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    // -- PeerSource tests --

    #[test]
    fn test_peer_source_default() {
        assert_eq!(PeerSource::default(), PeerSource::Configured);
    }

    #[test]
    fn test_peer_source_display() {
        assert_eq!(PeerSource::Configured.to_string(), "configured");
        assert_eq!(PeerSource::Discovered.to_string(), "discovered");
    }

    #[test]
    fn test_peer_source_serialize() {
        assert_eq!(
            serde_json::to_string(&PeerSource::Configured).unwrap(),
            "\"configured\""
        );
        assert_eq!(
            serde_json::to_string(&PeerSource::Discovered).unwrap(),
            "\"discovered\""
        );
    }

    #[test]
    fn test_peer_source_deserialize() {
        assert_eq!(
            serde_json::from_str::<PeerSource>("\"configured\"").unwrap(),
            PeerSource::Configured
        );
        assert_eq!(
            serde_json::from_str::<PeerSource>("\"discovered\"").unwrap(),
            PeerSource::Discovered
        );
    }

    #[test]
    fn test_peer_source_debug() {
        assert_eq!(format!("{:?}", PeerSource::Configured), "Configured");
        assert_eq!(format!("{:?}", PeerSource::Discovered), "Discovered");
    }

    #[test]
    fn test_peer_source_clone_and_copy() {
        let s = PeerSource::Discovered;
        let s2 = s;
        #[allow(clippy::clone_on_copy)]
        let s3 = s.clone();
        assert_eq!(s, s2);
        assert_eq!(s, s3);
    }

    #[test]
    fn test_peer_source_eq() {
        assert_eq!(PeerSource::Configured, PeerSource::Configured);
        assert_eq!(PeerSource::Discovered, PeerSource::Discovered);
        assert_ne!(PeerSource::Configured, PeerSource::Discovered);
    }

    #[test]
    fn test_peer_info_source_default_on_deserialize() {
        // JSON without "source" field should default to Configured
        let json = r#"{"name":"old","address":"h:1","status":"online","node_info":null,"session_count":null}"#;
        let info: PeerInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.source, PeerSource::Configured);
    }

    #[test]
    fn test_peer_info_source_roundtrip() {
        let info = PeerInfo {
            name: "disc".into(),
            address: "h:1".into(),
            status: PeerStatus::Online,
            node_info: None,
            session_count: None,
            source: PeerSource::Discovered,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"source\":\"discovered\""));
        let parsed: PeerInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.source, PeerSource::Discovered);
    }
}
