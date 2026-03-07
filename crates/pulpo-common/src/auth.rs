use std::fmt;

use serde::{Deserialize, Serialize};

/// How the daemon binds to the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BindMode {
    /// Bind to `127.0.0.1` — only reachable from the local machine. No discovery, no auth.
    #[default]
    Local,
    /// Bind to the Tailscale interface IP — reachable only over tailnet.
    /// Discovery via Tailscale API. No auth token (Tailscale handles authentication).
    Tailscale,
    /// Bind to `0.0.0.0` — reachable from the network (requires auth token).
    /// Discovery via mDNS or seed.
    Public,
    /// Bind to `0.0.0.0` — for container environments (no auth, trusts container network isolation).
    Container,
}

impl fmt::Display for BindMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local => write!(f, "local"),
            Self::Tailscale => write!(f, "tailscale"),
            Self::Public => write!(f, "public"),
            Self::Container => write!(f, "container"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bind_mode_default_is_local() {
        assert_eq!(BindMode::default(), BindMode::Local);
    }

    #[test]
    fn test_bind_mode_serialize() {
        assert_eq!(
            serde_json::to_string(&BindMode::Local).unwrap(),
            "\"local\""
        );
        assert_eq!(
            serde_json::to_string(&BindMode::Tailscale).unwrap(),
            "\"tailscale\""
        );
        assert_eq!(
            serde_json::to_string(&BindMode::Public).unwrap(),
            "\"public\""
        );
        assert_eq!(
            serde_json::to_string(&BindMode::Container).unwrap(),
            "\"container\""
        );
    }

    #[test]
    fn test_bind_mode_deserialize() {
        assert_eq!(
            serde_json::from_str::<BindMode>("\"local\"").unwrap(),
            BindMode::Local
        );
        assert_eq!(
            serde_json::from_str::<BindMode>("\"tailscale\"").unwrap(),
            BindMode::Tailscale
        );
        assert_eq!(
            serde_json::from_str::<BindMode>("\"public\"").unwrap(),
            BindMode::Public
        );
        assert_eq!(
            serde_json::from_str::<BindMode>("\"container\"").unwrap(),
            BindMode::Container
        );
    }

    #[test]
    fn test_bind_mode_invalid_deserialize() {
        assert!(serde_json::from_str::<BindMode>("\"invalid\"").is_err());
    }

    #[test]
    fn test_bind_mode_display() {
        assert_eq!(BindMode::Local.to_string(), "local");
        assert_eq!(BindMode::Tailscale.to_string(), "tailscale");
        assert_eq!(BindMode::Public.to_string(), "public");
        assert_eq!(BindMode::Container.to_string(), "container");
    }

    #[test]
    fn test_bind_mode_debug() {
        assert_eq!(format!("{:?}", BindMode::Local), "Local");
        assert_eq!(format!("{:?}", BindMode::Tailscale), "Tailscale");
        assert_eq!(format!("{:?}", BindMode::Public), "Public");
        assert_eq!(format!("{:?}", BindMode::Container), "Container");
    }

    #[test]
    fn test_bind_mode_clone_and_copy() {
        let mode = BindMode::Public;
        let mode2 = mode;
        #[allow(clippy::clone_on_copy)]
        let mode3 = mode.clone();
        assert_eq!(mode, mode2);
        assert_eq!(mode, mode3);
    }

    #[test]
    fn test_bind_mode_roundtrip() {
        for mode in [
            BindMode::Local,
            BindMode::Tailscale,
            BindMode::Public,
            BindMode::Container,
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            let deserialized: BindMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, deserialized);
        }
    }

    #[test]
    fn test_bind_mode_toml_roundtrip() {
        #[derive(Serialize, Deserialize)]
        struct Wrapper {
            bind: BindMode,
        }
        let w = Wrapper {
            bind: BindMode::Public,
        };
        let toml_str = toml::to_string(&w).unwrap();
        assert!(toml_str.contains("public"));
        let parsed: Wrapper = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.bind, BindMode::Public);
    }

    #[test]
    fn test_bind_mode_toml_roundtrip_container() {
        #[derive(Serialize, Deserialize)]
        struct Wrapper {
            bind: BindMode,
        }
        let w = Wrapper {
            bind: BindMode::Container,
        };
        let toml_str = toml::to_string(&w).unwrap();
        assert!(toml_str.contains("container"));
        let parsed: Wrapper = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.bind, BindMode::Container);
    }

    #[test]
    fn test_bind_mode_toml_roundtrip_tailscale() {
        #[derive(Serialize, Deserialize)]
        struct Wrapper {
            bind: BindMode,
        }
        let w = Wrapper {
            bind: BindMode::Tailscale,
        };
        let toml_str = toml::to_string(&w).unwrap();
        assert!(toml_str.contains("tailscale"));
        let parsed: Wrapper = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.bind, BindMode::Tailscale);
    }
}
