use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct GuardConfig {
    pub unrestricted: bool,
}

/// Custom deserializer that handles the new format, the old preset format, and legacy DB rows.
///
/// New format: `{"unrestricted": true}`
/// Old preset format: `{"preset": "unrestricted"}` → `unrestricted: true`
/// Legacy format: `{"shell": "unrestricted", "network": true, ...}` → inferred
impl<'de> Deserialize<'de> for GuardConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawGuardConfig {
            // New format
            unrestricted: Option<bool>,
            // Old preset format
            preset: Option<String>,
            // Legacy format fields
            shell: Option<String>,
            network: Option<bool>,
            install_packages: Option<bool>,
            git_push: Option<bool>,
        }

        let raw = RawGuardConfig::deserialize(deserializer)?;

        // New format takes priority
        if let Some(unrestricted) = raw.unrestricted {
            return Ok(Self { unrestricted });
        }

        // Old preset format
        if let Some(ref preset) = raw.preset {
            let unrestricted = matches!(preset.as_str(), "unrestricted" | "yolo");
            return Ok(Self { unrestricted });
        }

        // Legacy format — infer from fields
        let shell = raw.shell.as_deref().unwrap_or("restricted");
        let network = raw.network.unwrap_or(true);
        let install_packages = raw.install_packages.unwrap_or(false);
        let git_push = raw.git_push.unwrap_or(false);

        let unrestricted = shell == "unrestricted" && network && install_packages && git_push;

        Ok(Self { unrestricted })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guard_config_default_is_restricted() {
        let config = GuardConfig::default();
        assert!(!config.unrestricted);
    }

    #[test]
    fn test_guard_config_unrestricted() {
        let config = GuardConfig { unrestricted: true };
        assert!(config.unrestricted);
    }

    #[test]
    fn test_guard_config_serialize_roundtrip() {
        let config = GuardConfig { unrestricted: true };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"unrestricted\":true"));
        let deserialized: GuardConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_guard_config_clone() {
        let config = GuardConfig { unrestricted: true };
        #[allow(clippy::redundant_clone)]
        let cloned = config.clone();
        assert_eq!(config, cloned);
    }

    #[test]
    fn test_guard_config_debug() {
        let config = GuardConfig::default();
        let debug = format!("{config:?}");
        assert!(debug.contains("GuardConfig"));
        assert!(debug.contains("false"));
    }

    // Backward-compatible deserialization tests

    #[test]
    fn test_guard_config_deserialize_new_format() {
        let json = r#"{"unrestricted":false}"#;
        let config: GuardConfig = serde_json::from_str(json).unwrap();
        assert!(!config.unrestricted);
    }

    #[test]
    fn test_guard_config_deserialize_new_format_true() {
        let json = r#"{"unrestricted":true}"#;
        let config: GuardConfig = serde_json::from_str(json).unwrap();
        assert!(config.unrestricted);
    }

    #[test]
    fn test_guard_config_deserialize_old_preset_standard() {
        let json = r#"{"preset":"standard"}"#;
        let config: GuardConfig = serde_json::from_str(json).unwrap();
        assert!(!config.unrestricted);
    }

    #[test]
    fn test_guard_config_deserialize_old_preset_strict() {
        let json = r#"{"preset":"strict"}"#;
        let config: GuardConfig = serde_json::from_str(json).unwrap();
        assert!(!config.unrestricted);
    }

    #[test]
    fn test_guard_config_deserialize_old_preset_unrestricted() {
        let json = r#"{"preset":"unrestricted"}"#;
        let config: GuardConfig = serde_json::from_str(json).unwrap();
        assert!(config.unrestricted);
    }

    #[test]
    fn test_guard_config_deserialize_old_preset_yolo() {
        let json = r#"{"preset":"yolo"}"#;
        let config: GuardConfig = serde_json::from_str(json).unwrap();
        assert!(config.unrestricted);
    }

    #[test]
    fn test_guard_config_deserialize_old_preset_ignores_env() {
        let json = r#"{"preset":"unrestricted","env":{"allow":["PATH"],"deny":[]}}"#;
        let config: GuardConfig = serde_json::from_str(json).unwrap();
        assert!(config.unrestricted);
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
        assert!(!config.unrestricted);
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
        assert!(!config.unrestricted);
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
        assert!(config.unrestricted);
    }

    #[test]
    fn test_guard_config_deserialize_legacy_with_env_ignored() {
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
        assert!(!config.unrestricted);
    }

    #[test]
    fn test_guard_config_deserialize_legacy_partial_yolo_is_restricted() {
        let json = r#"{
            "shell": "unrestricted",
            "network": true,
            "install_packages": false,
            "git_push": true
        }"#;
        let config: GuardConfig = serde_json::from_str(json).unwrap();
        assert!(!config.unrestricted);
    }
}
