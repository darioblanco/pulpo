use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GuardPreset {
    Strict,
    Standard,
    #[serde(alias = "yolo")]
    Unrestricted,
}

impl fmt::Display for GuardPreset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Strict => write!(f, "strict"),
            Self::Standard => write!(f, "standard"),
            Self::Unrestricted => write!(f, "unrestricted"),
        }
    }
}

impl FromStr for GuardPreset {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "strict" => Ok(Self::Strict),
            "standard" => Ok(Self::Standard),
            "unrestricted" | "yolo" => Ok(Self::Unrestricted),
            other => Err(format!("unknown guard preset: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct GuardConfig {
    pub preset: GuardPreset,
}

impl Default for GuardConfig {
    fn default() -> Self {
        Self {
            preset: GuardPreset::Standard,
        }
    }
}

/// Custom deserializer that handles both the new format and the legacy DB format.
///
/// New format: `{"preset": "standard"}`
/// Legacy format: `{"file_write": "repo_only", "file_read": "workspace", "shell": "restricted",
///   "network": true, "install_packages": false, "git_push": false, ...}`
impl<'de> Deserialize<'de> for GuardConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawGuardConfig {
            // New format field
            preset: Option<GuardPreset>,
            // Legacy format fields
            shell: Option<String>,
            network: Option<bool>,
            install_packages: Option<bool>,
            git_push: Option<bool>,
        }

        let raw = RawGuardConfig::deserialize(deserializer)?;

        if let Some(preset) = raw.preset {
            // New format
            return Ok(Self { preset });
        }

        // Legacy format — infer preset from fields
        let shell = raw.shell.as_deref().unwrap_or("restricted");
        let network = raw.network.unwrap_or(true);
        let install_packages = raw.install_packages.unwrap_or(false);
        let git_push = raw.git_push.unwrap_or(false);

        let preset = if shell == "unrestricted" && network && install_packages && git_push {
            GuardPreset::Unrestricted
        } else if shell == "none" {
            GuardPreset::Strict
        } else {
            GuardPreset::Standard
        };

        Ok(Self { preset })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // GuardPreset tests

    #[test]
    fn test_guard_preset_serialize() {
        assert_eq!(
            serde_json::to_string(&GuardPreset::Strict).unwrap(),
            "\"strict\""
        );
        assert_eq!(
            serde_json::to_string(&GuardPreset::Standard).unwrap(),
            "\"standard\""
        );
        assert_eq!(
            serde_json::to_string(&GuardPreset::Unrestricted).unwrap(),
            "\"unrestricted\""
        );
    }

    #[test]
    fn test_guard_preset_deserialize() {
        assert_eq!(
            serde_json::from_str::<GuardPreset>("\"strict\"").unwrap(),
            GuardPreset::Strict
        );
        assert_eq!(
            serde_json::from_str::<GuardPreset>("\"standard\"").unwrap(),
            GuardPreset::Standard
        );
        assert_eq!(
            serde_json::from_str::<GuardPreset>("\"unrestricted\"").unwrap(),
            GuardPreset::Unrestricted
        );
    }

    #[test]
    fn test_guard_preset_deserialize_yolo_alias() {
        // Backward compat: "yolo" in old DB rows maps to Unrestricted
        assert_eq!(
            serde_json::from_str::<GuardPreset>("\"yolo\"").unwrap(),
            GuardPreset::Unrestricted
        );
    }

    #[test]
    fn test_guard_preset_from_str_yolo_alias() {
        assert_eq!(
            "yolo".parse::<GuardPreset>().unwrap(),
            GuardPreset::Unrestricted
        );
    }

    #[test]
    fn test_guard_preset_invalid_deserialize() {
        assert!(serde_json::from_str::<GuardPreset>("\"invalid\"").is_err());
    }

    #[test]
    fn test_guard_preset_display() {
        assert_eq!(GuardPreset::Strict.to_string(), "strict");
        assert_eq!(GuardPreset::Standard.to_string(), "standard");
        assert_eq!(GuardPreset::Unrestricted.to_string(), "unrestricted");
    }

    #[test]
    fn test_guard_preset_from_str() {
        assert_eq!(
            "strict".parse::<GuardPreset>().unwrap(),
            GuardPreset::Strict
        );
        assert_eq!(
            "standard".parse::<GuardPreset>().unwrap(),
            GuardPreset::Standard
        );
        assert_eq!(
            "unrestricted".parse::<GuardPreset>().unwrap(),
            GuardPreset::Unrestricted
        );
    }

    #[test]
    fn test_guard_preset_from_str_invalid() {
        let err = "invalid".parse::<GuardPreset>().unwrap_err();
        assert!(err.contains("unknown guard preset"));
    }

    #[test]
    fn test_guard_preset_clone_and_copy() {
        let p = GuardPreset::Standard;
        let p2 = p;
        #[allow(clippy::clone_on_copy)]
        let p3 = p.clone();
        assert_eq!(p, p2);
        assert_eq!(p, p3);
    }

    #[test]
    fn test_guard_preset_debug() {
        assert_eq!(format!("{:?}", GuardPreset::Strict), "Strict");
        assert_eq!(format!("{:?}", GuardPreset::Standard), "Standard");
        assert_eq!(format!("{:?}", GuardPreset::Unrestricted), "Unrestricted");
    }

    // GuardConfig tests

    #[test]
    fn test_guard_config_default_is_standard() {
        let config = GuardConfig::default();
        assert_eq!(config.preset, GuardPreset::Standard);
    }

    #[test]
    fn test_guard_config_strict() {
        let config = GuardConfig {
            preset: GuardPreset::Strict,
        };
        assert_eq!(config.preset, GuardPreset::Strict);
    }

    #[test]
    fn test_guard_config_unrestricted() {
        let config = GuardConfig {
            preset: GuardPreset::Unrestricted,
        };
        assert_eq!(config.preset, GuardPreset::Unrestricted);
    }

    #[test]
    fn test_guard_config_serialize_roundtrip() {
        let config = GuardConfig {
            preset: GuardPreset::Strict,
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: GuardConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_guard_config_clone() {
        let config = GuardConfig {
            preset: GuardPreset::Unrestricted,
        };
        #[allow(clippy::redundant_clone)]
        let cloned = config.clone();
        assert_eq!(config, cloned);
    }

    #[test]
    fn test_guard_config_debug() {
        let config = GuardConfig::default();
        let debug = format!("{config:?}");
        assert!(debug.contains("GuardConfig"));
        assert!(debug.contains("Standard"));
    }

    // Backward-compatible deserialization tests

    #[test]
    fn test_guard_config_deserialize_new_format() {
        let json = r#"{"preset":"strict"}"#;
        let config: GuardConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.preset, GuardPreset::Strict);
    }

    #[test]
    fn test_guard_config_deserialize_new_format_ignores_env() {
        // Old DB rows may still have an env field — silently ignore it
        let json = r#"{"preset":"unrestricted","env":{"allow":["PATH"],"deny":[]}}"#;
        let config: GuardConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.preset, GuardPreset::Unrestricted);
    }

    #[test]
    fn test_guard_config_deserialize_legacy_standard() {
        let json = r#"{
            "file_write": "repo_only",
            "file_read": "workspace",
            "shell": "restricted",
            "network": true,
            "install_packages": false,
            "git_push": false
        }"#;
        let config: GuardConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.preset, GuardPreset::Standard);
    }

    #[test]
    fn test_guard_config_deserialize_legacy_strict() {
        let json = r#"{
            "file_write": "repo_only",
            "file_read": "repo_only",
            "shell": "none",
            "network": false,
            "install_packages": false,
            "git_push": false
        }"#;
        let config: GuardConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.preset, GuardPreset::Strict);
    }

    #[test]
    fn test_guard_config_deserialize_legacy_unrestricted() {
        let json = r#"{
            "file_write": "unrestricted",
            "file_read": "unrestricted",
            "shell": "unrestricted",
            "network": true,
            "install_packages": true,
            "git_push": true
        }"#;
        let config: GuardConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.preset, GuardPreset::Unrestricted);
    }

    #[test]
    fn test_guard_config_deserialize_legacy_with_env_ignored() {
        // Legacy rows with env field — env is silently ignored
        let json = r#"{
            "file_write": "repo_only",
            "file_read": "workspace",
            "shell": "restricted",
            "network": true,
            "install_packages": false,
            "git_push": false,
            "env": {"allow": ["PATH"], "deny": ["SECRET"]}
        }"#;
        let config: GuardConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.preset, GuardPreset::Standard);
    }

    #[test]
    fn test_guard_config_deserialize_legacy_partial_yolo_is_standard() {
        // Has unrestricted shell and network, but missing install_packages/git_push
        let json = r#"{
            "shell": "unrestricted",
            "network": true,
            "install_packages": false,
            "git_push": true
        }"#;
        let config: GuardConfig = serde_json::from_str(json).unwrap();
        // Not all yolo fields set, so falls to Standard
        assert_eq!(config.preset, GuardPreset::Standard);
    }
}
