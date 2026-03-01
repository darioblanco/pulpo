use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FileScope {
    RepoOnly,
    Workspace,
    Unrestricted,
}

impl fmt::Display for FileScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RepoOnly => write!(f, "repo_only"),
            Self::Workspace => write!(f, "workspace"),
            Self::Unrestricted => write!(f, "unrestricted"),
        }
    }
}

impl FromStr for FileScope {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "repo_only" => Ok(Self::RepoOnly),
            "workspace" => Ok(Self::Workspace),
            "unrestricted" => Ok(Self::Unrestricted),
            other => Err(format!("unknown file scope: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShellAccess {
    None,
    Restricted,
    Unrestricted,
}

impl fmt::Display for ShellAccess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "none"),
            Self::Restricted => write!(f, "restricted"),
            Self::Unrestricted => write!(f, "unrestricted"),
        }
    }
}

impl FromStr for ShellAccess {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "none" => Ok(Self::None),
            "restricted" => Ok(Self::Restricted),
            "unrestricted" => Ok(Self::Unrestricted),
            other => Err(format!("unknown shell access: {other}")),
        }
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GuardConfig {
    pub file_write: FileScope,
    pub file_read: FileScope,
    pub shell: ShellAccess,
    pub network: bool,
    pub install_packages: bool,
    pub git_push: bool,
    #[serde(default)]
    pub env: EnvFilter,
}

impl GuardConfig {
    #[must_use]
    pub fn from_preset(preset: GuardPreset) -> Self {
        match preset {
            GuardPreset::Strict => Self {
                file_write: FileScope::RepoOnly,
                file_read: FileScope::RepoOnly,
                shell: ShellAccess::None,
                network: false,
                install_packages: false,
                git_push: false,
                env: EnvFilter::default(),
            },
            GuardPreset::Standard => Self {
                file_write: FileScope::RepoOnly,
                file_read: FileScope::Workspace,
                shell: ShellAccess::Restricted,
                network: true,
                install_packages: false,
                git_push: false,
                env: EnvFilter::default(),
            },
            GuardPreset::Yolo => Self {
                file_write: FileScope::Unrestricted,
                file_read: FileScope::Unrestricted,
                shell: ShellAccess::Unrestricted,
                network: true,
                install_packages: true,
                git_push: true,
                env: EnvFilter::default(),
            },
        }
    }
}

