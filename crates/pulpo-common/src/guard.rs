use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GuardPreset {
    Strict,
    Standard,
    Yolo,
}

impl fmt::Display for GuardPreset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Strict => write!(f, "strict"),
            Self::Standard => write!(f, "standard"),
            Self::Yolo => write!(f, "yolo"),
        }
    }
}

impl FromStr for GuardPreset {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "strict" => Ok(Self::Strict),
            "standard" => Ok(Self::Standard),
            "yolo" => Ok(Self::Yolo),
            other => Err(format!("unknown guard preset: {other}")),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EnvFilter {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct GuardConfig {
    pub preset: GuardPreset,
    #[serde(default)]
    pub env: EnvFilter,
}

impl Default for GuardConfig {
    fn default() -> Self {
        Self {
            preset: GuardPreset::Standard,
            env: EnvFilter::default(),
        }
    }
}

/// Custom deserializer that handles both the new format and the legacy DB format.
///
/// New format: `{"preset": "standard", "env": {...}}`
/// Legacy format: `{"file_write": "repo_only", "file_read": "workspace", "shell": "restricted",
///   "network": true, "install_packages": false, "git_push": false, "env": {...}}`
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
            // Common field
            #[serde(default)]
            env: EnvFilter,
        }

        let raw = RawGuardConfig::deserialize(deserializer)?;

        if let Some(preset) = raw.preset {
            // New format
            return Ok(Self {
                preset,
                env: raw.env,
            });
        }

        // Legacy format — infer preset from fields
        let shell = raw.shell.as_deref().unwrap_or("restricted");
        let network = raw.network.unwrap_or(true);
        let install_packages = raw.install_packages.unwrap_or(false);
        let git_push = raw.git_push.unwrap_or(false);

        let preset = if shell == "unrestricted" && network && install_packages && git_push {
            GuardPreset::Yolo
        } else if shell == "none" {
            GuardPreset::Strict
        } else {
            GuardPreset::Standard
        };

        Ok(Self {
            preset,
            env: raw.env,
        })
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
            serde_json::to_string(&GuardPreset::Yolo).unwrap(),
            "\"yolo\""
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
            serde_json::from_str::<GuardPreset>("\"yolo\"").unwrap(),
            GuardPreset::Yolo
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
        assert_eq!(GuardPreset::Yolo.to_string(), "yolo");
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
        assert_eq!("yolo".parse::<GuardPreset>().unwrap(), GuardPreset::Yolo);
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
        assert_eq!(format!("{:?}", GuardPreset::Yolo), "Yolo");
    }

    // EnvFilter tests

    #[test]
    fn test_env_filter_default() {
        let f = EnvFilter::default();
        assert!(f.allow.is_empty());
        assert!(f.deny.is_empty());
    }

    #[test]
    fn test_env_filter_serialize() {
        let f = EnvFilter {
            allow: vec!["PATH".into(), "HOME".into()],
            deny: vec!["AWS_*".into()],
        };
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("PATH"));
        assert!(json.contains("AWS_*"));
    }

    #[test]
    fn test_env_filter_deserialize() {
        let json = r#"{"allow":["PATH"],"deny":["SECRET"]}"#;
        let f: EnvFilter = serde_json::from_str(json).unwrap();
        assert_eq!(f.allow, vec!["PATH"]);
        assert_eq!(f.deny, vec!["SECRET"]);
    }

    #[test]
    fn test_env_filter_roundtrip() {
        let f = EnvFilter {
            allow: vec!["A".into()],
            deny: vec!["B".into()],
        };
        let json = serde_json::to_string(&f).unwrap();
        let f2: EnvFilter = serde_json::from_str(&json).unwrap();
        assert_eq!(f, f2);
    }

    #[test]
    fn test_env_filter_clone() {
        let f = EnvFilter {
            allow: vec!["X".into()],
            deny: vec!["Y".into()],
        };
        #[allow(clippy::redundant_clone)]
        let f2 = f.clone();
        assert_eq!(f, f2);
    }

    #[test]
    fn test_env_filter_debug() {
        let f = EnvFilter::default();
        let debug = format!("{f:?}");
        assert!(debug.contains("EnvFilter"));
    }

    #[test]
    fn test_env_filter_deserialize_empty_object() {
        let json = "{}";
        let f: EnvFilter = serde_json::from_str(json).unwrap();
        assert!(f.allow.is_empty());
        assert!(f.deny.is_empty());
    }

    // GuardConfig tests

    #[test]
    fn test_guard_config_default_is_standard() {
        let config = GuardConfig::default();
        assert_eq!(config.preset, GuardPreset::Standard);
        assert_eq!(config.env, EnvFilter::default());
    }

    #[test]
    fn test_guard_config_strict() {
        let config = GuardConfig {
            preset: GuardPreset::Strict,
            env: EnvFilter::default(),
        };
        assert_eq!(config.preset, GuardPreset::Strict);
    }

    #[test]
    fn test_guard_config_yolo() {
        let config = GuardConfig {
            preset: GuardPreset::Yolo,
            env: EnvFilter::default(),
        };
        assert_eq!(config.preset, GuardPreset::Yolo);
    }

    #[test]
    fn test_guard_config_serialize_roundtrip() {
        let config = GuardConfig {
            preset: GuardPreset::Strict,
            env: EnvFilter::default(),
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: GuardConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_guard_config_serialize_with_env() {
        let config = GuardConfig {
            preset: GuardPreset::Standard,
            env: EnvFilter {
                allow: vec!["PATH".into()],
                deny: vec!["AWS_*".into()],
            },
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: GuardConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
        assert_eq!(deserialized.env.allow, vec!["PATH"]);
        assert_eq!(deserialized.env.deny, vec!["AWS_*"]);
    }

    #[test]
    fn test_guard_config_clone() {
        let config = GuardConfig {
            preset: GuardPreset::Yolo,
            env: EnvFilter::default(),
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
        let json = r#"{"preset":"strict","env":{"allow":["PATH"],"deny":[]}}"#;
        let config: GuardConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.preset, GuardPreset::Strict);
        assert_eq!(config.env.allow, vec!["PATH"]);
    }

    #[test]
    fn test_guard_config_deserialize_new_format_without_env() {
        let json = r#"{"preset":"yolo"}"#;
        let config: GuardConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.preset, GuardPreset::Yolo);
        assert_eq!(config.env, EnvFilter::default());
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
        assert_eq!(config.env, EnvFilter::default());
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
    fn test_guard_config_deserialize_legacy_yolo() {
        let json = r#"{
            "file_write": "unrestricted",
            "file_read": "unrestricted",
            "shell": "unrestricted",
            "network": true,
            "install_packages": true,
            "git_push": true
        }"#;
        let config: GuardConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.preset, GuardPreset::Yolo);
    }

    #[test]
    fn test_guard_config_deserialize_legacy_with_env() {
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
        assert_eq!(config.env.allow, vec!["PATH"]);
        assert_eq!(config.env.deny, vec!["SECRET"]);
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
