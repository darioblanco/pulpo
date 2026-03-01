use std::fmt;

use serde::{Deserialize, Serialize};

/// How the daemon binds to the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BindMode {
    /// Bind to `127.0.0.1` — only reachable from the local machine.
    #[default]
    Local,
    /// Bind to `0.0.0.0` — reachable from the LAN (requires auth token).
    Lan,
}

impl fmt::Display for BindMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local => write!(f, "local"),
            Self::Lan => write!(f, "lan"),
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
        assert_eq!(serde_json::to_string(&BindMode::Lan).unwrap(), "\"lan\"");
    }

    #[test]
    fn test_bind_mode_deserialize() {
        assert_eq!(
            serde_json::from_str::<BindMode>("\"local\"").unwrap(),
            BindMode::Local
        );
        assert_eq!(
            serde_json::from_str::<BindMode>("\"lan\"").unwrap(),
            BindMode::Lan
        );
    }

    #[test]
    fn test_bind_mode_invalid_deserialize() {
        assert!(serde_json::from_str::<BindMode>("\"invalid\"").is_err());
    }

    #[test]
    fn test_bind_mode_display() {
        assert_eq!(BindMode::Local.to_string(), "local");
        assert_eq!(BindMode::Lan.to_string(), "lan");
    }

    #[test]
    fn test_bind_mode_debug() {
        assert_eq!(format!("{:?}", BindMode::Local), "Local");
        assert_eq!(format!("{:?}", BindMode::Lan), "Lan");
    }

    #[test]
    fn test_bind_mode_clone_and_copy() {
        let mode = BindMode::Lan;
        let mode2 = mode;
        #[allow(clippy::clone_on_copy)]
        let mode3 = mode.clone();
        assert_eq!(mode, mode2);
        assert_eq!(mode, mode3);
    }

    #[test]
    fn test_bind_mode_roundtrip() {
        for mode in [BindMode::Local, BindMode::Lan] {
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
            bind: BindMode::Lan,
        };
        let toml_str = toml::to_string(&w).unwrap();
        assert!(toml_str.contains("lan"));
        let parsed: Wrapper = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.bind, BindMode::Lan);
    }
}