impl Default for GuardConfig {
    fn default() -> Self {
        Self::from_preset(GuardPreset::Standard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // FileScope tests

    #[test]
    fn test_file_scope_serialize() {
        assert_eq!(
            serde_json::to_string(&FileScope::RepoOnly).unwrap(),
            "\"repo_only\""
        );
        assert_eq!(
            serde_json::to_string(&FileScope::Workspace).unwrap(),
            "\"workspace\""
        );
        assert_eq!(
            serde_json::to_string(&FileScope::Unrestricted).unwrap(),
            "\"unrestricted\""
        );
    }

    #[test]
    fn test_file_scope_deserialize() {
        assert_eq!(
            serde_json::from_str::<FileScope>("\"repo_only\"").unwrap(),
            FileScope::RepoOnly
        );
        assert_eq!(
            serde_json::from_str::<FileScope>("\"workspace\"").unwrap(),
            FileScope::Workspace
        );
        assert_eq!(
            serde_json::from_str::<FileScope>("\"unrestricted\"").unwrap(),
            FileScope::Unrestricted
        );
    }

    #[test]
    fn test_file_scope_invalid_deserialize() {
        assert!(serde_json::from_str::<FileScope>("\"invalid\"").is_err());
    }

    #[test]
    fn test_file_scope_display() {
        assert_eq!(FileScope::RepoOnly.to_string(), "repo_only");
        assert_eq!(FileScope::Workspace.to_string(), "workspace");
        assert_eq!(FileScope::Unrestricted.to_string(), "unrestricted");
    }

    #[test]
    fn test_file_scope_from_str() {
        assert_eq!(
            "repo_only".parse::<FileScope>().unwrap(),
            FileScope::RepoOnly
        );
        assert_eq!(
            "workspace".parse::<FileScope>().unwrap(),
            FileScope::Workspace
        );
        assert_eq!(
            "unrestricted".parse::<FileScope>().unwrap(),
            FileScope::Unrestricted
        );
    }

    #[test]
    fn test_file_scope_from_str_invalid() {
        let err = "invalid".parse::<FileScope>().unwrap_err();
        assert!(err.contains("unknown file scope"));
    }

    #[test]
    fn test_file_scope_clone_and_copy() {
        let s = FileScope::RepoOnly;
        let s2 = s;
        #[allow(clippy::clone_on_copy)]
        let s3 = s.clone();
        assert_eq!(s, s2);
        assert_eq!(s, s3);
    }

    #[test]
    fn test_file_scope_debug() {
        assert_eq!(format!("{:?}", FileScope::RepoOnly), "RepoOnly");
        assert_eq!(format!("{:?}", FileScope::Workspace), "Workspace");
        assert_eq!(format!("{:?}", FileScope::Unrestricted), "Unrestricted");
    }

    // ShellAccess tests

    #[test]
    fn test_shell_access_serialize() {
        assert_eq!(
            serde_json::to_string(&ShellAccess::None).unwrap(),
            "\"none\""
        );
        assert_eq!(
            serde_json::to_string(&ShellAccess::Restricted).unwrap(),
            "\"restricted\""
        );
        assert_eq!(
            serde_json::to_string(&ShellAccess::Unrestricted).unwrap(),
            "\"unrestricted\""
        );
    }

    #[test]
    fn test_shell_access_deserialize() {
        assert_eq!(
            serde_json::from_str::<ShellAccess>("\"none\"").unwrap(),
            ShellAccess::None
        );
        assert_eq!(
            serde_json::from_str::<ShellAccess>("\"restricted\"").unwrap(),
            ShellAccess::Restricted
        );
        assert_eq!(
            serde_json::from_str::<ShellAccess>("\"unrestricted\"").unwrap(),
            ShellAccess::Unrestricted
        );
    }

    #[test]
    fn test_shell_access_invalid_deserialize() {
        assert!(serde_json::from_str::<ShellAccess>("\"invalid\"").is_err());
    }

    #[test]
    fn test_shell_access_display() {
        assert_eq!(ShellAccess::None.to_string(), "none");
        assert_eq!(ShellAccess::Restricted.to_string(), "restricted");
        assert_eq!(ShellAccess::Unrestricted.to_string(), "unrestricted");
    }

    #[test]
    fn test_shell_access_from_str() {
        assert_eq!("none".parse::<ShellAccess>().unwrap(), ShellAccess::None);
        assert_eq!(
            "restricted".parse::<ShellAccess>().unwrap(),
            ShellAccess::Restricted
        );
        assert_eq!(
            "unrestricted".parse::<ShellAccess>().unwrap(),
            ShellAccess::Unrestricted
        );
    }

    #[test]
    fn test_shell_access_from_str_invalid() {
        let err = "invalid".parse::<ShellAccess>().unwrap_err();
        assert!(err.contains("unknown shell access"));
    }

    #[test]
    fn test_shell_access_clone_and_copy() {
        let s = ShellAccess::Restricted;
        let s2 = s;
        #[allow(clippy::clone_on_copy)]
        let s3 = s.clone();
        assert_eq!(s, s2);
        assert_eq!(s, s3);
    }

    #[test]
    fn test_shell_access_debug() {
        assert_eq!(format!("{:?}", ShellAccess::None), "None");
        assert_eq!(format!("{:?}", ShellAccess::Restricted), "Restricted");
        assert_eq!(format!("{:?}", ShellAccess::Unrestricted), "Unrestricted");
    }

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
        let standard = GuardConfig::from_preset(GuardPreset::Standard);
        assert_eq!(config, standard);
    }

    #[test]
    fn test_guard_config_from_preset_strict() {
        let config = GuardConfig::from_preset(GuardPreset::Strict);
        assert_eq!(config.file_write, FileScope::RepoOnly);
        assert_eq!(config.file_read, FileScope::RepoOnly);
        assert_eq!(config.shell, ShellAccess::None);
        assert!(!config.network);
        assert!(!config.install_packages);
        assert!(!config.git_push);
    }

    #[test]
    fn test_guard_config_from_preset_standard() {
        let config = GuardConfig::from_preset(GuardPreset::Standard);
        assert_eq!(config.file_write, FileScope::RepoOnly);
        assert_eq!(config.file_read, FileScope::Workspace);
        assert_eq!(config.shell, ShellAccess::Restricted);
        assert!(config.network);
        assert!(!config.install_packages);
        assert!(!config.git_push);
    }

    #[test]
    fn test_guard_config_from_preset_yolo() {
        let config = GuardConfig::from_preset(GuardPreset::Yolo);
        assert_eq!(config.file_write, FileScope::Unrestricted);
        assert_eq!(config.file_read, FileScope::Unrestricted);
        assert_eq!(config.shell, ShellAccess::Unrestricted);
        assert!(config.network);
        assert!(config.install_packages);
        assert!(config.git_push);
    }

    #[test]
    fn test_guard_config_serialize_roundtrip() {
        let config = GuardConfig::from_preset(GuardPreset::Strict);
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: GuardConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_guard_config_serialize_with_env() {
        let mut config = GuardConfig::from_preset(GuardPreset::Standard);
        config.env = EnvFilter {
            allow: vec!["PATH".into()],
            deny: vec!["AWS_*".into()],
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: GuardConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
        assert_eq!(deserialized.env.allow, vec!["PATH"]);
        assert_eq!(deserialized.env.deny, vec!["AWS_*"]);
    }

    #[test]
    fn test_guard_config_clone() {
        let config = GuardConfig::from_preset(GuardPreset::Yolo);
        #[allow(clippy::redundant_clone)]
        let cloned = config.clone();
        assert_eq!(config, cloned);
    }

    #[test]
    fn test_guard_config_debug() {
        let config = GuardConfig::default();
        let debug = format!("{config:?}");
        assert!(debug.contains("GuardConfig"));
        assert!(debug.contains("Restricted"));
    }

    #[test]
    fn test_guard_config_deserialize_without_env() {
        let json = r#"{
            "file_write": "repo_only",
            "file_read": "workspace",
            "shell": "restricted",
            "network": true,
            "install_packages": false,
            "git_push": false
        }"#;
        let config: GuardConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.env, EnvFilter::default());
    }
}
